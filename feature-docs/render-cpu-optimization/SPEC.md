# Feature: GUI Render CPU Optimization

## Overview

Reduce emterm GUI CPU usage (currently 10–15% idle / ~80% during PTY output) by
fixing three confirmed root causes identified in the 2026-07-05 investigation
(`tmp/cpu-usage-investigation-2026-07-05.md`): a dead dirty-row skip path, full
per-frame cell reprocessing, and a permanent 60Hz polling loop.

## Objectives

- Make the dirty-row set honest so the existing "0 dirty rows → skip draw"
  early return actually fires on unchanged frames.
- Drive rendering by dirty rows with a per-row `CellInstance` cache and a
  persistent instance buffer, eliminating full-grid reshaping and per-frame
  buffer creation.
- Replace the always-rearmed 16ms `ControlFlow::WaitUntil` with true
  `ControlFlow::Wait` plus explicit wakeups.
- Add env-gated performance counters for measurement and regression detection.

## Technical Requirements

### Functional Requirements

- **FR1 (Stage 1 — honest dirty set):** Remove the unconditional cursor-row
  `push_unique` in `dirty_rows_this_frame` (`src-tauri/src/app.rs:3749`).
  Keep conditional pushes: cursor movement (old row + new row) and blink phase
  flips (only when blink is enabled). On an unchanged frame the dirty set MUST
  be empty, allowing the early return at `src-tauri/src/window_host.rs:1391`
  to skip drawing.
- **FR2 (block cursor as overlay):** Move block-cursor drawing from the grid
  instance bake-in (`src-tauri/src/render/mod.rs:659,713` fg/bg inversion) to
  the egui overlay layer (`draw_cursor`, `src-tauri/src/render/mod.rs:275`),
  implemented as a cell-rect fill plus a single glyph redrawn in inverted
  color. Correct rectangles for normal cells, wide (2-cell) characters, emoji,
  and empty cells; no cursor drawn when scrolled back and the cursor is
  off-screen; no broken rect on fold rows or at screen edges.
- **FR3 (Stage 2 — dirty-row-driven rendering):** Cache per-row
  `Vec<CellInstance>` on the CPU side. Reshape/rebuild only dirty rows;
  concatenate cached rows otherwise. Invalidation rules: scroll → all rows
  dirty; resize / font change → full cache drop; selection / hover / search
  highlight change → affected rows dirty. The instance sequence produced via
  the row cache MUST be byte-identical to a full rebuild.
- **FR4 (persistent instance buffer):** Replace the per-frame
  `device.create_buffer` (`src-tauri/src/render/terminal_grid_pass.rs:962-977`)
  with a persistent buffer, grown on demand, updated via `queue.write_buffer`.
  The growth policy MUST be a pure function with unit tests.
- **FR5 (wait-driven event loop):** Stop unconditionally rearming
  `ControlFlow::WaitUntil(now + 16ms)` at the end of `about_to_wait`
  (`src-tauri/src/window_host.rs:2321,3229`). When nothing is pending, enter
  true `ControlFlow::Wait`; wake explicitly on PTY output, user input, IME,
  and the blink timer (blink-period wakeups are acceptable while blink is
  enabled and the window is focused).
- **FR6 (performance counters):** Behind `EMTERM_RENDER_PERF=1` (same style as
  `EMTERM_FONT_PERF`), log "frames drawn" and "rows rebuilt" counters at warn
  level. Zero measurable overhead when the variable is unset.

### Non-Functional Requirements

- **NFR1 - Performance:** Idle and output-time CPU usage measurably lower than
  before the change, verified by `/proc` utime+stime sampling (procedure in
  investigation report §6). No absolute numeric target; before/after reduction
  is the pass criterion.
- **NFR2 - Visual fidelity:** No rendering regressions: cursor 3 shapes ×
  blink on/off, cursor over CJK/emoji, selection / search / hover highlights,
  TUI apps (vim, Claude Code) ghosting, display after mux tab switch. The
  known limitation (selection highlight残り on TUI row rewrite) must not get
  worse.
- **NFR3 - Design provenance:** Before implementation, validate the approach
  against Alacritty (damage tracking) and WezTerm (shaped line cache) official
  docs / issues / changelogs. No code reuse from either project; implementation
  written independently.
- **NFR4 - Build integrity:** GUI-only code stays behind `#[cfg(feature =
  "gui")]`; the `--no-default-features` (CLI-only) build keeps compiling.

## Implementation Approach

### Affected Code (from investigation)

| Area | Location | Change |
| --- | --- | --- |
| Dirty set construction | `src-tauri/src/app.rs:3728-3750` | drop unconditional cursor push; keep conditional pushes |
| Draw skip early return | `src-tauri/src/window_host.rs:1391` | becomes reachable; extract skip decision into a pure function |
| Block cursor bake-in | `src-tauri/src/render/mod.rs:614,659,713` | remove inversion from instance build |
| Cursor overlay | `src-tauri/src/render/mod.rs:275` (`draw_cursor`), `src-tauri/src/render/cursor.rs` | add block-shape overlay path |
| Cell shaping / instance build | `src-tauri/src/render/terminal_grid_pass.rs:630`, `src-tauri/src/render/mod.rs:614` | per-row cache, dirty-row rebuild |
| Instance buffer | `src-tauri/src/render/terminal_grid_pass.rs:962-977` | persistent buffer + `write_buffer` |
| Event loop control flow | `src-tauri/src/window_host.rs:2321,3229` | `Wait` + explicit wakeups |
| Perf counters | render pipeline + `logging` conventions | env-gated warn-level counters |

