---
title: "flaky-frame-work-pending-test"
created_date: 2026-08-18
status: draft
---

# flaky-frame-work-pending-test - Requirements Document

## 1. Overview

### 1.1 Background

The `--lib` test suite of the `emterm` crate contains an order-dependent test.
`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
(`src-tauri/src/app/tests/timing.rs`, around line 135, assertion
`assert!(app.frame_work_pending())`) fails when the suite runs at cargo's default
test parallelism and passes when the suite runs with `-- --test-threads=1`. The
failure is pre-existing and independent of the feature that surfaced it: it
reproduces on base `main` (12cebc80) under the same command (ASM-01).

Because the suite exits non-zero on this failure, unattended (batch) workflow runs
fail on a result unrelated to the feature under test, and every feature must
separately prove that the failure also occurs on its base commit.

### 1.2 Purpose

Remove the order dependence so that the `--lib` suite passes reliably at default
parallelism, without weakening, hiding, or skipping the test, and without lowering
the suite's parallelism.

### 1.3 Scope

**In scope**

- Diagnosing the shared or process-global state that the timing test contends for.
- Fixing that contention, either on the test side (isolation / reset /
  serialization) or on the production-code side (state-management revision).
- Recording the diagnosis and the chosen fix location in this feature's documents.

**Out of scope**

- The non-determinism of the `src-tauri/src/tabs.rs` replay tests under parallel
  execution — a separate, known issue documented in `test/README.md` (ASM-02).
- The known intermittent `tmux_sockets` discovery flake. If it surfaces during the
  three consecutive verification runs it is reported, not folded into this feature
  (ASM-03).
- Integration test targets, which keep their existing invocation rules — in
  particular `src-tauri/tests/mux_hot_upgrade.rs` continues to be invoked with
  `-- --test-threads=1` as documented in `test/README.md`, and that remains correct
  (NFR2).
- Any user-visible surface. This feature has no new UI, no change to rendered
  output, no new user-facing command, and no interaction with the project's design
  tokens; the design step is therefore skipped.

## 2. Business Requirements

### 2.1 Business Goals

| ID | Goal |
|----|------|
| OBJ-01 | Make the `--lib` test suite of the `emterm` crate pass reliably at cargo's default test parallelism, so that unattended (batch) workflow runs stop exiting non-zero on a failure unrelated to the feature under test. |
| OBJ-02 | Remove the recurring verification cost of proving, per feature, that `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing` fails on the base commit as well and is therefore not caused by the feature's own changes. |
| OBJ-03 | Leave a durable record of which shared or global state the test was contending for, and whether the fix landed in the test or in the production code, so the same class of order dependence is recognizable next time. |

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| Developer running the `--lib` suite | Runs `cargo test --lib` at default parallelism during ordinary development and needs the result to be trustworthy. |
| Unattended (batch) workflow run | Executes the suite without a human present; a non-zero exit on an unrelated flake stops the run. |

### 2.3 Expected Effects

- The `--lib` suite's result at default parallelism becomes attributable to the code
  under test rather than to scheduling luck (OBJ-01).
- The per-feature cost of re-proving that this failure is pre-existing disappears
  (OBJ-02).
- The contended state and the fix location are documented for future recognition of
  the same failure class (OBJ-03).

## 3. Use Cases

### 3.1 Use Case List

| ID | Use case | Actor | Priority |
|----|----------|-------|----------|
| UC01 | Run the `--lib` suite at default parallelism and get a trustworthy result | Developer running the `--lib` suite | High |
| UC02 | Complete an unattended workflow run without an unrelated non-zero exit | Unattended (batch) workflow run | High |

### 3.2 Use Case Details

#### UC01: Run the `--lib` suite at default parallelism and get a trustworthy result

**Actor**: Developer running the `--lib` suite

**Preconditions**:
- The fix is applied.

**Main flow**:
1. The developer runs
   `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
   with no `--test-threads` override.
2. All tests, including
   `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`, execute.
3. The run reports 0 failed.

