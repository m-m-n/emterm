# Feature: Terminal Multiplexer

## Overview

Native terminal multiplexer integrated into eMterm, eliminating the VT100 double-parse bottleneck that degrades performance when using tmux with high-throughput tools like Claude Code. The daemon relays raw PTY bytes via IPC to the GUI, where existing WASM parsers process them directly — removing the virtual terminal regeneration layer entirely.

Each mux window holds a single pane. Pane splitting, pane navigation, pane zoom, and copy mode are not part of this specification.

## Objectives

- Eliminate the VT100 double-parse bottleneck present in tmux/Zellij/Screen
- Maintain near-native eMterm performance in multiplexer mode
- Preserve OSC extensions (Markdown/image display) through multiplexer
- Provide tmux-compatible user experience for easy migration

## User Stories

### US1: Start Multiplexer Session
As an eMterm user, I want to run `emterm mux` to start a multiplexer session, so that I can manage multiple terminal windows.

**Acceptance Criteria:**
- [ ] `emterm mux` starts a daemon (if not running) and switches GUI to mux mode
- [ ] A new session with one window (one pane) is created
- [ ] The pane displays a shell prompt

### US2: Detach and Reattach
As an eMterm user, I want to detach from a session and reattach later, so that long-running processes survive GUI closure.

**Acceptance Criteria:**
- [ ] `prefix + d` detaches and returns to shell
- [ ] Daemon and PTY sessions survive after detach
- [ ] `emterm mux attach` restores the session with full screen state
- [ ] PTY output during detach is captured and replayed on reattach

### US3: Manage Windows
As an eMterm user, I want to create and switch between windows, so that I can organize my work into groups.

**Acceptance Criteria:**
- [ ] `prefix + c` creates a new window
- [ ] `prefix + n` / `prefix + p` navigates between windows
- [ ] Windows appear as sub-tabs in the mux tab group
- [ ] `prefix + ,` renames a window

### US4: Migrate from tmux
As a tmux user, I want to import my tmux.conf settings, so that my keybindings and preferences carry over.

**Acceptance Criteria:**
- [ ] On first mux startup, `~/.tmux.conf` is automatically parsed and settings imported to `settings.json` `mux` section
- [ ] Unsupported settings produce warnings (logged)
- [ ] Prefix key, keybindings, base-index, mouse, status-position are converted
- [ ] Imported keybindings are editable in the settings panel

## Technical Requirements

### Functional Requirements

- **FR1: Daemon Process** — Background daemon manages PTY sessions, communicates with GUI via Unix domain socket. Auto-starts on first `emterm mux`, auto-exits when all sessions end.
- **FR2: IPC Protocol** — Length-prefixed binary frames over Unix domain socket. PTY data as raw bytes (no serialization), control messages via bincode.
- **FR3: Session Management** — Session > Window > Pane hierarchy with actor-model Session Manager. One pane per window. Cascade: PTY exit → pane close → window close → session end → daemon exit.
- **FR4: OSC Signaling** — `OSC 777 ; emterm ; mux ; attach ; <socket_path> ; <session_id> ST` and `OSC 777 ; emterm ; mux ; detach ST` for CLI→GUI mode switching.
- **FR5: GUI Mode Switching** — Tab integration: mux tab group with sub-tabs for windows. Normal tabs and mux tab groups coexist. AtomicBool flag pauses original PTY reader during mux mode. On window switch, GUI requests a fresh snapshot from the daemon so the target pane's display matches the daemon authoritative state (no reliance on client-side incremental buffering).
- **FR6: Snapshot-Based State Sync** — Three scenarios use the same per-pane `shadow_parser` (vt100::Parser) as the authoritative screen source: (a) detach — GUI serializes WASM grid state on detach for future reattach; (b) reattach — daemon replays shadow parser screen + ring buffer delta; (c) window switch — daemon replays shadow parser screen only (no delta). Per-pane ring buffer (64MB) accumulates PTY output while detached. Atomic attach process prevents data loss.
- **FR7: Window Management** — Multiple windows per session. Each window has exactly one pane. Tab group UI with auto-expand (active) / auto-compact (inactive, shows "mux (N)"). 0.3s animation for expand/compact.
- **FR8: Status Bar** — HTML-rendered status bar. Daemon pushes state changes (session name, window list). Event-driven (no polling). Local info (clock) managed by GUI.
- **FR9: Clipboard Paste** — `prefix + ]` pastes from the system clipboard into the active pane's PTY. No tmux buffer abstraction. Daemon not involved.
- **FR10: tmux.conf Conversion** — Regex-based line-oriented parser. Converts prefix, keybindings, base-index, mouse, status-position, default-terminal. Ignores if-shell, run-shell, plugins, format strings, set-hook with warnings. Unsupported keybinding actions (split, zoom, copy-mode, etc.) are logged and skipped.
- **FR11: Prefix Key** — Processed in GUI (TypeScript). Default `Ctrl+b`. Loaded from local settings (`settings.json` mux section), configurable via settings UI.
- **FR12: Flow Control** — Bounded channel (capacity 256) + adaptive batching (4ms accumulation window). Per-pane independent channels with round-robin select! consumption. Backpressure chain: socket buffer → channel → PTY reader → kernel buffer → process write().
- **FR13: Environment Variables** — `TERM_PROGRAM=emterm` and `TERM_PROGRAM_VERSION=<version>` set by GUI on PTY startup. `EMTERM_MUX=1` and `EMTERM_MUX_SOCKET=<path>` set by daemon in PTY environment. Nesting prevention via `EMTERM_MUX` check.
- **FR14: WASM Instance Management** — Independent WASM instance per pane (grid, parser state, cursor). ~2-4MB per pane. Module compilation cached, only instantiation per pane.

