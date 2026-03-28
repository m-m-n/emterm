# Feature: Mux Protocol Redesign

## Overview

Redesign the emterm mux protocol based on tmux architecture research. Key changes: remove blocking handshake (tmux-style no-check startup), add daemon-side grid for reattach/window-switch, transmit all windows to client for instant tab switching, and use GUI tabs instead of a status bar for window management.

## Objectives

- Remove blocking OSC handshake that causes freeze
- Enable `emterm mux` to work over SSH without pre-checks
- Support multi-window with GUI tab integration
- Maintain daemon-side grid state for reattach and window switching

## Architecture

### Data Model

```
Session ──[1:N]──> Window ──[1:N]──> Pane
   │                 │                  │
   │              name, id           PTY, grid (daemon-side)
   │              active_pane
   │
 active_window (index)
```

### Component Roles

| Component | Role |
|-----------|------|
| **Daemon** (remote) | PTY management, VT100 parse → grid, raw bytes forwarding |
| **Bridge** (remote) | APC encode/decode, stdin/stdout ↔ Unix socket translation |
| **eMterm GUI** (local) | WASM grid (per window), Canvas rendering, tab UI |

### Data Flow

```
Pane PTY output
  → Daemon: VT100 parse → update grid (for reattach)
  → Daemon: forward raw bytes → Bridge
  → Bridge: APC encode → stdout → SSH → eMterm
  → eMterm: WASM parse → grid → Canvas render

User input
  → eMterm: APC encode → ptyClient.write() → SSH
  → Bridge: APC decode → Daemon socket
  → Daemon: route to Pane PTY stdin
```

## Technical Requirements

### Functional Requirements

- **FR1: No-Check Startup** - `emterm mux` starts the bridge immediately without any handshake or environment variable check. Non-eMterm terminals naturally fail (APC sequences are ignored, bridge times out waiting for Welcome response).

- **FR2: Daemon-Side Grid** - Daemon maintains a VT100-parsed grid per pane (via `vt100` crate shadow parser, already exists). Used for reattach screen restoration.

- **FR3: Raw Bytes Forwarding** - Daemon forwards raw PTY output bytes to the connected client via the bridge. Client-side WASM parses independently for rendering.

- **FR4: All-Window Streaming** - Daemon streams PTY output for ALL windows (not just the active one) to the client. Each PtyOutput message carries a pane_id, allowing the client to route data to the correct WASM grid.

- **FR5: Window ↔ GUI Tab Mapping** - Each mux window maps to an eMterm GUI tab. Window creation/destruction/switching are communicated via control messages and reflected in the tab bar.

- **FR6: Window Lifecycle Messages** - The following control messages manage windows:
  - `CreateWindow` (0x12): Client → Daemon. Create a new window with optional name/command.
  - `SwitchWindow` (0x13): Client → Daemon. Notify daemon of active window change (for priority/optimization).
  - `RenameWindow` (0x14): Client → Daemon. Rename a window.
  - `DestroyWindow` (0x15): Client → Daemon. Close a window and its panes.
  - `StatusUpdate` (0x16): Daemon → Client. Push window list updates (names, active index, flags).

- **FR7: Window Switch Behavior** - When the user switches tabs:
  1. Client sends `SwitchWindow` to daemon (informational)
  2. Client switches which WASM grid is rendered on Canvas
  3. No data transfer needed — the target window's grid is already up-to-date (FR4)

- **FR8: Reattach Screen Restoration** - On reattach, daemon sends the shadow parser's `contents_formatted()` for each pane, followed by any ring buffer data accumulated during detach. Client rebuilds WASM grids from this data.

- **FR9: Bridge Timeout** - If the bridge does not receive a Welcome response from the daemon within 5 seconds after Hello, it exits with an error. This naturally handles non-eMterm terminals (APC never reaches a daemon).

- **FR10: Nesting Prevention** - The `EMTERM_MUX=1` environment variable check remains to prevent running mux inside mux.

### Non-Functional Requirements

- **NFR1 - Latency:** No blocking checks on startup. Bridge connects to daemon and starts forwarding immediately.
- **NFR2 - Memory:** Each daemon-side grid uses the `vt100` crate's shadow parser (already allocated per pane). No additional memory overhead.
- **NFR3 - Bandwidth:** All-window streaming increases bandwidth proportionally to active windows. Idle windows produce minimal PTY output.

## Protocol Messages (Existing + Changes)

### Frame Format

```
[length: u32][type: u8][pane_id: u32][payload: variable]
```

Over-the-wire (bridge ↔ eMterm): APC-encoded with base64.

```
ESC _ emterm-mux;<base64(frame_body)> ESC \
```

### Message Types

