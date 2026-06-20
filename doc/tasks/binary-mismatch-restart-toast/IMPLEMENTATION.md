# Implementation Plan: Binary-Mismatch Restart Toast

## Overview

Detect, on a failed self-spawn, that the running binary no longer matches the on-disk binary (inode comparison), and show an auto-dismissing top-right toast on the main window prompting a restart. Linux only.

## Objectives

- Establish a self-binary baseline at startup and a reactive inode-comparison detector.
- Route all four self-spawn sites through one shared module so a failed spawn raises a single process-global restart signal.
- Surface that signal as a 4-second auto-dismissing, i18n (ja/en) toast reusing the existing SFTP toast pattern.

## Prerequisites

### Development Environment
- Rust toolchain pinned by the project (rustfmt style_edition=2024).
- Linux host for manual verification (binary-replacement repro).

### Dependencies
- No new external crate. Uses the standard library and the platform metadata query for device/inode (unix).
- Internal: existing SFTP toast model (`sftp::ui`), the render module's inline `t(ja, en)` i18n closure, the App frame pump.

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: winit + wgpu + egui (native terminal), GUI feature only
- **Key Libraries**: std only (process spawn, platform metadata for device/inode)

### Design Approach

- **Reactive detection (案A only).** Detection runs solely when a self-spawn fails; there is no periodic or focus-time polling. This keeps the normal render/input path cost-free.
- **One shared detection module** (`self_exec`) owns: the startup baseline, the inode-comparison predicate, the self-exe path resolver, the spawn helper, and a process-global restart flag.
- **Spawning and detection use different executable references (critical).** Spawning resolves the executable via the fresh `current_exe()` path, exactly as each site does today — so a replaced binary still fails the self-spawn (the running process keeps the old, now-unlinked inode and resolves to a non-existent path), and that failure is the reactive trigger. Detection instead reads the device/inode of the **startup baseline path** (captured before any replacement) and compares it to the recorded identity. Using the baseline path for spawning would let a replaced binary spawn the new version successfully, suppressing the trigger and breaking the accepted premise (current_exe-based self-reexec). The split is deliberate.
- **Process-global signal bridges off-thread producers to the App.** The image viewer spawns on a dedicated worker thread, so the failure signal must not assume the App thread. A process-global atomic flag is set by any spawn site, which also wakes the winit event loop (via the existing wake mechanism) so an idle shell still produces a frame; the App consumes the flag once per frame.
- **Toast lives in the App, frame-time driven.** The toast mirrors the SFTP toast's monotonic frame-time dismiss model rather than a wall-clock timer.
- **Platform isolation.** Detection is meaningful only on Linux/unix; on other targets the detector is a compile-time no-op returning "not missing". All new code sits under the `gui` feature so the CLI-only build is untouched.

### Component Interaction

1. Startup wires the baseline holder once (records the executable identity).
2. Each self-spawn site resolves the executable through the shared module and, on spawn failure, asks the module to note the failure.
3. The module sets the restart flag only when the detector reports a mismatch.
4. The App frame pump consumes the flag, arming/refreshing the toast with the current frame time.
5. The render pass draws the toast while it is active; the pump prunes it after the linger window.

## Implementation Phases

### Phase 1: Detection core module (`self_exec`)

**Goal**: A GUI-gated module providing baseline capture, a pure inode-comparison predicate, the self-exe resolver, the spawn helper, and the process-global restart flag — with the comparison logic unit-tested in isolation.

**Files to Create**:
- `src-tauri/src/self_exec.rs` - baseline holder, detector, resolver, spawn helper, restart flag.

**Files to Modify**:
- `src-tauri/src/lib.rs` - declare the module under the `gui` feature gate.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| baseline init | Resolve the executable path once and record (path, device, inode); record "no baseline" if resolution fails | Called once early in GUI startup | A one-time process-global baseline holder is set |
| is_missing (pure) | Decide mismatch from a baseline and the current (device, inode) read of the baseline path | A baseline exists | Returns mismatch=true when current read is absent or differs |
| self_binary_missing | Read the baseline path's current device/inode and apply is_missing | — | Linux/unix: real result; other targets: always "not missing"; no baseline: "not missing" |
| self_exe_path | Provide the executable path for spawn sites by resolving `current_exe()` fresh (NOT the baseline path) | — | Returns a path or an error matching the existing per-site resolution failure |
| note_spawn_failure | If self_binary_missing, set the restart flag and wake the event loop | Called after a failed self-spawn | Restart flag set iff mismatch detected; a frame is scheduled |
| restart_required | Report whether the restart flag is set, for the App to consume | — | Returns and lets the App clear the flag for the frame |
| spawn_self | Build a self-targeted process command, let the caller configure it, spawn, and note failure on error | Caller supplies command configuration | Returns the child or the spawn error after noting failure |

**Function Contracts** (signature-with-contract level, not code):

```
is_missing(baseline, current_dev_ino) -> bool
  Precondition: baseline holds (device, inode); current_dev_ino is the read of the
                baseline path, or "absent" when the path can no longer be queried.
  Postcondition: true when current is "absent" OR differs from baseline; else false.

self_binary_missing() -> bool
  Postcondition: false when no baseline; false on non-unix targets;
                 otherwise is_missing(baseline, read(baseline.path)).
```

