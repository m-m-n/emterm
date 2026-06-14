# Implementation Plan: mux Window Tabs (native-poc port)

## Overview

Add a WebView-equivalent mux tab group to native-poc: a mux-attached tab becomes a group that toggles compact (`mux (N)`) / expanded (per-window sub-tabs), with window switch / create / rename / move driven over the existing APC-over-PTY inband protocol (no daemon socket).

## Objectives

- Maintain per-tab mux window state and render it as a tab group in egui.
- Send structured mux control messages by APC-encoding `MuxMessage` to the PTY (parity with WebView `MuxClient.sendControl`).
- Wire and extend the prefix-key latch for new-window / rename / move; load and dynamically apply `mux.*` UI settings.

## Prerequisites

### Development Environment
- Rust toolchain (workspace pinned). Build via `CARGO_TARGET_DIR` paths per `.claude/rules/native-poc-build-location.md` (never `cd native-poc/`).

### Dependencies
- `crates/mux_ipc` (existing, unchanged) — `MuxMessage`, `MessageType`, `SessionInfo`, `WindowInfo`, `RenameWindowMsg`, `MoveWindowMsg`, `CreateWindowPayload`.
- Phase 4 mux attach path (existing) — APC decode, `Tab::apply_mux_message`, status bar, `mux::prefix::Latch` (forward-staged).

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: tao + wgpu + egui (native-poc)
- **Key Libraries**: `mux_ipc` (protocol), `egui` (tab bar / dialogs)

### Design Approach

Both WebView and native-poc use APC inband; only the `emterm mux` bridge owns the daemon socket. native-poc reads inbound APC (`on_apc`) and writes outbound APC to the same PTY. This task makes the native side **symmetric**: it already decodes inbound APC, so we add an outbound APC encoder and the window-state bookkeeping the decoder feeds.

Window state lives on the mux-attached `Tab`. The tab bar reads that state to render the group. Prefix actions and sub-tab clicks mutate the active index / window list and emit outbound APC; daemon responses (Snapshot / PaneCreated / RenameWindow / PtyExited) flow back through the existing decode route and reconcile the state.

### Component Interaction

```
keyboard ─▶ prefix Latch ─▶ App mux-action dispatch ─▶ Tab.send_control (APC out) ─▶ PTY ─▶ bridge ─▶ daemon
tab bar  ─▶ sub-tab click ─▶ App switch/select ──────────┘
PTY in ─▶ on_apc ─▶ App.on_mux_message ─▶ Tab.apply_mux_message ─▶ window state ─▶ tab bar render
settings save ─▶ App.apply_settings ─▶ keybind table + tab_always_expand + statusbar/status_position
```

## Implementation Phases

### Phase 1: Window state model + settings loader

**Goal**: A mux-attached tab holds an ordered window list with active index and expansion state; mux UI settings are read by the native loader.

**Files to Create**:
- `native-poc/src/mux/window_group.rs` - mux window state model + compact/expanded controller (port of `tab-group.ts`).

**Files to Modify**:
- `native-poc/src/tabs.rs` - add mux window state fields to the mux-attached tab.
- `native-poc/src/settings.rs` - load `mux.tab_always_expand`, `mux.status_position`, `mux.keybinds`, `mux.statusbar.*` into the native settings model.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Window list model | Hold ordered windows (id, name), active index, pane ids, expanded flag | Tab is mux-attached | Queries return consistent window/active state |
| Group label | Produce the compact label `mux (N)` | Window list present | Label reflects current window count |
| Expansion controller | Toggle / set compact↔expanded; seed from `tab_always_expand` | — | Expansion state matches user intent / setting |
| mux settings loader | Map `mux.*` keys into native settings, warn+default on invalid | settings.json parsed | mux fields available to the app |

**Processing Flow**:
1. On mux attach, initialize window list from the session (Phase 2 feeds it); expansion seeded from `tab_always_expand`.
2. Active-index changes clamp into `[0, len-1]`; shrinking the list re-clamps.

**Invariant (F1)**: the window list and `pane_ids` are parallel, index-aligned collections (same length, same order) — every mutation (append / remove / reorder) must update both together, matching the WebView `muxWindows` / `muxPaneIds` pairing.

**Implementation Steps**:
1. **Window state model** - define the window/active/expanded state and its query/mutate contracts (no protocol concerns yet).
2. **Compact/expanded controller** - toggle + label + active-window accessors, seeded by `tab_always_expand`.
3. **Tab integration** - attach the state to the mux tab; lifecycle (created/cleared on attach/detach).
4. **Settings loader** - read the four `mux.*` groups with validation and defaults matching src-tauri / WebView.

