# Implementation Plan: mux Client + Tab Bar UI + Windows IME (Phase 4)

## Overview

Port mux client and tab bar UI from the legacy Tauri build to `native-poc` (tao + wgpu + egui), verify Windows MS-IME on the same stack, and render a native egui status bar. Implemented as six sub-phases (4-A through 4-F) that incrementally close the parity gap with legacy emterm.

## Objectives

- Extract a shared `crates/mux_ipc/` crate so both `src-tauri` and `native-poc` use one IPC implementation.
- Deliver an egui tab bar, central keybinds module, mux client (attach/detach/window switch), prefix-key state machine, native PTY pause-with-ring-buffer.
- Render an egui native status bar consuming the existing `StatusUpdateMsg`.
- Verify Windows MS-IME preedit + commit through egui built-in IME hooks.
- Keep legacy `src-tauri` build/test green throughout (Phase 7 will retire it).

## Prerequisites

### Development Environment

- Rust workspace (existing). Docker E2E image for hermetic build/test (`docker compose -f docker-compose.e2e.yml`).
- Host machine for native-poc smoke runs (Vulkan surface ext required; Docker + Xvfb cannot drive native-poc smoke).
- For Phase 4-E (Windows IME): Windows 10/11 VM or hardware with MS-IME enabled.

### Dependencies

- Phase 1 deliverables: `native-poc/` Cargo project, tao + wgpu + egui scaffold.
- Phase 2 deliverable: `crates/term_core/` (grid + parser used to apply mux snapshots).
- Phase 3 deliverable: `crates/term_images/` (image overlays inherited unchanged into mux mode).
- Legacy mux daemon: `src-tauri/src/mux/{daemon,session,bridge,...}.rs` continues to act as the IPC server; only the `ipc/` subdirectory moves.

## Architecture Overview

### Technology Stack

- **Language**: Rust (stable channel pinned by workspace).
- **GUI**: tao (window/event loop) + wgpu (GPU surface) + egui (immediate-mode UI).
- **Mux IPC**: bincode framed messages over Unix domain socket (unchanged from current `src-tauri/src/mux/ipc/`).
- **Key libraries (already pinned)**: `tao`, `wgpu`, `egui`, `egui-wgpu`, `bincode`, `serde`, `log`, `env_logger`, `arboard`, `notify-rust`.

### Design Approach

Bottom-up extraction first (`mux_ipc` crate), then UI surface (tab bar / keybinds), then behavior (mux client + prefix), then peripheral surface (status bar), then platform verification (Windows IME), then long-run stability gate. Each sub-phase keeps `cargo test --workspace` green; legacy `src-tauri` is never broken.

### Component Interaction

```
[native-poc App] ──holds──> [Tabs vec]
                ──draws──> [TabBar egui widget] ──emits──> AppEvent
                ──draws──> [StatusBar egui widget] ◀──consumes── StatusUpdateMsg
                ──hosts──> [Term grid render (wgpu)]
                ──hosts──> [IME preedit overlay]
[Tab] ──owns──> [PtySession] ──pauses_on──> [Mux mode active]
              ──owns──> [optional Mux Client] ──speaks──> mux_ipc::Client
                                            ──connects──> [emterm mux daemon]
[Prefix Latch] ──reads_from──> egui::Event::Key
              ──drives──> [Mux Client commands]
```

## Implementation Phases

### Phase 4-A: `mux_ipc` Crate Extraction (protocol-only)

**Goal**: Carve out shared protocol data types from `src-tauri/src/mux/ipc/protocol.rs` into a workspace crate while keeping all preexisting `src-tauri` tests green. `codec.rs` (tokio_util-based server framing) and `connection.rs` (server accept loop tied to `crate::mux::session::*`) **stay in src-tauri** because they embed server-only logic — moving them would drag the entire mux runtime across the crate boundary, which violates the no-functional-change requirement.

**Files to Create**:
- `crates/mux_ipc/Cargo.toml` — crate manifest (deps: `base64`, `serde`, `serde_derive`; NO tokio, NO bincode).
- `crates/mux_ipc/src/lib.rs` — `pub mod protocol;` re-export root.

