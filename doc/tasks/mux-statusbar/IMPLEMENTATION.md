# Implementation Plan: Mux Status Bar

## Overview

Enable the mux daemon to periodically execute user-configured commands, resolve template strings (`{cmd:name}`, `{hostname}`, `{cwd}`), and push the results to the GUI's status bar OSC layer via the StatusUpdate IPC message.

## Objectives

- Add `mux.statusbar` settings section with enabled flag, left/right templates, and command definitions
- Implement daemon-side command execution engine with independent per-command timers and 1-second render cycle
- Replace unused `StatusUpdateMsg` format and add `RequestStatusUpdate` message type
- Wire StatusUpdate display to OSC layer on frontend, with tab-switch and mux-exit cleanup
- Remove unused `MuxStatusBar` class

## Prerequisites

### Development Environment
- Rust toolchain (stable)
- Bun runtime
- Docker (for test execution)

### Dependencies
- Existing mux daemon infrastructure (`src-tauri/src/mux/`)
- Existing status bar OSC layer (`src/status-bar/osc-controller.ts`)
- Existing settings system (`src-tauri/src/commands/config/settings.rs`)

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri + standalone daemon)
- **Frontend**: Vanilla TypeScript
- **IPC**: APC-over-PTY (mux inband protocol, bincode serialization)

### Design Approach
Bottom-up: settings → protocol → daemon engine → frontend wiring → cleanup

### Component Interaction

```
Daemon (Rust)
  ├── Settings reader (reads settings.json at startup)
  ├── Command runner (per-command independent timers, async exec with timeout)
  ├── Template engine (resolves {cmd:name}, {hostname}, {cwd})
  └── Render timer (1s interval, diff-sends StatusUpdate)
        ↓ StatusUpdate (0x16)
Bridge (pass-through)
        ↓ APC / OSC 9999
GUI (TypeScript)
  ├── MuxClient (decodes StatusUpdate, callback)
  ├── main.ts (routes to OscLayerController)
  └── Tab switch handler (RequestStatusUpdate / clear)
```

## Implementation Phases

### Phase 1: Settings & Protocol Foundation

**Goal**: Define the data structures and message types that all other phases depend on.

