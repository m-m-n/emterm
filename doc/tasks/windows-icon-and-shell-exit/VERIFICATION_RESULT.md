# Verification Result: Windows Application Icon and Shell-Exit Tab Close

**Verification date**: 2026-06-22
**Verifier**: sdd.6-verify (Linux host; no Windows target available)
**VERIFICATION.md**: `doc/tasks/windows-icon-and-shell-exit/VERIFICATION.md`
**SPEC.md**: `doc/tasks/windows-icon-and-shell-exit/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/windows-icon-and-shell-exit/IMPLEMENTATION.md`

> Notes on scope:
> - Build / unit-test / format / static-analysis / dead-code checks were validated in **sdd.5-check** and are intentionally not re-run here.
> - This sandbox is a Linux host with `cargo-xwin` (type-level Windows checks only). Real Windows runtime behavior (icon rendering, ConPTY watcher latency) is **Pending Manual on a Windows host**.

---

## Status Legend

| Mark | Meaning |
|------|---------|
| Verified | Implementation present and confirmed by code inspection and/or sdd.5 automated checks |
| Partial | Implementation present; some aspects only verifiable on a Windows host |
| Pending Manual | Cannot be verified without a Windows host; requires user execution |
| Failed | Implementation missing or incorrect |

---

## Summary Table

| Category | Count | Verified | Partial | Pending Manual | Failed |
|----------|------:|---------:|--------:|---------------:|-------:|
| File structure | 7 | 7 | 0 | 0 | 0 |
| FR (Functional Requirements) | 12 | 8 | 4 | 0 | 0 |
| NFR (Non-Functional Requirements) | 7 | 4 | 1 | 2 | 0 |
| Test scenarios TS-1 .. TS-7 | 7 | 3 | 2 | 2 | 0 |
| Manual scenarios M-1 .. M-10 | 10 | 0 | 1 (M-1) | 9 | 0 |
| **Total** | **43** | **22** | **8** | **13** | **0** |

**Overall verdict**: All code-level deliverables are in place. No regressions introduced on Linux (sdd.5 PASS, 1911 tests). Windows-specific behavior (icon rendering + ConPTY watcher exit-detection latency) requires manual verification on a Windows host.

---

## File Structure Verification

| Path | Expectation | Actual | Verdict |
|------|-------------|--------|---------|
| `src-tauri/src/window_icon.rs` | exists with `app_icon()` + private `decode_icon()` helper | exists, 92 lines, both functions present | Verified |
| `src-tauri/build.rs` | `embed_windows_icon_resource()` function, gated on `CARGO_CFG_TARGET_OS == "windows"` | line 32 gate + line 67 `embed_windows_icon_resource()` calling `winresource::WindowsResource::new().set_icon("icons/icon.ico").compile()` (build.rs:71-74) | Verified |
| `src-tauri/Cargo.toml` `[build-dependencies]` | contains `winresource` | line 183: `winresource = "0.1"` under `[build-dependencies]` | Verified |
| `src-tauri/src/lib.rs` | `pub mod window_icon;` under `#[cfg(feature = "gui")]` | lib.rs:124-125: `#[cfg(feature = "gui")] pub mod window_icon;` | Verified |
| `src-tauri/src/window_host.rs` (WindowHost::new) | `.with_window_icon(crate::window_icon::app_icon())` | window_host.rs:333 inside `WindowHost::new` chain | Verified |
| `src-tauri/src/webview_host/windows.rs` (WebViewApp::resumed) | same call | windows.rs:84 inside `WebViewApp::resumed`'s `WindowAttributes::default()` chain | Verified |
| `src-tauri/src/pty/mod.rs` Windows additions | `#[cfg(windows)]`-gated `Weak` master + `child_killer` + `watcher_join` + `watcher_loop` | pty/mod.rs:16-17 `Weak` import; :83-84 `master: Weak<...>`; :97-98 `child_killer: Box<dyn ChildKiller + Send + Sync>`; :107-108 `watcher_join: Option<JoinHandle<()>>`; :556-572 `watcher_loop()` | Verified |

---

## Functional Requirements (FR1 .. FR12)

