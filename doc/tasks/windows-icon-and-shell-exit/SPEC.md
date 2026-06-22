# Feature: Windows Application Icon and Shell-Exit Tab Close

## Overview

On Windows, eMterm currently exhibits two unresolved issues reported in `tmp/issues-windows-mux-2026-06-22.md`:

1. The application has no icon attached to the `.exe` Windows resource, the winit main window, or the wry child WebView windows. The taskbar, Alt+Tab, title bar, and Explorer all show a default icon, harming identifiability.
2. When a shell process exits naturally (e.g. typing `exit` in PowerShell, Ctrl+D), the corresponding tab is not closed automatically. The `Drop for PtySession` shutdown path correctly handles X-button closes, but the spontaneous shell-exit path depends on Windows ConPTY producing an output-pipe EOF after the child exits — which it does not do reliably under typical timing.

This feature wires up icons across all three Windows paths and adds a child-process watcher thread to `PtySession` so that natural shell exits reliably propagate through the existing `PtyEvent::Exited` → `Tab::pump` → `App::pump_all retain(!exited)` chain.

This feature is Windows-targeted. Linux/macOS behavior remains unchanged.

## Objectives

- Embed `icon.ico` into `emterm.exe` via a Windows-resource crate at build time (covers taskbar, Alt+Tab, Explorer).
- Set the window icon on both the winit main window and every wry child WebView window via `WindowAttributes::with_window_icon()` (covers title bars).
- Share the icon-load implementation in a small module so winit and wry call paths both use the same code.
- Add a Windows-only child watcher to `PtySession` that observes shell process exit and unblocks the reader thread, producing `PtyEvent::Exited` exactly once per session.
- Preserve the existing 4-step `Drop for PtySession` shutdown sequence verbatim (it is required for X-button closes).
- Leave Linux/macOS encoders and PTY behavior bit-identical to the current implementation.

## User Stories

### US1: eMterm app is identifiable in the Windows taskbar

As a Windows user, I want to see the eMterm icon in the taskbar, Alt+Tab switcher, and Explorer, so that I can find the eMterm window quickly among other applications.

**Acceptance Criteria:**
- [ ] After installing a Windows release build, `emterm.exe` in Explorer shows the eMterm icon.
- [ ] Pinning eMterm to the taskbar shows the eMterm icon (not the default Win32 icon).
- [ ] Alt+Tab shows the eMterm icon while the window is open.

### US2: Window title bars show the eMterm icon

As a Windows user, I want the title bar (top-left) of the main eMterm window and every child WebView (Markdown viewer, settings panel, data viewer) to show the eMterm icon, so that I can identify the application in window-management UI.

**Acceptance Criteria:**
- [ ] The main winit window shows the eMterm icon in its title bar.
- [ ] Markdown / data / settings WebViews show the eMterm icon in their title bars.
- [ ] On Linux/Wayland, the same code path runs and at minimum does not regress (the winit `with_window_icon` API also works on Linux, but Linux WM display behavior depends on the desktop environment).

### US3: Typing `exit` in a PowerShell tab closes the tab

As a Windows user, I want the tab to close automatically when the shell process exits (via `exit`, Ctrl+D, or a crash), so that I do not have to click the X button after every shell session.

**Acceptance Criteria:**
- [ ] Inside a Windows eMterm tab running PowerShell, typing `exit` closes the tab within 500 ms.
- [ ] Sending a kill signal to the shell from outside eMterm also closes the tab.
- [ ] When all tabs close, the existing last-tab behavior applies unchanged.

### US4: X-button close path still works

As a Windows user, I want clicking the tab's X button to keep working exactly as before (no "応答なし" hang), so that I retain explicit close control.

**Acceptance Criteria:**
- [ ] Clicking the tab X button on Windows still produces a clean shutdown (the existing 4-step Drop sequence runs).
- [ ] No new deadlock is introduced between the watcher thread and the Drop path.

### US5: Linux behavior is unchanged

As a Linux user, I want shell exit and X-button close to behave exactly as they do today, so that no regression is introduced.

