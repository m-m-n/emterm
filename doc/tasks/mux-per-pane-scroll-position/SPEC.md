# Feature: Per-Pane Scroll Position in mux

## Overview

In mux mode, scrolling up in one pane (mux window) and then switching to another pane causes the other pane to be displayed at the same scroll position. The scroll position is currently shared across panes. This feature makes the scroll position per-pane, so switching panes saves and restores each pane's own scroll position.

## Objectives

- Save and restore scroll position per mux pane on pane switch
- Restore the previous scroll position when returning to a pane (not reset to bottom)
- Keep background-pane output behavior consistent with the existing scroll-pin logic
- No regression in non-mux tab scroll-position isolation

## User Stories

### US1: Preserve scroll position when switching panes
As a mux user, I want each pane to remember its own scroll position, so that switching panes does not lose my place in the scrollback.

**Acceptance Criteria:**
- [ ] After scrolling up in pane A and switching to pane B, pane B shows its own scroll position
- [ ] Returning to pane A restores pane A's previous scroll position

### US2: Background output respects scroll-pin
As a mux user, I want a background pane that receives new output to follow the same scroll-pin rule as the active pane, so that scrolled-up panes keep their position and bottom-pinned panes follow new output.

**Acceptance Criteria:**
- [ ] A scrolled-up background pane keeps its position when it receives output
- [ ] A bottom-pinned background pane follows new output

## Technical Requirements

### Functional Requirements
- **FR1:** On mux pane switch, persist the outgoing pane's scroll position and restore the incoming pane's saved scroll position, so each pane retains its own scroll offset.
- **FR2:** When a background (inactive) pane receives output, apply the existing scroll-pin behavior to that pane's saved scroll position (same rule as the active pane).

### Non-Functional Requirements
- **NFR1 - Performance:** Saving/restoring the scroll offset is a single numeric value on the pane-switch path; performance impact is negligible.
- **NFR2 - No regression:** Non-mux tabs keep their existing independent renderer/scrollOffset behavior unchanged.

## Implementation Approach

### Architecture

**Current behavior (root cause):**

- Each non-mux tab owns an independent `TerminalApp` → independent `CanvasRenderer` → independent `scrollOffset`. Scroll positions are already isolated.
- In mux mode, a single `TerminalApp`/`CanvasRenderer` hosts multiple mux panes. On pane switch, `switchMuxWindow()` swaps the WASM grid and `TerminalState` via `saveMuxPaneState()` / `restoreMuxPaneState()`, **but `CanvasRenderer.scrollOffset` is not part of the saved/restored pane state.** The previous pane's `scrollOffset` is therefore applied to the newly restored grid, making panes appear to share scroll position.

**Fix direction:**

- Include the renderer's scroll offset in the per-pane saved state, so it is saved on switch-out and restored on switch-in.

### Key Locations

| Purpose | File | Reference |
|---------|------|-----------|
| Scroll offset storage (TS) | `src/terminal/canvas-renderer.ts` | `scrollOffset`, `getScrollOffset()` / `setScrollOffset()` |
| Mux pane switch | `src/terminal-app/mux/mux-window-manager.ts` | `switchMuxWindow()` |
| Per-pane grid save/restore | `src/terminal/state-mux-pane.ts` | `saveMuxPaneState()` / `restoreMuxPaneState()` |
| Mux pane state shape | `src/terminal/state-mux-pane.ts` | `MuxPaneGridState` |
| Scroll-pin correction | `src/terminal/scroll-pin.ts` | `computeAdjustedScrollOffset()` |

### Data Flow

```
switchMuxWindow(target)
  → save outgoing pane: MuxPaneGridState { grid..., scrollOffset = renderer.getScrollOffset() }
  → restore incoming pane grid (restoreMuxPaneState)
  → renderer.setScrollOffset(incoming pane saved scrollOffset)
  → renderer.forceRender()
```

For FR2, a background pane receiving output applies the existing scroll-pin correction (`computeAdjustedScrollOffset`) to its *saved* scroll offset, matching how the active pane's offset is corrected on scrollback growth.

### Dependencies

**Internal Dependencies:**
- mux pane state management (`state-mux-pane.ts`, `MuxPaneGridState`)
- `CanvasRenderer` scroll offset API
- existing scroll-pin logic (`scroll-pin.ts`)

**External Dependencies:**
- None

## Test Scenarios

### Unit Tests
- [ ] Saving a pane's state captures the current renderer scroll offset
- [ ] Restoring a pane's state sets the renderer scroll offset to the saved value
- [ ] A pane saved at bottom (offset 0) restores at bottom
- [ ] Background-pane output applies scroll-pin correction to the saved offset

### Integration Tests
- [ ] `switchMuxWindow()` round-trip (A → B → A) restores A's scroll offset

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (WebdriverIO + tauri-driver, Docker)
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] Scenario: scroll up in pane A, switch to pane B (shows B's position), return to A (A's position restored)

### Edge Cases
- [ ] All panes at bottom → switching does not introduce scroll
- [ ] Switching to a pane whose scrollback grew while it was in the background keeps the visible content consistent with scroll-pin

## Success Criteria

- [ ] FR1, FR2 implemented and tested
- [ ] Pane switch restores per-pane scroll position
- [ ] Non-mux tab scroll isolation unaffected (regression check)
- [ ] All test scenarios pass

## References

- 要件定義書: `doc/tasks/mux-per-pane-scroll-position/要件定義書.md`
- Related: `doc/tasks/pin-viewport-when-scrolled-up/`, `doc/tasks/mux-scrollback-retention/`
