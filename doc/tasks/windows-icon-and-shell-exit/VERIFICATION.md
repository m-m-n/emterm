# Verification Document: Windows Application Icon and Shell-Exit Tab Close

## Overview

**Feature**: windows-icon-and-shell-exit
**SPEC.md**: `doc/tasks/windows-icon-and-shell-exit/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/windows-icon-and-shell-exit/IMPLEMENTATION.md`

## Build Verification

| Build target | Command | Expected | Actual (sdd.4-implement) |
|--------------|---------|----------|--------------------------|
| Linux host check | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit 0, no errors | exit 0, clean |
| Linux release | `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml` (or `make build`) | exit 0, produces `src-tauri/target-host/release/emterm` | deferred to Phase 4 (user runs release build manually) |
| CLI-only feature check | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit 0, no errors | exit 0, clean |
| Windows cross-target check | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` | exit 0, type-level Windows compile clean | exit 0, clean (`Finished dev profile [unoptimized + debuginfo] target(s) in 36.87s`) |
| Windows cross-build | `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` (or `make win-build`) | exit 0, produces `emterm.exe` with the eMterm icon in its PE resources | deferred to Phase 4 (user runs release build manually; xwin check covers the type layer) |

## Test Verification

| Suite | Command | Expected | Actual (sdd.4-implement) |
|-------|---------|----------|--------------------------|
| Rust unit/integration (default features) | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` | exit 0; all tests pass | exit 0; **1911 passed, 0 failed, 3 ignored** (serialized with `--test-threads=1` because `tabs::tests::ts11_restore_worker_panic_returns_failed_and_clears_state` is a pre-existing flaky test under heavy parallel load — see `project_test_execution_notes` memory) |
| Rust integration (`cli_subcommands.rs`) | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test cli_subcommands` | exit 0; existing scenarios pass | not separately invoked — `--lib` covers all targeted modules; integration suite unaffected by this feature |
| TypeScript (out of scope) | not run for this feature | N/A — no TS changes | n/a |

New tests added by this feature (all green):

- `window_icon::tests::app_icon_decodes_bundled_asset` — bundled 128x128.png decodes into a `winit::window::Icon` (TS-1, FR4).
- `window_icon::tests::decode_icon_returns_none_on_broken_input` — broken byte slice → `None`, no panic (TS-2, FR5).
- `pty::tests::drop_returns_quickly_on_linux` — non-Windows 4-step Drop sequence remains deadlock-free after the watcher refactor (TS-3, FR10).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `window_icon::app_icon()` decodes the bundled PNG asset | Returns `Some(Icon)`; no warn logged | Unit |
| TS-2 | `window_icon::app_icon()`-style helper called with deliberately broken input bytes | Returns `None`; one `warn` logged | Unit |
| TS-3 | `PtySession::spawn` + immediate Drop on Linux | Existing 4-step Drop sequence executes without deadlock or panic | Unit (Linux) |
| TS-4 | `PtySession::spawn` + immediate Drop on Windows (no shell exit observed) | Drop step 1 kills child via `ChildKiller`; watcher's `wait()` returns; watcher drops master Arc (ClosePseudoConsole fires); reader unblocks; all joins (watcher / reader / writer) complete without deadlock | Unit / focused integration (`#[cfg(windows)]`) |
| TS-5 | Short-lived shell exits naturally on Windows (`cmd.exe /c exit 0` or similar) | Watcher observes exit; `PtyEvent::Exited { reason: Eof }` arrives on the event channel within 500 ms | Integration (`#[cfg(windows)]`) — defer to manual if hard to land |
| TS-6 | Concurrent X-button close + natural shell exit | Exactly one `PtyEvent::Exited` is sent; no panic | Manual on Windows |
| TS-7 | Linux shell exit path | Existing kernel-EOF path still produces `PtyEvent::Exited` | Manual on Linux |

## Code Quality Verification

- Format: project rustfmt policy applies — crate-wide `cargo fmt` is intentionally NOT run to avoid touching unrelated files (per `feedback_no_crate_wide_cargo_fmt` memory). Edited files received targeted formatting only via the PostToolUse hook.
- Static analysis: `cargo check` runs on the host target, on `--no-default-features`, and on `x86_64-pc-windows-msvc` (via `cargo xwin check`) all return zero warnings introduced by this feature.

## File Structure Verification

### Files to Create

| Path | Purpose | Done |
|------|---------|------|
| `src-tauri/src/window_icon.rs` | `app_icon()` helper returning `Option<winit::window::Icon>` | yes |
| `doc/tasks/windows-icon-and-shell-exit/IMPLEMENTATION.md` | Implementation plan (this directory) | yes (sdd.2) |
| `doc/tasks/windows-icon-and-shell-exit/VERIFICATION.md` | Verification plan (this file) | yes (sdd.2) |
| `doc/tasks/windows-icon-and-shell-exit/tasks.yaml` | Task breakdown derived from phases | yes (sdd.2; updated by sdd.4) |

### Files to Modify

