# Implementation Plan: Mermaid Diagram Zoom Popup + Copy Button Fix

## Overview

Extend the Markdown viewer's Mermaid rendering pipeline to add a fullscreen popup with zoom / pan controls, and fix the currently-inert Copy button. All changes are confined to the child WebView layer (`src-tauri/web-shared/markdown/` + `src-tauri/web-shared/i18n/locales/`).

## Objectives

- Add a Spread button to the Mermaid toolbar that opens a fullscreen popup viewer for the diagram
- Provide zoom (`+/-` buttons, mouse wheel, keyboard `+/-`), pan (left-mouse drag), and reset (`0` key / button) inside the popup
- Wire up the existing Copy button so it writes the Mermaid source to the clipboard with success/error feedback
- Preserve Mermaid's `securityLevel: "strict"` and avoid `innerHTML` on unsanitized content

## Prerequisites

### Development Environment

- Bun (bundler / test runner / package manager)
- TypeScript 5.x
- Existing project setup per `CLAUDE.md` (child WebView bundles under `src-tauri/viewer/`)

### Dependencies

- `mermaid` (already installed, `^11.12.2`) — no version change
- `happy-dom` (already dev-installed) — used by the new unit tests
- No new npm packages
- Depends on existing modules:
  - `src-tauri/web-shared/markdown/mermaid-renderer.ts` (extended)
  - `src-tauri/web-shared/markdown/fullscreen.css` (extended)
  - `src-tauri/web-shared/i18n/index.ts` (`t()` helper — existing)

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (child WebView bundles only; no Rust changes)
- **Framework**: none (vanilla DOM) — plus Mermaid for rendering
- **Key Libraries**:
  - `mermaid` — SVG diagram rendering (unchanged, `securityLevel: "strict"`)
  - Bun test runner + `happy-dom` — unit tests
  - `navigator.clipboard` (browser Web API) — clipboard writes

### Design Approach

The Mermaid renderer stays the toolbar owner. A separate module `mermaid-popup.ts` owns the popup lifecycle (open, transform state, event listeners, close, focus/scroll restoration). The renderer delegates to the popup module on Spread click via a well-defined interface — no shared globals, all state is contained inside the popup controller returned to the caller.

**Key design decisions**:
- Popup DOM is appended to `document.body` (not the block wrapper) so it overlays the entire viewer
- The SVG is *cloned* — the original stays in the toolbar-bearing block for normal viewing
- Zoom / pan state is stored in the popup module's closure and applied as a single `transform: translate(...) scale(...)` on the cloned SVG each frame
- Keyboard shortcuts are captured at the overlay level so they cannot leak to the underlying Markdown viewer (in particular ESC)

### Component Interaction