**Files to Modify**:
- top-level `Cargo.toml` — add `crates/mux_ipc` to `[workspace] members`.
- `src-tauri/Cargo.toml` — add `mux_ipc` path dependency.
- `src-tauri/src/mux/ipc/protocol.rs` — replace content with `pub use mux_ipc::protocol::*;` (1-line shim) so existing `super::protocol::*` callers in codec/connection/handlers keep resolving.

**Files to Move** (`git mv`, no content rewrite):
- `src-tauri/src/mux/ipc/protocol.rs` → `crates/mux_ipc/src/protocol.rs` (then immediately re-create the shim file at the original path).

**Files NOT Moved** (server-only or Tauri-bound):
- `codec.rs`, `connection.rs`, `handlers.rs`, `pty_spawn.rs`, `reattach.rs`, `statusbar.rs`, `mod.rs` — all stay in `src-tauri/src/mux/ipc/`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `mux_ipc::protocol` | All wire data types: `MuxMessage`, `ClientType`, `HelloMsg`, `WelcomeMsg`, `StatusUpdateMsg`, etc., plus constants (`PROTOCOL_VERSION`, `MAX_FRAME_LENGTH`) | none | wire-compatible structs usable from sync or async contexts |
| `src-tauri/.../ipc/protocol.rs` (shim) | One-line re-export of `mux_ipc::protocol::*` | mux_ipc available as dep | legacy `use super::protocol::*` continues to work |

**Processing Flow**:

1. Create `crates/mux_ipc/Cargo.toml` + `lib.rs` skeleton.
2. Register the crate as a workspace member.
3. `git mv` `src-tauri/src/mux/ipc/protocol.rs` → `crates/mux_ipc/src/protocol.rs`.
4. Re-create `src-tauri/src/mux/ipc/protocol.rs` as a 1-line `pub use mux_ipc::protocol::*;` shim.
5. Add `mux_ipc` path dep to `src-tauri/Cargo.toml`.
6. Run `cargo build --workspace` + `cargo test --workspace`; both must remain green.

**Implementation Steps**:

1. **Carve out the crate** — author manifest + `lib.rs` exporting `protocol`.
2. **Workspace integration** — add member entry + src-tauri path dep.
3. **Move source** — `git mv` `protocol.rs` into the new crate; preserve content verbatim.
4. **Compatibility shim** — re-create `src-tauri/src/mux/ipc/protocol.rs` as a single-line re-export.
5. **Verify** — full workspace build/test; clippy warning count must not regress.

**Note**: `connection.rs` and `codec.rs` (server-side, tokio_util-based) are explicitly left untouched. native-poc writes its own blocking sync framing in Phase 4-C (`native-poc/src/mux/wire.rs`) using `mux_ipc::protocol::MuxMessage` + bincode + a 4-byte BE length prefix.

**Dependencies**: Requires Phase 3 (term_images extraction precedent); blocks Phase 4-C (mux client requires the crate).

**Testing Approach**:
- Unit: pre-existing IPC tests must continue to pass after the move (TS-mux-1).
- Integration: none new.
- E2E: not applicable.
- Manual: none.

**Acceptance Criteria**:
- [ ] `cargo build --workspace` exits 0.
- [ ] `cargo test --workspace` exits 0.
- [ ] `cargo clippy -p mux_ipc -- -D warnings` clean.
- [ ] No content change to moved files (verify via `git log --follow`).

**Estimated Effort**: small.

---

### Phase 4-B: Tab Bar Widget + Central Keybinds

**Goal**: Replace the stub `native-poc/src/ui/tab_bar.rs` and `keybinds.rs` with functional implementations. New / close / switch / title rendering and FR2 keybinds become operational.

**Files to Create**:
- (none; expand existing stubs)

