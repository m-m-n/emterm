# Feature: flaky-frame-work-pending-test

## Overview

`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
(`src-tauri/src/app/tests/timing.rs`, around line 135, assertion
`assert!(app.frame_work_pending())`) fails when the `emterm` crate's `--lib` suite runs
at cargo's default test parallelism and passes when it runs with `-- --test-threads=1`.
This feature identifies the shared or process-global state the test contends for,
eliminates that order dependence at its source, and records the diagnosis — without
weakening the assertion, hiding the test, or lowering the suite's parallelism.

Requirement definitions and their provenance live in `REQUIREMENTS.md`; this document is
the implementation-facing rendering of the same requirements.

## Objectives

- **OBJ-01**: Make the `--lib` test suite of the `emterm` crate pass reliably at cargo's
  default test parallelism, so that unattended (batch) workflow runs stop exiting
  non-zero on a failure unrelated to the feature under test.
- **OBJ-02**: Remove the recurring verification cost of proving, per feature, that
  `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
  fails on the base commit as well and is therefore not caused by the feature's own
  changes.
- **OBJ-03**: Leave a durable record of which shared or global state the test was
  contending for, and whether the fix landed in the test or in the production code, so
  the same class of order dependence is recognizable next time.

## User Stories

### US1: A trustworthy `--lib` result at default parallelism

As a developer running the `emterm` crate's `--lib` suite, I want the suite to pass at
cargo's default parallelism, so that a failure tells me something about the code under
test rather than about test scheduling.

**Acceptance Criteria:**
- [ ] AC-01: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      run three consecutive times at cargo's default parallelism (no `--test-threads`
      override) completes with 0 failed on every run.
- [ ] AC-03: `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
      still appears in the executed test list of the default `--lib` run (not
      `#[ignore]`d, not removed).
- [ ] AC-07: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds.

### US2: A fix that keeps the test's proving power

As a developer relying on this test, I want the fix to address the contended state
rather than the assertion, so that the test still proves what it proved before.

**Acceptance Criteria:**
- [ ] AC-02: The failing assertion at `src-tauri/src/app/tests/timing.rs:135` still
      asserts `app.frame_work_pending()` under the same scenario (restart flag raised,
      nothing consumed); the assertion has not been removed, inverted, or made
      conditional.
- [ ] AC-06: No change lands that sets `-- --test-threads=1` for the `--lib` suite in
      `workflow.yaml`, CI configuration, `test/README.md`'s unit-test section, or the
      project's documented test command.

### US3: A durable record of the diagnosis

As a developer meeting this failure class again, I want the contended state and the fix
location written down, so that I recognize the pattern instead of re-deriving it.

**Acceptance Criteria:**
- [ ] AC-04: The root cause is written down: the specific shared/global state, the other
      test(s) that mutate it, and the mechanism by which the interleaving makes
      `frame_work_pending()` observe `false`.
- [ ] AC-05: The record states explicitly whether the fix was applied to the test side
      (isolation / reset / serialization) or the production-code side (state management
      revision).

## Technical Requirements

### Functional Requirements

- **FR1 — Identify the shared state behind the order dependence:** Determine why
  `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
  (`src-tauri/src/app/tests/timing.rs`, around line 135, assertion
  `assert!(app.frame_work_pending())`) fails under parallel execution and passes under
  `-- --test-threads=1`. The outcome must name the concrete shared or process-global
  state that other tests in the same binary mutate concurrently (for example a `static`
  / `OnceLock` / atomic flag, a process-wide palette or settings slot, an environment
  variable, or a shared filesystem/socket path). *(status: resolved)*
- **FR2 — Eliminate the order dependence at its source:** Fix the identified contention
  either by isolating the test (resetting or serializing the shared state, or giving the
  unit under test its own instance of it) or by revising the production code's state
  management so the state is no longer process-global. Both directions are acceptable;
  the choice is an implementation decision. *(status: resolved)*
- **FR3 — Record the diagnosis and the chosen fix location:** Document, in the feature's
  own documents, the identified shared state, the tests that were contending for it, and
  whether the fix was applied to the test side or the production-code side. *(status:
  resolved)*
- **FR4 — Preserve the assertion's strength:** The existing assertion semantics of the
  test are preserved. Removing the assertion, relaxing it, retrying until green, or
  otherwise weakening what the test proves does not satisfy this feature. *(status:
  resolved)*
- **FR5 — Do not hide the test:** Marking the test `#[ignore]`, deleting it, or gating it
  behind a feature flag so it stops running in the default `--lib` invocation does not
  satisfy this feature. *(status: resolved)*
