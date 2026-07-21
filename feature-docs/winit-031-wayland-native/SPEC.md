# Feature: winit 0.31 Migration and Wayland Native Startup

## Overview

Upgrade the winit dependency from 0.30.9 to 0.31.0-beta.2 and make the Linux
default backend Wayland native. This removes the X11-forcing logic in
`build_event_loop()` that causes stray `q` input when an Xwayland client is
closed with Ctrl+Q (X11 focus-revert synthetic key press path, confirmed in
`tmp/discussion-vlc-ctrl-q-stray-input.md`). File drag-and-drop (the SFTP
upload entry point) is migrated to winit 0.31's new drag API so it works on
native Wayland.

## Objectives

- Eliminate the stray-`q` bug at its root by running Wayland native by default
- Keep file D&D working on native Wayland via winit 0.31's implemented Wayland DnD
- Preserve X11 support as an explicit opt-in (`EMTERM_BACKEND=x11`) with a
  synthetic-key-press guard

## User Stories

### US1: Wayland native startup
As a Linux Wayland user, I want eMterm to run as a native Wayland client, so
that closing Xwayland apps (VLC etc.) with Ctrl+Q never leaks a `q` into my
terminal.

**Acceptance Criteria:**
- [ ] On a Wayland session, eMterm starts with the Wayland backend by default
- [ ] Closing an Xwayland Qt app (e.g. `QT_QPA_PLATFORM=xcb strawberry`) with
      Ctrl+Q does not input `q` into eMterm

### US2: File D&D on Wayland
As a Linux user, I want to drag files onto the eMterm window on native
Wayland, so that the SFTP upload entry point keeps working.

**Acceptance Criteria:**
- [ ] Dropping a file on the terminal window on native Wayland triggers the
      same handling as the 0.30 X11 `DroppedFile` path did

### US3: X11 opt-in with guard
As a Linux user who explicitly selects X11, I want synthetic key presses to
be ignored, so that the stray-`q` path is also closed on the X11 backend.

**Acceptance Criteria:**
- [ ] `EMTERM_BACKEND=x11` still selects the X11 backend when `DISPLAY` is set
- [ ] Synthetic key presses are not forwarded to the PTY

## Technical Requirements

### Functional Requirements

- **FR1:** Migrate the winit dependency to 0.31.0-beta.2. Follow the crate
  split (`winit-core` / `winit-wayland` / `winit-x11` etc.) and all API
  changes (event loop construction, window creation, keyboard / IME / drag
  events) so that the full GUI build compiles and existing behavior is
  preserved. Related crates that integrate with winit (egui / wgpu / wry glue
  layers inside this repo) are adapted as needed.
- **FR2:** Remove the X11-forcing branch in `build_event_loop()`
  (`src-tauri/src/main.rs`). Default (no `EMTERM_BACKEND` or unknown value):
  let winit auto-select, which prefers Wayland. `EMTERM_BACKEND=wayland`
  keeps forcing Wayland; `EMTERM_BACKEND=x11` forces X11 when `DISPLAY` is
  set. Update the stale comment describing the forcing rationale.
- **FR3:** Migrate file drag-and-drop from the removed
  `DroppedFile` / `HoveredFile` / `HoveredFileCancelled` events to winit
  0.31's `DragEntered` / `DragMoved` / `DragDropped` / `DragLeft` events
  (paths now arrive as a list per drag session). Connect dropped paths to the
  existing SFTP upload entry point with unchanged downstream behavior. A drop
  without file paths is ignored.
- **FR4:** Ignore synthetic key presses: `WindowEvent::KeyboardInput` events
  with `is_synthetic == true` must not reach key handling (PTY write /
  keybinding dispatch) in `window_host.rs`. This guards the X11 backend's
  FocusIn synthetic-press path; Wayland behavior is unchanged.

### Non-Functional Requirements

- **NFR1 - Windows compatibility:** The Windows target keeps compiling
  (`cargo xwin check` for `x86_64-pc-windows-msvc`).