| ID | Requirement | Evidence | Verdict |
|----|-------------|----------|---------|
| FR1 | Embed `icon.ico` into PE resource on Windows target only | build.rs:32 (target-OS gate) + build.rs:67-79 (`winresource::WindowsResource::new().set_icon("icons/icon.ico").compile()`); Cargo.toml:183 (`winresource = "0.1"` in `[build-dependencies]`); Linux build leaves `winresource` uninvoked. sdd.5 confirmed `cargo xwin check` PASS. Whether the resulting `.exe`'s Explorer/taskbar icon renders is observable only on Windows. | Partial (Windows visual = Pending Manual M-2..M-4) |
| FR2 | winit main window icon via `WindowAttributes::with_window_icon(Some(icon))` | window_host.rs:333 inside `WindowHost::new` adds `.with_window_icon(crate::window_icon::app_icon())` to the attribute chain | Verified (code); Windows title-bar rendering Pending Manual M-5 |
| FR3 | wry child WebView windows on Windows apply the same call | webview_host/windows.rs:84 inside `WebViewApp::resumed` adds `.with_window_icon(crate::window_icon::app_icon())`. `webview_host/linux.rs` intentionally untouched (matches SPEC FR3) | Verified (code); Windows title-bar rendering Pending Manual M-6/M-7 |
| FR4 | Shared `crate::window_icon::app_icon()` helper used by both call sites | window_icon.rs:36 `pub fn app_icon() -> Option<Icon>`; used by both window_host.rs:333 and webview_host/windows.rs:84 | Verified |
| FR5 | Fail-soft on decode error: warn-log + `None` | window_icon.rs:46-61 `decode_icon` returns `None` and `log::warn!`s on both `image::load_from_memory` failure and `Icon::from_rgba` failure; unit test `decode_icon_returns_none_on_broken_input` proves no-panic behavior on garbage input | Verified |
| FR6 | Windows watcher thread blocks on `Child::wait()` and drops master Arc on return | pty/mod.rs:323-328 spawns `native-poc-pty-watcher`; :557-572 `watcher_loop` calls `child.wait()` then `drop(master)`. Strong Arc held only by watcher; struct holds only `Weak` | Verified (code); end-to-end Windows latency Pending Manual M-8 |
| FR7 | Exactly one `PtyEvent::Exited` per session | Structurally guaranteed: `watcher_loop` never sends events (pty/mod.rs:557-572); only `reader_loop` sends `PtyEvent::Exited { Eof }` and breaks immediately (pty/mod.rs:593-596). X-button kill + natural exit converge on the same single send | Verified (code); concurrent X-button race Pending Manual M-10 |
| FR8 | Non-Windows: existing 4-step Drop kept bit-identical. Windows: documented 6-step Drop | pty/mod.rs:426-479 non-Windows 4-step Drop (gated `#[cfg(not(windows))]`) unchanged; :481-540 Windows 6-step (`#[cfg(windows)]`): (1) `child_killer.kill()`, (2) replace `input_tx` with disconnected dummy, (3) no-op for master, (4) `watcher_join`, (5) `reader_join`, (6) `writer_join`. Both arms documented with comment blocks (lines 427-434 and 481-504) | Verified |
| FR9 | No deadlock between watcher and Drop; struct retains `ChildKiller` + `Weak` only | pty/mod.rs:317 `child_killer = child.clone_killer()` BEFORE moving child into watcher; :321-326 watcher receives `child` + strong `master_arc`; struct stores `child_killer` + `master_weak` (no shared Mutex<Child> between watcher and struct) | Verified |
| FR10 | Non-Windows parity (no watcher, no struct-shape change) | pty/mod.rs:76 `#[cfg(not(windows))] master: Option<Arc<...>>`; :89 `#[cfg(not(windows))] child: Arc<Mutex<...>>`; :298-309 non-Windows spawn arm matches the pre-feature layout; :392-406 non-Windows `resize` unchanged. Unit test `drop_returns_quickly_on_linux` (pty/mod.rs:850-875) is a regression guard. sdd.5: Linux `--lib` PASS 1911 tests | Verified |
| FR11 | Watcher `JoinHandle` joined in Drop step 4 before reader/writer joins | pty/mod.rs:107-108 `watcher_join: Option<JoinHandle<()>>` field; :336 stored in struct; :524-526 Drop step 4 joins watcher; comment block at :496-501 documents why step 4 must precede step 5 (watcher's master-drop is what unblocks reader's ReadFile) | Verified |
| FR12 | `Child::wait()` error → warn-log + watcher still drops master | pty/mod.rs:561-566 `match child.wait()` handles `Err(e)` with `log::warn!`; :571 `drop(master)` runs unconditionally afterwards (outside the match) so ClosePseudoConsole fires on both Ok and Err paths | Verified |

**FR Summary**: 8 Verified, 4 Partial (FR1/FR2/FR3/FR6 — code complete; visual / latency confirmation requires Windows host).

---

## Non-Functional Requirements (NFR1 .. NFR7)

