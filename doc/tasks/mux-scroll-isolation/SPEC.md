# Feature: mux Scroll Isolation

## Overview

In mux mode, scroll position, displayed content, and scrollback history are currently not isolated per tab or per mux pane. Switching panes/windows loses scrollback history (until a detach→re-attach), leaves residual rows from the previously shown unit, and carries the scroll position across all units. This feature isolates scroll position per native tab and per mux pane, restores scrollback on pane switch, and clears stale rows on switch.

## Objectives

- Restore each pane's scrollback on a mux window/pane switch so past output is scrollable immediately (no detach→re-attach).
- Fully refresh the terminal area on tab/pane switch so a shorter incoming unit does not show the previous unit's rows.
- Hold scroll position per native tab and per mux pane instead of a single shared App-level value.
- No regression for non-mux tabs and single-window mux.

## User Stories

### US1: Scroll to past output right after switching
As a mux user, I want a pane to keep its scrollback after I switch away and back, so that I can scroll to past logs without detaching and re-attaching.

**Acceptance Criteria:**
- [ ] After output in pane A → switch to pane B → return to A, wheel / Shift+PageUp scrolls A's past output
- [ ] No detach→re-attach cycle is required to see the history

### US2: No residual content on switch
As a mux user, I want switching to a tab/pane to show only that unit's content, so that the previous (longer) unit's rows do not remain on screen.

**Acceptance Criteria:**
- [ ] Switching from a longer unit to a shorter unit leaves no residual rows at the bottom

### US3: Per-unit scroll position
As a mux user, I want each tab/pane to remember its own scroll position, so that switching does not move my place in another unit.

**Acceptance Criteria:**
- [ ] Scrolling up in unit A then switching to unit B shows B at its own position
- [ ] Returning to A restores A's previous scroll position

## Technical Requirements

### Functional Requirements

- **FR1:** On a mux window/pane switch, the on-demand pane snapshot (`RequestPaneSnapshot` response) includes the target pane's scrollback, mirroring the reattach path. The client replays it so the restored core holds the pane's history and the user can scroll to past output immediately after switching. (symptom ①)
- **FR2:** When the active native tab or mux pane changes, the terminal area is fully re-rendered so that rows not emitted by the incoming unit do not retain the previous unit's content. A shorter incoming unit shows no residual rows from a longer outgoing unit. (symptom ②)
- **FR3:** Scroll position is stored per native tab and per mux pane, not as a single shared `App` value. On switch, the outgoing unit's scroll position is saved and the incoming unit's saved scroll position is restored (`Live` → bottom, `OffsetFromLive(n)` → that offset). (symptom ③)

### Non-Functional Requirements

- **NFR1 - No regression:** Non-mux tabs and single-window mux retain their current scroll and render behavior. Existing scroll-pin behavior (`pin-viewport-when-scrolled-up`) is preserved.
- **NFR2 - Performance:** Saving/restoring scroll position is a single numeric value on the switch path (negligible). Including scrollback in the on-demand snapshot raises its transferred size to match the reattach path; this is acceptable.
- **NFR3 - Usability:** A pane returned to after being in the background shows content consistent with its saved scroll position (a bottom-pinned pane follows new output; a scrolled-up pane keeps its place).

## Implementation Approach

### Architecture

**Current behavior (root causes):**

- **Scroll offset is App-global.** `App::scroll_position: ScrollPosition` (`app.rs:227`) is a single field. `scroll_up_by` / `scroll_down_by` / `scroll_set_offset` (`app.rs:2953`, `2970`, `2989`) read/write it directly, and the renderer reads `app.scroll_offset()` (`window_host.rs:1569`) regardless of the active tab. `switch_to_tab` (`app.rs:1383-1406`) does not touch it. → symptom ③ (shared across all tabs and panes).
- **Each native tab owns its own `TerminalCore`** (`Tab::core: Arc<Mutex<TerminalCore>>`, `tabs.rs:44`). In mux, one tab carries a `mux_group: MuxWindowGroup` (`tabs.rs`) whose panes (`MuxWindow`) share the single tab core; pane switch swaps core contents via snapshot replay.
- **On-demand snapshot is screen-only.** `handle_request_pane_snapshot` → `build_shadow_parser_snapshot` (`mux/ipc/handlers.rs:415-460`, `mux/ipc/reattach.rs:23-37`) emits `ESC[H ESC[2J` + visible screen only, no scrollback. The client applies it via `reset_frame_for_replay` → `reset_and_replay`, and `reset()` clears scrollback (`crates/term_core/src/terminal_core.rs:433-451`). The reattach path (`collect_reattach_data`, `reattach.rs:126-176`) *does* send scrollback — the two paths are asymmetric. → symptom ① (history lost on switch).
- **Renderer does not clear non-emitted rows.** The grid pass draws the active tab's core; rows the incoming unit does not emit retain the previous frame's cells. → symptom ② (residual content).

**Fix direction:**

- **FR3:** Move scroll position out of `App` into per-tab state, and additionally persist it per mux pane. On native tab switch, save/restore `Tab` scroll position. On mux pane switch, save the outgoing pane's scroll position and restore the incoming pane's saved value alongside the snapshot replay.
- **FR1:** Make the on-demand snapshot include the pane's scrollback (same construction as the reattach path: `ESC[H ESC[2J` + scrollback + shadow), so `reset_and_replay` rebuilds the pane's history on the client.
- **FR2:** On switch, force a full redraw / clear so non-emitted rows do not show stale content from the previous unit.

### Key Locations