**Alternative flows**:
- A failure unrelated to this feature (for example the known `tmux_sockets`
  discovery flake, ASM-03) surfaces; it is reported separately rather than treated
  as part of this feature.

**Postconditions**:
- The suite result reflects the state of the code under test, not the test
  execution order.

#### UC02: Complete an unattended workflow run without an unrelated non-zero exit

**Actor**: Unattended (batch) workflow run

**Preconditions**:
- The fix is applied.

**Main flow**:
1. The workflow invokes the `--lib` suite at default parallelism as part of its
   verification step.
2. The suite completes green.
3. The workflow proceeds without a human being asked to confirm that the failure was
   pre-existing.

**Postconditions**:
- No per-feature base-commit reproduction is needed for this test (OBJ-02).

## 4. Functional Requirements

### 4.1 Function List

| ID | Name | Description | Priority |
|----|------|-------------|----------|
| FR1 | Identify the shared state behind the order dependence | Name the concrete shared or process-global state that causes the parallel-only failure. | High |
| FR2 | Eliminate the order dependence at its source | Fix the contention on the test side or the production-code side. | High |
| FR3 | Record the diagnosis and the chosen fix location | Document the state, the contending tests, and where the fix landed. | High |
| FR4 | Preserve the assertion's strength | Keep the existing assertion semantics intact. | High |
| FR5 | Do not hide the test | Keep the test running in the default `--lib` invocation. | High |
| FR6 | Do not resolve the feature by lowering parallelism | Do not switch the `--lib` suite to `-- --test-threads=1`. | High |

### 4.2 Function Details

#### FR1: Identify the shared state behind the order dependence

**Description**: Determine why
`app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
(`src-tauri/src/app/tests/timing.rs`, around line 135, assertion
`assert!(app.frame_work_pending())`) fails under parallel execution and passes under
`-- --test-threads=1`. The outcome must name the concrete shared or process-global
state that other tests in the same binary mutate concurrently (for example a
`static` / `OnceLock` / atomic flag, a process-wide palette or settings slot, an
environment variable, or a shared filesystem/socket path).

**Status**: resolved

**Business rules**:
- The outcome names concrete state, not a general suspicion of "flakiness".
- The mechanism by which the interleaving makes `frame_work_pending()` observe
  `false` is part of the outcome (AC-04).

#### FR2: Eliminate the order dependence at its source

**Description**: Fix the identified contention either by isolating the test
(resetting or serializing the shared state, or giving the unit under test its own
instance of it) or by revising the production code's state management so the state is
no longer process-global. Both directions are acceptable; the choice is an
implementation decision.

**Status**: resolved

**Business rules**:
- The fix addresses the contention itself rather than the scheduling that exposes it
  (see TS-03).

#### FR3: Record the diagnosis and the chosen fix location

**Description**: Document, in the feature's own documents, the identified shared
state, the tests that were contending for it, and whether the fix was applied to the
test side or the production-code side.

**Status**: resolved

#### FR4: Preserve the assertion's strength

**Description**: The existing assertion semantics of the test are preserved. Removing
the assertion, relaxing it, retrying until green, or otherwise weakening what the test
proves does not satisfy this feature.

**Status**: resolved

**Error cases**:

| Outcome | Condition | Handling |
|---------|-----------|----------|
| Requirement not satisfied | The assertion is removed, inverted, made conditional, relaxed, or wrapped in a retry loop | The feature is not closed by such a change (AC-02). |

#### FR5: Do not hide the test

**Description**: Marking the test `#[ignore]`, deleting it, or gating it behind a
feature flag so it stops running in the default `--lib` invocation does not satisfy
this feature.

**Status**: resolved

**Error cases**:

| Outcome | Condition | Handling |
|---------|-----------|----------|
| Requirement not satisfied | The test no longer appears in the executed test list of the default `--lib` run | The feature is not closed by such a change (AC-03). |

#### FR6: Do not resolve the feature by lowering parallelism