```
┌────────────────────────────────────────────────────────────────┐
│  MermaidRenderer.renderBlock()   (mermaid-renderer.ts)         │
│    - constructs toolbar: [Chart, Code, Spread, Copy]           │
│    - Chart / Code onClick     → existing toggle handler        │
│    - Spread onClick           → openMermaidPopup(...)          │
│    - Copy onClick             → navigator.clipboard.writeText  │
└──────────────┬─────────────────────────────────────────────────┘
               │  (invokes)
               ▼
┌────────────────────────────────────────────────────────────────┐
│  openMermaidPopup({svg, triggerButton})   (mermaid-popup.ts)   │
│    creates overlay DOM, wires listeners, returns controller    │
└────────────────────────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Copy Button Fix (small, isolated)

**Goal**: Restore the Copy button so clicking it writes the Mermaid source to the clipboard with visible success / error feedback. No layout / toolbar order change in this phase.

**Files to Modify**:
- `src-tauri/web-shared/markdown/mermaid-renderer.ts` — attach a `click` handler to `copyBtn` inside `renderBlock`

**Files to Create**:
- `src-tauri/web-shared/markdown/mermaid-renderer.test.ts` — unit tests for the copy click handler (uses `happy-dom` per existing test-setup pattern)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Copy click handler | Write Mermaid source to clipboard and show feedback | `copyBtn` exists in toolbar; `source` is captured from `renderBlock` closure | Clipboard contains `source`; button shows `.copy-success` (or `.copy-error` on failure) for ~1500ms then reverts |

**Processing Flow**:
1. User clicks `.mermaid-copy-btn`
2. Handler invokes clipboard write API with the block's Mermaid source (captured from `renderBlock` closure or read from `data-mermaid-source` attribute)
   - Success → add `.copy-success` class, set label to `markdown.copySuccess`
   - Failure → add `.copy-error` class, set label to `markdown.copyFailed`, log a `console.warn` (release-visible)
3. After ~1500ms, remove the feedback class and restore the original label (`markdown.copyCode`)

**Implementation Steps**:
1. **Capture source in closure** — Confirm `source` (the Mermaid code string) is in the `renderBlock` scope; it already is (see current implementation)
2. **Attach click listener** — Register a `click` listener on `copyBtn` inside `renderBlock` that performs the clipboard write and feedback flow
3. **Add success/error feedback helper** — Small helper (inline or method) that toggles `.copy-success` / `.copy-error` + label change, then reverts after 1500ms via `setTimeout`
4. **Handle exception path** — Wrap the clipboard call in try/catch (or `.then/.catch`) so a rejection does not leak as an unhandled promise
5. **Write unit tests** — Cover success and failure paths using a mocked `navigator.clipboard`

**Dependencies**: None — self-contained inside `mermaid-renderer.ts`. Blocks nothing; Phase 2 is independent.

**Testing Approach**:
- Unit: Click on `copyBtn` writes exact `source` to clipboard; success adds `.copy-success` and reverts after timer; failure adds `.copy-error` and reverts
- Integration: (covered by Phase 2's viewer entry integration)
- E2E: none in-repo; manual verification on Linux WebKitGTK and Windows WebView2
- Manual: Confirm the copied text pastes as expected in a text editor

**Acceptance Criteria**:
- [ ] Clicking Copy writes the Mermaid source to the clipboard
- [ ] Success visual feedback appears for ~1.5s
- [ ] Failure visual feedback appears if clipboard is unavailable
- [ ] `bun test` passes; `bun run typecheck` passes

**Estimated Effort**: small

---

### Phase 2: Spread Button and Popup Feature

**Goal**: Add the Spread button to the Mermaid toolbar and implement the fullscreen popup with zoom, pan, close, focus management, and scroll lock. Provide unit tests covering the popup lifecycle and keyboard/mouse behaviors.

**Files to Create**:
- `src-tauri/web-shared/markdown/mermaid-popup.ts` — `openMermaidPopup(opts): MermaidPopupController`
- `src-tauri/web-shared/markdown/mermaid-popup.test.ts` — unit tests for popup lifecycle, zoom clamp, pan, reset, ESC-close, background-click-close, focus, scroll lock, resize refit

**Files to Modify**:
- `src-tauri/web-shared/markdown/mermaid-renderer.ts` — add Spread button between Code and Copy; wire its `onClick` to `openMermaidPopup`
- `src-tauri/web-shared/markdown/fullscreen.css` — add styles for `.mermaid-spread-btn`, `.mermaid-popup-overlay`, `.mermaid-popup-stage`, `.mermaid-popup-controls`, `.mermaid-popup-close`
- `src-tauri/web-shared/i18n/locales/en.json` — add `markdown.mermaidSpread`, `markdown.mermaidPopupClose`, `markdown.mermaidPopupZoomIn`, `markdown.mermaidPopupZoomOut`, `markdown.mermaidPopupReset`
- `src-tauri/web-shared/i18n/locales/ja.json` — same keys in Japanese

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Spread button | Toolbar entry point for opening the popup | `.mermaid-block-wrapper` has a rendered `.mermaid-diagram > svg` | Clicking invokes `openMermaidPopup({svg, triggerButton})` |
| Popup overlay | Fullscreen container hosting the diagram clone, controls, close button | Called with valid `svg` element and `triggerButton` element | Overlay is appended to `document.body`, background is scroll-locked, focus is on the close button |
| Zoom state controller | Track `scale`, `panX`, `panY`; recompute fit factor `k` on open and resize; apply CSS transform to cloned SVG | Overlay is mounted | SVG has `transform: translate(panX, panY) scale(scale * k)` reflecting latest state |
| Event router | Route wheel, mousedown/move/up, keydown (+ / - / 0 / ESC), background click, resize, and close button click into controller state changes | Overlay is mounted; listeners are attached during open | Listeners are removed on close |
| Controller close | Detach listeners, remove overlay, restore body overflow, refocus trigger button | Overlay is currently open | All popup DOM and side effects reverted |

**Function Contracts**:

```
openMermaidPopup(opts: {svg: SVGElement, triggerButton: HTMLElement}) -> MermaidPopupController
  Precondition:
    - opts.svg is an SVGElement currently in the DOM (used to derive intrinsic dimensions)
    - opts.triggerButton is an HTMLElement that has .focus() available
    - No other popup is currently open (single-instance invariant — see Open Questions if this is ever violated)
  Postcondition:
    - A .mermaid-popup-overlay element is appended to document.body
    - The overlay contains a clone of opts.svg
    - document.body.style.overflow is set to "hidden" (previous value stashed for restore)
    - Focus is moved to the popup's close button
    - Returns a controller with .close() that reverts all of the above