| Purpose | File | Reference |
|---------|------|-----------|
| Scroll position field (App-global, to relocate) | `src-tauri/src/app.rs` | `scroll_position`, `scroll_offset()` |
| Scroll mutators | `src-tauri/src/app.rs` | `scroll_up_by` / `scroll_down_by` / `scroll_set_offset` (and jump variants) |
| Native tab switch | `src-tauri/src/app.rs` | `switch_to_tab` (`1383-1406`) |
| Tab struct / core ownership | `src-tauri/src/tabs.rs` | `Tab` (`37-169`), `core` (`44`), `mux_group` |
| mux pane group | `src-tauri/src/mux/.../window_group.rs` | `MuxWindowGroup` (`windows`, `pane_ids`, active index) |
| mux pane switch (client) | `src-tauri/src/tabs.rs` | `SwitchWindow` handling (`737-757`), `request_pane_snapshot` (`754`) |
| Snapshot apply (client) | `src-tauri/src/tabs.rs` | `Snapshot`/`SnapshotRestore` → `reset_frame_for_replay` (`458-473`) |
| On-demand snapshot (daemon) | `src-tauri/src/mux/ipc/handlers.rs` | `handle_request_pane_snapshot` (`415-460`) |
| Snapshot builder (screen-only) | `src-tauri/src/mux/ipc/reattach.rs` | `build_shadow_parser_snapshot` (`23-37`) |
| Reattach builder (scrollback, symmetry baseline) | `src-tauri/src/mux/ipc/reattach.rs` | `collect_reattach_data` (`126-176`) |
| Renderer scroll offset read | `src-tauri/src/window_host.rs` | `app.scroll_offset()` (`1569`) |
| Cell collection / row mapping | `src-tauri/src/render/mod.rs` | `collect_cell_inputs` (`604-735`) |
| Core reset / replay | `crates/term_core/src/terminal_core.rs` | `reset_and_replay` / `reset()` (`433-451`) |

### Data Flow

```
Native tab switch (switch_to_tab):
  save  outgoing Tab.scroll_position = current scroll position
  set   active tab
  load  current scroll position = incoming Tab.scroll_position
  full redraw (clear stale rows)

mux pane switch (SwitchWindow):
  save  outgoing pane.scroll_position
  request_pane_snapshot(target pane)
  daemon → Snapshot { ESC[H ESC[2J + scrollback + shadow }   (FR1)
  client reset_frame_for_replay → reset_and_replay (rebuilds scrollback)
  load  current scroll position = incoming pane.scroll_position
  full redraw (clear stale rows)
```

### Dependencies

**Internal Dependencies:**
- `App` scroll state and renderer (`app.rs`, `window_host.rs`, `render/mod.rs`)
- `Tab` / `MuxWindowGroup` / `MuxWindow` state (`tabs.rs`, mux window group)
- mux IPC snapshot/reattach (`mux/ipc/handlers.rs`, `mux/ipc/reattach.rs`)
- `TerminalCore` reset/replay (`crates/term_core`)
- existing scroll-pin logic (`pin-viewport-when-scrolled-up`)

**External Dependencies:**
- None

### File Structure

```
src-tauri/src/
├── app.rs              # relocate scroll_position to per-tab; save/restore on switch
├── tabs.rs            # Tab/pane scroll state; pane switch save/restore; snapshot apply
├── window_host.rs     # renderer scroll-offset source; full redraw on switch
├── render/mod.rs      # row mapping / stale-row clear
└── mux/ipc/
    ├── handlers.rs    # include scrollback in on-demand snapshot
    └── reattach.rs    # snapshot builder shared with reattach path
crates/term_core/src/
└── terminal_core.rs   # reset/replay (unchanged behavior, exercised by FR1)
```

## Test Scenarios

### Unit Tests
- [ ] Switching native tabs saves the outgoing tab's scroll position and restores the incoming tab's
- [ ] A unit saved at bottom (`Live`) restores at bottom
- [ ] A unit saved at `OffsetFromLive(n)` restores at offset n
- [ ] The on-demand snapshot builder emits scrollback before the shadow screen (matches reattach construction)

### Integration Tests
- [ ] mux pane switch round-trip (A → B → A) restores A's scroll position and A's scrollback is scrollable

### E2E Tests
**Existing E2E tests**: None detected for the native build
**Run command**: Not detected
- [ ] Manual: scroll up in pane A, switch to pane B (shows B's position), return to A (A's position and history restored)

### Edge Cases
- [ ] Switching to a pane with empty scrollback does not crash and shows no residual rows
- [ ] All tabs/panes at bottom (`Live`) → switching introduces no scroll
- [ ] Long unit → short unit switch leaves no residual rows at the bottom
- [ ] A background pane whose scrollback grew while inactive shows content consistent with its saved scroll position on return

### Performance Tests
- [ ] On-demand snapshot with scrollback transfers comparably to the reattach path (no unexpected blow-up)

## Security Considerations

- Not applicable (no new external input, auth, or data exposure surface).

## Error Handling

- A snapshot with empty or missing scrollback replays as an empty history (no error).
- Restoring a scroll offset beyond the available scrollback resolves to the nearest valid position at render time (the existing `scrollback_lines` clamp in the scroll mutators is a safety net; the actual bound is the live scrollback length used by `collect_cell_inputs`).

## Success Criteria

- [ ] FR1, FR2, FR3 implemented and tested
- [ ] Pane/window switch restores scrollback and per-unit scroll position
- [ ] No residual rows on switch
- [ ] Non-mux tab and single-window mux scroll/render unaffected (regression check)
- [ ] `cargo test` (default features) and CLI-only `cargo check` pass

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。

- None

## References

- Investigation report: `tmp/mux-scroll-investigation.md`
- 要件定義書: `doc/tasks/mux-scroll-isolation/要件定義書.md`
- Related: `doc/tasks/mux-per-pane-scroll-position/`, `doc/tasks/pin-viewport-when-scrolled-up/`, `doc/tasks/mux-scrollback-retention/`
