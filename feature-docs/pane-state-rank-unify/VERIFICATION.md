# Verification Document: pane-state-rank-unify

## Overview
**Feature**: pane-state-rank-unify / **SPEC.md**: `feature-docs/pane-state-rank-unify/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier; the feature has a single task, task0001, so no file is shared between tasks) / **THREAT-MODEL.md**: `feature-docs/pane-state-rank-unify/THREAT-MODEL.md` (verdict: no-trust-boundary)

All commands run from the integration worktree root, without changing directory.

## Build Verification
- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only, NFR2 / TS-4): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both commands

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Test listing (TS-3 name check): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Coverage target: not set (no coverage tool is configured in workflow.yaml); every task0001 Acceptance Criterion maps to at least one scenario below
- Known unrelated flakiness: the `tabs` replay tests can fail non-deterministically under parallel execution, and the `tmux_sockets` discover tests rarely fail under parallel execution. A failure confined to those tests is re-run with `--test-threads=1` before it is treated as a regression of this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | For every ordered pair (a, b) of `ProgramState::ALL` (25 pairs), compare the ordering of the `ProgramState::rank` values with the ordering of the `AgentState::compose_rank` values of the mapped `AgentState` values; for every `ProgramState`, compare the mapped `AgentState`'s display word with the `ProgramState`'s protocol word | All 25 orderings agree; all five words are equal | Unit |
| TS-2 | For every `ProgramState` s, compare the `ProgramState::rank` value of s with the `AgentState::compose_rank` value of its mapped `AgentState` | Equal for all five states | Unit |
| TS-3 | Run the full `--lib` suite, including `program_status::tests::ac5_rank_is_blocked_working_error_done_idle`, the `program_status::tests::ac5_*` `Table::summary` tests (a tie goes to the more recently updated record; a higher rank beats recency) and the `agent_status::tests::compose_*` tests (a tie goes to the OSC 7501 side) | All pass; each named test still appears under its current name in the test listing; the diff of `src-tauri/src/program_status/tests.rs` is additions only and the test module in `src-tauri/src/agent_status.rs` has no changed lines | Regression (Unit) |
| TS-4 | Run the `--no-default-features` cargo check | Compiles with exit code 0 | Build |
| TS-5 | Review the doc comments of `AgentState::compose_rank` and `ProgramState::rank` against FR4 / FR5 | The `compose_rank` doc names itself as the single definition, names `ProgramState::rank` / `Table::summary` as derived users and describes `agent_status_model`'s `priority_rank` as a separately defined, unseen-aware cross-pane order not derived from it; the `ProgramState::rank` doc refers to `compose_rank` as the source of the order | Review |

## Code Quality Verification
- Format: not configured (workflow.yaml `project.components.src-tauri.format_command` is empty)
- Static analysis: not configured
- Structural checks (by diff inspection, task0001 AC-1 / AC-2 / AC-7): `src-tauri/src/program_status.rs` has no rank numeric literal for the five states; the ProgramState -> AgentState correspondence has no wildcard arm and no optional result; `src-tauri/src/agent_status.rs` gains no reference to `program_status`

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements (FR1-FR5) are implemented and tested | Functional Requirements Coverage table below: TS-1, TS-2 pass and TS-5 review passes |
| SC-2 | All test scenarios (TS-1 to TS-5) pass | Test Verification and Manual Testing sections |
| SC-3 | NFR1-NFR3 are satisfied | TS-3 and TS-4 pass |
| SC-4 | Code review is completed | Review phase result for this feature |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2 (unit) plus the structural check that `program_status.rs` has no rank literal |
| FR2 | task0001 | TS-2 (unit) plus the structural check that the correspondence has no wildcard arm |
| FR3 | task0001 | TS-1 (unit) |
| FR4 | task0001 | TS-5 (doc review) |
| FR5 | task0001 | TS-5 (doc review) |
| NFR1 | task0001 | TS-3 (regression suite, tie-break tests included) |
| NFR2 | task0001 | TS-4 (`--no-default-features` check) |
| NFR3 | task0001 | TS-3 (names present in the test listing, existing test code unchanged in the diff) |

## Manual Testing (E2E Not Possible)
- [ ] TS-5: read the doc comments of `AgentState::compose_rank` (`src-tauri/src/agent_status.rs`) and `ProgramState::rank` (`src-tauri/src/program_status.rs`) and confirm each statement required by FR4 / FR5

## Performance / Security Verification (if applicable)
- Not applicable: SPEC.md states no performance requirement, and THREAT-MODEL.md's verdict is no-trust-boundary (no TM-n).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (default features, TS-4) | 2 | 2 | 0 | 0 |
| Unit (TS-1, TS-2) | 2 | 2 | 0 | 0 |
| Regression (TS-3) | 1 | 1 | 0 | 0 |
| Doc review (TS-5) | 1 | 0 | 0 | 1 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 6 | 5 | 0 | 1 |
