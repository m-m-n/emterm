# Implementation Plan: Native Terminal Feature Port (Phase 3)

## Overview

Replace the Phase 1 PoC stand-ins in `native-poc/` with production-quality terminal behavior matching the current Tauri/WebView build: dirty-row diff rendering, full SGR + cursor + selection + scrollback, inline Kitty/SIXEL images via a newly-extracted `crates/term_images/` crate, complete OSC `action_type` coverage, and 12+ hour stability re-verification on Linux. The legacy Tauri build must keep compiling and testing throughout.

## Objectives

- Extract `src-tauri/src/image/` and `src-tauri/src/ansi/{apc,dcs}.rs` into a shared workspace crate `crates/term_images/` so that both `src-tauri` and `native-poc` can consume the same Kitty/SIXEL decoders without pulling in Tauri itself.
- Wire `native-poc` to call those decoders from its `TerminalCallbacks` impl and overlay decoded images on the existing egui+wgpu surface with proper z-order and scroll tracking.
- Convert the renderer in `native-poc/src/render/` from full-frame redraw to per-row dirty diff driven by `TerminalCore`'s existing `is_row_dirty` / `get_dirty_rows` / `clear_dirty` APIs.
- Implement word/line selection modes, Linux PRIMARY auto-copy, CLIPBOARD shortcuts, middle-click paste, and bracketed paste mode in `native-poc/src/selection.rs`.
- Expand the OSC dispatch in `native-poc/src/callbacks.rs` from the Phase 1 subset (titles + OSC 777 queue) to every `action_type` emitted by `term_core::osc_handler` (0,1,2,4,7,8,9,10,11,12,22,52,104,110,111,112,133,100,101,255), each with a corresponding unit test.
- Resolve the wgpu surface initialization panic (`ERROR_SURFACE_LOST_KHR`) inherited from Phase 1 so that the 12+ hour session gate is reachable.
- Keep `cargo test --workspace` green throughout, including image-decoder tests after their relocation.

## Prerequisites

### Development Environment

- Rust toolchain matching the workspace (Phase 2 baseline: stable 1.79+, edition 2021).
- Docker + docker compose for the canonical build/test path (per `CLAUDE.md` and `sdd.yaml.project.components.main`).
- `bun` only required for the legacy Tauri regression check (Phase 5 of restruct.md / Phase 7 still depends on it). Not used for `native-poc` work.

### Dependencies

- Phase 2 SDD (`doc/tasks/term-core-rust-crate/`) is fully completed — `crates/term_core/` exists and is a workspace member (verified via current `Cargo.toml`).
- `native-poc/` already path-depends on `term_core` (Phase 6 swap completed).
- `crates/term_core/src/terminal_modes.rs` already exposes `is_row_dirty(row) -> bool`, `get_dirty_rows() -> Vec<u16>`, `mark_row_dirty(row)`, `mark_all_dirty()`, `clear_dirty()`. **No additive change to `term_core` is required** (resolves OQ4).
- `crates/term_core/src/terminal_cursor.rs` already exposes `get_cursor_style`, `get_cursor_blink`, `get_cursor_visible`, `get_cursor_fg`, `get_cursor_bg`. **No additive change to `term_core` is required for FR2** (resolves OQ6).
- `src-tauri/src/image/` and `src-tauri/src/ansi/{apc,dcs}.rs` have no `tauri::*` / `tauri_runtime::*` references — they are safely portable into a tauri-free crate (verified by grep).
- The current `refactor/native-terminal-hybrid` branch HEAD compiles via `cargo build --workspace` and `cargo test --workspace` exits 0 (SC-6 legacy compatibility gate; legacy E2E excluded per SPEC.md SC-6 rationale).

## Architecture Overview

### Technology Stack

- **Language**: Rust (edition 2021).
- **Window / event loop**: `tao` 0.34 (existing).
- **GPU surface + UI**: `wgpu` 22 + `egui` 0.29 + `egui-wgpu` 0.29 (existing).
- **PTY**: `portable-pty` 0.8 (existing).
- **Clipboard**: `arboard` 3 (existing; already pulled into `native-poc`).
- **Notifications**: `notify-rust` (new in Phase 3) — OSC 9 surface.
- **Image decoding** (moved into `crates/term_images/`): `image` crate (PNG/JPEG/GIF/WebP), `flate2` (zlib for Kitty compressed payloads). Versions inherit from `src-tauri` to avoid duplicate transitive copies in `Cargo.lock`.
- **Serialization**: `serde` (already in use for image events; required only by the shared crate, not as part of any IPC because `native-poc` consumes the structs directly).

### Design Approach

Four parallel tracks of work, each contained in its own phase to allow targeted re-verification:

1. **Workspace re-layout** (Phase 1) — move shared image/ANSI code into a new tauri-free crate so subsequent phases can build on it. This is the lowest-risk change but it is the gate for everything else, hence first.
2. **Stability prerequisite** (Phase 0) — fix the wgpu surface-lost panic on launch. Without this the 12 h stability gate (NFR2) is unreachable and the developer cannot exercise the rest of the work manually. Runs in parallel with Phase 1 but is the on-ramp for Phases 3–7.
3. **Terminal-rendering quality lift** (Phases 2–4) — dirty-row diff, full SGR, cursor, selection, scrollback. These touch the existing `native-poc/src/{render,selection,settings,tabs,window_host}.rs` modules.
4. **Inline images + OSC parity** (Phases 5–6) — add the new `native-poc/src/image/` module that consumes `term_images::ImageProcessor`, expand `NativeCallbacks` to cover every `action_type` with tests, add notifications + clipboard policy.
5. **Manual verification** (Phase 7) — 12+ hour Claude Code session with periodic memory sampling, plus visual parity comparison against the legacy WebView build.

### Component Interaction

```
PTY bytes
    -> TerminalCore::process_pty_data (term_core, unchanged)
         |-- grid mutation -> dirty bits flipped on TerminalCore
         |-- on_osc(action_type, data) -> NativeCallbacks (extended)
         |-- on_apc(data)               -> NativeCallbacks -> term_images::ImageProcessor::process_kitty_command
         |-- on_dcs(data)               -> NativeCallbacks -> term_images::ImageProcessor::process_sixel
         |-- on_bell                    -> bell counter + optional notify-rust
         '-- on_device_response(bytes)  -> PtySession::write (Phase 1 existing)

Per egui frame on the main thread:
    WindowHost::redraw
       -> Renderer::draw
            |-- query TerminalCore::get_dirty_rows
            |-- for each dirty row: tessellate cells (text + bg)
            |-- cursor overlay (using term_core cursor getters)
            |-- selection overlay (using App.selection)
            |-- image overlay (ImageLayer.draw — wgpu textures, z-ordered)
            '-- TerminalCore::clear_dirty
```