### Non-Functional Requirements

- **NFR1 - Performance:** No perceptible latency degradation compared to normal mode. Raw bytes transfer (no serialization/compression for PTY data). Adaptive batching for high-throughput output. Per-pane backpressure isolation.
- **NFR2 - Security:** Socket path validation (allowed directories only, no path traversal). File permission protection on Unix socket. No authentication needed (local-only).
- **NFR3 - Reliability:** Automatic recovery on daemon crash (GUI returns to normal mode). IPC disconnect retry with fallback. Snapshot deserialization failure results in empty screen reattach.
- **NFR4 - Compatibility:** Linux and Windows support. Windows AF_UNIX requires Windows 10 1803+. tmux-compatible keybindings at user operation level for the subset of actions this specification retains.
- **NFR5 - Resource Usage:** Per-pane ring buffer capped at 64MB. WASM instances ~2-4MB each. IPC frame max 16MB.

## Implementation Approach

### Architecture

**Overall Structure:**

```
┌─────────────────────────────────────────────────┐
│  eMterm GUI (Tauri)                              │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐    │
│  │ Window 1  │  │ Window 2  │  │ Window 3  │    │
│  │Canvas+WASM│  │Canvas+WASM│  │Canvas+WASM│    │
│  └───────────┘  └───────────┘  └───────────┘    │
│  Multiplexer Client (OSC, prefix key, tab group) │
└──────────────────┬──────────────────────────────┘
                   │ IPC (Unix socket)
┌──────────────────┴──────────────────────────────┐
│  emterm mux daemon                               │
│  Session Manager (actor model)                   │
│  ┌─────┐ ┌─────┐ ┌─────┐                       │
│  │PTY 1│ │PTY 2│ │PTY 3│  + Snapshot Store      │
│  └─────┘ └─────┘ └─────┘  + Ring Buffers        │
└─────────────────────────────────────────────────┘
```

**Three-Party Communication:**

- CLI → GUI: OSC sequences (via stdout/PTY)
- GUI ↔ Daemon: IPC socket (binary protocol, data + control)
- Daemon → CLI: IPC socket (control messages only, detach notification)

**Daemon Internal Architecture:**

Output Path (PTY → Client):
- Per-pane PTY reader → bounded channel → Mux Writer task (select! round-robin) → IPC socket

Input Path (Client → PTY):
- IPC socket → Demux Reader task → per-pane PTY writer

Session Manager: actor model (single tokio task + message-driven) for sequential state mutation.

