# Implementation Plan: relocate-wrap-cursor-clamp

## Overview

The tail of `relocate_widened_base_via_wrap` (`crates/term_core/src/print_handler.rs`) sets the
cursor column to a fixed value that lies outside the grid on 1- and 2-column terminals, so the next
printed character is silently discarded. This feature replaces that unconditional assignment with a
column-count boundary clamp shaped like the one the non-final-column widening path
(`widen_after_merge`) already uses, and pins the resulting cursor contract with unit tests.

## Technology Stack

- **Language**: Rust — the change is confined to the `term_core` library crate (NFR1).
- **Test harness**: the crate's built-in unit-test facility, exercised from the inline test module
  `crates/term_core/src/print_handler/tests.rs`, per term_core's existing convention (NFR4).
- **Formatting**: rustfmt, style_edition 2024 (NFR4).
- **New dependencies**: none. No crate is added, upgraded or removed, so no new license enters the
  project and `project.license` (MIT) is unaffected. There is nothing for the license review
  perspective to cross-check for this feature.

## Layer Structure

- `term_core` print path (owner of the change): the VS16 retroactive-widening logic. Both the
  non-final-column path and the final-column relocation path live here, and the change touches only
  the cursor update at the tail of the relocation path.
- `term_core` cursor/grid primitives (read-only context): the carriage-return and line-feed helpers
  used by the relocation path leave the wrap-pending flag untouched, and the grid's cell lookup
  rejects out-of-range columns. Neither is modified; both constrain the contract in D3 and D4.
- Consumers of `term_core` (the terminal binary and its mux/render layers): unchanged. No exported
  item, signature, or observable behaviour on 3-or-more-column grids changes, so no consumer needs
  adaptation (NFR1, NFR3).

Allowed dependency direction is unchanged: nothing outside `term_core` is read or written by this
feature, and `term_core` gains no new outward dependency.

## Shared Components

The feature is a single task (D1) and introduces no new component, so nothing is shared between
tasks. The one contract the feature defines is stated here regardless, because the review and verify
phases read it independently of the task plan.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Cursor update at the tail of `relocate_widened_base_via_wrap` | Leave the cursor in a valid position after a last-column base cell has been relocated to the start of the next row | **Pre**: the relocated base cell occupies column 0 of the new row; a spacer occupies column 1 when that column exists; the wrap-pending flag may still carry the value it held on entry to relocation. **Post**: the cursor column always points inside the grid. When the prospective column (relocated base column + widened width, i.e. 0 + 2) is at or past the column count, the cursor is the last column and wrap-pending is raised; otherwise the cursor column is 2 and wrap-pending is cleared. The cursor row and all non-cursor post-steps are unchanged from today. | task0001 |

## Conventions

- **Naming**: no new public or private item is introduced. The existing 1-column test is renamed so
  its name expresses pinning the cursor contract rather than only panic-freedom (FR5).
- **Error handling**: this path has no fallible operations and none is added. Out-of-grid
  coordinates keep being absorbed by the existing bounds-checked cell lookup, which is not modified.
- **Logging**: none added; the print path stays allocation- and log-free.
- **Tests**: inline test module only — no new `tests/` directory, no new test helper module (NFR4).
- **Formatting**: rustfmt is applied to the added and changed lines only. Pre-existing formatting
  drift elsewhere in the test file is left untouched (NFR4).

## Cross-task Design Decisions

### D1. One task for the whole feature

The production change and every test that pins it live in the same two files, and the tests ARE the
acceptance contract of the production change: each must be red before the change and green after,
inside one TDD session. Splitting production and tests into separate tasks is not viable here —
tasks run fully in parallel with no ordering, so a test-only task could never observe the production
change, and both tasks would still be confined to the same two files. The work also fits a single
implementer session (7 acceptance criteria, one function tail plus three unit-test edits).

Affected tasks: task0001.