`ImageLayer` is the only new component that owns GPU resources beyond what egui creates. It keeps its own LRU table of wgpu textures keyed by `image_id`, mirroring the byte-level LRU in `term_images::ImageProcessor`.

## Resolved Open Questions

The SPEC enumerates seven Open Questions for the plan to settle. They are resolved here, with each Phase that depends on the resolution cross-referenced.

| OQ | Resolution | Driven by |
|----|------------|-----------|
| OQ1 | **Option (a) — new shared crate `crates/term_images/`** with `image/` + `ansi/{apc,dcs}.rs` moved in. Both `src-tauri` and `native-poc` depend on it via `path = "../crates/term_images"`. `tauri` is not pulled in because the moved code never references `tauri::*` (verified). | Phase 1 |
| OQ2 | **Mirror legacy fields/defaults: `clipboard_read_osc52: bool` (default `true`) + `clipboard_max_size_osc52: u32` (default `10 * 1024 * 1024` = 10 MB).** The legacy WebView build already exposes these two fields and defaults to allow with a 10 MB cap (`src-tauri/src/commands/config/settings.rs:458-464`, `src/terminal-app/osc-handler.ts:188-189`). Native-poc reads the *same* field names from `settings.json` so user expectations and existing settings UIs remain consistent. Interactive prompt ("ask" mode) is deferred to Phase 5+ since it requires a UI surface that does not exist in Phase 3. | Phase 6 |
| OQ3 | **`notify-rust` 0.x** for OSC 9 delivery. Rate-limit: suppress identical `(title, body)` pairs that arrive within 1 second of each other (in-process dedupe table with TTL). This matches "notify but do not spam" intent of the current WebView build without depending on its exact JS-side debounce. | Phase 6 |
| OQ4 | **No `term_core` change required.** `is_row_dirty(row)` and `get_dirty_rows() -> Vec<u16>` already exist (`terminal_modes.rs:67-102`). `clear_dirty()` clears all bits; the renderer calls it once per frame after consuming the list. | Phase 2 |
| OQ5 | **`Rgba8UnormSrgb`, un-premultiplied source.** The image decoder already produces un-premultiplied RGBA8. The wgpu render pipeline configures `BlendState` with `src_factor = SrcAlpha`, `dst_factor = OneMinusSrcAlpha` for both color and alpha, which matches the CSS canvas `globalCompositeOperation: 'source-over'` behavior the WebView uses. | Phase 5 |
| OQ6 | **No `term_core` change required.** All DECSCUSR/OSC 22/DECTCEM/OSC 12 state is already exposed via `terminal_cursor.rs` getters listed above. | Phase 3 |
| OQ7 | **`log::warn!` only.** OSC 1337 in Phase 3 is parsed by `term_core` and reaches `NativeCallbacks::on_osc(action_type=101, data)`, where it is logged. No image subset of OSC 1337 is implemented; Kitty is the canonical image protocol. | Phase 6 |

## Implementation Phases

### Phase 0: wgpu surface-initialization fix (stability prerequisite)

**Goal**: Eliminate the `ERROR_SURFACE_LOST_KHR` panic that fires during `WindowHost::new` on the Linux dev machine so that downstream phases can be exercised manually and the 12+ hour session gate is reachable.

**Files to Create**: (none)

**Files to Modify**:
- `native-poc/src/window_host.rs` — make first-frame surface configure resilient to the `Lost` / `Outdated` race that happens when tao reports an initial size before the surface is ready.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `WindowHost` | Owns wgpu surface + first-frame configure | tao window is created | Surface is configured **lazily** on the first `redraw_requested` event, not in `new()`; on `SurfaceError::Lost` / `Outdated` the next frame reconfigures before calling `get_current_texture()` |
| `WindowHost::redraw` | Tolerates a `Lost`/`Outdated` error on the very first frame | surface_dirty=true | Reconfigures and returns; the event loop sends a new `RedrawRequested` |

**Processing Flow** (diagram-convertible):

1. tao `Event::Resumed` fires.
2. WindowHost::new finishes synchronously without calling surface.configure.
3. First `Event::RedrawRequested` fires.
   - Branch A: surface_dirty=true (initial state) -> configure with current physical_size -> proceed.
   - Branch B: previous frame returned Lost/Outdated -> configure with current physical_size -> proceed.
4. surface.get_current_texture() — on Lost/Outdated set surface_dirty=true, request_redraw, return.

**Implementation Steps** (5 max):

1. **Defer surface.configure**: move first-frame configure out of `WindowHost::new()` into the redraw path, gated by `surface_dirty`.
2. **Tighten the Lost/Outdated branch**: ensure the existing match arm sets `surface_dirty=true` and triggers a redraw without panicking.
3. **Validate on the Linux dev machine**: launch `cargo run -p emterm-native-poc` and confirm no panic on three consecutive launches.
4. **Regression**: existing window_host tests (if any) keep passing under `cargo test -p emterm-native-poc`.

**Dependencies**: Blocks all manual exercise of Phases 3–7. Code-wise independent from Phase 1 (workspace re-layout) and can run in parallel.

**Testing Approach**:
- Unit: a smoke test that constructs `WindowHost` against a headless event loop (best-effort; may stay a manual test on the dev machine if headless wgpu is not available in Docker).
- Manual: 3-launch smoke check on Linux dev.

**Acceptance Criteria**:
- [ ] `cargo run -p emterm-native-poc` launches a window without panicking 3 times in a row.
- [ ] Resizing the window does not regress the existing Lost/Outdated recovery branch.

**Estimated Effort**: small.

---

### Phase 1: Extract `crates/term_images/` shared crate

**Goal**: Move the Kitty / SIXEL decoders and the APC / DCS parsers out of `src-tauri` and into a workspace-shared crate that both `src-tauri` and `native-poc` depend on by path.

**Files to Create**:
- `crates/term_images/Cargo.toml` — pure Rust crate, deps inherited from `src-tauri` (`image`, `flate2`, `serde`, etc.), no `tauri`.
- `crates/term_images/src/lib.rs` — module root re-exporting `image::*` (renamed `image_proc` internally to avoid clashing with the `image` crate) and `ansi::{apc, dcs}`.

