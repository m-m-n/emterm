# Verification Document: program-status-ac8-allowlist-gaps

## Overview
**Feature**: program-status-ac8-allowlist-gaps / **SPEC.md**: `feature-docs/program-status-ac8-allowlist-gaps/SPEC.md` / **IMPLEMENTATION.md**: not written (reduced tier; one task, no file declared by more than one task) / **THREAT-MODEL.md**: `feature-docs/program-status-ac8-allowlist-gaps/THREAT-MODEL.md` (verdict `no-trust-boundary`)

All commands run from the project root.

## Build Verification
- Command (component `src-tauri`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (component `src-tauri-cli`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured. Completeness is judged by every scenario below mapping to at least one passing test or inspection.

### Test Scenarios from SPEC.md

TS-1 to TS-13 are SPEC.md's Unit Tests. TS-14 is derived from NFR3, TS-15
from SPEC.md's Integration Tests item (NFR4), TS-16 from NFR1 and TS-17 from
NFR2. Inputs written with `<TAB>`, `<LF>` and `<U+0301>` contain U+0009, a
line feed and U+0301 respectively.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `pub (crate) use winit::event::Event;` fed to `ac8_check_allowlist` | At least one violation | Unit |
| TS-2 | `use<TAB>winit::event::Event;` | At least one violation | Unit |
| TS-3 | `pub(crate)use winit::event::Event;` | At least one violation | Unit |
| TS-4 | `pub<TAB>use winit::event::Event;` | At least one violation | Unit |
| TS-5 | `use super::agent_status<U+0301>::X;` | At least one violation | Unit |
| TS-6 | `use super::agent_status as status;` and `super::agent_status` followed by `)` | At least one violation each | Unit |
| TS-7 | `use super::agent_status`, `use super::agent_status;`, `f(super::agent_status, 1)`, `super::agent_status::AgentState` | No violation | Unit |
| TS-8 | `use<LF>winit::event::Event;` | At least one violation | Unit |
| TS-9 | `pub use<LF><LF>winit::event::Event;` (blank line between) | At least one violation | Unit |
| TS-10 | `#[cfg(feature = "gui")] use winit::event::Event;` | At least one violation | Unit |
| TS-11 | `#[cfg(all(unix, feature = "gui"))] pub use winit::event::Event;` | At least one violation | Unit |
| TS-12 | Existing `ac8_allowlist_ac2_*` allowed snippets, and `#[derive(Debug)]` lines with no use statement | No violation | Unit |
| TS-13 | `ac8_the_module_uses_only_always_built_crates` against `program_status.rs` | No violation; test passes | Unit |
| TS-14 | Every violation returned for TS-1 to TS-6 and TS-8 to TS-11 | Contains the offending line text, `super::agent_status`, and `without the `gui` feature` | Unit |
| TS-15 | The test command above | Exit code 0; every ac8 and ac8_allowlist test, and every new self-test, passes | Integration |
| TS-16 | The integrated change set from `workflow.implement.base_commit` to the integration branch tip | Outside `feature-docs/program-status-ac8-allowlist-gaps/**` and `test-docs/program-status-ac8-allowlist-gaps/**`, only `src-tauri/src/program_status/tests.rs` changes; `program_status.rs`, `lib.rs` and `agent_status.rs` are unchanged | Inspection |
| TS-17 | The same change set | `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` are unchanged; no crate or dev-dependency is added | Inspection |

## Code Quality Verification
- Format: not configured (`format_command` is empty for both components in workflow.yaml)
- Static analysis: no separate command. The two build commands above succeed, and the test command's compiler output shows no new warning from `src-tauri/src/program_status/tests.rs` (for example, no helper left unused after the identifier-continuation judgement is replaced)

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Whitespace forms of use lines are recognized and rejected | TS-1 to TS-4 pass |
| AC-2 | `super::agent_status` is allowed only before end of text, `::`, `;` or `,` | TS-5 to TS-7 pass |
| AC-3 | A bare use line and a leading same-line attribute are recognized and rejected | TS-8 to TS-11 pass |
| AC-4 | Existing behaviour preserved | TS-12, TS-13 pass; existing ac8 / ac8_allowlist test names unchanged |
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below has a passing verification for every ID |
| SC-2 | All test scenarios pass | TS-1 to TS-17 pass |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-4 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-8, TS-9 |
| FR4 | task0001 | TS-10, TS-11 |
| FR5 | task0001 | TS-5, TS-6, TS-7 |
| FR6 | task0001 | TS-1 to TS-13 |
| NFR1 | task0001 | TS-16 |
| NFR2 | task0001 | TS-17 |
| NFR3 | task0001 | TS-14 |
| NFR4 | task0001 | TS-15 |

## Manual Testing (E2E Not Possible)
- None. Every scenario is an automated test or a change-set inspection; the feature has no UI and the design step was skipped.

## Performance / Security Verification (if applicable)
- Not applicable. THREAT-MODEL.md's verdict is `no-trust-boundary` (no TM-n), and SPEC.md states no performance requirement.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit test scenarios (TS-1 to TS-14) | 14 | 14 | 0 | 0 |
| Integration test scenario (TS-15) | 1 | 1 | 0 | 0 |
| Change-set inspection (TS-16, TS-17) | 2 | 2 | 0 | 0 |
| Code quality (no new warning) | 1 | 1 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 20 | 20 | 0 | 0 |
