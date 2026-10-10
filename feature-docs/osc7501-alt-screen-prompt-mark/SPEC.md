# Feature: osc7501-alt-screen-prompt-mark

## Overview

`apply_program_status_feed` (`src-tauri/src/tabs/output_pipeline.rs:188-208`)
matches prompt-start candidates (OSC 133 A) against `live_marks` front to back
by kind only. When one processing unit contains "alt-screen A -> leave alt
screen -> OSC 7501 report -> main-screen A", the alt-screen A consumes the only
main-screen A, and the main-screen A after the report is ignored. The working /
blocked record then remains until the next prompt start.

Candidates are pushed by the OSC 133 branch in `callbacks.rs` regardless of
whether the alt screen is active. `term_core` excludes OSC 133 received under
`MODE_ALT_SCREEN` from the live marks.

Requirements document: `feature-docs/osc7501-alt-screen-prompt-mark/REQUIREMENTS.md`.

## Objectives

- When one processing unit contains, in this order, an alt-screen OSC 133 A, a
  return from the alt screen, an OSC 7501 report, and a main-screen OSC 133 A,
  the main-screen A after the report discards the working / blocked / idle
  record.

## Technical Requirements

### Functional Requirements

- **FR1:** Alt-screen candidates are excluded from matching. OSC 133 candidates
  received on the alt screen (the alternate screen enabled by `?47` / `?1047` /
  `?1049`) are never used for matching against the OSC 7501 record table,
  regardless of kind or order. They do not consume main-screen marks.
- **FR2:** Main-screen A is applied in order. An OSC 133 A received on the main
  screen is applied to the record table in its order relative to the reports,
  even when an alt-screen A precedes it in the same processing unit. It
  discards working / blocked / idle records and keeps done / error records.
- **FR3:** Existing behavior is preserved. The following existing behavior does
  not change:
  - Candidates before the last RIS in the same processing unit are not applied.
  - An alt-screen A does not discard any record.
  - A mark other than A discards nothing.
  - For a tab connected to mux at the start of processing, OSC 7501 feed built
    from the inner content is dropped.

### Non-Functional Requirements

- **NFR1 - Processing cost:** Processing the feed of one processing unit
  finishes in time proportional to the feed length, does not block, and
  performs no I/O.
- **NFR2 - Other paths preserved:** The behavior of the mux-connected path and
  of the response to OSC 7501 queries does not change.

## Implementation Approach

### Scope

- Target: the plain-tab OSC 7501 path — `apply_program_status_feed` and the
  part that supplies candidates to it (the OSC 133 branch in `callbacks.rs`).
- Alt-screen detection uses the same criterion as `term_core`'s
  `MODE_ALT_SCREEN` (shared by `?47` / `?1047` / `?1049`).
- Out of scope:
  - The same kind-only matching defect in the agent status latch
    (`src-tauri/src/agent_status_model.rs` `reconcile_latch_feed`,
    `LatchFeedEvent::PromptMark`).
  - The OSC 7501 record table on the mux daemon side.

### Data Flow

```
PTY bytes -> process_combined
          -> callbacks.rs OSC 133 branch (prompt-start candidates)
          -> apply_program_status_feed (output_pipeline.rs)
          -> OSC 7501 record table
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/osc7501-alt-screen-prompt-mark/**`
- `test-docs/osc7501-alt-screen-prompt-mark/**`

`feature-docs/osc7501-alt-screen-prompt-mark/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/osc7501-alt-screen-prompt-mark/**` covers
`test-docs/osc7501-alt-screen-prompt-mark/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Acceptance Criteria

- [ ] **AC-1** (FR1, FR2): After feeding
  `\x1b[?1049h\x1b]133;A\x07\x1b[?1049l\x1b]7501;id=job:state=working\x07\x1b]133;A\x07`
  to a plain tab in a single `process_combined` call, the record table is
  empty and the pending summary changes are `[Some(working), None]`.
- [ ] **AC-2** (FR1): Using `?1047` or `?47` for the alt-screen switch gives
  the same result as AC-1.
- [ ] **AC-3** (FR1, FR2): When one processing unit contains, in this order, a
  main-screen A, an alt-screen A, a return, a report, and a main-screen A, the
  report's record is discarded by the last main-screen A.
- [ ] **AC-4** (FR3, NFR2): The existing osc7501 / program_status_feed tests in
  `src-tauri/src/tabs/tests/output_pipeline.rs` and
  `src-tauri/src/callbacks/tests.rs` all pass without being renamed.
- [ ] **AC-5** (FR1, FR2, FR3):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes in full, and
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  passes.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (FR1, FR2): Feed the AC-1 reproduction bytes in one
  `process_combined` call - the record table is empty and the pending summary
  changes are `[Some(working), None]`.
- [ ] **TS-2** (FR1): Feed the same sequence with the alt screen switched by
  `?1047` and by `?47` - the result matches AC-1.
- [ ] **TS-3** (FR1, FR2): Feed main-screen A, `?1049h`, alt-screen A,
  `?1049l`, a working report, and main-screen A in one call - the record table
  is empty.
- [ ] **TS-4** (FR1): Feed two alt-screen A marks, a return, a working report,
  and a main-screen A in one call - the record table is empty.

### Integration Tests

- [ ] **TS-5** (FR3, NFR1, NFR2): Run the full `--lib` suite including the
  existing osc7501_* tests and program_status_feed tests - all pass.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] A main-screen A precedes the alt-screen A in the same processing unit
  (TS-3).
- [ ] Multiple alt-screen A marks precede the main-screen A (TS-4).

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 through AC-5 are satisfied

## Assumptions

- **A1:** The target is limited to the plain-tab OSC 7501 path
  (`apply_program_status_feed` and the part that supplies candidates to it).
  The behavior of dropping feed built from inner content on mux-connected tabs
  does not change.
- **A2:** Alt-screen detection uses the same criterion as `term_core`'s
  `MODE_ALT_SCREEN` (shared by `?47` / `?1047` / `?1049`).
- **A3:** The existing osc7501_* tests pass without being renamed. New tests
  are added for regression detection.
- **A4:** The same defect in `reconcile_latch_feed` is out of scope for this
  feature.

## Open Questions

None.

## References

- Requirements: `feature-docs/osc7501-alt-screen-prompt-mark/REQUIREMENTS.md`
- Source ticket: Notion bug "OSC 7501: alt-screen OSC 133 A consumes the
  main-screen prompt start, and the working / blocked record remains"
  (osc7501-program-status review round 1 medium, stable_id c07288af79f66c3b)
