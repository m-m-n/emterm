# Verification Document: verify-font-bootstrap-classification

## Overview

**Feature**: verify-font-bootstrap-classification /
**SPEC.md**: `feature-docs/verify-font-bootstrap-classification/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/verify-font-bootstrap-classification/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/verify-font-bootstrap-classification/tasks/task0001.md`
and `feature-docs/verify-font-bootstrap-classification/tasks/task0002.md`.

## Build Verification

TypeScript component (the child WebView bundles — regression only; this
feature changes no bundled source):

- Command: `bun run build:viewer && bun run build:settings`
- Expected: exit code 0, no errors

Rust component (regression only; this feature changes no Rust source):

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `bun test`
- Command (Rust, regression only): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: not applicable — this project configures no coverage
  tooling and no coverage gate.

> **Do not conflate the two commands above.** The Rust component's test
> command is this project's own verification command; the command the script
> reproduces inside its un-fetched scenario is the literal reproduction
> command pinned by FR7, which carries no test-harness flag. No flag may be
> propagated from one to the other.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Feed the classification a synthetic captured log holding a `test result:` line with a non-zero failed count, together with a non-zero exit status | The warned outcome, with a message stating the observed exit status and the executed count derived from that log, and containing no build-stop wording | Unit |
| TS-2 | Feed the classification a synthetic captured log holding no `test result:` line, together with a non-zero exit status | The failing outcome, with a message naming a build stop and stating the observed exit status and a derived executed count of zero (not a hardcoded literal) | Unit |
| TS-3 | Feed the classification a synthetic captured log holding no `test result:` line, together with a zero exit status | The failing outcome, with the unchanged wording `command exited 0 but 0 tests executed` | Unit |
| TS-4 | Map the TS-1 outcome through the script's reporting path | The scenario emits its own prefixed warning line carrying the message, then the same passed-verdict line a clean pass produces, surfaces the captured log's tail on standard error, counts toward the passed tally, and leaves the aggregate exit status at success when the other two scenarios pass | Integration |
| TS-5 | Inspect the un-fetched scenario's cargo invocation and the CI job's step | The invocation is byte-for-byte the literal reproduction command with no added cargo or test-harness flag and no test-thread flag anywhere in the script; the CI step keeps its exact one-line run string with no arguments, no cache step, no acquisition-script reference and no opt-out environment variable | Static |
| TS-6 | Exercise the executed-count derivation on edge-case logs: two `test result:` lines, a log with none, a suite reporting zero passed and zero failed, and a non-cargo exit status alongside a non-zero count | The two lines are summed; the other two logs derive zero and take the zero-executed branch; the non-cargo exit status with a non-zero count still takes the test-failure branch with cause-neutral wording | Unit |
| TS-7 | Inspect the script for the untouched surface | The fetch-failure scenario, the already-fetched scenario, the exit-time cleanup, the counting rule, the scenario invocation order and the summary line are unchanged; the unset-variable and pipeline-failure options are still set and exit-on-error is still not enabled; `feature-docs/worktree-font-bootstrap/SPEC.md` and `.github/workflows/release.yml` are unmodified | Static |
| TS-8 | Run the default test invocation with no argument naming the new test file, in an environment with no Rust toolchain on PATH and no network access | The regression test is collected, runs and passes; the per-task record exists under `test-docs/verify-font-bootstrap-classification/` | Integration |
| TS-9 | Feed the classification a log path that names no readable file — once absent, once present but unreadable — together with a non-zero exit status | The failing outcome in both cases, with a message naming the log path and the fact that the executed count could not be derived, stating no test count at all (in particular no empty count field) and containing neither the build-stop wording nor the executed-and-failed wording | Unit |
| TS-10 | Feed the classification the same unreadable log paths together with a zero exit status, and separately an exit-status argument that is not an integer | The failing outcome in every case — never the passed and never the warned outcome — with a message naming the input that could not be interpreted | Unit |

## Code Quality Verification

- Format: `bunx biome check .`
- Format (Rust, regression only): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: no separate linter is configured for shell scripts in this
  project; the shell changes are covered by TS-5 / TS-7's static assertions
  and by review.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | An executed-but-failing run is reported as a test failure, never as a stopped build | TS-1 |
| AC2 | A run that stopped before any test ran is reported as a FAIL naming a build stop and the observed exit status | TS-2 |
| AC3 | No verdict message contains a hardcoded test count, and none states a count that was not derived for that run | TS-1, TS-2, TS-6, TS-9, TS-10 |
| AC4 | With the AC1 situation and the other two scenarios passing, the summary reports every scenario passed and the script exits 0 | TS-4, plus the manual end-to-end run |
| AC5 | Zero exit status with zero executed tests still reports FAIL | TS-3 |
| AC6 | The cargo invocation is the literal reproduction command with no added flag, and the CI step's command string is unchanged | TS-5 |
| AC7 | An automated regression test covers AC1, AC2, AC3 and AC5, runs in the default test suite, and its record exists under `test-docs/verify-font-bootstrap-classification/` | TS-1, TS-2, TS-3, TS-8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001, task0002 | TS-2, TS-9 |
| FR3 | task0001 | TS-1 |
| FR4 | task0001, task0002 | TS-3, TS-10 |
| FR5 | task0001, task0002 | TS-1, TS-2, TS-9, TS-10 |
| FR6 | task0001 | TS-4 |
| FR7 | task0001 | TS-5 |
| FR8 | task0001 | TS-5 |
| FR9 | task0001 | TS-1, TS-2, TS-8 |
| FR10 | task0001 | TS-7 |
| NFR1 | task0001 | TS-8 |
| NFR2 | task0001 | TS-7 |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-5 |
| NFR5 | task0001 | TS-4 |

## E2E Testing

The project configures no E2E framework (`project.components.*.e2e_test_command`
is empty for both components), so there is no automated E2E layer for this
feature. The end-to-end behaviour of the script is covered by the manual
section below.

## Manual Testing (E2E Not Possible)

These need a real Rust toolchain, a real network and several minutes of
builds, which the automated suites deliberately avoid (NFR1).

- [ ] Run `bash scripts/verify-font-bootstrap.sh` from the repository root on
      a machine with a Rust toolchain and network access. Confirm the summary
      line reports every scenario passed and the command exits 0.
- [ ] Confirm nothing is left behind afterwards: no scenario worktree, no
      stale worktree registration (`git worktree list` shows only the
      expected trees), and no leftover scratch directory (NFR3).
- [ ] Reproduce the warned branch end to end: on a checkout whose library
      suite has at least one failing test, run the script and confirm the
      un-fetched scenario emits a distinct warning line naming the failure
      plus the log tail, still reports the scenario as passed, and the script
      exits 0 (AC4, NFR5).
- [ ] Reproduce the build-stop branch end to end: on a tree where the build
      stops before any test runs, confirm the un-fetched scenario reports
      FAIL naming a build stop with the observed exit status and a zero
      executed count, and the script exits non-zero (AC2).
- [ ] Confirm the verify-font-bootstrap CI job is green on the feature branch,
      and that the bun job — which installs no Rust toolchain — is green too
      (NFR1).

There is no mockup comparison item: the design step is skipped for this
feature and it has no visual surface.

## Performance / Security Verification (if applicable)

Not applicable. The feature adds no authentication, authorization, user-input
surface, data storage or network surface, and states no performance
requirement. The one new file is a set of shell function definitions with no
side effects, invoked once per script run.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 10 | 10 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| Success criteria | 7 | 7 | 0 | 1 (AC4 also confirmed end to end) |
| Manual checks | 5 | 0 | 0 | 5 |
