# Verification Document: mux-alt-tracker-ris-compound-decset

## Overview

**Feature**: mux-alt-tracker-ris-compound-decset / **SPEC.md**: `feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-alt-tracker-ris-compound-decset/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-alt-tracker-ris-compound-decset/THREAT-MODEL.md`

Run every command from the project root, with `--manifest-path` and the
quick-check target directory (`.claude/rules/core-build-location.md`).

## Build Verification

- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 and no errors for both commands.

## Test Verification

- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: every test passes.
- Coverage target: no coverage tool is configured for these components. The
  scenarios below set the coverage that verification checks.

### Test Scenarios from SPEC.md

| ID | SPEC scenario | Scenario | Expected Result | Test Type |
|----|---------------|----------|-----------------|-----------|
| TS-1 | TS1 | Tracker unit tests in `altscreen_split_read.rs` with RIS and with an `ESC c` in a designator slot. Starting states: main screen, an alternate screen where both rules agree, and both mismatch directions. Deliveries: one read, every single cut, every pair of cuts, one byte per read (task0001 AC-1, AC-2, AC-3). | Each read's live spans equal the client-rule oracle, and no adoption occurs. `ESC [ \| c` is not a RIS. `ESC [?10 \| ESC c` and `ESC [?1;10 \| ESC c` are a RIS. An `ESC c` in a designator slot leaves the alternate-screen mark not live. | Unit |
| TS-2 | TS1 | Tracker unit tests with the compound forms C1h, C1l, Crh, C7h, C7l, C0h and Ceh in both mismatch directions and in a matched state, in one read and in every delivery. Also: the designator-slot compounds, and the parse-discard inputs (task0001 AC-4, AC-5). | In a mismatched state, the read that holds the final byte is the whole read for `l` and empty for `h`, with exactly one adoption, and later marks are live exactly after `l`. The matched-state and designator-slot behavior is unchanged. Discarded parses give no detection. | Unit |
| TS-3 | TS2 | Robustness tests (task0001 AC-6). Inputs: every CSI prefix followed by all 256 byte values; over-long and wrap-around parameter values; more than 1000 parameters; a 1 MiB stream in 64 KiB and 1-byte reads; each in a matched state and in both mismatch directions. | No panic. The spans are ascending, non-overlapping and within the read. The carried partial is at most 7 bytes. | Unit |
| TS-4 | TS3 | Pins of the real shadow parser (vt100) (task0001 AC-3, AC-4). | It returns to the main screen after RIS, takes `ESC ( ESC c` as a RIS, follows `ESC [?1;1049h` and ignores `ESC [?1;1047h`. | Unit |
| TS-5 | TS4 | SC-1y mux rows K38-K57 in `daemon/tests.rs`. Each row runs through `drive_reads` from the S-W seed in every delivery the row lists (task0001 AC-7). | Every run ends in the row's final OSC 7501 table and OSC 777 state and, where given, its table-change count. | Integration |
| TS-6 | TS5 | SC-1y plain-tab rows K38-K57 in `tabs/tests/output_pipeline.rs`, each processed in one `process_combined` call on a plain tab seeded with S-W (task0002 AC-1, AC-2). | Every row ends in the same outcome that SC-1y lists. | Integration |
| TS-7 | TS6 | Regression: the src-tauri and term_core `--lib` suites in full and the `--no-default-features` check. Every existing test runs with no change to its code (task0001 AC-8, task0002 AC-3). | All pass. `altsplit_ac1_random_streams_and_cuts_match_the_oracle` still compares more than 2000 streams. | Regression |

## Code Quality Verification

- Format: not configured (`format_command` is empty in workflow.yaml
  `project.components`).
- Static analysis: not configured.
- Diff inspection against the integration branch's base, done by the
  verify phase:
  - No file under `crates/term_core/` changes (NFR5).
  - No log call is added (NFR3).
  - No platform-gated code is added (NFR4).
  - The main-buffer extraction, the switch-pattern list, `to_write` and
    `cuts`, `ScrollbackWriteFilter`, `AgentStatusFeedScanner` and
    `pty_reader_loop` are unchanged (FR8).
  - No existing test function changes (FR8).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR8 are all implemented and tested | Functional Requirements Coverage below: every requirement maps to at least one task and one scenario |
