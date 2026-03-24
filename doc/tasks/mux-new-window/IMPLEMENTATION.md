# Implementation Plan: mux new-window CLI Command

## Overview

`emterm mux new-window` サブコマンドを追加し、CLI からアクティブな mux セッションに新しいウィンドウを作成する。オプションでウィンドウ名 (`-n`) と初期コマンド (`-c`) を指定可能。

## Objectives

- CLI からスクリプトで複数ウィンドウを作成できるようにする
- IPC プロトコルの CreateWindow メッセージにペイロードを追加する
- ダイアモン側で初期コマンドを PTY に書き込む

## Prerequisites

### Development Environment
- Rust toolchain (existing project setup)
- Docker (for testing)

### Dependencies
- No new external dependencies required
- All changes use existing crate dependencies (clap, serde, bincode, tokio)

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: Tauri (CLI side uses clap for argument parsing)
- **Key Libraries**: clap (CLI), bincode/serde (IPC serialization), portable-pty (PTY)

### Design Approach
- Extend the existing CLI/IPC pattern established by `mux ls` and `mux kill`
- CLI client performs handshake, sends one control message, receives response, exits
- Daemon handler extends existing `handle_create_window` to accept structured payload
- Backward compatible: empty payload defaults to current behavior (name="shell", no command)

### Component Interaction

```
CLI (emterm mux new-window -n editor -c "nvim")
  |
  v
cli_handshake() -> UnixStream + session list
  |
  v
Send CreateWindow(CreateWindowPayload { name, command })
  |
  v
Daemon: handle_create_window
  |-- spawn_pty()
  |-- create_window(name)
  |-- register_pane_and_start_reader()
  |-- write command + "\n" to PTY (with delay)
  |-- send PaneCreated response
  |-- send StatusUpdate to GUI
  v
CLI: receive PaneCreated -> exit 0
```

## Implementation Phases

### Phase 1: IPC Protocol Extension

**Goal**: CreateWindow メッセージがウィンドウ名とコマンドを運べるようにする

**Files to Modify**:
- `src-tauri/src/mux/ipc/protocol.rs` - CreateWindowPayload 構造体を追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| CreateWindowPayload | Window name and command serialization | Valid serializable fields | Roundtrip serialization preserves data |

**Implementation Steps**:
1. **Define CreateWindowPayload** - name (optional string) and command (optional string) fields, serializable
2. **Add unit tests** - Serialization roundtrip for all field combinations (both None, name only, command only, both present)

**Dependencies**: None (standalone data structure)

**Testing Approach**:
- Unit: Roundtrip serialization tests for all field combinations

**Acceptance Criteria**:
- [ ] CreateWindowPayload serializes/deserializes correctly with bincode
- [ ] Empty payload deserialization returns defaults (backward compat)

**Estimated Effort**: small

---

### Phase 2: Daemon Handler Extension

**Goal**: handle_create_window がペイロードを受け取り、ウィンドウ名を設定し、初期コマンドを PTY に書き込む

**Files to Modify**:
- `src-tauri/src/mux/ipc/handlers.rs` - handle_create_window にペイロードデコード、名前設定、コマンド書き込みを追加
- `src-tauri/src/mux/ipc/connection.rs` - handle_create_window 呼び出しにメッセージを渡す

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handle_create_window | Decode payload, use name for window, write command to PTY | Valid CreateWindow message received | Window created with specified name; command written to PTY if present |

**Processing Flow**:
1. Decode CreateWindowPayload from message
   - Payload empty or decode fails -> use defaults (name="shell", command=None)
   - Payload decoded -> extract name and command
2. Spawn PTY (existing logic, unchanged)
3. Create window with specified name (or "shell" if empty/None)
4. Register pane and start reader (existing logic)
5. If command is present and non-empty:
   - Wait short delay (~50ms) for shell readiness
   - Write command + "\n" to pane's PTY writer
6. Send PaneCreated response
7. Send StatusUpdate to GUI clients

**Implementation Steps**:
1. **Update handle_create_window signature** - Accept the MuxMessage to decode payload
2. **Decode CreateWindowPayload** - Extract name and command, fallback to defaults for backward compatibility
3. **Use window name from payload** - Pass to create_window instead of hardcoded "shell"
4. **Write initial command to PTY** - After pane registration, with short delay for shell readiness
5. **Update route_message** - Pass full message to handle_create_window instead of discarding it