| ID | Requirement | Evidence | Verdict |
|----|-------------|----------|---------|
| NFR1 | Build-time isolation: `winresource` runs only when target-OS=windows | build.rs:32 `if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")` gate; Cargo.toml:183 places `winresource` under `[build-dependencies]` (never a runtime dep). sdd.5: Linux host `cargo check` PASS with no `winresource` resolution at runtime | Verified |
| NFR2 | Bundled asset ≤ ~50 KB increase | window_icon.rs:27 `include_bytes!("../icons/128x128.png")` (single existing asset, no new files). 128x128 PNG well under 50 KB | Verified |
| NFR3 | Startup decode < 10 ms | Code is a single PNG decode + `Icon::from_rgba` (window_icon.rs:46-62). No micro-benchmark instrumented; spec accepts informal estimate | Pending Manual (informal observation during M-1/M-5) |
| NFR4 | Watcher CPU negligible | Watcher idles in blocking `Child::wait()` (pty/mod.rs:561); no polling | Verified (by design / code) |
| NFR5 | Shell-exit detection < 500 ms | Architectural fit: `Child::wait()` returns immediately on kernel exit signal → `drop(master)` → ConPTY ClosePseudoConsole → reader EOF → `PtyEvent::Exited`. Real wall-clock latency only measurable on Windows | Pending Manual M-8 |
| NFR6 | Documentation comments | Doc comments confirmed: window_icon.rs:1-17, 21-27, 29-35, 40-44; pty/mod.rs:55-66 (ownership invariant), :280-296 (platform split rationale), :383-391 (Weak upgrade rationale), :427-504 (both Drop arms with step-by-step commentary), :543-555 (watcher_loop doc) | Verified |
| NFR7 | Linux/macOS bit-identical PTY semantics | pty/mod.rs:298-309 non-Windows spawn arm + :426-479 non-Windows Drop are gated `#[cfg(not(windows))]` and structurally unchanged from pre-feature code. sdd.5: 1911 `cargo test --lib` PASS (Linux) including new `drop_returns_quickly_on_linux` regression guard | Verified |

**NFR Summary**: 4 Verified, 1 partially manual (NFR3 informal), 2 Pending Manual (NFR5 latency requires Windows).

---

## Test Scenarios (TS-1 .. TS-7)

| ID | Scenario | Test artifact | sdd.5 Result | Verdict |
|----|----------|---------------|--------------|---------|
| TS-1 | `app_icon()` decodes bundled PNG | `window_icon::tests::app_icon_decodes_bundled_asset` (window_icon.rs:71-78) | PASS (part of `--lib` 1911 PASS) | Verified |
| TS-2 | Broken bytes → `None` + warn | `window_icon::tests::decode_icon_returns_none_on_broken_input` (window_icon.rs:83-90) | PASS | Verified |
| TS-3 | Linux Drop returns quickly (4-step regression guard) | `pty::tests::drop_returns_quickly_on_linux` (pty/mod.rs:850-875; gated `#[cfg(all(unix, not(target_os = "macos")))]`) | PASS | Verified |
| TS-4 | Windows Drop joins watcher cleanly without shell-exit signal | No runtime test; type-level only via `cargo xwin check` | xwin check PASS (Finished dev profile in 36.87s per VERIFICATION.md) | Partial (type-level; runtime requires Windows) |
| TS-5 | Short-lived child on Windows produces `PtyEvent::Exited` within 500 ms | Not implemented as automated test (SPEC explicitly allows deferral to manual) | n/a | Pending Manual M-8 (`exit` inside PowerShell) |
| TS-6 | Concurrent X-button + natural exit emits exactly one `Exited` | Structurally guaranteed by `reader_loop`'s break-after-send (pty/mod.rs:593-596) and watcher never sending events. No runtime test | Code review only | Partial (code-level) → Pending Manual M-10 |
| TS-7 | Linux shell exit still produces `Exited` via kernel-EOF | Existing reader_loop path untouched on `#[cfg(not(windows))]`; TS-3 regression guard backs this up structurally | sdd.5 PASS | Verified via TS-3 + code review; full end-to-end Pending Manual M-1 |

**TS Summary**: 3 Verified (TS-1, TS-2, TS-3), 2 Partial (TS-4 type-level, TS-7 via proxy), 2 Pending Manual (TS-5, TS-6).

---

## Manual Testing Scenarios (M-1 .. M-10)

> All Windows checks require a Windows host running the binary produced by `make win-build`. Sandbox is Linux-only.

