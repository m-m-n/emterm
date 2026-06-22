# Implementation Plan: Windows Application Icon and Shell-Exit Tab Close

## Overview

Wire the bundled eMterm icon assets to the three Windows display paths (.exe resource / winit window / wry child WebView) and introduce a Windows-only child-exit watcher in `PtySession` so that a shell terminating on its own propagates through the existing `PtyEvent::Exited` → `Tab::pump` → `App::pump_all` retain-filter chain. Linux/macOS behavior remains bit-identical.

## Objectives

- Embed `icon.ico` into `emterm.exe` via a Windows-target build step.
- Provide a shared icon-load module consumed by winit and wry call sites.
- Add a Windows-only child watcher to `PtySession` without disturbing the existing 4-step Drop sequence.
- Keep the implementation Windows-targeted; Linux/macOS code paths and existing tests must not regress.

## Prerequisites

### Development Environment

- Rust toolchain pinned by repo (rust-toolchain.toml).
- `cargo-xwin` already installed (`make setup`) for `make win-build`.
- Linux host with `bun` available for the GUI-build dist embedding.

### Dependencies

- Existing: `winit`, `wry`, `portable-pty` 0.8.x, `image` (already in scope).
- New build-dependency: `winresource` (or `embed-resource`) — Windows resource compiler crate. Cargo gracefully no-ops on non-Windows targets via `build.rs` gating.
- Existing assets: `src-tauri/icons/icon.ico`, `src-tauri/icons/32x32.png`, `src-tauri/icons/128x128.png`, `src-tauri/icons/128x128@2x.png`.

## Architecture Overview

### Technology Stack

- **Language**: Rust 2024 edition
- **Framework**: native winit + wgpu + wry stack as documented in CLAUDE.md
- **Key Libraries**: `winresource` (build-only, Windows target) — Windows resource compiler. `winit::window::Icon` API — RGBA-buffer-based icon attachment. `portable-pty` `Child` / `ChildKiller` traits — child process lifecycle split.

### Design Approach

Two largely independent subsystems are introduced:

1. **Icon subsystem.** A small new module (working name `crate::window_icon`) loads a bundled PNG once at startup, decodes it to RGBA, and exposes it as a `winit::window::Icon`. Both winit and wry window-creation sites consume that single helper, eliminating duplication. The `.exe` resource is handled by a Windows-target-gated `build.rs` extension that uses the `winresource` crate to attach `icon.ico` to the PE resource section.

2. **Shell-exit watcher subsystem.** `PtySession` retains a `ChildKiller` (obtained from the existing `Child` via `clone_killer()` at spawn time) and hands the `Child` itself to a Windows-only watcher thread that blocks inside `Child::wait()`. The watcher also takes ownership of the **strong** `Arc<Mutex<MasterPty>>`; the struct keeps only a `Weak` reference, which `resize()` upgrades on use. When `wait()` returns, the watcher drops its master `Arc` — being the sole owner, this fires `ClosePseudoConsole`. The reader thread's blocking read unblocks with `Ok(0)` and emits exactly one `PtyEvent::Exited { reason: Eof }`. On non-Windows, the struct shape and 4-step Drop sequence are bit-identical to today; on Windows the Drop sequence is extended to 6 steps (kill_killer → drop input_tx → no-op for master → join watcher → join reader → join writer).

### Component Interaction

```
                ┌─────────────────────┐
                │  build.rs (Windows) │ — Windows resource step
                └──────────┬──────────┘
                           │ embeds icon.ico
                           ▼
                       emterm.exe (PE resource)

           crate::window_icon::app_icon() ───► Option<winit::window::Icon>
              ▲                              ▲
              │                              │
WindowHost::new (winit)            WebViewApp::resumed (wry)


PtySession::spawn ── Windows ──► spawn watcher thread
                                  │ owns: Box<dyn Child + Send + Sync>   (moved)
                                  │       Arc<Mutex<MasterPty>>          (sole strong ref)
                                  │ struct keeps:
                                  │       Box<dyn ChildKiller + Send + Sync>
                                  │       Weak<Mutex<MasterPty>>         (for resize)
                                  │       watcher_join: Option<JoinHandle>
                                  └─ child.wait() blocks
                                     ↳ on return: drop(master Arc) → ClosePseudoConsole
                                       ↳ reader_loop unblocks → PtyEvent::Exited
```

