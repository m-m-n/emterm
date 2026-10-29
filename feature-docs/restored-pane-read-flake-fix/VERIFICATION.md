# Verification Document: restored-pane-read-flake-fix

## Overview
**Feature**: restored-pane-read-flake-fix / **SPEC.md**: `feature-docs/restored-pane-read-flake-fix/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/restored-pane-read-flake-fix/IMPLEMENTATION.md`

Target test: `mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`.
Run every command from the integration worktree root.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only feature gate, NFR3): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: not measured (no coverage tooling is configured). Every
  scenario below must pass.

### Test Scenarios from SPEC.md
TS-1 to TS-6 come from SPEC.md. TS-7 and TS-8 cover the read-error and
deadline cases of SPEC.md's Edge Cases and FR3. TS-9 covers NFR4's early
return. TS-10 covers the NFR1 and NFR3 change-set inspection.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | A scripted reader returns a strict prefix of the echo (`rest`), then the rest (`ored-write` plus carriage return and newline). Variant: the echo split into three or more chunks, including a one-byte first chunk and a marker straddling chunk boundaries | The read-until-marker helper reports the marker found. The collected bytes hold the full echo. A single-read implementation fails this scenario | Unit |
| TS-2 | A scripted reader returns a strict prefix of the marker, then end of data | The helper reports the marker missing with reason end of data. The failure text contains the debug-escaped collected bytes, the reason, and the meaning of the original message | Unit |
| TS-3 | The target test runs against a real PTY pair with the read-until-marker logic and a 5-second deadline | The target test passes. It keeps its name and unix gate | Unit (real PTY) |
| TS-4 | The target test runs alone 150 times (exact-name match) while one busy-loop thread per logical CPU occupies the machine | 0 failures of the target test (before the fix: 28 of 150) | Performance (load) |
| TS-5 | Full `--lib` run; `--no-default-features` cargo check; `git diff` of the Cargo files against the feature's base | The target test passes in the full run (other known intermittent failures do not count, SPEC A5). The check compiles. `src-tauri/Cargo.toml` and `src-tauri/Cargo.lock` are unchanged | Integration |
| TS-6 | Inspect the comment just before the after-restore write in `src-tauri/src/mux/upgrade/tests.rs` | It contains no form of "deterministic" and no single-read claim. It still states the restored pane is writable and names the target test. It describes the target test's read side as a bounded read-until-marker check. It makes no claim that the pane-level test has no background reader thread | Inspection |
| TS-7 | A scripted reader returns a strict prefix of the marker, then a read error | The helper reports the marker missing with reason read error. The failure text contains the debug-escaped collected bytes and the error's description | Unit |
| TS-8 | A scripted reader returns a prefix, then withholds data. Variant: it keeps trickling non-marker bytes. The helper is called with a short deadline | The helper reports the marker missing with reason deadline, within a generous upper bound of the deadline (the deadline is absolute). The failure text contains the debug-escaped collected bytes | Unit |
| TS-9 | A scripted reader returns the complete echo, then withholds data. The helper is called with a deadline of at least 5 seconds | The helper reports the marker found in clearly less time than the deadline | Unit |
| TS-10 | Inspect the feature diff under `src-tauri/` against the feature's base | Only `src-tauri/src/mux/session/pane/tests.rs` and `src-tauri/src/mux/upgrade/tests.rs` change. No production file changes. The target test keeps its name and unix gate. The new test helpers use no unix-only API | Inspection |

## Code Quality Verification
- Format: not configured (workflow.yaml `format_command` is empty). Formatting
  of the changed regions follows the repository's rustfmt configuration. No
  crate-wide formatting is run.
- Static analysis: none configured. The build commands above must report no
  new warnings in the changed files.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | The target test passes when the echo arrives in several chunks whose first chunk is shorter than the 14-byte marker | TS-1, TS-3 |
| AC-2 | When the marker does not arrive (deadline, EOF, or read error), the failure message contains the collected bytes | TS-2, TS-7, TS-8 |
| AC-3 | A test deterministically produces a prefix-only first read and passes only with the read-until-marker logic | TS-1 |
| AC-4 | The original reproduction (target test alone, 150 runs, busy-loop threads on every logical CPU) gives 0 failures | TS-4 |
| AC-5 | The full `--lib` suite runs with the target test passing | TS-5 |
| AC-6 | The comment in `src-tauri/src/mux/upgrade/tests.rs` no longer says the pane-level test proves the read side deterministically | TS-6 |
| AC-7 | `Cargo.toml` / `Cargo.lock` are unchanged and the `--no-default-features` check compiles | TS-5, TS-10 |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-3, TS-4, TS-5 |
| FR2 | task0001 | TS-3, TS-4, TS-8 |
| FR3 | task0001 | TS-2, TS-7, TS-8 |
| FR4 | task0001 | TS-1 |
| FR5 | task0002 | TS-6 |
| NFR1 | task0001, task0002 | TS-5, TS-10 |
| NFR2 | task0001 | TS-5 |
| NFR3 | task0001 | TS-5, TS-10 |
| NFR4 | task0001 | TS-4, TS-9 |

## Manual Testing (E2E Not Possible)
- [ ] TS-6: read the reworded comment in `src-tauri/src/mux/upgrade/tests.rs`
  against the criteria in the TS-6 row.
- [ ] TS-10: read the feature diff under `src-tauri/` against the criteria in
  the TS-10 row.

## Performance / Security Verification (if applicable)
- NFR4 / TS-9: a passing helper call returns without waiting for the
  deadline. The target test's deadline is 5 seconds.
- AC-4 / TS-4 (KPI: 0 of 150): build the `--lib` test binary once, then run
  only the target test with exact-name matching
  (`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --exact mux::session::pane::tests::from_restored_pane_can_write_and_read_through_its_adopted_master`,
  or the built test binary with the same filter) 150 times. Meanwhile, one
  busy-loop thread per logical CPU of the verifying machine runs (SPEC A4).
  The load generator is test-time tooling kept outside the repository and
  never committed. Its run time is bounded so it ends by itself after the
  150 runs, and no process has to be killed. Count failures of the target
  test. Expected: 0.
- Security: THREAT-MODEL.md verdict is `no-trust-boundary`, so there are no
  TM items.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit tests | 6 (TS-1, TS-2, TS-3, TS-7, TS-8, TS-9) | 6 | 0 | 0 |
| Integration | 1 (TS-5) | 1 | 0 | 0 |
| Performance (load) | 1 (TS-4) | 1 | 0 | 0 |
| Inspection | 2 (TS-6, TS-10) | 0 | 0 | 2 |
| Security (TM-n) | 0 | 0 | 0 | 0 |
| Total | 12 | 10 | 0 | 2 |
