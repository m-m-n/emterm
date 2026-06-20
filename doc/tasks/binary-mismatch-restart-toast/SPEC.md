# Feature: Binary-Mismatch Restart Toast

## Overview

eMterm launches its settings panel, viewers, and mux daemon by re-executing itself via `std::env::current_exe()`. When the binary is replaced on disk (e.g. `apt`/`dpkg` `rename(2)`), the running process keeps the old, now-unlinked inode and `current_exe()` starts returning `/usr/bin/emterm (deleted)`, so child `spawn()` fails with `ENOENT`. This feature detects that self-binary mismatch reactively (on a failed self-spawn) and shows a top-right toast on the main window prompting the user to restart. Linux only.

## Objectives

- Detect, on a failed self-spawn, that the running binary no longer matches the on-disk binary (inode comparison).
- Route all four self-spawn sites through one shared module so detection is uniform.
- Show an auto-dismissing toast on the main window, i18n (ja/en), reusing the existing SFTP toast pattern.

## User Stories

### US1: Notice after update
As an eMterm user who updated the package while the app is still running, I want a visible hint when a feature silently fails to launch, so that I understand I need to restart.

**Acceptance Criteria:**
- [ ] After the binary is replaced, using settings / viewer / image / mux shows a toast.
- [ ] The toast auto-dismisses after 4 seconds.
- [ ] The toast text follows the active language (ja/en).
- [ ] Terminal rendering / input are unaffected.

## Technical Requirements

### Functional Requirements

- **FR1 — Startup inode baseline:** On main-terminal (GUI) startup, resolve `current_exe()` and record the baseline `(path, device, inode)` of the executable. If resolution fails, record "no baseline".
- **FR2 — Mismatch detection:** `self_binary_missing()` returns `true` when the baseline path's current `(device, inode)` differs from the recorded baseline, or the path can no longer be `stat`-ed (`ENOENT`). Returns `false` when they match or when no baseline was established. Inode comparison (not `.exists()`). Linux/unix only; on non-target platforms it is a compile-time no-op returning `false`.
- **FR3 — Shared self-spawn + reactive signal:** All four self-spawn sites go through a shared `self_exec` module. On a `spawn()` failure, the site calls a shared "note spawn failure" entry that sets a process-global `RESTART_REQUIRED` flag iff `self_binary_missing()` is `true`, and wakes the winit event loop so the App consumes the flag promptly (the failure may originate off the App thread, e.g. the image-viewer worker). Each site preserves its existing executable-resolution timing (no change to when/whether `current_exe()` is cached).
- **FR4 — Toast rendering:** The main window renders a top-right toast (reusing the SFTP toast `egui::Area` / `Frame::popup` pattern) when the restart state is active. Offset so it does not overlap concurrent SFTP toasts.
- **FR5 — Auto-dismiss (frame-time):** The toast auto-dismisses 4 seconds (`TOAST_LINGER_SECS = 4.0`) after it was armed, using egui frame-time (monotonic). Re-arming on a subsequent failed spawn keeps a single toast and refreshes `dismiss_at`.
- **FR6 — i18n:** Toast text is provided via the existing inline `t(ja, en)` closure used by the render module — ja: `eMterm が更新されました。再起動してください`, en: `eMterm was updated. Please restart.`

### Non-Functional Requirements

- **NFR1 - Performance:** Detection runs only on a self-spawn failure (reactive). No added per-frame or per-keystroke cost in the normal path.
- **NFR2 - Robustness:** A failed self-spawn never blocks the terminal; the existing "terminal unaffected" behavior at every site is preserved. Detection errors fall back to "not missing".
- **NFR3 - Platform isolation:** Linux only. On Windows / non-unix, `self_binary_missing()` is a no-op (`false`) and the build must not break.
- **NFR4 - Build gating:** New code lives under the `gui` feature (the four spawn sites and the toast are GUI-only). The CLI-only build (`--no-default-features`) must still compile.
- **NFR5 - Testability:** The inode comparison and the toast dismiss logic are pure functions, unit-testable without process globals or real spawns.

## Implementation Approach

### Architecture

```
                         (GUI main-terminal process)
  startup ──► self_exec::init()  ── records baseline (path, dev, inode)

  settings_launcher ─┐
  viewer/mod        ─┤  spawn self ──► on Err ──► self_exec::note_spawn_failure()
  viewer/image      ─┤  (image: on image-viewer-spawn thread)        │
  mux/daemon        ─┘                                               ▼
                                              static RESTART_REQUIRED: AtomicBool
                                                               │ (App reads each frame)
                                                               ▼
  App frame pump ── if restart_required(): restart_toast.arm(now)  (dismiss_at = now + 4.0)
                 ── prune when now >= dismiss_at
                                                               │
                                                               ▼
  render ── draw top-right toast (egui::Area + Frame::popup) when restart_toast active
```

