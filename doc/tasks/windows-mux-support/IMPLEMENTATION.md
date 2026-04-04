# Implementation Plan: Windows Mux Support

## Overview
Enable eMterm's mux functionality on Windows by abstracting stream types to support Named Pipes, implementing Windows-specific daemon/bridge/CLI, and adding Console API raw mode for the bridge process.

## Objectives
- Generalize IPC stream types from concrete `UnixStream` to trait-based `AsyncRead + AsyncWrite`
- Implement Named Pipe-based daemon, bridge, and CLI on Windows
- Implement Console API raw mode for the bridge process
- Maintain zero impact on existing Unix implementation

## Prerequisites

### Development Environment
- Rust toolchain with Windows cross-compilation target
- Windows 11 for testing
- GitHub Actions Windows runner for CI

### Dependencies
- `windows-sys` crate for Console API bindings (new)
- `tokio` with `net` feature (existing — includes `windows::named_pipe`)
- `portable-pty` (existing — already supports Windows)

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Async Runtime**: tokio (existing)
- **IPC (Unix)**: Unix domain sockets (unchanged)
- **IPC (Windows)**: Named Pipes via `tokio::net::windows::named_pipe`
- **Terminal Control (Windows)**: `windows-sys` (`Win32_System_Console`)

### Design Approach
Two-layer architecture: platform-agnostic IPC handlers over platform-specific transport. The IPC handler layer (connection, handlers, reattach) is generified to accept any `AsyncRead + AsyncWrite` stream. Transport creation (Unix socket vs Named Pipe) is isolated in platform-specific code.

### Component Interaction
```
Shared (platform-agnostic):
  codec.rs, protocol.rs ─── message serialization
  connection.rs<S> ──────── per-client connection handler (generic stream)
  handlers.rs<S> ────────── IPC message handlers (generic stream)
  reattach.rs<S> ────────── detach/reattach logic (generic stream)
  statusbar.rs ──────────── status bar engine (no stream dependency)
  session/ ──────────────── session/window/pane management

Platform-specific (cfg-gated):
  daemon.rs ─── Unix: UnixListener / Windows: NamedPipeServer
  bridge.rs ─── Unix: UnixStream + libc termios / Windows: NamedPipeClient + Console API
  cli.rs ────── Unix: std UnixStream / Windows: std Named Pipe (blocking)
```

## Implementation Phases

### Phase 1: Stream Type Generification

**Goal**: Make IPC handler layer platform-agnostic by replacing concrete `UnixStream` with generic trait bounds. Unix behavior remains identical.

**Files to Modify**:
- `src-tauri/src/mux/ipc/connection.rs` — Generify stream type in function signatures
- `src-tauri/src/mux/ipc/handlers.rs` — Generify `Framed<UnixStream, MuxCodec>` parameters
- `src-tauri/src/mux/ipc/reattach.rs` — Generify `send_reattach_data` parameter

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_connection<S>` | Per-client connection lifecycle | Stream implements AsyncRead + AsyncWrite + Unpin + Send | Handles handshake, routing, cleanup regardless of transport |
| `handle_cli_client<S>` | CLI command processing | Generic framed stream | Processes CLI messages over any transport |
| `route_message<S>` | Message dispatch to handlers | Generic framed stream | Routes messages to appropriate handler |
| Handler functions | Individual message handling | Generic framed stream | Read/write IPC messages over any transport |
| `send_reattach_data<S>` | Send buffered output on reattach | Generic framed stream | Drains ring buffer to client |

**Implementation Steps**:
1. **Generify connection.rs** — Change `handle_connection`, `handle_cli_client`, `route_message` to accept generic stream with `AsyncRead + AsyncWrite + Unpin + Send` bounds
2. **Generify handlers.rs** — Change all handler functions to accept generic `Framed<S, MuxCodec>` instead of `Framed<UnixStream, MuxCodec>`
3. **Generify reattach.rs** — Change `send_reattach_data` to accept generic framed stream
4. **Update mod.rs gates** — Remove `#[cfg(unix)]` from connection, handlers, reattach, statusbar, pty_spawn modules (they are now platform-agnostic)
5. **Verify Unix compilation** — Ensure existing Unix code compiles and tests pass with generified types

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: Existing Unix tests must pass unchanged (generic types are backward-compatible)
- Integration: `cargo test` on Linux to verify no regressions

