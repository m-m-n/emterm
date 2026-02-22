# Feature: Kitty Protocol Compatibility

## Overview

Improve eMterm's Kitty Graphics Protocol compatibility to work correctly with external tools that rely on Kitty capability detection (treemd, kitten icat, ratatui-image). The current implementation has timing issues with query responses and incomplete device response support that cause rendering failures in these tools.

## Objectives

- Fix Kitty query response timing so capability detection libraries (crossterm/ratatui-image) work correctly
- Implement XTWINOPS (CSI 14t/16t/18t) device responses for accurate cell size reporting
- Ensure Kitty image pipeline works with kitten icat (display via image viewer overlay)
- Support Kitty animation frames
- Ensure treemd renders markdown correctly without red background or warnings

## User Stories

### US1: TUI Application Compatibility
As a user, I want to use treemd to view markdown files, so that I can read formatted documentation in the terminal.

**Acceptance Criteria:**
- [ ] treemd opens markdown files without "⚠ No interactive elements in this section" warning
- [ ] treemd renders content without red background
- [ ] treemd correctly detects eMterm's capabilities

### US2: Kitty Image Display
As a user, I want to view images using `kitten icat`, so that I can preview images inline without leaving the terminal.

**Acceptance Criteria:**
- [ ] `kitten icat image.png` displays the image in eMterm's image viewer overlay
- [ ] Behavior matches `emterm image image.png`
- [ ] Image is displayed at correct dimensions

### US3: Kitty Animation Support
As a user, I want Kitty animation frames to display correctly, so that animated content in TUI apps works properly.

**Acceptance Criteria:**
- [ ] Kitty animation control commands (a=f, a=a) are handled
- [ ] treemd Kitty-mode animations render correctly

## Technical Requirements

### Functional Requirements

- **FR1: Synchronous Kitty query response** — Kitty Graphics Protocol queries (`a=q`) must be responded to synchronously in the WASM layer, using the same response path as DA1/DSR (response_buffer + fire_device_response_callback). This prevents late responses from being misinterpreted as key events by crossterm.

- **FR2: XTWINOPS device responses** — eMterm must respond to CSI 14t (text area pixel size), CSI 16t (cell size in pixels), and CSI 18t (text area in characters). These responses enable ratatui-image to determine correct pixel dimensions for Kitty rendering.

- **FR3: Cell size synchronization on buffer switch** — When switching to/from alternate buffer (CSI ?1049h/l, ?47h/l), the newly created WASM core must have `cell_size_px` set to the actual character dimensions, not defaults (8x16).

- **FR4: Kitty image pipeline compatibility** — Kitty image commands (`a=T`, `a=t`, `a=p`) received from external tools (kitten icat) must be processed through the existing APC→Tauri IPC→Rust backend→image_event pipeline and displayed in the image viewer overlay.

- **FR5: Kitty animation frame support** — Kitty animation control commands (`a=f` frame, `a=a` animate) must be handled by the image pipeline.

### Non-Functional Requirements

- **NFR1 — Response timing:** All device responses (Kitty query, DA1, CSI 16t, DSR) must arrive at the PTY within the same processing pass, before ratatui-image's 2000ms timeout.
- **NFR2 — No regression:** Existing `emterm image` functionality must not be affected.
- **NFR3 — Performance:** Synchronous Kitty query handling must not add measurable latency to PTY data processing.

## Implementation Approach

### Architecture

**Response Data Flow (Synchronous — queries/device status):**
```
PTY data → WASM parser → detect sequence type:
  APC (a=q)  → try_handle_kitty_query() → response_buffer → callback → PTY write
  CSI 16t    → handle_xtwinops_cell_size() → response_buffer → callback → PTY write
  CSI c      → handle_primary_device_attributes() → response_buffer → callback → PTY write
  CSI 5n     → handle_device_status_report() → response_buffer → callback → PTY write
```

**Image Data Flow (Asynchronous — actual image data):**
```
PTY data → WASM parser → APC (a≠q) → fire_apc_callback()
→ TS handleApcCallback() → Tauri invoke("process_image_data")
→ Rust KittyHandler::handle_command() → decode image
→ emit image_event → TS handleImageEvent()
→ Image viewer overlay display
```

**Capability Detection Sequence (ratatui-image):**
```
Tool sends (all at once):
  \x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\   (Kitty query)
  \x1b[c                                          (DA1)
  \x1b[16t                                        (CSI 16t — cell size)
  \x1b[5n                                         (DSR — sentinel)

eMterm responds (synchronously during process_pty_data):
  \x1b_Gi=31;OK\x1b\\                             (Kitty OK)
  \x1b[?64;1;2;6;22c                              (DA1)
  \x1b[6;<cell_h>;<cell_w>t                        (CSI 16t)
  \x1b[0n                                          (DSR OK)
```

### Component Changes

**WASM layer (`wasm/src/`):**
- `apc_handler.rs` — Kitty query detection and synchronous response (exists)
- `csi_device.rs` — XTWINOPS handlers (exists)
- `csi_dispatch.rs` — CSI `t` dispatch (exists)
- `terminal_core.rs` — `cell_width_px`/`cell_height_px` fields, `set_cell_size_px()` (exists)

