# Verification Document: agent-exit-after-icon-tests-yaml-name-drift

## Overview
**Feature**: agent-exit-after-icon-tests-yaml-name-drift / **SPEC.md**: `feature-docs/agent-exit-after-icon-tests-yaml-name-drift/SPEC.md` / **IMPLEMENTATION.md**: not produced (reduced tier, single task) / **THREAT-MODEL.md**: `feature-docs/agent-exit-after-icon-tests-yaml-name-drift/THREAT-MODEL.md` (verdict: no-trust-boundary)

All commands run from the project root on Linux. The guard test lives in a
module that builds only on Unix, so it is verified on Linux only.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not applicable (no coverage tooling is configured; the
  change adds one guard test and corrects one record line)
- Known flaky set: the record's `baseline_failures` (the `tabs::tests` ts*
  tests and `tabs::tests::welcome_without_windows_leaves_group_none`). When
  they flake, re-run with `-- --test-threads=1`.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Red check: the FR3 guard test run against the unfixed record (old name on line 51) | The guard fails on checks (a), (b) and (c); the failure output names the old name. Verified from the implement-phase red record for task0001 (`red_confirmed: true`) | Unit |
| TS-2 | The FR3 guard test run after FR1 is applied | The guard passes | Unit |
| TS-3 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list` | The output contains `mux::upgrade::tests::rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path: test`, and a `<name>: test` line for every other `mux::upgrade::tests` name the record lists (AC-2 and AC-4 entries) | Command |
| TS-4 | `git diff <base_revision> -- test-docs/agent-exit-after-icon/task0006.tests.yaml` | Exactly one changed line, line 51; `red_reason` (with its old-name mention on lines 57-58) and the supersede comment on lines 45-46 are unchanged; no supersede note is added | Command |
| TS-5 | Full `--lib` suite (Test Verification command above) | Failures, if any, are within the record's `baseline_failures` set | Command |
| TS-6 | `git diff --stat <base_revision> -- src-tauri/src` and `git diff <base_revision> -- src-tauri/src/mux/upgrade/tests.rs` | Only `src-tauri/src/mux/upgrade/tests.rs` changes under `src-tauri/src`; its diff has added lines only, adding exactly one test function; no existing test function is renamed or modified | Command |

`<base_revision>` is the feature's implement base commit
(`workflow.implement.base_commit` in workflow.yaml).

## Code Quality Verification
- Format: not configured (`project.components.src-tauri.format_command` is
  empty); no format gate. Crate-wide formatting is not run.
- Static analysis: the Build Verification command above

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SPEC AC-1 | Record line 51 reads the current name, and that name appears as a `: test` line in the `--list` output | TS-2, TS-3 |
| SPEC AC-2 | The record's diff against the base revision is exactly one line, line 51 | TS-4 |
| SPEC AC-3 | The FR3 test passes on the fixed record, fails with the old name on line 51, and fails for any `mux::upgrade::tests::*` entry without a matching definition | TS-1, TS-2 |
| SPEC AC-4 | Full `--lib` run has no new failures; the only `src-tauri/src` change is the added test | TS-5, TS-6 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-3 |
| FR2 | task0001 | TS-4 |
| FR3 | task0001 | TS-1, TS-2 |
| NFR1 | task0001 | TS-2, TS-3 |
| NFR2 | task0001 | TS-5, TS-6 |
| NFR3 | task0001 | TS-5 |

## Manual Testing (E2E Not Possible)
- None. Every scenario above is a unit test or a command check.

## Performance / Security Verification (if applicable)
- Not applicable. THREAT-MODEL.md's verdict is `no-trust-boundary`, so no
  TM-n item exists, and SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests (TS-1, TS-2) | 2 | 2 | 0 | 0 |
| Command checks (TS-3, TS-4, TS-5, TS-6) | 4 | 4 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 7 | 7 | 0 | 0 |