**Acceptance Criteria:**
- [ ] On Linux builds, shell exit still produces `PtyEvent::Exited` via the kernel-side EOF path (the existing `reader_loop` `Ok(0)` branch).
- [ ] No new watcher thread runs on Linux (or the watcher is a no-op).

## Technical Requirements

### Functional Requirements

#### Icon path

- **FR1 — `.exe` Windows resource icon:** A Windows-resource build script step MUST embed `src-tauri/icons/icon.ico` into the `emterm.exe` PE resource section on the `x86_64-pc-windows-msvc` (and any future `aarch64-pc-windows-msvc`) target. Non-Windows targets MUST be untouched.
- **FR2 — winit main window icon:** `WindowHost::new()` in `src-tauri/src/window_host.rs` MUST call `WindowAttributes::with_window_icon(Some(icon))` with a `winit::window::Icon` decoded from a bundled PNG asset (`32x32.png` and `128x128.png` are both embedded; one is selected as the source).
- **FR3 — wry child WebView icon:** The window-attribute builder in `src-tauri/src/webview_host/windows.rs` (function `resumed` in `WebViewApp`) MUST apply the same `with_window_icon()` call so that Markdown, settings, and data viewers all carry the icon. The sibling `src-tauri/src/webview_host/linux.rs` is intentionally NOT modified by this feature (per the explicit "Windows-only icon work" scope decision recorded in `要件定義書.md` section 14.1).
- **FR4 — Shared icon module:** A small module (working name `crate::window_icon`) MUST expose a single function (e.g. `app_icon() -> Option<winit::window::Icon>`) used by both FR2 and FR3 call sites. PNG decoding goes through the existing `image` crate.
- **FR5 — Fail-soft on icon error:** Icon decode failure MUST log a `log::warn!` and return `None`. The caller MUST pass `None` to `with_window_icon` (the window is still created, just without the icon).

#### Shell-exit path

- **FR6 — Windows child-exit watcher:** On Windows builds (`#[cfg(windows)]`), `PtySession::spawn()` MUST spawn a watcher thread that blocks on `Child::wait()` and, upon return, drops the watcher-owned master PTY Arc — fires `ClosePseudoConsole`, which is what unblocks the reader thread's `ReadFile` so that `reader_loop` emits exactly one `PtyEvent::Exited { reason: Eof }`. The watcher MUST then exit cleanly.
- **FR7 — Single-shot exit event:** Exactly one `PtyEvent::Exited` is emitted per session. This is structurally guaranteed: the watcher never sends events itself (it only drops the master Arc so the existing single `Exited` send inside `reader_loop` runs once on EOF), and `reader_loop` already breaks out of its loop after the first send. The X-button close path and the natural-exit path converge on the same `Exited` send.
- **FR8 — Drop ordering:** The existing `Drop for PtySession` shutdown semantics MUST be preserved. On non-Windows builds the existing 4-step sequence (kill child → close input channel → drop master → join reader/writer) MUST remain bit-identical. On Windows builds the sequence is extended to a 6-step variant that preserves the same effective behavior: (1) `ChildKiller::kill` — unblocks the watcher's `wait()`; (2) close input channel — unblocks writer; (3) no-op for the master (the watcher owns the only remaining strong Arc); (4) join the watcher thread — guarantees the watcher has dropped the master and the reader has observed EOF; (5) join the reader; (6) join the writer. The Windows order MUST be documented in code comments alongside the existing 4-step comment block.
- **FR9 — No deadlock between watcher and Drop:** Drop MUST be able to make progress even if the watcher is mid-`wait()`. The watcher MUST own the `Box<dyn Child + Send + Sync>` (moved at spawn time), and the struct MUST retain only `Box<dyn ChildKiller + Send + Sync>` (from `Child::clone_killer()`) for the Drop step 1 kill path. The struct MUST NOT share an `Arc<Mutex<...>>` over the `Child` with the watcher. The master PTY ownership split MUST follow the same principle: the watcher owns the strong `Arc<Mutex<Box<dyn MasterPty + Send>>>`; the struct retains `Weak<Mutex<Box<dyn MasterPty + Send>>>` for `resize()` (`Weak::upgrade()` on use; `warn`-log when the upgrade fails because the watcher has already dropped the master after a natural exit).
- **FR10 — Non-Windows parity:** On `#[cfg(not(windows))]`, `PtySession::spawn` and `Drop for PtySession` MUST behave bit-identical to the current implementation. No watcher thread is spawned. The existing kernel-EOF path for `reader_loop` `Ok(0)` continues to be the natural exit signal. Non-Windows `master` retains the `Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>` shape; only the Windows variant switches to `Weak`.
- **FR11 — Watcher thread cleanup:** PtySession MUST keep a `JoinHandle` for the watcher (Windows-only field) and join it during Drop step 4 (before joining reader/writer) so the watcher's master-drop completes before the reader's blocking read is expected to return. The thread MUST NOT outlive the session.
- **FR12 — Error handling for `wait()`:** If `Child::wait()` returns `Err`, the watcher MUST `log::warn!` the error and exit. The tab will then close only via X-button (acceptable degradation; this is the same outcome as before this feature). On `wait()` `Err`, the watcher MUST still drop its master Arc on exit so a subsequent X-button close path's `ChildKiller::kill` → `wait` unblock → ClosePseudoConsole chain remains intact.

