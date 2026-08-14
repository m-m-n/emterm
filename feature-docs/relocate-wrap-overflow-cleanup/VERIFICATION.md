# Verification Document: relocate-wrap-overflow-cleanup

## Overview

**Feature**: relocate-wrap-overflow-cleanup /
**SPEC.md**: `feature-docs/relocate-wrap-overflow-cleanup/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/relocate-wrap-overflow-cleanup/IMPLEMENTATION.md`

This document covers the INTEGRATED verification run after every task is
merged. Task-level acceptance criteria live in
`feature-docs/relocate-wrap-overflow-cleanup/tasks/task0001.md`.

All commands are run from the project root (never after `cd`-ing into a
subdirectory).

## Build Verification

- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path crates/term_core/Cargo.toml`
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only feature gate): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, no new warnings attributable to this change

## Test Verification

- Command (term_core): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coverage target: not measured — the project has no coverage tooling. The
  coverage contract for this feature is the per-requirement traceability table
  below: every functional requirement maps to at least one scenario ID.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Relocation onto a row whose columns 0 and 1 already hold overflow-bound content, triggered by a variation-selector widening of a last-column base with auto-wrap on, with no scroll | Neither column retains a table entry, neither retains a reverse-index entry, both cells report not-overflow | Unit |
| TS2 | Relocated content that itself exceeds the inline capacity | The entry at column 0 of the new row is present and equals the relocated content; the cell reports overflow-bound | Unit |
| TS3 | Existing relocation tests (`test_retroactive_widen_at_last_column_wraps_with_autowrap`, `test_relocate_widened_base_via_wrap_spacer_creation_blanks_existing_pair_spacer`) run unmodified | All their assertions (cell characters, widths, cursor column/row, wrap flags, wrapped-line flag) still hold | Unit (regression) |
| TS4 | An overflow-bound cell is overwritten with an ASCII byte through the slow ASCII writer and through the byte-dispatch fast path | Both leave the table and the reverse index free of that cell's entry, identical to pre-change behavior | Unit (regression) |
| TS5 | Suite level: the term_core library suite, the src-tauri library suite and the CLI-only check | All pass | Suite |
| EC1 | The relocation's line feed scrolls (the widened base sat on the last row) | The scrolled-in row ends free of marker-less entries; no panic; the eviction path is undisturbed | Unit (edge) |
| EC2 | The new row has no column 1 (terminal too narrow) | The spacer-side removal is skipped without panic | Unit (edge) |
| EC3 | No entry exists at a removal key | The removal attempt reports nothing removed and the reverse-index update is skipped | Unit (edge) |
| EC4 | The last column of a row is removed from the reverse index | The row key itself is dropped | Unit (edge) |

## Code Quality Verification

- Format (term_core): `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
- Format (main): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Static analysis: no separate lint command is configured for this project;
  compiler warnings from the build commands above serve as the static-analysis
  signal.
- Note: this project does not enforce rustfmt crate-wide, so a pre-existing
  formatting difference in a file this feature does not touch is NOT a failure
  of this feature. The criterion is that the three files listed in
  IMPLEMENTATION.md's scope are format-clean; unrelated drift is reported, not
  "fixed" by reformatting other files.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | The relocation path deletes the overflow entry at both the relocated base write and the spacer write, in the same shape the grapheme writer uses | TS1, TS2 pass; review confirms the conditional base branch and the unconditional spacer removal, each with a guarded reverse-index update |
| AC2 | Both ASCII writers' marker-gated cleanup blocks state the invariant and the deletion obligation | Manual inspection of both blocks (see Manual Testing) |
| AC3 | A unit test covers the non-scrolling line-feed descent onto an overflow-bound row | TS1 exists, and fails on the unmodified code |
| AC4 | Existing term_core and src-tauri tests pass and the CLI-only check succeeds | TS3, TS5 |
| AC5 | If approach (a) proved infeasible, approach (b) was taken and the per-byte cost argument is recorded | Manual: confirm the implementer's report; absent such a report, confirm approach (a) is what shipped (both gates still marker-based) |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2 |
| FR2 | task0001 | TS1 |
| FR3 | task0001 | TS1, EC3, EC4 |
| FR4 | task0001 | TS4 + manual inspection of both gate comments |
| FR5 | task0001 | TS4 + manual inspection that neither gate uses the ring-wide non-empty-table form |
| FR6 | task0001 | TS1 |
| FR7 | task0001 | TS3 |
| FR8 | task0001 | TS5 |
| NFR1 | task0001 | Manual inspection: no table access added to the per-byte ASCII path; both pre-write marker reads unmoved; added removals reachable only from the relocation path |
| NFR2 | task0001 | Manual inspection: the diff touches only the three files in scope; no dependency or dev-dependency added; term_core's public API unchanged |
| NFR3 | task0001 | EC1, EC2, EC3 |
| NFR4 | task0001 | Manual inspection: new tests follow test/README.md (inline test module, explicitly constructed core, input via the print / PTY-data entry points, file-local naming); no new dev-dependency |
| NFR5 | task0001 | Manual inspection: the invariant text sits at both dependent gates, not only in a feature document |

## E2E Testing

Not applicable. The project has no E2E framework (`e2e_test_command` is empty
for every component; `test/README.md` records that there are no E2E tests), and
this change has no user-visible surface.

## Manual Testing (E2E Not Possible)

Inspection items — each is a review-time read of the diff, not a runtime check.
They exist because a comment's presence, a gate's shape and a scope constraint
have no test projection.

- [ ] Both ASCII writers' marker-gated cleanup blocks state the invariant (an
      entry exists at a key only while that cell reports overflow-bound) and
      the obligation (a write that clears the marker owns removing that cell's
      entry). (AC2, FR4, NFR5)
- [ ] Neither ASCII gate was changed to the ring-wide non-empty-table form, and
      neither pre-write marker read moved after its write. (FR5, NFR1)
- [ ] No table access, absolute-row computation or other work was added to the
      per-byte ASCII path. (NFR1)
- [ ] The diff touches only `crates/term_core/src/print_handler.rs`,
      `crates/term_core/src/terminal_dispatch.rs` and
      `crates/term_core/src/print_handler/tests.rs`; no manifest change, no
      dependency or dev-dependency added, term_core's public API unchanged.
      (NFR2)
- [ ] The new tests follow test/README.md conventions and add no framework.
      (NFR4)
- [ ] The reverse-index update at every new removal is guarded on the removal
      having actually removed something. (FR3)
- [ ] No mockup comparison applies: the design step was skipped for this
      feature (no user-visible surface).

## Performance / Security Verification (if applicable)

- NFR1 (performance): satisfied structurally, not by measurement. The added
  removals are reachable only from a variation-selector widening of a
  last-column base cell with auto-wrap on, never from the per-byte ASCII path,
  and add at most two hash operations per relocation. Verified by the manual
  inspection items above; no benchmark is added.
- Security: not applicable. Internal invariant repair with no new input
  handling, no user-facing surface and no API change. The repair does reduce an
  unbounded-looking retention of content derived from terminal output; the
  retention was already bounded by columns × ring rows × inline-overflow size.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 3 | 3 | 0 | 0 |
| Test scenarios | 9 (TS1-TS5, EC1-EC4) | 9 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| Success criteria | 5 (AC1-AC5) | 3 | 0 | 2 |
| Inspection items | 7 | 0 | 0 | 7 |
