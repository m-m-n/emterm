# Verification Document: --version Flag Classification and Usage Listing

## Overview

**Feature**: version-flag-classification /
**SPEC.md**: `feature-docs/version-flag-classification/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/version-flag-classification/IMPLEMENTATION.md`

## Build Verification

- Command (gui build): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command (gui build, unit + `--version` integration tests):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib --test cli_subcommands -- --test-threads=1`
- Command (CLI-only build, unit tests):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib -- --test-threads=1`
- Coverage target: no project-wide coverage threshold is configured. The
  requirement here is that every acceptance criterion of task0001 has a
  corresponding assertion.

### Known-flaky exclusion

The `tabs` replay tests are timing-sensitive and fail non-deterministically
independently of this feature (recorded in the task as out of scope). They run
with `--test-threads=1` to reduce flakiness; a failure confined to `tabs::tests`
does not count as a verification failure for this feature. Any other failure
does.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Classify `--version` alone on the gui build | Proceed outcome | Unit |
| TS-2 | Classify a recognized valueless flag followed by `--version` on the gui build | Proceed outcome | Unit |
| TS-3 | Classify `--version` alone on the CLI-only build | Proceed outcome | Unit |
| TS-4 | Classify `--version` followed by an unrecognized flag | Unknown outcome carrying the unrecognized flag, not `--version` | Unit |
| TS-5 | Classify `--version` together with a help flag | Help outcome | Unit |
| TS-6 | Usage text on the gui build | Contains a `--version` Options line and the subcommand-help guidance line | Unit |
| TS-7 | Usage text on the CLI-only build | Contains a `--version` Options line and the subcommand-help guidance line | Unit |
| TS-8 | Flag-table contents on both builds | Match the new expected sets exactly | Unit |
| TS-9 | The `--version` entry's dispatch target | Declares no child-window target | Unit |
| TS-10 | `--version` supplied as a value-taking flag's value | Consumed as the value, never classified | Unit |
| TS-11 | Existing `--version` behavior end to end | Crate version on stdout, empty stderr, exit 0, no log directory created; all five existing tests pass unmodified | Integration |
| TS-12 | Unrecognized flag handling regression | An unknown flag still produces the unrecognized-argument outcome | Unit |

## Code Quality Verification

- Format: no workflow-level format command is configured. Rust formatting is
  applied by the project's editor-side PostToolUse hook on the files touched.
- Static analysis: none configured beyond the compiler; the two `cargo check`
  commands above stand in for it.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | Requirements coverage table below; TS-1..TS-12 pass |
| SC-2 | Both builds compile | The two `cargo check` commands exit 0 |
| SC-3 | Existing `--version` integration tests pass unmodified | TS-11, plus `git diff` shows no change to `src-tauri/tests/cli_subcommands.rs` |
| SC-4 | `RECOGNIZED_FLAGS` remains the only flag list | Inspect the GUI dispatch loop: it derives its dispatch set from the table; no flag-name literal appears outside `arg_dispatch.rs` |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-8 |
| FR2 | task0001 | TS-3, TS-8 |
| FR3 | task0001 | TS-9, SC-4 |
| FR4 | task0001 | TS-6 |
| FR5 | task0001 | TS-7 |
| FR6 | task0001 | TS-1, TS-2, TS-3, TS-4 |
| NFR1 | task0001 | SC-4, TS-8 |
| NFR2 | task0001 | TS-11, SC-2 |
| NFR3 | task0001 | TS-8, SC-3 |

## E2E Testing

No E2E framework is configured in this project. Omitted.

## Manual Testing (E2E Not Possible)

- [ ] Run the built binary with `--help` and confirm the Options block reads
      naturally with the `--version` line aligned to its neighbours (column
      alignment is a visual judgement the tests only partially cover).
- [ ] Run the built binary with a recognized valueless flag followed by
      `--version` and confirm no `unrecognized argument` message appears.

## Performance / Security Verification

Not applicable — the change classifies command-line arguments and emits static
text.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 12 | 12 | 0 | 0 |
| Success criteria | 4 | 3 | 0 | 1 (SC-4 partly by inspection) |
| Manual checks | 2 | 0 | 0 | 2 |
