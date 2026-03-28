# Implementation Plan: Mux Protocol Redesign

## Overview

Redesign the mux protocol based on tmux architecture research. Remove blocking OSC handshake, keep all-window streaming (already working), and add GUI tab integration for windows.

## Current State Analysis

Much of the required infrastructure **already exists**:
- All pane output in active session already streams to client (`connection.rs:142`)
- Shadow parser per pane already exists (`pane.rs:54`)
- Window lifecycle messages already defined in protocol (`0x12-0x16`)
- Frontend mux-window-manager.ts already handles WASM grid swap on switch
- Reattach with screen restoration already works (`reattach.rs`)

**What's broken/missing:**
- Blocking OSC handshake freezes on startup
- SwitchWindow handler not implemented (`connection.rs:304-306`)
- GUI tab bar not integrated with mux windows
- Bridge has no Welcome timeout

## Implementation Phases

### Phase 1: Remove Blocking Handshake (FR1, FR9, FR10)

**Goal:** Bridge starts instantly, times out if daemon doesn't respond.

**Files:**
- `src-tauri/src/mux/cli.rs`

**Changes:**
1. Remove `handshake_emterm()` function entirely (lines 13-118)
2. Remove `check_emterm_environment()` if any remnants exist
3. Remove OSC query/ACK handler from `src/terminal-app/osc-handler.ts` (lines 331-335)
4. In `execute_mux()`: remove handshake call, keep only nesting check
5. In `execute_attach()`: same
6. In `bridge_main_loop()`: add 5-second timeout on Welcome response read (`sock_reader.read_exact` at line 227)

**Tests:**
- Bridge starts without delay when daemon is running
- Bridge exits with error after 5s if daemon unreachable
- Nesting check still works (EMTERM_MUX=1)

### Phase 2: Implement SwitchWindow Handler (FR6, FR7)

**Goal:** SwitchWindow message updates daemon's active window tracking.

**Files:**
- `src-tauri/src/mux/ipc/connection.rs` (line 304-306)
- `src-tauri/src/mux/session/session.rs`

**Changes:**
1. In `route_message()`: implement SwitchWindow handler
   - Decode window_id from pane_id field (or payload)
   - Call `session.set_active_window(window_id)`
   - Send StatusUpdate back to client
2. In `MuxSession`: add `set_active_window()` method

**Tests:**
- SwitchWindow message updates active_window_id in session
- StatusUpdate sent after window switch

### Phase 3: GUI Tab ↔ Mux Window Integration (FR5)

**Goal:** Mux windows appear as tabs in eMterm's GUI tab bar.

**Files:**
- `src/terminal-app/mux/mux-session.ts`
- `src/terminal-app/mux/mux-window-manager.ts`
- `src/terminal-app/index.ts` (TerminalApp)
- `src/tab-bar/` (tab bar UI)

**Changes:**
1. When mux mode is entered and PaneCreated messages arrive:
   - Group panes by window (using StatusUpdate's window_names)
   - Create a GUI tab for each window
   - Route PtyOutput to the correct window's WASM grid by pane_id

2. On tab click:
   - Send SwitchWindow to daemon
   - Switch active WASM grid for Canvas rendering (instant, no data needed)

3. On StatusUpdate from daemon:
   - Update tab names
   - Add/remove tabs as needed
   - Show activity indicators (bell, etc.)

4. On CreateWindow (prefix+c or tab "+" button):
   - Send CreateWindow to daemon
   - Wait for PaneCreated + StatusUpdate
   - Create new tab with new WASM grid

5. On DestroyWindow (close tab):
   - Send DestroyWindow to daemon
   - Wait for StatusUpdate
   - Remove tab

**Tests:**
- Window creation adds a tab
- Window destruction removes a tab
- Tab switch renders correct content instantly
- StatusUpdate updates tab names

### Phase 4: All-Window Output Routing (FR4, FR3)

**Goal:** Ensure output from ALL windows routes to correct WASM grids.

**Files:**
- `src/terminal-app/mux/mux-session.ts` (output routing, lines 119-150)
- `src/terminal-app/mux/mux-window-manager.ts`

**Changes:**
1. Current routing already handles pane_id dispatch (line 119-150 in mux-session.ts)
2. Ensure non-active window panes also get routed to their WASM grids
   - Currently: non-active pane output goes to `savedState.primaryGrid.core.process_pty_data(data)` (line 135)
   - This is correct — inactive panes process data into their grid silently
3. Verify that window-switch shows immediately-current grid content

**Tests:**
- Background window receives output and grid updates
- Switch to background window shows latest content
- No data loss during rapid window switching

### Phase 5: Reattach with Multi-Window (FR8, FR2)

**Goal:** Reattach restores all window grids correctly.

**Files:**
- `src-tauri/src/mux/ipc/reattach.rs`
- `src-tauri/src/mux/ipc/connection.rs`

**Changes:**
1. Current reattach already sends all pane data (reattach.rs lines 66-73)
2. Ensure PaneCreated messages include window association info
   - Currently pane_id is in PaneCreated but window grouping comes from StatusUpdate
   - Send StatusUpdate after all PaneCreated messages during reattach
3. Client must create tabs for each window during reattach, then populate WASM grids

**Tests:**
- Detach, reconnect: all windows restored
- Tab bar shows correct windows after reattach
- Active window state preserved

## Implementation Order

```
Phase 1 (Handshake removal)     ← IMMEDIATE FIX for freeze bug
  ↓
Phase 2 (SwitchWindow handler)  ← Backend foundation
  ↓
Phase 3 (GUI Tab integration)   ← Main feature work
  ↓
Phase 4 (Output routing verify) ← Mostly verification of existing behavior
  ↓
Phase 5 (Reattach multi-window) ← Polish
```

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| Tab bar UI complexity | Medium | Reuse existing tab infrastructure |
| Output routing regression | Medium | Existing pane_id dispatch already works |
| Reattach timing | Low | Current mechanism already handles this |
| Bridge timeout edge cases | Low | 5s timeout is generous; daemon starts in <1s |

## Key Files Reference

| File | Purpose | Phase |
|------|---------|-------|
| `src-tauri/src/mux/cli.rs` | Bridge + handshake | 1 |
| `src/terminal-app/osc-handler.ts` | OSC ACK handler (remove) | 1 |
| `src-tauri/src/mux/ipc/connection.rs` | Daemon message routing | 2 |
| `src-tauri/src/mux/session/session.rs` | Session active window | 2 |
| `src/terminal-app/mux/mux-session.ts` | Frontend mux lifecycle | 3, 4 |
| `src/terminal-app/mux/mux-window-manager.ts` | Window/grid switching | 3, 4 |
| `src/terminal-app/index.ts` | TerminalApp tab management | 3 |
| `src-tauri/src/mux/ipc/reattach.rs` | Reattach data collection | 5 |