- **NFR2 - CLI build compatibility:** The CLI-only build
  (`--no-default-features`) keeps compiling; winit stays behind the `gui`
  feature gate.
- **NFR3 - Maintainability:** Pin the beta version in `Cargo.toml` with a
  comment stating it is a beta and is to be bumped when 0.31 stable lands.

## Implementation Approach

### Architecture

Current backend selection (`src-tauri/src/main.rs` `build_event_loop()`):

```
EMTERM_BACKEND=wayland → no forcing (Wayland)
EMTERM_BACKEND=x11     → force X11 if DISPLAY set
auto                   → force X11 if WAYLAND_DISPLAY && DISPLAY   ← removed
```

New backend selection:

```
EMTERM_BACKEND=wayland → force Wayland
EMTERM_BACKEND=x11     → force X11 if DISPLAY set
auto (default)         → winit auto-select (Wayland preferred)
```

Affected areas (to be confirmed during planning):

- `src-tauri/Cargo.toml` — winit dependency (crate split), version pins for
  glue crates if required
- `src-tauri/src/main.rs` — `build_event_loop()`, event loop / app handler
  API changes
- `src-tauri/src/window_host.rs` — keyboard input (`is_synthetic` guard),
  D&D events, IME events, any changed `WindowEvent` variants
- Any other GUI modules using winit types (`app`, `callbacks`, `render`,
  `ui`, `tabs`, …)

### Dependencies

**External Dependencies:**
- winit 0.31.0-beta.2 (pinned; beta — bump to stable when released)
- Existing: wgpu, egui, wry, swash — versions adjusted only if the winit
  bump requires it

## Test Scenarios

### Unit Tests

- [ ] Backend selection logic: `EMTERM_BACKEND` parsing yields the expected
      force-wayland / force-x11 / auto decision (extract the decision into a
      testable pure function)
- [ ] Synthetic key press guard: a synthetic press does not produce PTY
      bytes / key handling; a real press does
- [ ] D&D path-list handling: a dropped path list maps to the existing
      upload entry point; an empty list is ignored

### Integration Tests

- [ ] Existing Rust test suite passes (`cargo test --lib`)
- [ ] CLI-only feature check passes (`cargo check --no-default-features`)
- [ ] Windows cross-check passes (`cargo xwin check`)

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario 1 (Wayland native): start eMterm on a Wayland session,
      verify keyboard input / IME / rendering / child WebView windows work
- [ ] Manual scenario 2 (stray-q): run `QT_QPA_PLATFORM=xcb` Qt app, close
      with Ctrl+Q while Claude Code is busy in eMterm — no `q` appears
- [ ] Manual scenario 3 (D&D): drag a file onto the terminal on native
      Wayland — SFTP upload entry point receives it
- [ ] Manual scenario 4 (X11 opt-in): `EMTERM_BACKEND=x11` starts on X11

### Edge Cases

- [ ] Drop without file paths (e.g. text drag) is ignored without error
- [ ] `DISPLAY`-only environment (pure X11 session): eMterm still starts
- [ ] Unknown `EMTERM_BACKEND` value falls back to auto selection

## Security Considerations

- **Input Validation:** Dropped path lists come from the compositor; they are
  passed to the existing upload entry point which already handles arbitrary
  paths. No new trust boundary is introduced.

## Error Handling

- Event loop construction failure keeps the existing
  `expect("emterm: failed to create winit event loop")` behavior.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All automated test scenarios pass
- [ ] Manual scenarios 1–4 verified on the user's machine
- [ ] Windows cross-check and CLI-only check pass
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。

- None

## References

- Investigation report: `tmp/discussion-vlc-ctrl-q-stray-input.md`
- winit 0.31 Wayland DnD: `winit-wayland/src/dnd.rs` (upstream)
- Current forcing logic: `src-tauri/src/main.rs` `build_event_loop()`
