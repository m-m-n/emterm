# Verification Document: lib-budget-tests-load-tolerance

## Overview

**Feature**: lib-budget-tests-load-tolerance / **SPEC.md**: `feature-docs/lib-budget-tests-load-tolerance/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/lib-budget-tests-load-tolerance/IMPLEMENTATION.md`

Every command runs from the project root of the integration worktree, with an explicit `CARGO_TARGET_DIR` and `--manifest-path` (no `cd`). TS-1 to TS-8 come from SPEC.md; TS-9 (SPEC EC3 / AC6) and TS-10 (SPEC AC7) are added here so that FR4, FR5, NFR1 and NFR2 each have a verifying scenario.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Command (Windows test code, NFR2 / AC7 — only where the cargo-xwin cross toolchain is available; otherwise record "not run" with the reason): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --lib --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors, no new warning from the changed files.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Targeted runs (same command plus a test-name filter): `thread_cpu_time` (TS-1 to TS-4, TS-9), `strip_concat_alternations_and_chains_finish_within_the_budget` (TS-6), `the_ring_at_a_cut_replays_like_the_raw_stream_over_the_token_corpus` (TS-5).
- Coverage target: not measured (test-only change). Every task Acceptance Criterion maps to a named test or to a diff / review check below.
- Known pre-existing flakes unrelated to this feature (tabs.rs replay tests under parallel execution, tmux_sockets discover) are recorded if they occur and are not attributed to this feature.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Helper ignores sleep: read thread CPU time, sleep a fixed 200-500 ms, read again (task0001 AC-2) | CPU delta is less than half the sleep; the wall-clock elapsed time over the same interval does not meet that bound | Unit |
| TS-2 | Helper ignores another thread's CPU: a spawned thread consumes at least 200 ms of its own CPU while the measuring thread waits in join (task0001 AC-3) | The spawned thread reached its amount; the measuring thread's delta is less than half of it | Unit |
| TS-3 | Helper counts its own thread: CPU work of two sizes on the measuring thread (task0001 AC-1) | Both deltas positive; the larger work's delta exceeds the smaller's; the smaller's delta is at least 1% of its wall-clock duration | Unit |
| TS-4 | The FR2 budget judgment with a short budget on a quadratic and a linear surrogate of the same size (task0001 AC-5) | Quadratic judged over the budget; linear judged within it | Unit |
| TS-5 | Token-corpus test without a time judgment (task0002 AC-1) | The test body has no wall-clock reading and no `BUDGET` use; the test passes | Unit + diff |
| TS-6 | Alternations test on the CPU budget (task0001 AC-6) | The test passes: the measured calls' thread CPU total is under 10 s and every correctness assertion holds | Unit |
| TS-7 | `--lib` under synthetic CPU load at load average 10-20 (procedure below) | Neither named test fails | Load |
| TS-8 | Scope unchanged: diff of the pty_spawn tests and Cargo.toml against the implement base commit (task0001 AC-6, task0002 AC-2) | Changes only in: the new helper module file, its one declaration in `tests.rs`, the helper import and the named test in `strip_concat_query.rs`, the named test in `round4_chain.rs`, the windows-sys feature list in `src-tauri/Cargo.toml`; every other `BUDGET` / elapsed check is byte-identical | Diff |
| TS-9 | Clock failure panics (SPEC EC3, AC6; task0001 AC-4) | The status-interpretation step given a failure status panics with a message naming the clock API; review finds no zero or wall-clock fallback path | Unit + Review |
| TS-10 | Windows dependency feature and builds (SPEC AC7; task0001 AC-7) | windows-sys lists Win32_System_Threading; no crate added; the three Build Verification commands succeed (the Windows one where available) | Build / Inspection |

### TS-7 procedure (synthetic load)

1. Build the test binary first, so compilation is not done under load: the Test Verification command with `--no-run`.
2. Read the core count and the current 1-minute load average from `/proc/loadavg`.
3. Start CPU-bound busy workers with the Bash tool's `run_in_background`, enough of them that the 1-minute load average reaches 10-20 on top of the current baseline. Each worker bounds its own run time by its own logic (long enough to cover the warm-up and the test run); the shell `timeout` command is not used.
4. Wait until the 1-minute load average is within 10-20, re-reading `/proc/loadavg`.
5. Run the Test Verification command (Bash tool `timeout` parameter at most 600000 ms; if the run may take longer, start it with `run_in_background` and wait for it).
6. Stop every load worker with `TaskStop`. `kill`, `pkill` and `killall` are not used.
7. Pass: both named tests report `ok`. Failures of other tests (wall-clock budgets kept by SPEC FR9, known flakes) are recorded with their names and are not attributed to this feature.

