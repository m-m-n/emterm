# Verification Document: osc133-mark-token-exact-match

## Overview
**Feature**: osc133-mark-token-exact-match / **SPEC.md**: `feature-docs/osc133-mark-token-exact-match/SPEC.md` / **IMPLEMENTATION.md**: not produced (reduced tier, one task, no file shared between tasks) / **THREAT-MODEL.md**: `feature-docs/osc133-mark-token-exact-match/THREAT-MODEL.md`

All commands run from the project root.

## Build Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Command (src-tauri CLI-only): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Coverage target: none set; every scenario below maps to at least one test.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Fresh plain test tab; one output-pipeline call with `\x1b]133;AB\x07\x1b]7501;id=job:state=working\x07\x1b]133;A\x07` (`src-tauri/src/tabs/tests/output_pipeline.rs`) | The tab's OSC 7501 record table is empty (no `job` record) | Integration |
| TS-2 | For each first token `AB`, `Ax`, `Bx`, `Cx`, `Dx`, `D0`: OSC 133 through the screen-aware notification with the alternate screen inactive and active, and through the existing `on_osc` notification (`src-tauri/src/callbacks/tests.rs`) | `pending_program_status_feed` and `pending_latch_feed` are both empty in every case | Unit |
| TS-3 | `A`, `B`, `C`, `D`, `D;0`, `D;x`, `A;aid=1` on the main screen (`src-tauri/src/callbacks/tests.rs`) | Exactly one candidate of the matching kind in each feed | Unit |
| TS-4 | Full `--lib` suites of src-tauri and term_core, and the `--no-default-features` check (commands above) | All tests pass, including the existing OSC 133 candidate tests named in SPEC.md A4; the check compiles | Integration |
| TS-5 | SPEC.md edge cases: payloads `""` and `;A` on either screen; `D;42` on the main screen; `D;42`, `D;x`, `A;aid=1` on the alternate screen (`src-tauri/src/callbacks/tests.rs`) | Empty first token pushes nothing to either feed; `D;42` on the main screen pushes exactly one command-end candidate to each feed; well-formed marks on the alternate screen push nothing | Unit |

## Code Quality Verification
- Format: not configured (`format_command` is empty for every component in workflow.yaml)
- Static analysis: not configured

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below: every FR/NFR has task0001 and at least one passing scenario |
| SC-2 | All test scenarios pass | TS-1 to TS-5 pass under the Test Verification commands |
| SC-3 | Code review is completed | The review phase is completed in workflow.yaml |
| AC-1 | Reproduction bytes leave no `job` record | TS-1 |
| AC-2 | Malformed first tokens leave both feeds empty on both screens | TS-2 |
| AC-3 | Well-formed tokens push exactly one matching candidate per feed on the main screen; existing OSC 133 candidate tests keep passing | TS-3, TS-4 |
| AC-4 | Full `--lib` suites pass and the `--no-default-features` check compiles | TS-4 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-5 |
| FR2 | task0001 | TS-1, TS-2, TS-5 |
| FR3 | task0001 | TS-3, TS-4, TS-5 |
| NFR1 | task0001 | TS-4, plus the term_core diff check below |

### NFR1 term_core diff check
- The feature diff against `workflow.implement.base_commit` lists no path under `crates/term_core/`.
- Expected: empty list.

## E2E Testing
None: no E2E framework is configured for this project in workflow.yaml.

## Manual Testing (E2E Not Possible)
None: TS-1 runs the reproduction bytes from the task description through the plain-tab output path.

## Performance / Security Verification (if applicable)
- Performance: not applicable (SPEC.md).
- Security: THREAT-MODEL.md verdict is `no-applicable-threat`; no TM-n item.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios (TS-1 to TS-5) | 5 | 5 | 0 | 0 |
| NFR1 term_core diff check | 1 | 1 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Manual | 0 | 0 | 0 | 0 |
| Total | 9 | 9 | 0 | 0 |
