# Implementation Plan: ascii-fast-path-wide-pair-cleanup

## Overview

Close the D2-invariant gap in the PTY-dispatch ASCII fast path so that an
ASCII character overwriting a fullwidth cell never leaves an orphaned
width-0 spacer, and make the D2-repair call-site documentation in the cell
layer match the resulting set of repairing paths.

## Technology Stack

- **Language**: Rust (workspace crate `crates/term_core`, edition/toolchain
  as already pinned by the workspace).
- **Test framework**: the standard library test harness via `cargo test`
  (`test/README.md`). No test-framework crate is introduced.
- **New dependencies**: none. NFR2 forbids adding any dependency or
  dev-dependency, so no new license enters the project; `project.license`
  (MIT) is unchanged and no license question arises for this feature.

## Layer Structure

Three layers inside `crates/term_core`, with a strictly downward dependency
direction:

| Layer | File(s) | Responsibility | May call |
|-------|---------|----------------|----------|
| Dispatch | `terminal_dispatch.rs` | Split incoming PTY bytes between the ASCII fast path and the parser-driven slow path; own the fast path's inline write step | Print layer, Cell layer |
| Print | `print_handler.rs` | Grapheme/ASCII print semantics, wide-pair rules R1/R2/R3, cursor advance and wrap | Cell layer |
| Cell | `terminal_cells.rs` | Cell read/write primitives, overflow-table bookkeeping, the D2 repair primitive | — |

The Cell layer never calls upward; the Print layer never calls into the
Dispatch layer. This feature adds one new downward edge only: Dispatch →
Print (the shared orphan-neighbour repair entry point).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| D2 invariant | The grid contract this feature restores | No cell of width 0 may remain whose left neighbour is not a width-2 base, and no width-2 base may remain whose right neighbour is not a width-0 spacer, after any write completes | task0001, task0002 |
| Wide-pair blank primitive (`blank_wide_pair_half`, cell layer) | Rewrite one half of a broken wide pair to a width-1 blank in place | Pre: none — self-guarding. Takes a column and a row. No-op (no mutation, no panic) when the position does not resolve to a cell or the cell's current width is neither 0 nor 2. Post: the target holds a width-1 space; fg/bg/flags/hyperlink preserved; any overflow-table entry and its reverse-index companion for that position removed; the row marked dirty | task0001 (indirectly), task0002 (documents it) |
| Orphan-neighbour repair entry point (`blank_orphaned_neighbor_before_overwrite`, print layer) | Rules R1/R2: blank whichever neighbour an imminent overwrite would orphan | Pre: called BEFORE the overwrite lands, with the target's pre-write width. Takes column, row and pre-write width. Post: when pre-write width is 2, a still-width-0 cell one column right is blanked; when pre-write width is 0 and the column is not 0, a still-width-2 cell one column left is blanked; otherwise nothing changes. Never touches the target cell itself. Never reads or writes outside the target's own row | task0001 (calls it), task0002 (documents it) |
| Overflow-entry removal shape (print layer, ASCII writer) | Keep the overflow table and its reverse index consistent when a cell's long content is replaced by a single ASCII byte | Pre: performed for the cell just overwritten, gated on that cell's own pre-write overflow marker (task0004 replaced the earlier ring-wide "overflow table non-empty" gate; the marker is read from the same cell record as `old_width`, so the per-byte ASCII path pays no table probe). Post: no overflow entry and no reverse-index entry remains for that absolute-row/column position | task0001, task0004 |

### Visibility of the repair entry point

The repair entry point is currently module-private to the print layer, so
the dispatch layer cannot reach it. Its visibility is raised to
crate-internal **in place** — it is not relocated to another module and its
signature is not changed. Rationale: the rules it encodes (R1/R2) belong
next to the print-path rules R3 documents, the diff stays minimal, and
crate-internal visibility keeps term_core's public API unchanged as NFR2
requires. Its three existing call sites are untouched.

## Conventions