**Acceptance Criteria**:
- [ ] All IPC handler functions accept generic stream types
- [ ] Existing Unix tests pass without modification
- [ ] No `UnixStream` references remain in connection.rs, handlers.rs, reattach.rs

**Estimated Effort**: small

---

### Phase 2: Windows Dependencies and Daemon

**Goal**: Add Windows dependencies and implement the Named Pipe daemon server.

**Files to Modify**:
- `src-tauri/Cargo.toml` — Add `windows-sys` dependency for Console API
- `src-tauri/src/mux/daemon.rs` — Add Windows Named Pipe daemon implementation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `pipe_path()` | Return Named Pipe path | Windows platform | Returns `\\.\pipe\emterm-mux-default` |
| `is_daemon_running()` (Windows) | Check daemon liveness | Pipe path known | Returns true if connection succeeds |
| `ensure_daemon_running()` (Windows) | Spawn daemon if needed | Executable path available | Daemon process running with DETACHED_PROCESS flag |
| `run_daemon()` (Windows) | Main daemon event loop | Named Pipe server created | Accepts clients, handles Ctrl+C, auto-exits on empty sessions |

**Processing Flow**:
1. `ensure_daemon_running()` checks if daemon responds on Named Pipe
   - If not running → spawn daemon with DETACHED_PROCESS + CREATE_NEW_PROCESS_GROUP flags
   - Wait for pipe to become available with exponential backoff
2. `run_daemon()` creates Named Pipe server
   - Loop: create pipe instance → wait for connection → spawn handler → repeat
   - On Ctrl+C → graceful shutdown
   - On all sessions empty → auto-exit

**Implementation Steps**:
1. **Add windows-sys dependency** — Add `Win32_System_Console` features to Cargo.toml under `[target.'cfg(windows)'.dependencies]`
2. **Implement `pipe_path()`** — Return Windows Named Pipe path string
3. **Implement Windows `is_daemon_running()`** — Attempt Named Pipe connection to verify daemon liveness
4. **Implement Windows `ensure_daemon_running()`** — Spawn daemon with process detachment flags, wait for pipe availability
5. **Implement Windows `run_daemon()`** — Named Pipe server loop with tokio select for client accept, Ctrl+C, and shutdown watch channel
6. **Fix existing Windows daemon stub** — Replace the broken `#[cfg(windows)]` select block that references undefined `listener`

**Dependencies**: Phase 1 (handlers must accept generic streams for `handle_connection` calls)

**Testing Approach**:
- Unit: `pipe_path()` returns valid Named Pipe path, `is_daemon_running()` returns false when no daemon
- Integration: Daemon starts and accepts connections (Windows CI)

**Acceptance Criteria**:
- [ ] Named Pipe daemon starts and accepts client connections on Windows
- [ ] Daemon survives parent terminal closure (DETACHED_PROCESS)
- [ ] Ctrl+C triggers graceful shutdown
- [ ] Auto-exit when all sessions are empty
- [ ] Stale pipe detection works

**Estimated Effort**: medium

---

### Phase 3: Windows Bridge

**Goal**: Implement the bridge process for Windows with Named Pipe client and Console API raw mode.

