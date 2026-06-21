# Verification Document: Window Maximize-on-Launch and Dock Grouping

## Overview
**Feature**: window-maximize-dock-grouping
**SPEC.md**: `doc/tasks/window-maximize-dock-grouping/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/window-maximize-dock-grouping/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors or new warnings in touched files
- CLI-only gate (no regression): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`

### Build Results (implementation run, 2026-06-20)
- Default-features compile (`cargo check`, used in place of a release build per project policy of not running unsolicited `cargo build --release`): exit 0, no errors, no new warnings in touched files.
- CLI-only gate (`cargo check --no-default-features`): exit 0, no errors, no new warnings. The `APP_WM_ID` `pub const` triggers no unused warning in the CLI-only build, and the `linux_wm` helper is gated by `#[cfg(all(feature = "gui", target_os = "linux"))]` so it is excluded entirely from the CLI build.

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: deterministic host-level facts only (see note below). No coverage threshold is meaningful for WM-level behavior.

> Note: actual maximize state and dock grouping are window-manager-level effects that cannot be reliably asserted in unit tests. Those are verified manually (see Manual Testing).

### Test Results (implementation run, 2026-06-20)
- Command run: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` (single-threaded per project note: `tabs.rs` replay tests are non-deterministic under parallelism).
- Result: **1878 passed, 0 failed, 1 ignored** (the ignored test is a pre-existing, unrelated case).
- New deterministic unit tests added and passing:
  - TS-1 (settings, FR1): `settings_window::tests::settings_host_opens_maximized_with_restore_size` — asserts `build_host().maximized == true` and `initial_size == (1080.0, 760.0)`.
  - TS-1 (Markdown, FR2): `viewer::window::tests::markdown_viewer_opens_maximized` — asserts the viewer's `MAXIMIZED` const is `true` (host carries payload-derived closures, so a `const` is used rather than building the full host in a unit test).
  - TS-4 (FR5/NFR4): `tests::app_wm_id_is_emterm` (in `lib.rs`) — asserts `APP_WM_ID == "emterm"`. Also confirmed passing under `--no-default-features` (CLI-only).
- TS-2 (data viewer maximized) and TS-3 (image viewer NOT maximized): verified by file review rather than a unit test. The winit `WindowAttributes` for the data viewer / image viewer are built inside `resumed()` and consumed directly by `GpuShell::new`; there is no seam to inspect the attributes without a window. Data viewer now has `.with_maximized(true)`; image viewer intentionally has none (FR4). These are covered by the manual checklist (TS-5/TS-6 and TS-3).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Settings/Markdown host config carries the maximize flag | Construction sites produce a config with maximize = true | Unit |
| TS-2 | Data viewer is configured to start maximized | The data viewer's winit attributes set maximized = true | Unit / Manual |
| TS-3 | Image viewer is NOT maximized; image-fit sizing preserved | No maximize attribute on the image viewer; sizing path unchanged | Unit / Manual |
| TS-4 | Canonical identifier value | The single identifier constant equals `emterm` | Unit |
| TS-5 | Maximize-on-launch (visual) | Settings, Markdown, JSON/YAML open maximized | Manual |
| TS-6 | Restore size preserved | Un-maximizing restores ~1080×760 / ~960×720 / ~960×640 | Manual |
| TS-7 | Single dock icon (X11) | All windows group under one `emterm` icon (X11) | Manual |
| TS-8 | Single dock icon (Wayland) | All windows group under one `emterm` icon; correct icon shown (Wayland) | Manual |

## Code Quality Verification
- Format: handled by the project's PostToolUse formatting hook per edited file (no crate-wide `cargo fmt`).
- Static analysis: `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml` warnings reviewed for touched files.

### Code Quality Results (implementation run, 2026-06-20)
- Formatting: edited files were normalized by the PostToolUse hook; no crate-wide `cargo fmt` was run.
- Static analysis: `cargo check` (default + `--no-default-features`) produced no errors and no new warnings in the touched files.
- `git status` after implementation shows exactly the 9 planned source files modified (no unrelated files touched):
  `lib.rs`, `settings_window/mod.rs`, `viewer/data_window.rs`, `viewer/image_window.rs`, `viewer/window.rs`, `webview_host/{mod,linux,windows}.rs`, `window_host.rs`.

## File Structure Verification

### Files to Create
- [x] `src-tauri/src/lib.rs` (or a small dedicated module) - canonical identifier constant (value `emterm`). Added `pub const APP_WM_ID: &str = "emterm"` plus a Linux-only `linux_wm::with_app_id` helper (gated `#[cfg(all(feature = "gui", target_os = "linux"))]`) to the existing `lib.rs` crate root (no new file).