- **Tests**: inline test module next to the code under test;
  `<subject>_<scenario>_<expected>` naming; one explicitly constructed
  terminal core per test; input driven through the PTY-dispatch entry point;
  assertions on the observable grid contract (cell content, cell width,
  overflow-table state) rather than on internal-only bookkeeping
  (`test/README.md`, NFR4).
- **Comments**: new explanatory comments cite the requirement ID they serve
  (e.g. FR1, NFR1), matching the existing style in the print layer.
- **No dependency changes**: no crate manifest is edited (NFR2).
- **No public API change**: nothing that is publicly exported from term_core
  gains, loses or changes a signature (NFR2).

## Cross-task Design Decisions

### D-1: Parity comes from shared code, not duplicated logic

The fast path reaches the SAME orphan-neighbour repair entry point the slow
ASCII writer uses, rather than re-deriving the rules locally. This is what
makes FR3's fast/slow parity a property of shared code. Duplicating the R1/R2
conditions in the dispatch layer is rejected: it would create a second place
for the rules to drift.

Affected tasks: task0001 (implements it), task0002 (documents the resulting
reachability).

### D-2: Authoritative post-change D2-repair call-site set

Tasks work against this single enumeration; task0001 makes it true in code
and task0002 / task0003 make it true in the cell layer's documentation
(task0003 added items 3 and 4 after review round 1 found the VS16
lazy-widening step missing from the original six-item list). After this
feature, the D2 repair primitive is reached from:

1. the print path's grapheme writer, before an overwrite (rules R1/R2) and
   before a wide-pair placeholder write (rule R3);
2. the print path's ASCII writer, before an overwrite (rules R1/R2);
3. the print path's VS16 retroactive-widening step (`widen_after_merge`),
   before writing the newly widened base's spacer (rule R3);
4. the print path's widened-base relocation-by-wrap step
   (`relocate_widened_base_via_wrap`, the last-column branch of 3) (rules
   R1/R2/R3);
5. **the PTY-dispatch ASCII fast path's write step, before an overwrite
   (rules R1/R2)** — added by this feature;
6. the ICH/DCH edit path's edge repair;
7. the range-erase edge-repair chokepoint.

No other write path performs the repair. A reader of the primitive's
documentation must be able to determine this set without consulting any file
outside the crate.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The repair is placed after the write, so the pre-write width is lost and the repair silently never fires | Medium | High (the bug survives with tests that look green) | task0001 pins the write-step ordering; its regression test drives real bytes through the dispatch entry point rather than the print entry point |
| A test accidentally exercises the slow path and passes without the fix | High | High (false confidence) | The fast path is only entered at a chunk boundary where the parser is ground-clean and no grapheme is buffered; task0001 requires every fast-path test to split its input across separate dispatch calls, and the parity test contrasts a single-call (slow) run with a split (fast) run |
| Width-0 cells that are combining-mark residue get treated as wide-pair spacers | Low | Medium (legitimate content blanked) | The repair entry point keys off the wide-pair relationship (a width-2 partner), not width 0 alone; the primitive is additionally self-guarding |
| Column arithmetic at row edges panics or leaks into the adjacent row | Low | High (crash on adversarial PTY input) | The repair entry point bounds both directions within the row and the primitive no-ops on unresolvable positions; NFR3 edge cases are covered by an explicit test |
| Doc enumeration and code drift apart again (the exact failure this feature repairs) | Medium | Low | D-2 fixes one authoritative enumeration both tasks implement against; the verification plan includes an inspection item comparing them |
| The two tasks land unevenly (documentation claims a repair the code does not perform, or vice versa) | Low | Medium | Both tasks implement against D-2 rather than against each other; review reads the integrated diff, where a half-landed feature is visible as an enumeration/code mismatch |

## Open Questions

- [ ] FR5 (documentation truthfulness) and NFR4 (test conventions) have no
      automated test in SPEC.md's scenario list; both are verified by
      inspection during review. Recorded as a known coverage gap rather than
      resolved by inventing a scenario ID.
