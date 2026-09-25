# Verification Document: erase-edge-repair-dead-sides

## Overview

**Feature**: erase-edge-repair-dead-sides / **SPEC.md**:
`feature-docs/erase-edge-repair-dead-sides/SPEC.md` / **IMPLEMENTATION.md**:
`feature-docs/erase-edge-repair-dead-sides/IMPLEMENTATION.md`

No tests are added (SPEC A-3). Verification consists of the existing tests
passing unchanged, the builds passing, and code / diff inspection. All
commands run from the repository root (integration worktree root).

## Build Verification

- term_core: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- src-tauri, default features: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- src-tauri, CLI only: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification

- Primary: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Regression (src-tauri consumes term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: not applicable — no tests are added; the changed paths are
  covered by the existing tests listed in TS-1 to TS-4.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | ECH both-edge repair: `test_erase_characters_spacer_start_blanks_left_base`, `test_erase_characters_base_at_range_end_blanks_right_spacer`, `test_erase_characters_cursor_on_spacer_of_overflow_base_blanks_left_base`, `test_handle_erase_characters_normal`, `test_handle_erase_characters_overflow_clamped`, `test_handle_erase_characters_dirty` | All pass with no change to the tests | Unit (existing) |
| TS-2 | EL 0 / ED 0 left-edge repair: `test_erase_in_line_to_end_spacer_at_cursor_blanks_left_base`, `test_erase_in_display_below_spacer_at_cursor_blanks_left_base`, `test_erase_in_line_to_end_base_at_cursor_no_extra_blank`, `test_erase_in_line_to_end_cursor_at_col_zero_no_left_partner`, `test_handle_erase_in_display_below`, `test_handle_erase_in_line_to_end` | All pass with no change to the tests | Unit (existing) |
| TS-3 | EL 1 / ED 1 right-edge repair: `test_erase_in_line_to_start_base_at_cursor_blanks_right_spacer`, `test_erase_in_display_above_base_at_cursor_blanks_right_spacer`, `test_erase_in_line_to_start_spacer_at_cursor_no_extra_blank`, `test_erase_in_line_to_start_cursor_at_last_col_no_right_partner`, `test_handle_erase_in_display_above`, `test_handle_erase_in_line_to_start` | All pass with no change to the tests | Unit (existing) |
| TS-4 | Full-row erase paths: `test_handle_erase_in_display_all`, `test_handle_erase_in_line_all`, `test_erase_in_line_all_wide_pair_no_partner_cleanup`, `test_erase_in_display_all_wide_pair_no_partner_cleanup`, `test_handle_erase_in_display_scrollback_returns_sentinel`, `test_handle_erase_in_display_invalid_mode` | All pass with no change to the tests | Unit (existing) |
| TS-5 | Whole-suite and build run: term_core `--lib` tests, src-tauri check with default features and with `--no-default-features` | All exit 0 | Integration |
| TS-6 | Dead-side removal (SPEC AC-1, AC-2): on the EL 0 path the right-edge width read of cell end − 1 and the partner blank at column end are absent; on the EL 1 path the left-edge width read of column 0 and the left repair branch are absent | Confirmed by reading `csi_screen.rs` | Inspection |
| TS-7 | Structure (SPEC AC-3): ED mode 0 / 1 erase the cursor row through `handle_erase_in_line` mode 0 / 1 (ED 0: cursor row first, then rows below; ED 1: rows above first, then cursor row); the capture → `clear_line_range` → repair flow is written once in `csi_screen.rs` and reached from ECH, EL 0 and EL 1 only; ECH keeps both edges and the empty-range plain-clear-only behavior | Confirmed by reading `csi_screen.rs` | Inspection |
| TS-8 | Change boundary (SPEC AC-4 code part, AC-5 diff part): `git diff` from `workflow.implement.base_commit` shows no change to `clear_line_range`, `clear_line`, EL 2, ED 2, no changed line inside any `#[cfg(test)]` module of term_core, no public signature change, and no change outside the non-test body of `crates/term_core/src/csi_screen.rs` and doc-comment text of `crates/term_core/src/terminal_cells.rs` | Confirmed by diff | Inspection |
| TS-9 | Doc comments (SPEC AC-7): the chokepoint doc comment in `csi_screen.rs` states the repaired edge per site in agreement with the code; the `blank_wide_pair_half` caller list in `terminal_cells.rs` is accurate | Confirmed by reading both files | Inspection |

### Edge Cases (covered by the scenarios above)

- EL 0 with the cursor at column 0: the start > 0 guard prevents a left repair (TS-2).
- EL 1 with the cursor at the last column: end equal to the column count is absorbed by the out-of-bounds guard (TS-3).
- ECH empty range (end ≤ start): plain range clear only (TS-7).

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Static analysis: none configured in `workflow.yaml`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | EL 0 / ED 0 cursor-row path does not run the right-edge capture or repair | TS-6 |
| AC-2 | EL 1 / ED 1 cursor-row path does not run the left-edge capture or repair branch | TS-6 |
| AC-3 | ED 0 / 1 delegate the cursor row to EL 0 / 1; capture → clear → repair written once | TS-7 |
| AC-4 | `clear_line_range`, `clear_line`, EL 2, ED 2 unchanged; full-row no-partner tests pass | TS-4, TS-8 |
| AC-5 | term_core `--lib` tests all pass; no diff in term_core `#[cfg(test)]` modules | TS-1 to TS-5, TS-8 |
| AC-6 | src-tauri check passes with default features and with `--no-default-features` | TS-5 |
| AC-7 | Chokepoint doc comment and `blank_wide_pair_half` caller list match the implementation | TS-9 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-2, TS-6 |
| FR2 | task0001 | TS-3, TS-6 |
| FR3 | task0001 | TS-1, TS-7 |
| FR4 | task0001 | TS-2, TS-3, TS-7 |
| FR5 | task0001 | TS-7 |
| FR6 | task0001 | TS-4, TS-8 |
| FR7 | task0001 | TS-9 |
| NFR1 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-5 |
| NFR2 | task0001 | TS-5, TS-8 |
| NFR3 | task0001 | TS-5, TS-8 |

## E2E Testing

None — the project has no E2E suite for term_core, and the change has no UI
or visual effect.

## Manual Testing (E2E Not Possible)

None — no UI, screen or visual change (design step skipped).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Unit / integration tests | TS-1 to TS-5 | 5 | 0 | 0 |
| Inspection (code / diff / doc) | TS-6 to TS-9 | 0 | 0 | 4 |
| Format | 1 | 1 | 0 | 0 |
