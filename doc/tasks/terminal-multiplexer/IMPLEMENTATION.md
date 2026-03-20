# Implementation Plan: Terminal Multiplexer

## Overview

Integrate a native terminal multiplexer into eMterm, adding a daemon process for PTY session management, IPC communication via Unix domain sockets, and a GUI multiplexer mode with pane splitting, window management, and tmux-compatible keybindings. The architecture eliminates VT100 double-parsing by relaying raw PTY bytes through the daemon directly to per-pane WASM parser instances in the GUI.

## Objectives

- Add daemon process with IPC socket communication for PTY session management
- Implement GUI multiplexer mode with per-pane Canvas + WASM rendering
- Support detach/reattach with snapshot-based state restoration
- Provide tmux-compatible user experience (prefix key, keybindings, pane/window management)

## Prerequisites

### Development Environment
- Rust toolchain (stable), wasm-pack, Bun, Tauri CLI
- Docker for testing (existing infrastructure)

### Dependencies
- `tokio` + `tokio-util` (async runtime + framing)
- `interprocess` (Unix socket abstraction)
- `bincode` (control message serialization)
- `serde` added to WASM crate (snapshot serialization)

## Architecture Overview

### Technology Stack
- **Backend (Daemon)**: Rust + Tokio async runtime
- **IPC**: Unix domain socket + length-prefixed binary frames
- **Frontend**: Vanilla TypeScript + CSS Grid for pane layout
- **WASM**: Extended with serde serialization for snapshot support
- **Serialization**: bincode (control), raw bytes (PTY data)

### Design Approach

The multiplexer adds a daemon layer between PTY processes and the GUI. The daemon is a standalone Rust binary entry point (reusing the existing `emterm` binary with `mux` subcommand). It manages PTY lifecycle and relays raw bytes — it never parses VT100 sequences.

The GUI switches between "normal mode" (direct PTY connection) and "mux mode" (IPC to daemon). In mux mode, each pane gets its own Canvas element and WASM TerminalCore instance, reusing the existing rendering pipeline.

### Component Interaction

```
CLI (emterm mux)  ──OSC──>  GUI (eMterm)
        │                       │
        └──socket──> Daemon <──socket──┘
                      │
                    PTY 1..N
```

Three-party coordination:
1. CLI sends OSC sequence through stdout/PTY to signal GUI
2. GUI connects to daemon via IPC socket for data path
3. CLI connects to daemon via IPC socket for control only (detach notification)

## Implementation Phases

### Phase 0: Foundation — Environment Variables and WASM Serialization

**Goal**: Establish prerequisites that later phases depend on. Set `TERM_PROGRAM` / `TERM_PROGRAM_VERSION` on PTY spawn. Add snapshot serialization infrastructure to WASM TerminalCore.

**Files to Create**:
- `wasm/src/snapshot.rs` — Dedicated `TerminalSnapshot` type (separated from runtime `TerminalCore`)

**Files to Modify**:
- `src-tauri/src/pty/session.rs` — Add `TERM_PROGRAM=emterm` and `TERM_PROGRAM_VERSION` to PTY environment
- `wasm/Cargo.toml` — Add `serde` dependency with `derive` feature
- `wasm/src/terminal_core.rs` — Add `to_snapshot()` and `from_snapshot()` methods exposed via wasm_bindgen
- `wasm/src/cell.rs` — Add serde derives to Cell, PackedColor
- `wasm/src/parser.rs` — Add serde derives to serializable parser state subset
- `wasm/src/ring_buffer.rs` — Serialization support for ring buffer state
- `src/terminal/wasm/terminal-core.ts` — Add TypeScript wrappers for snapshot/restore

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TERM_PROGRAM env | Identify eMterm environment | PTY spawn call | `TERM_PROGRAM=emterm` present in child process environment |
| TerminalSnapshot | Dedicated serializable type — grid state, cursor, modes, scroll region. Excludes runtime-only fields (callbacks, pixel metrics). Versioned envelope for forward compatibility. | TerminalCore exists | Clean separation of persistent state vs runtime state |
| Snapshot API | `to_snapshot() -> Vec<u8>` extracts state into TerminalSnapshot and serializes with version header. `from_snapshot(bytes) -> TerminalCore` validates version, deserializes, and constructs new runtime instance. | serde derives on snapshot types | Versioned binary snapshot available from JS. Callbacks unset on restore. |

**Processing Flow**:
1. PTY spawn sets environment variables in CommandBuilder
2. TerminalSnapshot struct defined with only serializable fields (cells, cursor, modes, scroll region, tab stops, charsets). Runtime-only fields (callbacks, pixel metrics) excluded by design — not via serde(skip) on TerminalCore.
3. `to_snapshot()` extracts state from TerminalCore into TerminalSnapshot, serializes with version header (u32 version + bincode payload)
4. `from_snapshot()` validates version, deserializes TerminalSnapshot, constructs new TerminalCore with callbacks unset

