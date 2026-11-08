# Feature: pty-spawn-parallel-budget-flake

## Overview

Make four named `mux::ipc::pty_spawn` tests pass in a parallel `cargo test --lib`
run with a verdict that does not depend on machine load. Three budget tests move
from a wall-clock (`Instant`) budget to a thread-CPU-time `CpuMeter` budget, and
two reattach tests arm the second P2 stop before the first release. A source-scan
guard test detects a return to either old form. Requirements:
`feature-docs/pty-spawn-parallel-budget-flake/REQUIREMENTS.md`.

## Objectives

- The four `mux::ipc::pty_spawn` tests named in the task pass in a parallel
  `cargo test --lib` run, and their verdict does not depend on machine load.
- The on-demand sibling of the reattach test no longer has the same arm-order race.
- A source-scan guard test detects a return to the wall-clock budget or to the
  release-before-arm order.

## User Stories

### US1: Load-independent budget verdicts
As a developer running `cargo test --lib`, I want the three budget tests to judge
production calls by CPU time, so that their verdict does not depend on machine load.

**Acceptance Criteria:**
- [ ] AC-1: The three FR1-FR3 tests contain no `Instant::now` / `.elapsed()`. Each
  measures production calls through a `CpuMeter`, judges the total against
  `BUDGET`, and asserts that the total is greater than zero. (FR1, FR2, FR3)
