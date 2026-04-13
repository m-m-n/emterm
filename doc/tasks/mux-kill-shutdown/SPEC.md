# Feature: Graceful Mux Kill via IPC Shutdown Message

## Overview

Replace the current `emterm mux kill` implementation (which only removes the socket file and suggests `pkill -f`) with a proper IPC-based shutdown. The CLI sends a `Shutdown` message through the existing socket/Named Pipe, triggering the daemon's graceful shutdown flow that terminates all PTY subprocesses.

## Objectives

- `emterm mux kill` terminates the daemon and all its subprocesses cleanly
- No `pkill -f` suggestion needed — a single command does everything
- Works on both Linux (Unix socket) and Windows (Named Pipe)

## User Stories

### US1: Kill mux daemon
As a terminal user, I want to run `emterm mux kill` and have all mux sessions terminate immediately, so that I don't need to manually hunt down processes.

**Acceptance Criteria:**
- [ ] `emterm mux kill` sends a Shutdown message to the daemon via IPC
- [ ] The daemon performs graceful shutdown (closes all PTYs, removes socket)
- [ ] The CLI exits with status 0 after sending the message
- [ ] All shell subprocesses spawned by the daemon are terminated

### US2: Kill when daemon is already dead
As a terminal user, I want `emterm mux kill` to clean up stale socket files when the daemon is not reachable, so that I can recover from crashes.

**Acceptance Criteria:**
- [ ] If the daemon socket exists but connection fails, the socket file is removed
- [ ] A message indicates the daemon was not running (socket cleaned up)
- [ ] CLI exits with status 0

## Technical Requirements

### Functional Requirements
- **FR1:** Add `Shutdown = 0x18` variant to `MessageType` enum in `protocol.rs`
- **FR2:** `execute_kill` connects to the daemon via IPC (Unix socket / Named Pipe), sends a Hello + Shutdown message, then exits (fire-and-forget)
- **FR3:** The daemon's `handle_cli_client` recognizes `MessageType::Shutdown` and triggers `shutdown_tx.send(true)`, which enters the existing `graceful_shutdown` flow
- **FR4:** If connection to the daemon fails, fall back to removing the socket/marker file and report that the daemon was not running
- **FR5:** Remove `pkill -f` / `taskkill` suggestions from CLI output

### Non-Functional Requirements
- **NFR1 - Cross-platform:** The implementation must work on both Linux and Windows using the existing IPC abstraction
- **NFR2 - Backward compatibility:** The new `MessageType` value must not conflict with existing values (use `0x18`)

## Implementation Approach

### Data Flow

```
CLI (emterm mux kill)
  → connect to socket / Named Pipe
  → send Hello (ClientType::Cli)
  → send MuxMessage { msg_type: Shutdown, pane_id: 0, payload: [] }
  → disconnect

Daemon
  → handle_cli_client receives Shutdown message
  → calls shutdown_tx.send(true)
  → main loop breaks via shutdown_rx.changed()
  → graceful_shutdown() marks all panes exited
  → socket file removed
  → daemon process exits
```

### Affected Files

```
src-tauri/src/mux/
├── ipc/
│   ├── protocol.rs      # Add MessageType::Shutdown = 0x18, from_u8 mapping
│   └── connection.rs    # Handle Shutdown in handle_cli_client
├── cli.rs               # Rewrite execute_kill to send IPC Shutdown message
└── daemon.rs            # (no changes — existing graceful_shutdown is reused)
```

### Dependencies

**Internal Dependencies:**
- `mux::ipc::protocol` — MessageType enum, MuxMessage
- `mux::ipc::connection` — handle_cli_client, shutdown_tx
- `mux::daemon` — graceful_shutdown (existing, no changes needed)

## Test Scenarios

### Unit Tests
- [ ] `MessageType::from_u8(0x18)` returns `Some(Shutdown)`
- [ ] `MessageType::Shutdown as u8` equals `0x18`

### Integration Tests
- [ ] `handle_cli_client` receiving a Shutdown message triggers shutdown signal
- [ ] `execute_kill` with no daemon running removes stale socket and succeeds
- [ ] `execute_kill` with a running daemon sends Shutdown and exits cleanly

### Edge Cases
- [ ] Daemon socket exists but daemon has crashed — fallback to socket removal
- [ ] Specific session argument (`emterm mux kill <session>`) — still unsupported, prints message

## Success Criteria

- [ ] `emterm mux kill` terminates daemon and all subprocesses without `pkill`
- [ ] Works on Linux (Unix socket) and Windows (Named Pipe)
- [ ] No `pkill -f` or `taskkill` suggestions in output
- [ ] Existing graceful_shutdown flow is reused (no duplication)
- [ ] All tests pass