- **FR6 — Do not resolve the feature by lowering parallelism:** Changing the project's,
  `workflow.yaml`'s, or CI's Rust test command for the `--lib` suite to
  `-- --test-threads=1` is explicitly NOT an acceptable outcome for this feature. Only
  isolating or fixing the order-dependent shared state closes it. (Resolved by answer
  `scope.test-threads-fallback` = `fix_only`; this overrides the "next-best option"
  wording in the original task description's constraints section.) *(status: resolved)*

### Non-Functional Requirements

- **NFR1 - Stability criterion:** Three consecutive runs of the `--lib` suite at default
  parallelism must complete green (0 failed) to satisfy the stability criterion.
- **NFR2 - Suite scope of the green requirement:** Only the `--lib` suite must be green
  at default parallelism. Integration test targets keep their existing invocation rules —
  in particular `src-tauri/tests/mux_hot_upgrade.rs` continues to be invoked with
  `-- --test-threads=1` as documented in `test/README.md`, and that remains correct.
- **NFR3 - No collateral test regressions:** The fix must not introduce new failures
  elsewhere in the `--lib` suite, and must not degrade the pass rate of tests that were
  already green.
- **NFR4 - Feature-gate integrity:** The CLI-only build must keep compiling
  (`cargo check --no-default-features`), since the fix may touch `src-tauri/src/app/` or
  state shared with CLI-visible modules.
- **NFR5 - Conventions:** Any new or modified test follows `test/README.md`: inline
  `#[cfg(test)] mod tests {}` next to the code under test, no new test framework crates,
  `<subject>_<scenario>_<expected>` naming, and explicit per-test construction of the unit
  under test rather than shared global fixtures.

## Implementation Approach

### Architecture

No architectural change is specified by the requirements. The subject is the `app`
module's test-visible state:

```
emterm --lib test binary (default features, gui on)
  ├── app::tests::timing::frame_work_pending_* ── observes ──▶ app.frame_work_pending()
  │                                                                    │
  │                                                          reads shared / process-global
  │                                                          state (to be named by FR1)
  └── other tests in the same binary ────────── mutate ───────────────▶
```

FR1 fills in the "state (to be named)" node; FR2 then either cuts the mutate edge for the
timing test (test-side isolation / reset / serialization) or removes the shared node
itself (production-side state-management revision). Which of the two directions is taken
is an implementation decision, and AC-05 requires the chosen direction to be stated
explicitly.

### Data Flow

```
default-parallelism run ─▶ interleaving of the contending test(s) with the timing test
                        ─▶ frame_work_pending() observes false
                        ─▶ assert!(app.frame_work_pending()) fails
```

The mechanism connecting the interleaving to the `false` observation is exactly what AC-04
requires to be written down.

### API Design

Not applicable. The feature adds no API surface.

### Database Schema

Not applicable. The feature introduces no persisted data.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/app/` — the module under test; the expected locus of the fix.
- `src-tauri/src/app/tests/timing.rs` — the inline test module containing the failing
  test.
- State shared with CLI-visible modules — may be touched if the fix takes the
  production-side direction (NFR4).

**External Dependencies:**
- None. NFR5 forbids adding new test framework crates.

### File Structure

```
src-tauri/src/
└── app/
    ├── ...                     # production code; state management may be revised (FR2)
    └── tests/
        └── timing.rs           # the order-dependent test (assertion at line 135)
```

## Declared Change Set

- `src-tauri/src/**` — the expected locus is `src-tauri/src/app/` and its inline test
  module (`src-tauri/src/app/tests/timing.rs`). FR2 permits a production-side
  state-management revision, and NFR4 notes the fix may touch state shared with
  CLI-visible modules, so the declaration is the superset covering both fix directions.

Every SPEC declares, by default, the following two workflow-generated entries in addition
to the feature-specific paths above:

- `feature-docs/flaky-frame-work-pending-test/**`
- `test-docs/flaky-frame-work-pending-test/**`

`feature-docs/flaky-frame-work-pending-test/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`workflow.yaml`, `phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These are generated
and owned by the phase documents and by `references/phase-state.md`; this section cites
them and restates none of their rules.

`test-docs/flaky-frame-work-pending-test/**` covers
`test-docs/flaky-frame-work-pending-test/{T}.tests.yaml`, the per-task test record. It is
generated and owned by `implement-phase.md`; this section cites it and restates none of
its rules.

These two default entries are part of the declaration unless the SPEC author explicitly
removes them; their absence is never assumed by silence — removal is a deliberate,
explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at verification
time must be CONTAINED IN the declared set, not equal to it. A feature that produces no
implement tasks generates no `test-docs/flaky-frame-work-pending-test/` directory at all;
the declared entry is still correct in that case — a declared path that never materializes
is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-03** (Targeted contention reproduction): Given the identified contending
      test(s) from FR1, when only the timing test and the identified contending test(s)
      are run together at default parallelism (e.g. by a filtered `--lib` invocation),
      both before and after the fix, then the pair reproduces the failure before the fix
      and passes after it, confirming the diagnosis rather than merely masking it with a
      scheduling change. — covers AC-04 (FR1, FR3).
- [ ] **TS-05** (Assertion strength preserved): Given the fix is applied, when the
      scenario's precondition is deliberately broken (restart flag not raised), then the
      test's assertion still fails, demonstrating the assertion retains discriminating
      power. — covers AC-02 (FR4).

### Integration Tests

- [ ] **TS-01** (Repeated default-parallelism lib run): Given the fix is applied on the
      feature branch, when
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      is run three times in succession with no `--test-threads` override, then all three
      runs report 0 failed. — covers AC-01 (NFR1, NFR2).
- [ ] **TS-02** (Baseline reproduction is captured before the fix): Given the unmodified
      base revision, when the same default-parallelism `--lib` command is run, then the
      failure of `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
      is observed and recorded, establishing that the subsequent green runs are
      attributable to the fix rather than to environmental luck. — covers AC-01, AC-04
      (NFR1, NFR2, FR1, FR3).
- [ ] **TS-04** (Serial run stays green): Given the fix is applied, when the `--lib` suite
      is run with `-- --test-threads=1`, then the suite is still green, confirming the fix
      did not trade parallel flakiness for serial breakage. — covers AC-01, AC-03 (NFR1,
      NFR2, FR5).
- [ ] **TS-06** (CLI-only feature gate): Given the fix is applied, when
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      is run, then it succeeds. — covers AC-07 (NFR4).

### E2E Tests

**Existing E2E tests**: None — no E2E infrastructure exists for this project (ASM-06).
**Run command**: Not detected.

### Edge Cases

- [ ] Serial execution after the fix: `-- --test-threads=1` must remain green, so that
      parallel flakiness is not traded for serial breakage (TS-04).
- [ ] Deliberately broken precondition: with the restart flag not raised, the assertion
      must still fail (TS-05).
- [ ] Out-of-scope flakes surfacing during the three consecutive runs — the `tabs.rs`
      replay non-determinism (ASM-02) and the `tmux_sockets` discovery flake (ASM-03) are
      reported separately, not folded into this feature.

### Performance Tests

Not applicable. The resolved requirements state no performance criterion.

## Security Considerations

Not applicable. The feature changes no authentication, authorization, input handling, or
data protection surface.

## Error Handling

No runtime error paths are introduced. The requirement-level failure modes are:

| Case | Condition | Outcome |
|------|-----------|---------|
| Weakened assertion | The assertion is removed, inverted, made conditional, relaxed, or retried until green | FR4 unmet; AC-02 fails. |
| Hidden test | The test is `#[ignore]`d, deleted, or feature-gated out of the default `--lib` run | FR5 unmet; AC-03 fails. |
| Lowered parallelism | `-- --test-threads=1` is set for the `--lib` suite in `workflow.yaml`, CI configuration, `test/README.md`'s unit-test section, or the project's documented test command | FR6 unmet; AC-06 fails. |
| Broken CLI-only build | `cargo check --no-default-features` fails after a production-side fix | NFR4 unmet; AC-07 fails. |

## Performance Optimization

Not applicable.

## Traceability

| Requirement | Acceptance criteria | Test scenarios |
|-------------|---------------------|----------------|
| FR1 | AC-04 | TS-02, TS-03 |
| FR2 | AC-01 (the observable outcome of the fix) | TS-01, TS-03 |
| FR3 | AC-04, AC-05 | TS-02, TS-03 |
| FR4 | AC-02 | TS-05 |
| FR5 | AC-03 | TS-04 |
| FR6 | AC-06 | — (verified by inspection of the landed change set) |
| NFR1 | AC-01 | TS-01, TS-02, TS-04 |
| NFR2 | AC-01 | TS-01, TS-02, TS-04 |
| NFR3 | — (observed through the whole-`--lib` scope of AC-01) | TS-01 |
| NFR4 | AC-07 | TS-06 |
| NFR5 | — (verified by inspection against `test/README.md`) | — |

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are satisfied.
- [ ] All acceptance criteria (AC-01 – AC-07) pass.
- [ ] All test scenarios (TS-01 – TS-06) pass.
- [ ] No new failures are introduced elsewhere in the `--lib` suite (NFR3).
- [ ] `cargo check --no-default-features` succeeds (NFR4, AC-07).
- [ ] New or modified tests follow `test/README.md`'s conventions (NFR5).
- [ ] The diagnosis and the chosen fix location are recorded (FR3, AC-04, AC-05).
- [ ] Code review is completed.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1–FR6, NFR1–NFR5) is `resolved`; no requirement carries
`status: tbd`.

## Assumptions

- **ASM-01**: The failure is pre-existing and independent of the
  `pending-notifications-drain` feature (PR #43); the task description reports it
  reproducing on base `main` (12cebc80) under the same command. *(source: task
  description)*
- **ASM-02**: The non-determinism of the `src-tauri/src/tabs.rs` replay tests under
  parallel execution is a separate, known issue and is out of scope; it is documented in
  `test/README.md`. *(source: task description + `test/README.md`)*
- **ASM-03**: The known intermittent `tmux_sockets` discovery flake is likewise out of
  scope and is not the subject of the stability criterion; if it surfaces during the three
  consecutive runs it is to be reported, not silently folded into this feature. *(source:
  analyst inference from project test conventions)*
- **ASM-04**: The relevant suite size is roughly 3251 tests (3247 passed / 1 failed / 3
  ignored as reported), all in the `emterm` crate's `--lib` target; the tests live in
  `--lib` and `--bin emterm` reports 0 tests. *(source: task description +
  `test/README.md`)*
- **ASM-05**: The unit under test (`app`) is a GUI-feature-gated module, so the failing
  test only runs with default features enabled. *(source: CLAUDE.md)*
- **ASM-06**: No E2E infrastructure exists for this project, so no E2E coverage is
  expected for this feature. *(source: `test/README.md` + envelope)*
- **ASM-07**: The stability threshold (3 consecutive green runs), the lib-only suite
  scope, and the rejection of the `--test-threads=1` fallback were resolved in batch mode
  by Codex consultation under gate `create-spec.requirement-clarification`, not by a human
  answer. *(source: batch resolution of packet create-spec-q0001)*

## Design Step

Skipped. The feature has no user-visible surface: it is a test-stability bug fix confined
to `src-tauri/src/app/` and its inline test module. There is no new UI, no change to
rendered output, no new user-facing command, and no interaction with the project's design
tokens.

## References

- Requirements document: `feature-docs/flaky-frame-work-pending-test/REQUIREMENTS.md`
- Failing test: `src-tauri/src/app/tests/timing.rs` (assertion at line 135)
- Test conventions and known flakes: `test/README.md`
- Integration target keeping `-- --test-threads=1`: `src-tauri/tests/mux_hot_upgrade.rs`
