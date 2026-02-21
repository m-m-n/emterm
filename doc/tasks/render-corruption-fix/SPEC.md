# Feature: Fix Rendering Corruption During Rapid Terminal Output

## Overview

eMterm exhibits display corruption when running applications that produce rapid, high-volume terminal output with frequent cursor movement and line updates, such as Claude Code. The corruption manifests as split words, duplicated characters, and misaligned text. Switching tmux windows (triggering a full screen redraw) restores correct display, indicating the differential rendering path produces incorrect output while the full redraw path works correctly.

## Objectives

- Eliminate display corruption during rapid terminal output (e.g., Claude Code usage)
- Ensure the differential rendering path (`render()`) produces identical visual results to the full redraw path (`forceRender()`)

## User Stories

### US1: Correct Display During Claude Code Usage
As an eMterm user, I want the terminal to display all text correctly when using Claude Code or similar high-throughput applications, so that I can read the output without visual artifacts.

**Acceptance Criteria:**
- [ ] Running Claude Code for extended sessions (10+ minutes) produces no visible display corruption
- [ ] Status line updates (spinner, progress) render without split words or duplicated characters
- [ ] Emoji characters render at correct positions during streaming output
- [ ] No manual intervention (e.g., tmux window switch) is needed to fix display

## Technical Requirements

### Functional Requirements
- **FR1:** The differential rendering path (`render()`) must produce pixel-identical output to `forceRender()` for any given terminal grid state
- **FR2:** All rendering paths must correctly handle rapid sequential updates within a single animation frame

### Non-Functional Requirements
- **NFR1 - Performance:** The fix must not degrade rendering performance to the point where frame drops are noticeable during normal usage
- **NFR2 - Compatibility:** The fix must work correctly across different `devicePixelRatio` values (1x, 1.5x, 2x)

## Investigation Notes

The following hypotheses were tested during investigation. None have been confirmed as the sole root cause.

### Hypothesis 1: Dirty bit misalignment after scroll optimization (Applied, not confirmed)
- **Theory:** When `scroll_up_internal(1)` uses the scroll_event optimization, pre-existing dirty bits are not shifted to match the viewport mapping change after `ring_push_blank()`
- **Fix applied:** Added `shift_dirty_down_by_one()` in `wasm/src/ring_buffer.rs`
- **Result:** Did not resolve the corruption

### Hypothesis 2: Fractional charHeight causing pixel misalignment (Applied, not confirmed)
- **Theory:** `charHeight` (e.g., 19.3px) being fractional causes `drawImage` canvas shifts to misalign with `Math.floor`-based row drawing positions
- **Fix applied:** `Math.ceil(ascent + descent)` in `canvas-renderer.ts` and `size.ts`
- **Result:** Did not resolve the corruption

### Hypothesis 3: Scroll optimization entirely (Tested, eliminated)
- **Theory:** The `drawImage` canvas self-copy scroll optimization is fundamentally broken
- **Test:** Completely disabled scroll optimization (always `mark_all_dirty()`)
- **Result:** Corruption still occurs. **Scroll optimization is not the cause.**

### Hypothesis 4: Single-pass vs two-pass rendering (Applied, not confirmed)
- **Theory:** `render()` draws each row sequentially (background + text per row), while `forceRender()` uses two passes (all backgrounds, then all text). Row N+1's background could overwrite row N's text descenders in single-pass mode.
- **Fix applied:** Changed `render()` to use two-pass rendering
- **Result:** Did not resolve the corruption on its own

### Hypothesis 5: Differential rendering path entirely (Tested, reverted)
- **Theory:** The differential rendering path in `render()` (drawing only dirty rows) has a fundamental issue compared to `forceRender()` (drawing all rows with full canvas clear). Exhaustive audit confirmed all dirty marking is correct across 28 cell-modification code paths, and all data sources are identical between the two paths.
- **Fix applied:** `render()` delegates to `forceRender()` (later reverted - corruption persists in forceRender too)
- **Result:** Did not resolve the corruption. **Confirmed: bug is in grid data/parser pipeline, not rendering.**

### Hypothesis 6: Grapheme buffer not flushed before non-Print actions (Applied)
- **Theory:** When emoji/Extended_Pictographic codepoints are buffered in the grapheme accumulator, non-Print actions (cursor movement, C0 controls, ESC sequences) are dispatched without first flushing the buffer. The buffered emoji is written to the grid at the WRONG cursor position (after the movement) when the next Print action triggers a flush. This causes cursor position drift that cascades into ASCII text misalignment.
- **Fix applied:** Added `flush_grapheme_buffer()` calls before all non-Print action arms in `dispatch_action()` (`wasm/src/terminal_core.rs`)
- **Result:** Pending user testing. 4 new tests verify correct flush timing.

### Hypothesis 7: DEC mode 1048h/l cursor save/restore timing and dual-slot bug (Applied)
- **Theory:** Two related bugs in cursor save/restore for DEC private mode 1048:
  1. **Timing bug:** 1048h/l queued a mode action for deferred TS-side processing. WASM continued processing remaining data in the chunk before the save/restore executed, causing the cursor to be saved at the FINAL position (after all data), not at the position when the 1048h sequence appeared.
  2. **Dual-slot bug:** ESC 7/8 used WASM's `saved_cursor` while 1048h/l used a separate TS-side `cursor.saved`, creating two independent saved cursor states. If an application mixed ESC 7 save with 1048l restore (or vice versa), it would restore from the wrong saved state.
- **Fix applied:** 1048h/l now calls WASM's `save_cursor()`/`restore_cursor()` immediately (same as ESC 7/8), sharing the same saved cursor slot. No mode action is queued. (`wasm/src/csi_modes.rs`)
- **Result:** Pending user testing. 2 new tests verify immediate execution and slot sharing.

### Key Observation
- `forceRender()` also shows corruption, confirming the bug is in grid data, not rendering
- Scroll optimization was disabled (always mark_all_dirty) to rule it out
- Grapheme buffer and cursor save/restore timing are the two identified data-level issues

## Architecture Context

```
PTY (Rust) → Binary Channel → PtyClient (TS) → process_pty_data (WASM) → callbacks (TS)
    → scheduleRender → requestAnimationFrame → render() → Canvas 2D
```

### Key Files
- `src/terminal/canvas-renderer.ts` - Canvas rendering (`render()` and `forceRender()`)
- `wasm/src/ring_buffer.rs` - Scroll and dirty bit logic
- `wasm/src/terminal_core.rs` - Grid state and dirty tracking
- `src/pty/size.ts` - Character size measurement

## Test Scenarios

### Manual Tests
- [ ] Run Claude Code in eMterm for 10+ minutes with active interaction
- [ ] Observe status line during Claude Code thinking/streaming phases
- [ ] Verify emoji rendering in wasm-pack build output
- [ ] Verify no corruption occurs without needing tmux window switch

### Unit Tests
- [ ] `shift_dirty_down_by_one` correctly shifts dirty bits across word boundaries
- [ ] `charHeight` is always an integer value after measurement
- [ ] Differential render produces same visual output as full render for a known grid state

## Success Criteria

- [ ] Claude Code can be used in eMterm without any visible display corruption
- [ ] No regression in rendering performance (frame time stays within acceptable range)
- [ ] All existing tests continue to pass
