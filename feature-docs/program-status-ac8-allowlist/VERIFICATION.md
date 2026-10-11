# Verification Document: program-status-ac8-allowlist

## Overview
**Feature**: program-status-ac8-allowlist / **SPEC.md**: `feature-docs/program-status-ac8-allowlist/SPEC.md` / **IMPLEMENTATION.md**: not created (tier `reduced`, single task, no file declared by more than one task) / **THREAT-MODEL.md**: `feature-docs/program-status-ac8-allowlist/THREAT-MODEL.md`

## Build Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (src-tauri-cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both commands.
- Both run from the integration worktree root, without changing directory.

## Test Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0. Every test under `program_status::tests` passes, including the new `ac8_allowlist_*` self-tests, `ac8_the_module_uses_only_always_built_crates`, `ac8_the_module_is_declared_without_a_gui_gate` and the `rank_unify_*` tests.
- Coverage target: not measured (workflow.yaml `project.components` configures no coverage tool). Completeness is judged by the scenario table below, where every SPEC.md requirement maps to at least one scenario.
- This feature changes only `program_status` tests. A failure outside `program_status::tests` is re-checked against the `implement` base commit before it is attributed to this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `ac8_the_module_uses_only_always_built_crates` runs the checker on the embedded text of `src-tauri/src/program_status.rs` | The test exists under this exact name and passes: no violation, since the file's only project path is `use super::agent_status::AgentState;` | Unit |
| TS-2 | Allowed synthetic sources `use super::agent_status::AgentState;`, `use std::fmt;`, `use base64::Engine as _;` | Each returns no violation | Unit |
| TS-3 | Prefix case `use super::agent_status_exit_latch::X;` | Returns a violation | Unit |
| TS-4 | `use crate::agent_status::AgentState;` | Returns a violation (`crate::` is always rejected) | Unit |
| TS-5 | `pub use super::tabs::X;` | Returns a violation (`pub use` is checked like `use`) | Unit |
| TS-6 | Non-use lines `let x = crate::foo::bar();` and `super::tabs::f();` | Each returns a violation | Unit |
| TS-7 | A use line rooted at another external crate, `use winit::event::Event;` | Returns a violation | Unit |
| TS-8 | Content of a violation's message | Mentions the `gui` feature invariant and the offending path | Unit |
| TS-9 | Integrated run of the src-tauri `test_command` and the src-tauri-cli `build_command` from workflow.yaml | Both exit 0; every `program_status` test passes, including `ac8_the_module_is_declared_without_a_gui_gate` and the `rank_unify_*` tests. SPEC.md's `--lib program_status` filter is a subset of this run | Integration |
| TS-10 | Edge case from SPEC.md: grouped import `use super::{agent_status::AgentState};` | Returns a violation | Unit |
| TS-11 | Edge case from SPEC.md: a `pub(crate) use` line, `pub(crate) use super::tabs::X;` | Returns a violation, the same as the `pub use` form | Unit |
| TS-12 | Change-set confinement: the paths changed between the `implement` base commit and the integration branch tip | Apart from `feature-docs/program-status-ac8-allowlist/` and `test-docs/program-status-ac8-allowlist/`, the only changed path is `src-tauri/src/program_status/tests.rs`. No Cargo manifest or lock file changes. `src-tauri/src/program_status.rs`, `src-tauri/src/lib.rs`, `test-docs/osc7501-program-status/task0001.tests.yaml` and `test-docs/pane-state-rank-unify/task0001.tests.yaml` are unchanged | Integration (diff inspection) |

## Code Quality Verification
- Format: not configured (`format_command` is empty in workflow.yaml).
- Static analysis: not configured. The build commands above are the compile-level check.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The ac8 check decides with the explicit allowlist `super::agent_status` over every `crate::` / `super::` occurrence in program_status.rs; the real file passes; FR6's rejected cases are violations and its allowed cases are not | TS-1, TS-2, TS-3, TS-4, TS-5, TS-6, TS-7, TS-10 and TS-11 pass |
| AC-2 | The ac8 failure message states the invariant it protects: program_status builds without the `gui` feature | TS-8 passes |
| SC-1 | All functional requirements are implemented and tested | Every row of the coverage table below has a task and passing scenarios |
| SC-2 | All test scenarios pass | TS-1 to TS-12 pass |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6 |
| FR2 | task0001 | TS-2, TS-3, TS-4, TS-10 |
| FR3 | task0001 | TS-2, TS-5, TS-7, TS-11 |
| FR4 | task0001 | TS-8 |
| FR5 | task0001 | TS-1 |
| FR6 | task0001 | TS-2, TS-3, TS-4, TS-5, TS-6, TS-7, TS-8, TS-10, TS-11 |
| FR7 | task0001 | TS-1, TS-12 |
| NFR1 | task0001 | TS-12 |
| NFR2 | task0001 | TS-9 |
| NFR3 | task0001 | TS-12 |

## Manual Testing (E2E Not Possible)
None. The change is test code only, with no UI and no runtime behavior change. The design step was skipped, so there is no mockup comparison.

## Performance / Security Verification (if applicable)
Not applicable. THREAT-MODEL.md's verdict is `no-trust-boundary`, so there are no TM-n items. SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests (TS-1 to TS-8, TS-10, TS-11) | 10 | 10 | 0 | 0 |
| Integration run (TS-9) | 1 | 1 | 0 | 0 |
| Change-set inspection (TS-12) | 1 | 1 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 14 | 14 | 0 | 0 |
