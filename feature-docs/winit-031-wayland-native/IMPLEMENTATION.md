# Implementation Plan: winit 0.31 Migration and Wayland Native Startup

## Overview

Upgrade winit from 0.30.9 to 0.31.0-beta.2, make Wayland the default backend
on Linux (removing the X11-forcing branch), migrate file D&D to the new drag
event set, and guard key handling against synthetic key presses.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate)
- **Key libraries**: winit 0.31.0-beta.2 (pinned exact beta; bump when 0.31
  stable lands). winit is dual-licensed Apache-2.0 — compatible with this
  project's MIT license. The 0.31 crate split (`winit-core` / `winit-wayland`
  / `winit-x11` etc.) may add these subcrates as direct dependencies; they
  share the winit project's Apache-2.0 licensing.
- Existing integrations kept at current versions unless the winit bump forces
  a change: wgpu 22, egui 0.29 + egui-wgpu 0.29 (custom in-repo winit→egui
  bridge; egui-winit is NOT used), wry 0.53 (Linux child WebViews are
  GTK-hosted and independent of winit; Windows child WebViews are
  winit-hosted via `webview_host/windows.rs`), raw-window-handle 0.6.

## Layer Structure

Unchanged. The migration stays inside the existing GUI layer
(`#[cfg(feature = "gui")]` modules); the CLI-only build must remain free of
winit types.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Backend selection decision | Decide the Linux event-loop backend from environment | Pure function: inputs are the `EMTERM_BACKEND` value (string, may be empty/unknown) and the presence flags of Wayland (`WAYLAND_DISPLAY` / `WAYLAND_SOCKET` non-empty) and X11 (`DISPLAY` non-empty); output is one of Auto / ForceWayland / ForceX11. `wayland` → ForceWayland; `x11` with X11 present → ForceX11; anything else → Auto (winit auto-select, Wayland preferred). Unit-testable without an event loop. | task0001 |
| Synthetic key press gate | Keep synthetic keyboard events out of key handling | Precondition: a `KeyboardInput` window event carrying the winit `is_synthetic` flag. Postcondition: when the flag is true, the event produces no PTY write and no keybinding dispatch; when false, behavior is unchanged. Placed at the top of the keyboard-input path in `window_host.rs`, before any state mutation. | task0002 (task0001 preserves the field's availability during migration) |

## Conventions

- Follow existing module conventions; no new modules unless the winit crate
  split requires re-exports.
- Comments referencing the removed X11-forcing rationale must be updated, not
  left stale (main.rs doc comment, sftp/ui.rs one-file-at-a-time note).

## Cross-task Design Decisions

### D1: FR2 (backend default) lives in task0001, not a separate task

`build_event_loop()` is rewritten wholesale for the 0.31 event-loop API; a
separate task changing the same function guarantees a merge conflict with no
independent value. task0001 implements the new default semantics directly
against the Backend selection decision contract above.

### D2: task0002 is implementable against winit 0.30 or 0.31

The `is_synthetic` flag exists in both versions. task0002 touches only the
keyboard-input arm of `window_host.rs` and must not depend on any 0.31-only
API, so the two tasks can proceed fully in parallel; the merge conflict in
`window_host.rs` (if any) is localized to the keyboard arm and resolved by
the standard parent-side-adoption protocol.

### D3: D&D migration maps a path set to the existing single-path entry point

winit 0.31 delivers dropped paths as a list per drag session
(`DragDropped`), replacing the one-`DroppedFile`-event-at-a-time model that
`sftp/ui.rs` currently batches manually. The migration feeds each path of
the dropped list into the existing SFTP upload entry point in order,
preserving downstream behavior; the manual batching workaround for "no
completion signal" is removed or simplified accordingly. Empty path list →
no action.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| winit 0.31 API churn is wider than the known list (Window as trait object, event-loop proxy / user-event model changes, ApplicationHandler signature changes, crate split re-exports) | High | High | task0001 treats "compiles + full test suite green on all three build variants" as the bar; unknown API moves are resolved against upstream 0.31 docs/changelog during implementation |
| wgpu 22 / egui 0.29 surface integration breaks against 0.31 window handles | Medium | High | raw-window-handle stays at 0.6 on both sides; if an incompatibility surfaces, adapt the in-repo bridge (no version bump of wgpu/egui inside this feature unless compilation is impossible otherwise) |
| beta API drift before 0.31 stable | Medium | Low | exact-version pin + Cargo.toml comment (NFR3) |
| Windows backend (`webview_host/windows.rs`, ConPTY key encoding paths) regressions | Medium | Medium | `cargo xwin check` in every task cycle and in verify; Windows behavior changes are out of scope beyond compiling |

## Open Questions

- [ ] None
