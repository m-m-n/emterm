# Verification Document: program-status-ac8-block-comment-carry-test

## Overview
**Feature**: program-status-ac8-block-comment-carry-test / **SPEC.md**: `feature-docs/program-status-ac8-block-comment-carry-test/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier, single task: `feature-docs/program-status-ac8-block-comment-carry-test/tasks/task0001.md`)

Every command below runs from the project root (NFR3). The branch-diff checks
compare against the implement base commit recorded in `workflow.yaml`
(`workflow.implement.base_commit`).

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every test passes, the two new `ac8_carry_` tests and every existing `ac8_` test included
- Test listing (TS-3): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Coverage target: not measured (the project test command reports no coverage); completeness is defined by TS-1 .. TS-4 below

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Run the FR1 (bracket form) carry test on the current scanner; apply the SPEC.md AC-1 carry-dropping mutant to the scanner's end-of-text return temporarily and run it again; revert the mutant | Passes on the current scanner; fails under the mutant with no violation returned; after the revert the scanner has no diff against the base | Unit |
| TS-2 | Same three steps for the FR2 (quote form) carry test | Passes on the current scanner; fails under the mutant, where the only violation is the end-of-source violation quoting the first line and no winit violation is returned; after the revert the scanner has no diff against the base | Unit |
| TS-3 | Run the test listing command; read AC-3 of `test-docs/program-status-ac8-attribute-comments/task0001.tests.yaml` | Each of the five AC-3 `tests` entries (three existing, two added) appears as a `<name>: test` line; `red_reason` names the two carry tests as the guard against a dropped block-comment carry, states that the line-spanning test passes under that mutant and no longer claims it is that guard; the nested-comment and `/*/` text and `red_confirmed: true` are unchanged | Record check |
| TS-4 | Run the full `--lib` test command; inspect the branch diff | Full run passes; outside this feature's own `feature-docs/` and `test-docs/` directories the diff is exactly `src-tauri/src/program_status/tests.rs` (added lines only: no existing test, helper, constant or checker line removed or changed) and the predecessor record (AC-3 only); `src-tauri/src/program_status.rs` and `feature-docs/program-status-ac8-attribute-comments/tasks/task0001.md` have no diff | Unit + Diff |

## Code Quality Verification
- Format: `CARGO_TARGET_DIR=src-tauri/target cargo fmt --manifest-path src-tauri/Cargo.toml --check` (workflow.yaml format_command with the NFR3 prefix) — expected: exit code 0
- Static analysis: none declared in workflow.yaml

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The FR1 test passes on the current implementation and fails under the carry-dropping mutant | TS-1 |
| AC-2 | The FR2 test passes on the current implementation and fails under the same mutant | TS-2 |
| AC-3 | Predecessor AC-3 lists the FR1 and FR2 test names (each resolving in the `--list` output) and its `red_reason` names them as the guard, without claiming the line-spanning test is that guard | TS-3 |
| AC-4 | The full `--lib` run passes with every existing ac8 test unchanged; the mutant is not in the final diff; the change set outside this feature's own docs is limited per NFR1 | TS-4 |
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below |
| SC-2 | All test scenarios pass | TS-1 .. TS-4 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4 |
| NFR1 | task0001 | TS-4 |
| NFR2 | task0001 | TS-3 |
| NFR3 | task0001 | TS-4 (every cargo command in this document and in the task test record carries `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`, run from the project root) |

## Manual Testing (E2E Not Possible)
- [ ] TS-3: read the AC-3 `red_reason` of the predecessor record against the criteria in the TS-3 row
- [ ] TS-1 / TS-2: confirm from the task test record that the mutant was applied only temporarily and reverted (the red evidence is recorded there)

## Performance / Security Verification (if applicable)
- Not applicable: THREAT-MODEL.md verdict is `no-trust-boundary` (no TM-n), and SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build / format | 2 | 2 | 0 | 0 |
| Unit with temporary mutant (TS-1, TS-2) | 2 | 2 | 0 | 0 |
| Record check (TS-3) | 1 | 0 | 0 | 1 |
| Unit + diff (TS-4) | 1 | 1 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 6 | 5 | 0 | 1 |
