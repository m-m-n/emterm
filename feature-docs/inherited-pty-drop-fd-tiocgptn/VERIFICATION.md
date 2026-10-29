# Verification Document: inherited-pty-drop-fd-tiocgptn

## Overview
**Feature**: inherited-pty-drop-fd-tiocgptn / **SPEC.md**: `feature-docs/inherited-pty-drop-fd-tiocgptn/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/inherited-pty-drop-fd-tiocgptn/IMPLEMENTATION.md`

Every command runs once, from the project root (the integration worktree
root). No verification step repeats a run, generates CPU load or relies on
sleep-based timing to show the flake is gone (NFR2).

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not applicable; the feature changes test code only, and
  coverage is by the scenarios below.

### Test Scenarios from SPEC.md
TS-1 to TS-6 come from SPEC.md; TS-7 and TS-8 are added by the plan to give
NFR1 and NFR2 a verification item.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor` alone (`--exact`) | Passes under that unchanged name, and the test body keeps the original pair open until the post-drop judgment completes. When the `--exact`-filtered command is not an approved command string, the target test's `ok` line from the TS-6 run is the evidence | Unit |
| TS-2 | Run `mux::inherited_pty::tests::closure_judgment_distinguishes_other_pty_masters`: pairs A and B open, baseline = A's pts number; judge B's master, a duplicate of B's master, a duplicate of A's master, a handle to A's slave, a temporary file, `-1` | Passes; the verdicts asserted are, in that order: not original, not original, original, not original, not original, not original | Unit |
| TS-3 | Red check: the TS-2 test with the judgment replaced by an `st_rdev` comparison | Fails at B's master case. Evidence: the red confirmation and reason recorded for that test in `test-docs/inherited-pty-drop-fd-tiocgptn/task0001.tests.yaml` | Unit (red check, record) |
| TS-4 | Failure message of the target test's post-drop assertion | Contains the fd number, the pre-drop pts number and the post-drop outcome (pts number on success, errno on failure) | Review |
| TS-5 | Comments and references | `inherited_pty.rs` contains no `stat_rdev` and no comment saying a reused descriptor refers to a different device; the target test's comment states the pts-number rationale; `upgrade/tests.rs` contains no `stat_rdev` reference | Review |
| TS-6 | One full `--lib` test run and one `--no-default-features` check | Both exit 0; every `mux::inherited_pty` test passes | Unit |
| TS-7 | (NFR1) The code diff from `workflow.implement.base_commit` to the integration branch | Code changes only inside the test section of `inherited_pty.rs` and in the `stat_dev_ino` doc comment of `upgrade/tests.rs`; `InheritedMasterPty` and its `Drop` are unchanged; no existing test is renamed | Review |
| TS-8 | (NFR2) The target test and the TS-2 test | Neither sleeps, repeats its scenario, spawns threads or generates load | Review |

## Code Quality Verification
- Format: no format command is declared (`project.components.rust.format_command` is empty).
- Static analysis: the TS-6 test build emits no new compiler warning from the
  `mux::inherited_pty` test module (for example an unused helper).

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Every row of Functional Requirements Coverage below is verified |
| SC-2 | All test scenarios pass | TS-1 to TS-8 |
| SC-3 | AC-1 to AC-5 are met | AC-1: TS-1; AC-2: TS-2, TS-3; AC-3: TS-4; AC-4: TS-5; AC-5: TS-6 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-4 |
| FR3 | task0001 | TS-2, TS-3 |
| FR4 | task0001 | TS-5 |
| FR5 | task0001 | TS-1 |
| NFR1 | task0001 | TS-7 |
| NFR2 | task0001 | TS-8 |
| NFR3 | task0001 | TS-1 |
| NFR4 | task0001 | TS-6 |

## Manual Testing (E2E Not Possible)
- None. Injection-based reconfirmation of the original failure is not
  required (SPEC A1); TS-2 builds the descriptor-reuse state in-process.

## Performance / Security Verification (if applicable)
- NFR2 (determinism): checked by TS-8; no verification step repeats a run or
  generates load.
- Security: THREAT-MODEL.md verdict is `no-trust-boundary`; there is no TM
  item.

## Verification Summary
Review items are code-reading checks done by the verify phase, not human
manual testing.

| Category | Items | Automated | E2E | Manual | Review |
|----------|-------|-----------|-----|--------|--------|
| Build (part of TS-6) | 2 | 2 | 0 | 0 | 0 |
| Unit tests (TS-1, TS-2, TS-3, TS-6) | 4 | 3 | 0 | 0 | 1 (TS-3 record) |
| Code review (TS-4, TS-5, TS-7, TS-8) | 4 | 0 | 0 | 0 | 4 |
| Security (TM-n) | 0 | 0 | 0 | 0 | 0 |
