# Feature: wide-pair-blank-primitive-unification

## Overview

Wide-pair partner blanking — the D2 invariant repair that rewrites the surviving
half of a broken wide pair into a width-1 space — is currently implemented twice:
`blank_wide_pair_split` in `csi_edit.rs` and `blank_wide_pair_partner` in
`print_handler.rs`. Separately, the range-erase edge repair is copy-pasted at five
call sites in `csi_screen.rs` after the el-ed-wide-pair-cleanup merge. This feature
unifies both into a single primitive on the cell-mutation base layer and a single
range-erase edge-repair chokepoint, without changing observable behavior.

Requirements source: `REQUIREMENTS.md` (Japanese) in this directory. This document
is the implementation-facing rendering of the same requirements.

## Objectives

- Eliminate the duplicated wide-pair partner-blanking implementations
  (`csi_edit.rs` `blank_wide_pair_split` and `print_handler.rs`
  `blank_wide_pair_partner`) so the D2 invariant repair has a single source of
  truth and cannot diverge between the print path and the CSI edit/erase paths
  (review finding b8a62feaf016ef08).
- Collect the range-erase edge repair — now copy-pasted at five call sites
  (ECH, EL 0, EL 1, ED 0, ED 1) after the el-ed-wide-pair-cleanup merge — into one
  chokepoint so future grid-mutating handlers do not re-derive the same boundary
  reasoning (review finding 931abe859e23fa5d).

## User Stories

### US1: One partner-blanking primitive to call

As a term_core maintainer, I want a single partner-blanking primitive on the
cell-mutation base layer, so that the print path and the CSI edit/erase paths
cannot diverge in how they repair the D2 invariant.

**Acceptance Criteria:**
- [ ] AC1: Exactly one partner-blanking primitive exists, located in the grid-cell
      mutation base layer (`terminal_cells.rs` or equivalent), with the width-0/2
      self-guard inside it.
- [ ] AC2: `print_handler.rs` no longer defines `blank_wide_pair_partner`; print and
      CSI edit/erase paths call the same primitive.
- [ ] AC3: `csi_edit.rs` no longer defines `blank_wide_pair_split` as an independent
      implementation (it is removed or is a thin forwarding shim scheduled for removal).

### US2: One range-erase edge-repair chokepoint

As a term_core maintainer, I want the range-erase edge repair collected into one
chokepoint, so that a future grid-mutating handler does not have to re-derive the
same boundary reasoning.

**Acceptance Criteria:**
- [ ] AC4: The five range-erase call sites in `csi_screen.rs` (ECH, EL 0/1, ED 0/1)
      share one edge-repair function instead of five inline copies; full-row paths
      (`clear_line`, EL 2, ED 2) are unaffected.
- [ ] AC5: The `old_width != 1` gate remains at the print call sites; no new memory
      access or branch is added to the width-1 ASCII fast path.
- [ ] AC6: The full term_core test suite and workspace tests pass.

## Technical Requirements

### Functional Requirements

- **FR1 — Single partner-blanking primitive on the cell-mutation base layer:** One
  `pub(crate)` primitive (suggested name `blank_wide_pair_half(col, row)`) lives in
  the grid-cell mutation base module (`terminal_cells.rs`), replacing both
  `blank_wide_pair_split` (`csi_edit.rs:161`) and `blank_wide_pair_partner`
  (`print_handler.rs:74`). It blanks a wide-pair half to a width-1 space, preserves
  fg/bg/flags/hyperlink, removes the overflow-table entry and its reverse-index
  entry, and marks the row dirty. (status: resolved)
- **FR2 — Self-guarding precondition inside the primitive:** The unified primitive
  itself checks that the target cell is currently width 0 or width 2 (and that
  `(col, row)` resolves to a valid index) and is a no-op otherwise — the semantics
  `blank_wide_pair_split` already has. Caller-side width checks remain only as
  call-avoidance optimizations, never as correctness requirements. (status: resolved)
- **FR3 — print path and CSI paths converge on the primitive:** `print_handler.rs`'s
  `blank_wide_pair_partner` is deleted; `blank_orphaned_neighbor_before_overwrite`
  (`print_handler.rs:105`) and `blank_orphaned_base_before_placeholder`
  (`print_handler.rs:129`) call the unified primitive. `csi_edit.rs`'s ICH/DCH
  repairs and `csi_screen.rs`'s ED/EL/ECH repairs call the same primitive (directly
  or via FR4's chokepoint). (status: resolved)
