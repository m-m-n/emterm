# Feature: Native Terminal Feature Port (Phase 3)

## Overview

Phase 3 of the eMterm restructuring plan (`tmp/restruct.md`) replaces the minimal terminal built in Phase 1 PoC with a production-quality implementation, using the `term_core` crate that was extracted in Phase 2. This SPEC covers grid rendering with dirty-row diff, full cursor/selection/scrollback behavior, inline image display (Kitty Graphics Protocol and SIXEL) reusing the existing `src-tauri/src/image/`, comprehensive OSC test coverage, and long-running stability re-verification. Mux, tab-bar UI, Windows IME, Wry viewer windows, the settings UI, i18n synchronization, and the retirement of the legacy Tauri build are deferred to Phase 4–7.

## Objectives

- Replace the Phase 1 PoC stand-ins (minimal renderer, minimal selection, 1000-line scrollback, no images, partial OSC) with production behavior matching the existing eMterm Tauri build.
- Render only dirty rows per frame, driven by `term_core`'s dirty-row state, instead of the Phase 1 full-frame redraw.
- Reuse `src-tauri/src/image/` (Kitty + SIXEL decoders and LRU image cache) from the native terminal so inline images render with pixel parity to the existing build.
- Cover every OSC `action_type` that `term_core::TerminalCore` emits via `cargo test`, and implement the native-side handler subset that is in scope for Phase 3.
- Pass a 12+ hour Claude Code session (longer than Phase 1's 8h gate) without screen loss, crash, or monotonic memory growth.

## User Stories

### US1: Interact with a production-quality native terminal

As an eMterm developer, I want the native terminal to render text, cursor, and SGR attributes with the same fidelity as the current WebView build, so that I can use it as a real day-to-day shell.

**Acceptance Criteria:**
- [ ] `cargo run -p emterm-native-poc` launches a window backed by `term_core`.
- [ ] Typing and shell output render with full SGR support (4/8/24-bit color, bold, italic, underline single/double/curly, reverse, blink, conceal, strikethrough).
- [ ] Cursor shape follows DECSCUSR (`CSI Ps SP q`), OSC 22, and visibility follows DECTCEM (`CSI ?25 h/l`).
- [ ] Only dirty rows are repainted (verifiable via debug logging or RUST_LOG=debug).

### US2: Scroll back through history

As an eMterm developer, I want to scroll back through a 10,000-line history buffer, so that I can review long command output.

**Acceptance Criteria:**
- [ ] Mouse wheel scrolls 3 lines per tick.
- [ ] `Shift+PageUp` / `Shift+PageDown` scroll by page; `Shift+Home` / `Shift+End` jump to top/bottom.
- [ ] Scrollback is suppressed while alt-screen is active (DECSET 1049).
- [ ] When scrolled away from the live region, new output does not auto-follow until the user returns to the bottom.

### US3: Select, copy, and paste with system clipboard integration

As an eMterm developer, I want text selection that follows current eMterm semantics, so that copy/paste matches my muscle memory.

**Acceptance Criteria:**
- [ ] Drag selects characters; double-click selects a word; triple-click selects a line.
- [ ] Selection auto-copies to Linux PRIMARY on mouse-up.
- [ ] `Ctrl+Shift+C` copies to CLIPBOARD; `Ctrl+Shift+V` pastes from CLIPBOARD; middle-click pastes from PRIMARY.
- [ ] When bracketed paste mode (DECSET 2004) is enabled, pasted content is wrapped in `\e[200~ ... \e[201~`.

### US4: View Kitty Graphics Protocol images inline

As an eMterm developer, I want `emterm image foo.png` to display the image inline in the native terminal at parity with the WebView build, so that AI-tooling image workflows continue to work.

**Acceptance Criteria:**
- [ ] Kitty APC payload is decoded via the existing `src-tauri/src/image/kitty.rs`.
- [ ] The decoded RGBA is uploaded to a wgpu texture and overlaid on the grid per `ImagePlacement` (row, col, columns, rows, z_index).
- [ ] Scrolling moves the placement together with the text rows.
- [ ] Memory quota (default 320 MB) is honored; oldest entries are evicted on overflow.

### US5: View SIXEL images inline

As an eMterm developer, I want SIXEL output (e.g. from `img2sixel`) to display inline, so that the new build matches the existing one.

**Acceptance Criteria:**
- [ ] SIXEL DCS payload (`ESC P q ... ESC \`) is decoded via the existing `src-tauri/src/image/sixel.rs`.
- [ ] Rendering uses the same overlay layer as US4.

### US6: Exercise all OSC action types

As an eMterm developer, I want every `term_core` OSC `action_type` to have a native-side test, so that no OSC behavior silently regresses.

**Acceptance Criteria:**
- [ ] Unit tests fire each `action_type` (0,1,2,4,7,8,9,10,11,12,22,52,104,110,111,112,133,100,101,255) and assert the documented native effect or queue.
- [ ] OSC 100 (emterm extension, `param=777`) and OSC 9999 (mux) are queued for Phase 5 / Phase 4 consumers and not acted on in Phase 3.

### US7: Resize without breakage

As an eMterm developer, I want window resize to reflow text and reposition images, so that the layout stays consistent.

**Acceptance Criteria:**
- [ ] Resize updates both `term_core` grid size and PTY winsize.
- [ ] `term_core::reflow` is invoked so wrapped lines stay coherent.
- [ ] Image placements remain at their original cell coordinates (recomputed in the new grid).

### US8: Run a 12+ hour Claude Code session

As an eMterm developer, I want a 12-hour real-usage run without screen loss or memory growth, so that the rewrite is demonstrably more stable than the WebView build.

**Acceptance Criteria:**
- [ ] Window remains responsive after 12+ hours of usage.
- [ ] RSS and GPU memory do not grow monotonically across 4-hour sampling intervals.
- [ ] No `ERROR_SURFACE_LOST_KHR`, no panic in the renderer / event loop.

### US9: Pass workspace-wide cargo test

As an eMterm developer, I want `cargo test --workspace` to be green at Phase 3 completion, so that the existing test corpus continues to gate regressions.

**Acceptance Criteria:**
- [ ] `cargo test --workspace` is green on the Linux dev machine.
- [ ] Image-decoder tests still execute after any workspace re-layout required by reusing `src-tauri/src/image/`.

## Technical Requirements

### Functional Requirements

- **FR1 – Dirty-row diff rendering:** The renderer reads `term_core`'s per-row dirty state and repaints only dirty rows each frame. Cursor moves and selection edits mark the affected rows dirty. Images use their own overlay-layer dirty tracking (add / delete / placement-move).
- **FR2 – Cursor:** Cursor shape (block / underline / bar) and blink follow DECSCUSR; cursor color follows OSC 12 / 112; cursor visibility follows DECTCEM (`CSI ?25 h/l`); shape can also be set via OSC 22 (`action_type=22`).
- **FR3 – Selection:** Character / word / line modes via single / double / triple click. Drag extends the selection. Mouse-up copies to PRIMARY (Linux). `Ctrl+Shift+C` copies to CLIPBOARD. Rectangular (block) selection is out of scope.
- **FR4 – Paste:** `Ctrl+Shift+V` pastes CLIPBOARD; middle-click pastes PRIMARY. Bracketed paste mode (DECSET 2004) wraps the payload in `\e[200~ ... \e[201~`. Paste content sanitization ensures the wrapper cannot be smuggled inside the body.
- **FR5 – Scrollback:** Default 10,000 lines (overridable from `settings.json` via `scrollback_lines`). Ring-buffer eviction when exceeded. Wheel = 3 lines/tick; Shift+PageUp/PageDown by page; Shift+Home/End to top/bottom. Suppressed in alt-screen.
- **FR6 – Inline images (Kitty):** APC payloads delivered by `term_core::TerminalCallbacks::on_apc` are decoded via the existing `image::ImageProcessor::process_kitty_command`. Decoded RGBA is uploaded to wgpu textures and overlaid per `ImagePlacement`. LRU cache with a default 320 MB quota is honored.
- **FR7 – Inline images (SIXEL):** DCS payloads delivered by `on_dcs` are decoded via `image::ImageProcessor::process_sixel` and overlaid by the same layer as FR6.
- **FR8 – OSC handlers (native side):** Every `action_type` emitted by `term_core::osc_handler::handle_osc_internal` has a native-side handler or explicit queue. See *OSC dispatch table* below.
- **FR9 – SGR rendering:** All SGR attributes (4/16/256/truecolor fg+bg, bold, italic, underline single/double/curly with optional color via SGR 58, reverse, blink, conceal, strikethrough) render correctly.
- **FR10 – Resize / reflow:** Window resize triggers `term_core::resize`, `term_core::reflow` for wrapped lines, and a `PtySession::resize` winsize update. Image placements are recomputed.
- **FR11 – Ambiguous width:** East Asian ambiguous-width characters render per `settings.json` `ambiguous_width_mode` (`narrow` / `wide`), matching existing eMterm.
- **FR12 – OSC 9 notifications:** Use `notify-rust` (or equivalent) to surface OSC 9 notifications. Permission policy mirrors the current eMterm behavior (TBD in plan; see Open Questions).
- **FR13 – OSC 52 clipboard:** Permission policy mirrors the current eMterm behavior (TBD in plan). Default-deny if undecided.
- **FR14 – Long-run stability:** Renderer must not leak wgpu textures / egui glyph atlas entries; image LRU must release wgpu textures on eviction.

### Non-Functional Requirements

- **NFR1 – Performance:** 60 FPS in normal interactive use on the Linux dev machine. Input latency no worse than Phase 1 PoC by feel. Image display latency ≤ 300 ms for a 1920×1080 PNG.
- **NFR2 – Stability:** 12+ hours of Claude Code session without screen loss, crash, or monotonic memory growth.
- **NFR3 – Logging:** `log` + `env_logger`. `RUST_LOG=info` prints lifecycle and parser warnings; `RUST_LOG=debug` exposes dirty-row tracing and image-layer diagnostics.
- **NFR4 – Module layout:** Keep the Phase 1 PoC layout (`render`, `pty`, `selection`, `tabs`, `app`, `window_host`, `viewer`, `ime`). Add `native-poc/src/image/` (or equivalent) for the inline-image layer.
- **NFR5 – Platform:** Linux only (Ubuntu 22.04 family dev machine). Windows is deferred to Phase 4.
- **NFR6 – Workspace:** Cargo workspace members unchanged (`src-tauri / wasm / crates/term_core / native-poc`). The legacy Tauri build continues to compile and run until Phase 7.

## Implementation Approach

### Architecture

**System Architecture (Phase 3 scope):**

```
┌────────────────────────────────────────────────────────────────┐
│  emterm-native-poc binary (Phase 1 chassis, Phase 3 features)  │
│                                                                │
│   tao event loop (main thread)                                 │
│     ├── Main window (egui + wgpu surface)                      │
│     │     ├── TabBar (Phase 1 minimal — Phase 4 will replace)  │
│     │     └── Terminal grid                                    │
│     │            ├── Text layer (dirty-row diff)               │
│     │            ├── Cursor layer                              │
│     │            ├── Selection layer                           │
│     │            └── Image overlay layer  ← Phase 3 NEW        │
│     │                                                          │
│     └── (Wry viewer / mux / settings deferred)                 │
│                                                                │
│   PTY reader threads (one per tab)                             │
│     └── bytes → term_core::process_pty_data                    │
│           ├── on_osc → NativeCallbacks (Phase 3 expands)       │
│           ├── on_apc → image::ImageProcessor                   │
│           ├── on_dcs → image::ImageProcessor (SIXEL)           │
│           └── on_bell / on_device_response (Phase 1 existing)  │
└────────────────────────────────────────────────────────────────┘
```

**Component Diagram (native-poc internal):**

- `App` — owns tabs, active index, selection state, image-layer state. (Phase 1 existing; selection & image-layer extended.)
- `Tab` — owns `TerminalCore`, `PtySession`, `cb_state`. (Phase 1 existing.)
- `NativeCallbacks` — `term_core::TerminalCallbacks` implementation. **Extended in Phase 3** to cover all OSC action types, APC (Kitty), DCS (SIXEL).
- `Renderer` (`render::mod`) — egui draw routines. **Extended in Phase 3** to use dirty rows and overlay images.
- `Selection` (`selection`) — **Extended in Phase 3** for word/line modes and PRIMARY auto-copy.
- `ImageLayer` (new module) — wraps `image::ImageProcessor`, owns wgpu textures, applies `ImagePlacement` updates.
- `Settings` — loader for `scrollback_lines`, `ambiguous_width_mode`, `image_memory_quota_mb`, plus Phase 1 fields.

### Data Flow

```
PTY bytes
   → term_core::process_pty_data
       ├── grid mutation                       → renderer dirty-row diff → wgpu
       ├── on_osc(action_type, data)           → NativeCallbacks → state update
       │                                       → (777,9999 → queues for Phase 4/5)
       ├── on_apc(data)                        → image::ImageProcessor.process_kitty_command
       │                                       → ImageEvent → ImageLayer (wgpu texture)
       ├── on_dcs(data)                        → image::ImageProcessor.process_sixel
       │                                       → ImageEvent → ImageLayer
       ├── on_bell                             → bell counter (visual bell hook is Phase 4+)
       └── on_device_response(bytes)           → PtySession.write (Phase 1 existing)

User events
   → tao → egui handler
       ├── mouse drag/click   → Selection state (extended)
       ├── keyboard          → key encoding (Phase 1) + scroll/clipboard shortcuts
       └── resize            → App.set_grid_size → reflow + PtySession.resize
```

### OSC Dispatch Table (term_core → native)

`term_core::osc_handler::handle_osc_internal` maps wire `param` to a flat `action_type` (`u8`). Phase 3 native-side behavior:

| wire param | action_type | name | Phase 3 native behavior |
|------------|-------------|------|--------------------------|
| 0 | 0 | SetTitleAndIcon | Update `Tab.title` (Phase 1 existing). |
| 1 | 1 | SetIconName | Log only; no UI surface in Phase 3. |
| 2 | 2 | SetTitle | Update `Tab.title` (Phase 1 existing). |
| 4 | 4 | SetColorPalette | Update terminal palette in `Theme`. |
| 7 | 7 | SetWorkingDirectory | Store on `Tab.cwd: Option<String>` (UI in Phase 4+). |
| 8 | 8 | Hyperlink | `term_core` stores the URI internally; native logs the action (clicking is Phase 4+). |
| 9 | 9 | Notification | Show OS notification via `notify-rust`. Permission policy = current eMterm (TBD plan). |
| 10 | 10 | SetForegroundColor | Update `Theme.fg`. |
| 11 | 11 | SetBackgroundColor | Update `Theme.bg`. |
| 12 | 12 | SetCursorColor | Update cursor color. |
| 22 | 22 | CursorShape | Update cursor shape. |
| 52 | 52 | Clipboard | Set/get clipboard per permission policy (TBD plan). |
| 104 | 104 | ResetColorPalette | Restore default palette. |
| 110 | 110 | ResetForegroundColor | Restore default fg. |
| 111 | 111 | ResetBackgroundColor | Restore default bg. |
| 112 | 112 | ResetCursorColor | Restore default cursor color. |
| 133 | 133 | SemanticPrompt | Store prompt marks on the grid (used by future search/fold; Phase 3 = state only). |
| 777 | 100 | EmtermExtension | Queue payload for Phase 5 viewer spawner (Phase 1 existing behavior). |
| 1337 | 101 | iTerm2 | Log only. Image features go through Kitty/SIXEL. |
| 9999 (data starts with `emterm-mux;`) | — | mux | `term_core` re-fires as APC; native queues for Phase 4. |
| other | 255 | Unknown | `log::warn!` and ignore. |

### Image Layer Design

```
APC bytes / DCS bytes
   → ansi::apc::parse_kitty_command / ansi::dcs::parse_sixel     (location TBD plan)
   → image::ImageProcessor (existing)
       └── Vec<ImageEvent>
            ├── ImageReady { image: DecodedImage }   → upload to wgpu texture, insert into ImageLayer.lru
            ├── Place { placement: ImagePlacement }  → ImageLayer.placements.insert
            ├── Delete { target: ImageDelete }       → ImageLayer.delete
            ├── Response { data: String }            → PtySession.write
            └── Animation(AnimationEvent)            → ImageLayer animation queue
```

Open issues (plan stage):

1. The existing `image::ImageProcessor` lives in `src-tauri/src/image/` and depends on `crate::ansi::apc::KittyCommand` / `crate::ansi::dcs::SixelData` from `src-tauri/src/ansi/`. To reuse it from `native-poc`, plan must pick one of:
    - (a) Extract `src-tauri/src/image/` and `src-tauri/src/ansi/{apc,dcs}.rs` into a new workspace crate (e.g. `crates/term_images/`), depended on by both `src-tauri` and `native-poc`.
    - (b) Make `src-tauri` a `lib`-style dependency of `native-poc` (likely undesirable — pulls in Tauri).
    - (c) Move only the image+APC/DCS modules out of `src-tauri/` into a shared crate; keep the rest of `src-tauri` untouched until Phase 7.

   Option (a) or (c) is preferred. Final selection is decided in `sdd.2-create-plan`.

2. wgpu texture upload format: `image::DecodedImage` stores RGBA8 (un-premultiplied). The image overlay must render with the same premultiplied / un-premultiplied alpha as the WebView build to reach pixel parity.

### Dirty-Row Rendering Strategy

```
egui frame N
   ├── for each row in 0..rows:
   │     if term_core.is_row_dirty(row) || cursor_moved_into(row) || selection_changed(row):
   │           repaint cells in row
   │     else:
   │           keep last frame's pixels (egui Mesh persists via repaint regions)
   ├── apply cursor overlay
   ├── apply selection overlay
   ├── apply image overlay (full repaint when any placement add/remove/scroll)
   └── term_core.clear_dirty()
```

Exact dirty-row API signatures from `term_core` are confirmed during plan (Open Question OQ4). If the API is missing per-row granularity, plan may extend `term_core` with a minimal addition gated by Phase 2 verify re-run.

### Selection Behavior

Per FR3:

```
mouse_down(pos):
   selection.anchor = cell_at(pos)
   selection.mode = match click_count {
       1 => Character,
       2 => Word,         // word boundaries from term_core::char_table or unicode segmentation
       3 => Line,
       _ => Character,
   }

mouse_drag(pos):
   selection.head = cell_at(pos)
   mark intersecting rows dirty

mouse_up:
   text = render selection text from term_core cells
   arboard::Clipboard::set_text_primary(text)   // Linux PRIMARY auto-copy

Ctrl+Shift+C:
   arboard::Clipboard::set_text(text)           // CLIPBOARD

Ctrl+Shift+V:
   text = arboard::Clipboard::get_text()
   if bracketed_paste_enabled:
       sanitize_bracket_sequences(&mut text)
       pty.write(b"\x1b[200~"); pty.write(text); pty.write(b"\x1b[201~")
   else:
       pty.write(text)
```

### Dependencies

**Internal Dependencies:**
- `crates/term_core` — ANSI parser, grid, OSC dispatch, callbacks trait. Established in Phase 2.
- `src-tauri/src/image/` (or extracted crate per OQ1) — Kitty + SIXEL decoders, LRU cache.
- `src-tauri/src/ansi/{apc,dcs}.rs` (or extracted crate per OQ1) — APC/DCS payload parsers.
- Existing `settings.json` loader — extended with new fields (`scrollback_lines`, `image_memory_quota_mb`).

**External Dependencies (Rust crates):**
- `tao` 0.34 — windowing + event loop (unchanged from Phase 1).
- `wgpu` 22 — GPU surface (unchanged from Phase 1).
- `egui` 0.29 / `egui-wgpu` 0.29 — UI layer (unchanged from Phase 1).
- `wry` 0.53 — declared but unused in Phase 3 (Phase 5 will use).
- `portable-pty` 0.8 — PTY abstraction (unchanged).
- `arboard` 3 — clipboard (unchanged).
- `notify-rust` (new in Phase 3) — OS notifications for OSC 9.
- `unicode-width` 0.2 — cell width (unchanged).
- `log` + `env_logger` — logging.
- `crossbeam-channel` / `parking_lot` (unchanged).

Concrete version bumps decided in `sdd.2-create-plan`; pin via `Cargo.lock`.

### File Structure (Phase 3 deltas vs. Phase 1)

```
native-poc/
├── Cargo.toml                   # add notify-rust; depend on the extracted image crate (per OQ1)
└── src/
    ├── app.rs                   # extend: image-layer state, scrollback API
    ├── callbacks.rs             # extend: all OSC action_types, APC + DCS routing to image::ImageProcessor
    ├── pty/                     # unchanged from Phase 1 (already production)
    ├── render/
    │   ├── mod.rs               # rewrite: dirty-row diff, SGR full, cursor shape, image overlay
    │   └── theme.rs             # extend: palette, fg/bg/cursor color updates
    ├── selection.rs             # extend: word/line modes, PRIMARY auto-copy, bracketed paste
    ├── tabs.rs                  # minor: store cwd, surface scrollback control
    ├── settings.rs              # extend: scrollback_lines, image_memory_quota_mb, ambiguous_width_mode
    ├── ui/                      # tab_bar.rs unchanged (Phase 4 will redo)
    ├── viewer/                  # unchanged (Phase 5 will wire OSC 777 queue)
    ├── image/                   # NEW directory
    │   ├── mod.rs               # ImageLayer (wgpu textures + LRU view)
    │   ├── overlay.rs           # rendering of placements
    │   └── parse.rs             # APC/DCS → image::ImageProcessor (uses extracted crate per OQ1)
    └── window_host.rs           # minor: pass dirty info to renderer; image-layer redraw scheduling

crates/                          # (workspace)
└── term_images/                 # (CONDITIONAL — only if OQ1 = option a)
    ├── Cargo.toml
    └── src/                     # contents of src-tauri/src/image/ + ansi/{apc,dcs}.rs
```

## Test Scenarios

### Unit Tests
- [ ] `NativeCallbacks::on_osc`: each `action_type` (0,1,2,4,7,8,9,10,11,12,22,52,104,110,111,112,133,100,101,255) invokes the documented native effect or queue.
- [ ] `NativeCallbacks::on_apc`: decodes a fixture Kitty payload via `image::ImageProcessor` and produces `ImageReady` + `Place`.
- [ ] `NativeCallbacks::on_dcs`: decodes a fixture SIXEL payload and produces `ImageReady` + `Place`.
- [ ] `Selection`: character / word / line modes produce expected ranges; bracketed paste wrap is applied.
- [ ] `Renderer`: dirty rows are computed correctly when cells change vs. when only cursor moves.
- [ ] `Settings`: new fields (`scrollback_lines`, `image_memory_quota_mb`, `ambiguous_width_mode`) parse with defaults when missing.
- [ ] `ImageLayer`: LRU eviction frees wgpu textures when the memory quota is exceeded.

### Integration Tests
- [ ] Spawn a bash PTY, write a sequence containing SGR truecolor + DECSCUSR + DECTCEM, assert grid state and dirty flags.
- [ ] Send a Kitty APC payload via the PTY, drive `term_core::process_pty_data`, observe an `ImageEvent::Place` reaching `ImageLayer`.
- [ ] Send a SIXEL DCS payload similarly.
- [ ] Resize the grid mid-stream; verify reflow keeps wrapped lines coherent and image placements update.

### E2E Tests
**Existing E2E tests:** `e2e-tests/` (WebdriverIO + tauri-driver) target the legacy Tauri build and remain in place until Phase 7. They do **not** drive `native-poc`.
**Run command:** `./scripts/run-e2e-docker.sh` (unchanged).
- [ ] Existing E2E suite continues to pass on `main` (no regressions caused by Phase 3 workspace changes if any).
- [ ] Phase 3 adds no new GUI-driven E2E specs; manual checklist replaces them (per project policy on Tauri E2E).

### Manual Verification
- [ ] Visual parity for Kitty Graphics Protocol against the current Tauri build (1–3 representative payloads).
- [ ] Visual parity for SIXEL against the current Tauri build.
- [ ] 12+ hour Claude Code session on the Linux dev machine with RSS / GPU memory samples at the 4h / 8h / 12h marks.
- [ ] SGR sampler (e.g. `printf` script exercising every attribute) compared side by side with the Tauri build.

### Edge Cases
- [ ] Malformed APC (truncated Kitty payload) → warning log, no crash.
- [ ] Malformed DCS (truncated SIXEL) → warning log, no crash.
- [ ] Image memory quota exceeded → LRU eviction, warning log, no panic.
- [ ] Scrollback overflow at exactly the configured cap → oldest line evicted, no off-by-one.
- [ ] Alt-screen toggle during selection → selection cleared (matches current eMterm).
- [ ] Rapid resize during heavy output → no panic, no stuck rows.

### Performance Tests
- [ ] Manual: type latency feels equal to or better than the current Tauri build.
- [ ] Manual: 60 FPS observed during normal interaction (e.g. `htop`, `vim`).
- [ ] Manual: scrolling through 10,000 lines feels smooth.
- [ ] Manual: a 1920×1080 PNG via Kitty renders within ~300 ms of the APC arrival.

## Security Considerations

- **Authentication / Authorization:** Not applicable (local desktop app).
- **Input Validation:** APC / DCS / OSC payloads are size-bounded; the image decoder rejects pathological dimensions; the parser must not allocate unbounded buffers on malformed input.
- **OSC 52 clipboard policy:** Mirror current eMterm. Default-deny if the existing policy cannot be reliably ported in Phase 3 (revisit during plan).
- **OSC 9 notification policy:** Mirror current eMterm; notifications are inherently user-visible so the risk is mostly notification-spam by hostile shell output. Rate-limiting decision deferred to plan.
- **Data Protection:** No new persistence beyond `settings.json` (read-only at runtime, write only when the user changes settings via Phase 4+).
- **XSS Prevention:** Not applicable in Phase 3 (no WebView involved).
- **Sandbox / process isolation:** Out of scope for Phase 3.

## Error Handling

### Error Codes

Logged via `log` crate; no user-facing codes.

| Code | Description | Severity | User message |
|------|-------------|----------|--------------|
| LOG_OSC_UNKNOWN | OSC `action_type=255` (unrecognized wire param) | warn | (log only) |
| LOG_APC_BAD | Kitty APC payload could not be decoded | warn | (log only) |
| LOG_DCS_BAD | SIXEL DCS payload could not be decoded | warn | (log only) |
| LOG_IMG_QUOTA | LRU eviction triggered due to memory quota | info | (log only) |
| LOG_IMG_DECODE | Image decode failed | warn | (log only; placement skipped) |
| LOG_OSC52_DENIED | OSC 52 clipboard request rejected by policy | warn | (log only) |
| LOG_OSC9_DROPPED | OSC 9 notification rate-limited or rejected | warn | (log only) |
| LOG_PTY_DEAD | PTY exited unexpectedly | info | Tab closes |
| LOG_RENDER_FALLBACK | Dirty-row diff disabled; falling back to full redraw (debug builds only) | warn | (log only) |

### Error Flow

```
Anomaly → log → continue (best-effort) → never crash the main thread
```

## Performance Optimization

### Performance Goals

- 60 FPS during interactive use (Linux dev machine, mid-range GPU).
- Input latency ≤ Phase 1 PoC (by feel).
- Image display ≤ 300 ms for 1920×1080 PNG.
- 12+ hour memory stability with no monotonic growth.

### Optimization Strategies

- Dirty-row diff rendering (FR1) avoids full-frame repaint.
- PTY reads stay on dedicated threads; the event loop never blocks.
- Allocations on the per-byte parse path stay in `term_core` (already optimized in Phase 2); `NativeCallbacks` minimizes per-event allocations.
- Image LRU evicts wgpu textures eagerly to bound GPU memory.

### Caching Strategy

- Image LRU: configurable quota (default 320 MB), keyed by image ID, eviction = oldest unused first.
- Glyph atlas: handled by `egui`; monitored as part of the 12h test.
- No additional caching layer in Phase 3.

## Success Criteria

- [ ] FR1–FR14 are demonstrably working.
- [ ] US1–US9 acceptance criteria are checked off.
- [ ] `cargo test --workspace` is green at Phase 3 completion.
- [ ] Manual visual parity for Kitty + SIXEL against the current Tauri build.
- [ ] 12+ hour Claude Code session passes without screen loss / crash / monotonic memory growth.
- [ ] No regressions in the legacy Tauri build's `cargo test` and E2E suite.

## Open Questions

> **Note**: Unresolved items are tracked in `sdd.yaml` with `status: tbd`. Resolve before `/em-sdd:sdd.2-create-plan` completes.

- [ ] **OQ1** — Workspace placement of the image + APC/DCS modules currently in `src-tauri/`. Options: new shared crate (`crates/term_images/`) or staying inside `src-tauri/` with `native-poc` depending on `src-tauri` as a library. (status: tbd)
- [ ] **OQ2** — OSC 52 clipboard permission policy port. Mirror current eMterm exactly vs. default-deny in Phase 3. (status: tbd)
- [ ] **OQ3** — OSC 9 notification rate-limiting policy and integration crate (`notify-rust` vs. raw dbus). (status: tbd)
- [ ] **OQ4** — `term_core` dirty-row API: exact signature(s) currently exposed and whether per-row granularity exists today. May require a small additive change to `term_core` (with Phase 2 verify rerun). (status: tbd)
- [ ] **OQ5** — wgpu texture format for RGBA images to match Tauri/WebView build's premultiplied-alpha behavior pixel-for-pixel. (status: tbd)
- [ ] **OQ6** — Whether DECSCUSR / OSC 22 / DECTCEM state is accessible via current `term_core` getters or needs additional accessor methods. (status: tbd)
- [ ] **OQ7** — Whether the iTerm2 OSC 1337 subset (image protocol, set marks, etc.) needs any native handling beyond `log::warn!` in Phase 3. (status: tbd)

## Implementation Phases (if applicable)

This SPEC covers Phase 3 of the eight-phase plan in `tmp/restruct.md`. Phases 1 and 2 are completed; phases 4–7 are tracked in separate SDDs and intentionally out of scope here.

### Phase 3: Native Terminal Feature Port (this SPEC)

**Goals:** Replace Phase 1 PoC stand-ins with production behavior, integrate inline images via existing decoders, cover all OSC types, re-verify long-run stability.

**Deliverables:**
- `native-poc/src/image/` overlay layer + wiring in `callbacks.rs`.
- Dirty-row diff renderer in `render/mod.rs`.
- Selection extensions (word / line / PRIMARY) in `selection.rs`.
- Scrollback expansion to 10,000 lines + alt-screen suppression.
- OSC action_type test matrix in `callbacks.rs` tests.
- (Conditional, per OQ1) extracted `crates/term_images/` crate.
- 12+ hour Claude Code session VERIFICATION evidence.

## References

- `tmp/restruct.md` — Restructuring strategy (Phase 3 spec source).
- `doc/tasks/native-terminal-poc/SPEC.md` — Phase 1 PoC SPEC (foundation that Phase 3 extends).
- `doc/tasks/native-terminal-poc/要件定義書.md` — Phase 1 PoC requirements (extended in this SDD's 要件定義書.md).
- `doc/tasks/term-core-rust-crate/` — Phase 2 SDD, completed.
- `crates/term_core/src/lib.rs` — public `term_core` API.
- `crates/term_core/src/osc_handler.rs` — wire `param` → `action_type` mapping.
- `crates/term_core/src/callbacks.rs` — `TerminalCallbacks` trait.
- `src-tauri/src/image/` — Kitty + SIXEL decoders, LRU cache, animation.
- `src-tauri/src/ansi/` — APC + DCS payload parsers (current location).
- `native-poc/src/` — Phase 1 PoC implementation.
- `CLAUDE.md` — Project-wide guidance, including the Docker-first testing convention.
- `e2e-tests/README.md` — Legacy E2E test runner (untouched in Phase 3).
