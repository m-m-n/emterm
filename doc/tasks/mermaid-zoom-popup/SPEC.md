# Feature: Mermaid Diagram Zoom Popup + Copy Button Fix

## Overview

Add a fullscreen popup overlay for Mermaid diagrams in the Markdown viewer that supports zoom (via `+`/`-` buttons, mouse wheel, keyboard) and pan (via left-mouse drag), so complex diagrams can be inspected up-close. Simultaneously fix the pre-existing Copy button on the Mermaid toolbar, which is rendered but has no click handler wired up.

## Objectives

- Let users pop out a Mermaid diagram to a fullscreen overlay and zoom / pan around it
- Wire up the currently-inert Copy button on the Mermaid toolbar so it copies the Mermaid source to the clipboard
- Keep the change scoped to the child Markdown viewer (`src-tauri/viewer/` + `src-tauri/web-shared/markdown/`); no changes to the native terminal or other child WebViews

## User Stories

### US1: Enlarge a Mermaid diagram to read it

As a Markdown viewer user, I want to click a "Spread" button on a Mermaid diagram to open it in a fullscreen popup, so that I can read a complex chart without squinting.

**Acceptance Criteria:**
- [ ] The `.mermaid-block-wrapper` toolbar shows a Spread button between the Code and Copy buttons: `[Chart | Code | Spread | Copy]`
- [ ] Clicking Spread opens a fullscreen overlay with a clone of the SVG centered
- [ ] The SVG is initially sized so it fits inside the "stage area" = window client area minus 10% padding on all sides; that fit-to-stage size is defined as `scale = 1.0`
- [ ] The overlay has a semi-transparent dark background (`rgba(0, 0, 0, 0.85)`)

### US2: Zoom and pan inside the popup

As a Markdown viewer user, I want to zoom in/out and drag the diagram around the popup, so that I can focus on the parts I care about.

**Acceptance Criteria:**
- [ ] `+` / `-` buttons and keyboard `+` / `-` change zoom in 0.25 steps
- [ ] Mouse wheel changes zoom continuously (factor 1.1 per notch)
- [ ] Zoom is clamped to `[0.25, 5.0]`
- [ ] Left-mouse drag pans the diagram
- [ ] `0` key or the reset button restores `scale = 1.0`, `panX = panY = 0`
- [ ] Wheel events over the popup do not scroll the background viewer

### US3: Close the popup

As a Markdown viewer user, I want obvious ways to close the popup, so that I can return to reading.

**Acceptance Criteria:**
- [ ] Clicking the `×` button (top-right) closes the popup
- [ ] Clicking the background (overlay itself, not the SVG) closes the popup
- [ ] Pressing `ESC` closes the popup without also closing the Markdown viewer window
- [ ] After closing, focus returns to the originating Spread button
- [ ] After closing, the background viewer's scroll state is fully restored

### US4: Copy the Mermaid source

As a Markdown viewer user, I want the Copy button on the Mermaid toolbar to actually copy the source, so that I can paste the diagram code elsewhere.

**Acceptance Criteria:**
- [ ] Clicking `.copy-code-button.mermaid-copy-btn` writes the value of `data-mermaid-source` to the clipboard via `navigator.clipboard.writeText()`
- [ ] On success, the button flashes a `.copy-success` state with the `markdown.copySuccess` label ("Copied!" / 「コピー完了」) for ~1.5s
- [ ] On failure, the button flashes a `.copy-error` state with the `markdown.copyFailed` label ("Failed" / 「失敗」)

## Technical Requirements

### Functional Requirements