**Implementation Steps**:
1. **Environment variables** — Add TERM_PROGRAM and TERM_PROGRAM_VERSION to PTY spawn environment
2. **TerminalSnapshot type** — Define dedicated snapshot struct with versioned envelope (version: u32 header). Extract serializable state from TerminalCore fields. Derive Serialize/Deserialize on snapshot type and its dependencies (Cell, PackedColor, cursor state subset, mode bits, ring buffer data). Parser state: include only if in ground state; otherwise skip and reset on restore.
3. **Snapshot API** — Implement to_snapshot/from_snapshot on TerminalCore. from_snapshot returns a new instance with callbacks unset; caller must re-register callbacks before processing further data. Version mismatch returns error (not panic).
4. **TypeScript wrappers** — Expose snapshot/restore through WASM proxy layer. After restore, re-register all JS callbacks (osc, apc, dcs, bel) before delta replay.
5. **Round-trip tests** — Verify serialization preserves all state fields. Test version mismatch handling. Test snapshot at various parser states (ground, mid-escape, mid-OSC).

**Dependencies**: None (foundation phase)

**Testing Approach**:
- Unit: Snapshot round-trip (create state → to_snapshot → from_snapshot → compare all fields)
- Unit: TERM_PROGRAM present in spawned PTY environment
- Unit: Cell/PackedColor/cursor state serialization round-trips
- Unit: Version mismatch handling (older version → graceful error, not crash)
- Unit: Snapshot while in alternate buffer → restore preserves both primary and alternate state

**Acceptance Criteria**:
- [ ] `TERM_PROGRAM=emterm` set in all spawned PTY processes
- [ ] TerminalSnapshot round-trip preserves all persistent state
- [ ] Snapshot includes version header; version mismatch returns error
- [ ] Snapshot size is reasonable (< 5MB for typical 200-row, 10000-line scrollback)
- [ ] Runtime-only fields (callbacks) cleanly excluded from snapshot type

**Estimated Effort**: medium

---

### Phase 1: Daemon Process and IPC Foundation

**Goal**: Standalone daemon process that listens on a Unix domain socket, accepts connections, completes handshake, and can spawn/manage a single PTY session.

**Files to Create**:
- `src-tauri/src/mux/mod.rs` — Module root, re-exports
- `src-tauri/src/mux/daemon.rs` — Daemon entry point (tokio runtime, socket listener, signal handling)
- `src-tauri/src/mux/ipc/mod.rs` — IPC module root
- `src-tauri/src/mux/ipc/protocol.rs` — Message type enum, frame header, serialization traits
- `src-tauri/src/mux/ipc/codec.rs` — LengthDelimited codec wrapper with max frame limit
- `src-tauri/src/mux/ipc/connection.rs` — Connection state machine (handshake, authenticated, streaming)
- `src-tauri/src/mux/session/mod.rs` — Session module root
- `src-tauri/src/mux/session/manager.rs` — Session manager (actor model: single tokio task + mpsc channel)
- `src-tauri/src/mux/session/session.rs` — Session state (windows, active window)
- `src-tauri/src/mux/session/window.rs` — Window state (panes, active pane, name)
- `src-tauri/src/mux/session/pane.rs` — Pane state (PTY handle, size, bounded channel)
- `src-tauri/src/mux/ring_buffer.rs` — Per-pane ring buffer for detached PTY output (64MB cap)
- `src-tauri/src/mux/cli.rs` — CLI subcommands (mux, mux attach, mux ls, mux kill, mux new)

**Files to Modify**:
- `src-tauri/Cargo.toml` — Add tokio-util, interprocess, bincode dependencies
- `src-tauri/src/main.rs` — Add `mux` subcommand to clap configuration
- `src-tauri/src/lib.rs` — Declare `mux` module

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Daemon | Process lifecycle, socket listener, signal handling | Socket path directory exists | Listening on Unix socket, handles SIGTERM gracefully |
| IPC Codec | Frame encoding/decoding with length prefix | Raw TCP/Unix stream | Typed messages with pane_id routing |
| Connection | Per-client state machine (handshake → streaming) | Socket accepted | Client type identified (GUI/CLI), protocol version verified |
| Session Manager | Actor managing all sessions/windows/panes | Daemon running | Creates/destroys sessions, routes messages, cascade cleanup |
| Pane | PTY ownership, reader thread, bounded channel | Session exists | PTY spawned, reader feeding channel, writer accepting input |
| Ring Buffer | Accumulate PTY output during detach | Pane exists, no GUI connected | Circular buffer with 64MB cap, oldest data overwritten |

