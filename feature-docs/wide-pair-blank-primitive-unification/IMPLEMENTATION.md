# Implementation Plan: wide-pair-blank-primitive-unification

## Overview

Behavior-preserving internal refactor of `crates/term_core`: unify the two
duplicated wide-pair partner-blanking implementations (`blank_wide_pair_split`
in `csi_edit.rs`, `blank_wide_pair_partner` in `print_handler.rs`) into a
single self-guarding primitive on the grid-cell mutation base layer, and
collect the five copy-pasted range-erase edge repairs in `csi_screen.rs` into
one chokepoint. No observable behavior changes; no public API changes.

## Technology Stack

- **Language**: Rust — existing `term_core` crate only. No new modules, no new
  files, no new crates.
- **Dependencies / license**: no new dependency is introduced; there are no new
  dependency licenses to record, and the project license (MIT) is unaffected.

## Task Decomposition Decision

This feature is planned as a **single task (task0001)**. The functional
requirements are interdependent (the FR4 chokepoint calls the FR1 primitive;
FR3 spans all three handler files), the change is confined to five
tightly-coupled files in one crate, and tasks execute fully in parallel with no
ordering mechanism — a multi-task split would either share files (guaranteed
merge conflicts) or require placeholder wiring for a refactor that is
comfortably one implementer session. The Shared Components table below pins the
two contracts that are the design core of the feature; with a single task there
are no cross-task consumers, so no integration-wiring owner beyond task0001 is
needed.

## Layer Structure

Three layers; dependencies point downward only:

| Layer | Location | Responsibility |
|-------|----------|----------------|
| CSI / print handlers | `csi_screen.rs`, `csi_edit.rs`, `print_handler.rs` | Execute parsed terminal sequences; decide where invariant repair is needed |
| Range-erase edge-repair chokepoint | `csi_screen.rs` | The single capture → clear → repair sequence for in-row range erases (ECH, EL 0/1, ED 0/1) |
| Partner-blanking primitive | `terminal_cells.rs` (grid-cell mutation base layer) | The single D2-invariant repair: blank one wide-pair half to a width-1 space |

Handlers may call the chokepoint or the primitive; the chokepoint calls only
the primitive and the existing invariant-unaware range clear; the primitive
touches only grid state (cells, overflow table, overflow reverse index, dirty
marks). `clear_line` / `clear_line_range` (in `terminal_rows.rs`) sit below all
of this and remain invariant-unaware (FR5) — they are not modified.

## Shared Components

(Single-task feature: these contracts are cross-module rather than cross-task.
They are pinned here because they are the design core every migration in
task0001 implements against.)

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Partner-blanking primitive — `blank_wide_pair_half(col, row)`, `pub(crate)`, on the terminal core type in `terminal_cells.rs` | The one D2-invariant repair: blank a wide-pair half (orphaned spacer or base) to a width-1 space | **Pre**: none — self-guarding. **Post**: if `(col, row)` resolves to a valid cell whose current width is 0 or 2 — the cell becomes a width-1 space; fg/bg/flags/hyperlink are preserved; the overflow-table entry for that position and, when one was removed, its reverse-index entry are removed; the row is marked dirty. Otherwise (width 1, or position does not resolve): strict no-op, no mutation. | task0001 |
| Range-erase edge-repair chokepoint — suggested name `erase_range_with_edge_repair(row, start, end)`, crate-internal, local to `csi_screen.rs` | The one capture-then-repair path for in-row range erases | **Pre**: `row` is a viewport row; `[start, end)` is the half-open erase range. **Post**: empty range (`end` not greater than `start`) → delegates to the plain invariant-unaware range clear only, with no capture and no repair (preserves ECH's degenerate behavior). Non-empty range → (1) captures, BEFORE clearing, whether the cell at `start` is a spacer (width 0) and whether the cell at `end - 1` is a base (width 2); (2) performs the invariant-unaware range clear; (3) calls the primitive on `start - 1` when the start cell was a spacer and `start` is positive, and on `end` when the last erased cell was a base — the `end == column count` case is absorbed by the primitive's out-of-bounds self-guard. | task0001 |

## Conventions

- Requirement / test IDs: `FR1`..`FR5`, `NFR1`..`NFR3`, `TS-1`..`TS-5` —
  literal string match with workflow.yaml and VERIFICATION.md.
- Behavior preservation: existing tests keep their assertions untouched (NFR2).
  New tests may be added; existing test comments that name the removed
  functions may be updated (comment edits are not assertion changes).
- Visibility: everything new is crate-internal (`pub(crate)` or narrower);
  `lib.rs` re-exports are unchanged (NFR3).
- Error handling: the primitive has no error return — violated preconditions
  are no-ops, never errors or panics (FR2).

## Cross-task Design Decisions (batch-resolved assumptions)

Batch mode: every decision below is determined by SPEC.md plus the current
sources and is recorded here as an assumption instead of a user question.

1. **Primitive name and home**: `blank_wide_pair_half(col, row)` in
   `terminal_cells.rs`, per SPEC's suggested shape. Rationale: FR1 places the
   primitive on the grid-cell mutation base layer; `terminal_cells.rs` is that
   layer.
2. **Self-guard contract**: the primitive adopts `blank_wide_pair_split`'s
   existing semantics — valid-index resolution and the width-0/2 check happen
   inside, no-op otherwise (FR2; REQUIREMENTS.md 14.1 confirms the two prior
   implementations are output-equivalent and names this contract as the one to
   adopt).
3. **`blank_wide_pair_split` is fully removed, not kept as a forwarding
   shim**: SPEC AC3 allows either; full removal is chosen so no
   scheduled-for-removal residue survives. All csi_edit / csi_screen call
   sites migrate to the primitive.
4. **ICH/DCH call the primitive directly**, not through the FR4 chokepoint:
   their edge conditions differ from the capture-then-repair range pattern
   (SPEC explicitly leaves this to planning and warns against forcing them
   into the chokepoint).
5. **Chokepoint shape = erase-range wrapper** (capture + clear + repair inside
   one function), rather than a repair-only function taking pre-captured
   flags: SPEC offers both; the wrapper absorbs the capture step too, so a
   call site cannot mis-order capture/clear/repair, and it folds ECH's
   degenerate empty-range early return into the same chokepoint.
6. **FR5 constraint comment survives at the chokepoint**: the
   `handle_erase_characters` comment (`csi_screen.rs:140-146`) documenting
   that partner repair is never folded into `clear_line_range` — because its
   full-row callers (`clear_line`, EL 2, ED 2) must not gain partner behavior
   — is relocated/adapted to the chokepoint with the constraint statement
   intact.
7. **Existing-file disposition**: IMPLEMENTATION.md, VERIFICATION.md and
   `tasks/` did not exist at dispatch (write_policy action `create`), so no
   overwrite/merge decision arises.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Subtle behavioral drift between the two prior implementations surfaces during unification | Low | High | Adopt the self-guarding contract (the superset); full suite must pass with assertions untouched (TS-1); direct primitive tests (TS-2, TS-3) |
| ASCII fast path gains work | Low | Medium | `old_width != 1` gates stay at the three print call sites (NFR1); verified by diff inspection plus the optional bench spot-check (TS-5) |
| Chokepoint accidentally extends partner behavior to full-row paths | Low | High | FR5 acceptance criterion + TS-4; constraint comment preserved at the chokepoint |

## Open Questions

- None. All eight requirements are resolved; no TBD requirements, no license
  conflict, no design-step open items (design step was skipped).