```

**Processing Flow** (Spread click):
1. User clicks `.mermaid-spread-btn`
2. Handler resolves the SVG element (`.mermaid-diagram > svg` inside the same block) and calls `openMermaidPopup({svg, triggerButton: spreadBtn})`
3. Popup module constructs overlay DOM (overlay, stage, controls, close), clones the SVG into the stage
4. Fit factor `k` is computed from `viewBox` (or bounding rect) vs. stage area (window inner size × 0.8 on each dimension); `scale=1.0` and `panX=panY=0` are the initial state
5. Overlay is appended to `document.body`; `document.body.style.overflow` is saved and set to `hidden`; focus moves to close button
6. Listeners are attached: wheel (on stage, `passive:false`), mousedown/mousemove/mouseup (drag), keydown (on overlay, `capture:true`), click (overlay itself → close if target is overlay), window resize
7. Any state change → apply single CSS transform `translate(panX,panY) scale(scale*k)` on cloned SVG

**Processing Flow** (close):
1. Close trigger occurs (× button click, background click, ESC keydown)
   - If ESC: `event.stopPropagation()` + `event.preventDefault()` at capture phase so the Markdown viewer's ESC handler does not fire
2. Detach all listeners registered in open flow
3. Remove overlay from `document.body`
4. Restore `document.body.style.overflow` to the saved value
5. Call `.focus()` on `opts.triggerButton`

**Implementation Steps**:
1. **i18n keys** — Add 5 new `markdown.mermaid*` keys to `en.json` / `ja.json` (Spread label + 4 popup labels)
2. **Popup module skeleton + DOM construction** — Create `mermaid-popup.ts` exporting `openMermaidPopup` and its types (`MermaidPopupController`, `MermaidPopupOptions`); build overlay / stage / controls / close DOM inside it
3. **Zoom/pan state controller** — Track `scale`/`panX`/`panY` in closure; recompute fit factor `k` on open + resize; apply transform to cloned SVG; clamp scale to `[0.25, 5.0]`
4. **Event listeners** — Wire wheel, mousedrag, keydown (+ / - / 0 / ESC at capture), background click, close button click, and window resize; ensure `preventDefault` on wheel and `stopPropagation` on ESC
5. **Focus + scroll lock + close lifecycle** — Save/restore body overflow, initial focus on close button, minimal focus trap on the 4 popup buttons, restore focus to trigger on close; `controller.close()` removes overlay + detaches listeners
6. **Toolbar integration** — In `mermaid-renderer.ts`, construct the Spread button (SVG icon, `aria-label`, `.mermaid-spread-btn` class), insert between `codeBtn` and `copyBtn`, wire onClick to `openMermaidPopup`
7. **CSS + unit tests** — Add `.mermaid-spread-btn` + `.mermaid-popup-*` styles in `fullscreen.css`; write TS-1, TS-2, TS-5 through TS-15 in `mermaid-popup.test.ts` and `mermaid-renderer.test.ts`

**Dependencies**: Depends on Phase 1's toolbar being intact (does not depend on the Copy handler itself; both phases can be done in either order). Blocks final `sdd.6-verify` acceptance.

**Testing Approach**:

- Unit (Bun + happy-dom):
  - Toolbar order after `renderBlock`: `[Chart, Code, Spread, Copy]`
  - `openMermaidPopup` appends overlay to `document.body` and clones SVG
  - Body overflow save/restore across open/close
  - Focus flow: on open → close button; on close → trigger button
  - Zoom clamp low (0.25 with `-`) and high (5.0 with `+`)
  - Reset key `0` returns state to `{scale:1.0, panX:0, panY:0}`
  - ESC keydown closes the overlay and stops propagation
  - Background click (target === overlay) closes; click on child does not
  - Wheel `deltaY < 0` multiplies scale, calls `preventDefault`
  - Drag: mousedown → mousemove(dx, dy) → mouseup updates pan
  - `window.resize` while open recomputes `k`
- Integration: existing viewer entry test (`src-tauri/viewer/web/entry.test.ts`) continues to pass
- E2E: none in repo; manual verification only
- Manual: Confirm real behavior on Linux WebKitGTK and Windows WebView2 (smoothness, cursor changes, scroll lock, ESC behavior)

**Acceptance Criteria**:
- [ ] Toolbar has 4 buttons in the order `[Chart | Code | Spread | Copy]`
- [ ] Spread click opens overlay with the SVG clone centered, fit-to-stage
- [ ] `+/-` buttons, `+/-` keys, and mouse wheel change zoom within `[0.25, 5.0]`
- [ ] Left drag pans the diagram; cursor toggles `grab` ↔ `grabbing`
- [ ] `0` key or reset button restores initial state
- [ ] ESC / × / background click close the overlay
- [ ] ESC does not close the parent Markdown viewer
- [ ] Body scroll is locked while overlay is open, restored on close
- [ ] Focus moves to close button on open and back to Spread on close
- [ ] Window resize while open recomputes fit
- [ ] `bun test` passes; `bun run typecheck` passes

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/
├── web-shared/
│   ├── markdown/
│   │   ├── mermaid-renderer.ts        # MODIFIED: Spread button + Copy handler
│   │   ├── mermaid-renderer.test.ts   # NEW: Copy handler + toolbar order tests
│   │   ├── mermaid-popup.ts           # NEW: openMermaidPopup + controller
│   │   ├── mermaid-popup.test.ts      # NEW: popup lifecycle + zoom/pan/close tests
│   │   ├── fullscreen.css             # MODIFIED: popup + spread-btn styles
│   │   ├── renderer.ts                # (unchanged)
│   │   ├── outline.ts / outline.css   # (unchanged)
│   │   └── types.ts                   # (unchanged)
│   └── i18n/
│       └── locales/
│           ├── en.json                # MODIFIED: 5 new markdown.mermaid* keys
│           └── ja.json                # MODIFIED: same 5 keys in Japanese
└── viewer/
    └── web/
        ├── entry.ts                   # (unchanged — MermaidRenderer usage unchanged)
        └── entry.test.ts              # (unchanged — regression only)

doc/tasks/mermaid-zoom-popup/
├── SPEC.md
├── 要件定義書.md
├── IMPLEMENTATION.md                  # this file
├── VERIFICATION.md                    # created next
├── sdd.yaml
└── tasks.yaml
```

