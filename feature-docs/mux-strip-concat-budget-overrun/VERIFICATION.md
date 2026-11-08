# Verification Document: mux-strip-concat-budget-overrun

## Overview
**Feature**: mux-strip-concat-budget-overrun / **SPEC.md**: `feature-docs/mux-strip-concat-budget-overrun/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-strip-concat-budget-overrun/IMPLEMENTATION.md`

The target test is `mux::ipc::pty_spawn::tests::strip_concat_query::strip_concat_alternations_and_chains_finish_within_the_budget`.
Every command runs from the integration worktree root. No command puts an
intentional load on the CPU (NFR2): each one is a single-threaded test run, a
compile check or a diff.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0; the target test passes in this serial run.
- Coverage target: not measured (workflow.yaml configures no coverage tool for
  this component); the scenarios below define the required coverage.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run the target test alone, single-threaded, with its output shown: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib strip_concat_alternations_and_chains_finish_within_the_budget -- --test-threads=1 --show-output` (SPEC AC-1, AC-2) | Exit code 0. The verdict line the target test writes shows a thread CPU total of 5 s or less against the 10 s budget on the development machine's debug build | Unit |
| TS-2 | Static check of the target test, reading `src-tauri/src/mux/ipc/pty_spawn/tests/strip_concat_query.rs` and its diff against `workflow.implement.base_commit` (SPEC AC-3) | The test loops over every `TARGETS` entry and both shapes (open CSI, lone ESC); it asserts in the test that each one-call input is longer than `SCROLLBACK_FILTER_PENDING_CAP`; it has the unit-aligned read loop, the 64 KiB read loop and the ESC chain block, with their output assertions; the diff leaves the `TARGETS`, `Target` and `BUDGET` definitions and the test's name unchanged | Static review |
| TS-3 | Scaling-ratio tests of the new module `strip_scaling_ratio`: the real `strip_pty_output_for_scrollback_write` and the real one-call `ScrollbackWriteFilter::feed` (input above the cap) at N and 4N (SPEC AC-5) | Both pass: each output equals its expected bytes and each 4N / N thread CPU-time ratio is below 8; each smaller measurement written to the test output is about 100 ms or more | Unit |
| TS-4 | Scaling-ratio judgment tests in `thread_cpu_time.rs`: fixed-duration cases, and a linear and a quadratic surrogate measured at N and 4N (SPEC AC-5) | The linear surrogate is judged linear and the quadratic one not linear; a ratio just below 8 is linear, exactly 8, above 8 and a zero smaller time are not | Unit |
| TS-5 | Run the module serially: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib mux::ipc::pty_spawn::tests -- --test-threads=1` (SPEC AC-6) | Exit code 0; the target test and the new tests of TS-3 and TS-4 are among the passed tests | Integration |
| TS-6 | Review the FR1 record in the target test's doc comment (SPEC AC-4) | The record states the measurement conditions (debug build, single-threaded run of the target test alone, `CpuMeter`, sizes N and 4N with every one-call input above the cap); the thread CPU time of each measured entry (both strips, the one-call feed, the unit-aligned reads, the 64 KiB reads, the ESC chain calls) per shape at N and 4N with their ratio; the largest per-cell ratio and its cell; the linear or non-linear verdict under IMPLEMENTATION.md D1; the branch taken; and the post-change total | Documentation review |
| TS-7 | Production output identity (NFR3, TM-1): the Test Verification command above, plus the diff of `src-tauri/src/mux/scrollback_filter.rs` and `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` against `workflow.implement.base_commit` | The serial `--lib` run passes with no existing test modified or removed. When the FR1 record's verdict is linear, both production files have no diff. When it is non-linear, the record shows every previously non-linear cell's ratio below 8 and the existing strip and write-filter tests pass unchanged | Integration + static |
| TS-8 | Windows compile of the test code (NFR4): `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml` | Exit code 0: the target test, the scaling-ratio judgment and the new module compile against the Windows thread-times clock | Manual |

## Code Quality Verification
- Format: workflow.yaml configures no format command (`format_command` is
  empty). No crate-wide formatter is run.
- Static analysis: none configured.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Every functional requirement is implemented and tested | Functional Requirements Coverage below: every requirement has a task and a passing scenario |
| SC-2 | Every test scenario passes | TS-1 to TS-8 |
| SC-3 | The Performance Goals are met: target test total 5 s or less; 4N / N ratio below 8; each FR3 measurement about 100 ms or more | TS-1 (verdict line), TS-3 (ratios and written times) |
| SC-4 | The FR1 breakdown and verdict are recorded | TS-6 (the record lives in the target test's doc comment; see IMPLEMENTATION.md Open Questions) |
| SC-5 | Code review is complete | The review phase result in workflow.yaml |
| AC-1 | The reproduction command passes on a debug build | TS-1 |
| AC-2 | The target test's measured total is 5 s or less; `BUDGET` stays 10 s | TS-1, TS-2 |
| AC-3 | 8 `TARGETS` × 2 shapes, both strips, three feed modes, the ESC chain, the cap assertion, shared definitions unchanged | TS-2, with the cap assertions executed by TS-1 |
| AC-4 | The FR1 breakdown and verdict are recorded | TS-6 |
| AC-5 | The scaling-ratio test passes on the real paths; the judgment rejects the quadratic surrogate and accepts the linear one | TS-3, TS-4 |
| AC-6 | The serial module run passes the target test and the new tests | TS-5 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-6 |
| FR2 | task0001 | TS-1, TS-2, TS-5 |
| FR3 | task0002 | TS-3, TS-4, TS-5 |
| NFR1 | task0001, task0002 | TS-1 (the verdict is thread CPU time through `CpuMeter`), TS-3 |
| NFR2 | task0001, task0002 | TS-1, TS-5 (single-threaded runs only; no load or stress run anywhere in this document) |
| NFR3 | task0001 | TS-7 |
| NFR4 | task0002 | TS-4, TS-8 |

## Manual Testing (E2E Not Possible)
- [ ] TS-2: read the target test and its diff against the implement base commit.
- [ ] TS-6: read the FR1 record in the target test's doc comment.
- [ ] TS-8: run the Windows compile check with the cross toolchain that `make setup` installs.

## Performance / Security Verification (if applicable)
- Target test total (SPEC AC-2): 5 s or less of thread CPU time on the
  development machine's debug build, single-threaded — read from the verdict
  line of TS-1. The test itself asserts only the unchanged 10 s `BUDGET`.
- Scaling ratio (FR3): 4N / N below 8 for the real write-path strip and the real
  one-call feed — TS-3.
- FR3 measurement size: each smaller measurement about 100 ms or more on a debug
  build — the times TS-3 writes to its output.
- No intentional CPU load (NFR2): no load test, stress test or repeated run is
  part of this verification; every command above is a single run.
- TM-1: production output stays byte-identical across a performance change at
  the PTY-output boundary — checked by TS-7 (serial suite with unchanged
  existing tests, and the production-file diff by verdict).
- TM-2: a super-linear cost in the strip or the one-call write-filter feed is
  detected — checked by TS-3 passing on the real paths, with TS-4 showing that
  the same judgment rejects a quadratic cost.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test suite (serial `--lib`) | 1 | 1 | 0 | 0 |
| Test scenarios (TS-1 to TS-8) | 8 | 5 | 0 | 3 |
| Performance (AC-2 total, ratio, measurement size, NFR2) | 4 | 3 | 0 | 1 |
| Threat mitigations (TM-1, TM-2) | 2 | 2 | 0 | 0 |