**Files to Modify**:
- `Cargo.toml` (workspace root) — add `crates/term_images` to `[workspace] members`.
- `src-tauri/Cargo.toml` — replace local `image`/`ansi` module references with `term_images = { path = "../crates/term_images" }`.
- `src-tauri/src/image/` — **moved verbatim via `git mv`** to `crates/term_images/src/image_proc/` (rename the inner module to `image_proc` so the directory does not shadow the upstream `image` crate). All `use crate::ansi::...` references update to `use crate::ansi::...` (paths internal to the new crate so they stay valid).
- `src-tauri/src/ansi/{apc.rs, dcs.rs, mod.rs}` — **moved via `git mv`** to `crates/term_images/src/ansi/`. The `mod.rs` may shrink (Phase 3 only needs `apc` + `dcs`; any other modules currently in `src-tauri/src/ansi/` stay in `src-tauri`).
- `src-tauri/src/lib.rs` — remove `pub mod image;` and `pub mod ansi;` for the moved modules; re-export from `term_images` where needed by Tauri command handlers, **keeping the public API of `src-tauri` unchanged** so the rest of the Tauri build is untouched.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `term_images::image_proc::ImageProcessor` | Same as `src-tauri/src/image/mod.rs` ImageProcessor | bytes/payload received | Returns `Vec<ImageEvent>` (existing API preserved) |
| `term_images::ansi::apc::parse_kitty_command` | Parse APC payload | raw bytes | Returns `Option<KittyCommand>` (existing API preserved) |
| `term_images::ansi::dcs::parse_sixel_sequence` | Parse DCS SIXEL payload | raw bytes | Returns `Option<SixelData>` (existing API preserved) |

**Processing Flow** (diagram-convertible):

1. `git mv src-tauri/src/image/  crates/term_images/src/image_proc/`.
2. `git mv src-tauri/src/ansi/apc.rs  crates/term_images/src/ansi/apc.rs`.
3. `git mv src-tauri/src/ansi/dcs.rs  crates/term_images/src/ansi/dcs.rs`.
4. Author the new `Cargo.toml` and `lib.rs`.
5. Patch import paths inside the moved files (the only changes are at file headers).
6. Patch `src-tauri/src/lib.rs` to re-export from `term_images` for backwards-compatible internal use.
7. Run `cargo build --workspace`, then `cargo test --workspace`.

**Implementation Steps** (7 max):

1. **Sanity check**: confirm no `tauri::*` / `tauri_runtime::*` / `tauri_specta::*` references inside the to-be-moved files (one-time grep).
2. **Create new crate scaffolding**: `Cargo.toml` + empty `lib.rs` registered in workspace members.
3. **Move files via `git mv`** (preserves history).
4. **Adjust import paths**: `crate::ansi::apc` -> `crate::ansi::apc` is unchanged (now inside the same crate); `crate::image` stays inside the new crate as a sibling module.
5. **Re-export to `src-tauri`**: add `pub use term_images::image_proc as image;` and `pub use term_images::ansi;` in `src-tauri/src/lib.rs` so existing call sites in the legacy build are unaffected.
6. **Verify both builds**: `cargo build --workspace` + `cargo test --workspace` exit 0 (Docker per `sdd.yaml`). This is the SC-6 / NFR6 legacy compatibility gate for sub-phase 1 — legacy E2E excluded per SPEC.md SC-6 rationale.

**Dependencies**: Blocks Phase 5 (native image layer). Independent of Phase 0; can run in parallel.

**Testing Approach**:
- Unit: relocated `#[cfg(test)]` blocks in `kitty.rs` / `sixel.rs` / `decoder.rs` / `apc.rs` / `dcs.rs` continue to run under `cargo test -p term_images`.
- Integration: `src-tauri/tests/integration/image_tests.rs` keeps passing after pointing to `term_images::...` paths.
- E2E: legacy E2E (`./scripts/run-e2e-docker.sh`) is excluded from this SDD's gate per SPEC.md SC-6 rationale; `cargo test --workspace` is the substitute legacy compatibility gate.

**Acceptance Criteria**:
- [ ] `crates/term_images/` exists with image_proc + ansi modules.
- [ ] `cargo build --workspace` exits 0.
- [ ] `cargo test --workspace` exits 0 (image-decoder tests now under `term_images`; this is the SC-6 legacy compatibility gate for sub-phase 1).
- [ ] `cargo metadata` shows `term_images` with no `tauri` ancestor in its dep tree.

> Legacy E2E (`./scripts/run-e2e-docker.sh`) is **excluded** from this gate per SPEC.md SC-6 rationale.

**Estimated Effort**: medium (file moves are mechanical but cargo path adjustments must be careful).

---

### Phase 2: Dirty-row diff renderer

**Goal**: Replace `native-poc/src/render/`'s full-frame redraw with a per-row diff driven by `term_core::TerminalCore::get_dirty_rows`.

**Files to Modify**:
- `native-poc/src/render/mod.rs` — rewrite the draw loop to skip rows where `is_row_dirty(row) == false` **and** cursor / selection do not touch the row.
- `native-poc/src/render/theme.rs` — extend to surface palette / fg / bg / cursor color updates from OSC 4/10/11/12/104/110/111/112 (color application lives here; OSC reception lives in Phase 6).
- `native-poc/src/app.rs` — track "rows touched by cursor movement this frame" and "rows touched by selection edit this frame" so the renderer has a complete dirty set.
- `native-poc/src/window_host.rs` — call `TerminalCore::clear_dirty()` once after the renderer consumes the dirty rows.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `Renderer::dirty_set` | Union of `TerminalCore.get_dirty_rows()` and cursor/selection row deltas | Renderer about to draw a frame | Returns a `Vec<u16>` of rows to repaint |
| `Renderer::draw_row(row)` | Tessellate cells of one row | row is in dirty_set | egui Mesh appended; row not redrawn until next dirty event |
| `App.cursor_row_history` | Last frame's cursor row + this frame's cursor row | A frame is being prepared | Both rows are added to dirty_set so cursor leaves no ghost |
| `App.selection_row_history` | Last frame's selection ranges + this frame's | Same | Old + new rows are dirty |

**Processing Flow** (diagram-convertible):

1. Per-frame entry.
2. dirty = TerminalCore.get_dirty_rows().
3. dirty += previous_cursor_row, current_cursor_row.
4. dirty += previous_selection_rows, current_selection_rows.
5. For each row in dirty: Renderer.draw_row(row).
6. Cursor overlay drawn over the cursor row.
7. Selection overlay drawn over current_selection_rows.
8. ImageLayer.draw — full repaint when any placement add/remove/scroll, otherwise reuse last tessellation.
9. TerminalCore.clear_dirty().

**Implementation Steps**:

1. **Build the dirty union**: introduce `App::dirty_rows_this_frame()` that merges the four sources above.
2. **Refactor `Renderer::draw`** to iterate the dirty set instead of `0..rows`.
3. **Keep egui's clip+repaint coherent**: when a row is not in the dirty set, the renderer must skip its mesh contribution; egui's persistent paint cache handles the visual continuity.
4. **Cursor / selection ghost prevention**: capture last-frame cursor & selection rows before drawing, so vacated rows are also redrawn.
5. **Fallback switch**: in debug builds, expose `EMTERM_FULL_REDRAW=1` env var that forces the dirty set to `0..rows` for triage (FR error code `LOG_RENDER_FALLBACK`).
6. **Verify with a debug log**: `RUST_LOG=debug` prints the per-frame dirty row count.