### Non-Functional Requirements

- **NFR1 — Build-time isolation:** The Windows resource embedding MUST only run when building for a Windows target. `build.rs` MUST gate the `winresource` (or equivalent) invocation behind `if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")` (or `cargo:target_os=windows` equivalent). Linux/macOS builds MUST NOT pull the resource crate as a transitive dependency at runtime (build-dep is fine).
- **NFR2 — Asset payload:** The PNG asset embedded into the binary (via `include_bytes!`) MUST be one of the existing `src-tauri/icons/*.png` files (e.g. `32x32.png` or `128x128.png`). No new asset files are introduced. Total embedded bytes increase ≤ ~50 KB.
- **NFR3 — Startup overhead:** Icon decode at startup MUST be < 10 ms on a modern machine. Single PNG decode + `Icon::from_rgba` is cheap.
- **NFR4 — Watcher runtime cost:** The watcher thread idles inside `Child::wait()`. CPU usage MUST be negligible when the shell is running.
- **NFR5 — Detection latency:** Shell-exit detection latency from process exit to `PtyEvent::Exited` arrival MUST be under 500 ms typical, ideally near-immediate. (Blocking `wait()` returns as soon as the kernel signals the watcher.)
- **NFR6 — Maintainability:** The watcher's lifecycle and the shared icon module MUST be documented with doc comments covering: who owns the master Arc, why Linux is excluded, and the relationship between watcher exit and the existing Drop sequence.
- **NFR7 — Portability:** Linux/macOS builds remain bit-identical for `PtySession` semantics, encoders, and PTY behavior. Windows-only logic is gated with `#[cfg(windows)]`.

## Implementation Approach

### Architecture

Two largely-orthogonal subsystems are touched:

```
Icon subsystem (Windows)
├── build.rs                             # add winresource step (Windows target only)
├── src/window_icon.rs (new, minimal)    # app_icon() -> Option<Icon>
├── src/window_host.rs                   # call .with_window_icon() in WindowAttributes
└── src/webview_host/windows.rs          # same call in WebViewApp::resumed

PTY subsystem (Windows)
└── src/pty/mod.rs                       # add child-exit watcher (#[cfg(windows)])
                                         # adjust PtySession struct to hold ChildKiller
                                         # plus the move-Child-into-thread pattern
```

Icons are pure additions that do not affect existing tests beyond compile-time. PTY changes touch a shared struct but the Windows-only watcher is gated behind `#[cfg(windows)]` and the struct shape on non-Windows remains unchanged.

### Data Flow (PTY subsystem)