**Files to Modify**:
- `src-tauri/src/commands/config/settings.rs` — Add `MuxStatusbarSettings` and `MuxStatusbarCommand` structs nested under `MuxSettings`
- `src-tauri/src/mux/ipc/protocol.rs` — Replace `StatusUpdateMsg` fields with `{ left, right }`, add `RequestStatusUpdate` (0x17) to `MessageType` enum and `from_u8`
- `src/settings/types.ts` — Add `MuxStatusbarSettings` and `MuxStatusbarCommand` interfaces, update `MuxSettings`
- `src/terminal/mux/mux-client.ts` — Update `MuxMessageType` constant to add `RequestStatusUpdate: 0x17`, rewrite `decodeStatusUpdateMsg` for new `{ left, right }` format, add `sendRequestStatusUpdate` method

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MuxStatusbarSettings` (Rust) | Deserialize `mux.statusbar` from JSON with defaults | Settings JSON exists | All fields have valid values or defaults |
| `MuxStatusbarCommand` (Rust) | Hold executable path and interval_ms | N/A | `interval_ms` defaults to 5000 |
| `StatusUpdateMsg` (Rust) | Carry resolved left/right strings | N/A | Serializable with bincode |
| `RequestStatusUpdate` (protocol) | GUI→Daemon empty-payload request | Message type 0x17 registered | Daemon can match on it |

**Implementation Steps**:
1. **Add Rust settings structs** — `MuxStatusbarSettings` with `enabled` (default false), `left`, `right`, `commands` fields; `MuxStatusbarCommand` with `executable`, `interval_ms` (default 5000). Add `statusbar` field to `MuxSettings` with `serde(default)`.
2. **Update StatusUpdateMsg** — Replace `session_name`, `window_names`, `active_window_index` with `left: String`, `right: String`.
3. **Add RequestStatusUpdate** — Add variant 0x17 to `MessageType` enum and its `from_u8` arm.
4. **Update TypeScript types** — Mirror the Rust settings structs in `types.ts`.
5. **Update MuxClient decoder and constants** — Rewrite `decodeStatusUpdateMsg` to decode two bincode strings (`left`, `right`). Add `RequestStatusUpdate` to `MuxMessageType`. Add `sendRequestStatusUpdate` method that sends type 0x17 with empty payload.

**Dependencies**: None (foundation phase)

**Testing Approach**:
- Unit: Settings deserialization with defaults, with full config, with missing fields
- Unit: StatusUpdateMsg bincode round-trip encode/decode (Rust)
- Unit: TypeScript `decodeStatusUpdateMsg` against known bincode bytes

**Acceptance Criteria**:
- [ ] `MuxStatusbarSettings::default()` has `enabled: false`, empty strings, empty commands
- [ ] `StatusUpdateMsg { left, right }` round-trips through bincode
- [ ] `MessageType::from_u8(0x17)` returns `Some(RequestStatusUpdate)`
- [ ] TypeScript decoder correctly parses Rust-encoded StatusUpdateMsg

**Estimated Effort**: small

---

### Phase 2: Daemon Status Bar Engine

**Goal**: Implement the daemon-side settings reader, command runner, template engine, and periodic StatusUpdate sender.

**Files to Create**:
- `src-tauri/src/mux/ipc/statusbar.rs` — Status bar engine: settings loading, command runner, template resolver, render timer

**Files to Modify**:
- `src-tauri/src/mux/ipc/mod.rs` — Add `statusbar` submodule (with `#[cfg(unix)]` gate)
- `src-tauri/src/mux/ipc/connection.rs` — Add status bar engine to `handle_connection` select! loop; handle `RequestStatusUpdate` in `route_message`
- `src-tauri/src/mux/ipc/pty_spawn.rs` — Add OSC 7 detection in `pty_reader_loop`, store cwd in shared state
- `src-tauri/src/mux/session/pane.rs` — Add `cwd: Arc<StdMutex<Option<String>>>` field to `MuxPane`
- `src-tauri/src/mux/tmux_import.rs` — Make `settings_file_path()` visibility `pub(crate)` so the statusbar module can reuse it

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `StatusBarEngine` | Orchestrate command execution, template resolution, and StatusUpdate generation | Settings loaded, connection active | Produces `StatusUpdateMsg` on render tick |
| Settings reader | Read and parse `mux.statusbar` from settings.json | File path resolved | Returns `MuxStatusbarSettings` or default on error |
| Command runner | Execute registered executables with independent timers and 5s timeout | Executable path validated | Caches stdout (first line, trimmed) per command |
| Template resolver | Replace `{cmd:name}`, `{hostname}`, `{cwd}` in template strings | Cached values available | Returns resolved string |
| OSC 7 detector | Scan PTY output bytes for OSC 7 pattern, extract path, update pane cwd | Runs in `pty_reader_loop` | `MuxPane.cwd` updated |

**Processing Flow** (StatusBarEngine lifecycle):
1. On GUI client connection (after Welcome handshake)
   - Read settings.json → parse `mux.statusbar`
   - If parse error → log warning, send StatusUpdate with error in `left` field
   - If not enabled → skip engine setup
2. If enabled: resolve `{hostname}` once (cached for session lifetime)
3. For each command in `commands` map: start independent async timer with `interval_ms`
4. Start render timer (1-second fixed interval)
5. On command timer tick → spawn process (no args, no shell), read stdout with 5s timeout, cache first line trimmed
6. On render timer tick → resolve templates with cached values + active pane cwd → compare with previous → if changed, produce StatusUpdateMsg
7. On `RequestStatusUpdate` from GUI → immediately resolve and send StatusUpdate

**Processing Flow** (OSC 7 detection in pty_reader_loop):
1. For each PTY output chunk, scan bytes for ESC ] 7 ; pattern
2. If found, extract path portion (between `file://host/` and ST terminator)
3. Update pane's shared cwd field

**Processing Flow** (Active pane tracking):
1. When `PtyInput` is received for a pane, record that pane_id as active
2. Template resolver reads active pane's cwd from its shared field

