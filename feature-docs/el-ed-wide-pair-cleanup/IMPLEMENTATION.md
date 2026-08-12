# Implementation Plan: el-ed-wide-pair-cleanup

## Overview

EL (CSI K) and ED (CSI J) leave orphaned wide-pair halves at the edges of their cursor-row `clear_line_range` erases. This plan adds the same wide-pair partner cleanup ECH already performs to the four EL/ED cursor-row call sites in `crates/term_core/src/csi_screen.rs`, guarded by inline regression tests. The feature is a single task; this document records the decisions that frame it.

## Technology Stack

- **Language**: Rust — existing `crates/term_core` crate; no other component is touched.
- **Key internal primitives (both unchanged)**: `TerminalCore::clear_line_range` (BCE range clear), `TerminalCore::blank_wide_pair_split` (partner blanking).
- **New dependencies / licenses**: none added (NFR1). `crates/term_core` keeps its current dependency set (serde / bincode / log / unicode-width); there is no new dependency license to record against the project's MIT license.

## Layer Structure

Single crate, one dependency direction:

- **CSI handlers** (`csi_screen.rs`: `handle_erase_in_display`, `handle_erase_in_line`, `handle_erase_characters`) — decide WHICH cells are affected, including the new boundary-partner decision.
- **Row/cell primitives** (`terminal_rows.rs`: `clear_line_range`, `clear_line`; `csi_edit.rs`: `blank_wide_pair_split`) — perform the mutations. This feature changes NO primitive; handlers gain calls only.

## Shared Components

Existing components this feature builds on — contracts pinned here so the task implements against them without further discovery:

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `blank_wide_pair_split(col, row)` — `csi_edit.rs`, pub(crate) | Blank one orphaned wide-pair half | Pre: any col/row (including out of bounds). Post: if the cell is width 0 (spacer) or width 2 (base), its content becomes a single space and its width becomes 1; fg/bg/flags/hyperlink are preserved; the cell's overflow entry is removed; the row is marked dirty. No-op for out-of-bounds columns and for cells that are not a spacer/base half (NFR4). | task0001 |
| `clear_line_range(row, start, end)` — `terminal_rows.rs` | BCE-clear `[start, end)` on one row | Unchanged by this feature (NFR2): BCE fill, overflow clearing, and dirty-row marking behave exactly as today. | task0001 |

## Conventions

- Inline `#[cfg(test)]` unit tests next to the handlers in `csi_screen.rs`; test names follow the crate convention `<subject>_<scenario>_<expected>` (see `test/README.md`).
- Build / test / format commands are the workflow.yaml project commands, verbatim (listed in VERIFICATION.md). Run from the project root; never `cd` into a crate directory.
- No file outside `crates/term_core/src/csi_screen.rs` is modified.

## Cross-task Design Decisions

### D1: Approach (a) — local pre-capture at the EL/ED call sites; nothing folded into `clear_line_range`

SPEC.md left two approaches open. **Approach (a) is chosen**: replicate ECH's local pre-capture pattern at the four EL/ED cursor-row call sites (`csi_screen.rs:14`, `:25`, `:49`, `:53`). Approach (b) — folding partner cleanup into `clear_line_range` with a full-row exception — is rejected.

Rationale:

1. Approach (b) is, in substance, the partner-cleanup chokepoint refactor that REQUIREMENTS.md §1.3 / FR7 explicitly keep out of scope. It would also require removing or deduplicating ECH's existing local cleanup to avoid applying the partner step twice, widening the change further.
2. `clear_line_range` is a public primitive with callers beyond the erase handlers SPEC.md analysed; changing its behavior would extend the blast radius past what NFR2 ("no change to erase semantics inside the cleared range") was scoped against.
3. Approach (a) follows the already-reviewed ECH pattern (`csi_screen.rs:74-93`), keeps every primitive untouched, and satisfies NFR2/NFR3 by construction.

**Comment reconciliation**: the existing comment at `csi_screen.rs:74-78` states the partner blanking "is never folded into clear_line_range itself" because that function "is shared with ED/EL (out of scope for this cleanup)". Under approach (a) the core claim remains true and the comment stays; only the clause describing ED/EL as out of scope must be refreshed to state that the ED/EL cursor-row call sites now perform the same local cleanup, while the durable reason — `clear_line_range` is a shared primitive whose full-row callers must not gain partner behavior — is kept. This refresh is part of task0001.

### D2: Uniform symmetric boundary pattern at all four call sites

Each of the four cursor-row erase call sites applies the same three-step shape ECH uses:

1. Before clearing, record two predicates from the pre-erase grid: (p1) the cell at the range start has width 0 (spacer); (p2) the cell at range end − 1 has width 2 (base). Capture must precede the clear — the BCE fill destroys the width information.
2. Perform the existing range clear, unchanged.
3. After clearing: if p1 holds and the range start is greater than column 0, blank the partner base at start − 1; if p2 holds, blank the partner spacer at the range end.

The pattern is applied symmetrically (both predicates) at every site even though each EL/ED variant has only one materially relevant edge: EL 0 / ED 0 erase `[col, cols)`, so the right-edge blank targets column `cols` (out of bounds — safe no-op per NFR4); EL 1 / ED 1 erase `[0, col+1)`, so the left edge is guarded by start > 0 with start = 0. Uniformity keeps all four sites reviewable against the single ECH reference pattern, and the per-call cost stays within NFR3's bound (at most two width lookups and two conditional single-cell writes — the profile ECH already pays).

### D3: Full-line clears stay outside the pattern (FR4)

EL 2, ED 2, and every full-row `clear_line` call inside ED 0/1 keep their current behavior with no partner step: a wide pair cannot straddle a full-row erase boundary. Only the four cursor-row `clear_line_range` call sites gain the D2 pattern.

## Known Remaining Work (FR7)

Deliberately out of scope, recorded here per FR7:

- Partner-cleanup primitive consolidation / chokepoint refactor (approach (b), rejected in D1).
- Overflow-path tests.
- ECH / DCH / ICH / print paths — already handled by PR #30 (wide-pair-overwrite-cleanup), merged into the integration base; no changes needed.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Pattern drift among the four duplicated call sites (e.g. capturing predicates after the clear) | Low | Medium | D2 fixes one uniform shape; TS-1/TS-2/TS-3 exercise each path; attribute-preservation assertions distinguish partner blanking from BCE fill |
| Boundary index mistakes at col 0 / col+1 == cols | Low | Medium | `blank_wide_pair_split` is a no-op out of bounds (NFR4); TS-5 covers both boundaries |
| Behavioral regression in existing width-1 EL/ED semantics | Low | High | Pre-existing `csi_screen.rs` tests remain unmodified and must pass (AC5); NFR2 forbids touching primitives |

## Open Questions

- None.
