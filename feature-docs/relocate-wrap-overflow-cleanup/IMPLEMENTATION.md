# Implementation Plan: relocate-wrap-overflow-cleanup

## Overview

Close the overflow-table deletion gap in term_core's wrap-relocation path so
that the invariant both ASCII writers' marker gate depends on holds in code,
and state that invariant at the two points that depend on it. Memory-retention
repair only — no observable behavior changes.

## Technology Stack

- **Language**: Rust (existing `crates/term_core` workspace crate).
- **Test harness**: the standard library test harness via `cargo test`
  (`#[cfg(test)]` inline modules). No new framework.
- **New dependencies**: none. No dependency and no dev-dependency is added, so
  the project's MIT license is unaffected and no license compatibility question
  arises for this feature.

## Layer Structure

The change lives entirely inside term_core's print subsystem. Dependency
direction is unchanged:

| Layer | Files | Role in this feature |
|---|---|---|
| Print write paths | `print_handler.rs` (grapheme writer, merge helpers, relocation, slow ASCII writer) | Owns every mutation of cell content and of the overflow table |
| Byte dispatch fast path | `terminal_dispatch.rs` | A parallel ASCII writer that must stay in parity with the slow one |
| Cell / index helpers | `cell.rs` (reverse-index helpers) | Called by the write paths; not modified |
| Readers | `terminal_cells.rs`, `snapshot.rs`, reflow, ring eviction | All gated on the cell's overflow marker; not modified |

term_core's public API is unchanged (NFR2).

## Shared Components

Only one task exists, so no component is built by one task and consumed by
another. The single entry below is the subsystem-wide contract this feature
establishes; it binds every current and future write path in the print
subsystem, not just this task.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Overflow-table entry lifecycle (`overflow` + `overflow_ridx` in `terminal_core.rs`, mutated from `print_handler.rs` / `terminal_dispatch.rs`) | Hold cell content that does not fit inline, keyed by (column, absolute row), with a row→column reverse index | **Invariant**: an entry exists at a key only while the cell at that key reports overflow-bound (its marker value is set). **Obligation (pre → post)**: any write that leaves a cell not overflow-bound, where it may previously have been overflow-bound, removes that cell's own entry; the reverse index is updated only when the removal actually removed something. **Postcondition**: after removal of a row's last column, the reverse index holds no entry for that row. | task0001 |

## Conventions

- **Removal shape**: every new removal mirrors the shape the existing removal
  sites already use — attempt removal at the (column, absolute row) key, and
  update the reverse index only when the attempt reports that an entry was
  actually removed. Never update the reverse index unconditionally.
- **Conditional vs unconditional**: a write whose result may still be
  overflow-bound branches (insert when still overflow-bound, remove
  otherwise); a write that always clears the marker (a spacer write) removes
  without a branch for that cell.
- **Marker read position**: in both ASCII writers, the pre-write marker read
  stays before the write that clears the marker. A read placed after the write
  always observes "not overflow-bound" and silently skips the cleanup.
- **Comment language**: code comments in English, matching the surrounding
  file. Comments reference requirement IDs (FRn/NFRn) as the surrounding code
  already does.
- **Error handling**: no new error paths, no new logging. Every added removal
  degrades silently when the target cell or the key does not exist.

## Cross-task Design Decisions

### D1: Approach (a) — establish the invariant in code

The invariant is made true by adding the missing removals at the relocation
path, rather than by returning both ASCII gates to the ring-wide
"table is non-empty anywhere" self-healing form. Rationale: the self-healing
form is what the prior feature deliberately removed to recover the per-byte
ASCII cost, and the relocation path is reached only from a variation-selector
widening of a last-column base cell with auto-wrap on — never per byte.
Approach (b) is a documented fallback only (see D3). Affected: task0001.

### D2: The obligation is documented at the point of dependence

The invariant text and the deletion obligation it creates live in both ASCII
writers' marker-gated cleanup blocks — not only in this document. A future
author adding a path that clears a marker must be able to read the obligation
where the dependence exists, without following an external reference.
Affected: task0001.

### D3: Fallback branch ownership

If approach (a) is shown infeasible during implementation, approach (b) is
taken instead: both ASCII gates return to the self-healing form and the record
states how the per-byte cost budget is then covered. Taking this branch is a
reportable plan deviation — the implementer states the infeasibility evidence
and the cost argument in its completion report rather than switching silently.
Affected: task0001.

### D4: Assertion surface for the invariant

The invariant has no observable projection by construction (every reader is
gated on the cell's overflow marker), so its regression test asserts on the
overflow table and its reverse index directly through in-crate visibility, or
through the snapshot type's public overflow field. This is a deliberate,
recorded deviation from the project's "assert on observable contracts" test
guidance. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A removal is placed where the cell is still overflow-bound, deleting live content | Low | High (visible wrong cell content) | Keep the conditional shape for the base write; cover the still-overflow case with its own test (TS2) |
| The regression test passes before the fix (does not actually pin the defect) | Medium | Medium (false confidence) | Write the test first and observe it fail on the unmodified code; pre-populate the target row with width-1 overflow-bound cells so the existing wide-pair blanking helpers, which only fire for width 2 / width 0 neighbors, cannot mask the defect |
| The added removals regress the per-byte ASCII budget | Low | Medium | Removals are confined to the relocation path; both ASCII gates and their marker-read positions stay untouched |
| Scope creep into the sibling feature's documents or into other removal sites | Medium | Low | Task plan lists the three permitted files and an explicit out-of-scope list |

## Open Questions

- [ ] None. Every requirement is resolved in workflow.yaml; the only
      conditional branch (the approach-(b) fallback) has an owner and a
      reporting rule in D3.