**Files to Modify**:
- `native-poc/src/ui/tab_bar.rs` — full egui widget implementation (responsibility: render tabs + emit TabEvent).
- `native-poc/src/ui/keybinds.rs` — central keybind dispatch (responsibility: map egui Key event + modifier to AppAction).
- `native-poc/src/ui/mod.rs` — export AppAction enum.
- `native-poc/src/app.rs` — invoke tab bar widget per frame; route TabEvent + AppAction back into tabs vector.
- `native-poc/src/tabs.rs` — minor: ensure `Tab::title()` returns the OSC-supplied title (already cached) or fallback `"shell"`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `tab_bar::draw(ctx, tabs, active_idx)` | Render egui top panel with one button per tab + new-tab button + close-tab affordance | tabs vec non-empty after window init | emits `TabEvent::{New, Close(i), Switch(i)}` or none |
| `keybinds::dispatch(key, mods)` | Map keyboard event to `AppAction` | egui Key event received | returns `Some(AppAction)` or `None` |
| `AppAction` | Enumerate routable user intents (new tab, close tab, switch tab, prefix latch, etc.) | n/a | exhaustive over Phase 4 keybinds |

**Processing Flow**:

1. Per frame, App calls `tab_bar::draw` and `status_bar::draw` and the terminal renderer.
2. Tab bar returns 0..1 `TabEvent`; App applies it (new tab spawns PTY; close tab terminates PTY; switch updates active index).
3. Egui Key events are filtered through `keybinds::dispatch`:
   - If `Ctrl+Shift+T` -> `AppAction::NewTab`.
   - If `Ctrl+Shift+W` -> `AppAction::CloseTab`.
   - If `Ctrl+Tab` -> `AppAction::NextTab`; `Ctrl+Shift+Tab` -> `PrevTab`.
   - If `Ctrl+<digit>` -> `AppAction::JumpTab(N)` clamped to tab count.
   - Else if prefix latch is active (set later in Phase 4-C) -> forward to prefix handler.
   - Else -> passthrough to active PTY writer.
4. Closing the last tab raises `AppEvent::ExitWindow` (matches Phase 1 native-poc behavior).

**Implementation Steps**:

1. **Define AppAction + TabEvent enums** — exhaustive over Phase 4 keybinds and tab UI affordances.
2. **Implement tab_bar widget** — fixed-height top panel, equal-width tabs (min 80px, scroll on overflow), close × button, active indicator.
3. **Implement keybinds dispatch** — table-driven mapping (modifier, key) -> AppAction.
4. **Wire into app loop** — call dispatch before passthrough; route TabEvent + AppAction.
5. **Add unit tests** — TS-tab-1 / TS-tab-2 / TS-kb-1 (drive simulated egui inputs / synthetic key events through dispatch).

**Dependencies**: Requires Phase 1 native-poc app structure; blocks Phase 4-C (prefix latch lives next to keybinds dispatch).

**Testing Approach**:
- Unit: TS-tab-1 / TS-tab-2 (simulated tab events), TS-kb-1 (keybind table).
- Integration: covered indirectly by Phase 4-F manual smoke.
- E2E: not applicable (no project E2E for native-poc).
- Manual: none in this phase.

**Acceptance Criteria**:
- [ ] Tab bar renders and responds to mouse + keyboard.
- [ ] `Ctrl+Shift+T/W`, `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+1..9` map to expected `AppAction`.
- [ ] Closing the last tab exits the window (parity).
- [ ] `cargo test --workspace` green.

**Estimated Effort**: medium.

---

### Phase 4-C: Mux Client (Attach / Detach / Window Switch) + Prefix State Machine

**Goal**: Implement `native-poc/src/mux/*` so a tab can attach to an emterm mux daemon over Unix socket, switch windows via prefix keys, and detach back to the native PTY.

