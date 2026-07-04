# Feature: selection-anchor-pivot

## Overview

Fix word-mode (double-click) and line-mode (triple-click) drag selection so
the originally clicked word / line stays part of the selection for the whole
drag. Today `Selection::extend` destructively rewrites `anchor` / `extent`
with boundary-snapped positions on every call, so from the second motion
event onward the original word / line is lost and the selection start jumps
to the word under the pointer.

## Objectives

- Keep the double-clicked word (or triple-clicked line) as an immutable pivot
  for the entire drag.
- Preserve the F9 absolute-row scroll-following model, including eviction
  compensation.
- Leave Character-mode selection behavior untouched.

## User Stories

### US1: Word-selection drag keeps the origin word
As a terminal user, I want a double-click-then-drag selection to always
include the word I double-clicked, so that the selection start does not jump
to a word on another line.

**Acceptance Criteria:**
- [ ] Double-click on a word, drag to a word on the line above: selection is
  [start of word under pointer .. end of origin word].
- [ ] Drag back inside the origin word: selection is exactly the origin word.
- [ ] Drag to a word below / after the origin word: selection is
  [start of origin word .. end of word under pointer].
- [ ] The same pivot rule applies horizontally (other words on the same row).

### US2: Line-selection drag keeps the origin line
As a terminal user, I want a triple-click-then-drag selection to always
include the line I triple-clicked, so that dragging up and back does not
shrink the selection past the origin line.

**Acceptance Criteria:**
- [ ] Triple-click a row, drag up/down repeatedly: selection is always
  [min(origin row, pointer row) .. max(origin row, pointer row)], full rows.
- [ ] Drag back to the origin row: selection is exactly the origin row.

## Technical Requirements

### Functional Requirements
- **FR1:** Word-mode pivot extension. `Selection` retains the original press
  position (`origin`, absolute row + col) unchanged for the lifetime of the
  drag. Each `extend(pos)` recomputes both endpoints from scratch: if `pos`
  precedes the origin word in reading order, the range is
  [word_start(pos) .. word_end(origin)]; if it follows, the range is
  [word_start(origin) .. word_end(pos)]; if `pos` is inside the origin word,
  the range is exactly the origin word. Word boundaries are computed against
  the live core at each extend (current behavior).
- **FR2:** Line-mode pivot extension. Same origin retention; each
  `extend(pos)` yields full rows
  [min(origin.row, pos.row) .. max(origin.row, pos.row)], col 0 through the
  last column.
- **FR3:** Absolute-row-model integration. `origin` uses the same absolute
  buffer-row coordinate as `anchor` / `extent`. `shift_rows_down` applies the
  same eviction compensation to `origin` as to the endpoints (clamp / drop
  rules unchanged). Reflow, alt-screen, and tab-switch selection clearing
  stay as-is.

### Non-Functional Requirements
- **NFR1 - Regression safety:** Character-mode selection, single-extend
  behavior covered by existing tests, `resolve()` copy output, fold
  interaction, and rendering (`contains`) are unchanged. All existing Rust
  unit tests keep passing. Per-motion-event work does not increase (word
  boundary lookup already runs on every extend today).

## Implementation Approach

### Architecture

Single-file logic change in the selection model; no event-flow changes.

```
window_host.rs (press/drag events)          selection.rs (model)
  press  → Selection::new_with_mode(pos, mode)   captures origin = pos
  motion → sel.extend(pos, &core)                recomputes anchor/extent
                                                 from origin + pos each call
```

- `Selection` gains an immutable origin position captured at construction
  (`new` / `new_with_mode`). `extend` for Word / Line modes derives
  `anchor` / `extent` from `origin` and `pos` instead of reading back its own
  previously-snapped `anchor`.
- The press-time immediate commit in `window_host.rs` (double / triple click
  with no motion yet) already calls `extend` with the press position; with
  FR1/FR2 semantics this yields exactly the origin word / line, so the
  press-site code needs no behavioral change.
- `ordered()`, `contains()`, `resolve()` continue to read
  `anchor` / `extent` and need no changes.
- `shift_rows_down` additionally shifts / clamps `origin.row` (FR3).

### Data Flow

```
Motion event → pixel_to_cell → screen_row_to_abs → Selection::extend(pos)
             → endpoints recomputed from (origin, pos) → redraw / resolve
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/selection.rs`: `Selection`, `word_boundary`, `shift_rows_down`
- `src-tauri/src/window_host.rs`: press / drag call sites (expected unchanged
  or minimally touched)

**External Dependencies:**
- None (no new crates)

### File Structure

```
src-tauri/src/
├── selection.rs      # Selection struct, extend, shift_rows_down + unit tests
└── window_host.rs    # call sites (verify only; change only if required)
```

## Test Scenarios

### Unit Tests
- [ ] TS-1: Word mode, two consecutive extends onto a word on the row above
  → range is [upper word start .. origin word end]. (FR1)
- [ ] TS-2: Word mode, extend up then back inside the origin word → range is
  exactly the origin word. (FR1)
- [ ] TS-3: Word mode, extend up then down past the origin word → range is
  [origin word start .. lower word end]. (FR1)
- [ ] TS-4: Word mode, horizontal drag to an earlier word on the same row →
  [pointer word start .. origin word end]. (FR1)
- [ ] TS-5: Line mode, repeated extends up / down / back → full rows, origin
  row always included; back on origin row → origin row only. (FR2)
- [ ] TS-6: `shift_rows_down` shifts `origin` with the endpoints; a fully
  evicted selection is dropped as today. (FR3)
- [ ] TS-7: Existing single-extend word / line / character tests keep
  passing unmodified. (NFR1)

### Integration Tests
- [ ] Not required beyond the unit level — the fix is contained in the
  selection model; press / drag wiring is exercised by existing
  `window_host.rs` tests.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario MT-1: in a running terminal, double-click a word,
  drag to the line above, confirm the selection spans from the word under
  the pointer to the origin word and the copied text matches.
- [ ] Manual scenario MT-2: triple-click a line, drag up then back down,
  confirm the origin line stays selected.

### Edge Cases
- [ ] Extend onto a whitespace / empty cell: endpoint collapses to that cell
  (existing `word_boundary` collapse rule) while the origin word edge is
  kept.
- [ ] Drag spanning scrollback rows (origin in scrollback, pointer in
  viewport or vice versa) selects across the boundary.
- [ ] Content under the drag changes between extends: boundaries are
  recomputed from the live core at each extend (no caching of the origin
  word's columns beyond the origin position itself).

### Performance Tests
- [ ] Not required: per-extend cost is unchanged (one `word_boundary` call
  per endpoint, as today).

## Security Considerations

- No new input surface. Selection text continues to flow through the
  existing `resolve()` → clipboard path (bracket-sequence sanitizing
  unchanged).

## Error Handling

- No new error states. Out-of-range positions are clamped by the existing
  `pixel_to_cell` / `screen_row_to_abs` path; `shift_rows_down` drop rules
  handle evicted selections.

## Performance Optimization

### Performance Goals
- No additional per-motion-event work compared to the current
  implementation.

## Success Criteria

- [ ] All functional requirements (FR1-FR3) are implemented and tested
- [ ] All unit test scenarios (TS-1..TS-7) pass
- [ ] NFR1 regression suite passes (full `cargo test --lib`)
- [ ] Code review is completed

## Open Questions

- None.

## References

- Requirements: feature-docs/selection-anchor-pivot/REQUIREMENTS.md
- Prior investigation: tmp/discussion-selection-behavior-fixes.md
- Absolute-row selection model (F9): src-tauri/src/selection.rs module docs
