# Verification Document: focus-loss-drag-cleanup-test

## Overview

**Feature**: focus-loss-drag-cleanup-test
**SPEC.md**: `feature-docs/focus-loss-drag-cleanup-test/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/focus-loss-drag-cleanup-test/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Per-task
acceptance criteria live in `feature-docs/focus-loss-drag-cleanup-test/tasks/`.

## Build Verification

- Command (main component):
  `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml`
- Command (cli_only component):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, no new warnings introduced by this feature.
- Run both from the project root; do not change directory into the crate.

## Test Verification

- Command (full suite):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- Command (window-free subset this feature adds):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0. Every pre-existing test passes unmodified; the suite
  differs from the baseline only by added tests.
- Coverage target: the project defines no coverage gate. The gate for this
  feature is requirement coverage instead — every FR and NFR below must map to
  at least one passing scenario.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Destination predicate truth table: all eight combinations of (selection present, copy-on-select, text empty) | Each combination asserts the destination set of the preserved write rule: no selection writes nothing; a present selection always writes PRIMARY including empty text; CLIPBOARD only when copy-on-select is on and the text is non-empty | Unit |
| TS-2 | State core effects with and without a live drag | With the drag flag set and an anchor pending, the core clears the flag and returns the consumed anchor; with neither set it returns the empty anchor and leaves the state clean | Unit |
| TS-3 | Recording sink payloads | Publishing a non-empty selection with copy-on-select on records exactly two calls, PRIMARY then CLIPBOARD, carrying identical text; with copy-on-select off it records exactly one PRIMARY call | Unit |
| TS-4 | Focus-loss cleanup end to end with no winit window | The guard reports true, the publish half runs, and one test asserts all three effects: drag flag cleared, pending anchor consumed and returned, selection published with the recorded payload equal to the resolved text | Unit |
| TS-5 | Empty-selection edge | A resolved selection whose text is empty still records a PRIMARY call and records no CLIPBOARD call even with copy-on-select on | Unit |
| TS-6 | The three existing source-scanning tests pass unmodified | The fold-toggle exclusion test, the forwarded-branch clear test and the copy-chord clipboard adjacency test all pass with no edit to their assertions | Regression |
| TS-7 | CLI-only feature check | The no-default-features check still succeeds, confirming no window-only type leaked into CLI-shared code | Build |
| TS-8 | Real-hardware PRIMARY / CLIPBOARD confirmation for the prior feature's AC-3 | Selecting text, removing focus and middle-click pasting elsewhere pastes the selection; a recorded sink call is not a substitute for this | Manual |

## Code Quality Verification

- Format: `make fmt`, kept scoped to the touched files. A crate-wide format
  run is forbidden by project rules; if one happens, revert every file outside
  this feature's change set.
- Static analysis: none configured beyond the compiler. The CLI-only check
  above doubles as the feature-gate check.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements FR1-FR8 are implemented and tested | The coverage table below has a task and a scenario for every ID |
| SC-2 | TS-1 through TS-7 pass; TS-8 is executed manually and recorded | Test and build commands above exit 0; the manual checklist below is completed and recorded |
| SC-3 | The SPEC's AC-1 through AC-6 are satisfied | Read each acceptance criterion against the merged tree; AC-4 and AC-5 additionally by the regression run |
| SC-4 | Production behaviour is unchanged | The full existing suite passes unmodified, including the three source-scanning tests; no destination, payload, ordering, state mutation or return value differs |
| SC-5 | Every new test runs window-free and none constructs the window host type | The library test target passes with no display server available; inspect the new tests for any window construction |
| SC-6 | The backfilled rows cite tests that exist | Search the merged tree for `focus_loss_cleanup_publishes_selection_without_a_window` and for the prefix `selection_publish_targets_`; each citation in the prior feature's documents resolves to a test in `src-tauri/src/window_host/tests.rs` |
| SC-7 | The prior feature's documents are backfilled and its AC-3 remains manual | Inspect the prior feature's SPEC.md TS-4 checkbox and VERIFICATION.md TS-4 / SC-C rows; confirm the write-request caveat and the unchanged manual mapping |
| SC-8 | Formatting stays scoped to the touched files | The feature's change set contains only the files declared by the two tasks |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-4 |
| FR2 | task0001 | TS-6 |
| FR3 | task0001 | TS-1, TS-5 |
| FR4 | task0001 | TS-3, TS-4, TS-7 |
| FR5 | task0001 | TS-4 |
| FR6 | task0001 | TS-1, TS-3, TS-5 |
| FR7 | task0001 | TS-6 |
| FR8 | task0002 | TS-8, plus the SC-6 and SC-7 document inspections |
| NFR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5 |
| NFR2 | task0001 | TS-1, TS-2 |
| NFR3 | task0001 | TS-6, TS-7 |
| NFR4 | task0001 | TS-6 |

### Verification Index

```yaml
verification_index:
  TS-1: [FR3, FR6, NFR1, NFR2]
  TS-2: [FR1, NFR1, NFR2]
  TS-3: [FR4, FR6, NFR1]
  TS-4: [FR1, FR4, FR5, NFR1]
  TS-5: [FR3, FR6, NFR1]
  TS-6: [FR2, FR7, NFR3, NFR4]
  TS-7: [FR4, NFR3]
  TS-8: [FR8]
```

## E2E Testing

Not applicable. The repository has no E2E infrastructure and the workflow's
`e2e_test_command` is empty for both components, so no E2E scenario is added.

## Manual Testing (E2E Not Possible)

- [ ] TS-8: select text in the terminal, remove window focus, then middle-click
      paste into another application and confirm the selection arrives. This
      confirms the prior feature's AC-3 at the OS level, which a recorded sink
      call cannot do.
- [ ] SC-7 document inspection: the prior feature's SPEC.md TS-4 checkbox and
      VERIFICATION.md TS-4 / SC-C rows cite this feature's tests, record that
      a recorded sink call is evidence of the write REQUEST only, and leave
      that feature's AC-3 mapped to a manual scenario.
- [ ] SC-6 citation check: every test identifier cited by those rows exists in
      `src-tauri/src/window_host/tests.rs`.

## Performance / Security Verification

Not applicable. The feature adds no runtime work to the shipped binary
(NFR3 requires observational neutrality) and touches no authenticated
surface, no external input parsing and no persisted data. The selection text
handled by the sink is the same data the current publish path handles.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit scenarios | 5 (TS-1..TS-5) | 5 | 0 | 0 |
| Regression scenarios | 1 (TS-6) | 1 | 0 | 0 |
| Build scenarios | 1 (TS-7) | 1 | 0 | 0 |
| Manual scenarios | 1 (TS-8) | 0 | 0 | 1 |
| Success criteria | 8 (SC-1..SC-8) | 5 | 0 | 3 |