**Files to Modify**:
- `src-tauri/src/mux/bridge.rs` — Add Windows bridge implementation

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `run_bridge()` (Windows) | Entry point for bridge process | Daemon running | Tokio runtime with bridge main loop |
| `bridge_main_loop()` (Windows) | Bidirectional stdin↔pipe forwarding | Named Pipe connection established | Forwards data until disconnect or error |
| `set_stdin_raw()` (Windows) | Set console to VT input mode | Console handle available | Returns original mode for restoration |
| `restore_stdin()` (Windows) | Restore original console mode | Original mode saved | Console mode restored |
| `RawModeGuard` (Windows) | RAII guard for mode restoration | Raw mode set | Auto-restores on drop |

**Processing Flow**:
1. Connect to Named Pipe
2. Save console mode and switch to `ENABLE_VIRTUAL_TERMINAL_INPUT`
3. Split pipe into reader/writer halves
4. Spawn two tasks:
   - stdin → pipe: read stdin, parse APC/OSC sequences, forward to daemon
   - pipe → stdout: read daemon frames, write to stdout
5. On disconnect or error → restore console mode via RAII guard

**Implementation Steps**:
1. **Implement Windows `set_stdin_raw()` and `restore_stdin()`** — Use `GetConsoleMode`/`SetConsoleMode` via `windows-sys`
2. **Implement Windows `RawModeGuard`** — RAII struct that restores console mode on drop
3. **Implement Windows `bridge_main_loop()`** — Named Pipe client connection, split into read/write, bidirectional forwarding with same APC/OSC parsing logic as Unix
4. **Implement Windows `run_bridge()`** — Create tokio runtime and run bridge main loop

**Dependencies**: Phase 2 (daemon must be running to accept connections)

**Testing Approach**:
- Unit: Console mode save/restore round-trip
- Integration: Bridge connects to daemon and exchanges handshake messages
- Manual: Interactive terminal session via bridge on Windows

**Acceptance Criteria**:
- [ ] Bridge connects to daemon via Named Pipe
- [ ] Console switches to raw mode and restores on exit
- [ ] Bidirectional data forwarding works (stdin→daemon, daemon→stdout)
- [ ] APC/OSC sequence parsing identical to Unix behavior

**Estimated Effort**: medium

---

### Phase 4: Windows CLI Commands

**Goal**: Implement all CLI command stubs for Windows.

**Files to Modify**:
- `src-tauri/src/mux/cli.rs` — Replace Windows stubs with Named Pipe implementations

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `cli_handshake()` (Windows) | Blocking Named Pipe handshake | Daemon running | Returns pipe stream and session list |
| `execute_script()` (Windows) | Start daemon and print pipe path | None | Daemon running, pipe path printed |
| `execute_new_window()` (Windows) | Create new window in session | Daemon running | Window created, confirmation displayed |
| `execute_switch_window()` (Windows) | Switch active window | Daemon running, target window exists | Active window changed |
| `execute_ls()` (Windows) | List sessions and windows | Daemon running | Session list printed to stdout |
| `execute_kill()` (Windows) | Terminate daemon | Daemon running | Daemon shut down, pipe removed |

**Processing Flow**:
1. `cli_handshake()` connects to Named Pipe (blocking), sends Hello message, receives Welcome with session list
2. Each CLI command calls `cli_handshake()`, sends specific IPC message, processes response
3. `execute_kill()` does not use IPC — terminates daemon by other means (process signal or special message)

**Implementation Steps**:
1. **Implement Windows `cli_handshake()`** — Blocking Named Pipe connection, handshake protocol (same message format as Unix)
2. **Implement `execute_script()`** — Ensure daemon running, print pipe path
3. **Implement `execute_new_window()` and `execute_switch_window()`** — Send IPC messages via handshake connection
4. **Implement `execute_ls()`** — Display session/window list from handshake response
5. **Implement `execute_kill()`** — Terminate daemon process

**Dependencies**: Phase 2 (daemon must exist), Phase 1 (shared protocol)

**Testing Approach**:
- Unit: CLI handshake message format validation
- Integration: CLI commands communicate with running daemon (Windows CI)

**Acceptance Criteria**:
- [ ] All six CLI commands work on Windows
- [ ] Command output format matches Unix behavior
- [ ] Error messages are descriptive when daemon is not running