**Dependencies**: Requires Phase 0 (window survives long enough to test). Independent of Phase 1.

**Testing Approach**:
- Unit: `dirty_rows_this_frame` returns the correct union for representative cursor moves, single-row writes, full-screen writes.
- Integration: drive `TerminalCore::process_pty_data` with a fixture sequence (single-line write, then cursor home, then bottom-line write); assert `get_dirty_rows` matches expectations after each step.
- Manual: visual confirmation of cursor blink + no ghosting + 60 FPS on `vim` / `htop`.

**Acceptance Criteria**:
- [ ] Renderer skips rows for which `is_row_dirty(row)` is false and which neither cursor nor selection touched.
- [ ] No visible ghosting on cursor / selection move.
- [ ] Debug log shows per-frame dirty-row count < `rows` during typical interactive use.

**Estimated Effort**: medium.

---

### Phase 3: Cursor + SGR rendering full reflection

**Goal**: Render every SGR attribute and every cursor shape / blink / visibility / color state that `term_core` already tracks.

**Files to Modify**:
- `native-poc/src/render/mod.rs` — apply bold, italic, reverse, blink, conceal, strikethrough, underline (single / double / curly) including SGR 58 underline color; ambiguous-width handling reads `settings.ambiguous_width_mode`.
- `native-poc/src/render/theme.rs` — resolve 4/8/24-bit color codes against the palette (with OSC 4/10/11/12 overrides applied earlier).
- `native-poc/src/settings.rs` — add `ambiguous_width_mode` (`narrow` | `wide`, default `narrow`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `Renderer::cell_style(cell)` | Map a `term_core` cell's packed fg/bg/flags into egui paint params | Cell exists in the grid | Returns concrete RGBA + font weight + italic/underline flags |
| `Renderer::draw_cursor` | Render block / underline / bar plus blink timer plus DECTCEM hidden | term_core cursor state is read | One overlay primitive per cursor on the dirty cursor rows |
| `width_of(ch, mode)` | Return display width in cells | `mode in {narrow, wide}` | Returns 0/1/2; East-Asian ambiguous = 2 if mode == wide else 1 |

**Processing Flow**:

1. Per dirty row, iterate cells left to right.
2. For each cell, compute style:
   - fg/bg using palette + 256/truecolor packed fields.
   - flags: bold -> use bold font face; italic -> italic face; underline tier; reverse -> swap fg/bg; blink -> alternate visibility at 1Hz; conceal -> draw bg only; strikethrough -> overlay line.
3. Cursor pass after cell pass:
   - if `get_cursor_visible()` == false: skip.
   - else style by `get_cursor_style()` (DECSCUSR / OSC 22), color by `get_cursor_fg()` (OSC 12), and blink by `get_cursor_blink()`.

**Implementation Steps**:

1. **Pull cursor state** from `term_core::TerminalCore` getters; remove any Phase 1 stubs that hardcoded block-only.
2. **Build a palette resolver** in `theme.rs` that handles indexed-256 plus any OSC 4 override (the override map lives on `Theme`).
3. **Extend `cell_style`** to honor all SGR flags listed in FR9.
4. **Wire `ambiguous_width_mode`** through `Settings -> App -> Renderer`.
5. **Cursor blink timer**: derive from `Instant::now()` modulo period (≈ 530ms to match xterm); blink is `App`-level so it can mark the cursor row dirty on toggle.

**Dependencies**: Requires Phase 2 (dirty-row infrastructure for cursor blink redraw).

**Testing Approach**:
- Unit: `cell_style` for each SGR combination (bold+reverse, underline-curly+color, etc.) returns the expected RGBA + flags.
- Manual: SGR sampler script side-by-side with the WebView build; visual parity check.

**Acceptance Criteria**:
- [ ] Every FR9 attribute renders correctly (verified manually against the WebView build).
- [ ] Cursor shape responds to DECSCUSR + OSC 22; visibility responds to DECTCEM.
- [ ] `ambiguous_width_mode` from `settings.json` is honored.

**Estimated Effort**: medium.

---

### Phase 4: Selection + scrollback + paste

**Goal**: Implement word / line selection, PRIMARY auto-copy, CLIPBOARD shortcuts, middle-click paste, bracketed paste, scrollback up to 10,000 lines with alt-screen suppression and auto-follow control.

**Files to Modify**:
- `native-poc/src/selection.rs` — extend `Selection` with `mode: Character|Word|Line` and the ordered-range/text-resolution semantics required by word/line; add a sanitizer for bracketed paste content.
- `native-poc/src/window_host.rs` — translate triple/double click events; translate `Ctrl+Shift+C`, `Ctrl+Shift+V`, middle-click; route `Shift+PageUp/Down`, `Shift+Home/End`, wheel into scrollback control.
- `native-poc/src/app.rs` — own a `ScrollPosition` enum (`Live` | `OffsetFromLive(rows: u32)`) and the alt-screen-aware update rule.
- `native-poc/src/settings.rs` — add `scrollback_lines` (default 10000).
- `native-poc/src/pty/mod.rs` — surface a write helper for bracketed-paste-wrapped payloads.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `Selection::extend(pos, mode)` | Extend selection respecting current mode | mouse-down already set anchor | Selection range covers character / word / line according to mode |
| `Selection::word_boundary(cell_at)` | Locate word edges around a cell | A cell exists at `cell_at` | Returns (start, end) cell pair using `term_core::char_table` or unicode segmentation |
| `bracketed_paste(text, enabled)` | Decide wire format for a paste | clipboard text obtained | If enabled and DECSET 2004: returns `\e[200~ <sanitized text> \e[201~`; else returns the text |
| `sanitize_bracket_sequences(text)` | Strip embedded `\e[201~` from paste body | raw text | Returns text safe to wrap |
| `App.scroll_position` | Current scroll offset relative to live tail | Frame is about to render | Renderer queries `ring_buffer` window at that offset |
| `App.on_pty_output()` | Decide whether new output auto-follows | scroll_position state | If `Live`, advance live tail; if offset > 0, keep offset (no auto-follow) |

**Processing Flow** (selection):

1. mouse_down at pos.
2. Selection.anchor = cell_at(pos). Selection.mode = match click_count { 1=>Char, 2=>Word, 3=>Line }.
3. mouse_drag pos -> Selection.head = cell_at(pos), mark new+old intersect rows dirty.
4. mouse_up -> text = Selection.resolve(term_core) -> arboard::Clipboard.set_text_primary(text).
5. Ctrl+Shift+C -> arboard::Clipboard.set_text(text).
6. Ctrl+Shift+V or middle-click -> text = arboard get -> bracketed_paste(text, term_core.get_mode(2004)) -> pty.write(payload).

**Processing Flow** (scrollback):

1. wheel up (3 lines/tick) -> scroll_position = OffsetFromLive(current + 3 lines).
2. Shift+PageUp -> +rows lines; Shift+End -> Live; Shift+Home -> max-offset.
3. alt-screen active (DECSET 1049) -> scroll_position pinned to Live; scroll inputs are ignored.
4. PTY emits new bytes -> if scroll_position == Live, live tail advances; else offset preserved.

**Implementation Steps**:

1. **Selection mode**: extend the existing `Selection` struct with the `mode` field and the word/line ranges.
2. **Click classifier**: detect double / triple click using a 500 ms window in `window_host.rs`.
3. **arboard integration**: hook PRIMARY on mouse-up; hook CLIPBOARD on Ctrl+Shift+C; hook paste on Ctrl+Shift+V and middle-click.
4. **Bracketed paste plumbing**: read DECSET 2004 from `term_core.get_mode`; wrap via the helper above.
5. **Scrollback state machine** in `app.rs`: the `ScrollPosition` enum and the alt-screen suppression rule.
6. **Settings**: add `scrollback_lines` + plumb to `term_core` ring-buffer cap at `Tab::spawn_shell`.

**Dependencies**: Requires Phase 2 (dirty-row infrastructure so selection drag stays performant) and Phase 3 (so the selection highlight paints correctly). Independent of Phase 5 / 6.

**Testing Approach**:
- Unit: `Selection` word/line boundary computation against fixture grids; `bracketed_paste` adds/omits wrap correctly; `sanitize_bracket_sequences` removes embedded `\e[201~`.
- Unit: `App.scroll_position` transitions are correct for wheel up/down, page up/down, home/end, alt-screen toggle.
- Integration: spawn a bash PTY, write a large output, scroll back, verify the viewport shows the expected ring-buffer slice.
- Manual: mouse word/line selection against running `vim`, copy with Ctrl+Shift+C, paste into another terminal app, verify content + that PRIMARY auto-copy works on selection.

**Acceptance Criteria**:
- [ ] Mouse drag / double-click / triple-click select character / word / line.
- [ ] PRIMARY auto-copy on mouse-up; CLIPBOARD on Ctrl+Shift+C; CLIPBOARD paste on Ctrl+Shift+V; PRIMARY paste on middle-click.
- [ ] Bracketed paste wrap applied iff DECSET 2004 is enabled.
- [ ] 10,000-line scrollback works; alt-screen suppresses scrollback; new output does not auto-follow when off-tail.

**Estimated Effort**: large.

---

### Phase 5: Inline image overlay layer (Kitty + SIXEL)

**Goal**: Render Kitty Graphics Protocol and SIXEL payloads inline at pixel parity with the WebView build using a new `native-poc/src/image/` module backed by `term_images`.

**Files to Create**:
- `native-poc/src/image/mod.rs` — `ImageLayer` orchestrator owning GPU textures + placement map.
- `native-poc/src/image/overlay.rs` — wgpu draw routine for placements.
- `native-poc/src/image/parse.rs` — APC / DCS payload -> `term_images::image_proc::ImageProcessor` adapter.

**Files to Modify**:
- `native-poc/Cargo.toml` — add `term_images = { path = "../crates/term_images" }`.
- `native-poc/src/callbacks.rs` — `on_apc` / `on_dcs` buffer raw payloads onto `NativeCallbackState.pending_apc` / `pending_dcs` (replacing the Phase 1 `log::debug!` stubs). Add `pending_apc: Vec<Vec<u8>>` and `pending_dcs: Vec<Vec<u8>>` fields to `NativeCallbackState`. Decode happens in `Tab::pump` because the trait method has no cursor access.
- `native-poc/src/render/mod.rs` — call `ImageLayer::draw` after cells/cursor/selection are drawn.
- `native-poc/src/app.rs` — own one `ImageLayer` per tab.
- `native-poc/src/settings.rs` — add `image_memory_quota_mb` (default 320).
- `native-poc/src/tabs.rs` — after each `process_pty_data` chunk, drain `cb_state.pending_apc` / `pending_dcs`, snapshot cursor, run `parse::decode_apc` / `decode_dcs`, push events into `ImageLayer::ingest`. Also pump `ImageEvent::Response { data }` back into PTY (terminal -> shell response).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `ImageLayer::ingest(events)` | Apply `Vec<ImageEvent>` from the decoder | Decoder returned events | Textures uploaded or removed, placements updated, animation queue advanced |
| `ImageLayer::upload(decoded)` | Convert `DecodedImage` to a wgpu texture | RGBA8 bytes available | Texture handle stored in LRU, indexed by `image_id` |
| `ImageLayer::evict_until_quota()` | LRU eviction | Memory quota exceeded | Oldest unused texture(s) dropped, `LOG_IMG_QUOTA` logged |
| `ImageLayer::draw(grid_offset)` | Draw all placements | placements list, z-order known | Quads drawn at (col,row) -> pixel coordinates, multiplied by cell size, clipped to viewport |
| `NativeCallbacks::on_apc(data)` | Buffer APC payload onto `NativeCallbackState.pending_apc` (trait method receives only `&self` + `&[u8]`; cursor coordinates are unavailable at this call site) | bytes available | Drained in `Tab::pump` after `process_pty_data` returns |
| `NativeCallbacks::on_dcs(data)` | Same buffering, onto `NativeCallbackState.pending_dcs` | bytes available | Drained in `Tab::pump` |
| `Tab::drain_pending_apc_dcs(image_layer)` | Decode buffered bytes against the now-current cursor position | `process_pty_data` returned, `core.lock()` held | For each pending APC/DCS, snapshot `core.get_cursor_row()` / `get_cursor_col()`, call `term_images::ansi::apc::parse_kitty_command` / `parse_sixel_sequence`, then `ImageProcessor::process_kitty_command(&cmd, cursor_row, cursor_col)` / `process_sixel(&data, cursor_row, cursor_col)`. Resulting `Vec<ImageEvent>` is fed to `ImageLayer::ingest`. |
| `parse::decode_apc(bytes, cursor_row, cursor_col, processor)` | Pure-logic adapter used by `Tab::drain_pending_apc_dcs` | bytes + cursor coords + processor | Returns `Vec<ImageEvent>` |
| `parse::decode_dcs(bytes, cursor_row, cursor_col, processor)` | Same for DCS/SIXEL | bytes + cursor coords + processor | Returns `Vec<ImageEvent>` |

> Note: the `TerminalCallbacks` trait surface is fixed (Phase 2). It does not include cursor coordinates and is `&self`-only. Therefore APC/DCS decoding cannot happen *inside* `on_apc` / `on_dcs`: those methods only buffer the raw bytes. The decode runs after `process_pty_data` returns in `Tab::pump`, at which point the cursor position reflects the end-of-chunk state — which matches the WebView build, where APC/DCS payloads are also drained on the post-callback path.

**Processing Flow**:

1. PTY bytes carrying APC arrive.
2. term_core parser detects APC -> calls `NativeCallbacks::on_apc(data)` (trait method, `&self`-only). `NativeCallbacks` appends `data.to_vec()` to `NativeCallbackState.pending_apc` and returns immediately.
3. After `process_pty_data` returns, `Tab::pump` locks `core`, reads cursor position, drains `pending_apc` / `pending_dcs`, and calls `parse::decode_apc(data, cursor_row, cursor_col, &mut processor)` / `decode_dcs(...)`. Each returns `Vec<ImageEvent>`.
4. ImageLayer.ingest(events):
   - `ImageReady { image }` -> upload(image) (creates wgpu texture, LRU insert).
   - `Place { placement }` -> placements.insert(placement).
   - `Delete { target }` -> remove matching placements/textures.
   - `Response { data }` -> queue write to PTY via `tabs::pump`.
   - `Animation(...)` -> animation queue update.
5. On scroll, placements are not mutated; the renderer adds the scroll offset when computing screen coordinates.
6. On resize, ImageLayer recomputes per-cell pixel dimensions; placements stay anchored to their (row, col).

**Implementation Steps**:

1. **`ImageLayer` skeleton**: struct with `textures: HashMap<u32, wgpu::Texture>`, `placements: BTreeMap<(z_index, image_id), ImagePlacement>`, `lru: LruVec`, quota accounting.
2. **Texture upload pipeline**: `Rgba8UnormSrgb`, un-premultiplied source; one pipeline per surface format; sampler with linear filter.
3. **Draw pass**: simple textured-quad pipeline placed after the egui scene; clipping to viewport; z-order from placement.
4. **APC/DCS parse adapter** in `parse.rs` consuming `term_images` APIs. The adapter signature is `decode_apc(bytes, cursor_row, cursor_col, &mut ImageProcessor)` / `decode_dcs(bytes, cursor_row, cursor_col, &mut ImageProcessor)` because the underlying `ImageProcessor::process_kitty_command` / `process_sixel` require the cursor coordinates.
5. **Wiring in `callbacks.rs` + `tabs.rs`**: replace the Phase 1 `log::debug!` stubs in `on_apc`/`on_dcs` with `state.pending_apc.push(data.to_vec())` / `state.pending_dcs.push(data.to_vec())`. Then in `Tab::pump`, after `core.lock().process_pty_data(...)` returns, drain those buffers, snapshot `core.get_cursor_{row,col}()`, and feed each entry to `parse::decode_apc` / `parse::decode_dcs`. The resulting `Vec<ImageEvent>` is fed to `ImageLayer::ingest`. (Reason: the `TerminalCallbacks` trait is `&self`-only with no cursor parameter, so the decode cannot happen inside the callback itself.)
6. **Quota + LRU**: derive memory from `width * height * 4` per stored texture; evict oldest when over.
7. **Response loop**: drain `ImageEvent::Response`s in `tabs::pump` and feed them into `PtySession::write` so Kitty status queries get answered.

**Dependencies**: Requires Phase 1 (term_images crate). Independent of Phase 4. Phase 6's OSC test matrix verifies APC/DCS coverage end-to-end.

**Testing Approach**:
- Unit: `parse::on_apc` decodes a fixture Kitty payload, returns `ImageReady` + `Place`.
- Unit: `parse::on_dcs` decodes a fixture SIXEL payload, returns `ImageReady` + `Place`.
- Unit: `ImageLayer.evict_until_quota` evicts oldest entries when memory exceeds the quota.
- Integration: send a Kitty APC through `term_core::process_pty_data` -> verify `ImageLayer.placements` gains the new entry.
- Manual: visual parity between Tauri (WebView) and native builds for representative payloads (Kitty PNG, Kitty animated GIF, SIXEL from `img2sixel`).

**Acceptance Criteria**:
- [ ] Kitty PNG via `emterm image foo.png` renders inline at expected position.
- [ ] SIXEL from `img2sixel` renders inline.
- [ ] Scrolling moves images with the text rows.
- [ ] LRU evicts oldest textures when 320MB quota is exceeded.

**Estimated Effort**: large.

---

### Phase 6: OSC dispatch matrix + notifications + clipboard policy

**Goal**: Cover every `action_type` emitted by `term_core::osc_handler` with a native-side handler (or explicit queue) and a unit test. Add OSC 9 notifications and OSC 52 clipboard with a default-deny policy.

**Files to Modify**:
- `native-poc/src/callbacks.rs` — extend the OSC match arm to cover every `action_type`:

| `action_type` | Native behavior in Phase 3 |
|---------------|---------------------------|
| 0, 2 | Update `tab.title` (existing). |
| 1 | Log only (no icon UI). |
| 4 | Update palette in `Theme` (parse data, set indexed slot). |
| 7 | Store `tab.cwd: Option<String>`. |
| 8 | Log; `term_core` registers the URI internally. |
| 9 | If rate-limit passes, fire `notify-rust` with title=tab.title, body=data. |
| 10 | `Theme.fg` update. |
| 11 | `Theme.bg` update. |
| 12 | `Theme.cursor_fg` update (also referenced by Phase 3 cursor render). |
| 22 | `Theme` / cursor style updated (DECSCUSR equivalent). |
| 52 | If `settings.clipboard_read_osc52 == true` (default) **and** payload length ≤ `settings.clipboard_max_size_osc52` (default 10 MB): arboard set/get; else log `LOG_OSC52_DENIED`. |
| 104 | Palette reset. |
| 110 | fg reset. |
| 111 | bg reset. |
| 112 | cursor color reset. |
| 133 | Store `prompt_mark` on the current row (state only; consumed by future search). |
| 100 (wire 777) | Push onto `osc_queue` (existing — Phase 5 viewer drains it). |
| 101 (wire 1337) | Log only. |
| 255 | `log::warn!` and ignore. |

- `native-poc/src/settings.rs` — add `clipboard_read_osc52: bool` (default `true`) and `clipboard_max_size_osc52: u32` (default `10 * 1024 * 1024`), mirroring the legacy `src-tauri/src/commands/config/settings.rs` fields so a single `settings.json` works for both builds.
- `native-poc/src/callbacks.rs` — add an in-process rate-limit table `(title, body) -> last_emitted: Instant`, suppress duplicates within 1 second.
- (NEW dependency) `notify-rust` added to `native-poc/Cargo.toml`.
- `native-poc/src/tabs.rs` — `mux` data (action_type-less wire 9999) was already re-fired by `term_core` as APC; document that the APC path in Phase 5 logs and queues it.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `NativeCallbacks::on_osc` | Per-action dispatch | action_type + data | Mutates `NativeCallbackState` and/or `Theme` and/or fires notify-rust; never panics |
| `Theme::apply_osc(action_type, data)` | Mutate color state | Theme exists | Palette/fg/bg/cursor color updated according to action |
| `NotificationRateLimiter` | Dedupe identical notifications within 1 s | (title, body) | Returns `should_emit: bool` |
| `Osc52Policy::check(settings, payload_len)` | Permission decision | Settings loaded | `clipboard_read_osc52 == true && payload_len <= clipboard_max_size_osc52` -> set/get; otherwise log `LOG_OSC52_DENIED` + drop |

**Processing Flow**:

1. `on_osc(action_type, data)` enters the match arm.
2. Branches as per the table; state mutations happen behind `state.lock()`.
3. For notification, dedupe; for clipboard, check policy; for color state, also `term_core.mark_all_dirty()` so the renderer repaints with the new palette.

**Implementation Steps**:

1. **Expand the match arm** in `NativeCallbacks::on_osc`.
2. **`Theme` mutators** for palette and fg/bg/cursor color including OSC 4/104, 10/110, 11/111, 12/112.
3. **`notify-rust` integration** + rate limiter (`HashMap<(String,String), Instant>` with a sweep on insert). The dispatcher is a trait (`trait NotificationSink { fn send(&self, title: &str, body: &str); }`) with a `NotifyRustSink` production impl and a `TestSink` for unit tests. `NotificationRateLimiter` takes a clock closure (`now: Box<dyn Fn() -> Instant>`) so TS-7 can advance time deterministically without spinning the real `Instant::now`.
4. **OSC 52 policy gate** reading `clipboard_read_osc52` (default `true`) and `clipboard_max_size_osc52` (default 10 MB) from `Settings`; deny if either gate fails and emit `LOG_OSC52_DENIED`.
5. **OSC 133 semantic prompt**: persist `prompt_mark` on the row (term_core has the cell-level mark mechanism in `terminal_rows.rs` already).
6. **Test each branch**: a Recorder-style `TerminalCallbacks` mock is already in `term_core::callbacks::tests`; mirror that pattern in `native-poc/src/callbacks.rs` tests.

**Dependencies**: Requires Phase 5 (`on_apc`/`on_dcs` are touched as part of that phase; this phase only adds the OSC branches).

**Testing Approach**:
- Unit: one test per `action_type` (0,1,2,4,7,8,9,10,11,12,22,52,104,110,111,112,133,100,101,255) confirming the documented effect or queue.
- Unit: rate limiter dedupes identical notifications within 1 s.
- Unit: OSC 52 policy honors `allow` vs `deny`.
- Manual: visible OS notification on `printf '\033]9;hello\007'`.

**Acceptance Criteria**:
- [ ] Every `action_type` has at least one unit test.
- [ ] OSC 9 fires an OS notification (default; rate-limited).
- [ ] OSC 52 honors `clipboard_read_osc52` (default `true`) and `clipboard_max_size_osc52` (default 10 MB), mirroring the legacy WebView build; denial logs `LOG_OSC52_DENIED`.
- [ ] OSC 4 / 10 / 11 / 12 visibly change palette/fg/bg/cursor color.

**Estimated Effort**: medium.

---

### Phase 7: Stability re-verification (12+ hour Claude Code session)

**Goal**: Demonstrate 12+ hour Claude Code session stability without crash, screen loss, or monotonic memory growth.

**Files to Create**: (none)

**Files to Modify**:
- `native-poc/README.md` — Phase 3 feature matrix update.

**Key Activities**:

| Activity | Method | Outcome |
|----------|--------|---------|
| Long-run session | Launch `cargo run -p emterm-native-poc`, run `claude` inside, use it for ≥ 12 h or idle | Window stays responsive |
| Memory sampling | `ps -o rss,vsz <pid>` and `nvidia-smi`/`radeontop` (whichever applies) at 4h / 8h / 12h marks | Three samples, no monotonic upward trend |
| Visual parity | Run Kitty + SIXEL fixtures side-by-side against the legacy WebView build | No visible differences |
| SGR parity | Run an SGR sampler script in both builds | No visible differences |
| Workspace tests | `cargo test --workspace` final pass (this is the SC-6 final legacy compatibility gate; legacy E2E excluded per SPEC.md SC-6 rationale) | Green |

**Implementation Steps**:

1. **Pre-run checklist**: Phase 0–6 all complete; `cargo test --workspace` green.
2. **Session run**: start the session, take a screenshot, leave running.
3. **Sample memory**: at 4h, 8h, 12h marks. Log to `tmp/phase3-stability.log`.
4. **Parity checks**: Kitty + SIXEL + SGR sampler.
5. **Update README** with final feature matrix.

**Dependencies**: Requires Phases 0–6 to be complete.

**Testing Approach**:
- Manual only. No automation in Phase 3 (per SPEC 12.1).

**Acceptance Criteria**:
- [ ] 12+ hour session completes without crash / screen loss.
- [ ] Memory samples at 4h / 8h / 12h show no monotonic growth.
- [ ] Kitty + SIXEL + SGR visual parity confirmed.
- [ ] `cargo test --workspace` exits 0 (SC-6 final legacy compatibility gate; legacy E2E excluded per SPEC.md SC-6 rationale).

**Estimated Effort**: small (mostly waiting + observing).

---

## Complete File Structure

```
emterm/
├── Cargo.toml                       # + crates/term_images in workspace members
├── crates/
│   ├── term_core/                   # unchanged
│   └── term_images/                 # NEW (Phase 1)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs               # module roots
│           ├── image_proc/          # was src-tauri/src/image/
│           │   ├── mod.rs
│           │   ├── animation.rs
│           │   ├── decoder.rs
│           │   ├── kitty.rs
│           │   ├── limiter.rs
│           │   ├── placement.rs
│           │   ├── sixel.rs
│           │   └── store.rs
│           └── ansi/                # was src-tauri/src/ansi/{apc,dcs}.rs
│               ├── mod.rs
│               ├── apc.rs
│               └── dcs.rs
├── src-tauri/
│   ├── Cargo.toml                   # + term_images path dep
│   └── src/
│       ├── lib.rs                   # re-export term_images modules for backwards compat
│       ├── image/                   # removed (moved to crates/term_images)
│       └── ansi/                    # only non-apc/dcs files (if any) remain; apc/dcs moved
├── native-poc/
│   ├── Cargo.toml                   # + term_images, + notify-rust
│   └── src/
│       ├── app.rs                   # extend: dirty-row union, scroll position, image layer per tab
│       ├── callbacks.rs             # extend: full OSC matrix, APC/DCS routing, OSC 52 policy
│       ├── selection.rs             # extend: word/line modes, bracketed paste helpers
│       ├── settings.rs              # extend: scrollback_lines, ambiguous_width_mode, image_memory_quota_mb, clipboard_read_osc52, clipboard_max_size_osc52
│       ├── tabs.rs                  # extend: cwd, scrollback control, ImageEvent response drain
│       ├── window_host.rs           # fix surface lost; route mouse/keyboard for selection/paste/scroll
│       ├── pty/                     # unchanged
│       ├── render/
│       │   ├── mod.rs               # rewrite: dirty-row diff, full SGR, cursor shape, image overlay call
│       │   └── theme.rs             # extend: palette, fg/bg/cursor color from OSC 4/10/11/12 etc.
│       ├── ui/                      # unchanged
│       ├── viewer/                  # unchanged (Phase 5 deferred)
│       ├── ime/                     # unchanged
│       └── image/                   # NEW (Phase 5)
│           ├── mod.rs               # ImageLayer
│           ├── overlay.rs           # wgpu textured-quad render
│           └── parse.rs             # APC/DCS -> term_images::image_proc::ImageProcessor adapter
└── doc/tasks/native-terminal-features/
    ├── SPEC.md                      # existing
    ├── 要件定義書.md                # existing
    ├── sdd.yaml                     # existing
    ├── IMPLEMENTATION.md            # NEW (this file)
    ├── VERIFICATION.md              # NEW
    └── tasks.yaml                   # NEW
```

## Testing Strategy

- **Unit**: `cargo test -p emterm-native-poc` and `cargo test -p term_images` carry the new Phase 3 surface. Target ≥ 80% coverage on `selection.rs`, `callbacks.rs`, and `image/parse.rs` (these are pure-logic paths). `term_core` and `term_images` retain their existing test bodies (no drops).
- **Integration**: `native-poc/tests/` houses end-to-end flows that drive `TerminalCore::process_pty_data` with byte sequences and assert state transitions (dirty rows, image events, OSC effects).
- **E2E**: legacy `e2e-tests/` (WebdriverIO + tauri-driver) is **excluded** from this SDD's gate per SPEC.md SC-6 rationale (preexisting failing-spec parity vs. `main` confirmed 2026-05-12; `src-tauri/` retires in Phase 7 of `tmp/restruct.md`). `cargo test --workspace` is the substitute legacy compatibility gate. There are **no new E2E specs** for `native-poc` because no headless driver covers tao+wgpu+egui; manual verification fills that gap.
- **Manual**: visual parity + 12 h session per Phase 7 above.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `term_images` (internal) | path | Kitty / SIXEL decoders + APC/DCS parsers, shared with `src-tauri` |
| `notify-rust` | latest 4.x | OSC 9 OS notifications (Linux: D-Bus org.freedesktop.Notifications) |
| `image` (transitive via `term_images`) | 0.25.x | PNG/JPEG/GIF/WebP decode (already in `src-tauri`'s lockfile) |
| `flate2` (transitive via `term_images`) | inherits from `src-tauri` | Kitty `z=1` zlib decompression |
| `arboard` 3 | existing | PRIMARY + CLIPBOARD on Linux |

External versions are pinned via the existing workspace `Cargo.lock`. No version bump is required for already-resident crates.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Image crate extraction breaks an obscure `src-tauri` test path | Medium | Medium | Phase 1 ends with `cargo test --workspace` green (SC-6 legacy compatibility gate; legacy E2E excluded per SPEC.md SC-6 rationale) before Phase 5 starts. `git mv` preserves blame so review is mechanical. |
| Dirty-row diff hides bug class (rows that should redraw but don't) | Medium | Medium | Debug `EMTERM_FULL_REDRAW=1` env var forces full redraw; ghosting checks during Phase 3 + Phase 7 visual parity. |
| `notify-rust` rate-limit too aggressive and drops legitimate notifications | Low | Low | Dedupe is keyed on `(title, body)` exact match within 1 s; manual confirm with `printf '\033]9;notif1\007'; printf '\033]9;notif2\007'`. |
| OSC 52 size cap (10 MB default) drops oversized clipboard payloads | Low | Low | Mirrors the legacy WebView build's existing cap; user can raise `clipboard_max_size_osc52` in `settings.json`. Denial path logs `LOG_OSC52_DENIED`. |
| wgpu texture allocation pattern leaks across resize storms | Medium | High | Phase 7 12h memory sampling at 4 / 8 / 12 h marks; per-frame texture handle count logged at `RUST_LOG=debug`. |
| Phase 1 PoC surface-lost panic recurs on hardware variants | Low | High | Phase 0 fix is on the very first frame configure path; defensive — applies to subsequent Lost / Outdated events too. |
| APC/DCS decode happens after `process_pty_data` returns (because the `TerminalCallbacks` trait is `&self`-only with no cursor parameter). Cursor position used for placement reflects end-of-chunk, which may differ from intra-chunk APC arrival point. | Low | Low | Matches the legacy WebView build (which also decodes payloads post-callback). For typical Kitty/SIXEL usage the cursor sits right after the payload start, so the difference is invisible. Documented in Phase 5. |
| Schedule slip on Phase 5 (largest phase) eats Phase 7 budget | Medium | Medium | If Phase 5 runs over, animation frames are deferred (use first-frame only) and revisited in a Phase 5.5 follow-up; Phase 7 12h session can still proceed on static-image parity. |

## Open Questions

All seven Open Questions in the SPEC are resolved above (see *Resolved Open Questions*). No further planner-level questions remain. Items intentionally deferred:

- [ ] OSC 52 interactive "ask" prompt (per-request user confirmation) — Phase 5+ (requires viewer/UI surface). Phase 3 uses only the binary `clipboard_read_osc52` toggle + `clipboard_max_size_osc52` size cap.
- [ ] OSC 1337 image subset — not in Phase 3 (Kitty is the canonical path).
- [ ] Automated 12 h session — Phase 3 is manual per SPEC.

## Success Metrics

- [ ] FR1–FR14 demonstrably working (see Phase acceptance criteria + VERIFICATION.md).
- [ ] US1–US9 acceptance criteria checked.
- [ ] `cargo test --workspace` green.
- [ ] Manual visual parity for Kitty + SIXEL + SGR vs. legacy Tauri build.
- [ ] 12+ hour Claude Code session passes (NFR2).
- [ ] Legacy Tauri build's `cargo test --workspace` continues to pass (NFR6 / SC-6). Legacy E2E is excluded from this SDD's gate per SPEC.md SC-6 rationale.