**Dependencies**: Blocks Phase 2-5.

**Testing Approach**:
- Unit: label = `mux (N)`; toggle transitions; active-index clamp on shrink; loader parses valid values and warns+defaults on invalid keybinds.

**Acceptance Criteria**:
- [ ] Window state queries/mutations behave per contract.
- [ ] mux settings load with correct defaults.

**Estimated Effort**: medium

---

### Phase 2: Bidirectional APC (outbound encode + inbound window reconcile)

**Goal**: native can send structured mux control messages and reconcile window state from daemon-pushed messages.

**Files to Modify**:
- `native-poc/src/mux/apc.rs` - add an outbound APC encoder (counterpart to the existing decoder).
- `native-poc/src/tabs.rs` - `apply_mux_message` handles `PaneCreated`, `SwitchWindow`, `RenameWindow`, `PtyExited`; `Welcome` ingests `SessionInfo.windows` + `active_window_index`; add a `send_control` contract that hands the encoded APC to the PTY writer.
- `native-poc/src/app.rs` - route the new inbound message effects; expose a path for actions to call `send_control` on the active mux tab.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| APC encoder | Encode `MuxMessage{type,pane_id,payload}` to an APC byte sequence | valid message | Bytes round-trip with the existing decoder |
| send_control | Encode + write to the active PTY (fire-and-forget) | tab has a live PTY | Bytes queued to PTY; no socket opened |
| Welcome ingest | Seed window list + active index from session info (additive; keep existing `mux_session_name` extraction intact) | attach handshake | Window list matches daemon at attach time; session name still set as before (F3) |
| PaneCreated handler | Append a window (initial name "Terminal"; daemon id = list index for fresh creates) | a create is pending | New window/pane appended, becomes active |
| PtyExited handler | Remove the window/pane for the exited pane; collapse group at one window | message references a known pane | Window list shrinks; group dissolves at 1 |
| RenameWindow (inbound) | Update a window label by id | message references a known window | Label reflects daemon name |
| SwitchWindow (inbound) | Sync active index to a daemon-initiated switch | message references a known pane | Active index matches daemon |

**Processing Flow**:
1. Outbound: action builds a `MuxMessage` -> encode APC -> write to PTY.
2. Inbound (existing decode route) branches by message type:
   - PaneCreated -> append window (consume pending-create count)
   - PtyExited -> remove matching window; re-clamp active index
   - RenameWindow -> relabel by window id
   - SwitchWindow -> set active index by pane id
   - Welcome -> seed list + active index
   - Snapshot / PtyOutput (existing) -> screen content

**Implementation Steps**:
1. **APC encoder** - encode contract; assert round-trip with decoder.
2. **send_control path** - wire encoder output to the active tab's PTY writer.
3. **Welcome ingest** - replace the current name-only extraction with full window-list seeding.
4. **Inbound window handlers** - PaneCreated / PtyExited / RenameWindow / SwitchWindow reconcile against the Phase 1 model.
5. **App routing** - surface inbound effects to the tab bar (re-render) and provide action access to `send_control`.

**Dependencies**: Requires Phase 1. Blocks Phase 3-5.

**Testing Approach**:
- Unit: encoder↔decoder round-trip per message type; each inbound handler's state transition; Welcome seeding; pending-create accounting.
- Integration: a scripted inbound APC sequence (attach → create → switch → rename → exit) drives the list to the expected state.

**Acceptance Criteria**:
- [ ] Outbound APC round-trips with the decoder.
- [ ] Each inbound message updates window state correctly.

**Estimated Effort**: large

---

### Phase 3: Prefix latch wiring + switch / create actions

**Goal**: prefix chords drive mux actions; window switch and new-window work end to end.

**Files to Modify**:
- `native-poc/src/mux/prefix.rs` - add `NewWindow`, `RenameWindow`, `MoveWindow` to the action enum; map follow-up keys from `mux.keybinds` (defaults `d`/`c`/`n`/`p`/`,`/`m`).
- `native-poc/src/app.rs` - mux-action dispatch: switch (next/prev/select), new-window; integrate latch into the key path.
- `native-poc/src/window_host.rs` - feed key events into the latch before normal keybinds/PTY passthrough when a mux tab is active.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Latch (extended) | Arm on prefix; map follow-up key to action; literal on double-prefix; timeout cancel | mux tab active | Emits an action or cancels |
| Keybind mapping | Resolve action bindings from `mux.keybinds`, warn+default on invalid | settings loaded | Latch uses effective bindings |
| Switch action | Update active index (wrap for n/p, clamp for digit), send SwitchWindow, apply snapshot | ≥2 windows | Active window changed; screen swapped |
| New-window action | Increment pending-create count, send CreateWindow | mux tab active | A create is pending; PaneCreated (Phase 2) appends it |