**Dependencies**: Requires Phase 1 (CreateWindowPayload)

**Testing Approach**:
- Unit: Payload decode fallback behavior (empty payload -> defaults)
- Integration: Window creation with name, command injection timing

**Acceptance Criteria**:
- [ ] Window created with name from payload
- [ ] Default name "shell" when no name provided
- [ ] Command written to PTY when specified
- [ ] Empty command string treated as no command
- [ ] Backward compatible with payloadless CreateWindow from GUI

**Estimated Effort**: medium

---

### Phase 3: CLI Client Extension

**Goal**: `emterm mux new-window` コマンドを追加し、CLI クライアントが CreateWindow メッセージを送受信できるようにする

**Files to Modify**:
- `src-tauri/src/main.rs` - new-window サブコマンド定義と dispatch を追加
- `src-tauri/src/mux/cli.rs` - execute_new_window 関数を追加
- `src-tauri/src/mux/ipc/connection.rs` - CLI クライアントがハンドシェイク後に1つのコントロールメッセージを送受信できるように拡張

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| new-window subcommand (main.rs) | Parse -n/--name and -c/--command args | Valid CLI arguments | Arguments passed to execute_new_window |
| execute_new_window (cli.rs) | Connect, handshake, send CreateWindow, receive PaneCreated | Daemon running | Window created, exit 0 |
| CLI message handling (connection.rs) | Allow CLI client to send/receive one control message after handshake | CLI Hello received | Process one message then disconnect |

**Processing Flow**:
1. Parse CLI arguments (-n, -c)
2. Call cli_handshake() to connect and get session list
3. Build CreateWindowPayload with name and command
4. Send CreateWindow message with payload (session_id from first session)
5. Read response
   - PaneCreated -> exit 0
   - Error -> print error to stderr, exit 1
   - Timeout (5s) -> error exit
6. Connection closed

**Implementation Steps**:
1. **Add new-window subcommand to clap definition** - With -n/--name and -c/--command optional arguments
2. **Add dispatch in main.rs** - Route "new-window" to execute_new_window
3. **Implement execute_new_window** - Handshake, build payload, send CreateWindow, await PaneCreated
4. **Extend daemon CLI client handling** - After Welcome, read one control message, process it, then disconnect (instead of immediate disconnect)
5. **Add Windows stub** - cfg(not(unix)) version returning platform error

**Dependencies**: Requires Phase 1 and Phase 2

**Testing Approach**:
- Unit: CLI argument parsing for all option combinations
- Integration: Full flow (CLI -> daemon -> window creation)
- E2E (Docker): Existing mux E2E tests pass without regression

**Acceptance Criteria**:
- [ ] `emterm mux new-window` creates a window
- [ ] `-n editor` sets window name
- [ ] `-c "nvim"` runs command in new window
- [ ] Error message on stderr when daemon not running
- [ ] Exit code 0 on success, 1 on failure
- [ ] `emterm mux new-window --help` shows correct usage

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
  main.rs                          # Add new-window subcommand + dispatch
  mux/
    cli.rs                         # Add execute_new_window()
    ipc/
      protocol.rs                  # Add CreateWindowPayload struct
      handlers.rs                  # Extend handle_create_window for payload
      connection.rs                # Extend CLI client to handle one control message
```

## Testing Strategy

- **Unit**: CreateWindowPayload serialization roundtrip, CLI argument parsing
- **Integration**: Full CLI -> daemon -> window creation flow (requires running daemon)
- **E2E (Docker)**: `./scripts/run-e2e-docker.sh test` - existing tests pass without regression
- **Manual**: `emterm mux new-window -n editor -c "nvim"` in running emterm

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | - | No new dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| CLI client handling race in daemon | Low | Medium | Single message + disconnect pattern, timeout on read |
| Command injection timing (shell not ready) | Medium | Low | 50ms delay before writing; acceptable since shell startup is fast |
| Backward compatibility break | Low | High | Empty payload defaults to current behavior; GUI sends CreateWindow without payload |

## Open Questions

None. All requirements are resolved.

## Success Metrics

- [ ] All functional requirements (FR1-FR4) implemented
- [ ] Unit tests pass for serialization and argument parsing
- [ ] Existing E2E tests pass without regression
- [ ] Linux and Windows compilation succeeds
- [ ] Command completes within 1 second (NFR1)