### Data Flow

**Normal Mode (existing):**
```
Keyboard → Tauri IPC → PTY write (synchronous, lock-free)
PTY read → Binary Channel → WASM → Canvas
```

**Multiplexer Mode:**
```
Keyboard → Tauri internal → IPC(socket) → daemon → PTY write
PTY read → daemon → IPC(socket) → Tauri internal → WASM → Canvas
```

### IPC Protocol

**Transport:**
- Linux: Unix domain socket
- Windows: Unix domain socket (AF_UNIX, Windows 10 1803+)

**Frame Format:**
```
[length: u32][type: u8][pane_id: u32][payload: variable]
```
- length: remaining bytes after the length field itself (= 5 + payload_len)
- type: message type identifier
- pane_id: pane identifier for multiplexing PTY data
- payload: raw bytes (PTY data) or bincode (control messages)

**Handshake:**
1. Client → Daemon: `Hello { client_type: GUI | CLI, protocol_version: u32 }`
2. Daemon → Client: `Welcome { sessions: [...], server_version: u32 }` or `Rejected { reason }`

**Message Types:**

| Type | Name | Direction | Payload | Purpose |
|------|------|-----------|---------|---------|
| 0x01 | PtyOutput | D→G | raw bytes | PTY output data |
| 0x02 | PtyInput | G→D | raw bytes | Keyboard input |
| 0x03 | Hello | C→D | bincode | Handshake |
| 0x04 | Welcome | D→C | bincode | Handshake response |
| 0x05 | CreatePane | G→D | bincode | Pane creation request |
| 0x06 | PaneCreated | D→G | bincode | Pane creation notification |
| 0x07 | DestroyPane | G→D | bincode | Pane termination |
| 0x08 | Resize | G→D | bincode (cols, rows) | Pane resize |
| 0x09 | Attach | C→D | bincode (session_id) | Session attach |
| 0x0A | Detach | G→D | bincode | Detach request |
| 0x0B | Detached | D→C | bincode | Detach notification (CLI exit) |
| 0x0C | Snapshot | G→D | bincode | Grid state save |
| 0x0D | SnapshotRestore | D→G | bincode | Grid state restore |
| 0x0E | SessionList | D→C | bincode | Session listing |
| 0x0F | Error | D→C | bincode (message) | Error notification |
| 0x10 | PtyExited | D→G | bincode (exit_code) | PTY process exit |
| 0x12 | CreateWindow | G→D | bincode | Window creation request |
| 0x13 | SwitchWindow | G→D | bincode | Switch active window |
| 0x14 | RenameWindow | G→D | bincode | Rename window |
| 0x15 | DestroyWindow | G→D | bincode | Window destruction |
| 0x16 | StatusUpdate | D→G | bincode | Status bar update |
| 0x17 | RequestStatusUpdate | G→D | empty | Request status refresh |
| 0x18 | Shutdown | C→D | empty | Daemon shutdown request |
| 0x19 | RequestPaneSnapshot | G→D | empty | Request on-demand screen replay for the given pane |

**RequestPaneSnapshot Response:**
The daemon answers by emitting a single `PtyOutput (0x01)` frame for the requested pane. The payload is `\x1b[H\x1b[2J` followed by `shadow_parser.screen().contents_formatted()`, which is self-contained ANSI sufficient to reproduce the current screen (alt-screen toggle, SGR state, cursor position, etc.). The client's normal PtyOutput path consumes the frame, so no separate `PaneSnapshot` message type is required.

### Snapshot and State Sync

**Authoritative Source:** Every connected pane has a daemon-side `shadow_parser: Arc<StdMutex<vt100::Parser>>` that observes all PTY output. The daemon treats this parser's `screen().contents_formatted()` output as the source of truth for visual state reconstruction — it is self-contained ANSI that reproduces the exact screen on replay.

**Detach Flow:**
1. GUI serializes WASM grid state → sends Snapshot to daemon
2. Daemon saves snapshot, starts ring buffer accumulation

