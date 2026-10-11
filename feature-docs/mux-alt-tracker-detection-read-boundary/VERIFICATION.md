# Verification Document: mux-alt-tracker-detection-read-boundary

## Overview

**Feature**: mux-alt-tracker-detection-read-boundary / **SPEC.md**: `feature-docs/mux-alt-tracker-detection-read-boundary/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-alt-tracker-detection-read-boundary/IMPLEMENTATION.md` / **THREAT-MODEL.md**: `feature-docs/mux-alt-tracker-detection-read-boundary/THREAT-MODEL.md`

Run every command from the project root, with `--manifest-path` and the
quick-check target directory (`.claude/rules/core-build-location.md`).

## Build Verification

- Command (default features): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only build, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0 and no errors for both commands.

## Test Verification

- Command (src-tauri): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Command (test listing for the renamed test, FR6): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`
- Expected: every test passes.
- Coverage target: no coverage tool is configured for these components. The
  scenarios below set the coverage that verification checks.

### Test Scenarios from SPEC.md

| ID | SPEC scenario | Scenario | Expected Result | Test Type |
|----|---------------|----------|-----------------|-----------|
| TS-1 | TS1 | Tracker unit tests in `altscreen_split_read.rs`: two compounds in one read after M1 (each l-form then each h-form) and after M2 (each h-form then each l-form), three alternating compounds, and reproduction 1 with its reverse (task0001 AC-1). | After M1 the read has no span and a later mark is not live. After M2 the read is the whole read and a later mark is live. Each such read counts one adoption. | Unit |
| TS-2 | TS1 | Tracker unit tests of a detection read that ends in a carried item: `ESC (` (reproduction 2), a lone `ESC`, `ESC [?10`, `ESC [?1;10` and `ESC ( ESC [?10` (task0001 AC-2). | The carried item is resolved in the next read: the designator slot is honored, `ESC` then `c` is a RIS, the partial completes its switch, the CSI parse is detected in a mismatched state. Spans and adoption counts are those the task plan lists. | Unit |
| TS-3 | TS1 | The TS-1 and TS-2 streams in every delivery: one read, every single cut, every pair of cuts and one byte per read (task0001 AC-3). | Every read's spans and every delivery's adoption count equal the per-delivery oracle, which takes two or more screen override points. | Unit |
| TS-4 | TS2 | The renamed test `altmismatch_ac4_a_detection_read_applies_later_switches_and_keeps_the_later_carry` with its four cases, and the predecessor's test-docs record (task0001 AC-4). | The test passes with the new expectations (partial length 5 and span 3..11 in the third case, partial length 1 in the fourth). Its full name appears as a test in the `--list` output. The record's AC-4 names it, carries the supersede note, and keeps `red_reason` unchanged. | Unit |
| TS-5 | TS3 | SC-1z mux rows K58-K64 in `daemon/tests.rs`. Each row runs through `drive_reads` from the S-W seed in every delivery the row lists (task0001 AC-5). | Every run ends in the row's final OSC 7501 table and OSC 777 state and, where given, its table-change count. | Integration |
| TS-6 | TS4 | SC-1z plain-tab rows K58-K64 in `tabs/tests/output_pipeline.rs`, each processed in one `process_combined` call on a plain tab seeded with S-W (task0002 AC-1, AC-2). | Every row ends in the outcome SC-1z lists, which is the same outcome TS-5 asserts for the mux side. | Integration |
| TS-7 | — | Robustness after a detection read: after M1 and after M2, a compound followed by each carried-item tail, then each of the 256 byte values in its own read, then A BEL (task0001 AC-6). | No panic. The spans are ascending, non-overlapping and within the read. The carried partial is at most 7 bytes. The existing robustness tests pass unchanged. | Unit |
| TS-8 | TS5 | Regression: the src-tauri and term_core `--lib` suites in full and the `--no-default-features` check. Every existing test except the renamed one runs with no change to its code (task0001 AC-7, task0002 AC-3). | All pass. | Regression |
| TS-9 | — | Diff inspection against the integration branch's base, done by the verify phase (task0001 AC-4, AC-7; task0002 AC-3). | The doc comments of the tracker, of the detection branch and of the scan describe FR1 and FR2. The predecessor's SPEC.md and REQUIREMENTS.md are unchanged. The items FR8 lists are unchanged. No log call and no platform gate are added. No file under `crates/term_core/` changes. Among existing test functions, only the renamed one changes. | Inspection |

## Code Quality Verification

- Format: not configured (`format_command` is empty in workflow.yaml
  `project.components`).
- Static analysis: not configured.
- Diff inspection: TS-9.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1-FR8 are all implemented and tested | Functional Requirements Coverage below: every requirement maps to at least one task and one scenario |
| SC-2 | AC1-AC5 are met | Acceptance Criteria Coverage below |
| SC-3 | TS1-TS5 all pass | TS-1 to TS-9 pass |
| SC-4 | Every existing test except the FR6 one passes unchanged | TS-8, plus the TS-9 check that only the renamed test function changes |
| SC-5 | `cargo check --no-default-features` succeeds | Build Verification, second command |

### Acceptance Criteria Coverage

| SPEC AC | Verification |
|---------|--------------|
| AC1 | TS-1 (reproduction 1 and its reverse); TS-5 and TS-6 rows K58 and K60 |
| AC2 | TS-2 (reproduction 2, `ESC [?10` then `47h`); TS-5 and TS-6 rows K62 (cut at 20) and K64 (cut at 23) |
| AC3 | TS-5 against TS-6, rows K58-K64 |
| AC4 | TS-4 |
| AC5 | TS-8 and Build Verification |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3, TS-5 (K58-K61), TS-6 |
| FR2 | task0001 | TS-2, TS-3, TS-5 (K62-K64), TS-6 |
| FR3 | task0001 | TS-1, TS-2, TS-3 |
| FR4 | task0001, task0002 | TS-3, TS-5, TS-6 |
| FR5 | task0001, task0002 | TS-1, TS-2, TS-3, TS-5, TS-6 |
| FR6 | task0001 | TS-4 |
| FR7 | task0001 | TS-9 |
| FR8 | task0001, task0002 | TS-8, TS-9 |
| NFR1 | task0001 | TS-7 |
| NFR2 | task0001 | TS-8 and Build Verification (`--no-default-features`) |
| NFR3 | task0001 | TS-9 (no log call) |
| NFR4 | task0001 | TS-9 (no platform gate) |
| NFR5 | task0001, task0002 | TS-8 (term_core suite), TS-9 (no change under `crates/term_core/`) |

## E2E Testing

The project has no E2E suite for this path (SPEC: existing E2E tests: none).
No E2E scenario applies.

## Manual Testing (E2E Not Possible)

None required. Every criterion is covered by the automated scenarios and by
the TS-9 diff inspection that the verify phase performs.

## Performance / Security Verification

- NFR1 (carried state O(1)): the tracker still carries one item at a time:
  a partial switch of at most 7 bytes, the designator wait, or the CSI-parse
  fields. TS-7 checks the partial bound. In addition, inspect the carried
  state for any field that grows with input.
- TM-1: After the first detection in a read, every later compound in that
  read is detected, and the carried item recorded after the last detected
  compound reaches the next read, so no alternate-screen mark counts and no
  main-screen mark is dropped. Checked by TS-1, TS-2 and TS-3, and by TS-5
  against TS-6 (the same SC-1z outcome on both paths) (task0001 AC-1, AC-2,
  AC-3, AC-5, task0002 AC-1).
- TM-2: The carried state keeps its kinds and fixed size, every index stays
  inside the read, and no input panics after a detection read. Checked by
  TS-7 (task0001 AC-6). In addition, inspect the resume paths for unchecked
  indexing and arithmetic.
- TM-3: The scrollback write path and the feed scanner are unchanged.
  Checked by TS-8 (including
  `altsplit_ac4_k16_reads_leave_the_ring_and_the_delivered_chunks_as_they_were`
  and the `round3_*` / `round4_*` write-path tests) and by the TS-9 diff
  inspection (task0001 AC-7).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 9 (TS-1 to TS-9) | 8 | 0 | 1 (TS-9 diff inspection) |
| Success criteria | 5 | 5 | 0 | 0 |
| Performance / Security | 4 (NFR1, TM-1, TM-2, TM-3) | 4 | 0 | 0 (NFR1 and TM-2 add an inspection beside their automated check) |
