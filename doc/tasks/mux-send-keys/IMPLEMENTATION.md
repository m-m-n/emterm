# Implementation Plan: mux send-keys CLI Command

## Overview

Add `emterm mux send-keys` subcommand that reads stdin and sends it as PtyInput to a target pane in the active mux session. Requires extending SessionInfo with per-window details for target resolution.

## Objectives

- Enable piping arbitrary data to mux windows from CLI
- Support window targeting by 0-based index via `-t` flag
- Default to active window when no target specified

## Prerequisites

### Development Environment

- Rust toolchain (existing)
- Bun (existing)

### Dependencies

- No new external dependencies required
- Internal: existing IPC protocol, cli_handshake pattern, PtyInput handler

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: Tauri (CLI mode, no GUI)
- **Key Libraries**: clap (CLI args), serde/bincode (IPC serialization)

### Design Approach

Follows the existing CLI command pattern: synchronous blocking I/O via cli_handshake, send one message, exit. The only structural change is enriching SessionInfo with per-window metadata so the CLI can resolve window index to pane ID.

### Component Interaction

```
stdin -> execute_send_keys -> cli_handshake -> Welcome(sessions with windows)
                           -> resolve pane_id from window index
                           -> send PtyInput(pane_id, data) -> daemon writes to PTY
```

## Implementation Phases

### Phase 1: Protocol Extension - WindowInfo and SessionInfo

**Goal**: Extend the IPC protocol so Welcome messages include per-window details (id, name, active_pane_id), enabling CLI commands to resolve window targets.

**Files to Modify**:
- `src-tauri/src/mux/ipc/protocol.rs` - Add WindowInfo struct, extend SessionInfo with windows field
- `src-tauri/src/mux/session/manager.rs` - Update session_list() to populate windows vec
- `src-tauri/src/mux/ipc/connection.rs` - No changes needed (already calls session_list())

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| WindowInfo | Per-window metadata for IPC | None | Serializable struct with id, name, active_pane_id |
| SessionInfo.windows | Window details in session info | SessionInfo exists | Contains ordered list of WindowInfo |
| session_list() | Build SessionInfo with window details | Session manager has sessions | Each SessionInfo includes populated windows vec |

**Implementation Steps**:
1. **Define WindowInfo** - New serializable struct in protocol.rs with id (u32), name (String), active_pane_id (u32)
2. **Extend SessionInfo** - Add `windows: Vec<WindowInfo>` field with serde(default) for backward compatibility
3. **Update session_list()** - Iterate windows in each session, build WindowInfo from MuxWindow fields (id, name, active_pane_id)
4. **Add unit tests** - WindowInfo serde roundtrip, extended SessionInfo backward compat (missing windows field), SessionInfo with windows roundtrip

**Dependencies**: None (foundation phase)

**Testing Approach**:
- Unit: Serialization roundtrip for WindowInfo, backward compatibility of SessionInfo without windows field

**Acceptance Criteria**:
- [ ] WindowInfo struct defined and serializable
- [ ] SessionInfo includes windows field with serde(default)
- [ ] session_list() populates windows from actual session state
- [ ] Existing protocol tests still pass

**Estimated Effort**: small

---

### Phase 2: CLI Subcommand and Execution

**Goal**: Add `send-keys` subcommand to clap and implement execute_send_keys() that reads stdin, connects to daemon, resolves target pane, and sends PtyInput.

**Files to Modify**:
- `src-tauri/src/main.rs` - Add send-keys subcommand definition with -t/--target option, dispatch to execute_send_keys
- `src-tauri/src/mux/cli.rs` - Add execute_send_keys() function (unix and non-unix variants)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| send-keys subcommand (clap) | Parse -t/--target option | mux subcommand exists | Accepts optional target index |
| execute_send_keys() | Read stdin, handshake, resolve pane, send PtyInput | Daemon running, valid target | Data written to target PTY, exit 0 |

**Processing Flow**:
1. Read all of stdin into byte buffer
   - Empty stdin (0 bytes) -> exit 0 immediately without connecting
2. Call cli_handshake() to get stream and sessions
   - Daemon not running -> error "No mux daemon running", exit 1
3. Find first session from sessions list (active session)
   - No sessions -> error "No active session", exit 1
4. Resolve target window
   - With -t index: validate index is within windows vec bounds -> error if out of range
   - Without -t: use active_window_index to index into windows vec
5. Extract active_pane_id from resolved WindowInfo
   - Pane ID is 0 (no active pane) -> error "No active pane in window N", exit 1
6. Send PtyInput(pane_id, stdin_data) via stream
7. Exit 0

**Implementation Steps**:
1. **Add clap subcommand** - Define send-keys under mux with -t/--target as optional u32 argument
2. **Add dispatch** - Match "send-keys" in main.rs, extract target option, call execute_send_keys
3. **Implement execute_send_keys (unix)** - Follow cli_handshake + single message pattern from execute_new_window, with stdin reading and target resolution logic
4. **Implement execute_send_keys (non-unix stub)** - Print platform unsupported message, exit 1
5. **Error handling** - Map all error conditions to stderr messages and exit code 1 per spec error table

**Dependencies**: Requires Phase 1 (WindowInfo in Welcome message)

**Testing Approach**:
- Unit: CLI argument parsing (send-keys with no args, with -t 0, with -t 5)
- Integration: Target resolution logic (active window default, index lookup, out-of-range)

**Acceptance Criteria**:
- [ ] `emterm mux send-keys --help` shows correct usage
- [ ] `printf 'text\r' | emterm mux send-keys -t 0` sends data to window 0
- [ ] `printf 'text\r' | emterm mux send-keys` sends to active window
- [ ] Empty stdin exits 0 without connecting
- [ ] Out-of-range index gives clear error and exit 1
- [ ] Daemon not running gives clear error and exit 1

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
  main.rs                        # Add send-keys subcommand + dispatch
  mux/
    cli.rs                       # Add execute_send_keys() (unix + non-unix)
    ipc/
      protocol.rs                # Add WindowInfo, extend SessionInfo
      connection.rs              # No changes (Welcome already uses session_list)
    session/
      manager.rs                 # Update session_list() to include windows
```

## Testing Strategy

- Unit: Protocol serde roundtrips, backward compatibility, argument parsing
- Integration: Target resolution logic, error conditions
- E2E (Docker): Existing mux E2E tests pass without regression

## Dependencies

No new external dependencies.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| bincode backward compat with new SessionInfo field | Low | Medium | serde(default) on windows field ensures old clients handle new format |
| Blocking stdin read hangs if no data piped | Low | Low | stdin from pipe reaches EOF; interactive stdin is user's responsibility |

## Open Questions

None. All requirements are resolved in the specification.

## Success Metrics

- [ ] All functional requirements FR1-FR6 implemented
- [ ] All error cases produce correct stderr messages and exit codes
- [ ] Existing mux tests pass without regression
- [ ] Cross-compilation succeeds (Linux + Windows)
