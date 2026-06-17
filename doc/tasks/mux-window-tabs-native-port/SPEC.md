# Feature: mux Window Tabs (native-poc port)

## Overview

Port the WebView mux tab-group UI into native-poc (tao + wgpu + egui). A native tab attached to a mux session becomes a "tab group" that toggles between **compact** (`mux (N)`) and **expanded** (one sub-tab per mux window). Window switch / create / rename / move are driven from the GUI using the existing APC-over-PTY inband protocol — native-poc never opens the daemon's Unix socket.

This is a follow-up to Phase 4 (`doc/tasks/mux-tabs-windows-ime/`), which implemented mux attach, status bar, and window switch via PTY-byte passthrough. Phase 4 deliberately did NOT expand mux windows into native tabs; this task adds that UI plus the structured GUI→daemon control path (rename/move carry payloads that cannot be expressed as raw prefix bytes).

## Objectives

- Render a mux tab group in egui with compact/expanded states and per-window sub-tabs (parity with `src/terminal/mux/tab-group.ts`).
- Maintain per-tab mux window state (window list, active index, pane IDs) updated from daemon-pushed APC messages.
- Send structured mux control messages (CreateWindow / SwitchWindow / RenameWindow / MoveWindow / Detach) by APC-encoding `MuxMessage` and writing to the PTY (parity with `MuxClient.sendControl`).
- Wire `mux::prefix::Latch` into the keybinds dispatch and extend it with new-window / rename / move actions (`mux.keybinds`).
- Load and dynamically apply `mux.tab_always_expand`, `mux.status_position`, `mux.keybinds`, `mux.statusbar.*`.

## User Stories

### US1: List mux windows as a tab group
As a mux user, I want my session's windows shown as a tab group so that I can see and reach every window from the tab bar.

**Acceptance Criteria:**
- [ ] Attaching to a mux session turns the originating tab into a tab group.
- [ ] The group renders one sub-tab per window, labelled `[N] name`, with the active window highlighted.
- [ ] A single mux window still renders as a sub-tab group (group dissolves only when the last window exits).
- [ ] Clicking a sub-tab switches to that window.
- [ ] There is no compact/expanded toggle (WebView parity).

### US2: Switch mux windows
As a mux user, I want to switch windows via sub-tab click or `prefix n/p/0..9` so that I keep tmux-equivalent ergonomics.

**Acceptance Criteria:**
- [ ] Sub-tab click activates that window.
- [ ] `prefix n` / `prefix p` cycle windows (wrap-around).
- [ ] `prefix 0..9` jumps to that window (clamped to existing windows).
- [ ] After a switch the daemon-pushed snapshot replaces the on-screen contents.

### US3: Create / rename / move / close windows
As a mux user, I want to create, rename, reorder and close windows from the GUI so that I manage my session without leaving native-poc.

**Acceptance Criteria:**
- [ ] `prefix c` creates a new window; the sub-tab appears when the daemon confirms.
- [ ] `prefix ,` opens a rename dialog seeded with the current name; confirming updates the label locally and notifies the daemon.
- [ ] `prefix m` opens a move dialog; confirming reorders locally (optimistic) and notifies the daemon, rolling back on send failure.
- [ ] When a window's shell exits, its sub-tab is removed; dropping to one window dissolves the group.

### US4: mux settings applied dynamically
As a user, I want mux UI settings to take effect on save so that I don't restart to see changes.

**Acceptance Criteria:**
- [ ] `mux.tab_always_expand`, `mux.status_position`, `mux.keybinds`, `mux.statusbar.*` are read by the native loader.
- [ ] Saving settings re-applies them via `apply_settings` without a restart.

## Technical Requirements

### Functional Requirements

