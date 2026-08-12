# Verification Document: wide-pair-blank-primitive-unification

## Overview

**Feature**: wide-pair-blank-primitive-unification /
**SPEC.md**: `feature-docs/wide-pair-blank-primitive-unification/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/wide-pair-blank-primitive-unification/IMPLEMENTATION.md`

This documents the integrated verification run by the verify phase. Task-level
acceptance criteria live in `tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors (the workspace build consuming term_core
  still compiles — NFR3's crate-boundary guarantee in practice)

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: all tests pass; every pre-existing test passes with its assertions
  untouched (NFR2)
- Coverage target: no numeric target for this refactor. The binding criteria
  are (a) the existing suite green with zero assertion edits, and (b) direct
  unit tests for the unified primitive (TS-2, TS-3) present and green.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Full existing term_core suite after unification, including the wide-pair cleanup tests in `csi_edit.rs`, `csi_screen.rs`, and `print_handler/tests.rs` | All pass with zero assertion changes | Unit/Integration |
| TS-2 | Unified primitive applied to a width-0 or width-2 wide-pair half | Cell becomes a width-1 space; fg/bg/flags/hyperlink preserved; overflow-table entry and reverse-index entry removed; row marked dirty | Unit |
| TS-3 | Unified primitive applied to a width-1 cell and to an out-of-bounds column | Strict no-op (self-guard): no mutation, no panic | Unit |
| TS-4 | Full-row erases (EL 2, ED 2) over rows containing wide pairs | Plain BCE fill; no attribute-preserving blank; no partner behavior | Unit |
| TS-5 | ASCII fast-path performance spot-check via the opt-in bench `snapshot_replay_bench_2mib_seq` | Bench remains available and runnable as a manual NFR1 spot-check; no automated perf assertion is added | Manual |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Static analysis: none configured for this feature (no additional lint
  command in workflow.yaml project.components)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | Exactly one partner-blanking primitive exists, in the grid-cell mutation base layer (`terminal_cells.rs`), with the width-0/2 self-guard inside it | Code review: primitive defined in `terminal_cells.rs`; no other partner-blanking implementation in the crate; TS-2, TS-3 |
| AC2 | `print_handler.rs` no longer defines `blank_wide_pair_partner`; print and CSI edit/erase paths call the same primitive | Code review: the name is absent from the crate; both helper functions call the primitive; TS-1 |
| AC3 | `csi_edit.rs` no longer defines `blank_wide_pair_split` as an independent implementation | Code review: the name is absent from the crate (fully removed, per IMPLEMENTATION.md decision 3); TS-1 |
| AC4 | The five range-erase call sites share one edge-repair function; full-row paths (`clear_line`, EL 2, ED 2) are unaffected | Code review: single chokepoint, five single-call sites, no inline copies; TS-1, TS-4 |
| AC5 | The `old_width != 1` gate remains at the print call sites; no new memory access or branch on the width-1 ASCII fast path | Diff inspection of `handle_print_ascii`, `write_grapheme_to_grid`, `relocate_widened_base_via_wrap`; optionally TS-5 |
| AC6 | The full term_core test suite and workspace tests pass | Test command + build command above |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-1 + AC2/AC3 code review |
| FR4 | task0001 | TS-1 + AC4 code review |
| FR5 | task0001 | TS-4 |
| NFR1 | task0001 | TS-5 (manual) + AC5 diff inspection |
| NFR2 | task0001 | TS-1 (zero assertion edits confirmed via the diff) |
| NFR3 | task0001 | TS-1 + code review: `lib.rs` untouched, new items `pub(crate)` or narrower |

## E2E Testing

Not applicable — the project has no E2E infrastructure, and this crate-internal
refactor has no user-visible surface.

## Manual Testing (E2E Not Possible)

- [ ] TS-5: run the opt-in bench `snapshot_replay_bench_2mib_seq` before and
      after the change as an NFR1 spot-check (optional; no numeric threshold —
      comparable results are the expectation).
- [ ] AC5 inspection: read the diff of the three print-path call sites and
      confirm the width-1 fast path gained no memory access and no branch.

(The design step was skipped, so there is no mockup visual-comparison item.)

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Tests (TS-1..TS-4) | 4 | 4 | 0 | 0 |
| Format | 1 | 1 | 0 | 0 |
| Performance (TS-5) | 1 | 0 | 0 | 1 |
| Code review criteria (AC1–AC5, NFR3) | 6 | 0 | 0 | 6 |