### New module: `self_exec` (GUI-gated)

Suggested location: `src-tauri/src/self_exec.rs`, declared `#[cfg(feature = "gui")]` in `lib.rs`.

```rust
/// Identity of the executable captured at startup.
struct SelfExeId { path: PathBuf, dev: u64, ino: u64 }

static SELF_EXE: OnceLock<Option<SelfExeId>> = OnceLock::new();
static RESTART_REQUIRED: AtomicBool = AtomicBool::new(false);

/// Capture the baseline once at GUI startup.
pub fn init();

/// Inode-comparison detection. Linux/unix only; false elsewhere.
pub fn self_binary_missing() -> bool;

/// If self_binary_missing(), set RESTART_REQUIRED and wake the winit loop so the
/// toast appears promptly even when the failure originates off the App thread.
/// Called by spawn sites on Err.
pub fn note_spawn_failure();

/// App reads this each frame to arm the toast.
pub fn restart_required() -> bool;

/// Resolver used by spawn sites. Resolves via `current_exe()` fresh on each call
/// — identical to today's per-site behavior. It does NOT return the startup
/// baseline path (see "Resolution vs detection" below).
pub fn self_exe_path() -> std::io::Result<PathBuf>;

/// Convenience: build `Command::new(self_exe_path()?)`, let caller configure it,
/// spawn, and on Err call note_spawn_failure() before returning the Err.
pub fn spawn_self(
    configure: impl FnOnce(&mut std::process::Command),
) -> std::io::Result<std::process::Child>;
```

#### Resolution vs detection (critical)

Spawning and detection use **different** executable references on purpose:

- **Spawning** resolves via `current_exe()` fresh (`self_exe_path`), exactly as the
  four sites do today. On Linux after a `rename(2)` replacement, the running
  process's `current_exe()` returns `…/emterm (deleted)`, so the spawn still fails
  with `ENOENT` — this failure is what triggers the reactive (案A) toast, and it
  keeps the accepted premise (current_exe-based self-reexec) unchanged.
- **Detection** stats the **startup baseline path** (the clean path captured at
  `init`) to read its current `(device, inode)`. After replacement that path points
  to the new inode (or is gone), so the comparison reports a mismatch.

If spawning instead used the baseline clean path, a replaced binary would spawn the
*new* version successfully — the spawn would not fail, the reactive toast would not
fire, and an old-version parent would launch a new-version child (contrary to the
accepted premise). Hence the deliberate split.

**Pure, testable core** (no globals, no FS in the test):

```rust
/// `current` = Some((dev, ino)) from stat of the baseline path, or None on ENOENT.
fn is_missing(baseline: &SelfExeId, current: Option<(u64, u64)>) -> bool {
    match current {
        None => true,                                  // path gone / replaced+deleted
        Some((dev, ino)) => (dev, ino) != (baseline.dev, baseline.ino),
    }
}
```

`self_binary_missing()` wraps `is_missing` by `stat`-ing `baseline.path` (via `std::os::unix::fs::MetadataExt`), returning `false` when there is no baseline.

### App state: restart toast

`App` gains a small struct mirroring the SFTP toast dismiss model:

```rust
struct RestartToast { dismiss_at: Option<f64> } // frame-time

impl RestartToast {
    fn arm(&mut self, now: f64)  { self.dismiss_at = Some(now + TOAST_LINGER_SECS); }
    fn prune(&mut self, now: f64) { if matches!(self.dismiss_at, Some(at) if now >= at) { self.dismiss_at = None; } }
    fn active(&self) -> bool { self.dismiss_at.is_some() }
}
```

Pump (alongside `pump_sftp(now)`, same frame-time `now`):

```rust
if self_exec::restart_required() { self.restart_toast.arm(now); /* consume global flag */ }
self.restart_toast.prune(now);
```

`TOAST_LINGER_SECS` (4.0) is reused from `sftp::ui` (or a shared const).

### Per-site integration

| Site | Change |
|------|--------|
| `settings_launcher.rs:74` | Replace `current_exe()` + `Command::new` + `.spawn()` with `self_exec::spawn_self(\|c\| { c.arg("--settings").stdout(piped()); })`; existing Err logging stays. |
| `viewer/mod.rs:254` (`spawn_child`) | Same, configuring `c.arg(flag).arg(path)`; keep payload cleanup on Err. |
| `viewer/image.rs:156` (`SpawnWorker::start`) | Resolve `self_exec::self_exe_path()` once at thread start; on each `spawn_viewer_child` Err call `self_exec::note_spawn_failure()`. |
| `mux/daemon.rs:155` (`ensure_daemon_running`) | Resolve via `self_exec::self_exe_path()`; on `cmd.spawn()` Err call `self_exec::note_spawn_failure()` before mapping to the `String` error. |

### Rendering