**Processing Flow**:
1. Key event on a mux tab -> latch.observe:
   - armed + follow-up `n`/`p`/digit -> switch action
   - armed + `c` -> new-window action
   - armed + `,`/`m` -> rename/move action (Phase 4)
   - armed + prefix again -> literal byte to PTY
   - armed + unknown key -> consume (no PTY write), cancel latch — matches WebView `prefix-key.ts` ("unknown key after prefix -- ignore"); do NOT fall through to PTY passthrough
   - armed + timeout -> cancel; the next key follows the normal path
2. Switch action updates the local active index, emits SwitchWindow, and the daemon snapshot swaps the screen.

**Implementation Steps**:
1. **Extend action enum** - add new-window / rename / move variants.
2. **Bindings from settings** - build the action table from `mux.keybinds`.
3. **Latch wiring** - intercept keys for active mux tabs ahead of PTY passthrough.
4. **Switch dispatch** - next/prev/select index math + SwitchWindow send.
5. **New-window dispatch** - pending-create + CreateWindow send.

**Dependencies**: Requires Phase 1-2. Blocks Phase 4 (shares dispatch).

**Testing Approach**:
- Unit: latch maps each chord (including custom bindings, double-prefix, unknown→cancel, timeout); switch index math (wrap/clamp); single-window switch is no-op.

**Acceptance Criteria**:
- [ ] prefix `n`/`p`/`0..9` switch windows; `c` creates one.
- [ ] Custom `mux.keybinds` override defaults; invalid chord warns + keeps default.

**Estimated Effort**: medium

---

### Phase 4: Rename / move dialogs

**Goal**: prefix `,` and `m` open egui dialogs that rename / reorder windows with optimistic local updates.

**Files to Create**:
- `native-poc/src/ui/rename_window_dialog.rs` - egui rename dialog (seeded current name).
- `native-poc/src/ui/move_window_dialog.rs` - egui move dialog (current/target position).

**Files to Modify**:
- `native-poc/src/app.rs` - rename/move action handlers: open dialog, resolve target by stable window id after the dialog, optimistic update, send control, rollback on move failure.
- `native-poc/src/render/mod.rs` - draw the active dialog overlay (same `&mut App` separation as existing overlays).
- `native-poc/src/window_host.rs` - route input to the dialog while open (swallow PTY).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Rename dialog | Capture a new name; cancel/empty = no-op | a window is active | Returns confirmed name or cancel |
| Move dialog | Capture a target position in `[1, N]` | ≥2 windows | Returns confirmed target or cancel |
| Rename handler | Re-resolve target by stable id; optimistic relabel; send RenameWindow | dialog confirmed | Label updated locally + daemon notified |
| Move handler | Optimistic reorder; send MoveWindow; rollback on failure | dialog confirmed | Order updated locally; reverted if send fails |
| Reentry guard | Prevent concurrent rename/move dialogs | — | Only one dialog of each kind at a time |

**Processing Flow**:
1. Action opens the dialog and captures the active window's stable id.
2. On confirm, re-resolve the window by id (abort if it closed during the dialog).
3. Rename: relabel locally, send RenameWindow with the active pane id.
4. Move: validate target (range, not same), reorder locally, send MoveWindow; on send failure, revert the reorder (daemon does not broadcast order).

**Implementation Steps**:
1. **Rename dialog UI** - text field, confirm/cancel, seeded name.
2. **Move dialog UI** - position input, range validation display.
3. **Rename handler** - stable-id re-resolve, optimistic label, send.
4. **Move handler** - optimistic reorder + rollback-on-failure.
5. **Input/overlay routing** - draw + capture keys while a dialog is open.

**Dependencies**: Requires Phase 1-3.

**Testing Approach**:
- Unit: move validation (range, same-position no-op); rollback on simulated send failure; stable-id re-resolution when the list changed.
- Manual: dialog visuals and focus (host-deferred).

**Acceptance Criteria**:
- [ ] `,` renames (local + daemon); empty/cancel no-op.
- [ ] `m` reorders optimistically; rolls back on send failure.

**Estimated Effort**: medium

---

### Phase 5: Tab group rendering + dynamic settings apply

**Goal**: the mux tab group is visible (compact/expanded sub-tabs) and all mux UI settings apply on save.

