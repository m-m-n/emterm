# Verification Document: Unknown-flag usage error

## Overview

**Feature**: unknown-flag-usage / **SPEC.md**: `feature-docs/unknown-flag-usage/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/unknown-flag-usage/IMPLEMENTATION.md`

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (cli): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib --no-default-features`
- Coverage target: every acceptance criterion of task0001 that is reachable
  from the library crate has at least one test. No project-wide coverage
  percentage is tracked.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Classify `["--help"]` and `["-h"]` | help-requested outcome | Unit |
| TS-2 | Classify `["--typo"]` | unrecognized-argument outcome carrying `--typo` | Unit |
| TS-3 | Classify `[]` | proceed outcome | Unit |
| TS-4 | Classify `["--viewer", "/tmp/p"]` on the gui build | proceed outcome | Unit |
| TS-5 | Classify `["--viewer", "--weird"]` on the gui build | proceed outcome — the value is consumed, not classified | Unit |
| TS-6 | Classify `["--settings"]` on the gui build | proceed outcome | Unit |
| TS-7 | Classify `["--typo", "--help"]` and `["--help", "--typo"]` | help-requested outcome in both orders | Unit |
| TS-8 | Classify `["-"]` and `["--"]` | unrecognized-argument outcome | Unit |
| TS-9 | Classify `["--a", "--b"]` | unrecognized-argument outcome carrying `--a` | Unit |
| TS-10 | Classify `["--settings"]` on the `--no-default-features` build | unrecognized-argument outcome | Unit |
| TS-11 | Existing CLI subcommand dispatch | `src-tauri/tests/cli_subcommands.rs` passes unchanged | Integration |
| TS-12 | Recognized-flag table vs `run_gui`'s accepted flags | the two sets are equal; the test fails if either changes alone | Unit |
| TS-13 | Usage text content | contains every bare-word subcommand, the `-h, --help` line, the guidance line `Run \`emterm <subcommand> --help\` for details.`, and — on the gui build — all five child-window flags | Unit |

## Code Quality Verification

- Format: not enforced by this project (`format_command` is empty in
  workflow.yaml; formatting is applied by the editor hook).
- Static analysis: covered by `cargo check` for both feature configurations.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements implemented and tested | Requirements coverage table below; TS-1..TS-13 pass |
| SC-2 | Default-feature check succeeds | Build Verification (main) |
| SC-3 | CLI-only check succeeds | Build Verification (cli) |
| SC-4 | No regression in existing CLI subcommand tests | TS-11 |
| SC-5 | Rejected invocations create no window and install no logger | Code reading of `main`'s branch arms (the classifier call precedes `logging::init()` and every windowing call) + MT-1 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-8, TS-9 |
| FR2 | task0001 | TS-1, TS-7 |
| FR3 | task0001 | TS-4, TS-5, TS-6, TS-10, TS-12 |
| FR4 | task0001 | TS-13 |
| FR5 | task0001 | SC-5, MT-1 |
| FR6 | task0001 | TS-11 |
| NFR1 | task0001 | TS-1..TS-13 all run under `cargo test --lib` |
| NFR2 | task0001 | Build Verification (main + cli), TS-10 |
| NFR3 | task0001 | TS-12 |

## E2E Testing

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test cli_subcommands`
- [ ] The existing `cli_subcommands` integration suite passes without
      regression (TS-11).

## Manual Testing (E2E Not Possible)

Process-level behavior (which stream, which exit code, whether a window
appears) cannot be observed from the in-process test harness.

- [ ] MT-1: On a release GUI build, `emterm --typo` prints an error line plus
      the usage to stderr, exits 2, and opens no window.
- [ ] MT-2: On a release GUI build, `emterm --help` prints the usage to stdout
      and exits 0, and opens no window.
- [ ] MT-3: On a release GUI build, `emterm` with no arguments still opens the
      terminal window, and opening a Markdown / image / data / HTML viewer and
      the settings window from within the terminal still works (these use the
      recognized child-window flags via self-exec).
- [ ] MT-4: On a CLI-only build, `emterm --help` prints the usage to stdout
      with exit 0 and `emterm --typo` prints the usage to stderr with exit 2.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Test scenarios | 13 | 13 | 1 (TS-11) | 0 |
| Success criteria | 5 | 4 | 0 | 1 (SC-5, jointly with MT-1) |
| Manual scenarios | 4 | 0 | 0 | 4 |