**Reattach Flow:**
1. GUI connects, sends Attach request
2. Daemon locks ring buffer writes (PTY readers wait on channel)
3. Daemon sends, per pane: `\x1b[H\x1b[2J` + `contents_formatted()` + ring buffer delta
4. GUI feeds the bytes through its WASM parser → grid reaches the exact daemon state
5. Daemon unlocks ring buffer, resumes streaming

**Window Switch Flow (on-demand snapshot):**
1. GUI user clicks a mux sub-tab (or invokes prefix+n/p)
2. GUI sends `RequestPaneSnapshot(paneId)` to the daemon
3. Daemon responds with a single `PtyOutput` frame: `\x1b[H\x1b[2J` + `shadow_parser.screen().contents_formatted()`
4. GUI's normal PtyOutput pipeline processes the bytes into the target pane's active WASM grid
5. The grid is guaranteed to match the daemon screen state; any stale client-side buffered state for the target pane is implicitly overwritten by the reset-and-replay bytes

This flow eliminates the reliance on accumulating incremental PTY output into a client-side saved grid while a pane is inactive. The display is always reconciled from the daemon shadow parser on switch, which guarantees "the visible screen matches the authoritative screen" without depending on every inactive-path byte having been correctly routed to the right client-side grid.

**Snapshot Contents (Detach Flow — client-serialized):**
- Visible screen (rows x cols cell data)
- Scrollback (up to configured limit)
- Cursor position and attributes (including shape)
- Terminal mode state (DEC modes, alternate buffer)
- Current character attributes (SGR state)
- Tab title (OSC 0/2)
- Current directory (OSC 7)

**Snapshot Contents (Reattach and Window Switch — daemon shadow_parser):**
The daemon-side `vt100::Parser::screen().contents_formatted()` produces self-contained ANSI that includes alt-screen enter/leave, SGR attributes, cursor position, and cell contents. The client does not need a separate deserializer — the bytes go through the existing PTY output parser (WASM).

**Ring Buffer:** Per-pane, 64MB cap. Overwrites oldest data. Used only for detach flow delta capture. Not used for window switch (the shadow parser already holds the latest screen).

### Session Hierarchy

```
Session
├── Window 1
│   └── Pane
└── Window 2
    └── Pane
```

Each window owns exactly one pane. The pane's PTY is tied to the window lifecycle.

### GUI Mode Switching

**Tab Integration (browser tab group style):**
- Mux-originated tab transforms into a tab group
- Mux windows expand as sub-tabs within the group
- Normal tabs and mux tab groups coexist

**Navigation (2 layers):**
- eMterm tab layer: Ctrl+digit, Ctrl+PageUp/Down
- Mux window layer: prefix+n/p + mouse click

**Expand/Compact:**
- Active mux tab: auto-expand (window tabs visible)
- Inactive mux tab: auto-compact (shows "mux (N)")
- 0.3s animation
- Setting: `mux_tab_always_expand` (default: off)

**Original PTY reader:** AtomicBool flag suppresses data forwarding to GUI during mux mode. Thread stays alive.

**Canvas/DOM:** Per-window Canvas elements created dynamically. Mux Canvases destroyed on detach, original tab display restored.

**Window Switch State Sync:** When the user switches between mux windows (sub-tab click, `prefix+n/p`, or CLI `emterm mux switch-window`), the GUI sends `RequestPaneSnapshot(paneId)` to the daemon in addition to `SwitchWindow`. The daemon replies with a `PtyOutput` frame containing `\x1b[H\x1b[2J` + `contents_formatted()` from the target pane's `shadow_parser`. The GUI pipeline processes these bytes into the active WASM grid, guaranteeing the displayed screen matches the daemon state. This replaces reliance on client-side incremental state accumulation for inactive panes (which is fragile across buffer-switch edge cases and potential races between `flushPtyPendingData` and in-flight PtyOutput messages).

### Dependencies

**Internal Dependencies:**
- WASM module: ANSI parser, grid, Unicode processing (reused per-pane)
- Canvas 2D renderer: differential drawing (reused per-pane)
- PTY management: portable-pty (reused in daemon)
- Settings system: serde + TypeScript mirror (extended with `mux` section)
- Tab bar: extended with tab group support
- OSC handler: extended with `mux` type

