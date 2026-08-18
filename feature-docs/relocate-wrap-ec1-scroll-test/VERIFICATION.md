# Verification Document: relocate-wrap-ec1-scroll-test

## Overview

**Feature**: relocate-wrap-ec1-scroll-test /
**SPEC.md**: `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/relocate-wrap-ec1-scroll-test/IMPLEMENTATION.md`

This document covers the integrated verification of the whole feature.
Task-level criteria live in `feature-docs/relocate-wrap-ec1-scroll-test/tasks/task0001.md`.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors, no new warnings attributable to this diff.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0. The set of passing tests equals the pre-feature set
  plus exactly one added test
  (`ring_buffer::tests::test_ring_push_blank_clears_recycled_row_overflow_entries`),
  with one test appearing under its new name
  (`print_handler::tests::test_relocate_widened_base_via_wrap_scrolls_without_panic`)
  instead of its old one. No test is removed.
- Coverage target: not applicable — this project defines no coverage
  threshold. Coverage for this feature is judged per acceptance criterion via
  the test-docs records (TS-6), not by a percentage.
- The `src-tauri` component's test command is **not required** for this
  feature: no file in this feature's scope belongs to that component, and the
  production code it links is byte-identical (FR8).

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `print_handler::tests::test_relocate_widened_base_via_wrap_scrolls_without_panic` — 5x2 terminal, no scrollback; the relocation's own line feed scrolls the viewport | No panic; cursor pinned to the last row; the relocated base and spacer land on the resolved row (content, width 2, width 0). No overflow assertions remain. Green before and after — a robustness check, not a defect pin | Unit |
| TS-2 | `ring_buffer::tests::test_ring_push_blank_clears_recycled_row_overflow_entries` — 5x2 terminal, scrollback capacity 0; two overflow-bound width-1 cells on viewport row 0, then a plain line feed from the last row | Pre-assertions confirm both overflow entries and the reverse-index row key exist; after the scroll the recycled slot carries neither. Red-confirmed by removing both of `ring_push_blank`'s clearing sites together | Unit |
| TS-3 | `print_handler::tests::test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row` and `print_handler::tests::test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`, both unchanged | Both green and byte-identical; the deletion branches at `print_handler.rs:493` / `518` stay pinned on the no-scroll path exactly as before this feature | Unit (regression) |
| TS-4 | Full `term_core` lib suite plus the format check | Both green / clean; zero runtime behavior change | Unit (suite) |
| TS-5 | Documentary review of SPEC.md's unreachability statement | The three-part mechanism and its file:line evidence are present and accurate, and the `shift_rows_up` out-of-scope note is present | Manual (review) |
| TS-6 | Documentary review of both test-docs records | This feature's own record exists and maps every acceptance criterion; the `relocate-wrap-overflow-cleanup` AC-6 entry names the renamed test and describes the vacuity correction; no other entry in that file is altered | Manual (review) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Static analysis: the build command above (`cargo check`) is the project's
  configured static check for this component; no separate lint command is
  defined in `workflow.yaml`.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | EC1 test renamed; its comment claims only no-panic / placement | TS-1 present under the new name in the test-command output; review the leading comment |