### Architecture Notes

- Two-layer rendering is preserved: wgpu instanced grid (1 draw call) below,
  egui overlay (cursor / IME / selection) above. FR2 moves the block cursor to
  the overlay layer, making the grid instance data independent of cursor
  position and blink phase — the precondition for FR1's honest dirty set.
- FR3's row cache subsumes a separate shaped-run cache (investigation §4(b));
  no independent shaped-run cache is built.
- FR5 is independent of FR1–FR4 and removes the idle CPU floor formed by the
  62/s `pump_ime` + `pump_all` polling.
- Out of scope (confirmed innocent): mux daemon/attach, GPU present, glyph
  atlas upload (already dirty-tracked), PTY input coalescing, resize memsets.

### Dependencies

**Internal:** `crates/term_core` (grid state, untouched), existing
`collect_cell_inputs` unit tests (12, `src-tauri/src/render/mod.rs:1698+`)
guard that boundary.

**External:** no new crates expected (winit / wgpu / egui / swash already in
use).

## Test Scenarios

### Unit Tests (`--lib`, wgpu-free CPU logic only)

- [ ] TS-1: Dirty-set property tests (`app.rs`): unchanged frame → empty set
  (write red first); cursor move → old+new rows; blink enabled → cursor row
  only on phase-flip frames; blink disabled → no phase-driven push; PTY output
  → only output rows.
- [ ] TS-2: Cursor overlay geometry tests (extend `render/cursor.rs` tests):
  normal / wide / emoji / empty cells; off-screen cursor during scrollback not
  drawn; fold rows and screen edges intact.
- [ ] TS-3: Draw-skip decision extracted as a pure function: "0 dirty rows and
  status bar unchanged → skip" tested directly.
- [ ] TS-4: Equivalence tests (Stage 2 safety net): mutation scenarios (write
  chars, scroll, selection change, resize) produce `CellInstance` sequences
  via row cache identical to full rebuild.
- [ ] TS-5: Invalidation trigger matrix: selection, hover, search, scroll,
  resize, font/theme change, focus gain/loss — each mutated singly, expected
  invalidation occurs.
- [ ] TS-6: Persistent-buffer growth policy pure-function tests.

### Performance Verification (final verify)

- [ ] TS-7: `EMTERM_RENDER_PERF=1` counters emit frames-drawn / rows-rebuilt
  at warn level.
- [ ] TS-8: Idle 10s → frame count on the order of blink count; `seq` bulk
  output → rebuilt-row count on the order of output lines; before/after CPU%
  compared via `/proc` sampling (report §6 procedure).

### E2E / Visual (final verify only, not in TDD loop)

**Existing E2E tests**: None (no dedicated E2E suite; manual verification per
project convention)
**Run command**: Not detected
- [ ] TS-9: Cursor 3 shapes × blink on/off; cursor over CJK and emoji;
  selection / search / hover; TUI (vim / Claude Code) ghosting; display after
  mux tab switch.
- [ ] TS-10: Known limitation (selection highlight residue on TUI row rewrite)
  not worsened.

### Edge Cases

- [ ] Wide character straddling the cursor cell (block cursor rect covers both
  cells).
- [ ] Cursor at the last column / last row (no out-of-bounds rect).
- [ ] Blink disabled entirely (no periodic wakeups from blink in FR5's wait
  loop).

## Error Handling

No new user-facing error paths. Buffer growth failure follows existing wgpu
allocation error handling. Perf counters are logging-only.

## Performance Optimization

### Goals

- Idle CPU%: below the current 10–15% (reduction confirmed by sampling).
- PTY output CPU%: below the current ~80%.
- Idle draw frames ≈ blink flips only; unchanged frames skip drawing entirely.

### Strategies

- Honest dirty tracking → draw skip on unchanged frames (FR1/FR2).
- Per-row instance cache → rebuild cost proportional to changed rows (FR3).
- Persistent instance buffer → no per-frame allocation/memset (FR4).
- Event-driven wakeups → no idle polling floor (FR5).

## Success Criteria

- [ ] All functional requirements (FR1–FR6) implemented and unit-tested.
- [ ] TS-1 … TS-8 pass; TS-9/TS-10 confirmed at final verify.
- [ ] Before/after CPU reduction confirmed for idle and output scenarios.
- [ ] `--no-default-features` build still compiles.
- [ ] No visual regressions per NFR2.

## Open Questions

None — all requirements confirmed with the user (scope: full Stage 1 + Stage 2
+ wait-driven loop; pass criterion: relative reduction; perf counters:
included).

## Implementation Phases

### Phase 1: Stage 1 (FR1 + FR2 + FR6 groundwork)
**Goals:** Empty dirty set on unchanged frames; draw skip fires.
**Deliverables:** cursor overlay path, honest dirty set, skip decision pure
function, perf counters.

### Phase 2: Stage 2 (FR3 + FR4)
**Goals:** Dirty-row-driven instance rebuild; persistent buffer.
**Deliverables:** row cache with invalidation rules, equivalence test suite,
buffer growth policy.

### Phase 3: Wait-driven loop (FR5)
**Goals:** No idle polling; event/timer-driven wakeups.
**Deliverables:** `ControlFlow::Wait` path with explicit wakeup sources.

## References

- Investigation report: `tmp/cpu-usage-investigation-2026-07-05.md`
- Requirements: `feature-docs/render-cpu-optimization/REQUIREMENTS.md`
- Benchmark reference policy: Alacritty damage tracking / WezTerm shaped line
  cache — official docs and issues only, no code reuse.
