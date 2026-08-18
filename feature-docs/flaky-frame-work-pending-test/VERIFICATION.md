# Verification Document: flaky-frame-work-pending-test

## Overview

**Feature**: flaky-frame-work-pending-test /
**SPEC.md**: `feature-docs/flaky-frame-work-pending-test/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/flaky-frame-work-pending-test/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level acceptance
criteria live in `feature-docs/flaky-frame-work-pending-test/tasks/task0001.md`.

Every command below is quoted verbatim and is the only form available to this workflow —
filtered or otherwise varied cargo invocations are not usable, so no verification item
depends on running a subset of the suite.

## Build Verification

- Command (component `rust`, GUI/default features):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (component `rust_cli`, CLI-only feature gate):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both.

## Test Verification

- Command (default parallelism — the stability subject):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  Run **three consecutive times**; every run must report `0 failed`.
- Command (serial safety check):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
  Run once; must report `0 failed`. This is a check, not a project test-command change —
  the project's documented `--lib` command stays at default parallelism (FR6).
- Coverage target: not applicable. The project has no coverage instrumentation; the gate
  is the whole-suite result above.
- Out-of-scope flakes: a run whose only failures are the documented `tabs.rs` replay
  non-determinism (ASM-02) or the `tmux_sockets` discovery flake (ASM-03) is recorded with
  its test name and repeated, per IMPLEMENTATION.md D4. A failure of
  `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing`, or any failure
  attributable to this feature's change set, fails verification outright.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Repeated default-parallelism `--lib` run: the default-parallelism command above run three times in succession | All three runs report `0 failed`; each summary line recorded in `DIAGNOSIS.md` | Integration |
| TS-02 | Baseline reproduction captured on the unmodified base state before any source change | The failure of `frame_work_pending_true_when_restart_flag_raised_and_consumes_nothing` is observed and quoted (command, test path, assertion message) in `DIAGNOSIS.md` | Integration |
| TS-03 | Targeted contention reproduction: the diagnosis is confirmed rather than masked | `DIAGNOSIS.md` enumerates every party that raises / clears / consumes / observes the contended state and states the mechanism; the deterministic regression guard (task0001 AC-6) fails against the unfixed state and passes after the fix | Unit + document review |
| TS-04 | Serial run stays green: the serial command above | `0 failed`; the target test is present in the executed list | Integration |
| TS-05 | Assertion strength preserved: the scenario's precondition is deliberately broken, the default-parallelism command is run, then the break is reverted | The target test fails while the precondition is broken; the suite is green again after the revert; both observations recorded in `DIAGNOSIS.md`; the break is absent from the final change set | Unit (manual evidence) |
| TS-06 | CLI-only feature gate: the `rust_cli` build command above | Exit code 0 | Integration |

## Code Quality Verification

- Format: `workflow.yaml` declares no format command for either component, and the project
  does not enforce crate-wide rustfmt. `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  is available as an advisory check only; it is not a pass/fail gate for this feature, and
  a crate-wide reformat is explicitly unwanted.
- Static analysis: none configured for this project; no lint gate applies.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-01 | Three consecutive default-parallelism `--lib` runs report `0 failed` | TS-01 (plus D4's handling of out-of-scope flakes) |
| AC-02 | The target assertion still asserts the frame pending-work predicate under the same scenario, unweakened | TS-05 plus inspection of the diff at `src-tauri/src/app/tests/timing.rs` |
| AC-03 | The target test still appears in the executed test list of the default `--lib` run | TS-01 / TS-04 executed-test list; inspection for `#[ignore]`, deletion or feature gating |
| AC-04 | The root cause is written down: state, contending tests, mechanism | Document review of `DIAGNOSIS.md` (TS-02, TS-03) |
| AC-05 | The record states explicitly whether the fix is test-side or production-side | Document review of `DIAGNOSIS.md` |
| AC-06 | No `-- --test-threads=1` lands for the `--lib` suite in `workflow.yaml`, CI config, `test/README.md`'s unit-test section or the documented test command | Inspection of the landed change set against those four locations |
| AC-07 | The CLI-only check succeeds | TS-06 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-02, TS-03 |
| FR2 | task0001 | TS-01, TS-03 |
| FR3 | task0001 | TS-02, TS-03 |
| FR4 | task0001 | TS-05 |
| FR5 | task0001 | TS-01, TS-04 |
| FR6 | task0001 | Inspection of the landed change set (AC-06); no automated scenario exists |
| NFR1 | task0001 | TS-01 |
| NFR2 | task0001 | TS-01, TS-04 |
| NFR3 | task0001 | TS-01 (whole-suite scope) |
| NFR4 | task0001 | TS-06 |
| NFR5 | task0001 | Inspection against `test/README.md`; no automated scenario exists (recorded gap) |

## Manual Testing (E2E Not Possible)

The project has no E2E infrastructure (ASM-06), so there is no E2E section. The following
items require human judgment rather than a command result:

- [ ] `DIAGNOSIS.md` names a concrete contended state and its owning file — not a general
      statement about flakiness (AC-04).
- [ ] `DIAGNOSIS.md` enumerates the contending parties by full test path and explains the
      mechanism that flips the observation (AC-04).
- [ ] `DIAGNOSIS.md` states the fix side explicitly, with the reason and the rejected
      alternatives (AC-05).
- [ ] `DIAGNOSIS.md` records the TS-05 evidence (failure under the deliberately broken
      precondition, and its revert) and the TS-02 baseline quote.
- [ ] The diff at `src-tauri/src/app/tests/timing.rs` shows the target assertion and its
      scenario unchanged in strength (AC-02).
- [ ] The change set introduces no `-- --test-threads=1` for the `--lib` suite anywhere
      (AC-06), and does not re-order or rename tests as a probability-shaping substitute
      for isolation (IMPLEMENTATION.md D3).
- [ ] New or modified tests follow `test/README.md`: inline test module, no new test
      framework crate, `<subject>_<scenario>_<expected>` naming, per-test construction of
      the unit under test (NFR5).
- [ ] The regression guard is deterministic — no sleeps, no wall-clock thresholds, no
      thread-interleaving assumptions (task0001 AC-6).

No mockup comparison applies: the design step is skipped and the feature has no visual
surface.

## Performance / Security Verification

Not applicable. The resolved requirements state no performance criterion, and the feature
changes no authentication, authorization, input-handling or data-protection surface.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-01 – TS-06) | 6 | 4 | 0 | 2 |
| Success criteria (AC-01 – AC-07) | 7 | 4 | 0 | 3 |
| Code quality | 0 (advisory only) | 0 | 0 | 0 |
| Manual review items | 8 | 0 | 0 | 8 |
