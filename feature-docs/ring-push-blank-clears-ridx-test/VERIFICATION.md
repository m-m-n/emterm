# Verification Document: ring-push-blank-clears-ridx-test

## Overview

**Feature**: ring-push-blank-clears-ridx-test /
**SPEC.md**: `feature-docs/ring-push-blank-clears-ridx-test/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/ring-push-blank-clears-ridx-test/IMPLEMENTATION.md`

This document covers the INTEGRATED verification of the feature. Task-level
acceptance criteria live in `feature-docs/ring-push-blank-clears-ridx-test/tasks/task0001.md`.

All commands are run from the project root, per the project's build-location
rule; no `cd` into a crate directory.

## Build Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Expected: exit code 0, no errors and no new warnings attributable to this
  feature.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Expected: exit code 0, every `term_core` library test green, run with the
  default parallelism (no thread-count restriction).
- Coverage target: **not applicable as a percentage** — the project configures
  no coverage tooling, and this feature adds no production code for a coverage
  metric to measure. The meaningful coverage measure here is mutation
  detection: TS3 below is the quantitative gate, and it is pass/fail rather than
  a percentage.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Row-scope observation: place overflow-bound content on the recycled row and on a survivor row of a 10x3 terminal core with scrollback capacity 2, capture both absolute row numbers, push one blank row | The recycled row's entries are absent from both `overflow` and `overflow_ridx`; the survivor row's entries are present in both. Asserted as four independent assertions | Unit |
| TS2 | Emptiness after a full cycle: continuing from TS1, push four more blank rows (five in total) | Both `overflow` and `overflow_ridx` are empty | Unit |
| TS3 | Mutation detection: replace the compress branch's row-scoped clears in `crates/term_core/src/ring_buffer.rs` with clear-everything counterparts, run the test command, then restore | `test_ring_push_blank_clears_ridx` FAILS while the mutation is in place, and the file is byte-identical to its committed content afterwards | Mutation (manual procedure — see Manual Testing) |
| TS4 | Crate-wide regression: after restoring, run the full `term_core` library test command | Exit code 0; every test green, including the sibling row-scope test and every pre-existing ring-buffer test | Regression |
| TS5 | Formatting: run the format command | No diff reported; no file outside the feature's declared change set is modified | Format |

## Code Quality Verification

- Format: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Static analysis: no lint command is declared for this component in
  `workflow.yaml`; the compiler's own diagnostics from the build command above
  are the static-analysis gate.
- Change-set containment: the working tree diff for the feature must touch
  exactly one source file, `crates/term_core/src/ring_buffer/tests.rs`, plus the
  workflow-generated `feature-docs/` and `test-docs/` entries. Any diff in
  `crates/term_core/src/ring_buffer.rs` is a verification failure (NFR1).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | All functional requirements FR1-FR8 are implemented | The Functional Requirements Coverage table below, plus reading the changed test against `tasks/task0001.md` |
| SC2 | All acceptance criteria AC1-AC7 hold | AC1-AC3 and AC6-AC7 via TS1, TS2, TS4, TS5; AC4 via manual item M1; AC5 via TS3 |
| SC3 | All test scenarios TS1-TS5 pass | Run the build, test and format commands above and carry out TS3's manual procedure |
| SC4 | No diff remains in `crates/term_core/src/ring_buffer.rs` | Inspect the working tree / the feature diff for that path — it must be empty |
| SC5 | The format command is clean | TS5 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS4 — the survivor-row fixture is what makes TS1's survival assertions meaningful |
| FR2 | task0001 | TS1, TS4 — absolute row numbers captured before the push; a viewport-relative reuse makes TS1 fail |
| FR3 | task0001 | TS1, TS4 — the anti-vacuity pre-assertions run inside TS1 |
| FR4 | task0001 | TS1, TS4 — the four independent post-push assertions |
| FR5 | task0001 | TS1, TS4 — the row-scope assertions occur at the one-push point, before the survivor is evicted |
| FR6 | task0001 | TS2, TS4 — the retained emptiness assertions after five pushes |
| FR7 | task0001 | Manual item M1 (doc-comment content review). No automated scenario — see Open Coverage Notes |
| FR8 | task0001 | TS3 — the mutation run must turn the test red |
| NFR1 | task0001 | TS3 (restoration half) and the change-set containment check under Code Quality Verification |
| NFR2 | task0001 | Manual item M2 (style conformance and absence of a new dev-dependency). No automated scenario — see Open Coverage Notes |
| NFR3 | task0001 | TS4 — the test command is run with default parallelism and no `--test-threads` restriction |
| NFR4 | task0001 | TS5 |

## E2E Testing

Not applicable. The component declares no E2E command (`e2e_test_command` is
empty in `workflow.yaml`), and the change is confined to one unit test inside
`term_core`.

## Manual Testing (E2E Not Possible)

- [x] **M1 — Doc-comment coverage boundary (FR7, AC4)**: read the doc comment of
      `test_ring_push_blank_clears_ridx` and confirm it states all three of:
      what the test proves (nothing beyond the evicted row is cleared); what it
      does not prove (that the compress branch's clear site fired); and the
      structural reason — within one push the new viewport's bottom absolute row
      equals the evicted absolute row, so the eviction-time clear and the
      unconditional bottom-row clear cannot be told apart by any fixture.
- [x] **M2 — Style conformance and dependency hygiene (NFR2)**: confirm the test
      mirrors the sibling test
      `test_ring_push_blank_clears_recycled_row_overflow_entries` in structure
      (capture absolute row keys, pre-assert non-vacuity, operate, post-assert
      removal and survival) and phrasing, that it remains inside the crate's
      inline test module, and that no manifest file appears in the change set.
- [x] **M3 — TS3 mutation procedure (FR8, AC5)**: carried out as follows, and
      recorded with its observed output:
      1. Edit `crates/term_core/src/ring_buffer.rs` so the compress branch's
         row-scoped clear calls become clear-everything calls on both tables.
      2. Run the test command from Test Verification.
      3. Confirm `test_ring_push_blank_clears_ridx` FAILS, and note which
         assertion fired.
      4. Restore the file and confirm its diff is empty.
      5. Re-run the test command and confirm everything is green again (this
         re-run is TS4).
- [x] **M4 — Anti-vacuity spot check (FR3)**: confirm that the pre-assertions
      fire against the fixture actually chosen, not merely against the baseline
      fixture named in SPEC.md — i.e. that the content written to the two rows
      really does exceed the inline per-cell capacity.

## Performance / Security Verification

Not applicable. The change adds no runtime path, no input handling, no network
surface and no persisted data. The only timing-adjacent requirement is NFR3
(determinism and no long-running path), covered by TS4.

## Open Coverage Notes

- **FR7** and **NFR2** carry no test scenario ID; their `tests` lists in
  `workflow.yaml` stay empty by design. Both are verified by the manual items
  above (M1, M2). Automating FR7 would mean asserting on comment text, which is
  a weaker check than reading it; NFR2's style half is a review judgement.
  These are recorded as open items rather than silently mapped to an unrelated
  scenario.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 1 | 1 | 0 | 0 |
| Test scenarios | 5 (TS1-TS5) | 4 (TS1, TS2, TS4, TS5) | 0 | 1 (TS3, via M3) |
| Success criteria | 5 (SC1-SC5) | 3 (SC3, SC4, SC5) | 0 | 2 (SC1, SC2 include manual reads) |
| Manual items | 4 (M1-M4) | 0 | 0 | 4 |