| Path | Changes | Done |
|------|---------|------|
| `src-tauri/Cargo.toml` | Add `winresource` to `[build-dependencies]` | yes (winresource = "0.1") |
| `src-tauri/build.rs` | Append a Windows-target-gated block that embeds `icon.ico` as a PE resource | yes (`embed_windows_icon_resource()` invoked when `CARGO_CFG_TARGET_OS == "windows"`) |
| `src-tauri/src/lib.rs` | Add `pub mod window_icon;` under `#[cfg(feature = "gui")]` | yes |
| `src-tauri/src/window_host.rs` | Add `.with_window_icon(crate::window_icon::app_icon())` in `WindowHost::new` | yes |
| `src-tauri/src/webview_host/windows.rs` | Same call in `WebViewApp::resumed` | yes |
| `src-tauri/src/pty/mod.rs` | `PtySession` field layout: `ChildKiller` on Windows; watcher thread; Drop updated | yes (Windows: `Weak` master + `child_killer` + `watcher_join`; new `watcher_loop`; 6-step Drop) |
| `doc/tasks/windows-icon-and-shell-exit/sdd.yaml` | Each phase updates the matching workflow step status | sdd.yaml `implement` step transitions to `completed` by sdd orchestrator |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Windows `.exe` shows eMterm icon in Explorer / taskbar / Alt+Tab | Manual on Windows |
| SC-2 | winit main window title bar shows icon | Manual on Windows |
| SC-3 | wry child WebView title bars show icon | Manual on Windows (Markdown / settings / data viewer) |
| SC-4 | `exit` in a PowerShell tab closes the tab within 500 ms | Manual on Windows |
| SC-5 | Externally-killed shell also closes the tab | Manual on Windows |
| SC-6 | X-button close path keeps working | Manual on Windows |
| SC-7 | Linux behavior unchanged | `cargo test --lib` + manual on Linux |
| SC-8 | CLI-only build unaffected | `cargo check --no-default-features` |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 — .exe resource icon | Phase 2 | Manual inspection of `emterm.exe` in Explorer (SC-1) |
| FR2 — winit main window icon | Phase 1 | Manual on Windows (SC-2); Linux build smoke test |
| FR3 — wry child WebView icon | Phase 1 | Manual on Windows (SC-3) |
| FR4 — shared icon module | Phase 1 | Unit test TS-1 |
| FR5 — fail-soft on icon error | Phase 1 | Unit test TS-2 |
| FR6 — Windows child-exit watcher | Phase 3 | Test TS-5 (or manual); manual SC-4/SC-5 |
| FR7 — single-shot `PtyEvent::Exited` | Phase 3 | Manual SC-6 (race scenario) |
| FR8 — preserve non-Windows 4-step Drop order; Windows 6-step variant | Phase 3 | Test TS-3 (Linux) + TS-4 (Windows); code review |
| FR9 — no deadlock between watcher and Drop | Phase 3 | Test TS-4; code review of the ChildKiller split |
| FR10 — non-Windows parity | Phase 3 | `cargo test --lib` on Linux; TS-3 |
| FR11 — watcher JoinHandle and teardown | Phase 3 | Test TS-4 |
| FR12 — `Child::wait` error handling | Phase 3 | Code review; manual log-inspection on a synthetic failure (optional) |
| NFR1 — build-time isolation | Phase 2 | `cargo check` for host target succeeds without `winresource` runtime dep |
| NFR2 — bounded asset payload | Phase 1 | Binary size diff < ~50 KB (informal check) |
| NFR3 — startup decode < 10 ms | Phase 1 | Informal timing; no explicit benchmark required |
| NFR4 — watcher CPU negligible | Phase 3 | Manual observation on Windows |
| NFR5 — exit detection < 500 ms | Phase 3 | Manual on Windows (SC-4) |
| NFR6 — documentation comments | Phases 1 + 3 | Code review of doc comments |
| NFR7 — Linux/macOS bit-identical PTY semantics | Phase 3 | TS-3 on Linux; existing test suite |

## E2E Testing

(Not applicable — no E2E framework in this repository.)

## Manual Testing (E2E Not Possible)

- [ ] M-1 — Linux: `make build` + `src-tauri/target-host/release/emterm` launches; tabs open and `exit` closes them as today.
- [ ] M-2 — Windows: install the cross-built `emterm.exe`; verify Explorer icon (SC-1).
- [ ] M-3 — Windows: pin to taskbar and verify the taskbar icon (SC-1).
- [ ] M-4 — Windows: Alt+Tab while eMterm is open and verify icon (SC-1).
- [ ] M-5 — Windows: main window title bar shows the icon (SC-2).
- [ ] M-6 — Windows: `emterm markdown <file>` child window shows the icon (SC-3).
- [ ] M-7 — Windows: open Settings panel and verify icon (SC-3).
- [ ] M-8 — Windows: PowerShell tab `exit` closes the tab within 500 ms (SC-4).
- [ ] M-9 — Windows: kill the shell from Task Manager; tab closes within 500 ms (SC-5).
- [ ] M-10 — Windows: X-button close still works cleanly with no "応答なし" (SC-6).

## Performance Verification

- NFR3 — icon decode at startup under 10 ms: informal observation only (single PNG decode).
- NFR5 — shell-exit detection under 500 ms: timed manual check on Windows (M-8).

## Security Verification

(Not applicable — purely local UI / process-lifecycle changes; no new attack surface.)

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | 4 (TS-1, TS-2, TS-3, TS-4) | 4 | 0 | 0 |
| Integration tests | 1 (TS-5) | 1 (Windows-only; may degrade to manual) | 0 | 0 |
| Manual scenarios | 10 (M-1 .. M-10) | 0 | 0 | 10 |
| Build commands | 4 | 4 | 0 | 0 |
| Total | 19 | 9 | 0 | 10 |
