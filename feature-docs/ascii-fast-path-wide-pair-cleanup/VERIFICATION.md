# Verification Document: ascii-fast-path-wide-pair-cleanup

## Overview

**Feature**: ascii-fast-path-wide-pair-cleanup
**SPEC.md**: `feature-docs/ascii-fast-path-wide-pair-cleanup/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/ascii-fast-path-wide-pair-cleanup/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Per-task
acceptance criteria live in `tasks/task0001.md` and `tasks/task0002.md`.
All commands are run from the project root (never `cd` into a crate
directory) with an explicit target directory, per the project's build rules.

## Build Verification

| Component | Command | Expected |
|-----------|---------|----------|
| term_core | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml` | exit code 0, no errors |
| emterm | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` | exit code 0, no errors |
| emterm (CLI-only) | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` | exit code 0, no errors |

New warnings introduced by this feature (unused import, dead code, unused
variable) count as failures — the change is a few lines in one hot path and
has no reason to produce any.

## Test Verification

| Component | Command |
|-----------|---------|
| term_core | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` |
| emterm | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` |

Coverage target: not applicable — the project configures no coverage tooling
(`test/README.md`: standard `cargo test` only, no coverage harness). Coverage
of this feature is judged by the scenario table below rather than by a
percentage.

Flake note: if unrelated replay tests fail non-deterministically, re-run the
affected suite with `-- --test-threads=1` (`test/README.md`).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | A fullwidth character is written by one dispatch call; a second dispatch call carries a carriage return followed by an ASCII byte that overwrites the wide base at column 0 | Column 0 holds the ASCII character at width 1, column 1 is a blank cell at width 1, no width-0 spacer remains | Unit |
| TS2 | The same stream (fullwidth character, carriage return, ASCII byte) is delivered (i) in a single dispatch call and (ii) split so the ASCII tail begins a fast-path-eligible call | Both cores end with identical grid contents, identical per-cell widths and identical overflow-table state | Unit |
| TS3 | A fullwidth character occupies columns 0-1; the cursor is placed on column 1 and a following dispatch call overwrites that spacer with an ASCII byte | Column 1 holds the ASCII character at width 1, column 0 is blanked to width 1, no orphan base survives | Unit |
| TS4 | The cell the fast path overwrites carries an overflow-table entry | The overflow entry and its reverse-index entry are gone, matching the slow ASCII writer's result for the same overwrite | Unit |
| TS5 | A pure-ASCII stream is processed through the fast path on a grid with no wide cells | The resulting grid matches the slow path's result for the same stream; every touched cell has width 1 | Unit |
| TS6 | Boundary shapes: a width-0 cell at column 0, a width-2 base in the last column of a row, and a width-0 cell whose left neighbour is not a width-2 base | No panic and no out-of-range access; no cell outside the overwritten cell's row is touched; no legitimate (non-partner) neighbour is blanked | Unit |
| TS7 | The full existing suites are run against the integrated change | term_core `--lib`, src-tauri `--lib` and the CLI-only check all pass | Suite-level |

## Code Quality Verification

- Format: no format command is configured in workflow.yaml for these
  components. Rust formatting is enforced by the project's editor-time hook;
  do not run a crate-wide reformat as part of verification.
- Static analysis: covered by the three `cargo check` invocations above
  (warnings inspected as stated).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | All functional requirements FR1-FR7 implemented and tested | Requirements-coverage table below; every row has at least one task and either a scenario or a recorded inspection item |
| SC2 | All scenarios TS1-TS7 pass | Run both test commands; TS1-TS6 appear as named unit tests, TS7 is the suites-green gate |
| SC3 | Performance meets NFR1 and the evaluation is recorded | Inspection item M2 |
| SC4 | Robustness requirement NFR3 satisfied | TS6 |
| SC5 | The D2-repair call-site doc comment matches the post-change code | Inspection item M1 |
| SC6 | Change confined to `crates/term_core`, no public API and no dependency change | Inspection item M3 |
| SC7 | New tests follow `test/README.md` | Inspection item M4 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS3 |
| FR2 | task0001 | TS4 |
| FR3 | task0001 | TS2 |
| FR4 | task0001 | TS5 + inspection M3 (admission conditions unchanged) |
| FR5 | task0002 | Inspection M1 (no automated scenario exists — recorded coverage gap) |
| FR6 | task0001 | TS1 |
| FR7 | task0001 | TS5, TS7 |
| NFR1 | task0001 | TS5 + inspection M2 |
| NFR2 | task0001, task0002 | TS7 + inspection M3 |
| NFR3 | task0001 | TS6 |
| NFR4 | task0001 | Inspection M4 (no automated scenario exists — recorded coverage gap) |

## E2E Testing

The project has no E2E framework (`test/README.md`: no compose file, no E2E
directory). No E2E item is defined for this feature.

## Manual Testing (E2E Not Possible)

Inspection items — each is a human judgment on the integrated diff, not a
runnable command.

- [ ] **M1 (FR5)**: The wide-pair blanking primitive's doc comment enumerates
      exactly the call sites listed in IMPLEMENTATION.md D-2, names the
      PTY-dispatch ASCII fast path distinctly from the print path's ASCII
      writer, and conveys the complete set without depending on a file outside
      the crate.
- [ ] **M2 (NFR1)**: The fast path's write step carries a recorded cost
      evaluation matching the NFR1 budget stated in `tasks/task0001.md`, and
      the implemented code matches it: the added work on a width-1 overwrite is
      one read of a field in the cell record already being written plus one
      untaken branch, with no allocation, no extra pass over the input buffer
      and no non-inlinable per-byte call.
- [ ] **M3 (FR4, NFR2)**: The diff touches only `terminal_dispatch.rs`,
      `print_handler.rs` (visibility line) and `terminal_cells.rs` (comment);
      the fast path's admission conditions are unchanged; no crate manifest is
      modified and nothing publicly exported from term_core changes.
- [ ] **M4 (NFR4)**: New tests live inline next to the code under test, use
      `<subject>_<scenario>_<expected>` naming, construct their own terminal
      core, drive input through the PTY-dispatch entry point, and assert on the
      observable grid contract.
- [ ] **M5 (regression confidence)**: At least one new scenario is confirmed to
      FAIL against the pre-change fast path (checked during implementation or
      by temporarily reverting the write-step change) — proving it exercises
      the fast path rather than the already-correct slow path.

No mockup comparison item applies: the design step was skipped (no
user-visible surface, no UI, no design-token consumption).

## Performance / Security Verification

- **NFR1 (performance)**: verified analytically, not by benchmark — the budget
  and the recorded evaluation are inspection item M2. No benchmark is added
  (the project's opt-in benches are unrelated to this path).
- **NFR3 (adversarial input)**: PTY bytes are treated as attacker-influenceable.
  TS6 covers the panic / out-of-range / wrong-neighbour surface; the repair
  keys off the wide-pair relationship rather than width 0 alone so
  combining-mark residue is not mistaken for a spacer.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit tests (TS1-TS6) | 6 | 6 | 0 | 0 |
| Suite-level gate (TS7) | 1 | 1 | 0 | 0 |
| Inspection (M1-M5) | 5 | 0 | 0 | 5 |
| **Total** | **15** | **10** | **0** | **5** |