- [ ] AC-4: One ordinary run of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  does not fail any of the five target tests or the guard. (FR1-FR6)
- [ ] AC-5: The serial run
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests -- --test-threads=1`
  passes the five target tests and the guard. (FR1-FR6)

### US2: No arm-order race in the reattach tests
As a developer running `cargo test --lib`, I want both reattach tests to arm the
second P2 stop before the first release, so that the reader cannot pass the second
P2 before the re-arm.

**Acceptance Criteria:**
- [ ] AC-2: In both FR4/FR5 reattach tests, the second `p2.arm()` comes before the
  first `release_tx.send`, and the `recv_timeout` values are unchanged. (FR4, FR5)

### US3: Regression guard
As a developer running `cargo test --lib`, I want a source-scan guard test, so that
a return to the wall-clock budget or to the release-before-arm order fails a test.

**Acceptance Criteria:**
- [ ] AC-3: The guard test passes on the changed source. Its negative-example unit
  tests reject the wall-clock form, reject the release-first order, and report a
  missing function name. (FR6)

## Technical Requirements

### Functional Requirements
- **FR1:** CPU-time budget for
  `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`.
  In `escape_state_carry.rs`, replace the `Instant` budget (`Instant::now` at 774,
  elapsed check at 872) with a `CpuMeter`. The meter measures the production
  write-filter calls (`ScrollbackWriteFilter::feed_with_cuts` reached through
  `feed_pieces`), and its total is judged against the shared `BUDGET` with
  `CpuMeter::judge`. The test asserts that the verdict is within the budget and that
  the measured total is greater than zero.
- **FR2:** CPU-time budget for
  `post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget`.
  In `post_strip_cut_csi.rs`, replace the `Instant` budget (776 / 846) with a
  `CpuMeter`. The meter measures the production write-filter calls
  (`feed_with_cuts` reached through `run_pieces` and `emitted_through`), and its
  total is judged against `BUDGET`. The test asserts that the verdict is within the
  budget and that the measured total is greater than zero.
- **FR3:** CPU-time budget for
  `open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal`.
  In `strip_open_string_body_closure.rs`, replace the `Instant` budget (235 / 244)
  with a `CpuMeter`. The meter measures the production calls inside
  `check_a_flushed_open_body_is_closed_at_the_next_removal` (`overflow()` and
  `feed_with_cuts`). The term_core oracle replay (`assert_replays_like_raw`) is not
  measured. The total is judged against `BUDGET`. The test asserts that the verdict
  is within the budget and that the measured total is greater than zero. The OSC
  sibling test that shares the helper (219-228) keeps its own `Instant` budget and
  its behavior.
- **FR4:** Arm order in
  `visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once`.
  In `tests.rs`, arm the second P2 stop (`output_capture.p2.arm()`, currently at
  5142) before the first release (`release_tx.send`, currently at 5137), while the
  reader is still paused at the first P2. This follows the pattern at
  `tests.rs:1893-1894`. The 5s `recv_timeout` values stay unchanged.
- **FR5:** Arm order in
  `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`.
  In `tests.rs`, arm the second P2 stop (currently at 5408) before the first release
  (currently at 5406), the same change as FR4. The 5s `recv_timeout` values stay
  unchanged.
- **FR6:** Source-scan guard test. Add a test that reads the source of the target
  tests and fails in each of these cases:
  (a) one of the three FR1-FR3 tests cannot be found by name;
  (b) one of their bodies judges with `Instant::now` / `.elapsed()` instead of the
  `CpuMeter` judgment, or lacks `CpuMeter::new`, `.measure(` or `.judge(BUDGET)`;
  (c) one of the two FR4/FR5 tests cannot be found, or has fewer than two
  `p2.arm()` calls;
  (d) in one of those two tests, the first `release_tx.send(` comes before the
  second `p2.arm()`.
  The judging logic is a pure function over a source string, with unit tests for
  negative examples: the old wall-clock form and the release-first order are
  rejected, the new forms are accepted, and a missing function name is reported as
  missing.

### Non-Functional Requirements
- **NFR1 - CPU-time budget:** The budgets of FR1-FR3 are judged as thread CPU time
  through `CpuMeter`. No new wall-clock budget is added.
- **NFR2 - No intentional CPU load:** Investigation, implementation and
  verification put no intentional CPU load: no load loops, no stress tools, no
  repeated or core-count parallel runs. Single ordinary runs only.
- **NFR3 - Unchanged surroundings:** Production code, test names, workloads, output
  assertions and the shared `BUDGET` / `TARGETS` / `Target` stay unchanged.
  Wall-clock checks other than FR1-FR3 stay unchanged.
- **NFR4 - Platforms:** The changed tests and the guard compile and judge on Linux
  and Windows.

## Implementation Approach

### Architecture

Only test code under `src-tauri/src/mux/ipc/pty_spawn/tests` changes. There is no
UI or visual output, and no production code changes (NFR3).

**Component Diagram:**
```
FR1-FR3 budget tests
  CpuMeter::new -> .measure(<production call>) ... -> .judge(BUDGET)
                                                     -> assert within budget
                                                     -> assert measured total > 0
  Not measured: input building, assertions, term_core oracle replay

FR4/FR5 reattach tests
  reader paused at first P2 -> output_capture.p2.arm() (second stop)
                            -> release_tx.send (first release)

FR6 guard test
  source string -> pure judging function -> pass / reject / report missing
```

### Data Flow

```
FR1: feed_pieces -> ScrollbackWriteFilter::feed_with_cuts   [measured]
FR2: run_pieces / emitted_through -> feed_with_cuts          [measured]
FR3: check_a_flushed_open_body_is_closed_at_the_next_removal
       -> overflow(), feed_with_cuts                         [measured]
       -> assert_replays_like_raw                            [not measured]
```

### API Design

Not applicable.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `CpuMeter` (`CpuMeter::new`, `.measure(`, `CpuMeter::judge`): thread-CPU-time
  budget judgment used by FR1-FR3.
- Shared `BUDGET` (10s), `TARGETS`, `Target`: unchanged (NFR3).
- Shared helpers `feed_pieces`, `run_pieces`,
  `check_a_flushed_open_body_is_closed_at_the_next_removal`: if one gains a meter
  parameter, its other callers keep their behavior. The exact measuring form is left
  to create-plan.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/mux/ipc/pty_spawn/tests/
├── escape_state_carry.rs                # FR1
├── post_strip_cut_csi.rs                # FR2
├── strip_open_string_body_closure.rs    # FR3
└── tests.rs                             # FR4, FR5
```

The FR6 guard test is added under the same tests code; its file is not fixed by the
requirements.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/pty-spawn-parallel-budget-flake/**`
- `test-docs/pty-spawn-parallel-budget-flake/**`

`feature-docs/pty-spawn-parallel-budget-flake/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/pty-spawn-parallel-budget-flake/**` covers
`test-docs/pty-spawn-parallel-budget-flake/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites it
and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/pty-spawn-parallel-budget-flake/` directory at all; the declared
`test-docs/pty-spawn-parallel-budget-flake/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1: Run the three FR1-FR3 tests - They pass, and each total is greater than
  zero and below `BUDGET`. (AC-1)
- [ ] TS-2: Run the two FR4/FR5 reattach tests - They pass. (AC-2)
- [ ] TS-3: Run the guard test on the current source - It passes. (AC-3)
- [ ] TS-4: Run the guard's negative examples (Instant budget snippet, release-first
  snippet, missing function name) - Each is rejected or reported. (AC-3)

### Integration Tests
- [ ] TS-5: Run the `--lib` suite once - None of the target tests or the guard
  fails. (AC-4)
- [ ] TS-6: Run the serial `pty_spawn::tests` run - The targets and the guard pass.
  (AC-5)

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- The negative examples of the guard are covered by TS-4.

### Performance Tests
- No load or stress tests (NFR2).

## Security Considerations

- Not applicable. Only test code changes.

## Error Handling

### Error Codes

Not applicable.

### Error Flow

```
Guard finds a missing target test        -> guard fails, reports the missing name
Guard finds Instant / missing CpuMeter   -> guard fails
Guard finds < 2 p2.arm() or release first -> guard fails
```

## Performance Optimization

### Performance Goals
- FR1-FR3: the measured thread CPU time of the production calls is within
  `BUDGET` (10s).

### Optimization Strategies
- Not applicable.

### Caching Strategy
- Not applicable.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC-1 through AC-5 are satisfied

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None.

## Assumptions

- A1: q-parallel-scope = target_and_same_race (batch-codex-consultation). The scope
  is the four named tests plus the identical release-then-arm order in
  `on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once`
  (`tests.rs:5406/5408`). The other wall-clock checks are not converted. Some are
  deadlock-detection deadlines over other threads (`tests.rs:2406`, `2735`), and
  their meaning would change.
- A2: q-regression-guard = source_scan_guard (batch-codex-consultation). Recurrence
  is detected by the FR6 source-scan guard, which fails on a missing target function
  and carries negative examples. The presence of `judge` alone does not prove that
  the calls were measured. Each converted test therefore also asserts a non-zero
  measured total, and the guard requires `.measure(` in the body.
- A3: q-design-step = skip (batch-decision-table).
- A4: The budget measures only production calls (strip / write-filter / overflow).
  Input building, assertions and term_core oracle replays are not measured.
  `BUDGET` stays 10s.
- A5: The reattach failures are an ordering race: the reader passes the second P2
  before the re-arm. They are not a budget issue. Arming while the reader is blocked
  in `PauseHook::hit` affects only the next hit.
- A6: No test is renamed, so no update to test-docs records is needed
  (`.claude/rules/test-docs-records.md`).
- A7: If a shared helper (`feed_pieces`, `run_pieces`,
  `check_a_flushed_open_body_is_closed_at_the_next_removal`) gains a meter
  parameter, its other callers keep their behavior. The exact measuring form is left
  to create-plan.

## Implementation Phases (if applicable)

Not applicable.

## References

- Requirements document: `feature-docs/pty-spawn-parallel-budget-flake/REQUIREMENTS.md`
- Test-docs record rule: `.claude/rules/test-docs-records.md`