- **FR1 — Spread button in toolbar**: Extend `MermaidRenderer.renderBlock` in `src-tauri/web-shared/markdown/mermaid-renderer.ts` to also create and append a `<button class="mermaid-spread-btn">` between the `codeBtn` and `copyBtn`. It uses a 14x14 viewBox SVG icon (an outward-pointing diagonal-arrow / "expand" glyph) matching the existing icon sizes, and has `aria-label = t("markdown.mermaidSpread")`. When Mermaid rendering fails (`.mermaid-error-banner` code path), the toolbar's Spread button is not emitted at all — the failure branch keeps the original code block and does not build a toolbar, so no additional gating logic is needed.
- **FR2 — Popup overlay open**: Clicking the Spread button constructs a `.mermaid-popup-overlay` element (positioned `fixed`, `inset: 0`, `z-index: 2000` so it sits above `.markdown-fullscreen-overlay` at 1000 and below `.dialog-shell` at 3000), appends it to `document.body`, clones the diagram SVG (`.mermaid-diagram > svg`) via `cloneNode(true)`, and centers the clone. **Clone sizing normalization**: Mermaid renders with `useMaxWidth: true`, so the source SVG carries `width="100%"` and an inline `style="max-width: <n>px"` which the clone inherits; a stylesheet rule cannot override inline styles, so after cloning, remove the `width` / `height` attributes and clear the inline `width` / `height` / `max-width` / `max-height` styles so the clone's untransformed base box equals the `viewBox`-derived intrinsic size. On construction, compute `stageWidth = window.innerWidth * 0.8`, `stageHeight = window.innerHeight * 0.8`, derive the SVG's intrinsic size from its `viewBox` (or `getBoundingClientRect()`), and set the initial fit factor `k` such that `k = min(stageWidth / svgWidth, stageHeight / svgHeight)`. Store `k` as the reference; the exposed `scale` variable starts at `1.0` and the applied CSS transform is `translate(panX, panY) scale(scale * k)`.
- **FR3 — Zoom controls**: The popup has three buttons in a bottom-right cluster (`.mermaid-popup-controls`), stacked top-to-bottom as zoom-in (`+`), reset (`0`), zoom-out (`-`) — matching the common zoom-control convention (zoom-in on top). Button clicks and keyboard `+` / `-` step scale by 0.25 (additive); wheel events multiply/divide by 1.1. All operations clamp scale to `[0.25, 5.0]`.
- **FR4 — Pan controls**: `mousedown` on the stage sets `dragging = true`; window-level `mousemove` while dragging updates `panX += event.movementX` / `panY += event.movementY`; window-level `mouseup` sets `dragging = false` (window-level listeners cover releases outside the stage, which is why no pointer capture or stage-`mouseleave` handling is needed). As a stuck-drag guard, `window` `blur` while dragging also sets `dragging = false`. Cursor is `grab` normally, `grabbing` during drag.
- **FR5 — Reset**: `0` key or reset button sets `scale = 1.0`, `panX = 0`, `panY = 0`.
- **FR6 — Close**: The popup listens for close triggers in three ways:
  1. Click on the `.mermaid-popup-close` `×` button
  2. Click on the overlay itself where `event.target === overlayEl` (so clicks on the SVG stage / controls do not close). **Pan-end guard**: a click synthesized at the end of a pan-drag (i.e. any drag `mousemove` occurred between `mousedown` and `mouseup`) must NOT close the popup, even when the `mouseup` lands on the overlay background — track a `didPan` flag set during drag movement and consume it in the overlay click handler.
  3. `keydown` for `ESC` on the overlay (registered with `capture: true`, calling `event.stopPropagation()` and `event.preventDefault()`)
  On any close: remove the overlay DOM node, restore `document.body.style.overflow` to its saved previous value, and call `.focus()` on the originating Spread button.
- **FR7 — Background scroll lock**: On open, save `document.body.style.overflow` and set it to `hidden`. On close, restore it.
- **FR8 — Copy button click handler**: In `renderBlock`, attach a `click` listener to `copyBtn` that awaits `navigator.clipboard.writeText(source)`. On resolve, add `.copy-success` and set `copyIcon.textContent = t("markdown.copySuccess")` for 1500ms, then restore `t("markdown.copyCode")` and remove the class. On reject, do the same with `.copy-error` and `t("markdown.copyFailed")`.
- **FR9 — Focus management**: On popup open, focus the `.mermaid-popup-close` button. On close, refocus the originating `.mermaid-spread-btn`. A minimal focus trap keeps `Tab` / `Shift+Tab` cycling within the four popup buttons in DOM order (close, zoom-in, reset, zoom-out), wrapping at both ends with `event.preventDefault()`.
- **FR10 — Resize handling**: While the popup is open, a `window.resize` listener recomputes the fit factor `k` so the diagram remains fit-to-stage when at `scale = 1.0`.

