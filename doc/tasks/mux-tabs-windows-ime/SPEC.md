# Feature: mux Client + Tab Bar UI + Windows IME (Phase 4)

## Overview

Port mux client and tab bar UI from the legacy Tauri build to the native `native-poc` (tao + wgpu + egui) stack, and verify Windows MS-IME on the same stack. This is Phase 4 of the emterm restructuring plan (`tmp/restruct.md`). Phases 1-3 delivered the single-PTY native terminal core (term_core + term_images + native-poc with 1801 tests passing). Phase 4 closes the gap to legacy emterm parity for the mux usage path and unlocks Phase 5 (Wry viewer integration).

## Objectives

- Ship an egui-based tab bar with new / close / switch / title display equivalent to the legacy `src/tabbar/`.
- Extract `src-tauri/src/mux/ipc/` into a workspace crate `crates/mux_ipc/` (mirroring the Phase 3 `term_images` extraction) so both legacy `src-tauri` and `native-poc` share a single protocol implementation.
- Connect `native-poc` to the existing `emterm mux` daemon, supporting attach / detach / window switch over the Unix socket protocol used today.
- Render the mux status bar in egui using the daemon-pushed `StatusUpdateMsg` already defined in `src-tauri/src/mux/ipc/statusbar.rs`.
- Verify Windows MS-IME `preedit` + `commit` paths work through `egui`'s built-in IME hooks (candidate window position is best effort).
- Keep Phase 1 Linux fcitx5 IME behavior intact.

## User Stories

### US1: Open / close / switch tabs in native-poc
As a user, I want to manage multiple PTY sessions through a native tab bar so that I don't lose mux-equivalent ergonomics when running the new build.

**Acceptance Criteria:**
- [ ] `Ctrl+Shift+T` spawns a new PTY and makes its tab active.
- [ ] `Ctrl+Shift+W` (or middle-click) closes the active tab; closing the last tab exits the window (Phase 1 parity).
- [ ] `Ctrl+Tab` / `Ctrl+Shift+Tab` cycle through tabs.
- [ ] `Ctrl+1..9` jumps directly to tab N (clamped to existing tabs).
- [ ] Each tab displays the PTY-supplied title (OSC 0/2) or fallback `"shell"`.

### US2: Attach to and detach from a mux session
As a Claude Code user, I want my long-running mux session to be reachable from native-poc so that the Phase 3 stability gains apply to mux scenarios too.

**Acceptance Criteria:**
- [ ] PTY emitting `OSC 777 ; emterm ; mux ; attach ; <socket> ; <session_id> ST` triggers an attach in the same tab.
- [ ] On attach, the daemon-provided snapshot is rendered before the user types again.
- [ ] `prefix d` (default `Ctrl+B`, then `d`) detaches and returns the tab to its original PTY.
- [ ] During mux mode, the original PTY reader is paused so duplicate output does not interleave.

### US3: Switch mux windows
As a mux user, I want `prefix n / p / 0..9` to switch windows within an attached session so that workflow parity with legacy emterm is preserved.

**Acceptance Criteria:**
- [ ] `prefix n` / `prefix p` cycle windows.
- [ ] `prefix 0..9` jumps to that window (clamped to existing windows).
- [ ] After every switch, the daemon snapshot replaces the on-screen contents.

### US4: Observe mux state in the status bar
As a mux user, I want a status bar showing the session name, the window list and a local clock so that I can navigate without the daemon `list-clients` command.

**Acceptance Criteria:**
- [ ] When attached, status bar shows `[<session_name>] win1 *win2 win3` (active window starred) and `HH:MM:SS`.
- [ ] When not attached, the status bar shows only the local clock (if `statusbar.enabled = true`).
- [ ] Position is configurable via `settings.json` (`top` / `bottom`, default `bottom`).
- [ ] No daemon polling is performed; updates are driven by daemon-pushed `StatusUpdateMsg`.

### US5: Type Japanese on Windows
As a Windows user, I want to use MS-IME so that Japanese input works in native-poc.

**Acceptance Criteria:**
- [ ] preedit text appears near the cursor while composing.
- [ ] committed text is written to the active PTY exactly once.
- [ ] Candidate window position is best effort (not gating).

## Technical Requirements

### Functional Requirements