**Files to Create**:
- `native-poc/src/mux/mod.rs` — module root.
- `native-poc/src/mux/wire.rs` — sync length-prefix framing: 4-byte BE prefix + bincode body of `mux_ipc::protocol::MuxMessage`. Validates against `MAX_FRAME_LENGTH`. Public API: `encode_into(buf, &MuxMessage)`, `read_frame(reader) -> Result<MuxMessage, WireError>`.
- `native-poc/src/mux/osc777.rs` — attach/detach OSC parser (parse `OSC 777 ; emterm ; mux ; <action> ; ...` after term_core dispatch).
- `native-poc/src/mux/client.rs` — blocking Unix socket client (`std::os::unix::net::UnixStream`), owns RX thread, exposes `connect / send / try_recv / shutdown`. Uses `wire.rs` for framing.
- `native-poc/src/mux/prefix.rs` — prefix-key latch state machine + per-tab pending-action timeout.
- `native-poc/src/mux/mock.rs` — `#[cfg(test)]` in-memory daemon for integration tests (channel-based, bypasses Unix socket).

**Files to Modify**:
- `native-poc/Cargo.toml` — add `mux_ipc` + `bincode` deps.
- `native-poc/src/app.rs` — hook OSC 777 events from term_core dispatch; integrate prefix latch with keybind layer; pause/resume native PTY reader.
- `native-poc/src/tabs.rs` — `Tab` gains optional `mux_client: Option<Mux Client>` + native PTY pause flag + 256 KB ring buffer.
- `native-poc/src/callbacks.rs` — extend OSC callback set to surface 777-attach/detach to the app layer.
- `native-poc/src/pty/*.rs` (existing reader/writer) — add `paused: AtomicBool` flag observed by reader; bytes during pause go to ring buffer.
- `native-poc/src/settings.rs` — add `mux.prefix_key` field with default `"Ctrl+B"`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `mux::wire::{encode_into, read_frame}` | Sync length-prefix (4-byte BE) + bincode of `MuxMessage`; enforces `MAX_FRAME_LENGTH` | byte stream open | one `MuxMessage` per `read_frame`, or typed `WireError` |
| `mux::osc777::parse(payload)` | Decode OSC 777 attach/detach payload + validate socket path + session ID | OSC 777 with `emterm ; mux` prefix received | `Some(MuxOscAction::{Attach{socket,session_id} | Detach})` or `None` on validation failure |
| `mux::client::Client::connect(path, sid)` | Open blocking `UnixStream`, spawn RX thread that reads frames via `wire::read_frame`, send `Hello{session_id}` | socket path valid, daemon listening | client handle ready to send / try_recv |
| `mux::client::Client::shutdown(self)` | Send `Detach`, drop sender, join RX thread | client handle owned | socket closed, RX joined |
| `mux::prefix::Latch` | Track armed state, follow-up key timeout (3 s), double-press literal passthrough | egui Key event observed by keybinds dispatch | emits `PrefixAction::{None, Literal, Detach, NextWindow, PrevWindow, SelectWindow(u8)}` |
| `mux::mock::run()` | `#[cfg(test)]` in-memory mock daemon driven by channels | none | scripted server responses for unit tests |
| `Tab` (extended) | Hold optional mux client + native PTY paused flag + 256 KB ring buffer | tab owned by app | mux mode toggle is atomic (no half-state) |

**Processing Flow** (attach):

1. PTY emits `OSC 777 ; emterm ; mux ; attach ; <socket> ; <session_id> ST`.
2. `term_core` OSC dispatch raises callback -> `App::on_mux_osc`.
3. `mux::osc777::parse` validates payload:
   - socket path starts with `/tmp/emterm-mux/` or `$XDG_RUNTIME_DIR/emterm-mux/`; otherwise return validation failure.
   - session ID matches `^[A-Za-z0-9_-]{1,64}$`; otherwise return failure.
4. Validation failure -> log warn, ignore.
5. Validation success -> `mux::client::Client::connect`:
   - Connection failure -> log warn, status bar transient banner (3 s), stay in native PTY mode.
   - Connection success -> set tab's `paused = true`, await first `Snapshot`, apply via `term_core::reset_and_replay`.

**Processing Flow** (detach):