**Processing Flow**:
1. `emterm mux --daemon` starts tokio runtime, creates socket listener
2. Client connects → Connection state machine runs handshake
   - Hello received → validate protocol version → send Welcome with session list (or Rejected)
3. GUI client sends Attach → session manager routes to session → streaming begins
   - Output path: PTY reader → bounded channel → select! mux writer → socket frame
   - Input path: socket frame → demux reader → PTY writer
4. CLI client connects for control only (session list, kill session)
5. SIGTERM → stop accepting, notify clients, SIGHUP to PTYs, cleanup socket

**Implementation Steps**:
1. **IPC protocol types** — Define message enum, frame format, bincode serialization for control messages
2. **Codec layer** — Implement LengthDelimited codec with max_frame_length(16MB)
3. **Daemon entry point** — Tokio runtime setup, socket creation, stale socket cleanup, signal handling
4. **Connection handler** — Handshake state machine, client type routing
5. **Session manager actor** — mpsc channel receiver loop, session/window/pane CRUD
6. **PTY pane with channels** — Spawn PTY, reader thread with bounded channel, writer dispatch
7. **CLI subcommands** — emterm mux (start daemon + connect), mux attach, mux ls, mux kill, mux new

**Dependencies**: Phase 0 (TERM_PROGRAM for nesting detection)

**Testing Approach**:
- Unit: IPC frame encoding/decoding round-trip, protocol message serialization
- Unit: Ring buffer write/read with overflow, capacity enforcement
- Unit: Socket path generation per platform (Linux/Windows)
- Integration: Daemon startup → socket creation → client connect → handshake → disconnect → cleanup
- Integration: Session lifecycle (create → PTY spawn → data flow → PTY exit → cascade cleanup → daemon exit)
- Integration: Stale socket detection and cleanup
- Integration: Graceful shutdown (SIGTERM sequence)
- Integration: Backpressure (fill bounded channel, verify PTY reader blocks)

**Acceptance Criteria**:
- [ ] Daemon starts, creates socket, accepts connections
- [ ] Handshake completes (Hello → Welcome)
- [ ] PTY data flows through daemon (input and output)
- [ ] Bounded channel provides backpressure under high throughput
- [ ] Daemon exits when last session ends
- [ ] Graceful shutdown on SIGTERM
- [ ] Stale socket files are cleaned up

**Estimated Effort**: large

---

### Phase 2: GUI Mux Mode — Single Pane Attach/Detach

**Goal**: CLI sends OSC sequence to trigger GUI mode switch. GUI connects to daemon, displays single pane with Canvas + WASM. Detach saves snapshot, reattach restores state from snapshot + delta replay.

**Files to Create**:
- `src/terminal/mux/index.ts` — Mux client module entry
- `src/terminal/mux/mux-client.ts` — IPC client (connects via Tauri command bridge to daemon socket)
- `src/terminal/mux/pane-manager.ts` — Per-pane Canvas + WASM instance lifecycle
- `src/terminal/mux/tab-group.ts` — Tab group UI (mux session as tab group in tab bar)
- `src-tauri/src/mux/snapshot.rs` — Snapshot store (save/load serialized grid state + ring buffer delta)
- `src-tauri/src/mux/bridge.rs` — Tauri commands bridging GUI ↔ daemon IPC socket

**Files to Modify**:
- `src/terminal-app/osc-handler.ts` — Add handler for `OSC 777;emterm;mux;attach` and `mux;detach`
- `src/terminal-app/index.ts` — Add mux mode state, pause/resume PTY reader
- `src/tab-bar/tab-manager.ts` — Support tab group type for mux sessions
- `src/tab-bar/tab-bar-ui.ts` — Render tab group visual treatment
- `src/tab-bar/types.ts` — Add MuxTab type
- `src-tauri/src/tauri_commands.rs` — Add mux bridge commands
- `src-tauri/src/mux/cli.rs` — Implement OSC sequence output for attach/detach commands
- `src-tauri/src/mux/daemon.rs` — Add snapshot store integration, attach/detach flow
- `src-tauri/src/mux/ipc/protocol.rs` — Add Snapshot/SnapshotRestore message handling

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OSC Handler (mux) | Parse mux attach/detach OSC, validate socket path | OSC sequence received | Socket path validated (allowed dirs, no traversal), mode switch initiated |
| Mux Client | IPC connection to daemon, message framing | Socket path validated | Connected, streaming PTY data |
| Pane Manager | Canvas + WASM instance per pane | Mux mode active | Canvas created, WASM initialized, rendering PTY output |
| Tab Group | Visual tab group in tab bar | Mux session active | Mux tab shows session info, compact/expand behavior |
| Snapshot Store | Save/load grid snapshots (WASM binary + TS metadata: title, CWD) + ring buffer | Daemon running | Snapshots persisted in memory, delta bytes accumulated |
| Bridge | Tauri IPC commands for GUI ↔ daemon socket | GUI + daemon running | GUI can connect/disconnect/send/receive via Tauri commands |