```
PtySession::spawn (Windows)
  ├─ build child + clone_killer → child_killer
  ├─ build master Arc (refcount = 1)
  ├─ spawn reader_loop thread        (does not hold the master Arc)
  ├─ spawn writer_loop thread        (does not hold the master Arc)
  └─ NEW: spawn watcher thread
              │ owns:  Box<dyn Child + Send + Sync>          ← moved
              │        Arc<Mutex<Box<dyn MasterPty + Send>>> ← moved (sole owner)
              ▼
       child.wait()  (blocks)
              │
              ▼
       returns (shell exited)
              │
              ▼
       drop(master_arc)          ─── refcount 1 → 0
                                     MasterPty drops → ClosePseudoConsole
              │
              ▼
       reader_loop's read() unblocks with Ok(0)
              │
              ▼
       sends exactly one PtyEvent::Exited { reason: Eof }
              │
              ▼
       Tab::pump sets exited=true → App::pump_all removes tab
              │
              ▼
       PtySession dropped → Drop::Windows runs:
         1. child_killer.kill()        (no-op: child already dead)
         2. drop input_tx              (writer exits)
         3. (no master to drop in struct)
         4. join watcher_join          (already exited)
         5. join reader_join           (already exited)
         6. join writer_join           (already exited)
```

On X-button close (existing path), `Drop::Windows` runs while the watcher is still mid-`wait()`. Step 1 kills the child via `ChildKiller::kill`, which unblocks the watcher's `wait()`. The watcher drops its master Arc → `ClosePseudoConsole` → reader unblocks → reader sends `PtyEvent::Exited` (this lands in a closed channel because the Tab's events Receiver has already been dropped — `reader_loop` already tolerates this defensively). Step 4 then joins the watcher, step 5 the reader, step 6 the writer. The single-shot constraint (FR7) follows from `reader_loop` only sending one `Exited` event before breaking out of its read loop.

### API Design

```rust
// New module: src/window_icon.rs
#[cfg(feature = "gui")]
pub mod window_icon {
    use winit::window::Icon;

    /// Load the bundled app icon. Returns None and logs a warn on failure.
    pub fn app_icon() -> Option<Icon> {
        // include_bytes!("../icons/128x128.png") + image::load_from_memory
        // + Icon::from_rgba; log::warn! and return None on any error.
        // ...
    }
}

// PtySession changes (Windows additions only, sketch):
struct PtySession {
    // ... existing fields ...

    // Master PTY handle:
    //   non-Windows: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>> (unchanged)
    //   Windows:     Weak<Mutex<Box<dyn MasterPty + Send>>>
    //                  — strong Arc lives in the watcher thread.
    #[cfg(not(windows))]
    master: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>,
    #[cfg(windows)]
    master: Weak<Mutex<Box<dyn MasterPty + Send>>>,

    // Child handle:
    //   non-Windows: Arc<Mutex<Box<dyn Child + Send + Sync>>> (unchanged)
    //   Windows:     Box<dyn ChildKiller + Send + Sync>
    //                  — Child itself is owned by the watcher thread.
    #[cfg(not(windows))]
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    #[cfg(windows)]
    child_killer: Box<dyn ChildKiller + Send + Sync>,

    #[cfg(windows)]
    watcher_join: Option<JoinHandle<()>>,
}
```

On Windows, the `Child` itself and the strong master `Arc` are owned by the watcher thread, and `PtySession` retains only a `ChildKiller` (via `Child::clone_killer()`) and a `Weak` for `resize()`. On Linux/macOS, the struct shape and behavior are unchanged. The `Send + Sync` bound matches portable-pty 0.8.1's `Child::clone_killer() -> Box<dyn ChildKiller + Send + Sync>` signature.

### Build configuration

`Cargo.toml`:
- Add `winresource` (or `embed-resource`) to `[build-dependencies]` (any Windows-target conditional handled in `build.rs`).
- No new runtime dependencies needed — `image` is already in scope for the existing image-viewer / icon-PNG path.

`build.rs`:
- After the existing `viewer/dist` / `settings/dist` embedding logic, append a Windows-target-gated block that invokes `winresource::WindowsResource::new().set_icon("icons/icon.ico").compile()`.

### File Structure