**Estimated Effort**: medium

---

### Phase 5: Reattach/Detach and CI

**Goal**: Verify reattach/detach works over Named Pipes and add Windows CI.

**Files to Modify**:
- `.github/workflows/ci.yml` (or equivalent) — Add Windows test job
- `src-tauri/src/mux/ipc/pty_spawn.rs` — Verify Windows shell detection works in mux context

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Reattach over Named Pipe | Resume session after disconnect | Detached session exists with buffered output | Buffered output replayed to new client |
| Windows CI | Automated build and test on Windows | GitHub Actions runner | Mux unit/integration tests pass |

**Processing Flow**:
1. Bridge disconnects (terminal closed)
   - Pane output switches to ring buffer mode (existing logic)
2. New bridge connects
   - Daemon detects existing session
   - Drains ring buffer and sends to new client (existing `send_reattach_data`)
   - Pane output switches back to connected mode

**Implementation Steps**:
1. **Verify reattach flow** — Test that `collect_reattach_data` and `send_reattach_data` work over Named Pipe transport (generified in Phase 1)
2. **Verify PTY spawning on Windows** — Confirm `portable-pty` spawns PowerShell correctly in mux context
3. **Add Windows CI job** — Build and run `cargo test` on Windows runner
4. **Integration test** — Daemon start → bridge connect → detach → reattach cycle

**Dependencies**: Phase 1-4 (all previous phases)

**Testing Approach**:
- Integration: Full lifecycle test (start → connect → detach → reattach → kill)
- CI: Windows GitHub Actions job

**Acceptance Criteria**:
- [ ] Reattach replays buffered output on Windows
- [ ] PTY spawning works with PowerShell
- [ ] Windows CI passes
- [ ] Linux CI continues to pass (no regressions)

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/
├── Cargo.toml                          # Add windows-sys dependency
└── src/
    └── mux/
        ├── daemon.rs                   # Add #[cfg(windows)] implementations
        ├── bridge.rs                   # Add #[cfg(windows)] implementations
        ├── cli.rs                      # Replace #[cfg(not(unix))] stubs
        └── ipc/
            ├── mod.rs                  # Remove #[cfg(unix)] from shared modules
            ├── connection.rs           # Generify stream type
            ├── handlers.rs             # Generify stream type
            ├── reattach.rs             # Generify stream type
            ├── statusbar.rs            # No changes (already agnostic)
            ├── pty_spawn.rs            # Already has Windows shell detection
            ├── codec.rs                # No changes (already agnostic)
            └── protocol.rs             # No changes (already agnostic)
```

## Testing Strategy
- Unit: Core logic (pipe path, console mode, handshake) — 80%+ coverage
- Integration: Daemon↔bridge↔CLI communication on Windows
- E2E (Docker): Existing Linux E2E tests for regression (`./scripts/run-e2e-docker.sh`)
- Manual: Interactive mux session on Windows (start, use, detach, reattach, kill)

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| windows-sys | 0.62+ | Console API (GetConsoleMode, SetConsoleMode, GetStdHandle) |
| tokio | existing | Named Pipe support (tokio::net::windows::named_pipe) |
| portable-pty | existing | PTY abstraction (already supports Windows) |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Named Pipe API differences from Unix sockets | Medium | Medium | Thorough testing of connection/disconnect semantics |
| Console API edge cases (piped stdin, non-console) | Low | Low | Graceful fallback when GetConsoleMode fails |
| tokio Named Pipe bugs | Low | High | Pin tokio version, test thoroughly |

## Open Questions
- [ ] Named Pipe security descriptor details (default ACL may be sufficient)

## Success Metrics
- [ ] All six mux CLI commands work on Windows
- [ ] Daemon survives terminal closure
- [ ] Reattach restores session state
- [ ] Windows CI green
- [ ] Linux CI green (no regressions)