**Processing Flow**:
1. User runs `emterm mux` in shell
2. CLI outputs `OSC 777;emterm;mux;attach;<socket_path>;<session_id> ST`
3. GUI's OSC handler validates socket path (allowed directory, no `../`)
4. GUI pauses original PTY reader (AtomicBool flag)
5. GUI connects to daemon via Tauri bridge command
6. Daemon sends Welcome → GUI sends Attach with session_id
7. Pane Manager creates Canvas + WASM instance, begins rendering PTY output
8. Tab transforms into tab group

Detach flow:
1. User triggers detach (prefix+d or programmatic)
2. GUI calls WASM snapshot_to_bytes() + collects TS-side metadata (tab title, CWD, image state) → sends Snapshot message to daemon
3. Daemon stores snapshot (WASM binary + metadata), starts ring buffer accumulation
4. GUI destroys mux Canvas, resumes original PTY reader
5. CLI receives Detached notification, exits

Reattach flow:
1. CLI sends OSC attach → GUI connects to daemon
2. Daemon locks ring buffer → sends SnapshotRestore (WASM binary + metadata) + delta bytes → unlocks
3. GUI restores WASM from snapshot → re-registers JS callbacks (osc/apc/dcs/bel) → restores TS-side metadata (title, CWD) → replays delta through parser
4. Streaming resumes

**Implementation Steps**:
1. **OSC handler extension** — Parse mux;attach and mux;detach OSC sequences with socket path validation
2. **Tauri bridge commands** — Commands for socket connect/disconnect/send/receive between GUI and daemon
3. **Mux client** — TypeScript IPC client that uses bridge commands to communicate with daemon
4. **Pane manager** — Canvas element creation, WASM TerminalCore instantiation, rendering pipeline setup. Each pane gets its own image layer and OSC callback routing. Per-pane ImageProcessor instance (or per-pane keyed access to shared processor) for Kitty/SIXEL chunked transfer tracking. Markdown/image OSC extensions routed per-pane via pane_id in WASM callbacks. Device responses (DSR, DECRQSS, Kitty query responses) must be routed to the correct pane's PTY via pane_id — not to a global PTY write path.
5. **Tab group UI** — Tab bar extension for mux tab groups with compact/expand visual treatment
6. **Snapshot flow** — Detach: WASM to_snapshot() + TS metadata → daemon store. Reattach: daemon send → WASM from_snapshot() + callback re-registration + TS metadata restore + delta replay. Snapshot version mismatch → graceful fallback to empty screen (not crash).
7. **Error recovery** — Socket disconnect detection, auto-return to normal mode, toast notification

**Dependencies**: Phase 0 (WASM snapshot), Phase 1 (daemon, IPC)

**Testing Approach**:
- Unit: OSC mux sequence parsing, socket path validation (allowed paths, path traversal rejection)
- Unit: Tab group state transitions (normal → mux → detach → normal)
- Integration: CLI → OSC → GUI mode switch → daemon connect → data flow → detach → normal mode
- Integration: Snapshot round-trip through daemon (detach → accumulate → reattach → verify screen state)
- E2E (Docker): Full attach/detach/reattach cycle with command execution

**Acceptance Criteria**:
- [ ] `emterm mux` switches GUI to mux mode with working terminal pane
- [ ] Detach returns to normal mode, daemon keeps running
- [ ] Reattach restores screen state (snapshot + delta)
- [ ] Daemon crash auto-recovers GUI to normal mode
- [ ] Socket path validation rejects `../` and non-allowed directories
- [ ] Non-eMterm environment shows error (TERM_PROGRAM check)
- [ ] Nesting prevented (EMTERM_MUX check)

**Estimated Effort**: large

---

### Phase 3: Pane Split and Layout

**Goal**: Multiple panes per window with binary tree layout, CSS Grid rendering, drag-resize, and per-pane WASM instances. Prefix key handling for pane operations.

**Files to Create**:
- `src/terminal/mux/layout.ts` — Binary tree layout engine (split, resize, remove, preset layouts)
- `src/terminal/mux/prefix-key.ts` — Prefix key state machine and keybinding dispatch
- `src/terminal/mux/pane-border.ts` — Pane border rendering, drag-resize handling