## Implementation Phases

### Phase 1: Shared icon module and call-site wiring

**Goal**: A single `app_icon()` helper exists and is wired into both winit and wry window-creation sites. Building on Linux behaves exactly as today (no panic, icon may or may not be shown depending on WM).

**Files to Create**:
- `src-tauri/src/window_icon.rs` — load the bundled PNG and return a `winit::window::Icon`; fail-soft with `warn` log and `None`. Gated `#[cfg(feature = "gui")]`.

**Files to Modify**:
- `src-tauri/src/lib.rs` — add `pub mod window_icon;` under the GUI feature gate.
- `src-tauri/src/window_host.rs` — extend the `WindowAttributes::default()` chain in `WindowHost::new` with `.with_window_icon(crate::window_icon::app_icon())`.
- `src-tauri/src/webview_host/windows.rs` — extend the `WindowAttributes::default()` chain in `WebViewApp::resumed` with the same call. The sibling `linux.rs` is intentionally left untouched per the Windows-only icon scope decision (see `要件定義書.md` section 14.1). `webview_host/mod.rs` confirms the OS-specific split: `#[cfg(target_os = "linux")] mod linux;` and `#[cfg(target_os = "windows")] mod windows;` already gate the per-platform implementations.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `window_icon::app_icon` | Read embedded PNG bytes, decode to RGBA, hand back `winit::window::Icon` | none | Returns `Some(Icon)` on success, `None` on decode failure (with `warn` log) |
| `WindowHost::new` | Build the winit main window with the app icon attached | event loop available, GUI feature on | Window created; icon present in title bar / taskbar fallbacks |
| `WebViewApp::resumed` (per webview_host) | Build each child WebView window with the app icon attached | host descriptor available | Child window created; icon present in title bar |

**Processing Flow** (diagram-convertible):
1. Module load
   - `include_bytes!` pulls the chosen PNG asset at compile time.
2. First call to `app_icon()`
   - Decode succeeds → wrap RGBA bytes into a winit Icon → return Some
   - Decode fails → `warn` log → return None
3. Caller (winit/wry) passes the Option directly into `with_window_icon()`.

**Implementation Steps** (5-7 max):
1. **Pick the source asset** — choose between `32x32.png` and `128x128.png` for the embedded payload. Default to `128x128.png` (winit accepts arbitrary RGBA and downsizes; larger source preserves quality).
2. **Add the `window_icon` module** — minimal helper returning `Option<winit::window::Icon>` with `warn`-on-error semantics.
3. **Expose via `lib.rs`** under `#[cfg(feature = "gui")]`.
4. **Wire winit call site** in `WindowHost::new` so the main window receives the icon.
5. **Wire wry call site** in `WebViewApp::resumed` so child WebViews receive the icon.
6. **Add unit tests** for the helper: success path (asset bytes decode) and an explicit failure path via a private test entrypoint that accepts arbitrary input bytes.

**Dependencies**: None. Blocks Phase 2 only indirectly (icons can land first and ship independently).

**Testing Approach**:
- Unit: helper's Ok and Err paths; failure path uses a private bytes-accepting entrypoint to avoid corrupting the bundled asset.
- Integration: none new (existing `cli_subcommands.rs` continues to pass).
- E2E: none (no E2E framework in repo).
- Manual: Linux GUI launch confirms no panic / no behavior regression. Windows is verified in Phase 3.

**Acceptance Criteria**:
- [ ] `app_icon()` returns `Some` for the bundled asset.
- [ ] `app_icon()` returns `None` and logs a warn on a deliberately-broken input.
- [ ] Linux release build (`make build`) starts the existing GUI without panic or warning related to the icon.
- [ ] `cargo check --no-default-features` still passes (CLI build does not pull the new module).

**Estimated Effort**: small (single helper + two call sites + minimal tests).

### Phase 2: Windows .exe resource icon

**Goal**: `emterm.exe` produced by `make win-build` carries `icon.ico` in its Windows resource section so taskbar, Alt+Tab, and Explorer all render the eMterm icon.

