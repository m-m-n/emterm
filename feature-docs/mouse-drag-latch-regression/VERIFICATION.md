# Verification Document: mouse-drag-latch-regression

## Overview

**Feature**: mouse-drag-latch-regression
**SPEC.md**: `feature-docs/mouse-drag-latch-regression/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mouse-drag-latch-regression/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/mouse-drag-latch-regression/tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings attributable to this feature.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0; the five new scenarios (TS-1 … TS-5) are present in
  the run and pass, and no previously passing test fails.
- Coverage target: the project runs no coverage instrumentation, so no
  percentage gate applies. The coverage criterion for this feature is
  MECHANISM coverage instead: all four named mechanisms
  (reset-clears-held-gesture-slot, held-unaware-apply-entry-point,
  ungated-no-owner-left-release, focus-loss-gate-inversion) are each guarded
  by at least one new test that fails when that mechanism regresses.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Wheel notch applied through the wheel path's apply-and-fold unit with left reported held, during a live left drag, then a left release | The held left gesture slot survives the tracking-inactive reset; the release names the selection-completion arm; the termination seam leaves the drag flag false; PRIMARY is recorded as published | Unit |
| TS-2 | A second (middle or right) button press applied through the held-aware apply with left reported held, during a live left drag, then a left release | The held left gesture slot survives; the release names the selection-completion arm; the drag flag ends false; PRIMARY is recorded as published | Unit |
| TS-3 | Left release decided from empty records with the drag-in-flight signal true, then the same release with the signal false, then a middle and a right release with the signal true | True → the selection-completion arm; false → nothing; middle and right → nothing in both directions | Unit |
| TS-4 | Focus loss while the left button is still physically held, followed by the later left release | The focus-loss predicate answers "do not terminate"; the later release decided from the emptied records still names the selection-completion arm; the drag flag ends false; PRIMARY is recorded as published | Unit |
| TS-5 | Focus loss with the left button not held | The focus-loss predicate answers "terminate"; the termination seam leaves the drag flag false, hands back the pending anchor, and PRIMARY is recorded as published | Unit |
| TS-6 | Production-code invariance of the change set | The feature's diff touches no file under `src-tauri/src/` other than test-module content inside `src-tauri/src/window_host/tests.rs`; no dependency manifest changes | Automated (change-set inspection) |
| TS-7 | The two existing focus-loss pins stay green and unmodified | `should_terminate_drag_terminates_only_when_in_flight_and_left_not_held` and `focus_loss_arm_never_calls_the_fold_click_toggle` appear unchanged in the diff and pass in the test run | Automated (change-set inspection + test run) |
| TS-8 | Feature-gate integrity | The CLI-only compile check passes, showing the new tests did not leak out of the GUI-gated module | Automated |
| TS-9 | Determinism and parallel safety | The five new scenarios pass when run by name filter WITHOUT a single-thread constraint, repeated three times, with no failure and no measurable runtime cost | Automated |
| TS-10 | Test-authoring conventions | Each new test carries a doc comment naming exactly one of the four mechanism labels; names follow the repository pattern; assertions target observable contracts only; no window-host, winit, GPU-surface, PTY or terminal-mode type is named by the new tests; no new file, compile unit or test dependency was added | Inspection |

Command notes:

- TS-8 uses the project's documented CLI-only check:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- TS-9 runs the same test binary as the Test Verification command above,
  filtered to the new scenario names and without the single-thread option.
- TS-6 and TS-7 inspect the feature branch's diff against the implement
  step's base commit.

## Code Quality Verification

- Format: `make fmt-check`
- Static analysis: none configured beyond the compiler for this crate; the
  build command above carries the compiler's own diagnostics.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The library test suite passes with the new tests included | Test Verification command; exit code 0 |
| AC-2 | A test exists that fails if the tracking-inactive reset goes back to clearing every gesture slot | TS-1 and TS-2, each confirmed by the reverted mutation probe recorded in the task's test record |
| AC-3 | A test exists that fails if either the button path or the wheel path stops passing the live held-button value to the held-aware apply entry point | TS-1 (wheel path) and TS-2 (button path), each confirmed by a reverted mutation probe |
| AC-4 | A test exists that fails if the no-owner left release stops consulting the drag-in-flight signal | TS-3, confirmed by a reverted mutation probe |
| AC-5 | A test exists that fails if the focus-loss arm terminates a drag whose left button is still held, and a test exists that fails if it stops terminating a drag whose left button is not held | TS-4 and TS-5 |
| AC-6 | The feature's diff touches no production behaviour under `src-tauri/src/` | TS-6 |
| AC-7 | The two existing focus-loss tests pass unmodified | TS-7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3 |
| FR2 | task0001 | TS-2, TS-3 |
| FR3 | task0001 | TS-4, TS-5 |
| FR4 | task0001 | TS-6, TS-M1 |
| FR5 | task0001 | TS-4, TS-7 |
| FR6 | task0001 | TS-10 |
| NFR1 | task0001 | TS-10 |
| NFR2 | task0001 | TS-6, TS-10 |
| NFR3 | task0001 | TS-10 |
| NFR4 | task0001 | TS-9 |
| NFR5 | task0001 | TS-10 |
| NFR6 | task0001 | TS-9 |
| NFR7 | task0001 | TS-8 |

## Manual Testing (E2E Not Possible)

The project has no E2E harness for the native terminal surface, so the
reproduction scenario is verified by hand. The design step was skipped for
this feature, so there is no mockup comparison item.

- [ ] TS-M1: launch a release build, start a left-drag selection on the grid,
      turn the wheel one notch (and, in a second pass, press a second button
      instead), release the left button, and confirm that pointer movement no
      longer extends the selection and that the selection reached PRIMARY.
      Repeat the same sequence substituting "move focus away from the window"
      for step 3. Expected to pass already at the base revision, because the
      production repair predates this feature (A3).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit regression scenarios (TS-1 … TS-5) | 5 | 5 | 0 | 0 |
| Invariance and convention checks (TS-6 … TS-10) | 5 | 4 | 0 | 1 |
| Manual reproduction (TS-M1) | 1 | 0 | 0 | 1 |
| Total | 11 | 9 | 0 | 2 |