### D2. The clamp mirrors the non-final-column path's branch structure, including its inner guard

The clamp is written as the same two-branch structure the non-final-column widening path uses:
compare the prospective cursor column against the column count; the at-or-past branch nests its
cursor and wrap-pending assignment inside the auto-wrap mode guard; the in-range branch assigns the
cursor column directly. The inner auto-wrap guard is kept even though the relocation path's sole
call site already sits inside an auto-wrap branch, which makes the guard's false side unreachable
and untestable. This trades one unreachable branch for direct textual comparability against the
acceptance criteria and against the review finding this feature closes (assumption A1, answer
`mirror-verbatim`).

Affected tasks: task0001. Verified by manual code-shape inspection (VERIFICATION.md), since a
literal shape match is not expressible as a runtime assertion.

### D3. The in-range branch keeps clearing the wrap-pending flag

The shape alignment applies to the cursor-column branch structure at the column-count boundary, not
to the wrap-pending line. The in-range branch retains its wrap-pending clear because the
carriage-return and line-feed helpers the relocation path uses leave the flag untouched: this single
line is the only thing that lowers a flag raised before relocation. Dropping it would leave
wrap-pending raised on 3-or-more-column grids and break the existing 5-column test's flag assertion
and the placement of the character printed after it. The non-final-column path has no such line
because that path is already reached with the flag lowered (assumption A3, NFR3).

Affected tasks: task0001.

### D4. Scope fence: cursor update only

The production diff is limited to the cursor update at the tail of the relocation path. Content
transfer, overflow-table synchronisation, the wrap-continuation flag, dirty marking and the
last-write bookkeeping are left byte-identical (FR4). The 1-column degraded case — a width-2 base
cell written with no spacer, because the spacer column does not exist — is existing behaviour
originating in the grid's column bounds check and is explicitly NOT changed (NFR2, assumption A2).

Affected tasks: task0001.

### D5. Verification split between task and feature

Task-level acceptance is the term_core unit suite (the task's own TDD contract). Feature-level
integrated verification adds the main-crate library suite as a regression check and the component
format check; both live in VERIFICATION.md, not in the task plan. The main-crate suite needs font
and web-bundle provisioning in a fresh worktree, so the term_core suite is the primary gate and the
main-crate suite is a secondary regression check.

Affected tasks: task0001 (task-level part only).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The wrap-pending clear on the in-range branch is dropped while "aligning the shape", regressing 3-or-more-column grids | Medium | High | D3 states the retention and its reason explicitly; the unmodified existing 5-column test (TS5) fails immediately if it is dropped |
| The clamp is written against the old cursor column instead of the relocated base column (0), producing the wrong boundary comparison | Low | High | The contract in Shared Components fixes the prospective column as relocated base column + widened width; TS2 (2-column grid) fails if the wrong base is used |
| Scope creep into the 1-column no-spacer degraded behaviour while touching the same function | Medium | Medium | NFR2 / D4 declare it out of scope; the task plan's Out of Scope section repeats the fence; the file set is limited to two files |
| The unreachable inner auto-wrap guard is flagged by review as dead code | High | Low | Recorded as a deliberate trade-off in D2 with its source answer, so review can judge it as accepted rather than as a defect |
| The format check fails on pre-existing drift unrelated to this change | Medium | Low | VERIFICATION.md judges the format check on the added/changed lines only (NFR4) and records the pre-existing drift as a known caveat |

## Open Questions

- [ ] NFR5 (E2E out of scope) has no implementing task and no verifying test by construction: it is
      a scope declaration satisfied by the absence of E2E items in VERIFICATION.md, not by anything
      a task builds. It is left with empty `tasks` / `tests` deliberately rather than force-mapped.
- [ ] AC5 (the clamp is textually identical in shape to the non-final-column path) is verifiable
      only by code inspection; it is carried as a manual verification item, so it is not covered by
      any automated test.