## Testing Strategy

- **Unit** (Bun + happy-dom): Target 90%+ coverage of `mermaid-popup.ts` and the new / modified paths in `mermaid-renderer.ts`. Cover all Test Scenarios TS-1 through TS-15 defined in VERIFICATION.md
- **Integration**: Existing viewer entry test verifies the renderer still runs end-to-end without regression
- **E2E**: No in-repo E2E framework for the child Markdown viewer; not applicable
- **Manual**: Cross-platform check on Linux WebKitGTK and Windows WebView2 (see VERIFICATION.md Manual Testing)

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| mermaid | ^11.12.2 | (existing) SVG diagram rendering, `securityLevel: "strict"` |
| happy-dom | ^20.3.1 | (existing dev) DOM environment for Bun unit tests |
| typescript | ^5.0.0 | (existing dev) `bun run typecheck` |

No new packages added.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `navigator.clipboard.writeText` permission denied in Wry WebView | Medium | Medium | Wrap in try/catch, show `.copy-error` UI, log `console.warn`. If persistent, fall back to `document.execCommand('copy')` on a temporary hidden textarea (added only if manual test shows failure) |
| SVG clone loses `<foreignObject>` / animation state | Low-Medium | Medium | Test with a variety of Mermaid diagrams (flowchart, sequence, ER, gantt). If any loss is visible, switch clone strategy to `outerHTML` round-trip |
| Wheel-scroll leaks to background viewer | Low | Medium | Register wheel listener with `passive: false` and `preventDefault()` — verify on both WebKitGTK and WebView2 |
| ESC-stopPropagation permanently disables viewer ESC | Very Low | High | Listeners are attached only while overlay lives; on close all are removed. Unit test verifies detach in `.close()` |
| Multiple popups opened (double-click race) | Low | Low | Simplest guard: track a module-level `activePopup` and return early / re-focus if one is already open. Alternative: allow only one Spread click via boolean flag on the button during open animation |
| `viewBox` missing on SVG (unlikely for Mermaid output) | Very Low | Low | Fall back to `getBoundingClientRect()` for intrinsic size |

## Open Questions

- [ ] Should we defensively guard against multiple popups (module-level `activePopup`), or is single-open-at-a-time already implicit because the Spread button is behind the popup once it opens? (Recommended: add the guard for safety; cost is trivial.)
- [ ] Whether to fall back to `document.execCommand('copy')` if `navigator.clipboard` is unavailable — resolved only if manual testing shows a failure

## Success Metrics

- [ ] All Phase 1 + Phase 2 acceptance criteria are checked
- [ ] `bun test` green with the new test files present
- [ ] `bun run typecheck` clean
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` unaffected (no regression)
- [ ] Manual verification on both Linux (WebKitGTK) and Windows (WebView2) confirms UX
- [ ] `emterm.log` shows no unhandled promise rejections or `console.warn` spam during normal use
