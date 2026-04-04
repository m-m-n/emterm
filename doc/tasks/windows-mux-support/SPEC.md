# Feature: Windows Mux Support

## Overview

Enable eMterm's mux functionality on Windows by implementing IPC via Named Pipes, daemon process detachment via Windows process creation flags, and terminal raw mode via Windows Console API. This provides session persistence, window management, and reattach/detach on Windows 11.

## Objectives

- Run the mux daemon on Windows using Named Pipes for IPC
- Implement bridge process with Windows Console API raw mode
- Support session management (ls, kill), window operations (new-window, switch-window), and reattach/detach
- Maintain zero impact on existing Unix implementation

## User Stories

### US1: Start Mux Session on Windows
As a Windows user, I want to run `emterm mux` to start a persistent terminal session, so that my work survives terminal window closure.

**Acceptance Criteria:**
- [ ] `emterm mux` spawns daemon (if not running) and connects via Named Pipe
- [ ] Terminal switches to raw mode for direct I/O
- [ ] Closing the terminal window does not kill the daemon

### US2: Reattach to Existing Session
As a Windows user, I want to reconnect to a detached mux session, so that I can resume my work.

**Acceptance Criteria:**
- [ ] `emterm mux` detects existing daemon and reattaches
- [ ] Buffered output since detach is replayed

### US3: Manage Windows and Sessions
As a Windows user, I want to create/switch windows and list/kill sessions via CLI.

**Acceptance Criteria:**
- [ ] `emterm mux new-window` creates a new window
- [ ] `emterm mux switch-window` switches active window
- [ ] `emterm mux ls` lists sessions and windows
- [ ] `emterm mux kill` terminates the daemon

## Technical Requirements

### Functional Requirements
- **FR1:** Named Pipe daemon — Listen on `\\.\pipe\emterm-mux-default`, accept client connections, manage sessions via `SessionManager`. Handle `Ctrl+C` for graceful shutdown. Auto-exit when all sessions are empty.
- **FR2:** Daemon process detachment — Spawn daemon with `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS` creation flags via `CommandExt::creation_flags()`. Redirect stdin to null, stdout to null, stderr to log file.
- **FR3:** Bridge connection — Connect to Named Pipe, set console to `ENABLE_VIRTUAL_TERMINAL_INPUT` via `SetConsoleMode`, forward stdin↔pipe bidirectionally, restore console mode on exit via RAII guard.
- **FR4:** Session management CLI — `emterm mux ls` and `emterm mux kill` communicate with daemon over Named Pipe using the same IPC protocol (codec/protocol layers are shared).
- **FR5:** Window operations CLI — `emterm mux new-window` and `emterm mux switch-window` send corresponding IPC messages over Named Pipe.
- **FR6:** Reattach/detach — On bridge disconnect, switch pane output to ring buffer. On reconnect, drain buffered output and send to new client. Shared logic with Unix implementation.
- **FR7:** Stale pipe detection — `is_daemon_running()` attempts Named Pipe connection to verify daemon liveness. Failed connection indicates stale state.

### Non-Functional Requirements
- **NFR1 - Performance:** Named Pipe communication latency comparable to Unix domain sockets. No measurable regression in PTY output throughput.
- **NFR2 - Security:** Named Pipe created with `PIPE_REJECT_REMOTE_CLIENTS` flag. Access restricted to current user via security descriptor.
- **NFR3 - Compatibility:** Minimum OS: Windows 11. IPC message format identical to Unix (shared codec/protocol).
- **NFR4 - Maintainability:** Platform-specific code isolated with `#[cfg(windows)]` / `#[cfg(unix)]`. Shared logic (session management, protocol, handlers) remains platform-agnostic.

## Implementation Approach

### Architecture