**Files to Modify**:
- `src-tauri/Cargo.toml` — add `winresource` to `[build-dependencies]`.
- `src-tauri/build.rs` — add a Windows-target-gated block that invokes the Windows-resource compile step to embed `icons/icon.ico`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `build.rs` Windows step | Run resource compilation when target_os == "windows" | `icons/icon.ico` exists | PE resource section contains the ICO; non-Windows targets see no change |

**Processing Flow** (diagram-convertible):
1. cargo invokes `build.rs`.
2. `build.rs` reads `CARGO_CFG_TARGET_OS`.
   - target_os == "windows" → invoke the resource compile step on `icons/icon.ico`.
   - otherwise → skip the block.
3. Linker picks up the produced resource object at link time for the Windows target.

**Implementation Steps** (5-7 max):
1. **Declare build dependency** in `Cargo.toml`'s `[build-dependencies]`.
2. **Extend `build.rs`** with a Windows-only block at the end (after the existing viewer/settings dist embed logic) that triggers the resource compilation.
3. **Verify on Linux** that `cargo check` for the host target still succeeds — the new build-dep must not be required for Linux compilation.
4. **Verify on Windows** that `make win-build` produces an `.exe` that, when inspected with a PE viewer (or run on Windows), shows the icon in Explorer.

**Dependencies**: Independent of Phase 1 and Phase 3. Cannot be tested cleanly on Linux beyond build-success; visual confirmation requires a Windows host (Phase 3 manual verification).

**Testing Approach**:
- Unit: none (build script step).
- Integration: `make win-build` succeeds.
- E2E: none.
- Manual: see Phase 3.

**Acceptance Criteria**:
- [ ] `make build` (Linux host) succeeds with no new warnings.
- [ ] `make win-build` succeeds and produces an `emterm.exe` that Explorer renders with the eMterm icon.
- [ ] `cargo check --no-default-features` still passes.

**Estimated Effort**: small.

### Phase 3: Windows child-exit watcher in PtySession

**Goal**: On Windows, a shell that exits naturally causes the corresponding tab to close within ~500 ms via the existing `PtyEvent::Exited` path. Linux remains bit-identical.

**Files to Modify**:
- `src-tauri/src/pty/mod.rs` — refactor `PtySession` field layout to hold `ChildKiller` on Windows and hand the `Child` to a watcher thread; add the watcher; update `Drop for PtySession` to join the watcher; preserve the 4-step shutdown ordering on both platforms.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `PtySession::spawn` (Windows extension) | Move `Box<dyn Child + Send + Sync>` and the strong `Arc<Mutex<MasterPty>>` into a watcher thread; retain `Box<dyn ChildKiller + Send + Sync>` plus a `Weak<Mutex<MasterPty>>` in the struct | shell spawned, reader/writer threads constructed | Watcher running, struct holds `ChildKiller` + Weak master + `watcher_join` |
| Child-exit watcher thread | Block on `Child::wait()`; on return, drop the master strong Arc to fire `ClosePseudoConsole` | spawned by `PtySession::spawn` | Master dropped; reader unblocks with EOF; watcher exits |
| `Drop for PtySession` (Windows variant) | Run a 6-step shutdown that uses the killer (step 1) and the watcher join (step 4) | Drop initiated | All threads joined; master already dropped by watcher |
| `PtySession::resize` (Windows variant) | Upgrade the `Weak` and call resize on success; warn-log on upgrade failure (master already dropped) | session in use | Master resized if still alive |

**Processing Flow** (diagram-convertible):
1. `PtySession::spawn` on Windows
   - Build child; obtain `ChildKiller` via `Child::clone_killer()`.
   - Build the master `Arc<Mutex<MasterPty>>` (strong refcount = 1).
   - Take a `Weak` of the master for the struct.
   - Move `Child`, the strong master `Arc`, and event/handler-side clones into the watcher thread.
   - Store `ChildKiller`, the master `Weak`, and the watcher `JoinHandle` in the struct.
2. Watcher thread loop is a single blocking `wait()`.
   - wait returns Ok(_): drop the master `Arc` (sole strong ref → MasterPty drops → ClosePseudoConsole), then exit.
   - wait returns Err(_): `warn` log, drop the master `Arc` defensively, then exit.