1. Keybinds detect `Ctrl+B` -> prefix latch armed (3 s timeout).
2. Next key `d` -> `PrefixAction::Detach`.
3. App sends `ControlMsg::Detach` to mux client.
4. Mux client closes socket and joins RX thread.
5. Tab's ring buffer is drained into `term_core` (preserve characters typed during mux mode? No — during mux mode the native PTY is paused; ring buffer holds only output from any underlying native reader that arrived before suspension. Drain ensures no loss).
6. Native PTY `paused` flag flips to false; reader resumes.
7. Tab bar / status bar redraw.

**Processing Flow** (window switch):

1. Prefix latch armed -> follow-up key:
   - `n` -> `ControlMsg::SelectWindow(Next)`.
   - `p` -> `ControlMsg::SelectWindow(Prev)`.
   - `<digit>` -> `ControlMsg::SelectWindow(Index(d))`.
2. Daemon responds with `Snapshot` -> `term_core::reset_and_replay`.
3. Status bar updates from subsequent `StatusUpdateMsg`.

**Implementation Steps**:

1. **OSC 777 plumbing** — extend `callbacks.rs` so term_core surfaces 777 events to the app; implement `osc777::parse` with validation.
2. **Mux Client wrapper** — `connect / send / try_recv / shutdown`; RX thread bridges socket reads to `mpsc` channel.
3. **Prefix latch** — table-driven state machine with timeout; literal-prefix passthrough on double press.
4. **Pause / ring buffer plumbing** — extend PTY reader to honor pause flag + write into ring buffer; replay on resume.
5. **Tab integration** — `Tab` gains mux_client field; app routes incoming `Snapshot` into `term_core::reset_and_replay`.
6. **Mock daemon for tests** — `mux::mock` exposes a `pair()` helper producing a Client + scripted server endpoint over in-memory transport.
7. **Tests** — TS-osc777-1/2, TS-prefix-1/2, TS-mux-int-1..4.

**Dependencies**: Requires Phase 4-A (mux_ipc) and Phase 4-B (keybinds). Blocks Phase 4-D status bar consumption (status bar wants `StatusUpdateMsg`, which only flows after attach).

**Testing Approach**:
- Unit: TS-osc777-* (parse + validate), TS-prefix-* (state machine).
- Integration: TS-mux-int-* (mock daemon round trip).
- E2E: not applicable.
- Manual: TS-manual-mux-1 / TS-manual-mux-2 (deferred to Phase 4-F).

**Acceptance Criteria**:
- [ ] Attach + detach round-trip succeeds against the mock daemon.
- [ ] Validation rejects invalid socket paths / session IDs.
- [ ] Prefix double-press passes literal byte to PTY.
- [ ] PTY pause + ring buffer + resume works (no byte loss in TS-mux-int-4).
- [ ] `cargo test --workspace` green.

**Estimated Effort**: large.

---

### Phase 4-D: Status Bar (egui)

**Goal**: Render an egui status bar consuming `StatusUpdateMsg`, supporting `statusbar.enabled` and `statusbar.position` settings.

**Files to Create**:
- `native-poc/src/ui/status_bar.rs` — egui widget rendering session/window/clock.

**Files to Modify**:
- `native-poc/src/ui/mod.rs` — export status_bar.
- `native-poc/src/app.rs` — draw status bar at configured position; track latest `StatusUpdateMsg` per tab.
- `native-poc/src/settings.rs` — add `statusbar.enabled` (default true) + `statusbar.position` (default `bottom`).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `status_bar::draw(ctx, state, settings)` | Render egui panel (top or bottom) with session/windows/clock | settings.statusbar.enabled true | egui panel inserted; no panel if disabled |
| `StatusBarState` | Cache last `StatusUpdateMsg` + local clock value | App holds one per active tab | render call returns immediately when state unchanged |
| `Settings::Statusbar` | Persist enable flag + position | settings.json loaded | invalid `position` falls back to bottom with warn log |

**Processing Flow**:

1. Each frame, App fetches active tab's `StatusBarState` (or `None` if no mux).
2. App computes current local `HH:MM:SS`.
3. `status_bar::draw` decides panel placement from `settings.statusbar.position`:
   - `top` -> insert above terminal.
   - `bottom` -> insert below terminal.
   - unknown -> fall back to bottom (log warn once per session).
4. If status state is `None`, show only clock (still respecting `statusbar.enabled`).
5. Repaint cadence: `ctx.request_repaint_after(1s)` when idle (clock-only updates).

**Implementation Steps**:

1. **Settings extension** — extend serde model + validation; emit `WARN` on unknown position.
2. **StatusBarState plumbing** — App stores latest `StatusUpdateMsg` per tab; updated by Mux Client RX path.
3. **Widget implementation** — egui top/bottom panel; clock + session/window strip.
4. **Idle repaint** — schedule 1 s repaint when no mux state changed in the last frame.
5. **Tests** — TS-status-1 / TS-status-2.

**Dependencies**: Requires Phase 4-C (Mux Client provides `StatusUpdateMsg`).

**Testing Approach**:
- Unit: TS-status-1 (render representative message), TS-status-2 (no state).
- Integration: smoke under TS-mux-int-1 (status updates flow after attach).
- E2E: not applicable.
- Manual: covered by TS-manual-mux-1 in Phase 4-F.

**Acceptance Criteria**:
- [ ] Active window starred in window list.
- [ ] Clock updates at 1 Hz.
- [ ] `statusbar.enabled = false` hides the panel entirely.
- [ ] Unknown `statusbar.position` falls back to bottom with warn log.

**Estimated Effort**: small.

---

### Phase 4-E: Windows MS-IME Verification + Linux fcitx5 Parity

**Goal**: Implement preedit overlay + commit handling via egui's IME events and confirm Windows MS-IME `preedit` + `commit` work. Re-verify Linux fcitx5 has not regressed.

**Files to Create**:
- `native-poc/src/ime/mod.rs` — module root.
- `native-poc/src/ime/preedit.rs` — overlay state + sanitization.
- `native-poc/src/ime/commit.rs` — PTY writer dispatch.

**Files to Modify**:
- `native-poc/src/app.rs` — route `egui::Event::Ime` to ime module.
- `native-poc/src/render/cursor.rs` — extend cursor renderer to draw preedit underline overlay near the active cell.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `ime::preedit::State` | Track current preedit string + cursor location anchor | IME enabled, egui IME event received | renderer can query latest preedit text + anchor |
| `ime::preedit::sanitize(s)` | Drop C0 (except `\t`,`\n`) and C1 bytes | input string from IME | returns rendering-safe text |
| `ime::commit::write(pty, s)` | Sanitize + write commit bytes to active PTY | active PTY writer present | bytes written exactly once; no bracketed-paste wrap |
| `render::cursor::draw_with_preedit` | Render cursor + preedit overlay underline at anchor cell | preedit state non-empty | preedit visible below cursor, wrap-aware |

**Processing Flow**:

1. Egui delivers `Event::Ime(ImeEvent::Preedit(text))`:
   - sanitize -> update `preedit::State::current`.
   - request repaint.
2. Egui delivers `Event::Ime(ImeEvent::Commit(text))`:
   - sanitize.
   - send bytes to active PTY writer.
   - clear `preedit::State`.
3. Renderer reads `preedit::State` and draws underline overlay at cursor anchor.
4. If app focus is lost or PTY closes, `preedit::State` is cleared.

**Implementation Steps**:

1. **Preedit state** — store latest string + anchor cell, sanitization function.
2. **Commit path** — wire `commit` to active tab's PTY writer; no bracketed paste wrap.
3. **Cursor renderer extension** — draw underline span beneath the cursor cell across the preedit length.
4. **Tests** — TS-ime-1 / TS-ime-2 / TS-ime-3 (sanitize C0/C1).
5. **Manual verification** — TS-manual-ime-linux (parity), TS-manual-ime-windows (gating).

**Dependencies**: Requires Phase 4-B (tab bar drives focus). Blocks Phase 4-F manual 12 h session if Windows IME is the user's primary input.

