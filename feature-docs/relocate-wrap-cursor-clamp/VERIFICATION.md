# Verification Document: relocate-wrap-cursor-clamp

## Overview

**Feature**: relocate-wrap-cursor-clamp
**SPEC.md**: `feature-docs/relocate-wrap-cursor-clamp/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/relocate-wrap-cursor-clamp/IMPLEMENTATION.md`

This document defines the INTEGRATED verification of the feature. Task-level acceptance criteria
live in `feature-docs/relocate-wrap-cursor-clamp/tasks/task0001.md`.

## Build Verification

Primary gate — term_core (the only crate this feature changes):

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings introduced by the change.

Regression checks (secondary):

- Main crate: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  — expected exit code 0. Confirms the term_core change does not break its only in-repo consumer.
- CLI-only feature gate:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  — expected exit code 0. Note: term_core sits behind the `gui` feature, so this build does not
  compile the changed crate; it is run as a cheap guard that the feature gates still hold, not as
  coverage of the change.

## Test Verification

Primary gate — term_core unit suite:

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0; TS1-TS5 below all pass; no previously passing test regresses.

Regression check — main crate library suite:

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Expected: exit code 0. This suite needs font and web-bundle provisioning in a fresh worktree, so
  it is a secondary regression check; a provisioning failure is reported as an environment issue,
  not as a feature failure. The term_core suite above remains the authoritative gate.

Coverage target: no numeric coverage threshold is defined for this feature. Coverage is judged by
TS1-TS5 all passing, which is the full set of behaviours the requirements name.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | 1-column grid: extend the existing relocation test with cursor-contract assertions | Cursor row 1, cursor column 0, wrap-pending raised, cell at column 0 of row 1 holds the digit-five-plus-VS16 grapheme. Red before the fix (cursor column stays 2) | Unit |
| TS2 | 2-column grid: new regression test printing a letter, the digit five, then VS16 | Cursor row 1, cursor column 1, wrap-pending raised, cell at column 0 of row 1 holds the grapheme with width 2, cell at column 1 of row 1 is a width-0 spacer. Red before the fix | Unit |
| TS3 | 2-column grid: one further character printed after TS2's state | The character is observable at column 0 of the row below the relocated row. Red before the fix (the character disappears) | Unit |
| TS4 | 1-column grid: one further character printed after TS1's state | The character is observable at column 0 of the row below the relocated row. Red before the fix (the character disappears) | Unit |
| TS5 | 5-column grid: the existing last-column widening test, unmodified | Cursor row 1, cursor column 2, wrap-pending lowered, the following character at column 2 of row 1. Green before and after the fix | Unit |
| TS6 | term_core component commands | build / test / format commands each succeed (format per the caveat below) | Command |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
  - **Known caveat (NFR4)**: this check has pre-existing drift in
    `crates/term_core/src/print_handler/tests.rs` that is unrelated to this feature. The check is
    judged on the lines this feature added or changed only; pre-existing drift elsewhere in the file
    is not a verification failure and must not be "fixed" by reformatting the file wholesale.
- Main-crate format (regression, same caveat handling):
  `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: no separate lint command is defined for this project's components; compiler
  warnings from the build commands above serve that role.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | 1-column grid after the digit five plus VS16: cursor row 1, cursor column 0, wrap-pending raised, relocated cell holds the digit-five-plus-VS16 grapheme | TS1 |
| AC2 | 2-column grid after a letter, the digit five and VS16: cursor row 1, cursor column 1, wrap-pending raised, relocated cell width 2, spacer width 0 | TS2 |
| AC3 | One more character printed right after AC1 / AC2 is observable on the grid | TS3, TS4 |
| AC4 | 5-column grid results are unchanged and the existing test stays green unmodified | TS5 |
| AC5 | The implemented clamp has the same branch structure as the non-final-column widening path, including the nested auto-wrap guard | Manual code inspection (see Manual Testing) |
| AC6 | term_core build / test / format commands succeed | TS6 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2 (observable outcome) + manual code-shape inspection for the branch structure |
| FR2 | task0001 | TS1, TS2 |
| FR3 | task0001 | TS5 |
| FR4 | task0001 | TS5 + manual diff-scope inspection |
| FR5 | task0001 | TS1 |
| FR6 | task0001 | TS2 |
| FR7 | task0001 | TS1, TS3, TS4 |
| NFR1 | task0001 | TS6 + manual diff-scope inspection (only the two in-scope files are modified) |
| NFR2 | task0001 | TS1 (the 1-column degraded cell state is exercised and left unchanged) + manual diff-scope inspection |
| NFR3 | task0001 | TS5 |
| NFR4 | task0001 | TS6 (format check, per the caveat above) |
| NFR5 | — | Satisfied by construction: this document defines no E2E item and the feature introduces no E2E infrastructure. No task implements it and no test verifies it (recorded as an open item in IMPLEMENTATION.md) |

## E2E Testing

Not applicable. The project has no E2E infrastructure — every component's `e2e_test_command` in
`workflow.yaml` is empty — and this feature introduces none (NFR5).

## Manual Testing (E2E Not Possible)

- [ ] **Clamp shape comparison (AC5, FR1)**: read the implemented clamp at the tail of
      `relocate_widened_base_via_wrap` side by side with the non-final-column widening path in
      `crates/term_core/src/print_handler.rs` and confirm the branch structure matches, with the
      inner auto-wrap mode guard present rather than elided.
- [ ] **Diff-scope inspection (FR4, NFR1, NFR2)**: confirm the production diff contains nothing
      beyond the cursor update — content transfer, overflow-table synchronisation, the
      wrap-continuation flag, dirty marking and the last-write bookkeeping unchanged — and that only
      `crates/term_core/src/print_handler.rs` and `crates/term_core/src/print_handler/tests.rs` are
      modified.
- [ ] **Red-before/green-after evidence (TS1-TS4)**: confirm the implementation report records that
      each new or extended assertion was red before the production change and green after.

No mockup comparison item applies: the design step was skipped and this feature produces no visual
artifact.

## Performance / Security Verification

Not applicable. The feature adds no authentication, authorization, external-input surface, or
persisted data, and defines no performance goal. The change is a bounds clamp on an existing
in-memory cursor update in the print path.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios (TS1-TS5) | 5 | 5 | 0 | 0 |
| Component commands (TS6) | 3 | 3 | 0 | 0 |
| Code quality / format | 2 | 2 | 0 | 0 |
| Manual inspection | 3 | 0 | 0 | 3 |
| **Total** | **16** | **13** | **0** | **3** |