**Transport Abstraction:**
```
┌──────────────────────────────────────────────┐
│           Shared Logic (platform-agnostic)    │
│  SessionManager, IPC handlers, protocol,     │
│  codec, reattach, ring_buffer                │
├──────────────────┬───────────────────────────┤
│  Unix Transport  │  Windows Transport        │
│  UnixListener    │  NamedPipeServer          │
│  UnixStream      │  NamedPipeClient          │
│  libc termios    │  Windows Console API      │
│  setsid()        │  DETACHED_PROCESS         │
│  SIGHUP handler  │  (not applicable)         │
└──────────────────┴───────────────────────────┘
```

The IPC handlers (`handle_connection`, `handle_create_window`, etc.) already accept `tokio::io::AsyncRead + AsyncWrite` compatible streams. Named Pipe streams from tokio implement these traits, so handler code is shared.

### Data Flow

```
Windows:
  User → emterm mux → Bridge (Named Pipe client)
       → Daemon (Named Pipe server) → PTY (portable-pty)
       ← PTY output ← Daemon ← Named Pipe ← Bridge → stdout
```

### Key Implementation Details

#### Named Pipe Server (daemon.rs)

```rust
#[cfg(windows)]
pub async fn run_daemon() -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::{ServerOptions, PipeMode};

    let pipe_name = pipe_path(); // \\.\pipe\emterm-mux-default

    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .reject_remote_clients(true)
            .pipe_mode(PipeMode::Byte)
            .create(&pipe_name)?;

        tokio::select! {
            result = server.connect() => {
                result?;
                tokio::spawn(handle_connection(
                    server, session_manager.clone(), shutdown_tx.clone()
                ));
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
        }
    }
    // graceful shutdown...
}
```

#### Named Pipe Client (bridge.rs, cli.rs)

```rust
#[cfg(windows)]
async fn connect_to_daemon(pipe_name: &str) -> tokio::io::Result<NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    ClientOptions::new().open(pipe_name)
}
```

#### Console Raw Mode (bridge.rs)

```rust
#[cfg(windows)]
fn set_stdin_raw() -> Option<u32> {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        let mut original_mode: u32 = 0;
        GetConsoleMode(handle, &mut original_mode);
        SetConsoleMode(handle, ENABLE_VIRTUAL_TERMINAL_INPUT);
        Some(original_mode)
    }
}

#[cfg(windows)]
fn restore_stdin(original_mode: u32) {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        SetConsoleMode(handle, original_mode);
    }
}
```

#### Daemon Spawning (daemon.rs)

```rust
#[cfg(windows)]
{
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
    const DETACHED_PROCESS: u32 = 0x00000008;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}
```

### Dependencies

**Internal Dependencies:**
- `mux::session::manager::SessionManager`: Shared session management (already platform-agnostic)
- `mux::ipc::protocol`, `mux::ipc::codec`: Shared IPC protocol (already platform-agnostic)
- `mux::ipc::handlers`: Shared message handlers (already platform-agnostic)
- `mux::ipc::reattach`: Shared reattach logic (already platform-agnostic)

**External Dependencies:**
- `tokio` (existing): `net` feature includes `windows::named_pipe`
- `windows-sys` (new): Windows Console API bindings (`Win32_System_Console`)
- `portable-pty` (existing): PTY abstraction (already supports Windows)

### File Structure

Changes are within existing files. No new files needed.

```
src-tauri/src/mux/
├── daemon.rs        # Add #[cfg(windows)] run_daemon(), pipe_path(),
│                    #   is_daemon_running(), ensure_daemon_running()
├── bridge.rs        # Add #[cfg(windows)] run_bridge(), set_stdin_raw(),
│                    #   restore_stdin(), RawModeGuard, bridge_main_loop()
├── cli.rs           # Add #[cfg(windows)] for execute_script(),
│                    #   execute_new_window(), execute_switch_window(),
│                    #   execute_ls(), execute_kill(), cli_handshake()
└── ipc/
    ├── mod.rs       # Remove #[cfg(unix)] gate on connection, handlers,
    │                #   reattach modules (make them platform-agnostic)
    ├── connection.rs  # Generalize stream type to AsyncRead+AsyncWrite
    ├── handlers.rs    # No changes needed (already generic)
    ├── reattach.rs    # No changes needed (already generic)
    └── pty_spawn.rs   # Already has #[cfg(windows)] shell detection
```