- **FR1 — Tab group UI**: egui tab bar renders a mux tab group as one sub-tab per window, labelled `[N] name`, with the active window highlighted. Rendered whenever the group holds at least one window; dissolves to a plain tab only at zero windows. Sub-tab click switches windows. No compact/expanded toggle. Port of the WebView `renderMuxSubTabs`.
- **FR2 — Window state model**: each mux-attached `Tab` holds `windows: Vec<MuxWindow{ id, name }>`, `active_window_index: usize`, `pane_ids: Vec<u32>`, `expanded: bool`.
- **FR3 — APC control send path**: a `send_control(msg_type, pane_id, payload)` that APC-encodes a `mux_ipc::protocol::MuxMessage` and writes it to the active PTY (`writeDirect` equivalent). Fire-and-forget; responses arrive as inbound APC. native never opens the daemon socket.
- **FR4 — Inbound message handling**: extend `Tab::apply_mux_message` to handle `PaneCreated`, `SwitchWindow`, `RenameWindow`, `PtyExited` (per-pane removal), and to ingest `SessionInfo.windows` / `active_window_index` from `Welcome`.
- **FR5 — Window switch**: sub-tab click and `PrefixAction::{NextWindow, PrevWindow, SelectWindow}` update the local active index, send `SwitchWindow`, and apply the daemon snapshot.
- **FR6 — New window**: `prefix c` (`PrefixAction::NewWindow`) sends `CreateWindow`; `PaneCreated` appends the window to the list. When the appended window becomes the active sub-tab (per `MuxWindowGroup::push`), the active terminal core is also reset (`reset_and_replay(b"")` via the shared `reset_frame_for_replay` helper) so the new tab opens on a clean screen and any pre-existing absolute-row selection / press anchor is dropped via `pending_frame_reset`.
- **FR7 — Rename window**: `prefix ,` opens an egui rename dialog; on confirm, optimistic local label update + send `RenameWindow { name }` with the active pane id. Inbound `RenameWindow` from the daemon also updates the label.
- **FR8 — Move window**: `prefix m` opens an egui move dialog; on confirm, optimistic local reorder + send `MoveWindow { target_index }`. The daemon does NOT broadcast the new order, so local state is authoritative; roll back on send failure.
- **FR9 — Window close**: inbound `PtyExited` removes the matching window/pane; collapsing to one window dissolves the group.
- **FR10 — Prefix latch wiring + extension**: wire `mux::prefix::Latch` into the keybinds dispatch (currently forward-staged). Add `NewWindow`, `RenameWindow`, `MoveWindow` to `PrefixAction`. Action bindings come from `mux.keybinds` (defaults: `d`/`c`/`n`/`p`/`,`/`m`, tmux-compatible). Double-prefix emits the literal byte.
- **FR11 — Settings load + dynamic apply**: add `mux.tab_always_expand`, `mux.status_position`, `mux.keybinds`, `mux.statusbar.*` to the native settings loader; apply via `App::apply_settings`.

### Non-Functional Requirements

- **NFR1 — Performance**: no impact on the PTY hot path; APC detection reuses the existing `on_apc` route.
- **NFR2 — SSH transparency**: keep APC inband (no socket); mux works identically over SSH.
- **NFR3 — Compatibility**: `src-tauri` build/test unaffected; `crates/mux_ipc` stays shared and unchanged in wire format.
- **NFR4 — Usability**: behavior parity with the WebView mux tab group.

## Implementation Approach

### Architecture

Both WebView and native-poc use the **APC-over-PTY inband protocol**; only the `emterm mux` bridge process owns a Unix socket. native-poc reads inbound APC from the PTY stream and writes outbound APC to it.

```
                 APC over PTY                Unix socket
  native-poc  ───────────────▶  emterm mux  ───────────▶  mux daemon
  (egui GUI)  ◀───────────────  (bridge,      ◀───────────  (sessions/
              inbound APC        in PTY)        MuxMessage    windows/panes)
                                                frames)
```