## Code Quality Verification

- Format: workflow.yaml configures no format command. Edited files are normalized by the project's formatting hook; no crate-wide formatting is run (TS-8 shows no unrelated file changed).
- Static analysis: the Build Verification and Test Verification output shows no new compiler warning from the changed files (in particular no unused import of the time types or `BUDGET` in `round4_chain.rs` / `strip_concat_query.rs`).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Under synthetic CPU load, the `--lib` command passes the two named tests (FR1, FR2) | TS-7 |
| AC2 | The token-corpus test has no elapsed-time assertion and its replay assertions still pass (FR1) | TS-5 |
| AC3 | The alternations test asserts the thread CPU total of the strip / snapshot-strip / write-filter calls is under 10 s, and its correctness assertions still pass (FR2, FR3) | TS-6, review of the measured-call set |
| AC4 | Helper tests pass: sleep not counted, another thread's CPU not counted, own CPU counted; a wall-clock helper fails the sleep case (FR3, FR6, FR7) | TS-1, TS-2, TS-3 |
| AC5 | The control test passes: the FR2 judgment with a short budget rejects a quadratic surrogate (FR8) | TS-4 |
| AC6 | The helper panics on clock failure and has no zero / wall-clock fallback (FR4) | TS-9 (test + code review) |
| AC7 | windows-sys has Win32_System_Threading enabled; the `--lib` build compiles; the Windows cross check compiles where available (FR5, NFR2) | TS-10 |
| AC8 | Wall-clock budgets other than the two named tests are unchanged (FR9) | TS-8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0002 | TS-5, TS-7 |
| FR2 | task0001 | TS-6, TS-7 |
| FR3 | task0001 | TS-1, TS-2, TS-3 |
| FR4 | task0001 | TS-9 |
| FR5 | task0001 | TS-10 |
| FR6 | task0001 | TS-1, TS-2 |
| FR7 | task0001 | TS-3 |
| FR8 | task0001 | TS-4 |
| FR9 | task0001, task0002 | TS-8 |
| NFR1 | task0001 | TS-10 |
| NFR2 | task0001 | TS-10 |
| NFR3 | task0001 | TS-2 |
| NFR4 | task0001 | TS-4 |

## Manual Testing (E2E Not Possible)

- [ ] Windows run (where a Windows machine is available): the `--lib` tests filtered to `thread_cpu_time` and to `strip_concat_alternations_and_chains_finish_within_the_budget` pass on the Windows clock branch (TS-1 to TS-4, TS-6, TS-9 on Windows). When no Windows machine is available, record it as not run.
- [ ] Code review (SPEC AC6): no path of the helper returns zero or a wall-clock reading when the clock API reports failure.
- [ ] Code review (SPEC FR2 / EC5): the calls passed through the meter in the alternations test are exactly the write strip, the snapshot strip and every write-filter feed; input and expected-output building, comparisons and the test-side output concatenation stay outside it.

## Performance / Security Verification

- FR2 performance target: the measured calls' thread CPU total is under 10 s — TS-6, and under load TS-7.
- NFR3 (per-thread measurement): CPU time of other threads is excluded — TS-2; the parallel full `--lib` run of TS-7 exercises it with libtest's concurrent tests.
- NFR4 (detection power): the same judgment rejects a quadratic surrogate — TS-4; the measured-call review above confirms every call whose cost would turn quadratic under a regression is inside the measured total.
- Security: THREAT-MODEL.md verdict is `no-trust-boundary`; there is no TM-n to verify.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 (default check, `--no-default-features` check, Windows test-code check) | 3 (Windows one where available) | 0 | 0 |
| Unit tests | 7 (TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-9) | 7 | 0 | 0 |
| Load | 1 (TS-7) | 1 | 0 | 0 |
| Diff / inspection | 2 (TS-8, TS-10) | 2 | 0 | 0 |
| Review / manual | 3 (Windows run, AC6 review, measured-call review) | 0 | 0 | 3 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