**Testing Approach**:
- Unit: TS-ime-1/2/3.
- Integration: not applicable (no automated IME driver).
- E2E: not applicable.
- Manual: TS-manual-ime-linux, TS-manual-ime-windows.

**Acceptance Criteria**:
- [ ] Preedit overlay renders aligned with cursor cell on both Linux and Windows.
- [ ] Commit writes exactly the displayed string to the PTY (no duplication / loss).
- [ ] Linux fcitx5 acceptance from Phase 1 still passes.
- [ ] Windows MS-IME preedit + commit work (candidate position best effort).

**Estimated Effort**: medium.

---

### Phase 4-F: Long-Run Stability + Final Gates

**Goal**: Manual 12-hour Claude Code session under mux; final `fmt`/`clippy`/README pass; update VERIFICATION_RESULT.md with measured values.

**Files to Create**:
- (none; documentation updates only)

**Files to Modify**:
- `native-poc/README.md` — Phase 4 feature matrix (mux client / status bar / Windows IME row).
- `doc/tasks/mux-tabs-windows-ime/VERIFICATION_RESULT.md` — created by sdd.6-verify (not this phase).
- Any clippy-style fixes surfaced after Phase 4-A through 4-E.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| 12 h soak run | Manual long-run with mux attached | host machine available | recorded RSS samples + crash log |
| Final gate | `cargo fmt --all --check`, `cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` | all phases merged | exit 0 across the board |
| README matrix | Document Phase 4 deliverables | implementation complete | matrix updated |

**Processing Flow**:

1. Run final gates locally + in Docker.
2. Resolve any clippy style violations (precedent: Phase 3 commit `4b6710b`).
3. Start a Claude Code session under mux for 12 hours, sampling RSS hourly (e.g. via `ps` snapshots) and noting any crash / screen-loss event.
4. Capture measurements in VERIFICATION_RESULT.md (written by sdd.6-verify, not here).

**Implementation Steps**:

1. **Fmt + clippy sweep** — apply mechanical fixes; document any new forward-staged warnings in sdd.yaml notes.
2. **README update** — add Phase 4 matrix row.
3. **Long-run kickoff** — start manual session; record sampling cadence.
4. **Acceptance report** — fold measurements into VERIFICATION_RESULT.md in sdd.6.

**Dependencies**: Requires Phases 4-A through 4-E complete and merged on the working branch.

**Testing Approach**:
- Unit: re-run.
- Integration: re-run.
- E2E: legacy regression gate `./scripts/run-e2e-docker.sh` (precedent: same fail list as `main`, treated as preexisting).
- Manual: 12 h soak (TS-manual-soak); Windows IME long-run if applicable.

**Acceptance Criteria**:
- [ ] All final gates exit 0.
- [ ] No clippy errors (warnings only allowed if documented as forward-staged).
- [ ] 12 h soak: no crash, no screen loss, RSS growth < 50 MB/hour.
- [ ] README matrix reflects Phase 4 deliverables.

**Estimated Effort**: medium (mostly soak time).

---

## Complete File Structure