### Non-Functional Requirements

- **NFR1 — Performance**: The transformation is applied via CSS `transform` on the SVG element only — no re-render of Mermaid. Frame budget: ~60fps for zoom / pan while dragging.
- **NFR2 — Security**: Mermaid's `securityLevel: "strict"` and the existing hidden-container render pattern in `renderBlock` are unchanged. The SVG clone is inserted via `appendChild(clonedNode)`, never via `innerHTML` on untrusted content. The popup overlay is appended to `document.body` (necessary for fullscreen positioning) but the popup controller guarantees the overlay DOM node and all its listeners are removed on close, so no leakage occurs across sessions. Clipboard writes only send the value from `data-mermaid-source`, which is the exact source text the user's Markdown provided.
- **NFR3 — Cross-platform**: Must work on Linux (WebKitGTK) and Windows (WebView2). No macOS support required. Keyboard, mouse, and wheel events must behave identically.
- **NFR4 — Maintainability**: New popup logic goes in `src-tauri/web-shared/markdown/mermaid-popup.ts` if the code exceeds ~100 lines; otherwise it may be inlined into `mermaid-renderer.ts`. The exact split is decided in the implementation plan (sdd.2). All new CSS goes into `src-tauri/web-shared/markdown/fullscreen.css` (Bun-bundled TS cannot `@import` CSS reliably in this project).
- **NFR5 — Accessibility**: The overlay has `role="dialog"` and `aria-modal="true"`. All buttons have `aria-label`s. Focus is managed on open/close.

## Implementation Approach

### Architecture

The Markdown viewer's child WebView entry (`src-tauri/viewer/web/entry.ts`) constructs `MermaidRenderer` and calls `renderAll(container)`. That produces one `mermaid-block` per diagram, each with a `mermaid-toolbar` positioned in the top-right of its `.mermaid-block-wrapper`. This feature adds one button to that toolbar and a companion popup module.

**Layered view:**
```
┌───────────────────────────────────────────────────────────────┐
│ src-tauri/viewer/web/entry.ts       (unchanged)               │
│   └─ new MermaidRenderer().renderAll(container)               │
├───────────────────────────────────────────────────────────────┤
│ web-shared/markdown/mermaid-renderer.ts  (extended: +Spread)  │
│   ├─ renderBlock() → toolbar = [Chart, Code, Spread*, Copy]   │
│   ├─ wires Spread onClick → openMermaidPopup(sourceSvg, ...)  │
│   └─ wires Copy onClick   → clipboard.writeText(source) *NEW  │
├───────────────────────────────────────────────────────────────┤
│ web-shared/markdown/mermaid-popup.ts  (NEW, if extracted)     │
│   openMermaidPopup(svgEl, opts) → HTMLElement                 │
│     └─ manages overlay lifecycle: open, zoom, pan, close      │
├───────────────────────────────────────────────────────────────┤
│ web-shared/markdown/fullscreen.css  (extended: +popup styles) │
└───────────────────────────────────────────────────────────────┘
```

### Data Flow

```
User clicks Spread button
  ↓
mermaid-renderer.ts: spreadBtn onclick
  ↓
mermaid-popup.ts: openMermaidPopup(svgEl, triggerEl)
  ├─ construct overlay DOM (.mermaid-popup-overlay ...)
  ├─ clone svgEl and insert into stage
  ├─ compute fit factor k, initialize scale=1.0, panX=panY=0
  ├─ attach listeners: wheel, mousedown/move/up, keydown, click-close, resize
  ├─ save & override document.body.style.overflow
  ├─ append to document.body
  ├─ focus close button
  └─ return controller { close() }
  ↓
User interacts → scale / panX / panY mutate → CSS transform re-applied
  ↓
User triggers close (×, background, ESC)
  ↓
controller.close()
  ├─ remove overlay
  ├─ restore body.overflow
  ├─ detach listeners
  └─ triggerEl.focus()
```

