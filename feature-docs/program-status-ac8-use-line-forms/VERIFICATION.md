# Verification Document: program-status-ac8-use-line-forms

## Overview
**Feature**: program-status-ac8-use-line-forms / **SPEC.md**: `feature-docs/program-status-ac8-use-line-forms/SPEC.md` / **IMPLEMENTATION.md**: not produced (reduced tier; a single task owns the only changed file)

## Build Verification
- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (src-tauri-cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0; every `program_status::tests::ac8` test passes
- Coverage target: not applicable (no coverage tool is configured; the behavior is pinned by the scenarios below)
- If a failure is confined to tests outside `program_status` (for example the `tabs` replay tests or the `tmux_sockets` discover tests, which have been observed to be order-sensitive under parallel execution), re-run that test alone before attributing it to this feature.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Multiple leading attributes on one line (AC-1): `#[cfg(a)] #[allow(b)] use winit::event::Event;`, `#[cfg(a)]#[allow(b)] pub use winit::event::Event;`, `#[cfg(a)] #[allow(b)] pub(crate) use winit::event::Event;`; allowed: `#[cfg(test)] #[allow(unused)] use std::fmt;` | The three rejected sources each give a violation with use root `winit`; the allowed source gives none | Unit |
| TS-2 | `use` followed directly by `::` or `{` (AC-2): `use::winit::event::Event;`, `use{winit::event::Event};`, `pub use::winit::event::Event;`, `pub(crate)use{winit::event::Event};`, `use ::std::fmt;`, `use {std::fmt};` | Each source is rejected (rejection asserted only) | Unit |
| TS-3 | String literals inside attributes (AC-3): `#[cfg(feature = "]")] use winit::event::Event;`, `#[cfg(feature = "[")] use winit::event::Event;`, `#[doc = "\"]"] use winit::event::Event;`; allowed: `#[cfg(feature = "]")] use std::fmt;` | The three rejected sources each give a violation with use root `winit`; the allowed source gives none | Unit |
| TS-4 | Multi-line attributes (AC-4): a multi-line `#[cfg(all(` attribute closing with `))] use winit::event::Event;`; allowed: the same attribute closing with `))] use std::fmt;`, a multi-line `#[derive(` attribute followed by `pub struct X;`, an attribute closing on its own line followed by `use std::fmt;` | The first source is rejected and the violation names the closing line; the allowed sources give none | Unit |
| TS-5 | Existing expectations (AC-5): the existing `ac8_*` tests and `ac8_the_module_uses_only_always_built_crates` | All pass unchanged on the current `program_status.rs` | Unit |
| TS-6 | Violation text (AC-1, AC-3, AC-4, NFR2): every violation from the TS-1 / TS-3 / TS-4 rejected sources | Each contains the offending line, `super::agent_status` and ``without the `gui` feature`` | Unit |
| TS-7 | Change scope (SPEC Success Criteria, NFR1): the integrated diff from `workflow.implement.base_commit` to the integration branch tip, excluding `feature-docs/program-status-ac8-use-line-forms/` and `test-docs/program-status-ac8-use-line-forms/` | The only changed path is `src-tauri/src/program_status/tests.rs`; no production code, `Cargo.toml`, `lib.rs` or new crate | Static (diff) |

## Code Quality Verification
- Format: none declared (workflow.yaml `format_command` is empty). Crate-wide formatting is not part of this verification.
- Static analysis: the two Build Verification commands.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements (FR1-FR6) are implemented and tested | Functional Requirements Coverage below; TS-1 to TS-6 pass |
| SC-2 | All test scenarios (TS-1 to TS-6) pass | Test Verification command |
| SC-3 | Changes are confined to `src-tauri/src/program_status/tests.rs` (NFR1) | TS-7 |
| SC-4 | Code review is completed | workflow.yaml `review` step is completed with `residual_critical_high` at 0 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-6 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3, TS-6 |
| FR4 | task0001 | TS-4, TS-6 |
| FR5 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| FR6 | task0001 | TS-5 |
| NFR1 | task0001 | TS-7 |
| NFR2 | task0001 | TS-6 |

## Manual Testing (E2E Not Possible)
- None. The change is test-only code with no user-visible behavior; the project has no E2E suite for it.

## Performance / Security Verification (if applicable)
- Performance: not applicable.
- Security: THREAT-MODEL.md verdict is `no-trust-boundary`; there is no TM-n item.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit test scenarios (TS-1 to TS-6) | 6 | 6 | 0 | 0 |
| Change scope (TS-7) | 1 | 1 | 0 | 0 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 9 | 9 | 0 | 0 |