| SC-2 | AC1-AC7 are met | Acceptance Criteria Coverage below |
| SC-3 | TS1-TS6 all pass | TS-1 to TS-7 pass |
| SC-4 | Existing tests pass unchanged | TS-7, plus the "no existing test function changes" diff inspection |
| SC-5 | `cargo check --no-default-features` succeeds | Build Verification, second command |

### Acceptance Criteria Coverage

| SPEC AC | Verification |
|---------|--------------|
| AC1 | TS-5 and TS-6 rows K38 and K39 in every delivery; TS-1 |
| AC2 | TS-5 and TS-6 row K49 (cut at 10 is the reproduction); TS-2 |
| AC3 | TS-5 and TS-6 rows K38-K40 (cut between `ESC` and `c`) and K41 (`ESC [ \| c`); TS-1 |
| AC4 | TS-5 and TS-6 rows K42 and K43; TS-1 (no adoption) |
| AC5 | TS-5 and TS-6 rows K44-K53 (cuts inside the compound and one byte per read); TS-2 |
| AC6 | TS-3 |
| AC7 | TS-7 and Build Verification |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-4, TS-5 (K38-K40) |
| FR2 | task0001 | TS-1, TS-5 (K38-K41, K56, K57) |
| FR3 | task0001 | TS-1, TS-4, TS-5 (K42, K43) |
| FR4 | task0001 | TS-2, TS-4, TS-5 (K44-K55) |
| FR5 | task0001 | TS-2, TS-5 (cuts inside the compounds of K44-K55) |
| FR6 | task0001, task0002 | TS-5, TS-6 |
| FR7 | task0001, task0002 | TS-1, TS-2, TS-5, TS-6 |
| FR8 | task0001, task0002 | TS-7 and the diff inspection |
| NFR1 | task0001 | TS-3 |
| NFR2 | task0001 | TS-7 and Build Verification (`--no-default-features`) |
| NFR3 | task0001 | TS-7 and the diff inspection (no log call) |
| NFR4 | task0001 | TS-7 and the diff inspection (no platform gate) |
| NFR5 | task0001, task0002 | TS-7 and the diff inspection (no change under `crates/term_core/`) |

## E2E Testing

The project has no E2E suite for this path (SPEC: existing E2E tests: none).
No E2E scenario applies.

## Manual Testing (E2E Not Possible)

None required. Every criterion is covered by the automated scenarios above.

## Performance / Security Verification

- NFR1 (carried state O(1)): the tracker carries only a partial switch of at
  most 7 bytes, the designator wait and three CSI-parse fields. TS-3 checks
  the partial bound. Inspect the carried state for any growable field.
- TM-1: The parameter value saturates and never reads as 47, 1047 or 1049.
  The carried state has a fixed size, and no input panics. Checked by TS-3
  (task0001 AC-6) and by inspecting the parse arithmetic for unchecked
  overflow.
- TM-2: The tracker follows term_core's screen through RIS, through an
  `ESC c` in a designator slot and through compounds in a mismatched state,
  so no alternate-screen mark counts and no main-screen mark is dropped.
  Checked by TS-1, TS-2 and TS-4, and by TS-5 against TS-6 (same SC-1y
  outcome on both paths) (task0001 AC-1 to AC-5 and AC-7, task0002 AC-1).
- TM-3: The scrollback write path is unchanged. Checked by TS-7 (including
  `altsplit_ac4_k16_reads_leave_the_ring_and_the_delivered_chunks_as_they_were`
  and the `round3_*` / `round4_*` write-path tests) and by the FR8 diff
  inspection (task0001 AC-8).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 7 (TS-1 to TS-7) | 7 | 0 | 0 |
| Code quality (diff inspection by the verify phase) | 5 | 5 | 0 | 0 |
| Success criteria | 5 | 5 | 0 | 0 |
| Performance / Security | 4 (NFR1, TM-1, TM-2, TM-3) | 4 | 0 | 0 |