### API Design

Not applicable — this is purely a client-side WebView UI feature. No IPC or HTTP endpoints are added or changed.

### Public TypeScript API

**File**: `src-tauri/web-shared/markdown/mermaid-popup.ts` (new, or as an inlined helper in `mermaid-renderer.ts`)

```ts
export interface MermaidPopupController {
  /** Remove the overlay, detach listeners, restore focus/scroll. */
  close(): void;
}

export interface MermaidPopupOptions {
  /** SVG element to clone (typically `.mermaid-diagram > svg`). */
  svg: SVGElement;
  /** The Spread button; focus returns here on close. */
  triggerButton: HTMLElement;
}

/** Open a fullscreen Mermaid diagram popup. Returns a controller for programmatic close. */
export function openMermaidPopup(opts: MermaidPopupOptions): MermaidPopupController;
```

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `src-tauri/web-shared/markdown/mermaid-renderer.ts`: extended (Spread button + Copy click handler)
- `src-tauri/web-shared/markdown/fullscreen.css`: extended (popup + spread-btn + copy-success/error styles for the new context)
- `src-tauri/web-shared/i18n/locales/{en,ja}.json`: 2 new keys — `markdown.mermaidSpread`, `markdown.mermaidPopupClose` (and any additional labels for zoom / reset if desired)

**External Dependencies:**
- `navigator.clipboard`: browser API, already available in WebKitGTK / WebView2
- No new npm packages

### File Structure

```
src-tauri/web-shared/markdown/
├── mermaid-renderer.ts          # extended: adds Spread button + wires Copy handler
├── mermaid-popup.ts             # NEW (if extracted): openMermaidPopup(...) impl
├── mermaid-popup.test.ts        # NEW: unit tests for openMermaidPopup lifecycle & keyboard/mouse
├── mermaid-renderer.test.ts     # NEW/extended: verify toolbar order + copy click handler
├── fullscreen.css               # extended: .mermaid-popup-overlay, .mermaid-popup-stage,
│                                #           .mermaid-popup-controls, .mermaid-popup-close,
│                                #           .mermaid-spread-btn
└── ...

src-tauri/web-shared/i18n/locales/
├── en.json                      # extended: markdown.mermaidSpread etc.
└── ja.json                      # extended: markdown.mermaidSpread etc.
```

Whether `mermaid-popup.ts` is a separate file or an internal function inside `mermaid-renderer.ts` is deferred to sdd.2 (implementation plan). NFR4 sets the threshold: extract at ~100 LoC.

## Test Scenarios

### Unit Tests

Framework: Bun's built-in test runner with `happy-dom` (see `test-setup.ts`).