```
src-tauri/
  build.rs                              # add Windows-target winresource step
  Cargo.toml                            # add winresource to [build-dependencies]
  src/
    window_icon.rs                      # NEW: app_icon() helper
    lib.rs                              # add `pub mod window_icon;` under `#[cfg(feature = "gui")]`
    window_host.rs                      # call .with_window_icon() in WindowAttributes
    webview_host/
      windows.rs                        # call .with_window_icon() in WebViewApp::resumed
    pty/
      mod.rs                            # PtySession struct + watcher (#[cfg(windows)])
```

No new asset files. Existing `src-tauri/icons/{icon.ico,32x32.png,128x128.png}` are used as-is.

## Test Scenarios

### Unit Tests

- [ ] `window_icon::app_icon()` returns `Some(Icon)` on Linux/Windows test runs (the PNG payload is embedded via `include_bytes!` and the test only exercises decode + `from_rgba`). The test MUST gate on `cfg(feature = "gui")` because `winit::window::Icon` is GUI-only.
- [ ] `window_icon::app_icon()` returns `None` (and logs a warn) when given an obviously-broken byte slice. Test via a private helper that takes an arbitrary slice and uses the same decode pipeline, so the test does not need to corrupt the bundled PNG.
- [ ] On Linux test runs, `PtySession` field layout and `Drop for PtySession` execute the existing 4-step sequence verbatim (regression-guard test in `src-tauri/src/pty/mod.rs`'s test module).
- [ ] On Windows test runs, the watcher thread is spawned and joined cleanly when a session is dropped without the shell having exited (Drop step 1 kills the child → wait returns → watcher exits → join succeeds).

### Integration Tests

- [ ] Existing `cli_subcommands.rs` integration tests continue to pass unchanged on Linux and on the Windows cross-build (`make win-build`).
- [ ] If the Windows watcher thread interaction with `Drop` is testable in isolation, add a focused test using portable-pty against a short-lived child (`cmd.exe /c exit 0` on Windows; the test can be `#[cfg(windows)]`).

### E2E Tests

**Existing E2E tests**: None (no `test/README.md` and no `e2e-tests/` directory in repo root).
**Run command**: N/A

Manual verification on Windows (cannot be automated in this repo today):
- [ ] Open Windows Explorer → `emterm.exe` shows the eMterm icon.
- [ ] Launch eMterm → taskbar shows the eMterm icon → right-click taskbar item shows the eMterm icon.
- [ ] `Alt+Tab` → eMterm thumbnail uses the eMterm icon.
- [ ] Open `emterm markdown README.md` (or equivalent) → child window title bar shows the eMterm icon.
- [ ] Open settings panel → its window title bar shows the eMterm icon.
- [ ] In a PowerShell tab, type `exit` → tab closes within ~500 ms.
- [ ] In a PowerShell tab, kill the shell PID from outside (e.g. Task Manager) → tab closes within ~500 ms.
- [ ] Click the tab X-button → tab closes immediately, no "応答なし" hang (regression check for the existing Drop path).

Manual verification on Linux:
- [ ] eMterm launches and the existing window behavior is unchanged. (The new `with_window_icon` call also runs on Linux; verify there is no panic and the WM either uses the icon or ignores it gracefully.)
- [ ] In a Linux shell tab, type `exit` → tab closes (existing kernel-EOF path).

### Edge Cases

- [ ] Watcher thread runs while `PtySession::drop` executes:
  - Drop step 1 kills the child → watcher's `wait()` returns → watcher drops its master clone (now redundant) → Drop step 3 runs `drop(master.take())` (no-op if watcher already drained the Arc; otherwise normal).
  - Drop step 4 joins the reader/writer. The watcher's own `JoinHandle` MUST also be joined; document where.
- [ ] Shell exits between `PtySession::spawn` return and the first read on the PTY (race window): the watcher correctly observes exit, but the reader may still send `Data` for any final buffered output before the EOF arrives. Implementation MUST NOT swallow these final bytes.
- [ ] Concurrent X-button close + shell exit: both paths converge on the same EOF + `PtyEvent::Exited`. The single-shot constraint (FR7) follows from `reader_loop` breaking after the first EOF send.
- [ ] Many simultaneous tab closures (e.g. quit eMterm with N open tabs): each `PtySession` has its own watcher; Drop must not block tab-close progress on a slow watcher join (acceptable: 1 ms-scale per session).