3. `reader_loop` observes the EOF (ConPTY's `ReadFile` returns 0) and sends exactly one `PtyEvent::Exited { reason: Eof }`.
4. App layer's existing retain filter removes the tab; `PtySession` is dropped.
5. `Drop::Windows` shutdown (6-step):
   - 1: `ChildKiller::kill()` — wakes any in-flight `wait()` on the watcher; no-op if child is already dead.
   - 2: `mem::replace` swap-in dummy `input_tx`; drop the real sender → writer exits.
   - 3: no master to drop (watcher owns the strong ref).
   - 4: `watcher_join.take().join()` — ensures the master has been dropped and the reader is in or past its EOF send.
   - 5: `reader_join.take().join()`.
   - 6: `writer_join.take().join()`.
6. `Drop::Non-Windows`: unchanged 4-step sequence.

**Implementation Steps** (5-7 max):
1. **Field layout refactor** — under `#[cfg(windows)]`, replace `Arc<Mutex<Box<dyn Child + Send + Sync>>>` with `Box<dyn ChildKiller + Send + Sync>` (matches portable-pty 0.8.1's `clone_killer()` return type); convert the master from `Option<Arc<Mutex<...>>>` to `Weak<Mutex<...>>`; add `watcher_join: Option<JoinHandle<()>>`. Non-Windows keeps the current shape verbatim.
2. **Adjust `spawn`** — on Windows: build child → `clone_killer()` → take a `Weak` of the master Arc → move the strong master Arc and the Child into the watcher thread → store killer + Weak + watcher_join in the struct. Reader and writer threads consume `try_clone_reader()` / `take_writer()` exactly as today.
3. **Adjust `resize`** — on Windows, `Weak::upgrade()` then resize when Some; warn-log when None. Non-Windows unchanged.
4. **Implement the watcher thread** — blocking `wait()`; on return drop the master strong `Arc` (Ok or Err path) so `ClosePseudoConsole` fires deterministically. `warn`-log on Err. Document the lifecycle and the convergence of natural-exit and X-button paths.
5. **Update `Drop`** — implement the Windows 6-step variant (kill_killer → drop input_tx → no-op for master → join watcher → join reader → join writer). Keep the existing non-Windows 4-step variant verbatim under `#[cfg(not(windows))]`. Annotate why joining the watcher precedes joining the reader (the watcher's master-drop is what unblocks the reader).
6. **Document the invariant** — comment block explaining the single-shot `PtyEvent::Exited` guarantee (`reader_loop` breaks after the first send) and the X-button vs natural-exit convergence (both end up firing `ClosePseudoConsole` exactly once).
7. **Add a Windows-targeted test** if practical: spawn a short-lived child via portable-pty (e.g. `cmd.exe /c exit 0`) and assert that `PtyEvent::Exited` arrives on the event channel before a tight timeout. If hard to land cleanly in this crate, defer to manual verification.

**Dependencies**: Independent of Phases 1 and 2. Can land in either order.

**Testing Approach**:
- Unit: existing `pty/mod.rs` test module covers the encoder; add a Linux regression test that confirms the field layout still constructs and drops cleanly when no shell-exit signal is present.
- Integration: a Windows-only test (`#[cfg(windows)]`) that exercises the watcher against a short-lived child. If a stable harness is hard to land in this crate, defer to manual verification.
- E2E: none.
- Manual: in a Windows eMterm tab, type `exit` and confirm tab closure within 500 ms; X-button close still works without "応答なし".

**Acceptance Criteria**:
- [ ] `cargo test --lib` on Linux still passes.
- [ ] `cargo check --no-default-features` still passes.
- [ ] `make win-build` succeeds.
- [ ] Non-Windows `Drop for PtySession` retains the documented 4-step sequence verbatim.
- [ ] Windows `Drop for PtySession` runs the 6-step sequence (kill_killer → drop input_tx → no-op master → join watcher → join reader → join writer) with a comment block documenting the dependency chain.
- [ ] No new deadlock between Drop step 1 and the watcher's `wait()` (the watcher owns the Child outright; the killer is a separate handle that does not require the watcher's mutex).
- [ ] Master `Weak::upgrade()` failure in `resize` is handled gracefully on Windows.

**Estimated Effort**: medium (field-layout refactor + thread lifecycle + cross-platform gating).

### Phase 4: Manual verification on Windows and Linux

**Goal**: Confirm US1–US5 acceptance criteria from `SPEC.md`.

**Files to Modify**: none.

**Implementation Steps**:
1. Build Linux release (`make build`), launch eMterm, run a few tabs including `exit`. Confirm no regression and that the title bar carries the icon (or at minimum no error log).
2. Build Windows release (`make win-build`), copy to a Windows host, install, and exercise:
   - Explorer shows the icon on `emterm.exe`.
   - Pinning to taskbar shows the icon.
   - Alt+Tab shows the icon.
   - Main window title bar shows the icon.
   - `emterm markdown <file>` child window shows the icon.
   - Settings panel window shows the icon.
   - PowerShell tab: `exit` closes the tab within 500 ms.
   - PowerShell tab: kill from Task Manager closes the tab.
   - X-button close still works cleanly.
3. File any follow-up bug if a corner case is broken.

**Dependencies**: Phases 1, 2, and 3.

**Testing Approach**: manual only.

**Acceptance Criteria**:
- [ ] All US1–US5 acceptance criteria from SPEC.md are checked off.

**Estimated Effort**: small (manual run-through).

---

## Complete File Structure

```
src-tauri/
  Cargo.toml                       # MOD: add winresource to [build-dependencies]
  build.rs                         # MOD: Windows-target resource embed
  src/
    lib.rs                         # MOD: pub mod window_icon under gui feature
    window_icon.rs                 # NEW: app_icon() helper
    window_host.rs                 # MOD: WindowAttributes.with_window_icon(...)
    webview_host/
      windows.rs                   # MOD: WindowAttributes.with_window_icon(...)
    pty/
      mod.rs                       # MOD: ChildKiller split + watcher thread (#[cfg(windows)])
doc/tasks/windows-icon-and-shell-exit/
  要件定義書.md                     # (from create-spec)
  SPEC.md                          # (from create-spec)
  IMPLEMENTATION.md                # (this file)
  VERIFICATION.md                  # (sibling)
  sdd.yaml                         # (from create-spec, updated by each step)
  tasks.yaml                       # (sibling; tasks broken out from this plan)
```

## Testing Strategy

- **Unit**: `window_icon::app_icon` Ok/Err paths; `PtySession` Drop ordering regression on Linux.
- **Integration**: existing `cli_subcommands.rs` continues to pass on Linux. Optional Windows-only test for the watcher path.
- **E2E**: none — no E2E framework in this repo per `test/README.md` absence.
- **Manual**: Phase 4 covers icon visual checks and shell-exit timing on Windows.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `winresource` | latest stable | Windows resource compile in `build.rs` (build-dep, Windows target only) |

(`image`, `winit`, `wry`, `portable-pty` are existing direct dependencies; no version bumps required.)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `winresource` build-dep conflicts with the existing Linux/Windows toolchain (cargo-xwin) | Low | Medium | Confirm via `make win-build` early in Phase 2; fall back to `embed-resource` if needed |
| ChildKiller split changes the existing non-Windows behavior by accident | Medium | High | Strict `#[cfg(windows)]` gating; add a Linux regression test asserting the existing 4-step Drop sequence still constructs and drops correctly |
| Watcher and Drop race on a Windows host | Medium | Medium | Watcher owns `Child` exclusively; Drop step 1 uses the retained `ChildKiller`; no shared `Mutex<Child>` between them |
| Linux side regresses due to shared icon module compile-time selection | Low | Low | Helper is `#[cfg(feature = "gui")]`-gated; Linux GUI build also calls `with_window_icon()` (winit accepts `Option`, so `None` is a clean no-op) |
| PNG decode failure at startup causes silent functional break | Low | Low | `warn`-level log; explicit fail-soft to `None`; manual verification step catches missing icon |

## Open Questions

- [ ] `winresource` vs `embed-resource` — both work; default to `winresource` per the report's recommendation. Confirm during Phase 2 if a build issue arises.
- [ ] Whether to embed `32x32.png` or `128x128.png` as the source for `app_icon` — default to `128x128.png`. Revisit only if startup decode time becomes a concern (NFR3).

## Success Metrics

- [ ] Functional completeness: FR1–FR12 implemented and verified.
- [ ] Quality: `cargo test --lib` on Linux and `cargo check --no-default-features` continue to pass; `make win-build` produces a working artifact.
- [ ] Performance: shell-exit detection latency under 500 ms on Windows; icon decode under 10 ms at startup.