| Type | ID | Direction | Payload | Description |
|------|----|-----------|---------|-------------|
| PtyOutput | 0x01 | D→C | raw bytes | PTY output for a pane (sent for ALL windows) |
| PtyInput | 0x02 | C→D | raw bytes | Keyboard input for a pane |
| Hello | 0x03 | C→D | HelloMsg | Client handshake request |
| Welcome | 0x04 | D→C | WelcomeMsg | Daemon handshake response with session list |
| CreatePane | 0x05 | C→D | — | Create a new pane |
| PaneCreated | 0x06 | D→C | pane_id | Notification of new pane |
| DestroyPane | 0x07 | C→D | — | Close a pane |
| Resize | 0x08 | C→D | ResizeMsg | Resize a pane |
| Attach | 0x09 | C→D | AttachMsg | Attach to a session |
| Detach | 0x0A | C→D | — | Detach from session |
| Detached | 0x0B | D→C | — | Confirm detach |
| Snapshot | 0x0C | — | — | Reserved |
| SnapshotRestore | 0x0D | — | — | Reserved |
| SessionList | 0x0E | D→C | sessions | Session listing |
| Error | 0x0F | D→C | ErrorMsg | Error notification |
| PtyExited | 0x10 | D→C | PtyExitedMsg | Pane process exited |
| CreateWindow | 0x12 | C→D | CreateWindowPayload | Create new window |
| SwitchWindow | 0x13 | C→D | window_id | Active window changed (informational) |
| RenameWindow | 0x14 | C→D | RenameWindowMsg | Rename window |
| DestroyWindow | 0x15 | C→D | window_id | Destroy window |
| StatusUpdate | 0x16 | D→C | StatusUpdateMsg | Window list update push |

### Startup Sequence

```
Bridge                              Daemon
  |                                    |
  |-- Hello (protocol_version) ------->|
  |<-- Welcome (sessions, windows) ----|
  |                                    |
  |  (for each pane in session:)       |
  |<-- PaneCreated (pane_id) ----------|
  |<-- PtyOutput (screen restore) -----|
  |                                    |
  |  (steady state: bidirectional)     |
  |<== PtyOutput (all windows) ========|
  |=== PtyInput ======================>|
```

### Window Tab Lifecycle

```
User clicks "+" tab
  → Client: CreateWindow → Daemon
  → Daemon: spawn PTY, create window
  → Daemon: PaneCreated → Client
  → Daemon: StatusUpdate → Client
  → Client: create new tab, WASM grid

User clicks tab
  → Client: SwitchWindow → Daemon (informational)
  → Client: switch Canvas to target WASM grid (instant, no data needed)

User closes tab
  → Client: DestroyWindow → Daemon
  → Daemon: kill PTY, remove window
  → Daemon: StatusUpdate → Client
  → Client: remove tab
```

## Changes from Current Implementation

| Area | Current | New |
|------|---------|-----|
| Startup check | Blocking OSC handshake | No check (FR1) |
| Daemon grid | Shadow parser for reattach only | Same (no change needed) |
| Data transfer | Active window only | All windows (FR4) |
| Window switching | Full screen restore on switch | Instant (WASM grid already current) |
| Tab integration | Not implemented | GUI tabs ↔ mux windows (FR5) |
| Bridge timeout | 2s handshake timeout | 5s Welcome timeout (FR9) |
| TERM_PROGRAM check | Removed (broken) | Stays removed |
| OSC query/ACK | Implemented but broken | Removed entirely |

## Test Scenarios

### Unit Tests
- [ ] Bridge exits with error after 5s if no Welcome received
- [ ] PtyOutput messages are routed by pane_id to correct handler
- [ ] StatusUpdate correctly serializes/deserializes window list
- [ ] CreateWindow/DestroyWindow round-trip through protocol

### Integration Tests
- [ ] Multi-window: create 3 windows, switch between them, verify all grids update
- [ ] Reattach: detach, reconnect, verify all window grids are restored
- [ ] Window close: destroy window, verify tab removed and no orphan panes

### Edge Cases
- [ ] Bridge started in non-eMterm terminal: APC ignored, bridge times out after 5s
- [ ] Rapid window switching: no data loss
- [ ] Large PTY output on non-active window: client buffers without visible lag

## Success Criteria

- [ ] `emterm mux` starts instantly (no handshake delay)
- [ ] Works over SSH without configuration
- [ ] Multiple windows accessible as GUI tabs
- [ ] Tab switching is instant (no screen redraw delay)
- [ ] Reattach restores all window states
- [ ] Non-eMterm terminals: bridge exits cleanly after timeout

## References

- Current protocol: `src-tauri/src/mux/ipc/protocol.rs`
- Bridge implementation: `src-tauri/src/mux/cli.rs`
- Daemon connection handler: `src-tauri/src/mux/ipc/connection.rs`
- OSC handler: `src/terminal-app/osc-handler.ts`
- tmux architecture research: `memory/project_mux_architecture_research.md`