- **FR1: egui tab bar widget** — Implements `native_poc::ui::tab_bar::draw()` in egui with new / close / switch buttons, title rendering, and a visual indicator for the active tab. Drag-to-reorder and right-click context menu are out of scope.
- **FR2: Tab keybinds** — `Ctrl+Shift+T` new tab, `Ctrl+Shift+W` close tab, `Ctrl+Tab` next, `Ctrl+Shift+Tab` previous, `Ctrl+1..9` jump. Implemented in `native_poc::ui::keybinds`.
- **FR3: `crates/mux_ipc/` extraction (scope-limited)** — `git mv src-tauri/src/mux/ipc/protocol.rs crates/mux_ipc/src/protocol.rs`. `codec.rs` (tokio_util-based server framing) and `connection.rs` (server-side handler containing `crate::mux::session::*` references and the per-client accept loop) **stay in `src-tauri`** because they embed server-only logic; moving them would drag the entire src-tauri mux runtime across the boundary. `pty_spawn.rs`, `handlers.rs`, `reattach.rs`, `statusbar.rs` also stay (Tauri-bound). `crates/mux_ipc/Cargo.toml` is created (pure `base64` + `serde` + `serde_derive` deps, NO tokio); workspace root adds it as a member; `src-tauri/src/mux/ipc/protocol.rs` becomes a 1-line shim `pub use mux_ipc::protocol::*;` for backward compatibility. native-poc writes its own blocking client (see FR4 / file structure) using std `UnixStream` + custom length-prefix framing + `bincode` over `mux_ipc::protocol::MuxMessage`.
- **FR4: mux attach (OSC 777)** — Native-poc parses `OSC 777 ; emterm ; mux ; attach ; <socket> ; <session_id> ST` via `term_core` OSC dispatch (already in place from Phase 3). The handler validates `<socket>` against the allowed prefixes `/tmp/emterm-mux/` and `$XDG_RUNTIME_DIR/emterm-mux/`, validates `<session_id>` against `^[A-Za-z0-9_-]{1,64}$`, then connects via `mux_ipc::connection::Client::connect(socket, session_id)`.
- **FR5: mux detach** — `prefix d` sends `ControlMsg::Detach` to the daemon, closes the client, and resumes the original PTY reader. The original tab/title is restored. Pending mux frames are dropped.
- **FR6: mux window switch** — `prefix n / p / <digit>` sends `ControlMsg::SelectWindow { kind: Next | Prev | Index(u8) }`; the daemon replies with a fresh `Snapshot` that replaces the on-screen `term_core` grid via the existing `term_core::reset_and_replay(...)` API (added if missing).
- **FR7: native PTY pause during mux** — When mux mode is entered, the per-tab native PTY reader thread is paused via an `AtomicBool` flag. Bytes still arriving from the native PTY are accumulated into a bounded ring buffer (256 KB, drop-oldest) so a `Detach` does not deadlock the writer. On detach, the ring buffer is replayed into `term_core`.
- **FR8: prefix key handling** — Prefix key is processed entirely in native-poc (no daemon roundtrip). Default `Ctrl+B`. Configurable via `settings.mux.prefix_key` (see Settings Schema). A double-press sends the prefix literally to the PTY (legacy parity). When mux mode is not active, the prefix is forwarded to PTY unchanged.
- **FR9: status bar widget** — Bottom (or top) egui panel that decodes `StatusUpdateMsg` from the daemon and renders session name, window list (active starred), and local clock (1-second tick via `ctx.request_repaint_after`).
- **FR10: status bar settings** — `settings.statusbar.enabled` (bool, default `true`), `settings.statusbar.position` (`"top"` | `"bottom"`, default `"bottom"`).
- **FR11: Windows MS-IME preedit** — `egui::Event::Ime(ImeEvent::Preedit(text))` is rendered as an overlay at the cursor cell with an underline. Existing `native_poc::render::cursor` is extended (not duplicated).
- **FR12: Windows MS-IME commit** — `egui::Event::Ime(ImeEvent::Commit(text))` writes `text.as_bytes()` to the active PTY via the existing pty writer channel. Bracketed paste wrapping is NOT applied (commits are user typing, not paste).
- **FR13: settings additions** — `settings.json` schema extended with `mux.prefix_key`, `statusbar.enabled`, `statusbar.position`. Backward compatible (missing keys → defaults).

### Non-Functional Requirements