- **FR4 — Range-erase edge repair chokepoint:** The identical capture-then-repair
  pattern (`start_is_spacer` / `last_is_base` before `clear_line_range`, blank calls
  after) is extracted into one shared function (suggested shape:
  `repair_range_edges(row, start, end, start_was_spacer, last_was_base)` or an
  erase-range wrapper) and ALL five existing range-erase call sites —
  `handle_erase_characters`, `handle_erase_in_line` modes 0/1,
  `handle_erase_in_display` modes 0/1 in `csi_screen.rs` — migrate onto it.
  (status: resolved)
- **FR5 — Full-row erase paths gain no partner behavior:** `clear_line_range` and
  `clear_line` themselves remain invariant-unaware; the full-row callers
  (`clear_line`, EL 2, ED 2) acquire no partner-blanking behavior. This constraint is
  already documented in the `handle_erase_characters` comment
  (`csi_screen.rs:140-146`) and must survive the refactor. (status: resolved)

### Non-Functional Requirements

- **NFR1 - Performance (ASCII fast-path performance unchanged):** The
  `old_width != 1` gate stays at the call sites (`handle_print_ascii` at
  `print_handler.rs:249-252`, `write_grapheme_to_grid` at `:174`,
  `relocate_widened_base_via_wrap` at `:477`); the width-1 common case performs no
  work beyond the single width read already on the touched cache line.
- **NFR2 - Maintainability (behavior-preserving refactor):** All existing term_core
  tests and the workspace tests pass without modification to their assertions.
- **NFR3 - Compatibility (no public API change):** Visibility changes stay within the
  crate (`pub(crate)`); term_core's public surface (`lib.rs` re-exports) is unchanged.

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────────────┐
│  CSI handlers                    print handler          │
│  csi_screen.rs   csi_edit.rs     print_handler.rs       │
│  (ECH/EL/ED)     (ICH/DCH)       (grapheme write)       │
├─────────────────────────────────────────────────────────┤
│  FR4 chokepoint: repair_range_edges(...)                │
│  (range-erase edge repair only)                         │
├─────────────────────────────────────────────────────────┤
│  FR1 primitive: blank_wide_pair_half(col, row)          │
│  terminal_cells.rs — grid-cell mutation base layer      │
├─────────────────────────────────────────────────────────┤
│  Grid cells + overflow table + reverse index            │
└─────────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
print_handler.rs
  blank_orphaned_neighbor_before_overwrite ─┐
  blank_orphaned_base_before_placeholder   ─┤
                                            │
csi_edit.rs                                 ├─► blank_wide_pair_half (FR1, FR2)
  ICH repair                               ─┤        terminal_cells.rs
  DCH repair                               ─┤
                                            │
csi_screen.rs                               │
  handle_erase_characters        (ECH)     ─┐
  handle_erase_in_line   mode 0            ─┤
  handle_erase_in_line   mode 1            ─┼─► repair_range_edges (FR4) ─┘
  handle_erase_in_display mode 0           ─┤
  handle_erase_in_display mode 1           ─┘

  handle_erase_in_line   mode 2 (EL 2)     ─┐
  handle_erase_in_display mode 2 (ED 2)    ─┼─► clear_line / clear_line_range
  clear_line callers                       ─┘    (invariant-unaware, FR5)
```

Whether ICH/DCH call the primitive directly or through the FR4 chokepoint is an
implementation-planning decision; their edge conditions differ from the range-erase
pattern and need not be forced into `repair_range_edges`.

### Data Flow

**Partner blanking (FR1 / FR2):**

```
caller (print or CSI)
  → [caller-side old_width != 1 gate — optimization only, NFR1]
  → blank_wide_pair_half(col, row)
      → resolve (col, row) to an index      → invalid  ⇒ no-op
      → read current width                  → width 1  ⇒ no-op
      → width 0 or 2:
          write width-1 space, keep fg/bg/flags/hyperlink
          remove overflow-table entry
          remove reverse-index entry
          mark row dirty