**Files to Modify**:
- `src/terminal/mux/pane-manager.ts` — Multi-pane support, per-pane WASM instance pool
- `src/terminal/mux/mux-client.ts` — Multi-pane message routing (pane_id dispatch)
- `src/terminal-app/handlers/keyboard.ts` — Prefix key interception in mux mode
- `src-tauri/src/mux/ipc/protocol.rs` — Add SplitPane message type
- `src-tauri/src/mux/session/manager.rs` — Pane split/close operations
- `src-tauri/src/mux/session/pane.rs` — Per-pane resize handling
- `src/settings/types.ts` — Add `mux` settings section (prefix, keybinds, mouse)
- `src-tauri/src/commands/config/settings.rs` — Add MuxSettings struct with serde defaults
- `src-tauri/src/commands/config/types.rs` — Add mux-related enums

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Layout Engine | Binary tree model, split/resize/remove, preset layouts | At least one pane exists | Tree correctly calculates pixel bounds for each pane |
| Prefix Key | State machine: idle → prefix-active → dispatch command | Mux mode active | Intercepts prefix+key combos, dispatches pane/window operations |
| Pane Border | Visual borders, active indicator, drag-resize | Layout computed | 1px borders with accent color on active, cursor change on hover, drag resizes |
| Mux Settings | Prefix key, keybindings, mouse, base_index | Settings system | Mux config persisted in settings.json, mirrored in TS AppSettings |

**Processing Flow**:
1. User presses prefix key (default Ctrl+b) → prefix key handler enters "waiting" state
2. Next keypress dispatched: `%` → vertical split, `"` → horizontal split, `o` → next pane, etc.
3. Split: GUI sends CreatePane to daemon → daemon spawns PTY → PaneCreated response
4. Layout engine splits current node in binary tree → recalculates pixel bounds
5. New Canvas + WASM instance created for new pane
6. CSS Grid template updated from binary tree → panes positioned
7. Resize messages sent to daemon for each affected pane (new col/row counts)
8. Window resize → layout recalculation (ratio preservation) → per-pane Resize messages

**Implementation Steps**:
1. **Mux settings** — Add mux section to settings (prefix key, keybinds, mouse, base_index)
2. **Binary tree layout** — Tree data structure with split/resize/remove, pixel-to-cell conversion
3. **Prefix key handler** — State machine intercepting keyboard events in mux mode
4. **Multi-pane rendering** — CSS Grid template generation from binary tree, dynamic Canvas creation
5. **Pane border UI** — Border rendering, active indicator, drag-resize with event handling
6. **Daemon pane operations** — CreatePane/DestroyPane/Resize message handling in session manager
7. **Pane zoom toggle** — prefix+z toggles active pane between full-window and original layout position. Layout engine saves/restores pre-zoom state.
8. **Preset layouts** — even-horizontal, even-vertical, main-horizontal, main-vertical, tiled

**Dependencies**: Phase 2 (single pane mux mode)

**Testing Approach**:
- Unit: Binary tree split/resize/remove calculations (all directions, nested splits)
- Unit: Preset layout generation (even-h, even-v, main-h, main-v, tiled)
- Unit: Pixel-to-cell conversion with various font metrics
- Unit: Prefix key state machine transitions
- Unit: Minimum pane size enforcement (2 rows x 10 cols)
- Integration: Split pane → new PTY → data flow → close pane → layout update
- E2E (Docker): Split → type in both panes → resize → close one → verify

**Acceptance Criteria**:
- [ ] Pane split (horizontal and vertical) creates new working pane
- [ ] Binary tree layout correctly positions panes
- [ ] Drag-resize changes pane proportions
- [ ] Prefix key intercepts and dispatches operations correctly
- [ ] Minimum pane size enforced (split refused below threshold)
- [ ] Window resize preserves pane proportions
- [ ] Active pane indicated by border color
- [ ] Pane close removes from tree and recalculates layout
- [ ] Preset layouts produce correct arrangements
- [ ] Pane zoom toggle (prefix+z) expands active pane to full window and restores

**Estimated Effort**: large

---

### Phase 4: Window Management

**Goal**: Multiple windows per session with tab group sub-tabs. Window creation, switching, renaming, closing. Auto-expand/compact animation.

**Files to Create**:
- (none — extends existing files)

**Files to Modify**:
- `src/terminal/mux/tab-group.ts` — Window sub-tabs, expand/compact animation, window navigation
- `src/terminal/mux/mux-client.ts` — Window management messages
- `src/terminal/mux/prefix-key.ts` — Window operation keybindings (c, n, p, comma)
- `src/tab-bar/tab-bar-ui.ts` — Sub-tab rendering within tab groups
- `src-tauri/src/mux/ipc/protocol.rs` — Add CreateWindow, SwitchWindow, RenameWindow message types
- `src-tauri/src/mux/session/manager.rs` — Window CRUD operations
- `src-tauri/src/mux/session/window.rs` — Window state management (active pane tracking)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Tab Group Windows | Sub-tab UI for mux windows | Mux tab group exists | Windows shown as sub-tabs, clickable, with compact/expand |
| Window Manager | Create/switch/rename/close windows | Session active | Windows correctly created and destroyed with cascade |
| Expand/Compact | Animation for active/inactive tab groups | Multiple tabs in bar | Active mux tab shows window sub-tabs, inactive shows "mux (N)" |

