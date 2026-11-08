# Verification Document: pty-spawn-parallel-budget-flake

## Overview
**Feature**: pty-spawn-parallel-budget-flake / **SPEC.md**: `feature-docs/pty-spawn-parallel-budget-flake/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/pty-spawn-parallel-budget-flake/IMPLEMENTATION.md`

Every command runs from the repository root (the integration worktree root), never
after changing into `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

The verify phase makes exactly two test runs, each once (NFR2). The results of every
scenario below are read from these two runs; no run is repeated to confirm a result.

- Run A (TS-5): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Run B (TS-6): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests -- --test-threads=1`
- Coverage target: not applicable (test-only change)

The target tests, by their libtest paths:

| Role | Test |
|------|------|
| FR1 budget | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget` |
| FR2 budget | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` |
| FR3 budget | `mux::ipc::pty_spawn::tests::strip_open_string_body_closure::open_body_closure_a_dcs_or_apc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal` |
| FR4 arm order | `mux::ipc::pty_spawn::tests::visible_reattach_redelivers_alt_screen_and_main_screen_color_queries_once` |
| FR5 arm order | `mux::ipc::pty_spawn::tests::on_demand_snapshot_redelivers_alt_screen_and_main_screen_color_queries_once` |
| FR6 guard | every test under `mux::ipc::pty_spawn::tests::budget_and_arm_order_guard` (the real-source guard test and its negative-example unit tests) |
| NFR3 sibling | `mux::ipc::pty_spawn::tests::strip_open_string_body_closure::open_body_closure_an_osc_body_left_open_by_an_overflow_flush_is_closed_at_the_next_removal` |

A failure in Run A of a test outside this table is recorded by name and is not part
of this feature's verdict. It is not re-run.

### Test Scenarios from SPEC.md

TS-7 and TS-8 are added by this plan for NFR3 and NFR4; TS-1 to TS-6 are SPEC.md's.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | The three FR1-FR3 budget tests (results from Run A and Run B) | They pass; each asserts a measured thread CPU total greater than zero and a within verdict against `BUDGET` (10 s) | Unit |
| TS-2 | The two FR4/FR5 reattach tests (results from Run A and Run B) | They pass | Unit |
| TS-3 | The real-source guard test on the changed source (results from Run A and Run B) | It passes: all five target tests are found and accepted | Unit |
| TS-4 | The guard's negative-example unit tests: wall-clock budget body, judge without measure, release-first order, a single arm, no first release, missing function name, tokens only in the following function, comment-only tokens, CR LF line ends | Each wrong form is rejected, the missing name is reported as missing by name, and the CpuMeter and arm-first forms are accepted | Unit |
| TS-5 | Run A once | None of the five target tests, the guard tests or the OSC sibling fails | Integration |
| TS-6 | Run B once | The five target tests, the guard tests and the OSC sibling pass | Integration |
| TS-7 | Diff of the feature branch against its base (NFR3) | Within `src-tauri/`, only the five files of task0001 change; no production file changes; no test is renamed; `BUDGET`, `TARGETS` and `Target` are unchanged; no wall-clock check other than the three FR1-FR3 budgets is added, removed or changed; workloads and output assertions of the five target tests are unchanged; every `recv_timeout` of the two reattach tests is still 5 s | Inspection |
| TS-8 | Windows (NFR4) | The diff adds no platform-specific code and no platform gate; the guard and the changed tests type-check for `x86_64-pc-windows-msvc` | Inspection / Manual |

## Code Quality Verification
- Format: no format command is configured (`format_command` is empty in workflow.yaml). Do not run crate-wide formatting; only the changed files may be formatted.
- Static analysis: none configured.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-6 |
| SC-2 | All test scenarios pass | TS-1 to TS-8 |
| SC-3 | Performance meets specified goals | TS-1: each FR1-FR3 measured thread CPU total is within `BUDGET` (10 s) |
| SC-4 | Documentation is complete | IMPLEMENTATION.md, task0001.md and this document exist; the test-docs record of task0001 lists the tests per acceptance criterion |
| SC-5 | Code review is completed | Review phase result in workflow.yaml |
| SC-6 | SPEC.md AC-1 to AC-5 are satisfied | AC-1: TS-1, TS-3; AC-2: TS-2, TS-3; AC-3: TS-3, TS-4; AC-4: TS-5; AC-5: TS-6 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-5, TS-6 |
| FR2 | task0001 | TS-1, TS-5, TS-6 |
| FR3 | task0001 | TS-1, TS-5, TS-6 |
| FR4 | task0001 | TS-2, TS-5, TS-6 |
| FR5 | task0001 | TS-2, TS-5, TS-6 |
| FR6 | task0001 | TS-3, TS-4, TS-5, TS-6 |
| NFR1 | task0001 | TS-1, TS-3 |
| NFR2 | task0001 | TS-5, TS-6 (each run exactly once, no repetition or load) |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0001 | TS-8 |

## E2E Testing
SPEC.md lists no existing E2E tests and no E2E run command; there is no E2E step.

## Manual Testing (E2E Not Possible)
- [ ] TS-8: when the Windows cross toolchain of `make setup` is installed, type-check the tests once for Windows with `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` (the web bundles must exist, as for any GUI build). Expected: exit code 0. Without the toolchain, TS-8 is judged by the inspection part alone and the skipped check is recorded.

## Performance / Security Verification
- FR1-FR3 performance goal: each measured thread CPU total is below `BUDGET` (10 s) — asserted inside the tests (TS-1).
- NFR1: the three budgets are judged as thread CPU time and no wall-clock budget is added — TS-1, TS-3, TS-7.
- NFR2: no intentional CPU load — Run A and Run B are each made once; no repeated runs, run loops, stress tools or core-count parallel runs. When a target test fails, report the failure and stop; do not reproduce it under load.
- Security: THREAT-MODEL.md verdict is `no-trust-boundary`; there is no mitigation item to verify.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit scenarios (TS-1 to TS-4) | 4 | 4 | 0 | 0 |
| Integration scenarios (TS-5, TS-6) | 2 | 2 | 0 | 0 |
| Inspection (TS-7) | 1 | 0 | 0 | 1 |
| Windows (TS-8) | 1 | 0 | 0 | 1 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