- [ ] **TS-1 — Toolbar order**: After `renderBlock`, the toolbar's children in DOM order are `chartBtn, codeBtn, spreadBtn, copyBtn`
- [ ] **TS-2 — Spread button attributes**: `.mermaid-spread-btn` has correct `type="button"` and `aria-label`
- [ ] **TS-3 — Copy click writes to clipboard**: Given `navigator.clipboard.writeText` mocked, clicking `copyBtn` calls it with the exact `source` string, and `.copy-success` class is added and removed after ~1.5s
- [ ] **TS-4 — Copy click failure**: Given `navigator.clipboard.writeText` rejects, `.copy-error` is added and removed after ~1.5s
- [ ] **TS-5 — openMermaidPopup creates overlay**: `openMermaidPopup({svg, triggerButton})` appends a `.mermaid-popup-overlay` (with `role="dialog"` + `aria-modal="true"`) to `document.body` and clones the given SVG into `.mermaid-popup-stage`
- [ ] **TS-6 — Scroll lock**: Opening the popup sets `document.body.style.overflow = "hidden"`; closing restores the previous value
- [ ] **TS-7 — Focus flow**: On open, focus is on `.mermaid-popup-close`; on close, focus is back on the trigger button
- [ ] **TS-8 — Zoom clamp lower**: With `scale = 0.25`, pressing `-` leaves scale at 0.25
- [ ] **TS-9 — Zoom clamp upper**: With `scale = 5.0`, pressing `+` leaves scale at 5.0
- [ ] **TS-10 — Reset**: After some zoom+pan, pressing `0` resets to `scale=1.0, panX=0, panY=0`
- [ ] **TS-11 — ESC closes popup**: Dispatching a keydown `Escape` on the overlay closes it and calls `stopPropagation()` (verified via a spied outer listener)
- [ ] **TS-12 — Background click closes**: Clicking the overlay itself (target === overlay) closes it; clicking a child does not
- [ ] **TS-13 — Wheel zoom**: `wheel` with `deltaY < 0` on the stage multiplies scale by 1.1 (clamped) and `preventDefault()`s the event
- [ ] **TS-14 — Drag pan**: mousedown → mousemove(dx, dy) → mouseup updates `panX, panY` by the sum of movementX/Y
- [ ] **TS-15 — Resize refit**: `window.dispatchEvent(new Event("resize"))` while popup is open recomputes the fit factor `k`
- [ ] **TS-16 — Button/keyboard zoom step is 0.25 additive**: From `scale = 1.0`, clicking `+` (or pressing key `+`) yields exactly `1.25`; clicking `-` from `1.25` yields `1.0`. The wheel path still multiplies/divides by 1.1
- [ ] **TS-17 — Tab focus trap**: With the popup open, `Tab` from the last button (zoom-out) wraps focus to the first (close); `Shift+Tab` from the first wraps to the last; focus never leaves the overlay
- [ ] **TS-18 — Pan-end click guard**: mousedown on stage → mousemove → mouseup landing on the overlay background does NOT close the popup; a clean click (no intervening drag movement) on the background still closes it
- [ ] **TS-19 — Clone sizing normalization**: The cloned SVG in the stage has no `width` / `height` attributes and no inline `max-width`, so the fit factor applies to the `viewBox`-derived intrinsic size
- [ ] **TS-20 — Blur clears drag**: Dispatching `window` `blur` while `dragging` is true clears the dragging state (subsequent mousemove does not pan)

### Integration Tests

- [ ] **I1**: Load a Markdown fixture with a Mermaid block in the viewer entry test suite, invoke `renderAll`, then verify the resulting DOM structure and behaviors end-to-end within happy-dom
- [ ] **I2**: `bun run typecheck` passes (`tsc --noEmit` scoped to `src-tauri/{viewer,settings}/web`)

### E2E Tests

**Existing E2E tests**: None. Per `test/README.md`, this project has no `docker-compose.e2e.yml` and no `e2e-tests/` directory. End-to-end behavior is validated manually by the user (or via `cargo test --test mux_throughput`-style Rust integration tests).

**Run command**: Not applicable for this feature — the popup lives inside the child WebView, which has no in-repo E2E harness.

- [ ] Existing bun unit tests continue to pass without regression (`bun test`)
- [ ] Existing Rust `--lib` tests continue to pass without regression (`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`)
- [ ] Manual verification on Linux (WebKitGTK) and Windows (WebView2) that Spread opens the popup and Copy writes to clipboard

### Edge Cases

- [ ] **E1 — Mermaid render failed**: When `renderBlock` catches an error and shows `.mermaid-error-banner`, no Spread button is emitted (or it is disabled)
- [ ] **E2 — Clipboard permission denied**: Falls back to the `.copy-error` UI; no unhandled promise rejection appears in the console log
- [ ] **E3 — ESC-double-press**: 1st ESC closes popup, 2nd ESC closes viewer (this is expected — captured / documented; do not attempt to swallow the 2nd)
- [ ] **E4 — Window resize during drag**: A resize while `dragging` is true does not break the pan state; on mouseup the recomputed `k` applies cleanly
- [ ] **E5 — Multiple diagrams**: Opening popup for diagram A, closing, then opening for diagram B works independently; focus returns to the correct trigger

### Performance Tests