In `render/mod.rs`, near the SFTP toast block (`sftp_toasts`, line ~518), add:

```rust
if app.restart_toast.active() {
    egui::Area::new(egui::Id::new("restart_toast"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, /* offset below SFTP stack */))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(t(
                    "eMterm が更新されました。再起動してください",
                    "eMterm was updated. Please restart.",
                ));
            });
        });
}
```

The exact vertical offset to avoid overlapping the SFTP toast stack is decided during implementation.

### Dependencies

**Internal:**
- `sftp::ui` — toast dismiss model / `TOAST_LINGER_SECS` reference.
- `render/mod.rs` — toast rendering site and the `t(ja, en)` i18n closure.
- `app` — frame pump (`pump_sftp` neighbor) and toast state.

**External:** none new (uses `std`, `std::os::unix::fs::MetadataExt`).

### File Structure

```
src-tauri/src/
├── self_exec.rs        # NEW: baseline capture, inode detection, spawn helper, restart flag (GUI-gated)
├── lib.rs              # declare `mod self_exec` under #[cfg(feature = "gui")]
├── settings_launcher.rs# route spawn through self_exec
├── viewer/mod.rs       # route spawn through self_exec
├── viewer/image.rs     # resolve via self_exec, note_spawn_failure on Err
├── mux/daemon.rs       # resolve via self_exec, note_spawn_failure on Err
├── app.rs              # RestartToast state + pump
└── render/mod.rs       # render restart toast
```

## Test Scenarios

### Unit Tests
- [ ] `is_missing`: baseline `(dev,ino)` == current → `false`.
- [ ] `is_missing`: current inode differs → `true`.
- [ ] `is_missing`: current `None` (ENOENT) → `true`.
- [ ] `self_binary_missing`: no baseline → `false` (detection disabled).
- [ ] `RestartToast`: `arm(now)` sets `dismiss_at == now + 4.0`.
- [ ] `RestartToast`: `prune` keeps while `now < at`, clears when `now >= at`.
- [ ] `RestartToast`: re-`arm` refreshes `dismiss_at` (single toast).

### Integration Tests
- [ ] CLI-only build compiles (`--no-default-features` `cargo check`).
- [ ] Default-feature build compiles and unit tests pass.

### E2E Tests
**Existing E2E tests**: None (native Rust app; `cargo test` unit/integration only).
**Run command**: Not detected.

### Edge Cases
- [ ] `current_exe()` fails at startup → no baseline → detection disabled (no false toast).
- [ ] Repeated failed spawns → single toast, `dismiss_at` refreshed.
- [ ] dev (`cargo run`) rebuild changes inode → toast may appear (accepted: not suppressed).
- [ ] Windows / non-unix build → `self_binary_missing()` no-op `false`, compiles.

### Manual Verification (Linux, sdd.6)
- [ ] Launch the release binary; replace it on disk (`cp newbin /usr/bin/emterm` via package update or `install`); open settings → toast appears, auto-dismisses in ~4s.

## Error Handling

| Condition | Handling |
|-----------|----------|
| `current_exe()` Err at startup | Record "no baseline"; detection returns `false`. |
| `current_exe()` / `self_exe_path()` Err at spawn time | Existing per-site warn + bail; `note_spawn_failure()` consults detection (no-baseline → no toast). |
| `spawn()` Err but binary intact | `self_binary_missing()` false → flag not set → no toast. |
| Toast + SFTP toast concurrent | Distinct `egui::Id` + vertical offset; no overlap. |

## Success Criteria

- [ ] FR1–FR6 implemented and unit-tested (pure cores).
- [ ] All four spawn sites routed through `self_exec`.
- [ ] Default-feature and CLI-only builds compile; unit tests pass.
- [ ] Linux manual repro shows the toast; auto-dismiss works.
- [ ] No regression to terminal rendering/input.

## Open Questions

> 未解決の要件は sdd.yaml で `status: tbd` として管理されています。

- None blocking. Known limitation (accepted, reactive design): the image-viewer
  worker resolves its executable once at worker-thread start. If the worker was
  already started before the binary was replaced, its cached clean path still
  resolves to the new binary and its child spawns succeed — so that site will not
  raise the toast. The settings / data-viewer / daemon sites resolve per use and
  do trigger. This is inherent to reactive (案A) detection and is acceptable.
- Cosmetic: exact vertical offset of the restart toast relative to the SFTP toast
  stack (decided during implementation).

## References

- Design report: `tmp/binary-mismatch-restart-toast-design.md`
- Requirements: `doc/tasks/binary-mismatch-restart-toast/要件定義書.md`
- Existing SFTP toast: `src-tauri/src/render/mod.rs:519`, `src-tauri/src/sftp/ui.rs`
- Self-spawn sites: `settings_launcher.rs:74`, `viewer/mod.rs:254`, `viewer/image.rs:156`, `mux/daemon.rs:155`
