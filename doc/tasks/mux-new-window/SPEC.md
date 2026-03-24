# Feature: mux new-window CLI Command

## Overview

Add `emterm mux new-window` subcommand to create a new window in the active mux session from the CLI. Supports optional window naming (`-n`) and initial command execution (`-c`).

## Objectives

- Enable scripted workspace setup via `emterm mux new-window`
- Support window naming and initial command execution in a single command
- Extend IPC protocol to carry window name and initial command

## User Stories

### US1: Scripted Workspace Setup
As a power user, I want to run a shell script that creates multiple named windows with initial commands, so that I can quickly set up my workspace after login.

**Acceptance Criteria:**
- [ ] `emterm mux new-window` creates a new window in the active session
- [ ] `-n editor` sets the window name to "editor" in the tab bar
- [ ] `-c "nvim"` runs `nvim` in the new window after shell starts
- [ ] Multiple `new-window` commands can be chained in a script

### US2: Error Handling
As a user, I want clear error messages when the daemon is not running, so that I know what went wrong.

**Acceptance Criteria:**
- [ ] Error message on stderr when daemon is not running
- [ ] Exit code 1 on failure, 0 on success

## Technical Requirements

### Functional Requirements
- **FR1:** Add `new-window` subcommand to `emterm mux` with `-n`/`--name` and `-c`/`--command` options
- **FR2:** Extend IPC `CreateWindow` message payload to include `name: Option<String>` and `command: Option<String>`
- **FR3:** Daemon handler writes command string + `\n` to PTY after window creation (with short delay for shell readiness)
- **FR4:** CLI connects to daemon via `cli_handshake()`, sends CreateWindow, waits for PaneCreated response

### Non-Functional Requirements
- **NFR1 - Performance:** Command completes within 1 second
- **NFR2 - Platform:** Linux and Windows support (Unix socket / named pipe)

## Implementation Approach

### Architecture

```
CLI (sync)                    Daemon (async)                 GUI
    |                             |                           |
    |-- Hello ------------------>|                           |
    |<-- Welcome (sessions) -----|                           |
    |-- CreateWindow(name,cmd) ->|                           |
    |                             |-- spawn_pty() ---------->|
    |                             |-- create_window(name) -->|
    |                             |-- write cmd+\n to PTY    |
    |<-- PaneCreated ------------|                           |
    |   (exit 0)                  |-- StatusUpdate --------->|
                                                    (tab bar update)
```

### Data Flow

1. CLI parses `-n` and `-c` arguments
2. CLI calls `cli_handshake()` to connect and authenticate
3. CLI sends `CreateWindow` message with serialized `CreateWindowPayload { name, command }`
4. Daemon `handle_create_window` receives payload, spawns PTY, creates window with name
5. If command is present, daemon writes `command + "\n"` to PTY writer after a short delay (~50ms)
6. Daemon sends `PaneCreated` response
7. CLI receives response and exits

### CLI Design

```
emterm mux new-window [OPTIONS]

OPTIONS:
  -n, --name <NAME>        Window name (displayed in tab bar)
  -c, --command <COMMAND>   Initial command to run
```

**Examples:**
```bash
emterm mux new-window
emterm mux new-window -n editor -c "nvim"
emterm mux new-window -n server -c "cd ~/project && bun dev"
emterm mux new-window -n shell
```

### IPC Protocol Extension

New payload struct for `CreateWindow` (0x12) message:

```rust
#[derive(Serialize, Deserialize)]
struct CreateWindowPayload {
    name: Option<String>,
    command: Option<String>,
}
```

Currently `CreateWindow` has no structured payload. The handler `handle_create_window` needs to accept and decode this payload. Backward compatibility: if payload is empty, use defaults (name = "shell", no command).

### Changes to CLI Client Type

Currently, `ClientType::Cli` connections are immediately disconnected after handshake (connection.rs line 95-98). For `new-window`, the CLI needs to:
1. Send Hello with `ClientType::Cli`
2. Receive Welcome
3. Send `CreateWindow` message
4. Receive `PaneCreated` response
5. Disconnect

The daemon's CLI handling needs to be extended to process one control message after handshake before disconnecting, rather than disconnecting immediately.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/main.rs`: CLI argument definition (clap builder)
- `src-tauri/src/mux/cli.rs`: Command execution functions
- `src-tauri/src/mux/ipc/protocol.rs`: Message types and payloads
- `src-tauri/src/mux/ipc/handlers.rs`: `handle_create_window`
- `src-tauri/src/mux/ipc/connection.rs`: CLI client message loop
- `src-tauri/src/mux/ipc/pty_spawn.rs`: PTY spawning (no changes needed if command is written post-spawn)

### File Structure

```
src-tauri/src/
├── main.rs                      # Add new-window subcommand definition
├── mux/
│   ├── cli.rs                   # Add execute_new_window()
│   └── ipc/
│       ├── protocol.rs          # Add CreateWindowPayload
│       ├── handlers.rs          # Extend handle_create_window to accept payload
│       └── connection.rs        # Extend CLI client handling for control messages
```

## Test Scenarios

### Unit Tests
- [ ] CreateWindowPayload serialization/deserialization roundtrip
- [ ] CreateWindowPayload with all fields None
- [ ] CreateWindowPayload with name only
- [ ] CreateWindowPayload with command only
- [ ] CreateWindowPayload with both fields

### Integration Tests
- [ ] CLI argument parsing: `new-window` with no options
- [ ] CLI argument parsing: `new-window -n editor`
- [ ] CLI argument parsing: `new-window -c "nvim"`
- [ ] CLI argument parsing: `new-window -n editor -c "nvim"`

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/*.e2e.js`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Empty name string (`-n ""`) - treat as no name (use default "shell")
- [ ] Empty command string (`-c ""`) - treat as no command
- [ ] Very long command string - no artificial limit, bounded by IPC frame max (16MB)
- [ ] Command with special characters (quotes, pipes, semicolons) - passed as-is to PTY
- [ ] Daemon not running - error message and exit code 1

## Error Handling

### Error Cases

| Condition | stderr Output | Exit Code |
|-----------|--------------|-----------|
| Daemon not running | "No mux daemon running" | 1 |
| Connection failed | "Failed to connect to daemon: {error}" | 1 |
| Handshake rejected | "Connection rejected: {reason}" | 1 |
| Window creation failed | "Failed to create window" | 1 |

## Security Considerations

- **Input Validation:** Command string is written directly to PTY (same trust model as typing in terminal)
- **IPC:** Unix domain socket with filesystem permissions (existing model)

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Existing mux E2E tests pass without regression
- [ ] Linux and Windows compilation succeeds
- [ ] CLI help text is correct (`emterm mux new-window --help`)

## Open Questions

> **Note**: No unresolved requirements.

## References

- Requirements document: `doc/tasks/mux-new-window/要件定義書.md`
- Existing mux CLI: `src-tauri/src/mux/cli.rs`
- IPC protocol: `src-tauri/src/mux/ipc/protocol.rs`