**Description**: Changing the project's, `workflow.yaml`'s, or CI's Rust test command
for the `--lib` suite to `-- --test-threads=1` is explicitly NOT an acceptable outcome
for this feature. Only isolating or fixing the order-dependent shared state closes it.
(Resolved by answer `scope.test-threads-fallback` = `fix_only`; this overrides the
"next-best option" wording in the original task description's constraints section.)

**Status**: resolved

## 5. Non-Functional Requirements

### 5.1 Stability (NFR1)

Three consecutive runs of the `--lib` suite at default parallelism must complete green
(0 failed) to satisfy the stability criterion.

### 5.2 Suite Scope of the Green Requirement (NFR2)

Only the `--lib` suite must be green at default parallelism. Integration test targets
keep their existing invocation rules — in particular
`src-tauri/tests/mux_hot_upgrade.rs` continues to be invoked with
`-- --test-threads=1` as documented in `test/README.md`, and that remains correct.

### 5.3 No Collateral Test Regressions (NFR3)

The fix must not introduce new failures elsewhere in the `--lib` suite, and must not
degrade the pass rate of tests that were already green.

### 5.4 Feature-Gate Integrity (NFR4)

The CLI-only build must keep compiling (`cargo check --no-default-features`), since
the fix may touch `src-tauri/src/app/` or state shared with CLI-visible modules.

### 5.5 Conventions (NFR5)

Any new or modified test follows `test/README.md`: inline `#[cfg(test)] mod tests {}`
next to the code under test, no new test framework crates,
`<subject>_<scenario>_<expected>` naming, and explicit per-test construction of the
unit under test rather than shared global fixtures.

## 6. UI/UX Requirements

Not applicable. This feature has no user-visible surface: no new UI, no change to
rendered output, no new user-facing command, and no interaction with the project's
design tokens. The design step is skipped for this reason.

## 7. Data Requirements

Not applicable. This feature introduces no persisted data and no data model change.

## 8. External Integrations

Not applicable. This feature adds no external system integration.

## 9. Constraints

### 9.1 Technical Constraints

- The unit under test (`app`) is a GUI-feature-gated module, so the failing test only
  runs with default features enabled (ASM-05).
- The CLI-only build (`cargo check --no-default-features`) must keep compiling
  (NFR4).
- New or modified tests follow `test/README.md`'s conventions (NFR5).
- No E2E infrastructure exists for this project, so no E2E coverage is expected for
  this feature (ASM-06).

### 9.2 Business Constraints

- Lowering the `--lib` suite's parallelism is not an acceptable resolution (FR6).
- Weakening or hiding the test is not an acceptable resolution (FR4, FR5).

### 9.3 Schedule Constraints

None stated.

### 9.4 Declared Change Set

**Feature-specific paths**:
- `src-tauri/src/**` — the expected locus is `src-tauri/src/app/` and its inline test
  module (`src-tauri/src/app/tests/timing.rs`), but FR2 permits a production-side
  state-management revision, and NFR4 notes that the fix may touch state shared with
  CLI-visible modules. The declaration is deliberately the superset that covers both
  fix directions.

**Default members** (always part of the declaration unless the SPEC author explicitly
excludes them):
- `feature-docs/{feature}/**`
- `test-docs/{feature}/**`

`feature-docs/{feature}/**` contains `REQUIREMENTS.md`, `SPEC.md`, `workflow.yaml`,
`phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`,
and the design artifacts the design step produces. Their producers are the phase
documents and `references/phase-state.md` (cited, not restated).

`test-docs/{feature}/**` contains `{T}.tests.yaml` (path form:
`test-docs/{feature}/{T}.tests.yaml`). Its producer is `implement-phase.md` (cited,
not restated).

**Semantics**:
- Default members are part of the declaration unless explicitly excluded; exclusion is
  a deliberate narrowing, never an omission by silence.
- The declaration is a SUPERSET assertion: the actual change set must be CONTAINED IN
  the declared set. A declared path that never materializes is not a violation.

## 10. Anticipated Issues and Risks

### 10.1 Technical Issues

| Issue | Impact | Mitigation |
|-------|--------|------------|
| A scheduling change masks the failure instead of fixing the contention | High | TS-03 reproduces the failure with only the identified contending test(s) before the fix and requires the pair to pass after it. |
| The fix trades parallel flakiness for serial breakage | Medium | TS-04 re-runs the suite with `-- --test-threads=1` after the fix. |
| Unrelated known flakes (`tabs.rs` replay, `tmux_sockets` discovery) surface during verification | Medium | They are out of scope (ASM-02, ASM-03) and are reported separately rather than folded into this feature. |
| A production-side state-management revision breaks the CLI-only build | Medium | NFR4 / AC-07 require `cargo check --no-default-features` to succeed. |

### 10.2 Business Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| The suite is made green by lowering parallelism, leaving the order dependence in place | Low | High | FR6 / AC-06 forbid setting `-- --test-threads=1` for the `--lib` suite anywhere. |
| The suite is made green by weakening or hiding the test | Low | High | FR4 / FR5, verified by AC-02, AC-03 and TS-05. |

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] **AC-01**: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      run three consecutive times at cargo's default parallelism (no `--test-threads`
      override) completes with 0 failed on every run. *(from OBJ-01, NFR1, NFR2)*