```
crates/
└── mux_ipc/                          # NEW (Phase 4-A; protocol-only)
    ├── Cargo.toml                    # deps: base64, serde, serde_derive
    └── src/
        ├── lib.rs                    # pub mod protocol;
        └── protocol.rs               # moved from src-tauri/src/mux/ipc/protocol.rs

src-tauri/src/mux/
├── ipc/                              # codec/connection STAY (server-only tokio_util glue)
│   ├── codec.rs                      # UNCHANGED (tokio_util)
│   ├── connection.rs                 # UNCHANGED (server accept loop)
│   ├── protocol.rs                   # SHIM: pub use mux_ipc::protocol::*;
│   ├── handlers.rs                   # UNCHANGED
│   ├── pty_spawn.rs                  # UNCHANGED
│   ├── reattach.rs                   # UNCHANGED
│   ├── statusbar.rs                  # UNCHANGED
│   └── mod.rs                        # UNCHANGED
└── mod.rs                            # unchanged unless internal references need adjust

native-poc/src/
├── mux/                              # NEW (Phase 4-C)
│   ├── mod.rs
│   ├── wire.rs                       # sync length-prefix + bincode framing
│   ├── osc777.rs
│   ├── client.rs                     # blocking UnixStream + RX thread
│   ├── prefix.rs
│   └── mock.rs                       # cfg(test)
├── ime/                              # NEW (Phase 4-E)
│   ├── mod.rs
│   ├── preedit.rs
│   └── commit.rs
├── ui/
│   ├── tab_bar.rs                    # FULL IMPL (Phase 4-B)
│   ├── keybinds.rs                   # FULL IMPL (Phase 4-B)
│   ├── status_bar.rs                 # NEW (Phase 4-D)
│   └── mod.rs
├── settings.rs                       # extended (mux.prefix_key, statusbar.*)
├── tabs.rs                           # extended (mux_client field, paused flag)
├── pty/                              # extended (pause flag honored by reader)
└── app.rs                            # extended (mux + ime routing)
```

## Testing Strategy

- **Unit**: TS-mux-1 (protocol move parity), TS-wire-1/2 (sync framing), TS-tab-1/2/3 (tab events incl. mux mode title prefix), TS-kb-1 (keybinds), TS-prefix-1/2/3 (prefix incl. timeout), TS-osc777-1/2/3, TS-status-1/2/3, TS-ime-1/2/3, TS-settings-1.
- **Integration**: TS-mux-int-1/2/3/4 (mock daemon round trips).
- **E2E**: existing legacy gate (`./scripts/run-e2e-docker.sh`) maintained as preexisting regression check; no new native-poc E2E (chrome-devtools not applicable).
- **Manual**: TS-manual-mux-1/2, TS-manual-ime-linux/windows, TS-manual-soak (12 h).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| bincode | (already pinned in workspace) | mux IPC wire format |
| serde / serde_derive | already pinned | message struct derive |
| egui / egui-wgpu | already pinned | tab bar / status bar / IME |
| tao | already pinned | window + raw IME fallback |
| arboard | already pinned | unaffected (used by Phase 3) |
| notify-rust | already pinned | unaffected |
| log / env_logger | already pinned | structured logging |

No new third-party crates introduced.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| egui Windows IME incomplete | medium | high | Fall back to tao raw IME plumbing; ultimate fallback: WebView hybrid (restruct.md risk table). |
| egui tab bar / status bar UX feels limiting | medium | medium | Documented WebView hybrid fallback in restruct.md; revisit if user dissatisfaction surfaces. |
| Mux daemon protocol drift during crate extraction | low | high | Move files verbatim with `git mv`; do not edit content. Verify by full test suite run. |
| Ring buffer overflow during long mux session | low | medium | 256 KB drop-oldest semantics; log on overflow; document in VERIFICATION.md. |
| Prefix latch timeout off-by-one (1 s vs 3 s drift) | low | low | Cover in TS-prefix-2; pin timeout via constant. |

## Open Questions

- [ ] OQ1 (from SPEC): drag-to-reorder tabs + right-click context menu — deferred.
- [ ] OQ2 (from SPEC): pane split — out of scope.
- [ ] **Implementation-specific**: ring buffer size (256 KB) chosen as plausible default; revisit if a 12 h soak shows overflows.
- [ ] **Implementation-specific**: prefix latch timeout (3 s) chosen to match common tmux defaults; revisit if user feedback differs.

## Success Metrics

- [ ] All FR1-FR13 implemented and covered by unit/integration tests.
- [ ] `cargo test --workspace` exit 0 with at least +30 new native-poc tests.
- [ ] `cargo fmt --all --check` clean; `cargo clippy -p emterm-native-poc -p mux_ipc -- -D warnings` exit 0.
- [ ] Manual TS-manual-mux-1/2, TS-manual-ime-linux/windows, TS-manual-soak all pass.
- [ ] Legacy `src-tauri` build/test green throughout.