**Processing Flow**:
1. `prefix + c` → GUI sends CreateWindow to daemon → daemon creates window with default pane
2. Daemon sends WindowCreated → GUI adds sub-tab, switches to new window
3. `prefix + n/p` → GUI switches active window (local operation, notifies daemon)
4. Tab click → switch window directly
5. Active mux tab group auto-expands showing all window sub-tabs
6. Switching away from mux tab → compact animation (0.3s) to "mux (N)"
7. Window close: last pane exit → window removed → if last window → session ends

**Implementation Steps**:
1. **Window management protocol** — CreateWindow, SwitchWindow, RenameWindow, DestroyWindow messages
2. **Daemon window operations** — Window CRUD in session manager, cascade cleanup
3. **Sub-tab UI** — Window sub-tabs within tab group, click to switch
4. **Window keybindings** — prefix+c (create), prefix+n/p (navigate), prefix+comma (rename)
5. **Expand/compact animation** — CSS transitions for tab group state, "mux (N)" compact label

**Dependencies**: Phase 3 (pane layout)

**Testing Approach**:
- Unit: Window create/switch/rename/close state transitions
- Unit: Tab group expand/compact logic
- Integration: Create window → switch → rename → close → verify tab group updates
- E2E (Docker): Multi-window workflow with switching

**Acceptance Criteria**:
- [ ] `prefix + c` creates new window with shell
- [ ] `prefix + n/p` navigates between windows
- [ ] `prefix + ,` renames window
- [ ] Sub-tabs appear correctly in tab group
- [ ] Active tab group expands, inactive compacts with animation
- [ ] `mux_tab_always_expand` setting works
- [ ] Last window close ends session

**Estimated Effort**: medium

---

### Phase 5: Status Bar

**Goal**: HTML-rendered status bar showing session name, window list, and time. Event-driven updates from daemon.

**Files to Create**:
- `src/terminal/mux/status-bar.ts` — Status bar component (HTML rendering, event handling)

**Files to Modify**:
- `src/terminal/mux/index.ts` — Status bar integration
- `src/terminal/mux/pane-manager.ts` — Layout accounting for status bar height
- `src-tauri/src/mux/ipc/protocol.rs` — Add StatusUpdate message type
- `src-tauri/src/mux/session/manager.rs` — Push status updates on state changes
- `src/settings/types.ts` — Add `mux.status_position` setting
- `doc/UI-DESIGN-GUIDELINES.yaml` — Status bar component specification

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Status Bar | HTML-rendered bar with session/window/time info | Mux mode active | Positioned top/bottom per setting, updates on events |
| Status Push | Daemon pushes state changes to GUI | GUI connected | Session name, window list, active window updated in real-time |

**Processing Flow**:
1. Daemon detects state change (window created, renamed, pane exit, etc.)
2. Daemon pushes StatusUpdate message to GUI with current state
3. GUI status bar updates HTML content (session name, window indicators, time)
4. Local time updated by GUI timer (no daemon involvement)

**Implementation Steps**:
1. **Status bar component** — HTML container, CSS styling per UI guidelines, position setting
2. **Event-driven updates** — Daemon pushes StatusUpdate on session/window/pane changes
3. **Status bar content** — Session name, window list with indicators, local time
4. **Layout integration** — Pane layout accounts for status bar height
5. **UI design guidelines** — Update doc/UI-DESIGN-GUIDELINES.yaml with status bar spec

**Dependencies**: Phase 4 (window management)

**Testing Approach**:
- Unit: Status bar rendering with various window configurations
- Integration: Window operations trigger status bar updates
- Manual: Visual verification of status bar appearance and position

**Acceptance Criteria**:
- [ ] Status bar renders at configured position (top/bottom)
- [ ] Session name and window list displayed correctly
- [ ] Time updates locally
- [ ] State changes reflect immediately (event-driven, no polling)

**Estimated Effort**: small

---

### Phase 6: Copy Mode

**Goal**: Enter copy mode to navigate scrollback with vi/emacs keybindings, select text, and copy to system clipboard. Search within scrollback.

**Files to Create**:
- `src/terminal/mux-copy-mode/index.ts` — Copy mode entry point, mode state management
- `src/terminal/mux-copy-mode/vi-keybinds.ts` — vi movement and selection keybindings
- `src/terminal/mux-copy-mode/emacs-keybinds.ts` — emacs movement and selection keybindings