| ID | Scenario | Verdict | Notes |
|----|----------|---------|-------|
| M-1 | Linux: `make build` + run; tabs open and `exit` closes them | Partial — code path unchanged on Linux (pty/mod.rs:298-309, :426-479 are `#[cfg(not(windows))]` and structurally identical). sdd.5 `cargo test --lib` confirms `drop_returns_quickly_on_linux` PASS. End-to-end `make build` + GUI launch not executed in this verification | User should run `make build` once and sanity-check |
| M-2 | Windows: Explorer icon on `emterm.exe` (SC-1) | Pending Manual | Requires Windows host + `make win-build` artifact |
| M-3 | Windows: taskbar icon when pinned (SC-1) | Pending Manual | Requires Windows host |
| M-4 | Windows: Alt+Tab icon (SC-1) | Pending Manual | Requires Windows host |
| M-5 | Windows: main window title-bar icon (SC-2) | Pending Manual | Verifies FR2 visually |
| M-6 | Windows: `emterm markdown <file>` child window title-bar icon (SC-3) | Pending Manual | Verifies FR3 visually (Markdown viewer) |
| M-7 | Windows: Settings panel title-bar icon (SC-3) | Pending Manual | Verifies FR3 visually (settings webview) |
| M-8 | Windows: PowerShell `exit` closes tab within 500 ms (SC-4) | Pending Manual | Verifies FR6 + NFR5 end-to-end |
| M-9 | Windows: kill shell from Task Manager → tab closes within 500 ms (SC-5) | Pending Manual | Verifies FR6 against external kill |
| M-10 | Windows: X-button still closes cleanly without "応答なし" (SC-6) | Pending Manual | Verifies FR7 + FR8 (6-step Drop) |

---

## Build / Test / Static Analysis (delegated to sdd.5-check)

Per VERIFICATION.md "Build Verification" / "Test Verification" tables, sdd.5-check already validated:

- Linux host `cargo check` — exit 0, clean
- Linux release `cargo build --release` — deferred to user run (build location: `src-tauri/target-host/release/emterm`)
- `cargo check --no-default-features` — exit 0, clean (CLI-only build unaffected)
- `cargo xwin check --target x86_64-pc-windows-msvc` — exit 0, clean (Finished dev profile in 36.87s)
- `cargo xwin build --release` — deferred to user run
- `cargo test --lib` — **1911 passed, 0 failed, 3 ignored** (`--test-threads=1` per `project_test_execution_notes`)

These are not re-run by sdd.6.

---

## Open Items / Recommended Next Actions

### What is fully done (no action required)
- All code-level FR (FR1 .. FR12) are implemented and present.
- All code-level NFR (NFR1 / NFR2 / NFR4 / NFR6 / NFR7) are satisfied.
- All testable scenarios (TS-1 / TS-2 / TS-3) are green via sdd.5.
- No Linux regressions detected.

### Pending: Windows host manual verification

To close US1, US2, US3, US4 acceptance criteria, run on a Windows host:

1. `make win-build` (or `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin build --release --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`).
2. Copy `src-tauri/target-win/x86_64-pc-windows-msvc/release/emterm.exe` (plus required runtime files) to a Windows host.
3. Execute the M-2 .. M-10 checklist below.

### Pending: Optional Linux smoke check (M-1)

User runs `make build` and `src-tauri/target-host/release/emterm` once to confirm Linux GUI launches normally (the same change-path on Linux is covered by sdd.5's automated test, so this is a low-risk visual confirmation).

---

## Windows Manual Verification Checklist (for the user)

Run these on a Windows host after copying the `make win-build` artifact:

- [ ] **M-2** — Open Windows Explorer, locate `emterm.exe`, confirm the eMterm icon is shown (not the generic Win32 icon).
- [ ] **M-3** — Launch `emterm.exe`, right-click its taskbar item, choose "Pin to taskbar", and confirm the pinned icon is the eMterm icon.
- [ ] **M-4** — With eMterm window open, press Alt+Tab and confirm the eMterm thumbnail uses the eMterm icon.
- [ ] **M-5** — Inspect the main eMterm window's title-bar (top-left) and confirm the eMterm icon is rendered.
- [ ] **M-6** — Run `emterm markdown <some-file.md>` from inside an eMterm tab; confirm the spawned Markdown-viewer child window's title-bar shows the eMterm icon.
- [ ] **M-7** — Open the Settings panel from eMterm and confirm the settings window's title-bar shows the eMterm icon.
- [ ] **M-8** — Open a PowerShell tab in eMterm, type `exit` and press Enter. Confirm the tab closes within roughly 500 ms (no perceptible hang).
- [ ] **M-9** — Open a PowerShell tab. From Task Manager, end the PowerShell process for that tab. Confirm the tab closes within roughly 500 ms.
- [ ] **M-10** — Open a PowerShell tab. Click the tab's X (close) button. Confirm the tab closes cleanly with no "応答なし" / unresponsive dialog (regression check for the 6-step Windows Drop sequence).

Recording results: once M-2 .. M-10 are confirmed, this VERIFICATION_RESULT.md can be updated to mark them Verified.