- **NFR1 - Performance:** mux snapshot draw latency < 200 ms for 1 MB snapshot. Prefix key detection < 5 ms. Tab switch within one frame.
- **NFR2 - Stability:** 12-hour Claude Code session with mux attach: no crash, no screen loss, no monotonic memory growth (RSS delta < 50 MB / hour).
- **NFR3 - Linux fcitx5 IME parity:** Phase 1 fcitx5 acceptance criteria continue to pass.
- **NFR4 - Workspace compatibility:** `cargo build --workspace` and `cargo test --workspace` keep passing through Phase 4 (no breakage of `src-tauri`).
- **NFR5 - Module layout:** `crates/mux_ipc/`, `native-poc/src/mux/`, `native-poc/src/ui/{tab_bar,keybinds,status_bar}.rs`, `native-poc/src/ime/`.
- **NFR6 - Logging:** prefix detect, mux attach, detach, window switch, snapshot apply via `log::info!`.

## Implementation Approach

### Architecture

**System Architecture (relevant slice):**
```
┌──────────────────────────────────────────────────────────┐
│  native-poc App (egui + wgpu)                            │
│  ┌──────────┐  ┌────────────┐  ┌─────────────────────┐  │
│  │ TabBar   │  │ TerminalGrid│ │ StatusBar           │  │
│  │ (egui)   │  │ (egui+wgpu) │ │ (egui)              │  │
│  └────┬─────┘  └──────┬──────┘ └──────────┬──────────┘  │
│       │               │                    │             │
│   ┌───▼───┐       ┌───▼────┐         ┌────▼────────┐    │
│   │ Tabs  │       │term_core│         │ Mux Client │    │
│   │ vec   │       │ + image │         │ (per-tab)  │    │
│   └───┬───┘       └─────────┘         └─────┬───────┘   │
│       │   ┌──────────┐                      │           │
│       └──▶│ PtySession│◀─pause/resume───────┘           │
│           └──────────┘                                  │
└────────────────────────────────────┼─────────────────────┘
                                     │ Unix socket
┌────────────────────────────────────▼─────────────────────┐
│  emterm mux daemon (unchanged, legacy `src-tauri/src/mux`│
│  + extracted `crates/mux_ipc`)                           │
└──────────────────────────────────────────────────────────┘
```

**Component Diagram:**
- `crates/mux_ipc` — pure-Rust IPC **protocol data types only** (no tokio, no framing). Used by both `src-tauri` (server) and `native-poc` (new sync client) for `MuxMessage` and friends.
- `native-poc/src/mux/` — new module: `client.rs` (blocking std Unix socket client), `wire.rs` (sync length-prefix framing: 4-byte BE length + bincode body, mirrors `MAX_FRAME_LENGTH` from `mux_ipc::protocol`), `osc777.rs` (attach trigger parsing), `prefix.rs` (prefix key state machine), `mock.rs` (in-memory test daemon).
- `native-poc/src/ui/tab_bar.rs` — egui widget; emits `TabEvent::{New, Close(i), Switch(i)}`.
- `native-poc/src/ui/status_bar.rs` — egui widget; consumes `StatusUpdateMsg`.
- `native-poc/src/ui/keybinds.rs` — central keybind table mapping `egui::Event::Key` to `AppAction`.
- `native-poc/src/ime/` — preedit overlay state + commit dispatch.

### Data Flow

**Attach flow:**
```
PTY  ──(OSC 777 attach)──▶  term_core OSC dispatch
                                ──▶ App::on_mux_attach(socket, session_id)
                                ──▶ Mux::Client::connect()
                                ──▶ daemon::Hello
daemon ──(Snapshot)──▶ Mux::Client
                                ──▶ term_core::reset_and_replay(snapshot)
                                ──▶ App pauses native PTY reader
                                ──▶ TabBar / StatusBar re-render
```

**Detach flow:**
```
egui::Event::Key(Ctrl+B)  ──▶ keybinds (prefix latch)
egui::Event::Key('d')      ──▶ App::on_mux_detach()
                              ──▶ Mux::Client::send(Detach)
                              ──▶ socket close
                              ──▶ replay native PTY ring buffer into term_core
                              ──▶ resume native PTY reader
                              ──▶ TabBar/StatusBar refresh
```

### API Design

Not applicable (no HTTP API). The mux IPC protocol is the existing bincode-over-Unix-socket scheme, unchanged.

