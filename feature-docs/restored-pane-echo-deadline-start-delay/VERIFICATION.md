# Verification Document: restored-pane-echo-deadline-start-delay

## Overview

**Feature**: restored-pane-echo-deadline-start-delay / **SPEC.md**: `feature-docs/restored-pane-echo-deadline-start-delay/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier, single task, no cross-task decisions) / **Task plan**: `feature-docs/restored-pane-echo-deadline-start-delay/tasks/task0001.md`

The change is test-only: under `src-tauri/`, only
`src-tauri/src/mux/session/pane/tests.rs` changes. Every command below runs
from the project root (the integration worktree root), never from inside
`src-tauri/`.

Load rule (NFR1): no verification step puts intentional load on the CPU. Do
not run anything with busy-loop threads or spin loops, `stress` /
`stress-ng`, or one-process-per-core / core-count parallel runs. The task's
original reproduction step (busy loops on every logical CPU) is not carried
out; TS-3 verifies the symptom deterministically instead (SPEC A3).

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every test in the `restored_echo_wait` module passes
- Coverage target: no coverage tool is configured for this project; coverage
  is judged by the scenario table below (every FR/NFR maps to at least one
  scenario)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run `read_echo_until_marker_reports_the_deadline_with_the_collected_bytes` normally (withheld-data script) | Passes: reason `DeadlinePassed(SHORT_DEADLINE)`, elapsed < `GENEROUS_BOUND`, collected bytes empty or exactly `restored-` (normally `restored-`), failure text contains `deadline`, `200ms` and the rendering of the collected bytes | Unit |
| TS-2 | Run `read_echo_until_marker_deadline_is_absolute_while_non_marker_bytes_trickle_in` normally (trickle script) | Passes: reason `DeadlinePassed(SHORT_DEADLINE)`, elapsed < `GENEROUS_BOUND`, collected bytes empty or `restored-` followed only by `.` bytes (normally starting with `restored-`), failure text contains `deadline`, `200ms` and the rendering of the collected bytes | Unit |
| TS-3 | Delayed-start test: a gated scripted reader whose first read is held until the test releases it after `read_echo_until_marker` returns, for the withheld-data and the trickle script | Passes: `DeadlinePassed(SHORT_DEADLINE)`, empty collected bytes, the shared deadline check passes. Red check: the test-docs record of task0001 shows the same test failing on the collected-bytes check against the strict pre-change rules | Unit |
| TS-4 | `describe_missing_echo` with a constructed missing-echo value: reason `DeadlinePassed(SHORT_DEADLINE)`, collected bytes `restored-` | Text contains `"restored-"` (with the double quotes), `deadline` and `200ms` | Unit |
| TS-5 | Full `--lib` run, `cargo check`, and diff inspection against the implement step's `base_commit` recorded in workflow.yaml | `--lib` suite passes with every `restored_echo_wait` test passing; `cargo check` exits 0; the list of changed files under `src-tauri/` is exactly `src-tauri/src/mux/session/pane/tests.rs`; `Cargo.toml` and `Cargo.lock` are unchanged | Integration |

### Edge Cases

- [ ] The reader thread starts after the deadline: collected bytes are empty,
  the reason is still `DeadlinePassed`, and the failure text shows the
  rendering of the empty collection (TS-3).
- [ ] The reader thread starts just before the deadline in the trickle test:
  collected bytes are `restored-` plus zero or more `.` bytes (TS-2).
- [ ] Withheld-data test: the 64-byte read buffer hands out `restored-` as one
  chunk, so collected bytes are empty or exactly `restored-`, never a partial
  prefix (TS-1).
- [ ] The injected start delay is released by the test after the call
  returns; if it never is, `SCRIPTED_READER_SAFETY_LIMIT` (6 s from reader
  construction) ends the reader, so the suite does not hang (TS-3, TS-5).
- [ ] Main-thread wake-up delay is covered only by the upper bound
  `GENEROUS_BOUND` (3 s); no lower bound on elapsed time is asserted
  (TS-1, TS-2, TS-3).

## Code Quality Verification

- Format: none configured (workflow.yaml `format_command` is empty)
- Static analysis: none configured

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-4 pass |
| SC-2 | All test scenarios pass | TS-1 to TS-5 pass |
| SC-3 | The reproduction no longer triggers the symptom | TS-3 passes (deterministic start delay replaces the busy-loop reproduction, SPEC A3 / NFR1) |
| SC-4 | A test detects the regression | TS-3 passes and its red check is recorded |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-4 |
| NFR1 | task0001 | TS-3 (deterministic delay injection without load), TS-5 (diff inspection for load-generating code) |
| NFR2 | task0001 | TS-5 |
| NFR3 | task0001 | TS-3 (start gate released by the test), TS-5 (suite run completes) |
| NFR4 | task0001 | TS-5 |

## E2E Testing

None. The project has no E2E suite for this area (SPEC: no
`docker-compose.e2e.yml`, no `e2e-tests/`).

## Manual Testing (E2E Not Possible)

Diff inspection by the verifier; no human run of the application is needed.

- [ ] NFR1: the diff adds no busy-loop thread, spin loop, stress tool or
  core-count parallel work; the start gate waits by blocking (a timed wait
  or sleeps between flag checks).
- [ ] NFR3: the gated first read stops waiting at
  `SCRIPTED_READER_SAFETY_LIMIT` measured from reader construction, the same
  limit the existing safety limit uses.

## Performance / Security Verification

- NFR1 (no intentional CPU load): diff inspection (Manual Testing above) and
  TS-3 (the start delay is injected by a gate, not by load).
- NFR3 (bounded tests): TS-3 releases the gate after the call returns; the
  safety-limit bound is checked by diff inspection (Manual Testing above);
  the TS-5 suite run completes.
- Threat model: verdict `no-trust-boundary` (THREAT-MODEL.md); no TM-n items.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit test scenarios (TS-1 to TS-4) | 4 | 4 | 0 | 0 |
| Integration scenario (TS-5: suite, check, changed-file list) | 1 | 1 | 0 | 0 |
| Diff inspection (NFR1, NFR3) | 2 | 0 | 0 | 2 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 8 | 6 | 0 | 2 |
