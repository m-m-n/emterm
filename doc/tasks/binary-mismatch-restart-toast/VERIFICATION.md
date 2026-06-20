# Verification Document: Binary-Mismatch Restart Toast

## Overview
**Feature**: binary-mismatch-restart-toast
**SPEC.md**: `doc/tasks/binary-mismatch-restart-toast/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/binary-mismatch-restart-toast/IMPLEMENTATION.md`

## Build Verification
- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors for both.

### Actual Result (sdd.4-implement)
- Default features `cargo check`: **PASS** (exit 0, no errors/warnings) — `Finished dev profile ... in 4.65s`.
- CLI-only `cargo check --no-default-features`: **PASS** (exit 0) — `Finished dev profile ... in 0.33s`. `self_exec` and the toast are absent from the CLI build (all four spawn sites and the render block are gui-gated).

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: the pure cores (detection predicate, toast arm/prune) fully covered; integration is build-level.

### Actual Result (sdd.4-implement)
- Full lib suite: **PASS** — `test result: ok. 1871 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`.
- New unit tests added and passing:
  - `self_exec::tests::is_missing_false_when_dev_ino_match` (TS-1)
  - `self_exec::tests::is_missing_true_when_inode_differs` (TS-2)
  - `self_exec::tests::is_missing_true_when_current_absent` (TS-3)
  - `self_exec::tests::self_binary_missing_false_without_baseline` (TS-4)
  - `app::tests::restart_toast_arm_sets_dismiss_at` (TS-5)
  - `app::tests::restart_toast_prune_keeps_then_clears` (TS-6)
  - `app::tests::restart_toast_rearm_refreshes_single_toast` (TS-7)
- The 1 ignored test is pre-existing and unrelated to this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | is_missing: baseline (device,inode) equals current read | returns false | Unit |
| TS-2 | is_missing: current inode differs from baseline | returns true | Unit |
| TS-3 | is_missing: current read absent (path gone) | returns true | Unit |
| TS-4 | self_binary_missing: no baseline recorded | returns false (detection disabled) | Unit |
| TS-5 | RestartToast.arm(now) | dismissal instant equals now + linger window | Unit |
| TS-6 | RestartToast.prune: now < instant keeps active; now >= instant clears | active then inactive | Unit |
| TS-7 | RestartToast re-arm after a prior arm | single toast, dismissal instant refreshed | Unit |
| TS-8 | CLI-only build (`--no-default-features`) | compiles (self_exec/toast absent) | Integration (build) |
| TS-9 | Default-feature build + unit tests | compiles, tests pass | Integration (build) |

**TS-1..TS-9 actual outcome (sdd.4-implement): all PASS.** TS-1..TS-7 covered by the unit tests listed under Test Verification; TS-8 by the CLI-only `cargo check`; TS-9 by the default `cargo check` + the full lib test run.

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` (per-file formatting also enforced by the project PostToolUse hook)
- Static analysis: standard `cargo check` warnings; no new crate added.

### Actual Result (sdd.4-implement)
- Per-file formatting was applied automatically by the project PostToolUse hook on each edited file (no crate-wide `cargo fmt` was run, per project policy).
- Static analysis: `cargo check` (default + CLI-only) reported **no warnings or errors** for the changed files. No new external crate was added (std + `std::os::unix::fs::MetadataExt` only).

## File Structure Verification
### Files to Create
- [x] `src-tauri/src/self_exec.rs` - baseline capture, inode detector, resolver, spawn helper, restart flag (gui-gated). **DONE**

### Files to Modify
- [x] `src-tauri/src/lib.rs` - declare `self_exec` under the `gui` feature. **DONE**
- [x] `src-tauri/src/settings_launcher.rs` - spawn via `self_exec`. **DONE**
- [x] `src-tauri/src/viewer/mod.rs` - spawn via `self_exec`. **DONE**
- [x] `src-tauri/src/viewer/image.rs` - resolve via `self_exec`; note failure off-thread. **DONE**
- [x] `src-tauri/src/mux/daemon.rs` - resolve via `self_exec`; note failure on launch error. **DONE**
- [x] `src-tauri/src/app.rs` - restart-toast state, frame pump. **DONE**
- [x] `src-tauri/src/render/mod.rs` - render the restart toast with i18n. **DONE**

### Deviation from the planned file set
- `src-tauri/src/main.rs` was modified (one line) to invoke `self_exec::init()` at the
  main-terminal startup entry (after `wakeup::install`, before the event loop). The plan
  located baseline init in "the App construction path"; placing it in `main.rs::run` rather
  than inside `App::with_settings` keeps the process-global baseline out of the unit-test
  `App::new()` path so TS-4 (`self_binary_missing` with no baseline) stays valid. This is the
  GUI/main-terminal startup point the plan intended; net behavior is unchanged.

### Existing E2E Regression (Phase 3.8)
- Not applicable: the project has no E2E framework for the native binary (per SPEC.md "E2E Tests: None"). No E2E regression run was performed.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR6 implemented; pure cores unit-tested | TS-1..TS-7 pass |
| SC-2 | All four spawn sites routed through `self_exec` | Code review of the four files; TS-8/TS-9 build |
| SC-3 | Default + CLI-only builds compile; unit tests pass | TS-8, TS-9 |
| SC-4 | Linux repro shows the toast; auto-dismiss works | Manual (M-1) |
| SC-5 | No regression to terminal rendering/input | Manual (M-2) |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 startup baseline | Phase 1/3 | TS-4 (no-baseline path) + manual M-1 |
| FR2 inode detection | Phase 1 | TS-1, TS-2, TS-3, TS-4 |
| FR3 four sites + signal | Phase 2 | TS-8, TS-9 (build) + manual M-1 |
| FR4 toast render | Phase 4 | Manual M-1 |
| FR5 auto-dismiss (frame-time) | Phase 3 | TS-5, TS-6, TS-7 |
| FR6 i18n ja/en | Phase 4 | Manual M-1 (language toggle) |
| NFR1 reactive perf | Phase 2 | Code review: detection only on spawn failure |
| NFR2 terminal unaffected | Phase 2 | Manual M-2 + code review of per-site failure handling |
| NFR3 Linux-only no-op | Phase 1 | TS-8/TS-9 build; non-unix detector is no-op |
| NFR4 gui gate / CLI build | Phase 1/2 | TS-8 |
| NFR5 testability | Phase 1/3 | TS-1..TS-7 |

## Manual Testing (E2E Not Possible)
- [ ] M-1: On Linux, run the release binary; replace it on disk (package update or `install`/`cp` over the path); open settings (and trigger a viewer / mux). A top-right toast appears with the active-language text and auto-dismisses in ~4 seconds; repeated triggers keep a single toast.
- [ ] M-2: Normal terminal rendering and key input are unaffected before and after the toast appears.
- [ ] M-3: With an unchanged binary, no toast appears during normal use of settings/viewer/mux.

## Performance Verification
- Detection executes only on a self-spawn failure (reactive); no per-frame or per-keystroke detection cost. Verified by code review.

## Security Verification
- [ ] No new external input, dependency, or privilege; behavior of existing spawn sites preserved.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Detection predicate | 4 | 4 (TS-1..4) | 0 | 0 |
| Toast logic | 3 | 3 (TS-5..7) | 0 | 0 |
| Build/feature gates | 2 | 2 (TS-8,9) | 0 | 0 |
| Visible toast / regression | 3 | 0 | 0 | 3 (M-1..3) |
| **Total** | **12** | **9** | **0** | **3** |
