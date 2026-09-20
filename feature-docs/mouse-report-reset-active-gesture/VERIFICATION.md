# Verification Document: mouse-report-reset-active-gesture

## Overview

**Feature**: mouse-report-reset-active-gesture /
**SPEC.md**: `feature-docs/mouse-report-reset-active-gesture/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/mouse-report-reset-active-gesture/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. The feature
is a single task; its acceptance criteria live in `tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors
- Additional command (NFR4, feature-gate surface):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0, zero failures, zero pre-existing tests removed or
  weakened
- Coverage target: no coverage tool is configured for this project, so no
  percentage gate applies. The substitute gate is traceability: every
  acceptance criterion in the task plan maps to at least one test listed
  below.
- Single-threaded execution is required by the project's test conventions
  (parallel runs are non-deterministic for parts of this crate).

### Test Scenarios from SPEC.md

TS-1 through TS-9 are SPEC.md's own scenarios. TS-10 through TS-14 are
derived here so that every non-functional requirement has a named
verification item.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Left press over the grid with tracking inactive, then a wheel notch over the grid, then the left release — one records value threaded through the application step | The release names the local selection-completion arm; the left slot survived the notch | Unit |
| TS-2 | Same as TS-1 with a middle press, and again with a right press, in place of the wheel notch | The left release still names the completion arm; the middle / right slot is recorded and cleared independently | Unit |
| TS-3 | Left release with an empty left slot and the drag-in-flight input true | The completion arm, not "nothing" | Unit |
| TS-4 | The focus-loss guard over all four boolean combinations, plus the window-free test `focus_loss_cleanup_publishes_selection_without_a_window` (`src-tauri/src/window_host/tests.rs`), which composes the window-free state core and the recording sink the way the focus-loss adapter composes them, over plain values, with no window host constructed and no selection resolved, plus the destination-predicate truth-table tests prefixed `selection_publish_targets_` in the same module | The guard is true exactly when the drag flag is set or a pending anchor exists; the window-free test clears the drag flag, consumes and returns the pending anchor, requests the publish with the expected destinations and payload, and never runs the fold toggle; the adapter's own gather-and-adapt wiring (`publish_local_drag`) is covered only by the call-site literals the source-scanning test in the same file checks against the event-loop file's text, not by these tests; a recorded sink call in these tests is evidence the publish path requested the write, not evidence of an OS-level PRIMARY selection or CLIPBOARD update | Unit |
| TS-5 | An outcome carrying a tracking-inactive reset applied with left held and its slot recorded | The cell-change cache is reset; the left slot is still recorded as locally owned | Unit |
| TS-6 | An outcome carrying a tracking-inactive reset with a middle slot recorded but middle not held; and an outcome carrying a tab-change reset with left held | The not-held middle slot is cleared; the tab-change reset clears every slot including the held one | Unit |
| TS-7 | The whole pre-existing decision-layer test module, including the byte-exact encoding cases, the release tab-targeting cases and the guard-rejected dispositions | Passes unchanged in meaning; no report byte sequence altered | Unit (existing) |
| TS-8 | A left press at a chrome position (records no owner, starts no drag) followed by its release | No PRIMARY write, no CLIPBOARD write, no fold toggle | Unit |
| TS-9 | Manual reproduction: left press-drag on the grid, wheel one notch without releasing, release left; repeat with a middle press and a right press in place of the notch; repeat with a focus loss in place of the release | The selection stops extending after the release / focus loss and the dragged text pastes back via middle-click; the resize hint and link hover respond again on the next motion | Manual |
| TS-10 | Decision-layer window-freedom and the structural routing test: no winit / egui / GPU / PTY / `term_core` type in any decision-layer signature; the delegate list names the companion application entry point INSTEAD OF the old one, and the existing focus-loss test's full input-bundle literal compiles with the new field | The structural assertion passes with the replaced entry; every new decision-layer test is a bare test constructing no window | Unit / Structural |
| TS-11 | Decision purity: the records value handed to each decision function is compared before and after the call | Unmodified in every case; all record mutation travels in the returned updates | Unit |
| TS-12 | Feature-gate surface: the crate compiles with default features off | Exit code 0 | Build |
| TS-13 | The bounded wheel-report duplication cap and its constant | Unchanged; the existing cap tests pass | Unit (existing) |
| TS-14 | Test-convention conformance: new tests are inline test-module units named `<subject>_<scenario>_<expected>`, constructed per test with no shared global fixture; `src-tauri/Cargo.toml` gains no dependency | Conforms; no dependency diff | Static review |

## Code Quality Verification