**External Dependencies (New):**
- `tokio` + `tokio-util::codec::LengthDelimitedCodec`: async runtime + framing
- `interprocess`: Unix socket abstraction with tokio integration (fallback to raw tokio if v2 issues)
- `bincode`: control message serialization

### File Structure

```
src-tauri/
├── src/
│   ├── mux/
│   │   ├── mod.rs              # Module root
│   │   ├── daemon.rs           # Daemon process entry point
│   │   ├── ipc/
│   │   │   ├── mod.rs
│   │   │   ├── protocol.rs     # Frame format, message types
│   │   │   ├── codec.rs        # LengthDelimited codec
│   │   │   └── connection.rs   # Socket connection handling
│   │   ├── session/
│   │   │   ├── mod.rs
│   │   │   ├── manager.rs      # Session manager (actor model)
│   │   │   ├── session.rs      # Session state
│   │   │   ├── window.rs       # Window state
│   │   │   └── pane.rs         # Pane state + PTY
│   │   ├── snapshot.rs         # Snapshot store
│   │   ├── ring_buffer.rs      # Per-pane ring buffer
│   │   └── cli.rs              # CLI subcommands (mux, mux attach, mux ls, mux kill, mux new)
│   └── ...
src/
├── terminal/
│   ├── mux/
│   │   ├── index.ts            # Mux client entry point
│   │   ├── mux-client.ts       # IPC client (Tauri channel)
│   │   ├── prefix-key.ts       # Prefix key handler
│   │   ├── tab-group.ts        # Tab group UI
│   │   └── mux-logger.ts       # Mux-scoped logger
│   └── ...
```

### Settings Schema

Added to existing `settings.json`:
```json
{
  "mux": {
    "prefix": "ctrl+b",
    "base_index": 0,
    "mouse": true,
    "status_position": "bottom",
    "tab_always_expand": false,
    "keybinds": {}
  }
}
```

Follows existing Settings Pattern: Rust `serde(default)` + TS `AppSettings` mirror. Unknown keys under `mux.keybinds` are silently dropped by serde.

### Environment Variables

| Variable | Set by | Purpose |
|----------|--------|---------|
| `TERM_PROGRAM=emterm` | eMterm GUI (PTY startup) | CLI detects eMterm environment |
| `TERM_PROGRAM_VERSION=<version>` | eMterm GUI (PTY startup) | Minimum version check |
| `EMTERM_MUX=1` | Daemon (PTY environment) | Nesting prevention |
| `EMTERM_MUX_SOCKET=<path>` | Daemon (PTY environment) | Socket path propagation |

## Test Scenarios

### Unit Tests
- [ ] IPC frame encoding/decoding round-trip
- [ ] Ring buffer write/read with overflow
- [ ] Snapshot serialization/deserialization
- [ ] Socket path validation (allowed dirs, path traversal rejection)
- [ ] Prefix key state machine
- [ ] tmux.conf parser (keybindings, options, unsupported directives)

### Integration Tests
- [ ] Daemon startup and socket creation
- [ ] IPC handshake (Hello → Welcome/Rejected)
- [ ] Full IPC message exchange (PtyInput → PtyOutput round-trip)
- [ ] Session lifecycle (create → use → destroy → daemon exit)
- [ ] Backpressure chain (bounded channel saturation)
- [ ] Graceful shutdown (SIGTERM handling)
- [ ] Stale socket detection and cleanup

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (26 specs)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] Daemon start → attach → type command → see output → detach → reattach → verify state
- [ ] Window create → switch → rename → close
- [ ] Paste from clipboard into active pane
- [ ] Daemon crash → GUI auto-recovery to normal mode

### Edge Cases
- [ ] Non-eMterm environment: `emterm mux` shows error and exits
- [ ] Nesting: `emterm mux` inside mux session shows error
- [ ] Max frame length exceeded (16MB)
- [ ] Ring buffer overflow during extended detach (64MB cap, oldest data overwritten)
- [ ] Snapshot deserialization failure (empty screen reattach)
- [ ] Concurrent attach (second GUI evicts first)
- [ ] Window resize during mux mode (per-pane Resize messages)