```

**Range erase (FR4 / FR5):**

```
ECH / EL 0 / EL 1 / ED 0 / ED 1
  → capture start_was_spacer / last_was_base
  → clear_line_range(row, start, end)        (invariant-unaware)
  → repair_range_edges(row, start, end, start_was_spacer, last_was_base)
      → blank_wide_pair_half(...) for each broken edge

clear_line / EL 2 / ED 2
  → clear_line(row)                          (plain BCE fill, no repair)
```

### API Design

No external API. The change adds one crate-internal primitive and one crate-internal
shared function; term_core's public surface (`lib.rs` re-exports) is unchanged (NFR3).

| Item | Visibility | Shape (suggested) |
|------|------------|-------------------|
| Partner-blanking primitive (FR1) | `pub(crate)` | `blank_wide_pair_half(col, row)` in `terminal_cells.rs` |
| Range-erase edge repair (FR4) | crate-internal | `repair_range_edges(row, start, end, start_was_spacer, last_was_base)`, or an erase-range wrapper |

### Database Schema

Not applicable — no persistent storage is involved. The mutated state is in-memory:
grid cells, the overflow table, and the overflow reverse index.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` `terminal_cells.rs`: hosts the unified primitive (FR1).
- `crates/term_core` `print_handler.rs`: loses `blank_wide_pair_partner`, calls the
  primitive from `blank_orphaned_neighbor_before_overwrite` and
  `blank_orphaned_base_before_placeholder` (FR3), keeps the `old_width != 1` gates (NFR1).
- `crates/term_core` `csi_edit.rs`: loses `blank_wide_pair_split` as an independent
  implementation; ICH/DCH repairs call the primitive (FR3).
- `crates/term_core` `csi_screen.rs`: five range-erase call sites migrate onto the
  FR4 chokepoint; full-row paths stay unchanged (FR4, FR5).
- el-ed-wide-pair-cleanup: already landed; this feature builds on top of it.

**External Dependencies:**
- None. No new crate dependency is introduced.

### File Structure

```
crates/term_core/src/
├── terminal_cells.rs      # FR1/FR2: blank_wide_pair_half (pub(crate))
├── print_handler.rs       # FR3: blank_wide_pair_partner deleted; callers migrated
│                          # NFR1: old_width != 1 gates retained (:174, :249-252, :477)
├── csi_edit.rs            # FR3: blank_wide_pair_split removed (or thin shim); ICH/DCH migrated
└── csi_screen.rs          # FR4: repair_range_edges shared by ECH, EL 0/1, ED 0/1
                           # FR5: clear_line / clear_line_range / EL 2 / ED 2 unchanged
```

## Test Scenarios

### Unit Tests
- [ ] TS1 (FR1, FR3, FR4, NFR2): All existing wide-pair cleanup tests in
      `csi_edit.rs`, `csi_screen.rs`, and the print-handler tests pass unchanged after
      the unification.
- [ ] TS2 (FR1): A parity test (or the existing attribute-preservation tests) confirms
      the unified primitive preserves fg/bg/flags/hyperlink and removes overflow +
      reverse-index entries.
- [ ] TS4 (FR5): Full-row erases (EL 2, ED 2) over rows containing wide pairs still
      produce plain BCE fill with no attribute-preserving blank.

### Integration Tests
- [ ] TS1 (FR1, FR3, FR4, NFR2): The full term_core suite and the workspace tests pass
      with no assertion changes (AC6).

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Not applicable — this is a crate-internal refactor with no user-visible surface.

### Edge Cases
- [ ] TS3 (FR2): A test confirms the primitive is a no-op on a width-1 cell and on an
      out-of-bounds column (the self-guard).

### Performance Tests
- [ ] TS5 (NFR1): The opt-in bench (`snapshot_replay_bench_2mib_seq`) remains available
      as a manual performance spot-check for NFR1; no automated perf assertion is added.

## Security Considerations

Not applicable. This is a crate-internal refactor of `term_core` grid mutation code
with no authentication, authorization, network, storage, or user-input surface of its
own. No security-related requirement was produced for this feature.

## Error Handling

The unified primitive has no error return: violated preconditions are no-ops, not
errors (FR2).

| Condition | Behavior |
|-----------|----------|
| Target cell width is 1 | No-op |
| `(col, row)` does not resolve to a valid index | No-op |
| Target cell width is 0 or 2 | Blank to width-1 space, preserve attributes, drop overflow + reverse-index entries, mark row dirty |