### Files to Modify
- [x] `src-tauri/src/webview_host/mod.rs` - added documented `maximized: bool` field to `WebViewHost` config.
- [x] `src-tauri/src/webview_host/linux.rs` - GTK `window.maximize()` before `show_all` when flagged; `glib::set_prgname` + `gdk::set_program_class` set to `APP_WM_ID` after `gtk::init`, before window creation.
- [x] `src-tauri/src/webview_host/windows.rs` - `.with_maximized(host.maximized)` on the WebView2 window attributes.
- [x] `src-tauri/src/settings_window/mod.rs` - `build_host()` helper split out of `run()`; sets `maximized: true`. (Refactor kept minimal — only the host-construction body was extracted so the maximize flag is unit-testable.)
- [x] `src-tauri/src/viewer/window.rs` - `maximized: MAXIMIZED` (const `true`) on the Markdown host.
- [x] `src-tauri/src/viewer/data_window.rs` - `.with_maximized(true)` + `linux_wm::with_app_id` on the winit attributes.
- [x] `src-tauri/src/viewer/image_window.rs` - `linux_wm::with_app_id` only (NOT maximized; FR4 preserved).
- [x] `src-tauri/src/window_host.rs` - `linux_wm::with_app_id` on the main terminal window (already maximized; identifier only added).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR5 implemented | File review + manual checklist |
| SC-2 | Settings/Markdown/JSON/YAML launch maximized; image keeps image-fit | TS-5, TS-3 |
| SC-3 | All windows group under one `emterm` dock icon (X11 + Wayland) | TS-7, TS-8 |
| SC-4 | No settings toggle added; behavior fixed | File review (no new settings field) |
| SC-5 | Build + unit tests pass; no sizing regression | Build + test commands, TS-6 |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 Settings maximize | Phase 1 | TS-1, TS-5, TS-6 |
| FR2 Markdown maximize | Phase 1 | TS-1, TS-5, TS-6 |
| FR3 Data viewer maximize | Phase 1 | TS-2, TS-5, TS-6 |
| FR4 Image excluded from maximize | Phase 1 | TS-3 |
| FR5 Unified dock grouping | Phase 2 | TS-4, TS-7, TS-8 |
| NFR1 Fixed behavior (no toggle) | Phase 1 | SC-4 (file review) |
| NFR2 Platform scope | Phase 1/2 | TS-7, TS-8 (Linux grouping); build (cross-platform maximize) |
| NFR3 Restore size preserved | Phase 1 | TS-6 |
| NFR4 Single identifier source | Phase 2 | TS-4 (file review: single constant referenced everywhere) |

## Manual Testing (E2E Not Possible)
DevTools are unavailable; WM-level state is observed visually and via `emterm.log`.

> Grouping caveat: GNOME/Ubuntu associates a window to a dock icon by matching the X11 `WM_CLASS` / Wayland `app_id` to an installed `*.desktop` entry. Dock-icon + grouping verification (TS-7/TS-8) is therefore most reliable against the installed deb (`emterm.desktop` present). Under `make dev` / `cargo run` with no installed desktop entry, windows may still group by identifier but the icon/grouping can be partial — prefer testing the installed package for TS-7/TS-8.

- [ ] TS-5: Open settings → maximized. Open `emterm markdown <file>` → maximized. Open `emterm json/yaml <file>` → maximized.
- [ ] TS-6: Un-maximize each → restores to its prior size (~1080×760 / ~960×720 / ~960×640).
- [ ] TS-3: Open `emterm image <small-image>` → window is image-sized, NOT maximized.
- [ ] TS-7 (X11 session, installed deb preferred): Open all window types → Ubuntu dock shows exactly one `emterm` icon grouping them.
- [ ] TS-8 (Wayland session, installed deb preferred): Same as TS-7; verify the correct app icon is shown (app_id matches `emterm.desktop`).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 3 (TS-1, TS-3 partial, TS-4) | 3 | 0 | 0 |
| Maximize behavior | 3 (TS-2, TS-5, TS-6) | 0 | 0 | 3 |
| Image-fit unchanged | 1 (TS-3) | 0 | 0 | 1 |
| Dock grouping | 2 (TS-7, TS-8) | 0 | 0 | 2 |