### Performance Tests
- [ ] High-throughput output (seq 1 1000000): no perceptible degradation vs normal mode
- [ ] Multi-window: high-throughput window doesn't starve others
- [ ] Reattach time: snapshot + 64MB delta replay under 2 seconds

## Security Considerations

- **Socket Path Validation:** GUI only accepts socket paths under allowed directories (`$XDG_RUNTIME_DIR/emterm/` or `~/.local/run/emterm/` on Linux, `%LOCALAPPDATA%\emterm\` on Windows). Paths containing `../` are rejected.
- **File Permissions:** Unix socket is protected by filesystem permissions.
- **No Authentication:** Local-only socket; no need for authentication tokens.
- **OSC Injection:** Socket path in OSC sequence is validated before use.

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Daemon startup failure | CLI shows error message, exits |
| IPC connection loss (daemon alive) | GUI retries several times → falls back to normal mode + toast notification |
| Daemon crash | GUI detects socket disconnect → toast notification → auto-return to normal mode |
| PTY process abnormal exit (in mux) | PtyExited sent to GUI, pane closes, window closes |
| Snapshot deserialization failure | Discard snapshot, reattach with empty screen |
| Protocol version mismatch | Daemon sends Rejected, client shows error |
| Stale socket file | Connect attempt fails → delete socket → create new |
| Unknown IPC message type from legacy client | Daemon logs a warning and discards the frame |

## Performance Optimization

### Performance Goals
- Key-to-echo latency: same as normal mode (no perceptible IPC overhead)
- Throughput: handle 4,000-6,700 scrolls/second (Claude Code workload) without UI jitter
- Reattach: snapshot + delta replay in under 2 seconds for 64MB buffer

### Optimization Strategies
- Raw bytes transfer for PTY data (no serialization, no compression)
- Per-pane bounded channels with round-robin consumption (no starvation)
- Adaptive batching (4ms accumulation window) for high-throughput output
- WASM module compilation cache (compile once, instantiate per pane)

## Success Criteria

- [ ] All functional requirements (FR1-FR14) are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals (no degradation vs normal mode)
- [ ] Security requirements are satisfied (socket path validation)
- [ ] Linux and Windows support
- [ ] tmux-compatible keybindings for the retained action set work correctly
- [ ] OSC extensions (Markdown/image) work in mux mode
- [ ] Code review completed

## Open Questions

> **Note**: All requirements have been resolved. No TBD items.

## Implementation Phases

### Phase 1: Daemon + IPC Foundation
**Goals:** Establish daemon process and IPC communication
**Deliverables:**
- Daemon process (startup, socket listener, shutdown)
- IPC protocol (frame codec, handshake, message types)
- CLI subcommand (`emterm mux --daemon`)
- Socket path management (Linux/Windows)

### Phase 2: Single Pane + Attach/Detach
**Goals:** Basic multiplexer functionality with one pane
**Deliverables:**
- OSC signaling (attach/detach)
- GUI mode switching (tab group, Canvas creation)
- Detach/reattach with snapshot and ring buffer
- Environment variable setup (TERM_PROGRAM, EMTERM_MUX)
- Error recovery (daemon crash → normal mode)

### Phase 3: Window Management
**Goals:** Multiple windows per session
**Deliverables:**
- Window create/switch/rename/close (one pane per window)
- Tab group UI (expand/compact animation)
- Window navigation (prefix+n/p, mouse click)

### Phase 4: Status Bar
**Goals:** Rich status bar display
**Deliverables:**
- HTML-rendered status bar
- Session/window info display
- Event-driven updates from daemon

### Phase 5: tmux.conf Conversion
**Goals:** Settings migration from tmux
**Deliverables:**
- Tokenizer-based tmux.conf parser
- Auto-import on first mux startup (`~/.tmux.conf` → `settings.json` mux section)
- Warning display for unsupported directives (backend log)
- Mux keybinding editor in settings panel

## References

- Design report: `tmp/emterm-tmux-report.md`
- Cleanup task: `doc/tasks/mux-feature-cleanup/SPEC.md`
