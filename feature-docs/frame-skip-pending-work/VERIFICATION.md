# Verification Document: frame-skip-pending-work

## Overview

**Feature**: frame-skip-pending-work /
**SPEC.md**: `feature-docs/frame-skip-pending-work/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/frame-skip-pending-work/IMPLEMENTATION.md`

## Build Verification

Three configurations, each expected to exit 0 with **zero warnings** (NFR2):

- main:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- cli_feature_gate:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- windows_cross:
  `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests`

## Test Verification

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: full pass (all pre-existing tests plus TS-1 … TS-6).
- Coverage: a coverage percentage is not tracked in this project; the target
  is full scenario coverage of TS-1 … TS-6 below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Fresh App: no toast, both SFTP channels empty, restart flag clear | `frame_work_pending()` is false | Unit |
| TS-2 | SFTP progress channel holds one event, no toast up | predicate true; the queued event is still receivable afterward (nothing consumed) | Unit |
| TS-3 | SFTP duplicate-check result channel holds one event | predicate true; the queued event is still receivable afterward (nothing consumed) | Unit |
| TS-4 | Restart flag raised | predicate true; the peek does not clear the flag — a subsequent `restart_required()` still returns true | Unit |
| TS-5 | `restart_required()` consume semantics unchanged | true on the first read after a failure, false on the second | Unit |
| TS-6 | Pre-toast pending work exists, no toast up | `next_toast_deadline()` returns a bounded (poll-cadence) deadline; a fully idle App returns none (IMPLEMENTATION.md D1) | Unit |
| TS-7 | AC1 / AC2 idle-window toast behavior | see Manual Testing | Manual |
| TS-8 | `pump_toasts` doc known-limitation paragraph | text reflects the new gating; no remaining "relies on another redraw trigger" claim | Inspection |
| TS-9 | Three check configurations + format check | all exit 0 with zero warnings | Build |
| TS-10 | NFR3 construction constraint | predicate is one atomic read plus channel emptiness checks; no locking beyond what App already holds; evaluation consumes nothing | Inspection (code review) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: none configured beyond compiler warnings (covered by
  TS-9's zero-warning requirement).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Progress toast appears promptly on an idle tab after the first progress event | TS-7 (manual) |
| AC2 | Restart toast arms and displays promptly while the app is idle after a self-spawn failure | TS-7 (manual) |
| AC3 | `pump_toasts` known-limitation doc updated to the new implementation | TS-8 |
| AC4 | Unit tests for the new predicate pass — true for a non-empty SFTP channel and for a set restart flag, each independently | TS-2, TS-3, TS-4 |
| AC5 | Full `--lib` test run passes; all three check configurations complete with zero warnings | Test Verification command + TS-9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-4, TS-5 |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR3 | task0001 | TS-7 (manual; call-site wiring inspected in review) |
| FR4 | task0001 | TS-7 (manual; call-site wiring inspected in review) |
| FR5 | task0001 | TS-6 (decision recorded as IMPLEMENTATION.md D1) |
| FR6 | task0001 | TS-8 |
| FR7 | task0001 | TS-2, TS-3, TS-4 |
| NFR1 | task0001 | TS-5 |
| NFR2 | task0001 | TS-9 |
| NFR3 | task0001 | TS-10 |

## E2E Testing

No E2E infrastructure exists in this project. End-to-end behavior is
validated manually (below), consistent with `test/README.md`.

## Manual Testing (E2E Not Possible)

- [ ] TS-7a (AC1): with an idle tab (cursor blink disabled or the window
      unfocused) and no toast up, start an SFTP upload. The progress toast
      appears promptly after the first progress event arrives — without
      touching the window first.
- [ ] TS-7b (AC2): cause a self-spawn failure via a binary swap (replace the
      on-disk binary while the app runs, then trigger a self-spawn such as
      opening the settings window), with the app otherwise idle. The restart
      toast arms and displays promptly (Phase 7 E-1).
- [ ] TS-8: read the `pump_toasts` doc comment — the known-limitation
      paragraph describes the new predicate-based gating.
- [ ] TS-10: review the predicate's construction — one atomic read plus
      constant-time channel emptiness checks plus the existing toast check;
      nothing consumed; no additional locking.

## Performance / Security Verification

- NFR3 (performance): verified constructively by TS-10; no separate load or
  stress test is specified (per SPEC.md).
- Security: not applicable — the feature changes an internal
  frame-scheduling predicate and processes no external input.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | TS-1 … TS-6 (6) | 6 | 0 | 0 |
| Build / format | TS-9 (1) | 1 | 0 | 0 |
| Manual / inspection | TS-7, TS-8, TS-10 (3) | 0 | 0 | 3 |
| **Total** | **10** | **7** | **0** | **3** |