**Files to Modify**:
- `native-poc/src/ui/tab_bar.rs` - render the group: compact `mux (N)` vs expanded sub-tabs; active highlight; click → toggle (group) / switch (sub-tab).
- `native-poc/src/app.rs` - `apply_settings`: rebuild the mux keybind table; apply `tab_always_expand`, `status_position`, `mux.statusbar.*`.
- `native-poc/src/ui/status_bar.rs` - honor `mux.status_position` and `mux.statusbar.*` for the mux status row.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Group renderer | Draw compact/expanded states + sub-tabs with active highlight | tab is mux-attached | Tab bar reflects window list + expansion |
| Sub-tab interaction | Click switch / group toggle hit-testing | group rendered | Click routes to switch or toggle |
| apply_settings (mux) | Re-apply keybinds + expand default + statusbar/position on save | settings changed | UI reflects new mux settings without restart |

**Processing Flow**:
1. Tab bar, for a mux tab: compact → one group cell (`mux (N)`); expanded → group cell + sub-tab per window.
2. Click: group cell → toggle; sub-tab → switch (Phase 3 path).
3. On settings save, `apply_settings` rebuilds the mux keybind table and re-applies expand/statusbar/position.

**Implementation Steps**:
1. **Compact rendering** - group cell with `mux (N)` + toggle hit area.
2. **Expanded rendering** - sub-tab row with active highlight.
3. **Click routing** - toggle vs switch.
4. **Dynamic apply** - extend `apply_settings` for mux keybinds + expand + statusbar/position.

**Dependencies**: Requires Phase 1-4.

**Testing Approach**:
- Unit: render-model construction (compact label, sub-tab count, active marker); click hit-test mapping.
- Manual: visual parity with WebView tab group (host-deferred).

**Acceptance Criteria**:
- [ ] Compact shows `mux (N)`; toggle expands to sub-tabs; active highlighted.
- [ ] `tab_always_expand` initial state honored; settings apply on save.

**Estimated Effort**: medium

---

## Complete File Structure

```
native-poc/src/
├── mux/
│   ├── apc.rs                    # + outbound APC encoder
│   ├── prefix.rs                 # + NewWindow/Rename/Move actions, settings bindings
│   └── window_group.rs           # NEW: window state model + compact/expanded controller
├── ui/
│   ├── tab_bar.rs                # + tab group / sub-tab rendering
│   ├── status_bar.rs             # + mux.status_position / mux.statusbar.*
│   ├── rename_window_dialog.rs   # NEW: rename dialog
│   └── move_window_dialog.rs     # NEW: move dialog
├── tabs.rs                       # mux window state + apply_mux_message extension + send_control
├── app.rs                        # mux-action dispatch + apply_settings (mux)
├── window_host.rs                # latch + dialog input routing
├── render/mod.rs                 # dialog overlay draw
└── settings.rs                   # mux.* loader
```

## Testing Strategy
- Unit: window-state model, APC encode round-trip, each inbound handler, prefix latch mapping, move validation/rollback, render-model construction. Target core logic 80%+.
- Integration: scripted inbound APC sequence reconciling window state.
- E2E: not in scope (unit-centric per requirements).
- Manual (host-deferred): tab-group visuals, dialog focus, mux daemon round-trip.

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| mux_ipc | workspace (existing) | protocol types (unchanged) |
| egui | workspace (existing) | tab group + dialogs |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| daemon does not broadcast move order | High (known) | Medium | local-authoritative optimistic reorder + rollback on send failure (WebView parity) |
| inbound message ordering (PaneCreated vs Snapshot) | Medium | Medium | pending-create accounting; reconcile by id, not position |
| latch intercept conflicts with normal keybinds | Medium | Medium | only intercept for active mux tabs; double-prefix literal passthrough; unknown-after-prefix consumed (F2) |
| `mux.keybinds` invalid chords | Low | Low | warn + default fallback |
| click routing ambiguity in expanded group (native tab vs sub-tab vs group toggle) | Medium | Medium | distinct hit-test regions in `tab_bar.rs`; group cell = toggle, sub-tab = switch, other tabs = normal (F4) |

## Open Questions
- [ ] None blocking. (OQ1 resolved: initial window name "Terminal"; OQ2 resolved: no explicit kill-window in WebView — removal on shell exit only.)

## Success Metrics
- [ ] All FRs implemented and unit-tested; `cargo test` (native-poc) green.
- [ ] `src-tauri` build/test unaffected; APC inband preserved (no socket).
- [ ] Behavior parity with WebView mux tab group (manual host gate).