Not required (no measured budget); frame smoothness is a qualitative acceptance criterion.

## Security Considerations

- **XSS Prevention**: The cloned SVG is inserted via `appendChild(clonedNode)`, not via `innerHTML`. Mermaid's `securityLevel: "strict"` remains the source of trust for the initial render. The popup only re-hosts the sanitized output.
- **Input Validation**: The only user-provided data written to the clipboard is `data-mermaid-source`, which is the exact string from the user's own Markdown code block. No transformation.
- **Focus / Modal**: `role="dialog"` + `aria-modal="true"`, and the popup is layered above the viewer, matching a normal modal. There is no privileged action inside the popup, so escape-through-focus is not a security concern.

## Error Handling

### Error Codes

Not applicable (in-page UI feature; no API layer).

### Error Flow

```
navigator.clipboard.writeText(source)
   └─ resolves → UI shows .copy-success for 1500ms
   └─ rejects  → UI shows .copy-error  for 1500ms + console.warn (release-visible)

renderBlock() catches mermaid.render() failure
   └─ Existing behavior kept: shows .mermaid-error-banner, no Spread button.

openMermaidPopup() called with a null / disconnected SVG
   └─ console.warn and return a no-op controller (defensive; should never happen in normal flow)
```

## Performance Optimization

### Performance Goals

- Spread click → visible overlay < 100ms
- Zoom / pan interaction: 60fps target on both Linux and Windows

### Optimization Strategies

- **SVG cloning is cheap**: `cloneNode(true)` on an already-rendered SVG is O(nodes); no Mermaid re-parsing
- **CSS transforms**: `transform: translate(x, y) scale(s)` is GPU-composited by both WebKitGTK and WebView2, so no per-frame layout
- **Event listener cleanup**: On popup close, `removeEventListener` for `wheel`, `mousemove`, `mouseup`, `keydown`, `resize` to avoid leaks

### Caching Strategy

Not applicable — nothing to cache.

## Success Criteria

- [ ] All functional requirements FR1–FR10 are implemented and unit-tested
- [ ] `bun test` passes without regression
- [ ] `bun run typecheck` passes without regression
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes (should be unaffected)
- [ ] Manual verification on Linux (WebKitGTK) and Windows (WebView2) confirms UX behavior
- [ ] The child Markdown viewer's `console.warn` output stays clean (no unhandled promise rejections from clipboard writes)

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- [ ] FR-INT — Whether the popup logic ships inline in `mermaid-renderer.ts` or as a new `mermaid-popup.ts` file. Decision deferred to the implementation plan (NFR4 threshold: ~100 LoC).

## Implementation Phases

Given the small scope (single WebView, one renderer file + one CSS file + optional new module), a single phase is sufficient. If splitting is preferred, a natural cut is:

### Phase 1: Copy button fix (small, isolated)

**Goals**: Fix the Copy click handler; keep the toolbar layout untouched.

**Deliverables:**
- `mermaid-renderer.ts` copy handler
- `mermaid-renderer.test.ts` copy tests (T3, T4)

### Phase 2: Popup feature

**Goals**: Add Spread button, popup overlay, zoom / pan / close, focus, scroll lock.

**Deliverables:**
- `mermaid-renderer.ts` toolbar extension (Spread button)
- `mermaid-popup.ts` (new) OR extended `mermaid-renderer.ts`
- `fullscreen.css` popup styles
- `mermaid-popup.test.ts` (T5–T15)
- i18n keys in `en.json` / `ja.json`

## References

- Discussion document: `./tmp/discussion-mermaid-zoom-popup.md`
- Existing implementation: `src-tauri/web-shared/markdown/mermaid-renderer.ts`
- Existing CSS: `src-tauri/web-shared/markdown/fullscreen.css`
- Related project rules:
  - `.claude/rules/debugging-constraints.md` — DevTools unavailable; use `emterm.log`
  - `.claude/rules/build-location.md` — always pass `--manifest-path` + `CARGO_TARGET_DIR`
  - `CLAUDE.md` — Bun bundler for child WebView, CSS `@import` limitations