| AC-2 | The vacuous assertions and the unused absolute-row binding are gone; the placement assertions remain | Read the test body; confirm no assertion is independent of the code under test; TS-1 green |
| AC-3 | `test_ring_push_blank_clears_recycled_row_overflow_entries` exists and passes on unmodified code | TS-2 green in the test-command output |
| AC-4 | Red confirmed by removing both clearing sites; the failure message is recorded with red confirmed, together with the single-site redundancy note | Read this feature's tests.yaml AC-4 entry: verbatim failure message, `red_confirmed: true`, the redundancy note and the two-tests-red note |
| AC-5 | TS-3's two tests unchanged and green; no non-`#[cfg(test)]` source line changed | TS-3 green; inspect the integrated diff — every changed Rust line lies inside a `#[cfg(test)]` module |
| AC-6 | SPEC's unreachability statement is present with mechanism and file:line evidence, plus the `shift_rows_up` out-of-scope note | TS-5 (documentary review) |
| AC-7 | This feature's record exists and maps every criterion; the `relocate-wrap-overflow-cleanup` AC-6 entry is corrected and no other entry altered | TS-6 (documentary review) plus a diff of that file showing the AC-6 entry as the only changed block |
| AC-8 | The `term_core` test command is green and its format command is clean | TS-4 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1 — the test runs under the new name; the `_or_stale_entries` suffix is gone |
| FR2 | task0001 | TS-1 — review of the rewritten leading comment against IMPLEMENTATION.md D2 |
| FR3 | task0001 | TS-1 — both absence-of-entry assertions and the unused binding are absent; the placement assertions remain |
| FR4 | task0001 | TS-2 — the new test exists, pre-asserts, scrolls without relocation, post-asserts |
| FR5 | task0001 | TS-2 (red confirmation with both sites removed) and TS-3 (TS1 unchanged and still green) |
| FR6 | task0001 | TS-5 — SPEC's unreachability section is present and accurate; the rewritten EC1 comment reflects it. Note: SPEC.md itself already discharges this requirement (create-spec phase); task0001 carries it into the test comment and must not contradict it |
| FR7 | task0001 | TS-6 — both records reviewed |
| FR8 | task0001 | TS-3 plus a diff inspection showing every changed Rust line inside a `#[cfg(test)]` module |
| NFR1 | task0001 | TS-4 — the suite passes before and after with the same test set plus the one added by FR4 |
| NFR2 | task0001 | TS-4 plus a diff inspection showing `crates/term_core/Cargo.toml` unchanged |
| NFR3 | task0001 | TS-4 — the suite is green under the default (parallel) harness; the new test owns its terminal instance and touches no process-global state |
| NFR4 | task0001 | TS-4 plus review: both test names follow the crate's convention and each carries a leading comment naming its criterion / requirement IDs |
| NFR5 | task0001 | TS-4 — the format check is clean |

## E2E Testing

Not applicable. The `term_core` component defines no E2E command in
`workflow.yaml` and no E2E inputs were resolved for this feature.

## Manual Testing (E2E Not Possible)

Both items are documentary reviews; neither has an automated projection. The
design step was skipped (test-only change, no visual artifact), so no mockup
comparison applies.

- [ ] TS-5 — Review `feature-docs/relocate-wrap-ec1-scroll-test/SPEC.md`'s
      "Unreachability of the deletion branches on the scroll path" section:
      all three mechanisms present, file:line evidence accurate against the
      current tree, and the `shift_rows_up` out-of-scope note present.
- [ ] TS-6 — Review both records: this feature's
      `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` maps every
      criterion AC-1 … AC-8 (with a stated reason wherever the test list is
      empty) and its AC-4 entry carries the verbatim failure message plus the
      redundancy and two-tests-red notes; the corrected AC-6 entry of
      `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` names the
      renamed test and describes the vacuity, with every other entry in that
      file byte-identical.
- [ ] Review that neither the new test's comment nor either record claims the
      eviction-time clearing property was previously unpinned — the
      pre-existing `test_ring_push_blank_clears_ridx` already covers it
      partially (IMPLEMENTATION.md D4).

## Performance / Security Verification (if applicable)

Not applicable. Zero runtime behavior change (NFR1), and the diff touches
test code and documentation records only (FR8) — no input-handling,
authentication, authorization or data-protection surface is modified. The
new test's runtime addition is negligible (NFR3).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 6 | 4 (TS-1 … TS-4) | 0 | 2 (TS-5, TS-6) |
| Code quality | 1 | 1 | 0 | 0 |
| Success criteria | 8 | 5 (AC-1 … AC-3, AC-5, AC-8) | 0 | 3 (AC-4, AC-6, AC-7) |
| Requirements | 13 | 11 | 0 | 2 (FR6, FR7) |