**Files to Modify**:
- `src/terminal/mux/prefix-key.ts` — Add copy mode entry (`prefix + [`) and paste (`prefix + ]`)
- `src/terminal/mux/pane-manager.ts` — Copy mode overlay on active pane
- `src/terminal/canvas-renderer.ts` — Selection highlight rendering in copy mode (extend existing)
- `wasm/src/terminal_core.rs` — Add scrollback search method (text search across ring buffer)
- `src/terminal/wasm/terminal-core.ts` — Expose search method to TypeScript

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Copy Mode Manager | Mode entry/exit, keybinding routing (vi/emacs) | Mux mode active, pane focused | Copy mode active, input routed to keybinding handler |
| Vi Keybinds | h/j/k/l movement, v selection, y yank, / search | Copy mode active | Cursor moves, text selected, copied to clipboard |
| Emacs Keybinds | Ctrl-n/p/f/b movement, Ctrl-space selection | Copy mode active | Cursor moves, text selected, copied to clipboard |
| Scrollback Search | Text search within ring buffer (WASM) | Grid data in WASM | Matching positions returned for highlighting |

**Processing Flow**:
1. `prefix + [` enters copy mode → pane stops forwarding input to PTY
2. Keybinding handler routes to vi or emacs handler based on setting
3. Movement keys move virtual cursor through scrollback (WASM grid read)
4. Selection mode marks start/end positions, Canvas renders highlight
5. Yank/copy sends selection text to system clipboard
6. `q` or `Escape` exits copy mode, resumes normal input
7. `prefix + ]` reads system clipboard → sends to PTY as input

**Implementation Steps**:
1. **Copy mode state** — Mode enter/exit, virtual cursor position tracking
2. **Vi keybindings** — Movement (h/j/k/l/w/b/0/$), selection (v/V), yank (y), search (/)
3. **Emacs keybindings** — Movement (C-n/p/f/b/a/e), selection (C-space), copy (M-w)
4. **Selection rendering** — Extend Canvas renderer for copy mode highlight overlay
5. **Scrollback search** — WASM method to search ring buffer text, return matching positions
6. **Clipboard integration** — Copy selection to system clipboard (Clipboard API), paste from clipboard to PTY

**Dependencies**: Phase 3 (pane management, prefix key)

**Testing Approach**:
- Unit: Vi/emacs keybinding dispatch
- Unit: Scrollback search (WASM) with various patterns
- Unit: Selection bounds calculation
- Integration: Enter copy mode → navigate → select → copy → verify clipboard
- Manual: Visual verification of selection highlight

**Acceptance Criteria**:
- [ ] `prefix + [` enters copy mode
- [ ] Vi keybindings move cursor and select text
- [ ] Emacs keybindings move cursor and select text
- [ ] Selected text copies to system clipboard
- [ ] `prefix + ]` pastes from clipboard
- [ ] Search highlights matching text
- [ ] `q`/`Escape` exits copy mode

**Estimated Effort**: medium

---

### Phase 7: tmux.conf Conversion

**Goal**: Parse tmux.conf files and convert supported settings to eMterm's `settings.json` mux section. Unsupported directives produce warnings.

**Files to Create**:
- `src-tauri/src/mux/tmux_conf/mod.rs` — tmux.conf parser module root
- `src-tauri/src/mux/tmux_conf/parser.rs` — Line-oriented regex-based parser
- `src-tauri/src/mux/tmux_conf/converter.rs` — Setting mapper (tmux directives → mux settings)

**Files to Modify**:
- `src-tauri/src/mux/cli.rs` — Add `emterm mux import-conf <path>` subcommand
- `src-tauri/src/mux/mod.rs` — Declare tmux_conf module

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Parser | Line-by-line regex parsing of tmux.conf | Valid file path | Parsed directives (set, bind-key, unbind-key, etc.) |
| Converter | Map tmux directives to mux settings | Parsed directives | Updated mux settings, list of warnings for unsupported items |

**Conversion targets**:
- `set -g prefix` → mux.prefix
- `bind-key` / `unbind-key` → mux.keybinds
- `set -g base-index` → mux.base_index
- `set -g mouse` → mux.mouse
- `set -g status-position` → mux.status_position
- `set -g default-terminal` → TERM setting

**Ignored with warnings**: `if-shell`, `run-shell`, format strings (`#{...}`), `set-hook`, plugins

**Implementation Steps**:
1. **tmux.conf parser** — Regex-based line parser for set/bind/unbind directives
2. **Setting converter** — Map parsed directives to mux settings JSON structure
3. **Warning generator** — Collect unsupported/ignored directives with explanations
4. **CLI subcommand** — `emterm mux import-conf` reads file, converts, writes to settings.json
5. **Output report** — Display converted settings and warnings to user

