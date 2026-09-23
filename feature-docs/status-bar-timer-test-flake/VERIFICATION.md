# Verification Document: status-bar-timer-test-flake

## Overview

**Feature**: status-bar-timer-test-flake /
**SPEC.md**: `feature-docs/status-bar-timer-test-flake/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/status-bar-timer-test-flake/IMPLEMENTATION.md`

Changed source files: `src-tauri/src/status_bar/providers/time.rs`,
`src-tauri/src/status_bar/runtime.rs`.

Run every command from the integration worktree root. Do not `cd` into
`src-tauri/`. Bound long runs with the Bash tool's `timeout` parameter.

## Build Verification

- Command (default features):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, NFR3):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 for both, with no new warnings from the two changed
  files.
- TypeScript component: no TypeScript file changes. Its build is not part of
  this feature's verification.

## Test Verification

- Command (workflow.yaml `rust.test_command`):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Targeted repeated runs (TS-7):
  - parallel, 20 runs:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib status_bar::`
  - serial, 20 runs:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib status_bar:: -- --test-threads=1`
  - oversubscribed (CPU load), 5 runs:
    `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib status_bar:: -- --test-threads=64`
- Full-suite repeated runs (AC1): 3 runs of the workflow.yaml test command
  and 3 runs of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`.
  Only the `status_bar::` tests are judged. A failure outside `status_bar::`
  is recorded as out-of-scope baseline noise and does not fail this
  feature.
- Coverage target: not measured. No coverage tooling is configured for the
  crate, so coverage is tracked by the scenario mapping below.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | TimeProvider with the manual tick source and a counting WakeFn. The test delivers N ticks one at a time, waiting for each (`time_provider_timer_thread_calls_wake_on_interval`) | Wake count is 0 before any delivery and exactly N afterwards | Unit |
| TS-2 | TimeProvider with the manual tick source. The test records v0 without calling get_value, then delivers N ticks (`time_provider_timer_thread_bumps_version_per_tick_without_get_value`) | Version equals v0 + N exactly; get_value is never called | Unit |
| TS-3 | StatusBarRuntime built through the test-only builder with the manual tick source, a counting WakeFn, a cwd source returning none, and no custom commands. The test delivers N ticks (`runtime_time_provider_timer_fires_wake`) | Runtime-supplied wake count equals N exactly | Integration |
| TS-4 | Delayed-tick regression test. It holds N ticks while the timer thread is parked, checks the counts with no sleep, then delivers them through the shared helper. **Verification-time check (AC3):** temporarily replace the shared helper's release-and-wait handshake with a fixed real-time sleep, for example 120 ms, keeping the count assertions. Run the regression test by name and confirm it fails. Restore the file, confirm with `git diff` that the source matches the committed state, re-run, and confirm it passes | Passes every run. Fails under the temporary fixed-window change. No residue after restoring | Unit |
| TS-5 | Drop a manual-source TimeProvider whose timer thread is parked waiting for a tick that never comes | Drop returns. The controller reports closed. Wake count and the retained version counter are unchanged. A tick released after Drop reports zero completed and does not block | Unit |
| TS-6 | Production-path regression: `time_provider_drop_joins_timer_thread` (60 s interval, Drop within 1 s), `time_provider_version_is_pure_load`, `time_provider_without_timer_does_not_spawn_thread`, `provider_set_format_bumps_version`, and every other status_bar test pass with unchanged assertions. Extended at create-plan to cover NFR4 (SPEC AC5): diff inspection shows the production source still does one timed wait for the configured interval on the existing pair, bumps before wake, and that Drop still sets stop, notifies all and joins. The public signatures listed in FR2 are unchanged and no added item is `pub` | All pass. Inspection confirms each point | Unit + Inspection |
| TS-7 | Targeted status_bar tests run repeatedly in parallel, with `--test-threads=1`, and oversubscribed (commands above). Also inspect the FR5–FR8 tests for thread sleeps and fixed real-time windows | Zero failures in every run. The inspection finds no sleep and no real-time window in the FR5–FR8 tests | Integration + Inspection |
| TS-8 | `cargo check` with default features and with `--no-default-features` on the Linux host. Extended at create-plan to cover NFR1 (SPEC AC6): the diff adds no crate to `src-tauri/Cargo.toml` or the lockfile | Both exit 0. No crate is added | Build + Inspection |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`. Judged
  on the two changed files only. Never run it without `--check`.
- Static analysis: no new compiler warnings from the two changed files in
  the two `cargo check` runs. Seam parts only tests use must not raise
  dead-code warnings in non-test builds.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Repeated `--lib` runs show no failures from the three rewritten tests or the regression test | Full-suite and targeted repeated runs (TS-7). Judge the four named tests |
| AC2 | The three rewritten tests and the regression test contain no thread sleep or fixed real-time window, and assert exact counts after known manual ticks | Code inspection of the four tests (TS-7 inspection) plus TS-1 to TS-4 passing |
| AC3 | The regression test passes with ticks held and then delivered, and fails under a temporary fixed real-time window check that is then reverted | TS-4 verification-time check |
| AC4 | Existing status_bar tests pass with unchanged assertions | TS-6, plus a diff showing no assertion change in the existing tests |
| AC5 | The production path keeps the timed wait for the configured interval, bumps before wake, and notifies all and joins on Drop. Public signatures are unchanged | TS-6 inspection of the diff |
| AC6 | Both `cargo check` runs succeed. No new crate in Cargo.toml or the lockfile | TS-8 |
| AC7 | The `runtime_time_provider_timer_fires_wake` comment no longer claims an initial git-branch wake when cwd_source returns None | Code inspection of the test comment (TS-3 review) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 (manual source drives the loop), TS-6 (production source in place) |
| FR2 | task0001 | TS-6 |
| FR3 | task0001 | TS-1, TS-2 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-1 |
| FR6 | task0001 | TS-2 |
| FR7 | task0001 | TS-3 |
| FR8 | task0001 | TS-4 |
| NFR1 | task0001 | TS-8 |
| NFR2 | task0001 | TS-7 |
| NFR3 | task0001 | TS-8 |
| NFR4 | task0001 | TS-6 |

## E2E Testing

Not applicable. The project has no E2E framework for this area, and the
feature changes no UI.

## Manual Testing (E2E Not Possible)

- [ ] Production clock liveness: in a GUI build started by the user, the
  status-bar `{time}` field on an idle terminal advances once per second
  (default interval). This covers the production tick path, which no longer
  has an automated tick test. The verifier does not start a release build
  itself.

## Performance / Security Verification (if applicable)

Not applicable.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 (default, no-default-features) | 2 | 0 | 0 |
| Unit / Integration scenarios | TS-1 to TS-8 | 8 (TS-4's fixed-window check, the TS-6 and TS-8 inspections and the TS-7 inspection are verifier-performed steps) | 0 | 0 |
| Code quality | format check, warnings | 2 | 0 | 0 |
| Success criteria | AC1 to AC7 | 7 | 0 | 0 |
| Manual | production clock liveness | 0 | 0 | 1 |
