# Verification Document: Revert the status bar's agent-status summary

## Overview

**Feature**: status-bar-agent-summary-revert /
**SPEC.md**: `feature-docs/status-bar-agent-summary-revert/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/status-bar-agent-summary-revert/IMPLEMENTATION.md`

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings
- Additional (CLI-only feature gate):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 — the removal must not disturb the non-GUI build

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Coverage target: not tracked numerically in this project. The requirement is
  no net loss of status-bar coverage: every pre-existing status-bar test other
  than the eight summary-specific ones still runs and passes.
- Known flakiness: the replay tests in the tabs module are non-deterministic
  under parallel execution. A failure there is attributable to this feature only
  if it also reproduces on the implement phase's base commit.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Status bar disabled in the view model | Visible row count is 0 and no panel is painted | Unit |
| TS-2 | App Line 1 template empty, App Line 2 and OSC empty, agent states present in the model | Visible row count is 0; the status bar reserves no height | Unit |
| TS-3 | App Line 1 template resolves to non-empty content | App Line 1 is visible and paints only its left/right sections — no dot, count, or agent-state color | Unit |
| TS-4 | App Line 2 configured while App Line 1 is empty | App Line 2 renders as the single visible row | Unit |
| TS-5 | Panel height for a given view model | Equals row height × visible row count | Unit |
| TS-6 | OSC row rendering and App-row section truncation/emoji behavior | Unchanged from before the feature; existing tests pass without assertion changes | Unit |
| TS-7 | Agent-status badge projections used by the tab bar and mux sidebar | Still present and behaving as before; their tests pass | Unit |
| TS-8 | Whole crate compiles in default and `--no-default-features` configurations | Both succeed with no new warnings | Integration |
| TS-9 | `doc/AGENT-STATUS.md` content | No claim that agent status appears in or can mislead the status bar; other surfaces still documented | Manual |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: covered by the build commands above (warning-free build is
  the acceptance bar); no separate linter is configured for this crate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | No agent-state element rendered in the status bar | TS-3 plus a source search confirming no agent-status identifier remains in the status-bar widget |
| SC-2 | App Line 1 visibility depends only on its resolved template content | TS-2, TS-3, TS-4 |
| SC-3 | Restored public signatures with all call sites updated | TS-8 (the crate does not compile otherwise) |
| SC-4 | No dead code left behind from the summary | TS-8 warning-free requirement plus a source search for the removed identifiers |
| SC-5 | Tab-bar and sidebar badges continue to work | TS-7 |
| SC-6 | Documentation matches the shipped UI | TS-9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-3, TS-8 |
| FR2 | task0001 | TS-2, TS-3, TS-4 |
| FR3 | task0001 | TS-5, TS-8 |
| FR4 | task0001 | TS-8 |
| FR5 | task0001 | TS-8 |
| FR6 | task0001 | TS-5, TS-8 |
| FR7 | task0001 | TS-7, TS-8 |
| FR8 | task0001 | TS-6 |
| FR9 | task0002 | TS-9 |
| NFR1 | task0001 | TS-8 (no aggregation call remains on the frame path) |
| NFR2 | task0001 | TS-8 plus the absence of any settings/protocol file in either task's scope |
| NFR3 | task0001 | TS-8 |
| NFR4 | task0001, task0002 | TS-7, TS-9, and the scope lists in both task plans |

## E2E Testing

The project has no E2E harness for the native terminal window (no
`e2e_test_command` in workflow.yaml), and the status bar is drawn by the
wgpu+egui pipeline, which has no automatable UI driver. No E2E scenarios apply.

## Manual Testing (E2E Not Possible)

- [ ] MT-1: Launch the terminal with no App Line 1 template configured, run an
      agent that reports state in a mux pane, and confirm the status bar does not
      gain a row or show any dot/count.
- [ ] MT-2: With an App Line 1 template configured, confirm the row shows only
      the template's content and its right section reaches the panel edge.
- [ ] MT-3: Confirm the tab bar and the mux sidebar still show agent-status
      badges while the agent runs.

Manual items require a release build the user runs; they are reported as
outstanding rather than executed by the verify phase.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 9 | 8 | 0 | 1 |
| Success criteria | 6 | 5 | 0 | 1 |
| Manual UI checks | 3 | 0 | 0 | 3 |