### Error Flow

```
call → self-guard (width 0/2 + valid index) → not satisfied ⇒ return without mutation
                                            → satisfied     ⇒ perform blanking
```

## Performance Optimization

### Performance Goals
- Width-1 ASCII fast path: no work beyond the single width read already on the touched
  cache line; no new memory access and no new branch (NFR1, AC5).

### Optimization Strategies
- Caller-side gating: keep the `old_width != 1` gate at `handle_print_ascii`
  (`print_handler.rs:249-252`), `write_grapheme_to_grid` (`:174`) and
  `relocate_widened_base_via_wrap` (`:477`) as a call-avoidance optimization, while
  the primitive's own self-guard carries correctness (FR2 + NFR1).

### Caching Strategy
Not applicable — no cache is introduced or changed.

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented and tested
- [ ] All test scenarios (TS1–TS5) pass
- [ ] AC1: Exactly one partner-blanking primitive exists in the grid-cell mutation base
      layer, with the width-0/2 self-guard inside it
- [ ] AC2: `print_handler.rs` no longer defines `blank_wide_pair_partner`; print and CSI
      edit/erase paths call the same primitive
- [ ] AC3: `csi_edit.rs` no longer defines `blank_wide_pair_split` as an independent
      implementation
- [ ] AC4: The five range-erase call sites share one edge-repair function; full-row paths
      are unaffected
- [ ] AC5: The `old_width != 1` gate remains at the print call sites; no new memory access
      or branch on the width-1 ASCII fast path (NFR1)
- [ ] AC6: The full term_core test suite and workspace tests pass (NFR2)
- [ ] NFR3: term_core's public surface (`lib.rs` re-exports) is unchanged
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. All requirements (FR1–FR5, NFR1–NFR3) are `resolved`.

## Assumptions

Carried over from the requirements analysis:

- The el-ed-wide-pair-cleanup feature is already merged into this worktree (verified in
  `csi_screen.rs`), so FR4 migrates five existing call sites rather than two; the task
  description's line reference `csi_screen.rs:65` is stale but the named functions all
  exist as described.
- The two existing implementations are output-equivalent (per finding b8a62feaf016ef08);
  the unified primitive adopts `blank_wide_pair_split`'s self-guarding contract.
- The "785 term_core tests" count is taken from the task description and was not verified
  by execution; the binding requirement is that the current in-tree suite passes.
- `feature-docs/wide-pair-overwrite-cleanup/` is NOT present in the integration worktree;
  requirements were validated directly against the source files.
- Whether ICH/DCH call the primitive directly or through the FR4 chokepoint is an
  implementation-planning decision; their edge conditions differ from the range-erase
  pattern and need not be forced into `repair_range_edges`.
- The EL/ED sibling task has already landed, so this feature builds on top of it.

## Implementation Phases (if applicable)

### Phase 1: Unify the partner-blanking primitive
**Goals:** FR1, FR2, FR3, NFR1, NFR3
**Deliverables:**
- `blank_wide_pair_half(col, row)` in `terminal_cells.rs` with the width-0/2 self-guard
- `blank_wide_pair_partner` deleted from `print_handler.rs`; its two callers migrated
- `blank_wide_pair_split` removed from `csi_edit.rs` (or reduced to a thin forwarding shim);
  ICH/DCH repairs migrated
- `old_width != 1` gates retained at the three print call sites

### Phase 2: Extract the range-erase edge-repair chokepoint
**Goals:** FR4, FR5, NFR2
**Deliverables:**
- One shared edge-repair function in `csi_screen.rs`
- ECH, EL 0, EL 1, ED 0, ED 1 migrated onto it
- `clear_line` / `clear_line_range` / EL 2 / ED 2 left invariant-unaware, with the
  existing `csi_screen.rs:140-146` comment preserved
- Full term_core and workspace test suites green with unmodified assertions

## References

- REQUIREMENTS.md: `feature-docs/wide-pair-blank-primitive-unification/REQUIREMENTS.md`
- Review finding b8a62feaf016ef08: duplicated wide-pair partner-blanking implementations
- Review finding 931abe859e23fa5d: range-erase edge repair copy-pasted at five call sites
- el-ed-wide-pair-cleanup: the sibling feature this one builds on top of