**Implementation Steps**:
1. **Make `settings_file_path()` pub(crate)** — Change visibility in `tmux_import.rs`
2. **Add cwd field to MuxPane** — Add `cwd: Arc<StdMutex<Option<String>>>` with a public getter/setter
3. **Add OSC 7 detection to pty_reader_loop** — After `shadow_parser.process(data)`, scan data for OSC 7 pattern, extract path, update `pane.cwd`
4. **Create statusbar.rs** — Implement settings reader (reuses `settings_file_path()`), command runner (async process spawn with timeout), template resolver (`{cmd:name}` / `{hostname}` / `{cwd}`), and render timer logic with differential send
5. **Integrate into connection.rs** — After Welcome, create `StatusBarEngine`. Add its render timer and command timers to the `select!` loop. Add `RequestStatusUpdate` arm to `route_message`. Track `active_pane_id` on `PtyInput`.
6. **Handle settings errors** — On file missing: create default file. On parse error: send StatusUpdate with error message in `left` field.

**Dependencies**: Requires Phase 1 (settings structs, protocol types)

**Testing Approach**:
- Unit: Template resolution with all variable types, unknown variables left as-is
- Unit: OSC 7 pattern detection from byte stream (various edge cases: split across chunks, no ST, embedded in other data)
- Unit: Command timeout behavior (mock process that sleeps)
- Unit: Settings file reading (missing file, invalid JSON, valid JSON)
- Unit: `~` expansion in executable path
- Integration: StatusBarEngine produces correct StatusUpdateMsg for given settings and cached values

**Acceptance Criteria**:
- [ ] Daemon reads `mux.statusbar` from settings.json at connection time
- [ ] Commands execute at their configured `interval_ms` independently
- [ ] Template `{cmd:name}` resolves to cached command stdout (first line, trimmed)
- [ ] Template `{hostname}` resolves to system hostname
- [ ] Template `{cwd}` resolves to active pane's OSC 7-cached cwd
- [ ] StatusUpdate sent only when resolved content changes (differential)
- [ ] Command timeout (5s) retains previous cached value
- [ ] Invalid settings → error message displayed in OSC layer
- [ ] Missing settings file → file created with defaults

**Estimated Effort**: large

---

### Phase 3: Frontend Wiring

**Goal**: Connect StatusUpdate reception to the OSC layer display, handle tab switching, and clean up on mux exit.

**Files to Modify**:
- `src/terminal-app/mux/mux-session.ts` — Register `onStatusUpdate` callback in `enterMuxMode`; clear OSC layer in `exitMuxMode`
- `src/main.ts` — In `tab:activated` handler, send `RequestStatusUpdate` if active tab is mux, or clear OSC layer if not; wire `onStatusUpdate` callback to `oscLayerController`

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| StatusUpdate callback | Route decoded StatusUpdate to OSC layer controller | MuxClient connected, callback registered | OSC layer shows left/right content |
| Tab switch handler | On tab activation, request fresh StatusUpdate or clear OSC layer | TabManager emits `tab:activated` | Correct status bar content for active tab |
| Exit cleanup | Clear OSC layer when mux mode ends | `exitMuxMode` called | OSC layer empty |

**Processing Flow** (StatusUpdate display):
1. In `enterMuxMode`: register `muxClient.setOnStatusUpdate(callback)` where callback calls `oscLayerController.handleCommand("set", "left", msg.left)` and `("set", "right", msg.right)`
2. The callback reference must be accessible from `main.ts` tab:activated handler

**Processing Flow** (Tab switch):
1. `tab:activated` event fires with `{ tab, previousTabId }`
2. Get `TerminalApp` for the new active tab
3. If app is in mux mode → call `muxClient.sendRequestStatusUpdate()`
4. If app is NOT in mux mode → call `oscLayerController.handleCommand("clear")`

**Processing Flow** (Mux exit cleanup):
1. In `exitMuxMode` (already handles state cleanup)
2. Add `oscLayerController.handleCommand("clear")` call

**Implementation Steps**:
1. **Register onStatusUpdate in enterMuxMode** — Wire the callback to push `left`/`right` to OSC layer controller
2. **Add tab:activated handler for mux status** — In `main.ts`, extend the existing `tab:activated` listener to check mux mode and either request StatusUpdate or clear OSC layer
3. **Add OSC clear in exitMuxMode** — Add clear call in `mux-session.ts` or via a callback from `main.ts`
4. **Wire onMuxStateChange for exit case** — In the existing `onMuxStateChange` callback in `main.ts`, when `windowCount === 0` (mux exited), clear OSC layer

**Dependencies**: Requires Phase 1 (MuxClient decoder/sender), Phase 2 (daemon sends StatusUpdate)