**Processing Flow** (self_binary_missing):
1. Look up the baseline holder.
   - No baseline -> return false (detection disabled).
2. On non-unix target -> return false (compile-time no-op).
3. Query the baseline path's current device/inode.
   - Query fails (path gone) -> return true.
   - Query succeeds and (device, inode) differs -> return true.
   - Otherwise -> return false.

**Implementation Steps**:
1. **Module skeleton** - Create `self_exec` under the `gui` feature; declare it in `lib.rs`.
2. **Baseline holder** - One-time process-global holder of the executable identity, plus the init entry.
3. **Pure predicate** - Implement `is_missing` independent of globals and the filesystem.
4. **Detector wrapper** - `self_binary_missing` reading the baseline path's current identity; unix-only real path, no-op elsewhere.
5. **Resolver + flag + spawn helper** - `self_exe_path`, the restart flag with `note_spawn_failure` / `restart_required`, and `spawn_self`.
6. **Unit tests** - Cover the pure predicate and the no-baseline path.

**Dependencies**: Blocks Phase 2 and Phase 3.

**Testing Approach**:
- Unit: is_missing for match / inode-differs / absent; self_binary_missing with no baseline.
- Integration: covered by build (default + CLI-only) in Phase 2/global checks.

**Acceptance Criteria**:
- [ ] Module compiles under the `gui` feature and is absent from the CLI-only build.
- [ ] Pure predicate unit tests pass.

**Estimated Effort**: small

---

### Phase 2: Route the four self-spawn sites through `self_exec`

**Goal**: Every self-spawn resolves the executable through `self_exec` and notes failure on error, so any failed self-spawn caused by a replaced binary raises the restart flag — without changing the existing "terminal unaffected" behavior.