**Dependencies**: Phase 3 (mux settings must exist)

**Testing Approach**:
- Unit: Parser with sample tmux.conf snippets (basic, complex, edge cases)
- Unit: Converter mapping correctness (each supported directive)
- Unit: Warning generation for unsupported directives
- Integration: Full tmux.conf file → import → verify settings.json

**Acceptance Criteria**:
- [ ] Supported settings correctly converted
- [ ] Unsupported directives produce clear warnings
- [ ] Comments and empty lines handled gracefully
- [ ] Malformed lines produce warnings (not errors)
- [ ] Output report shows what was converted and what was skipped

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── mux/
│   ├── mod.rs                  # Module root, feature gate, re-exports
│   ├── daemon.rs               # Daemon process (tokio runtime, socket, signals)
│   ├── bridge.rs               # Tauri commands bridging GUI ↔ daemon socket
│   ├── cli.rs                  # CLI subcommands (mux, attach, ls, kill, new, import-conf)
│   ├── snapshot.rs             # Snapshot store (WASM binary + TS metadata + ring buffer delta)
│   ├── ring_buffer.rs          # Per-pane circular buffer (64MB cap)
wasm/src/
├── snapshot.rs                 # TerminalSnapshot type (versioned, serializable)
│   ├── ipc/
│   │   ├── mod.rs              # IPC module root
│   │   ├── protocol.rs         # Message types, frame header, serialization
│   │   ├── codec.rs            # LengthDelimited codec wrapper
│   │   └── connection.rs       # Connection state machine
│   ├── session/
│   │   ├── mod.rs              # Session module root
│   │   ├── manager.rs          # Actor-model session manager
│   │   ├── session.rs          # Session state
│   │   ├── window.rs           # Window state
│   │   └── pane.rs             # Pane state + PTY + channels
│   └── tmux_conf/
│       ├── mod.rs              # tmux.conf module root
│       ├── parser.rs           # Regex-based tmux.conf parser
│       └── converter.rs        # tmux directive → mux setting mapper
src/
├── terminal/
│   ├── mux/
│   │   ├── index.ts            # Mux client module entry
│   │   ├── mux-client.ts       # IPC client via Tauri bridge
│   │   ├── layout.ts           # Binary tree layout engine
│   │   ├── pane-manager.ts     # Per-pane Canvas + WASM lifecycle
│   │   ├── pane-border.ts      # Border rendering, drag-resize
│   │   ├── prefix-key.ts       # Prefix key state machine
│   │   ├── tab-group.ts        # Tab group UI, expand/compact
│   │   └── status-bar.ts       # HTML status bar component
│   └── mux-copy-mode/
│       ├── index.ts            # Copy mode state manager
│       ├── vi-keybinds.ts      # Vi keybinding handler
│       └── emacs-keybinds.ts   # Emacs keybinding handler
```

## Testing Strategy

- **Unit tests**: Core logic coverage (IPC protocol, layout engine, ring buffer, parser, keybindings). Target 80%+ for critical components (protocol, layout, snapshot).
- **Integration tests**: Real socket IPC, daemon lifecycle, session management, PTY data flow, backpressure. Use Rust integration tests with actual Unix sockets.
- **E2E (Docker)**: Full user workflows (start mux → split panes → detach → reattach → verify state). Extend existing `run-e2e-docker.sh` infrastructure.
- **Manual**: Visual verification of pane borders, status bar appearance, animation timing, drag-resize feel.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tokio-util | latest | LengthDelimitedCodec for IPC framing |
| interprocess | latest | Unix socket abstraction with tokio integration |
| bincode | 2.x | Control message binary serialization |
| serde (WASM) | 1.0 | TerminalCore snapshot serialization |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| interprocess v2 API breakage | Medium | Medium | Fallback to raw tokio UnixStream (cfg-gated) |
| WASM snapshot size too large | Low | Medium | Optimize: exclude scrollback from snapshot if > threshold, rely on delta replay |
| Pane layout performance (many panes) | Low | Low | CSS Grid handles layout natively, tree recalc is O(n) where n = pane count |
| Windows AF_UNIX compatibility | Low | High | Test on Windows 10 1803+. Document minimum requirement. |
| Daemon/GUI version mismatch | Low | Medium | Protocol version in handshake, reject incompatible versions |

## Open Questions

- (none — all requirements resolved in specification)

## Success Metrics

- [ ] All 7 phases implemented and tested
- [ ] High-throughput benchmark (seq 1 1000000) shows no degradation vs normal mode
- [ ] Detach/reattach preserves full screen state
- [ ] tmux-compatible keybindings work for all specified operations
- [ ] OSC extensions (Markdown/image) work in mux mode
- [ ] Linux and Windows E2E tests pass
