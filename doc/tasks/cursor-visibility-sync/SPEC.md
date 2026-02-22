# Feature: DECTCEM Cursor Visibility Sync Fix

## Overview

Fix cursor visibility not responding to DECTCEM (CSI ?25h / CSI ?25l) escape sequences. When TUI applications send CSI ?25l to hide the cursor, the blinking cursor remains visible because the WASM-side mode change is not synchronized back to the TypeScript rendering layer.

## Objectives

- Fix cursor visibility toggling via CSI ?25h / CSI ?25l
- Ensure all WASM boolean mode changes are reflected in TS state after PTY data processing
- Maintain rendering performance (no unnecessary overhead per data chunk)

## User Stories

### US1: Cursor Hidden in TUI Applications
As a terminal user, I want the cursor to be hidden when TUI applications (vim, htop, fzf, Claude Code, etc.) send the hide cursor escape sequence, so that the UI matches the application's intent.

**Acceptance Criteria:**
- [ ] CSI ?25l hides the blinking cursor
- [ ] CSI ?25h restores the cursor
- [ ] Cursor visibility works correctly across alternate/primary buffer switches
- [ ] No visible cursor flicker during rapid mode changes

## Technical Requirements

### Functional Requirements
- **FR1:** After `process_pty_data()` completes, sync WASM boolean modes (including `MODE_CURSOR_VISIBLE`) to TS `TerminalModes`
- **FR2:** Cursor blink mode (ATT160 / mode 12) must also be synced correctly via the same mechanism
- **FR3:** The sync must occur after `process_pty_data()` completes, before mode action processing (to prevent `syncModesToWasm` in `setDecPrivateMode` from overwriting WASM-managed mode bits with stale TS values)

### Non-Functional Requirements
- **NFR1 - Performance:** The sync operation must not add measurable latency to PTY data processing (8 WASM boundary reads per chunk is acceptable)

## Root Cause Analysis

### Current Data Flow (Broken)
```
PTY output → WASM process_pty_data()
  → CSI ?25l parsed
  → WASM handle_set_mode(25, false) called internally
  → MODE_CURSOR_VISIBLE bit cleared in WASM bitfield
  → Returns MODE_ACTION_NONE (0)
  → mode_actions queue: nothing pushed (action == 0)

TS side after process_pty_data():
  → core.take_mode_actions() → empty for mode 25
  → state.modes.cursorVisible remains TRUE
  → Renderer reads state.cursorVisible → TRUE
  → Cursor still rendered (BUG)
```

### Fixed Data Flow
```
PTY output → WASM process_pty_data()
  → CSI ?25l parsed
  → WASM MODE_CURSOR_VISIBLE bit cleared

TS side after process_pty_data():
  → core.take_mode_actions() processed
  → syncModesFromWasm(state.modes, core) called  ← NEW
  → state.modes.cursorVisible = false (read from WASM)
  → Renderer reads state.cursorVisible → FALSE
  → Cursor hidden (CORRECT)
```

## Implementation Approach

### Fix Location

**File: `src/terminal-app/index.ts`** (setupPtyHandlers, onData callback)

After mode actions are processed and before render scheduling, add:
```typescript
// Sync boolean modes from WASM to TS (cursor visible, blink, autowrap, etc.)
this.state.syncModesFromWasm();
```

**File: `src/terminal/state.ts`** (add public method if not exists)

Expose `syncModesFromWasm()` as a public method that reads from the active core.

### Affected Files

| File | Change |
|------|--------|
| `src/terminal-app/index.ts` | Add `syncModesFromWasm()` call after mode action processing |
| `src/terminal/state.ts` | Expose public `syncModesFromWasm()` method (if needed) |

### Dependencies

**Internal Dependencies:**
- `src/terminal/modes.ts`: `syncModesFromWasm()` function (already exists)
- `wasm/src/csi_modes.rs`: Mode 25 handler (no changes needed)
- `wasm/src/terminal_core.rs`: `get_mode()` API (no changes needed)

## Test Scenarios

### Unit Tests
- [ ] Test: After WASM processes CSI ?25l, calling syncModesFromWasm updates TS modes.cursorVisible to false
- [ ] Test: After WASM processes CSI ?25h, calling syncModesFromWasm updates TS modes.cursorVisible to true
- [ ] Test: Mode 12 (cursor blink) is also correctly synced

### Integration Tests
- [ ] Test: Full data flow - PTY data containing CSI ?25l results in cursor hidden after render cycle

### Edge Cases
- [ ] Multiple mode changes in a single data chunk (e.g., CSI ?25l followed by CSI ?25h)
- [ ] Mode changes interleaved with buffer switches (CSI ?1049h + CSI ?25l)
- [ ] Rapid PTY output with frequent mode toggles

## Success Criteria

- [ ] CSI ?25l correctly hides the cursor in TUI applications
- [ ] CSI ?25h correctly shows the cursor
- [ ] All existing mode-related tests continue to pass
- [ ] No measurable performance regression in PTY data processing