#### `crates/mux_ipc` public API (post-extraction, scope-limited)

```rust
pub mod protocol;    // MuxMessage / ClientType / HelloMsg / WelcomeMsg / StatusUpdateMsg /
                     // AttachMsg / ResizeMsg / etc. — pure serde data types, no tokio.
```

`codec` and `connection` are intentionally **NOT** part of `mux_ipc`:
- The legacy tokio_util-based codec stays at `src-tauri/src/mux/ipc/codec.rs`.
- The server connection accept loop stays at `src-tauri/src/mux/ipc/connection.rs`.
- native-poc uses its own sync wire/client modules (see file structure).

#### New `native_poc::mux::client::Client`

```rust
pub struct Client {
    sock: UnixStream,
    session_id: String,
    rx_thread: JoinHandle<()>,
    tx: mpsc::Sender<ClientToServer>,
    rx_events: mpsc::Receiver<ServerToClient>,
}

impl Client {
    pub fn connect(socket_path: &Path, session_id: &str) -> Result<Self, ConnectError>;
    pub fn send(&self, msg: ClientToServer) -> Result<(), SendError>;
    pub fn try_recv(&self) -> Option<ServerToClient>; // called from App main loop
    pub fn shutdown(self);
}
```

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `term_core` (Phase 2): grid + parser used to apply snapshots.
- `term_images` (Phase 3): unchanged; image overlays continue to work in mux mode (daemon-supplied byte stream goes through the same parser).
- `src-tauri/src/mux/`: unchanged daemon side; only `ipc/` subdirectory is moved.