Outbound (GUI → daemon): `MuxMessage{ msg_type, pane_id, payload }` → APC encode (`ESC _ emterm-mux;<base64(frame_body)> ESC \`) → PTY write. Mirror of `MuxClient.sendControl`.

Inbound (daemon → GUI): PTY output contains `emterm-mux;<base64>` APC → `on_apc` → `MuxMessage::from_apc` → `App::on_mux_message` → `Tab::apply_mux_message`.

### Data Flow

```
prefix c ─▶ PrefixAction::NewWindow ─▶ send_control(CreateWindow) ─▶ (bridge) ─▶ daemon
                                                                                   │
PaneCreated APC ◀──────────────────────────────────────────────────────────────────┘
   └─▶ Tab::apply_mux_message ─▶ windows.push / pane_ids.push ─▶ tab bar re-render
                              └─▶ reset_frame_for_replay(b"") ─▶ screen clear + pending_frame_reset latch

sub-tab click / prefix n ─▶ active_window_index = i ─▶ send_control(SwitchWindow, pane_ids[i])
   └─▶ daemon Snapshot APC ─▶ reset_and_replay ─▶ screen swap
```

### Message Mapping (port reference)

| Action | Outbound MuxMessage | Inbound effect |
|--------|---------------------|----------------|
| attach | (existing) | `Welcome.SessionInfo{ windows, active_window_index }` → seed window list |
| new window | `CreateWindow` | `PaneCreated{ pane_id }` → append window + reset active core (via `reset_frame_for_replay`) |
| switch | `SwitchWindow` (pane_id) | `Snapshot` → screen swap; `SwitchWindow` (remote) → sync active index |
| rename | `RenameWindow{ name }` (active pane) | `RenameWindow{ name }` → update label |
| move | `MoveWindow{ target_index }` | none (daemon does not broadcast); local optimistic |
| close | — | `PtyExited` → remove window/pane |
| detach | `Detach` | `Detached` → dissolve group, restore native PTY |

WebView source of truth: `src/terminal/mux/mux-client.ts` (`handleIncomingApc`, `sendControl`), `src/terminal-app/mux/mux-window-manager.ts` (`switchMuxWindow`, `handleMuxPaneCreated`, `handleMuxPaneExited`, `reorderMuxWindows`, `handleRemoteSwitchWindow`), `src/terminal-app/mux/mux-action-handler.ts` (`handleMuxAction`), `src/terminal/mux/prefix-key.ts` (`DEFAULT_ACTION_BINDINGS`), `src/terminal/mux/tab-group.ts`.

### Dependencies

**Internal:**
- `crates/mux_ipc` — `MuxMessage`, `MessageType`, `SessionInfo`, `WindowInfo`, `RenameWindowMsg`, `MoveWindowMsg`, `CreateWindowPayload` (unchanged).
- `native-poc/src/mux/apc.rs` — APC decode (extend with an encode counterpart for the send path).
- `native-poc/src/mux/prefix.rs` — `Latch` / `PrefixAction` (wire + extend).
- `native-poc/src/tabs.rs` — `apply_mux_message`, mux window state.
- `native-poc/src/app.rs` — `on_mux_message`, `apply_settings`, prefix dispatch.
- `native-poc/src/ui/tab_bar.rs` — tab group rendering.
- `native-poc/src/settings.rs` — mux settings loader.

**External:** none new.

### File Structure

```
native-poc/src/
├── mux/
│   ├── apc.rs              # + encode_mux_message (APC out)
│   ├── prefix.rs           # + NewWindow / RenameWindow / MoveWindow actions; wired to dispatch
│   └── window_group.rs     # NEW: MuxWindow state model + compact/expanded controller (tab-group.ts port)
├── ui/
│   ├── tab_bar.rs          # + tab group / sub-tab rendering
│   ├── rename_window_dialog.rs  # NEW: egui rename dialog
│   └── move_window_dialog.rs    # NEW: egui move dialog
├── tabs.rs                 # apply_mux_message extension + window state on Tab
├── app.rs                  # prefix dispatch + mux action handling + apply_settings
└── settings.rs             # mux.* loader (tab_always_expand / status_position / keybinds / statusbar)
```

(Exact module split finalized in IMPLEMENTATION.md.)

## Test Scenarios

### Unit Tests
- [ ] `apply_mux_message` PaneCreated appends a window + pane id, resets the active terminal core (via `reset_frame_for_replay(b"")`), and latches `pending_frame_reset` so any pre-existing selection is dropped.
- [ ] `apply_mux_message` PtyExited removes the matching window; one-window collapse dissolves the group.
- [ ] `apply_mux_message` RenameWindow updates the label by window id.
- [ ] Welcome ingests `SessionInfo.windows` + `active_window_index`.
- [ ] Window state model: compact/expanded toggle, `getCompactLabel` = `mux (N)`, active-index clamping on shrink.
- [ ] Prefix latch: `c`/`,`/`m`/`n`/`p`/`0..9`/double-prefix map to the right `PrefixAction`; custom `mux.keybinds` override defaults; unknown key cancels.
- [ ] Move: optimistic reorder then rollback on simulated send failure.
- [ ] APC encode round-trips with the existing decoder for CreateWindow / RenameWindow / MoveWindow / SwitchWindow.

### Integration Tests
- [ ] Mock inbound APC stream (attach → new window → switch → rename → close) drives the window list to the expected state.

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (`./scripts/run-e2e-docker.sh`).
**Run command**: `./scripts/run-e2e-docker.sh test`
- Not in scope for this task (unit-test-centric per requirements). Manual GUI gates are host-deferred (Docker cannot drive native windows / mux daemon).

### Edge Cases
- [ ] `prefix 0..9` beyond window count → clamp, no-op past range.
- [ ] Rename/move dialog open while the target window closes → abort by stable window id.
- [ ] `mux.keybinds` with an invalid chord → warn + fall back to default.
- [ ] Switch with a single window → no-op.

## Security Considerations

- **Input Validation:** rename text and move target are clamped/sanitized as in the WebView (`[1, N]` range for move; empty name ignored). APC payloads decoded with existing bounded checks.
- **No new attack surface:** native does not open a socket; the bridge owns the daemon connection.

## Error Handling

| Condition | Handling |
|-----------|----------|
| MoveWindow send fails | roll back optimistic local reorder, log warn |
| Malformed inbound APC payload | log warn, ignore (existing behavior) |
| CreateWindow with no PaneCreated reply | window not added (no phantom sub-tab) |
| Invalid `mux.keybinds` chord | warn, keep default binding |

## Success Criteria

- [ ] All FRs implemented and unit-tested.
- [ ] `cargo test` (native-poc) green; `cargo fmt` / `clippy` clean (forward-staged-warning policy as Phase 3/4).
- [ ] `src-tauri` build/test unaffected.
- [ ] Behavior parity with the WebView mux tab group (manual host gate).

## Open Questions

> Unresolved requirements are tracked in sdd.yaml as `status: tbd`. Resolve before `/em-sdd:sdd.2-create-plan`.

- [ ] FR6: initial window name when `PaneCreated` carries no name — confirm the WebView naming rule during planning.
- [ ] FR9: whether an explicit "kill window" action exists in the WebView (vs. removal only on shell exit) — confirm during planning.

## References

- Phase 4 SDD: `doc/tasks/mux-tabs-windows-ime/`
- APC inband protocol: `doc/tasks/mux-inband-protocol/SPEC.md`
- Protocol types: `crates/mux_ipc/src/protocol.rs`
- WebView mux client: `src/terminal/mux/`, `src/terminal-app/mux/`