**Files to Modify**:
- `src-tauri/src/settings_launcher.rs` - settings window spawn.
- `src-tauri/src/viewer/mod.rs` - Markdown / data viewer spawn.
- `src-tauri/src/viewer/image.rs` - image viewer spawn (dedicated worker thread).
- `src-tauri/src/mux/daemon.rs` - mux daemon launch (`ensure_daemon_running`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| settings spawn | Configure args/stdout through the spawn helper; keep existing failure logging | A spawn is requested | On failure, restart flag noted; terminal unaffected |
| viewer spawn | Configure flag+payload args through the spawn helper; keep payload cleanup on failure | A render request to spawn | On failure, restart flag noted; orphan payload removed |
| image worker | Resolve the executable once at worker start via the resolver; note failure per failed child spawn | Worker thread started | Off-thread spawn failure raises the restart flag |
| daemon launch | Resolve the executable via the resolver; note failure before mapping the launch error | A daemon start is requested | On failure, restart flag noted; existing error string returned |

**Processing Flow** (per site):
1. Resolve the executable through `self_exec` (replacing the direct resolution).
2. Build/configure the process command as the site already does.
3. Attempt spawn.
   - Success -> existing tracking/logging unchanged.
   - Failure -> note the failure through `self_exec`, then run the site's existing failure handling (log, cleanup, error return).

**Implementation Steps**:
1. **Settings + viewer sites** - Route through `spawn_self`, preserving each command's configuration and failure handling.
2. **Image worker** - Resolve once at thread start; note failure on each failed child spawn.
3. **Daemon launch** - Resolve via the resolver; note failure on launch error before returning.

**Dependencies**: Requires Phase 1. Blocks the end-to-end toast trigger.

**Testing Approach**:
- Integration: default-feature build compiles; CLI-only build compiles (sites are GUI-only).
- Manual: with a replaced binary, each site's failure raises the toast (verified in Phase 4 + sdd.6).

**Acceptance Criteria**:
- [ ] All four sites resolve via `self_exec` and note failure on error.
- [ ] Existing per-site behavior (logging, payload cleanup, error return) is preserved.

**Estimated Effort**: small

---

### Phase 3: App restart-toast state and frame pump

**Goal**: The App owns a single frame-time restart toast, wires the baseline init at startup, and each frame consumes the restart flag to arm/refresh the toast and prunes it after the 4-second linger.

**Files to Modify**:
- `src-tauri/src/app.rs` - add restart-toast state; wire baseline init at startup; consume the flag and prune in the frame pump (alongside the SFTP toast pump).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| restart toast state | Hold an optional frame-time dismissal instant | — | Active while a dismissal instant is set |
| arm | Set the dismissal instant to now + linger window (a local 4-second constant owned by this feature, not imported from the SFTP module) | A mismatch was signaled | A single toast is (re)armed; prior instant overwritten |
| prune | Clear the dismissal instant once the frame time reaches it | Called each frame with the frame time | Toast becomes inactive after the linger window |
| startup wiring | Invoke baseline init once during GUI startup | App is being constructed | Baseline established before any runtime spawn |
| pump integration | Consume the restart flag and prune within the existing frame pump | Frame pump runs with monotonic frame time | Toast state reflects signals; redraw requested on change |

**Function Contracts**:

```
arm(now)
  Postcondition: dismissal instant = now + linger window (single toast).
prune(now)
  Postcondition: if dismissal instant set and now >= it, toast becomes inactive.
```

**Processing Flow** (per frame, in the pump):
1. If the restart flag is set, arm the toast with the current frame time and clear the flag.
2. Prune the toast against the current frame time.
3. Report whether state changed so the caller can request a redraw.

**Implementation Steps**:
1. **State** - Add the restart-toast field and its arm/prune/active behavior.
2. **Startup init** - Invoke `self_exec` baseline init once during GUI startup.
3. **Pump** - Consume the restart flag and prune in the frame pump using the existing frame time.
4. **Unit tests** - arm sets the instant; prune keeps/clears by frame time; re-arm refreshes.

**Dependencies**: Requires Phase 1. Pairs with Phase 4 for visible output.

**Testing Approach**:
- Unit: arm / prune / re-arm against synthetic frame times.

**Acceptance Criteria**:
- [ ] Baseline init runs once at startup.
- [ ] Toast arms on signal, refreshes on repeat, and auto-clears after the linger window.
- [ ] Toast-state unit tests pass.

**Estimated Effort**: small

---

### Phase 4: Render the toast with i18n

**Goal**: The render pass draws the restart toast in the top-right region (not overlapping concurrent SFTP toasts) with ja/en text while the toast is active.

**Files to Modify**:
- `src-tauri/src/render/mod.rs` - draw the restart toast near the existing SFTP toast block.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| toast render | Draw a single top-right popup with the restart message when active | Toast is active | A non-overlapping toast is shown using the existing popup style |
| i18n text | Provide ja/en strings via the existing inline translation closure | Render has the translation closure | Message follows the active language |

**Processing Flow**:
1. If the restart toast is active, place a popup anchored to the top-right with an offset chosen so it does not overlap the SFTP toast stack.
2. Render the localized message (ja: "eMterm が更新されました。再起動してください" / en: "eMterm was updated. Please restart.").

**Implementation Steps**:
1. **Render block** - Add a top-right popup driven by the toast-active state, reusing the SFTP popup style.
2. **Offset** - Choose a vertical offset that avoids overlap with the SFTP toast stack.
3. **i18n** - Supply the ja/en strings via the existing translation closure.

**Dependencies**: Requires Phase 3.

**Testing Approach**:
- Manual: Linux binary-replacement repro shows the toast; it auto-dismisses (sdd.6).

**Acceptance Criteria**:
- [ ] Toast renders top-right while active and does not overlap SFTP toasts.
- [ ] Text follows the active language.

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── self_exec.rs          # NEW (Phase 1): baseline, detector, resolver, spawn helper, restart flag (gui-gated)
├── lib.rs                # MOD (Phase 1): declare self_exec under #[cfg(feature = "gui")]
├── settings_launcher.rs  # MOD (Phase 2): spawn via self_exec
├── viewer/mod.rs         # MOD (Phase 2): spawn via self_exec
├── viewer/image.rs       # MOD (Phase 2): resolve via self_exec, note failure off-thread
├── mux/daemon.rs         # MOD (Phase 2): resolve via self_exec, note failure on launch error
├── app.rs                # MOD (Phase 3): restart-toast state + startup init + frame pump
└── render/mod.rs         # MOD (Phase 4): render restart toast + i18n
```

## Testing Strategy

- Unit: detection predicate and toast arm/prune logic as pure, global-free functions (the highest-value, deterministic surface).
- Integration: default-feature build + tests pass; CLI-only build compiles (feature-gate safety).
- E2E: none (native app; no E2E framework in the project).
- Manual: Linux binary-replacement repro for the visible toast and auto-dismiss (sdd.6).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | - | Uses std + platform metadata (device/inode) on unix |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Dev rebuild changes inode and shows a toast | Medium | Low | Accepted by decision (no debug suppression); reactive-only limits it to spawn failures |
| SFTP + restart toast overlap | Low | Low | Distinct id + chosen vertical offset |
| Off-thread (image worker) signal delivery | Low | Medium | Process-global atomic flag consumed by the App each frame |
| Process globals hinder testing | Low | Low | Keep the comparison and toast logic as pure functions; test those |

## Open Questions

- [ ] Exact vertical offset for the restart toast relative to the SFTP toast stack (decided during Phase 4 implementation; cosmetic).
- [ ] Known limitation (accepted): the image-viewer worker caches its executable path at worker-thread start. If the worker was already running before the binary was replaced, its child spawns still succeed (cached clean path → new binary) and that site will not raise the toast. The per-use sites (settings / data-viewer / daemon) still trigger. Inherent to reactive (案A) detection; not changing scope.

## Success Metrics

- [ ] All four spawn sites routed through `self_exec`.
- [ ] Detection and toast logic unit-tested (pure cores).
- [ ] Default + CLI-only builds compile; unit tests pass.
- [ ] Linux manual repro shows the toast with 4-second auto-dismiss.
- [ ] No regression to terminal rendering/input.
