# Feature: mux send-keys CLI Command

## Overview

Add `emterm mux send-keys` subcommand to send stdin data as key input to a specific window's pane in the active mux session. Data is read from stdin and forwarded as raw bytes to the target PTY via the existing `PtyInput` message.

## Objectives

- Enable sending arbitrary data (text, control characters, escape sequences) to mux windows from CLI via stdin
- Allow targeting specific windows by index
- Keep implementation minimal by leveraging `printf`/shell escapes instead of custom key name parsing

## User Stories

### US1: Scripted Workspace Setup
As a power user, I want to pipe text and control characters into `send-keys` to set up my workspace automatically after login.

**Acceptance Criteria:**
- [ ] `printf 'glances\r' | emterm mux send-keys -t 0` sends text followed by Enter to window 0
- [ ] `-t <window-index>` targets a specific window by index
- [ ] Without `-t`, send to the active window of the active session
- [ ] Command reads all of stdin, sends it, and exits immediately

### US2: Error Handling
As a user, I want clear error messages when the target window does not exist.

**Acceptance Criteria:**
- [ ] Error message on stderr when daemon is not running
- [ ] Error message when target window index is out of range
- [ ] Exit code 1 on failure, 0 on success

## Technical Requirements

### Functional Requirements

- **FR1:** Add `send-keys` subcommand to `emterm mux` with `-t`/`--target` option
- **FR2:** Read all data from stdin as raw bytes (no interpretation or key name parsing)
- **FR3:** CLI connects to daemon via `cli_handshake()`, resolves target window's active pane, sends `PtyInput` message with stdin data, then disconnects
- **FR4:** Without `-t`, send to the active window of the active session
- **FR5:** With `-t <index>`, send to the window at the given 0-based index in the active session
- **FR6:** If stdin is empty (EOF without data), exit with code 0 without sending

### Non-Functional Requirements

- **NFR1 - Performance:** Command completes within 500ms
- **NFR2 - Platform:** Linux and Windows support (Unix socket / named pipe)

## Implementation Approach

### Architecture

```
CLI (sync)                    Daemon (async)
    |                             |
    |-- [read stdin to buf] --    |
    |-- Hello ------------------>|
    |<-- Welcome (sessions) -----|
    |-- PtyInput(pane_id, buf)->|
    |   (exit 0)                  |-- write buf to PTY
```

### Data Flow

1. CLI reads all of stdin into a byte buffer
2. CLI calls `cli_handshake()` to connect and authenticate
3. CLI resolves target pane ID from Welcome message's session info
4. CLI sends `PtyInput(pane_id, data)` message
5. Daemon writes data to PTY (existing handler at `connection.rs:316`)
6. CLI exits

### Window Targeting Resolution

The current `SessionInfo` in the Welcome message contains `window_count` and `active_window_index` but not per-window details.

**Extend SessionInfo** with `windows: Vec<WindowInfo>`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub name: String,
    pub active_pane_id: u32,
}
```

Update `SessionInfo`:
```rust
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub window_count: u32,
    pub pane_count: u32,
    pub active_window_index: u32,
    pub windows: Vec<WindowInfo>,  // NEW
}
```

Backward compatibility: `#[serde(default)]` on `windows` field so existing clients receiving old Welcome messages get an empty vec.

### CLI Design

```
emterm mux send-keys [OPTIONS]

Reads data from stdin and sends it as key input to the target pane.

OPTIONS:
  -t, --target <INDEX>   Target window index (0-based, default: active window)
```

**Examples:**
```bash
# Send "glances" + Enter to window 0
printf 'glances\r' | emterm mux send-keys -t 0

# Send "ssh router" + Enter to window 1
printf 'ssh router\r' | emterm mux send-keys -t 1

# Send to active window
printf 'ls -la\r' | emterm mux send-keys

# Send Ctrl-C to window 3
printf '\x03' | emterm mux send-keys -t 3

# Send complex command
printf 'cd ~/src/my_projects\r' | emterm mux send-keys -t 4

# Here-string
emterm mux send-keys -t 0 <<< $'glances\r'
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/main.rs`: CLI argument definition (clap builder)
- `src-tauri/src/mux/cli.rs`: Command execution functions, `cli_handshake()`
- `src-tauri/src/mux/ipc/protocol.rs`: `SessionInfo`, `WindowInfo` (new), `MuxMessage::pty_input()`
- `src-tauri/src/mux/ipc/connection.rs`: CLI client message handling, PtyInput handler
- `src-tauri/src/mux/session/manager.rs`: Session/window listing for Welcome message

### File Structure

```
src-tauri/src/
├── main.rs                      # Add send-keys subcommand definition
├── mux/
│   ├── cli.rs                   # Add execute_send_keys()
│   └── ipc/
│       ├── protocol.rs          # Add WindowInfo, extend SessionInfo
│       ├── connection.rs        # Update Welcome construction to include WindowInfo
│       └── handlers.rs          # No changes (PtyInput handler already exists)
```

## Test Scenarios

### Unit Tests
- [ ] `WindowInfo` serialization/deserialization roundtrip
- [ ] Extended `SessionInfo` backward compatibility (missing `windows` field defaults to empty vec)
- [ ] Extended `SessionInfo` with windows roundtrip

### Integration Tests
- [ ] CLI argument parsing: `send-keys` with no options
- [ ] CLI argument parsing: `send-keys -t 0`
- [ ] CLI argument parsing: `send-keys -t 5`

### Edge Cases
- [ ] Empty stdin (EOF) -> exit 0 without sending
- [ ] Target index out of range -> error message and exit code 1
- [ ] Daemon not running -> error message and exit code 1
- [ ] Large stdin data -> bounded by IPC frame max (16MB)
- [ ] Binary data in stdin -> forwarded as-is

## Error Handling

### Error Cases

| Condition | stderr Output | Exit Code |
|-----------|--------------|-----------|
| Daemon not running | "No mux daemon running" | 1 |
| Connection failed | "Failed to connect to daemon: {error}" | 1 |
| Handshake rejected | "Connection rejected: {reason}" | 1 |
| No active session | "No active session" | 1 |
| Window index out of range | "Window index {n} out of range (0..{max})" | 1 |
| No active pane in window | "No active pane in window {n}" | 1 |
| Stdin read error | "Failed to read stdin: {error}" | 1 |

## Security Considerations

- **Input Validation:** Stdin data is written directly to PTY (same trust model as typing in terminal)
- **IPC:** Unix domain socket with filesystem permissions (existing model)
- **Size limit:** Stdin is bounded by `MAX_FRAME_LENGTH` (16MB) to prevent OOM

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Existing mux E2E tests pass without regression
- [ ] Linux and Windows compilation succeeds
- [ ] CLI help text is correct (`emterm mux send-keys --help`)
- [ ] Init script example works end-to-end

## Open Questions

> **Note**: No unresolved requirements.

## References

- Existing mux CLI: `src-tauri/src/mux/cli.rs`
- IPC protocol: `src-tauri/src/mux/ipc/protocol.rs`
- PtyInput handler: `src-tauri/src/mux/ipc/connection.rs:316`
- mux-new-window spec: `doc/tasks/mux-new-window/SPEC.md`