### Performance Tests

N/A — both subsystems are off the hot path (startup-once for icons; one thread per session for the watcher).

## Security Considerations

N/A — icons are static bundled assets; the watcher thread only observes a local child process the user already spawned.

## Error Handling

- Icon load failure → `log::warn!` and `None`. Window is created without an icon.
- Child resource crate compile failure on Windows → cargo build fails fast at the `build.rs` step. No silent fallback (developer must fix the asset path).
- `Child::wait()` error → `log::warn!` and watcher exits. The user can still close the tab via X-button.

## Performance Optimization

### Performance Goals

- App startup: no measurable increase (single PNG decode at most).
- Tab close detection: under 500 ms wall-clock after shell exit.

### Optimization Strategies

None planned.

### Caching Strategy

None (the icon is read once at startup; no runtime caching needed).

## Success Criteria

- [ ] FR1–FR12 are implemented and verified by the tests and manual checks above.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes on Linux.
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` still passes (CLI-only build unaffected).
- [ ] `make win-build` produces a `emterm.exe` whose Windows resource section contains an icon.
- [ ] Manual verification on a Windows host confirms US1–US4 acceptance criteria.
- [ ] Manual verification on Linux confirms US5 acceptance criteria.
- [ ] `IMPLEMENTATION.md` records the final design of the watcher thread (move-Child-into-thread vs polling try_wait, and the chosen master-drop signaling mechanism).

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。`/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- (none — FR9's final implementation detail is intentionally deferred to IMPLEMENTATION.md, not a tbd)

## Implementation Phases

### Phase 1: Icon plumbing

**Goals:** Land FR1–FR5.
**Deliverables:**
- `winresource` added to `[build-dependencies]` with Windows-target gating in `build.rs`.
- `src/window_icon.rs` (or equivalent name) with `app_icon()` helper.
- `with_window_icon()` calls in `window_host.rs` and `webview_host/windows.rs`.
- Unit test for the helper's success and failure paths.

### Phase 2: Windows child watcher

**Goals:** Land FR6–FR12.
**Deliverables:**
- `PtySession` struct updated to hold `ChildKiller` on Windows (and a `watcher_join` field).
- Watcher thread spawn inside `PtySession::spawn` gated with `#[cfg(windows)]`.
- `Drop for PtySession` updated to join the watcher (Windows) without disturbing the existing 4-step sequence.
- Unit test (or focused integration test) on Windows that spawns a short-lived child and observes `PtyEvent::Exited` via the watcher.

### Phase 3: Manual verification

**Goals:** Confirm US1–US5 on Windows and Linux.
**Deliverables:**
- User-run manual checks against the acceptance criteria.
- VERIFICATION_RESULT.md populated.

## References

- `tmp/issues-windows-mux-2026-06-22.md` — origin investigation report (Issues 2 and 3)
- `src-tauri/src/pty/mod.rs:107-238` — current `PtySession::spawn`
- `src-tauri/src/pty/mod.rs:270-292` — current `PtySession::write` / `resize`
- `src-tauri/src/pty/mod.rs:294-344` — current `Drop for PtySession` (4-step shutdown)
- `src-tauri/src/pty/mod.rs:347-398` — current `reader_loop`
- `src-tauri/src/window_host.rs:321-365` — current `WindowHost::new` window creation
- `src-tauri/src/webview_host/windows.rs:60-100` — current `WebViewApp::resumed` window creation
- `src-tauri/icons/` — bundled `icon.ico`, `32x32.png`, `128x128.png`, `128x128@2x.png`
- portable-pty 0.8.x `Child` and `ChildKiller` trait
- winit `WindowAttributes::with_window_icon()` API
- winresource crate (Windows-resource build script helper)
- `doc/tasks/windows-icon-and-shell-exit/要件定義書.md` — Japanese requirements document