- Format: no format command is configured in `workflow.yaml`
  `project.components`. The project's own convention is the pinned rustfmt
  style edition applied to the touched files only — never a crate-wide
  reformat, which would churn unrelated files.
- Static analysis: no separate lint command is configured; the build command
  above is the gate (warnings introduced by this feature are treated as
  findings in review).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-A | All functional requirements FR1–FR6 implemented and tested | The coverage table below, with every row naming the task and at least one test |
| SC-B | All test scenarios TS-1..TS-14 pass | Test Verification section; TS-9 is user-executed |
| SC-C | All SPEC acceptance criteria AC-1..AC-9 satisfied | AC-1/AC-2 → TS-1/TS-2; AC-3 → TS-4 (`focus_loss_cleanup_publishes_selection_without_a_window`, `src-tauri/src/window_host/tests.rs`, which composes the window-free state core and the recording sink the way the focus-loss adapter composes them, over plain values, with no window host constructed and no selection resolved, plus the `selection_publish_targets_` truth-table tests) for the internal state and requested write — the adapter's own gather-and-adapt wiring (`publish_local_drag`) is covered only by the call-site literals the source-scanning test in the same file checks against the event-loop file's text, not by these tests — plus TS-9 (manual) for the real-hardware PRIMARY confirmation; a recorded sink call in TS-4 is evidence of the requested write only, not an OS-level PRIMARY or CLIPBOARD update; AC-4 → TS-4 plus TS-9's motion check; AC-5/AC-6 → TS-5/TS-6; AC-7 → TS-7; AC-8 → TS-8; AC-9 → the TS-1/TS-2/TS-4 tests demonstrably failing before the change |
| SC-D | Security requirements satisfied: the wheel notch cap intact, no new PTY bytes | TS-13 and TS-7 |
| SC-E | The default-features-off build still compiles | TS-12 |
| SC-F | The decision layer remains window-free and the decision functions remain pure | TS-10 and TS-11 |
| SC-G | Documentation complete | IMPLEMENTATION.md, the task plan and this document present and consistent with the delivered change |
| SC-H | Code review completed | The review phase's own record |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-5, TS-6 |
| FR2 | task0001 | TS-1, TS-2, TS-3 |
| FR3 | task0001 | TS-4, TS-9 |
| FR4 | task0001 | TS-7 |
| FR5 | task0001 | TS-8 |
| FR6 | task0001 | TS-5, TS-6 |
| NFR1 | task0001 | TS-10 |
| NFR2 | task0001 | TS-11 |
| NFR3 | task0001 | TS-14 |
| NFR4 | task0001 | TS-12 |
| NFR5 | task0001 | TS-13 |

## E2E Testing

No E2E automation exists in this project and none is added by this feature
(`e2e_test_command` is empty in `workflow.yaml`). Every automated scenario
above is unit-level inside the touched modules.

## Manual Testing (E2E Not Possible)

- [ ] TS-9a: Left press-drag on the terminal grid, turn the wheel one notch
      without releasing, then release left — the selection stops extending and
      the dragged text pastes back with a middle click.
- [ ] TS-9b: The same sequence with a middle-button press, and again with a
      right-button press, in place of the wheel notch — identical outcome.
- [ ] TS-9c: Start a left drag and switch window focus away while still
      holding — on return the selection has not kept extending and the dragged
      text pastes back with a middle click.
- [ ] TS-9d: After each of the above, move the pointer to a window edge and
      over a link — the resize cursor and the link hover both respond, and a
      link printed by fresh PTY output is detected.
- [ ] TS-9e: Press and release on the tab bar, the status bar, the scrollbar
      overlay and the mux sidebar while an earlier selection still exists —
      PRIMARY is not overwritten and no fold toggles.
- [ ] TS-9f: With a mouse-reporting application running, exercise press,
      release, drag and wheel — behaviour is indistinguishable from the
      revision before this change.

No mockup comparison applies: the design step was skipped for this feature
(no user-visible surface change, no design token touched).

## Performance / Security Verification

- NFR5 (security property): the bounded wheel-report duplication cap and its
  constant are unchanged — verified by TS-13 and by inspecting the diff for
  any edit to that helper.
- FR4 (byte invariance): no report sequence gains or loses bytes — verified by
  TS-7's byte-exact cases passing unchanged.
- No new attack surface: the change writes no additional bytes to the PTY,
  persists nothing, and adds no network or WebView surface.
- Performance: no target is stated for this feature. The only work added to
  the pointer hot path is a held-button check inside the application step.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 14 | 12 | 0 | 2 (TS-9, TS-14 static review) |
| Success criteria | 8 | 6 | 0 | 2 (SC-G, SC-H) |
| Requirements coverage | 11 | 11 | 0 | 0 |