- [ ] **AC-02**: The failing assertion at `src-tauri/src/app/tests/timing.rs:135`
      still asserts `app.frame_work_pending()` under the same scenario (restart flag
      raised, nothing consumed); the assertion has not been removed, inverted, or made
      conditional. *(from FR4)*
- [ ] **AC-03**:
      `app::tests::timing::frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`
      still appears in the executed test list of the default `--lib` run (not
      `#[ignore]`d, not removed). *(from FR5)*
- [ ] **AC-04**: The root cause is written down: the specific shared/global state, the
      other test(s) that mutate it, and the mechanism by which the interleaving makes
      `frame_work_pending()` observe `false`. *(from FR1, FR3)*
- [ ] **AC-05**: The record states explicitly whether the fix was applied to the test
      side (isolation / reset / serialization) or the production-code side (state
      management revision). *(from FR3)*
- [ ] **AC-06**: No change lands that sets `-- --test-threads=1` for the `--lib` suite
      in `workflow.yaml`, CI configuration, `test/README.md`'s unit-test section, or
      the project's documented test command. *(from FR6)*
- [ ] **AC-07**: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds. *(from NFR4)*

### 11.2 KPI

| Metric | Target | Measurement |
|--------|--------|-------------|
| Consecutive green `--lib` runs at default parallelism | 3 of 3 | Repeated execution of the AC-01 command (TS-01) |
| New failures introduced elsewhere in `--lib` | 0 | Whole-suite result of the AC-01 runs (NFR3) |

## 12. Test Scenarios

### 12.1 Test Perspectives

- [ ] **TS-01 — Repeated default-parallelism lib run** (normal case): Given the fix is
      applied on the feature branch, when
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      is run three times in succession with no `--test-threads` override, then all
      three runs report 0 failed. *(covers AC-01)*
- [ ] **TS-02 — Baseline reproduction is captured before the fix** (regression
      baseline): Given the unmodified base revision, when the same
      default-parallelism `--lib` command is run, then the failure of
      `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing` is
      observed and recorded, establishing that the subsequent green runs are
      attributable to the fix rather than to environmental luck. *(covers AC-01,
      AC-04)*
- [ ] **TS-03 — Targeted contention reproduction** (diagnosis confirmation): Given the
      identified contending test(s) from FR1, when only the timing test and the
      identified contending test(s) are run together at default parallelism (e.g. by a
      filtered `--lib` invocation), both before and after the fix, then the pair
      reproduces the failure before the fix and passes after it, confirming the
      diagnosis rather than merely masking it with a scheduling change. *(covers
      AC-04)*