**Testing Approach**:
- Unit: `decodeStatusUpdateMsg` with valid and malformed data
- Manual: Verify status bar shows command output when mux mode is active
- Manual: Verify tab switch between mux and non-mux tabs updates/clears OSC layer
- Manual: Verify OSC layer clears on mux detach/exit

**Acceptance Criteria**:
- [ ] StatusUpdate from daemon appears in OSC layer (left and right sections)
- [ ] Switching to non-mux tab clears OSC layer
- [ ] Switching back to mux tab triggers RequestStatusUpdate and shows fresh data
- [ ] Exiting mux mode clears OSC layer
- [ ] Multiple mux tabs each show their own daemon's status on activation

**Estimated Effort**: small

---

### Phase 4: Cleanup & Validation

**Goal**: Remove dead code, run all tests, verify no regressions.

**Files to Delete**:
- `src/terminal/mux/status-bar.ts` — Unused `MuxStatusBar` class, superseded by OSC layer approach

**Files to Modify**:
- Any files importing from `status-bar.ts` — Remove dead imports

**Implementation Steps**:
1. **Remove MuxStatusBar** — Delete `src/terminal/mux/status-bar.ts` and clean up any imports
2. **Run TypeScript typecheck** — Verify no type errors from removed file
3. **Run Rust tests** — `cargo test` to verify protocol/settings changes
4. **Run TypeScript tests** — `bun test` to verify frontend changes
5. **Run E2E tests** — Verify no regressions in existing functionality

**Dependencies**: Requires Phase 3 (all functional code in place)

**Testing Approach**:
- Automated: Full Rust test suite, TypeScript test suite, typecheck
- E2E (Docker): Full existing E2E suite passes

**Acceptance Criteria**:
- [ ] `MuxStatusBar` class removed, no dead imports remain
- [ ] `cargo test` passes
- [ ] `bun test` passes
- [ ] `bun run typecheck` passes
- [ ] E2E tests pass without regression

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── commands/config/settings.rs    # + MuxStatusbarSettings, MuxStatusbarCommand
├── mux/
│   ├── ipc/
│   │   ├── mod.rs                 # + statusbar module declaration
│   │   ├── protocol.rs            # StatusUpdateMsg changed, RequestStatusUpdate added
│   │   ├── connection.rs          # + StatusBarEngine integration, RequestStatusUpdate handler
│   │   ├── statusbar.rs           # NEW: settings reader, command runner, template engine
│   │   └── pty_spawn.rs           # + OSC 7 detection in pty_reader_loop
│   ├── session/pane.rs            # + cwd field
│   └── tmux_import.rs             # settings_file_path() visibility change

src/
├── settings/types.ts              # + MuxStatusbarSettings, MuxStatusbarCommand
├── terminal/mux/
│   ├── mux-client.ts              # + RequestStatusUpdate, updated decoder
│   └── status-bar.ts              # DELETED
├── terminal-app/mux/mux-session.ts  # + onStatusUpdate registration, exit cleanup
└── main.ts                        # + tab:activated handler for mux status
```

## Testing Strategy

- **Unit (Rust)**: Settings deserialization, template resolution, OSC 7 parsing, command timeout, StatusUpdateMsg bincode round-trip. Target 80%+ coverage on new code.
- **Unit (TypeScript)**: `decodeStatusUpdateMsg` with valid/malformed data.
- **Integration**: StatusBarEngine produces correct output for given settings.
- **E2E (Docker)**: Existing E2E suite passes without regression.
- **Manual**: Visual verification of status bar display, tab switching, mux exit cleanup.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | — | All functionality uses existing crate/library dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| OSC 7 detection false positive in binary data | Low | Low | Pattern requires specific byte sequence including ST terminator |
| Command execution blocks daemon | Low | High | Async execution with 5s timeout, kill on timeout |
| Settings file race condition (GUI + daemon) | Low | Low | Daemon reads only at startup, does not write after creation |
| select! loop complexity with additional timers | Medium | Medium | StatusBarEngine encapsulates all timer logic, exposes single poll interface |

## Open Questions

None.

## Success Metrics

- [ ] All FR1-FR11 implemented and verified
- [ ] All unit tests pass
- [ ] No regression in existing E2E tests
- [ ] Security: only registered executables can be run
- [ ] Dead code (`MuxStatusBar`) removed