**TypeScript layer (`src/`):**
- `terminal-app/index.ts` — `set_cell_size_px` calls at init, resize, and buffer switch
- `terminal/state.ts` — Cell size propagation to WASM cores on buffer switch

**Rust backend (`src-tauri/src/image/`):**
- `kitty.rs` — Verify animation frame handling (`a=f`, `a=a`)
- `animation.rs` — Animation state management

### Dependencies

**Internal Dependencies:**
- WASM parser (apc_handler, csi_dispatch, csi_device)
- Callback mechanism (fire_device_response_callback)
- Image pipeline (APC callback → Tauri IPC → Rust image handler)
- Image viewer overlay (WebView-based)

**External Dependencies:**
- ratatui-image v10.0.6+ (Kitty capability detection behavior)
- crossterm (terminal capability detection, raw mode)
- kitten icat (Kitty Graphics Protocol client)

### File Structure

```
wasm/src/
├── apc_handler.rs          # Kitty query synchronous response
├── csi_device.rs           # XTWINOPS handlers (CSI 14t/16t/18t)
├── csi_dispatch.rs         # CSI dispatch (includes 't')
├── terminal_core.rs        # cell_size_px fields, set_cell_size_px()
src/
├── terminal-app/index.ts   # set_cell_size_px calls, APC/image handling
├── terminal/state.ts       # syncModesFromWasm, cell size propagation
src-tauri/src/image/
├── kitty.rs                # Kitty protocol handler
├── animation.rs            # Animation frame management
```

## Test Scenarios

### Unit Tests (WASM)
- [ ] Kitty query with image ID responds with correct format
- [ ] Kitty query without image ID responds correctly
- [ ] Kitty query with quiet=1 suppresses response
- [ ] Non-query Kitty APC passes through to normal callback
- [ ] CSI 16t responds with correct cell dimensions
- [ ] CSI 14t responds with correct text area pixel size
- [ ] CSI 18t responds with correct text area character size
- [ ] Cell size defaults to 8x16 when not set

### Unit Tests (TypeScript)
- [ ] syncModesFromWasm reads WASM mode bits correctly
- [ ] set_cell_size_px called on init with measured character size
- [ ] set_cell_size_px called on resize with new character size

### Integration Tests
- [ ] Capability detection sequence (Kitty query + DA1 + CSI 16t + DSR) produces correct responses in correct order
- [ ] Buffer switch (CSI ?1049h) creates alternate core with correct cell size
- [ ] Kitty image data (a=T) routes through async pipeline to image handler

### E2E Tests (Manual)
- [ ] `treemd README.md` displays without warnings or red background
- [ ] `kitten icat image.png` displays image in viewer overlay
- [ ] `emterm image image.png` continues to work correctly

## Error Handling

| Error | Condition | Handling |
|-------|-----------|----------|
| Kitty query timeout | ratatui-image doesn't receive responses within 2000ms | Ensure synchronous response path has no delays |
| Invalid cell size | cell_size_px not set before CSI 16t query | Use defaults (8x16), log warning |
| APC callback not registered | Core created without callback registration | fire_device_response_callback is no-op when callback is None |

## Success Criteria

- [ ] All functional requirements (FR1-FR5) are implemented and tested
- [ ] treemd displays markdown files correctly (no red background, no warnings)
- [ ] `kitten icat` displays images via the viewer overlay
- [ ] All existing tests pass (WASM 448+, TS 1909+, Rust all)
- [ ] No regression in `emterm image` functionality
- [ ] Kitty animation frames are handled

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。

- [ ] FR4: Kitty image pipeline — Root cause of viewer not launching needs investigation (affects both markdown and image viewers)
- [ ] FR3: Red background root cause — Need to determine if it's a Kitty rendering pipeline issue or a cell size / response ordering issue

## Implementation Phases

### Phase 1: Synchronous Responses (Implemented)
**Goals:** Fix Kitty query timing and add XTWINOPS support
**Deliverables:**
- Kitty query sync response in WASM (apc_handler.rs)
- CSI 14t/16t/18t handlers (csi_device.rs, csi_dispatch.rs)
- set_cell_size_px integration (terminal_core.rs, index.ts)

### Phase 2: Pipeline Investigation & Fix
**Goals:** Fix red background in treemd and kitten icat image display
**Deliverables:**
- Root cause analysis of red background
- Root cause analysis of viewer not launching
- Fixes for Kitty image pipeline compatibility
- Cell size sync on buffer switch

### Phase 3: Animation Support
**Goals:** Support Kitty animation frames
**Deliverables:**
- Animation frame command handling (a=f, a=a)
- Integration with existing animation manager

## References

- Kitty Graphics Protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- ratatui-image: https://github.com/benjajaja/ratatui-image
- treemd: https://github.com/Epistates/treemd
- ratatui-image v10.0.6 CHANGELOG: WezTerm/Konsole blacklisted for Kitty/Sixel detection