- [ ] **TS-04 — Serial run stays green** (boundary case): Given the fix is applied,
      when the `--lib` suite is run with `-- --test-threads=1`, then the suite is still
      green, confirming the fix did not trade parallel flakiness for serial breakage.
      *(covers AC-01, AC-03)*
- [ ] **TS-05 — Assertion strength preserved** (negative case): Given the fix is
      applied, when the scenario's precondition is deliberately broken (restart flag
      not raised), then the test's assertion still fails, demonstrating the assertion
      retains discriminating power. *(covers AC-02)*
- [ ] **TS-06 — CLI-only feature gate** (build case): Given the fix is applied, when
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      is run, then it succeeds. *(covers AC-07)*

No E2E scenario exists: no E2E infrastructure exists for this project (ASM-06). No
performance or security scenario is stated in the resolved requirements.

## 13. Glossary

| Term | Definition |
|------|------------|
| Order dependence | A test outcome that depends on which other tests run concurrently or beforehand, rather than only on the code under test. |
| Default parallelism | Cargo's own test thread count, i.e. an invocation with no `--test-threads` override. |
| `--lib` suite | The `emterm` crate's library test target; roughly 3251 tests (3247 passed / 1 failed / 3 ignored as reported). `--bin emterm` reports 0 tests (ASM-04). |
| Contending test | A test in the same binary that mutates the shared or process-global state the timing test observes. |

## 14. Confirmations

### 14.1 Confirmed Items

Resolved in batch mode by Codex consultation under gate
`create-spec.requirement-clarification`, not by a human answer (ASM-07):

- [x] Stability threshold: three consecutive green `--lib` runs at default parallelism
      (NFR1).
- [x] Suite scope of the green requirement: `--lib` only; integration targets keep
      their existing invocation rules (NFR2).
- [x] `scope.test-threads-fallback` = `fix_only`: switching the `--lib` suite to
      `-- --test-threads=1` is not an acceptable resolution; this overrides the
      "next-best option" wording in the original task description's constraints
      section (FR6).

Assumptions carried from analysis:

- [x] ASM-01: The failure is pre-existing and independent of the
      `pending-notifications-drain` feature (PR #43); the task description reports it
      reproducing on base `main` (12cebc80) under the same command.
- [x] ASM-02: The non-determinism of the `src-tauri/src/tabs.rs` replay tests under
      parallel execution is a separate, known issue and is out of scope; it is
      documented in `test/README.md`.
- [x] ASM-03: The known intermittent `tmux_sockets` discovery flake is likewise out of
      scope and is not the subject of the stability criterion; if it surfaces during
      the three consecutive runs it is to be reported, not silently folded into this
      feature.
- [x] ASM-04: The relevant suite size is roughly 3251 tests (3247 passed / 1 failed /
      3 ignored as reported), all in the `emterm` crate's `--lib` target; the tests
      live in `--lib` and `--bin emterm` reports 0 tests.
- [x] ASM-05: The unit under test (`app`) is a GUI-feature-gated module, so the failing
      test only runs with default features enabled.
- [x] ASM-06: No E2E infrastructure exists for this project, so no E2E coverage is
      expected for this feature.
- [x] ASM-07: The stability threshold (3 consecutive green runs), the lib-only suite
      scope, and the rejection of the `--test-threads=1` fallback were resolved in
      batch mode by Codex consultation under gate
      `create-spec.requirement-clarification`, not by a human answer.

### 14.2 Open / Deferred Items

None. Every functional and non-functional requirement is `resolved`; no requirement
carries `status: tbd`.

## 15. References

- `src-tauri/src/app/tests/timing.rs` — location of the order-dependent test
  (assertion at line 135).
- `test/README.md` — unit-test conventions (NFR5), the `mux_hot_upgrade.rs`
  `-- --test-threads=1` rule (NFR2), and the known `tabs.rs` replay non-determinism
  (ASM-02).
- `src-tauri/tests/mux_hot_upgrade.rs` — integration target whose existing invocation
  rule is unchanged by this feature.
- `SPEC.md` — the implementation-facing rendering of these requirements.
