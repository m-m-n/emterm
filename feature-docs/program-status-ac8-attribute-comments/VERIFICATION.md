# Verification Document: program-status-ac8-attribute-comments

## Overview
**Feature**: program-status-ac8-attribute-comments / **SPEC.md**: `feature-docs/program-status-ac8-attribute-comments/SPEC.md` / **IMPLEMENTATION.md**: not produced (reduced tier; the feature has one task, so there is no cross-task decision)

- Scenario IDs TS-1 to TS-10 correspond to the SPEC.md test scenarios with the same
  number. TS-11 and TS-12 are added here to verify NFR1 and NFR2, which have no SPEC.md
  scenario.
- `THREAT-MODEL.md` verdict: no-trust-boundary (no TM-n).
- Every command runs from the project root.
- In the scenario table, `\n` inside a quoted source marks a line break.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: no coverage tool is configured for this component; every requirement maps to at least one scenario below.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `ac8_check_allowlist` on `#[allow(dead_code /* [ */)]\nstruct X;\nuse winit::event::Event;` | A violation reporting use root `winit` | Unit |
| TS-2 | The source of TS-1 with `/* " */` in place of `/* [ */` | A violation reporting use root `winit` | Unit |
| TS-3 | `#[allow(dead_code /* [ */)]\nstruct X;\nuse std::fmt;` | No violation | Unit |
| TS-4 | A multi-line attribute with a line comment holding `]`, `[` or `"`, e.g. `#[cfg(all( // ]\nunix))] use winit::event::Event;` | A violation reporting use root `winit`; the attribute closes on the second line | Unit |
| TS-5 | A block comment holding `]` that spans lines inside a multi-line attribute: `#[cfg(all( /* ]\n] */ unix))]\nuse winit::event::Event;` | A violation reporting use root `winit` | Unit |
| TS-6 | A nested block comment: `#[allow(dead_code /* /* */ [ */)]\nstruct X;\nuse winit::event::Event;` | A violation reporting use root `winit` | Unit |
| TS-7 | A comment opener inside an attribute string literal: `#[doc = "/*"] use winit::event::Event;` | A violation reporting use root `winit` | Unit |
| TS-8 | An attribute that never closes: `#[cfg(all(\nunix,\nuse std::fmt;` | A violation whose text contains the line `#[cfg(all(`, `super::agent_status` and ``without the `gui` feature`` | Unit |
| TS-9 | An attribute whose block comment never closes: `#[allow(dead_code /* ]\nuse std::fmt;` | A violation naming the line holding the attribute's `#[` | Unit |
| TS-10 | The test command above | `ac8_the_module_uses_only_always_built_crates` and every existing ac8 self-test (`ac8_allowlist_*`, `ac8_gaps_*`, `ac8_forms_*`) pass with unchanged expectations | Integration |
| TS-11 | Change scope: list the files changed between `workflow.implement.base_commit` in workflow.yaml and the integration branch HEAD | Outside `feature-docs/program-status-ac8-attribute-comments/` and `test-docs/program-status-ac8-attribute-comments/`, the only changed file is `src-tauri/src/program_status/tests.rs` | Static check |
| TS-12 | Read the doc comments of `Ac8OpenAttribute`, `ac8_scan_attribute`, `ac8_after_leading_attributes` and `ac8_check_allowlist` | They describe the comment handling inside attributes and the end-of-source rule consistently with the behavior; none states the old bracket-and-quote rule without the comment exception | Manual review |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — Expected: no formatting difference reported for `src-tauri/src/program_status/tests.rs`
- Static analysis: none configured in workflow.yaml project.components

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below: every requirement maps to task0001 and to at least one passing scenario |
| SC-2 | All test scenarios pass | TS-1 to TS-12 |
| SC-3 | Code review is completed | The review step in workflow.yaml is completed |
| US1-AC1 | Comment characters inside an attribute do not affect its open/close judgement | TS-1 to TS-7 |
| US1-AC2 | A source ending with an attribute still open reports a violation naming the line holding its `#[` | TS-8, TS-9 |
| US1-AC3 | Self-tests for the reproduction source and its quote variant return a winit violation | TS-1, TS-2 |
| US1-AC4 | Existing allowlist test and self-test expectations are unchanged | TS-10 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-4 (unit) |
| FR2 | task0001 | TS-1, TS-2, TS-3, TS-5, TS-6, TS-7 (unit) |
| FR3 | task0001 | TS-8, TS-9 (unit) |
| FR4 | task0001 | TS-1, TS-2 (unit) |
| FR5 | task0001 | TS-10 (integration) |
| NFR1 | task0001 | TS-11 (static check of the branch diff) |
| NFR2 | task0001 | TS-12 (manual review) |
| NFR3 | task0001 | TS-10 (the test command form) |

## Manual Testing (E2E Not Possible)
- [ ] TS-12: doc comments of the four ac8 checker items match the behavior (NFR2)

## Performance / Security Verification (if applicable)
- Not applicable: SPEC.md states no performance requirement, and THREAT-MODEL.md's verdict is no-trust-boundary, so there is no TM-n to verify.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit test scenarios (TS-1 to TS-9) | 9 | 9 | 0 | 0 |
| Integration test (TS-10) | 1 | 1 | 0 | 0 |
| Change scope (TS-11) | 1 | 1 | 0 | 0 |
| Code quality (format) | 1 | 1 | 0 | 0 |
| Doc comment review (TS-12) | 1 | 0 | 0 | 1 |
| Total | 14 | 13 | 0 | 1 |
