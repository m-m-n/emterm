# Verification Document: survivor-assert-key-stability

## Overview

**Feature**: survivor-assert-key-stability
**SPEC.md**: `feature-docs/survivor-assert-key-stability/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/survivor-assert-key-stability/IMPLEMENTATION.md`

This document covers the integrated verification of the feature as a whole.
Task-level acceptance criteria live in `feature-docs/survivor-assert-key-stability/tasks/task0001.md`.

The feature enlarges one existing unit test so it observes ring slot key
stability and survivor row content survival. Because the deliverable is test
code, verification is not only "the suite is green" — it must also show the
enlarged test is *capable* of turning red for the defect it targets. That is
what TS2 exists for; a green suite alone does not verify this feature.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors and no new warnings.
- Run from the project root. Do not change directory into the crate; the
  manifest path and target directory are given explicitly so concurrent
  sessions agree on the build location.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0, every test in the crate passing, including
  `test_ring_push_blank_clears_recycled_row_overflow_entries`.
- Coverage target: not measured. The project configures no coverage tool and
  this feature introduces none (IMPLEMENTATION.md decision D4). Adequacy is
  argued through the requirement-to-scenario mapping below and through the
  mutation red-check, not through a coverage percentage.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Baseline green: run the crate unit-test command with no mutation in place | Every test passes, including the blank-push row-scope test | Unit (automated) |
| TS2 | Red under fill-index mutation: corrupt only the Step 3 fill target's slice index, leaving the overflow-clear side correct, then run the same command | The survivor row is blanked, so the content observation fails and the blank-push row-scope test is red | Unit, mutation-injected (developer-executed) |
| TS3 | Green after reverting the mutation: restore the production file and re-run the same command | Every test passes again and no mutation remains in the working tree | Unit, mutation-injected (developer-executed) |
| TS4 | Formatting check: run the crate-scoped format check | Exits reporting no diff | Static (automated) |

TS2 is the decisive scenario. It must be observed to fail *at the content
observation*: a failure reported by any other assertion means the mutation was
not the intended fill-target-only one, and the check must be redone.

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Static analysis: no project-configured linter beyond the compiler's own
  diagnostics for this crate; the build verification command above is the static
  analysis gate.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The post-line-feed survival block asserts that viewport row 0's absolute ring slot key equals the survivor key captured before the scroll | Read the survival block in the test file and confirm the equality observation is present |
| AC2 | The survival block observes the survivor row's leading cell through the cell grapheme accessor and asserts it matches the grapheme the fixture printed | Read the survival block and confirm the content observation is present with the column-first argument order |
| AC3 | The pre-existing removal post-assertions and survivor-presence assertions remain | Inspect the committed diff: it must consist of insertions into the survival block only, with no deletion or rewrite of an existing assertion |
| AC4 | The Step 3 slice-index-only mutation makes the test fail, and reverting it makes the test pass | TS2 followed by TS3 |
| AC5 | The crate unit-test command is green | TS1 |
| AC6 | The crate-scoped format check is clean | TS4 |
| AC7 | The permanent diff is confined to the test file | Inspect the committed diff and confirm it touches only `crates/term_core/src/ring_buffer/tests.rs`; confirm the production ring-buffer file carries no change |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (the enlarged test passes with the key stability observation in place); AC1 source inspection |
| FR2 | task0001 | TS1 and TS2 (the content observation passes on a healthy tree and fails under the mutation); AC2 source inspection |
| FR3 | task0001 | TS1; AC3 diff inspection confirming the change is additive |
| FR4 | task0001 | TS2 and TS3 |
| FR5 | task0001 | TS3 and AC7 diff inspection |
| NFR1 | task0001 | TS1; source inspection that the addition stays inside the existing inline test module with no new crate or dependency |
| NFR2 | task0001 | TS4 |
| NFR3 | task0001 | TS1; source inspection confirming the addition is assertion lines only, with no process spawn, I/O or sleep |
| NFR4 | task0001 | TS1 run under the suite's default parallel execution; source inspection confirming the additions touch only the test's own terminal core |

## Manual Testing (E2E Not Possible)

No E2E framework applies — the feature is confined to a unit test inside one
crate, and workflow.yaml records no E2E command for this component.

TS2 and TS3 cannot be automated in the suite, because they require deliberately
breaking production code and then restoring it. They are executed by hand and
their outcome recorded.

- [ ] TS2: inject the Step 3 fill-target-only mutation, run the crate unit-test
      command, and confirm the blank-push row-scope test is red **and** that the
      reported failure comes from the survivor-content observation.
- [ ] TS3: restore the production ring-buffer file, re-run the crate unit-test
      command, and confirm the suite is green.
- [ ] Working-tree cleanliness: confirm the production ring-buffer file shows no
      diff after TS3, so no part of the mutation survived.
- [ ] Diff scope: confirm the committed change touches only the test file
      (AC7) and is purely additive within the survival block (AC3).

## Performance / Security Verification

- NFR3 (no runtime impact): the addition is assertion lines only. Confirm by
  inspection that no process is spawned and no I/O or sleep is performed; no
  timing threshold is set, because a few assertions cannot move the suite's
  runtime measurably.
- Security: not applicable. The change adds assertions to an in-crate unit test.
  It introduces no input surface, no data handling, and no production-code
  behaviour change.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit tests | 3 (TS1, TS2, TS3) | 1 (TS1) | 0 | 2 (TS2, TS3) |
| Code quality | 1 (TS4) | 1 | 0 | 0 |
| Diff / scope inspection | 2 (AC3, AC7) | 0 | 0 | 2 |
| Performance | 1 (NFR3) | 0 | 0 | 1 |
| **Total** | **8** | **3** | **0** | **5** |
