# Implementation Plan: erase-edge-repair-dead-sides

## Overview

Remove the structurally unreachable edge capture / edge repair from the
in-row range-erase chokepoint of term_core's CSI screen handlers, and make
ED 0 / ED 1 erase their cursor row through EL 0 / EL 1. The externally
observable behavior of ED / EL / ECH does not change. The feature is a single
task (`task0001`).

## Technology Stack

- **Language**: Rust — crate `term_core` (`crates/term_core/`)
- **New dependencies**: none (no license impact; project license MIT)

## Layer Structure

One module changes: `crates/term_core/src/csi_screen.rs` (the CSI ED / EL /
ECH handlers on the terminal core). It consumes the cell-level primitives of
`crates/term_core/src/terminal_cells.rs`. Dependency direction stays
csi_screen → terminal_cells; terminal_cells changes in doc-comment text only.

Call structure after the change:

1. ED mode 0 → EL mode 0 on the cursor row → full-row clear of every row below the cursor row.
2. ED mode 1 → full-row clear of every row above the cursor row → EL mode 1 on the cursor row.
3. ED mode 2 and EL mode 2 → full-row clear only (no partner repair).
4. EL mode 0 → range-erase chokepoint, left-edge repair only.
5. EL mode 1 → range-erase chokepoint, right-edge repair only.
6. ECH → range-erase chokepoint, both-edge repair.

## Shared Components

No component is shared between tasks (single-task feature). The existing
terminal_cells primitives below are the fixed boundary the task builds on;
their signatures and behavior do not change (NFR2).

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `get_cell_width` | Read the width of the cell at (col, row) | No precondition; returns 1 for a position that does not resolve to a cell; no side effects | task0001 |
| `clear_line_range` | Plain, wide-pair-unaware clear of a half-open column range in one row | Unchanged; shared with full-row callers | task0001 |
| `clear_line` | Full-row clear | Unchanged; never reaches partner repair | task0001 |
| `blank_wide_pair_half` | Rewrite an orphaned wide-pair half (width 0 or 2) to a width-1 space and mark the row dirty | Self-guarding: no-op with no mutation when the position does not resolve to a cell or the width is neither 0 nor 2 | task0001 |

## Conventions

- **Behavior preservation**: grid content, cell widths, attributes, overflow
  entries, dirty flags and return values of ECH / EL 0-2 / ED 0-3 stay
  identical to the pre-change implementation (NFR1).
- **Test modules are frozen**: no edit inside any `#[cfg(test)]` module of
  term_core, comments included, and no new test anywhere (NFR3; SPEC A-2,
  A-3).
- **Change boundary**: code changes only in the non-test body of
  `csi_screen.rs`; `terminal_cells.rs` in doc-comment text only (NFR2). No new
  item becomes visible outside `csi_screen.rs`.
- **Formatting**: after any formatter run, the diff is re-checked so that no
  line inside a `#[cfg(test)]` module and no file outside the two listed files
  changed.
- **Logging**: no log output is added or changed.

## Cross-task Design Decisions

### D1: Single-task decomposition

All edits land in one private chokepoint and three public handlers of one
file. Splitting would produce overlapping edits to the same functions with no
parallelism benefit. Affected tasks: task0001.

### D2: Edge-selection shape (SPEC A-5)

Resolved in `tasks/task0001.md` (Design section), since only task0001 uses it.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Behavior drift at boundaries (cursor at column 0, cursor at the last column, cursor column at or past the column count, ECH empty range) | Low | Medium | The empty-range short-circuit applies to every edge selection; boundary tests in TS-1 / TS-2 / TS-3 run unchanged |
| A `#[cfg(test)]` module changes by accident (formatter, rename sweep) | Medium | High (NFR3) | Diff inspection restricted to test modules (TS-8) |
| ED delegation changes the processing order | Low | Medium | Order fixed in task0001 Acceptance Criteria; ED tests in TS-2 / TS-3 |
| `blank_wide_pair_half` caller list drifts from the actual callers | Low | Low | The chokepoint keeps its name and role; doc inspection (TS-9) |

## Open Questions

- None.