## Test Scenarios

### Unit Tests
- [ ] `pipe_path()` returns valid Named Pipe path on Windows
- [ ] `is_daemon_running()` returns false when no daemon is running
- [ ] Console mode save/restore round-trips correctly
- [ ] Daemon spawning uses correct creation flags

### Integration Tests
- [ ] Daemon starts and accepts Named Pipe connections
- [ ] Bridge connects to daemon and exchanges messages
- [ ] CLI commands (ls, kill, new-window, switch-window) communicate correctly
- [ ] Reattach replays buffered output

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (Docker/Linux only)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression (Linux)
- [ ] Windows CI: daemon start → bridge connect → detach → reattach

### Edge Cases
- [ ] Multiple simultaneous Named Pipe clients
- [ ] Daemon crash and restart (stale pipe detection)
- [ ] Console handle invalid (non-console stdin, e.g., piped input)
- [ ] Bridge exit while daemon has active PTY sessions

### Performance Tests
- [ ] Named Pipe throughput: PTY output streamed without blocking

## Security Considerations

- **Named Pipe Access Control:** `PIPE_REJECT_REMOTE_CLIENTS` prevents network access. Security descriptor limits to current user.
- **Log File:** Written to `%LOCALAPPDATA%\emterm\mux-daemon.log` with user-only permissions.

## Error Handling

### Error Codes

| Code | Description | User Message |
|------|-------------|--------------|
| PIPE_CREATE_FAILED | Cannot create Named Pipe | "Failed to create Named Pipe: {error}" |
| PIPE_CONNECT_FAILED | Cannot connect to daemon | "Failed to connect to mux daemon: {error}" |
| DAEMON_SPAWN_FAILED | Cannot spawn daemon process | "Failed to spawn daemon: {error}" |
| CONSOLE_MODE_FAILED | Cannot set console mode | "Failed to set console mode: {error}" |

### Error Flow

```
Error Occurs → Log to stderr/log file → Return descriptive error message → Exit with non-zero code
```

## Success Criteria

- [ ] All functional requirements (FR1-FR7) implemented
- [ ] All unit and integration tests pass on Windows
- [ ] Existing Unix tests pass without regression
- [ ] Daemon survives terminal window closure on Windows
- [ ] GitHub Actions CI passes for both Linux and Windows
- [ ] No new `unsafe` code beyond Console API FFI calls

## Open Questions

> **Note**: No unresolved requirements at this time.

## Implementation Phases

### Phase 1: Transport and Daemon
**Goals:** Named Pipe daemon, process detachment, stale detection
**Deliverables:**
- `#[cfg(windows)]` `run_daemon()` with Named Pipe server
- `#[cfg(windows)]` `ensure_daemon_running()` with DETACHED_PROCESS
- `#[cfg(windows)]` `is_daemon_running()` and `pipe_path()`
- Generalize `ipc/mod.rs` to remove Unix-only gates

### Phase 2: Bridge and CLI
**Goals:** Bridge connection, raw mode, all CLI commands
**Deliverables:**
- `#[cfg(windows)]` bridge with Console API raw mode
- `#[cfg(windows)]` CLI commands: ls, kill, new-window, switch-window
- `#[cfg(windows)]` cli_handshake over Named Pipe

### Phase 3: Reattach and Testing
**Goals:** Detach/reattach, comprehensive testing
**Deliverables:**
- Verify reattach works over Named Pipe transport
- Unit tests, integration tests
- Windows CI in GitHub Actions

## References

- Existing Unix implementation: `src-tauri/src/mux/`
- tokio Named Pipes: `tokio::net::windows::named_pipe`
- windows-sys crate: Windows Console API bindings
- portable-pty: Cross-platform PTY abstraction