**External Dependencies:**
- `egui` (Phase 1/3 already pinned): IME events, panels.
- `tao`: raw IME events (fallback path if egui's coverage on Windows is insufficient).
- No new third-party crates expected. `mux_ipc` depends on `base64`, `serde`, `serde_derive` (already used today). native-poc's `mux/wire.rs` adds a dep on `bincode` (already in workspace).

### File Structure

```
crates/
├── mux_ipc/                          # NEW (protocol data types only)
│   ├── Cargo.toml                    # deps: base64, serde, serde_derive
│   └── src/
│       ├── lib.rs                    # pub mod protocol;
│       └── protocol.rs               # moved from src-tauri/src/mux/ipc/protocol.rs

src-tauri/src/mux/
├── ipc/                              # codec/connection STAY (server tokio_util glue)
│   ├── codec.rs                      # UNCHANGED location (still tokio_util)
│   ├── connection.rs                 # UNCHANGED location (server handler)
│   ├── protocol.rs                   # 1-line shim: pub use mux_ipc::protocol::*;
│   ├── handlers.rs                   # UNCHANGED
│   ├── pty_spawn.rs                  # UNCHANGED
│   ├── reattach.rs                   # UNCHANGED
│   ├── statusbar.rs                  # UNCHANGED
│   └── mod.rs                        # UNCHANGED
└── mod.rs                            # updated re-exports if needed

native-poc/src/
├── mux/                              # NEW
│   ├── mod.rs
│   ├── wire.rs                       # sync length-prefix framing + bincode
│   ├── client.rs                     # blocking std::os::unix::net::UnixStream client
│   ├── osc777.rs                     # attach-trigger parser
│   ├── prefix.rs                     # prefix-key state machine
│   └── mock.rs                       # in-memory test daemon (cfg(test))
├── ui/
│   ├── tab_bar.rs                    # MAIN IMPL (was 1-line stub)
│   ├── keybinds.rs                   # MAIN IMPL (was 1-line stub)
│   ├── status_bar.rs                 # NEW
│   └── mod.rs
├── ime/                              # NEW (was empty dir)
│   ├── mod.rs
│   ├── preedit.rs                    # overlay rendering
│   └── commit.rs                     # PTY write
└── settings.rs                       # extended for mux/statusbar settings

doc/tasks/mux-tabs-windows-ime/
├── 要件定義書.md
├── SPEC.md                           # this file
├── IMPLEMENTATION.md                 # generated by sdd.2-create-plan
├── VERIFICATION.md                   # generated by sdd.2-create-plan
└── sdd.yaml
```

### Settings Schema

`settings.json` is extended with:

```jsonc
{
  "mux": {
    "prefix_key": "Ctrl+B"            // single-key combo, parseable by keybinds module
  },
  "statusbar": {
    "enabled": true,
    "position": "bottom"              // "top" | "bottom"
  }
}
```

All keys are optional; missing keys fall back to the defaults above. Existing settings files keep parsing without changes.

### Environment Variables

No new environment variables. Existing `EMTERM_MUX=1` / `EMTERM_MUX_SOCKET=<path>` set by daemon in spawned PTYs (legacy behavior) are inherited unchanged.

## Test Scenarios

### Unit Tests

- [ ] **TS-mux-1**: `crates/mux_ipc::protocol` — preexisting `src-tauri/src/mux/ipc/protocol.rs` unit tests pass after `git mv` (zero behavior change).
- [ ] **TS-wire-1**: `native_poc::mux::wire` round-trips a `MuxMessage` via length-prefix + bincode without loss.
- [ ] **TS-wire-2**: `native_poc::mux::wire` rejects frames larger than `MAX_FRAME_LENGTH` with a typed error.
- [ ] **TS-tab-1**: `tab_bar::draw` produces expected `TabEvent` for simulated egui input (new, close, switch).
- [ ] **TS-tab-2**: closing the last tab emits an `AppEvent::ExitWindow` (Phase 1 parity).
- [ ] **TS-tab-3**: when a tab is in mux mode, its title is rendered with the `[mux:<session>]` prefix.
- [ ] **TS-kb-1**: `keybinds::dispatch` maps `Ctrl+Shift+T/W/Tab/digit` correctly.
- [ ] **TS-prefix-1**: prefix state machine: single `Ctrl+B` arms, then next key (`d`, `n`, `p`, digit) triggers action.
- [ ] **TS-prefix-2**: double prefix (`Ctrl+B Ctrl+B`) sends literal `0x02` to PTY when armed.
- [ ] **TS-prefix-3**: prefix latch armed without follow-up within 3 s auto-cancels (literal byte not sent).
- [ ] **TS-osc777-1**: valid `OSC 777 ; emterm ; mux ; attach ; <socket> ; <session_id> ST` parses into the attach action.
- [ ] **TS-osc777-2**: detach OSC (`OSC 777 ; emterm ; mux ; detach ST`) is parsed and triggers detach.
- [ ] **TS-osc777-3**: invalid socket path (outside allowed prefixes) or session ID (not matching pattern) is rejected.
- [ ] **TS-status-1**: `status_bar::draw` renders correctly for representative `StatusUpdateMsg` values.
- [ ] **TS-status-2**: when no mux state is present, only clock is rendered (and only if `statusbar.enabled`).
- [ ] **TS-status-3**: `statusbar.enabled = false` produces no panel at all; unknown `position` falls back to bottom with warn log.
- [ ] **TS-ime-1**: `Ime::Preedit(text)` causes a preedit overlay state update.
- [ ] **TS-ime-2**: `Ime::Commit(text)` enqueues the bytes to the active PTY writer (mocked).
- [ ] **TS-ime-3**: preedit containing C0/C1 bytes is sanitized (control chars stripped) before rendering.
- [ ] **TS-settings-1**: missing `mux.prefix_key` / `statusbar.*` fields fall back to defaults; old `settings.json` files still parse.

### Integration Tests

- [ ] **TS-mux-int-1**: `native_poc::mux::mock` in-memory daemon: client `connect → Hello → Snapshot → SelectWindow → Snapshot → Detach` round-trips with no protocol errors and produces correct `term_core` state transitions.
- [ ] **TS-mux-int-2**: client `connect` to nonexistent socket returns `ConnectError::Io(ENOENT)` and leaves the app in a recoverable state (tab stays in native PTY mode).
- [ ] **TS-mux-int-3**: daemon-side abrupt close mid-session is observed as `ServerToClient` channel closure; client resumes native PTY mode automatically.
- [ ] **TS-mux-int-4**: native PTY pause/resume: bytes arriving while paused accumulate in the 256 KB ring buffer; on detach, ring buffer replay restores screen.

### E2E Tests

**Existing E2E tests**: `e2e-tests/specs/*.e2e.js` (chrome-devtools is NOT applicable to native-poc; Tauri E2E runs against legacy `src-tauri` only).
**Run command**: `./scripts/run-e2e-docker.sh` (legacy only — still gated for `src-tauri` regression).

- [ ] Existing legacy E2E tests pass without regression on `cargo test --workspace` (`src-tauri` unchanged).
- [ ] **TS-manual-mux-1** (host): launch native-poc, `emterm mux new`, attach via OSC 777, verify snapshot draws and prefix keys work.
- [ ] **TS-manual-mux-2** (host): detach with `prefix d` from native-poc, re-attach via `emterm mux attach`, verify state is preserved across detach/reattach.
- [ ] **TS-manual-ime-linux** (host): Linux fcitx5 preedit + commit (Phase 1 parity).
- [ ] **TS-manual-ime-windows** (Windows VM or hardware): MS-IME preedit + commit (gating).

### Edge Cases

- [ ] **EC1**: closing the only tab in a window → window exits (Phase 1 parity).
- [ ] **EC2**: `Ctrl+1..9` jump to nonexistent tab clamps to the last existing tab (no error).
- [ ] **EC3**: receiving `OSC 777 attach` while already in mux mode → log warning, ignore (keep current attach).
- [ ] **EC4**: settings file with `statusbar.position = "left"` → log warning, fall back to `bottom`.
- [ ] **EC5**: preedit text longer than terminal width → wraps within overlay (no truncation).
- [ ] **EC6**: prefix key armed but no follow-up within 3 s → auto-cancel (literal prefix not sent).

### Performance Tests

- [ ] **TS-perf-1**: snapshot apply (1 MB scrollback) completes within 200 ms on dev machine.
- [ ] **TS-perf-2**: prefix detect → daemon send round-trip < 5 ms.

## Security Considerations

- **Socket path validation**: OSC 777 supplied socket path must start with `/tmp/emterm-mux/` or `$XDG_RUNTIME_DIR/emterm-mux/`. Reject otherwise (log + abort attach).
- **Session ID validation**: must match `^[A-Za-z0-9_-]{1,64}$`. Reject otherwise.
- **IME sanitization**: preedit / commit strings are filtered to drop C0 (`0x00-0x1F` excluding `\t`, `\n`) and C1 (`0x80-0x9F`) bytes before rendering / writing to PTY.
- **Settings validation**: `mux.prefix_key` parsed by the keybinds module; unknown tokens fall back to default with a warning log.
- **No new authentication / authorization concerns**: mux daemon socket inherits filesystem permissions; same as legacy.
- **No new XSS / SQL / CSRF surfaces** (no web layer).

## Error Handling

### Error Codes

| Code | Description | Severity | User-Facing Message |
|------|-------------|----------|---------------------|
| MUX_E001 | Invalid OSC 777 socket path (outside allowed prefixes) | warn | (logged only; OSC ignored) |
| MUX_E002 | Invalid OSC 777 session ID | warn | (logged only) |
| MUX_E003 | mux daemon socket connect failure | warn | Status bar shows `mux: not connected` for 3 s |
| MUX_E004 | mux daemon protocol error | warn | Force detach, resume native PTY |
| IME_E001 | egui IME event lost (channel full) | debug | (logged only) |
| CFG_E001 | Unknown `statusbar.position` value | warn | (logged, fall back to bottom) |

### Error Flow

```
OSC 777 received → validate prefix + session_id → connect socket
  ├─ ok → render snapshot, pause PTY
  └─ err → log error, stay in native PTY mode, no UI banner (legacy parity)

mux daemon stream:
  recv loop ──ok──> apply message
            ──io err──> shutdown client, replay ring buffer, resume native PTY,
                       log warn, status bar transient banner (3 s)
```

## Performance Optimization

### Performance Goals

- mux snapshot apply: < 200 ms for 1 MB snapshot
- Prefix key detect: < 5 ms
- Tab switch: < 1 frame (16 ms @ 60 FPS)
- Status bar refresh: ≤ 1 Hz when idle (clock only)

### Optimization Strategies

- **Snapshot replay**: use `term_core::reset_and_replay` to stream snapshot bytes through the existing parser; do not deserialize into intermediate structures.
- **Status bar idle cost**: `ctx.request_repaint_after(Duration::from_secs(1))` so the entire egui pass runs at 1 Hz when only the clock is changing. Active PTY frames still drive the repaint at full rate.
- **Mux RX thread**: a single tokio-free std-thread per client polling the socket; messages forwarded via `mpsc` to the main thread.

### Caching Strategy

- Mux snapshot is consumed once and never cached (the daemon authoritative state is the source of truth).
- Tab title is cached on `term_core` `OSC 0/2` callbacks (no recompute per frame).

## Success Criteria

- [ ] FR1-FR13 implemented; all unit + integration tests above pass.
- [ ] `cargo build --workspace` succeeds on Linux + Windows.
- [ ] `cargo test --workspace` exit 0.
- [ ] `cargo fmt --all -- --check` clean.
- [ ] `cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings`: zero errors. Forward-staged warnings allowed only if documented in `sdd.yaml` notes (Phase 3 precedent).
- [ ] Manual TS-manual-mux-1, TS-manual-mux-2, TS-manual-ime-linux, TS-manual-ime-windows pass on host.
- [ ] 12-hour Claude Code session under mux: no crash / no screen loss / RSS growth < 50 MB/hour (recorded in VERIFICATION_RESULT.md).
- [ ] Legacy `src-tauri` build/test unaffected.

## Open Questions

None blocking implementation. The two design choices below are deliberately deferred and recorded for future re-evaluation:

- [ ] **OQ1**: Drag-to-reorder tabs and right-click context menu — deferred (UX nicety, not blocking parity).
- [ ] **OQ2**: Pane split (multi-pane per window) — out of scope per restruct.md; revisit in a separate SDD if needed.

## Implementation Phases

The plan author (sdd.2-create-plan) will subdivide into ordered sub-phases. Suggested partition:

### Sub-Phase 4-A: `mux_ipc` extraction (protocol-only)
**Goals:** Carve out shared protocol data types; keep both `src-tauri` and workspace green.
**Deliverables:**
- `crates/mux_ipc/` containing only `protocol.rs` (moved from `src-tauri/src/mux/ipc/protocol.rs`)
- `src-tauri/src/mux/ipc/protocol.rs` becomes a 1-line `pub use mux_ipc::protocol::*;` shim
- `codec.rs` and `connection.rs` stay in `src-tauri` (server-only)
- `cargo test --workspace` exits 0

### Sub-Phase 4-B: tab bar + keybinds
**Goals:** egui tab bar widget + central keybinds.
**Deliverables:**
- `native_poc::ui::tab_bar::draw()` fully functional
- `native_poc::ui::keybinds` with all FR2 bindings
- TS-tab-* and TS-kb-* tests passing

### Sub-Phase 4-C: mux client (attach / detach / window switch)
**Goals:** Functional sync mux client + mock-daemon integration tests.
**Deliverables:**
- `native_poc::mux::{wire, client, osc777, prefix, mock}` (`wire.rs` provides sync length-prefix framing + bincode using `mux_ipc::protocol::MuxMessage` and `MAX_FRAME_LENGTH`)
- TS-osc777-*, TS-prefix-*, TS-mux-int-*, TS-wire-* passing
- Manual TS-manual-mux-1 succeeds against a real daemon

### Sub-Phase 4-D: status bar (egui)
**Goals:** egui-rendered status bar consuming `StatusUpdateMsg`.
**Deliverables:**
- `native_poc::ui::status_bar`
- TS-status-* passing
- Settings additions (`statusbar.enabled`, `statusbar.position`)

### Sub-Phase 4-E: Windows MS-IME verification
**Goals:** preedit + commit on Windows via egui built-in IME.
**Deliverables:**
- `native_poc::ime::{preedit, commit}`
- TS-ime-* passing
- Manual TS-manual-ime-windows pass; Linux fcitx5 unaffected

### Sub-Phase 4-F: 12h stability + final gates
**Goals:** long-run check, clippy / fmt / docs final pass.
**Deliverables:**
- VERIFICATION_RESULT.md with 12h session log
- README updated with Phase 4 matrix
- Final `cargo build / test / fmt / clippy` clean

## References

- restruct.md: `tmp/restruct.md` (Phase 4 section + risk mitigation table for WebView fallback)
- Legacy mux spec: `doc/tasks/terminal-multiplexer/SPEC.md`
- Legacy mux status bar spec: `doc/tasks/mux-statusbar/SPEC.md`
- Phase 1 PoC spec: `doc/tasks/native-terminal-poc/SPEC.md`
- Phase 3 features spec: `doc/tasks/native-terminal-features/SPEC.md`
- mux IPC source (current): `src-tauri/src/mux/ipc/`
- native-poc current layout: `native-poc/src/`
