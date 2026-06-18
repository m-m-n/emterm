# Implementation Plan: mux Scroll Isolation

## Overview

Isolate scroll position, displayed content, and scrollback history per native tab and per mux pane in mux mode, so switching between tabs/panes restores each unit's own scroll position and scrollback while leaving no residual rows from the previous unit. Native build only (`src-tauri/src/`); the WebView build (`src/`) is untouched on this branch.

## Objectives

- Store scroll position per native tab and per mux pane instead of a single App-global value, saving the outgoing unit's position and restoring the incoming unit's on every switch (FR3, symptom ③).
- Include the target pane's scrollback in the on-demand pane snapshot (mirroring the reattach path) so past output is scrollable immediately after a pane switch, with no detach/re-attach (FR1, symptom ①).
- Force a full re-render of the terminal area on tab/pane switch so a shorter incoming unit shows no residual rows from a longer outgoing unit (FR2, symptom ②).
- No regression for non-mux tabs, single-window mux, or the existing scroll-pin behavior (NFR1).

## Prerequisites

### Development Environment

- Rust toolchain as configured for the `emterm` crate (workspace at `src-tauri/Cargo.toml`).
- Commands run from the project root with an explicit `CARGO_TARGET_DIR` and `--manifest-path src-tauri/Cargo.toml` (no `cd`):
  - Unit/integration tests: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
  - CLI-only feature check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - Release build (run only on explicit user request): `CARGO_TARGET_DIR=src-tauri/target-host cargo build --release --manifest-path src-tauri/Cargo.toml`

### Dependencies

- No new external dependencies.
- Internal components that must already exist (all present on `refactor/promote-native-poc`):
  - App scroll state and renderer (`app.rs`, `window_host.rs`, `render/mod.rs`).
  - Tab / mux window-group state (`tabs.rs`, `mux/window_group.rs`).
  - mux IPC snapshot/reattach builders (`mux/ipc/handlers.rs`, `mux/ipc/reattach.rs`).
  - `TerminalCore` reset/replay (`crates/term_core`).
  - Existing scroll-pin logic (`doc/tasks/pin-viewport-when-scrolled-up`).

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: native terminal stack — winit (event loop), wgpu (GPU surface), egui (in-process UI); mux multiplexing in-process with an IPC daemon
- **Key Libraries**:
  - `term_core` — ANSI parser + grid + scrollback (`reset_and_replay`, scrollback length)
  - `mux_ipc` — mux protocol message types (snapshot/reattach payloads)

### Design Approach

The scroll position is currently a single `App` field (`scroll_position`) read by the renderer regardless of which tab/pane is active. The fix relocates the *persisted* scroll position to per-unit state while keeping the existing scroll mutators and renderer reading a single "current active unit" value. On a switch, the active value is saved into the outgoing unit's slot and reloaded from the incoming unit's slot. This keeps the hot scroll path (wheel / PageUp) a single in-place numeric read/write (NFR2) and avoids touching the scroll-pin logic (NFR1).

Two units of isolation exist and are handled symmetrically:

- **Native tab**: each tab owns its own `TerminalCore`. Saving/restoring is a pure scroll-position swap at `switch_to_tab`.
- **mux pane**: panes inside one tab share that tab's single `TerminalCore`; their screen is swapped via snapshot replay on switch. Saving/restoring the per-pane scroll position is threaded alongside the existing `request_pane_snapshot` reconcile, so the restore lands together with the replayed content.

For FR1, the on-demand snapshot builder is brought into symmetry with the reattach builder so both emit `ESC[H ESC[2J` + scrollback + shadow screen; the client's existing `reset_and_replay` then rebuilds the pane's history with no client-side change. For FR2, the switch paths set the renderer's existing full-redraw flag so non-emitted rows are cleared.

### Component Interaction

```
Native tab switch (switch_to_tab):
  save  outgoing tab's scroll slot  <- current active scroll position
  set   active tab index
  load  current active scroll position <- incoming tab's scroll slot
  flag  full redraw (clears stale rows)        [FR2]

mux pane switch (Self::switch_to / inbound SwitchWindow):
  save  outgoing pane's scroll slot <- current active scroll position
  request_pane_snapshot(target pane)
    daemon: handle_request_pane_snapshot
      build snapshot = ESC[H ESC[2J + pane scrollback + shadow screen   [FR1]
    client: apply_mux_message(Snapshot) -> reset_frame_for_replay -> reset_and_replay
            (rebuilds scrollback + screen)
  load  current active scroll position <- incoming pane's scroll slot   [FR3]
  flag  full redraw (clears stale rows)        [FR2]
```

The daemon already holds each pane's scrollback (the same buffer the reattach path reads), so FR1 adds no new daemon state — only the on-demand response carries bytes it previously omitted.

## Implementation Phases

### Phase 1: Per-unit scroll position state (FR3)

**Goal**: Scroll position is persisted per native tab and per mux pane; native tab switch saves the outgoing tab's position and restores the incoming tab's, with `Live` restoring to the bottom and `OffsetFromLive(n)` restoring to offset `n`. Non-mux tab behavior verified unchanged.

**Files to Modify**:
- `src-tauri/src/tabs.rs` — add a per-tab scroll-position field on `Tab`, and a per-pane scroll-position field on the mux window entry; expose group-level save/restore accessors keyed by active index.
- `src-tauri/src/mux/window_group.rs` — extend the mux window entry to carry its own scroll position; add mutating accessors to read/write the active pane's stored position (parallel-array invariant preserved).
- `src-tauri/src/app.rs` — keep `scroll_position` as the *active unit's* live value; at `switch_to_tab`, save the active value into the outgoing tab's slot and load the incoming tab's slot. Construction/default remains `Live`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Per-tab scroll slot | Hold a tab's last scroll position while it is inactive | Tab exists | Slot equals the tab's scroll position at the moment it was deactivated |
| Per-pane scroll slot | Hold a pane's last scroll position while another pane is active in the same tab | Pane exists in the group | Slot equals the pane's scroll position at the moment it was deactivated |
| Active scroll value (`App.scroll_position`) | Single source the renderer and mutators use for the currently-shown unit | — | Always reflects the active unit's position |
| `switch_to_tab` save/restore | Persist outgoing, restore incoming on native tab change | `idx` is a valid, different tab | Active value equals incoming tab's slot; outgoing tab's slot equals its last shown position |

**Processing Flow** (diagram-convertible):
1. Native tab switch requested for index `idx`.
   - `idx` out of range or equal to active -> no-op (unchanged).
   - otherwise:
     1. Save active scroll value into the currently-active tab's slot.
     2. Set active tab index to `idx`.
     3. Load active scroll value from the now-active tab's slot.
     4. Continue with existing search/selection reset and full-redraw flag.

**Implementation Steps** (5-7 max):
1. **Add per-tab scroll slot** — give `Tab` a scroll-position field defaulting to `Live`, initialized at tab construction.
2. **Add per-pane scroll slot** — give the mux window entry a scroll-position field defaulting to `Live`, set when a window/pane is seeded or appended.
3. **Group save/restore accessors** — add methods on the window group to read and write the active pane's stored scroll position, without exposing the parallel arrays directly.
4. **Wire native tab switch** — in `switch_to_tab`, save the outgoing tab's position and restore the incoming tab's before the existing reset logic.
5. **Tests** — unit tests for tab save/restore round-trip and `Live` / `OffsetFromLive(n)` restore semantics.

**Dependencies**: Requires nothing. Blocks Phase 3 (pane switch save/restore reuses these accessors).

**Testing Approach**:
- Unit: native tab switch saves outgoing and restores incoming position; `Live` restores to bottom; `OffsetFromLive(n)` restores to offset `n`; default position is `Live`.
- Integration: covered with Phase 3's pane round-trip.
- E2E: none for the native build.
- Manual: scroll up in tab A, switch to tab B, return to A — A shows its prior position, B is unaffected.

**Acceptance Criteria**:
- [ ] Each native tab retains its own scroll position across switches.
- [ ] Each mux pane has a stored scroll-position slot ready for Phase 3 wiring.
- [ ] `App.scroll_position` remains the single value read by the renderer and scroll mutators (scroll-pin path untouched).
- [ ] Non-mux tab switching has no behavioral change beyond the new per-tab restore.

**Estimated Effort**: medium

---

### Phase 2: Scrollback in the on-demand pane snapshot (FR1)

**Goal**: The on-demand `RequestPaneSnapshot` response includes the target pane's scrollback in the same construction as the reattach path (`ESC[H ESC[2J` + scrollback + shadow screen), so after a pane switch the client's replayed core holds the pane's history and the user can scroll to past output immediately, with no detach/re-attach. No client change required.

**Files to Modify**:
- `src-tauri/src/mux/ipc/reattach.rs` — extend the on-demand snapshot builder so it can include the pane's scrollback bytes, reusing the same ordering as the reattach builder (which already prepends scrollback). Keep a single source of truth for the byte layout shared by both paths.
- `src-tauri/src/mux/ipc/handlers.rs` — in the on-demand snapshot handler, resolve the pane's scrollback (the daemon already holds it via the same buffer the reattach path reads) and pass it to the builder so the response carries history.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| On-demand snapshot builder | Produce bytes that reproduce the pane's screen *and* scrollback | Shadow parser and scrollback are accessible for the pane | Output equals reattach construction: clear + scrollback + shadow |
| On-demand snapshot handler | Resolve the pane, read its scrollback without clearing it, build and enqueue the snapshot | Pane id resolves to a live pane | Client receives a snapshot whose replay rebuilds the pane's history |

**Processing Flow** (diagram-convertible):
1. Client requests a pane snapshot for a pane id.
2. Daemon resolves the pane.
   - pane not found -> log and ignore (unchanged).
   - pane found:
     1. Read the pane's scrollback bytes (no clear; buffer lives for the pane's lifetime).
     2. Read the shadow parser's screen contents.
     3. Build snapshot = clear-and-home + scrollback + shadow screen.
     4. Enqueue the snapshot as a PTY-output chunk for the pane (unchanged transport).
3. Client applies the snapshot via the existing reset-and-replay, rebuilding scrollback and screen.

**Implementation Steps** (5-7 max):
1. **Shared byte layout** — factor the snapshot byte ordering so the on-demand and reattach builders agree on "clear + scrollback + shadow"; the reattach path already produces this layout, so the on-demand path adopts it.
2. **Read pane scrollback in the handler** — obtain the pane's scrollback bytes alongside the shadow parser already resolved in the handler, reading without clearing.
3. **Build with scrollback** — assemble the on-demand snapshot using the shared layout including the scrollback bytes.
4. **Logging** — keep the existing snapshot-size log line so release builds (warn+) still capture the now-larger payload size for diagnostics.
5. **Tests** — assert the on-demand builder emits scrollback before the shadow screen (matches reattach construction) and that an empty scrollback yields a valid clear+shadow snapshot.

**Dependencies**: Requires nothing. Blocks Phase 3 (pane switch relies on the history-bearing snapshot to demonstrate scrollable history after switch).

**Testing Approach**:
- Unit: snapshot builder emits scrollback ahead of the shadow screen; empty scrollback produces a clear+shadow snapshot with no panic.
- Integration: with Phase 3, a pane round-trip leaves the returned pane's scrollback scrollable.
- E2E: none for the native build.
- Manual: flush a large output in pane A, switch away and back, confirm wheel / Shift+PageUp reaches A's past output without detach/re-attach.

**Acceptance Criteria**:
- [ ] On-demand snapshot includes the pane's scrollback in reattach ordering.
- [ ] No client-side change is required for history to reappear after a switch.
- [ ] An empty/missing scrollback replays as empty history without error.
- [ ] Snapshot-size log line remains for release-build diagnostics.

**Estimated Effort**: small

---

### Phase 3: Pane-switch save/restore + full re-render on switch (FR2, FR3 pane wiring)

**Goal**: On a mux pane switch (local prefix switch and daemon-initiated `SwitchWindow`), the outgoing pane's scroll position is saved and the incoming pane's is restored alongside the snapshot replay, and the terminal area is fully re-rendered on both tab and pane switch so a shorter incoming unit leaves no residual rows. Existing scroll-pin behavior and single-window mux are unaffected.

**Files to Modify**:
- `src-tauri/src/app.rs` — at the local pane-switch path (`Self::switch_to`), save the active scroll value into the outgoing pane's slot before committing the new active index, and restore the active value from the incoming pane's slot after the snapshot request; ensure the full-redraw flag is set on this path.
- `src-tauri/src/tabs.rs` — at the inbound `SwitchWindow` reconcile (which already calls `request_pane_snapshot`), apply the same outgoing-save / incoming-restore of per-pane scroll position and signal a full redraw.
- `src-tauri/src/window_host.rs` — ensure the renderer's full-redraw signal causes the terminal area to clear non-emitted rows on switch (the renderer reads `App.scroll_offset()` for the active unit; confirm the switch-triggered redraw clears stale cells).
- `src-tauri/src/render/mod.rs` — confirm the row-mapping pass, when driven by a full redraw, emits cleared cells for rows the incoming (shorter) unit does not cover, so no previous-unit content remains.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Pane-switch save/restore | Persist outgoing pane position, restore incoming pane position around the snapshot request | Tab has a mux group with the target pane | Active scroll value equals incoming pane's slot; outgoing pane's slot equals its last shown position |
| Inbound switch reconcile | Apply the same save/restore on daemon-initiated `SwitchWindow` | Switch synced to a known pane | Per-pane scroll consistent with the local switch path |
| Full-redraw on switch | Clear the terminal area so non-emitted rows lose previous-unit content | A switch occurred | Rendered frame shows only the incoming unit's rows |

**Processing Flow** (diagram-convertible):
1. Pane switch requested (local prefix or inbound switch).
   - target pane missing / group empty -> no-op (unchanged).
   - otherwise:
     1. Save active scroll value into the outgoing (currently active) pane's slot.
     2. Commit new active pane index.
     3. Request the pane snapshot (history-bearing, from Phase 2).
     4. Restore active scroll value from the incoming pane's slot.
     5. Set the full-redraw flag.
2. On the next render, the full redraw clears rows the incoming unit does not emit, and the restored scroll position positions the viewport.

**Implementation Steps** (5-7 max):
1. **Local pane-switch save** — before committing the new active index in the local switch path, save the active scroll value into the outgoing pane's slot.
2. **Local pane-switch restore** — after the snapshot request, restore the active scroll value from the incoming pane's slot.
3. **Inbound switch parity** — apply the same save/restore at the inbound `SwitchWindow` reconcile so remote-initiated switches behave identically.
4. **Full redraw on switch** — set the renderer's full-redraw flag on the pane-switch paths (native tab switch already sets it in Phase 1) and verify the renderer clears non-emitted rows.
5. **Tests** — integration test for an A → B → A pane round-trip restoring A's scroll position and leaving A's history scrollable; edge tests for empty scrollback and all-`Live` switches; a long→short switch leaves no residual rows.

**Dependencies**: Requires Phase 1 (per-pane slot + accessors) and Phase 2 (history-bearing snapshot). Blocks nothing.

**Testing Approach**:
- Unit: pane switch saves outgoing and restores incoming position; all-`Live` panes introduce no scroll on switch.
- Integration: A → B → A pane round-trip restores A's scroll position and A's scrollback is scrollable; switching to a pane with empty scrollback does not crash and shows no residual rows.
- E2E: none for the native build.
- Manual: long unit → short unit switch leaves no residual rows at the bottom; a background pane whose scrollback grew while inactive shows content consistent with its saved scroll position on return.

**Acceptance Criteria**:
- [ ] Local and inbound pane switches save/restore per-pane scroll position.
- [ ] After a pane switch, the returned pane's history is scrollable immediately.
- [ ] Switching from a longer unit to a shorter unit leaves no residual rows.
- [ ] Single-window mux and non-mux tabs show no regression; scroll-pin preserved.
- [ ] `cargo test` (default features) and CLI-only `cargo check` pass.

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
├── app.rs                 # active scroll value stays single; save/restore at tab + pane switch; full-redraw on switch
├── tabs.rs                # per-tab + per-pane scroll slots; inbound SwitchWindow save/restore; snapshot apply (unchanged recipe)
├── window_host.rs         # renderer reads active-unit scroll offset; full redraw clears stale rows on switch
├── render/mod.rs          # row mapping clears non-emitted rows under full redraw
└── mux/
    ├── window_group.rs    # per-pane scroll slot on the window entry; active-pane scroll accessors
    └── ipc/
        ├── handlers.rs    # on-demand snapshot reads pane scrollback and builds history-bearing snapshot
        └── reattach.rs    # snapshot byte layout shared by on-demand + reattach paths
crates/term_core/src/
└── terminal_core.rs       # reset_and_replay / reset (unchanged behavior; exercised by FR1)
```

## Testing Strategy

- Unit: scroll save/restore semantics (tab + pane), snapshot builder ordering (scrollback before shadow), empty-scrollback safety. Target: cover the new save/restore and builder branches.
- Integration: pane round-trip (A → B → A) restoring scroll position and scrollback; long→short switch residual-row check; empty-scrollback switch.
- E2E: no E2E framework detected for the native build (`e2e_test_command` is empty in sdd.yaml). Switch-time visual behavior is verified manually.
- Manual: items requiring human visual judgment (residual rows, viewport position after switch, background-grown pane on return).
- Regression: non-mux tab and single-window mux scroll/render behavior, and existing scroll-pin behavior, verified by the existing scroll test suite plus manual spot-check.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none) | — | No new external dependencies; all changes use existing crates (`term_core`, `mux_ipc`). |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Per-pane scroll save/restore desyncs from the snapshot replay (restore lands before/after the wrong frame) | Medium | Medium | Save before committing the active index, restore after the snapshot request, on both local and inbound switch paths; integration round-trip test asserts the restored position |
| Larger on-demand snapshot transfer for big scrollback | Low | Low | Matches the reattach path (NFR2 accepts this); keep the snapshot-size log for diagnostics |
| Full redraw on switch misses some non-emitted rows (residual remains) | Medium | Medium | Drive the existing full-redraw flag that tab switch already uses; long→short integration + manual check |
| Regression in scroll-pin or non-mux scroll from relocating the persisted position | Medium | High | Keep `App.scroll_position` as the single active value the mutators/renderer use; only add save/restore at switch boundaries; run the existing scroll test suite |
| Restoring an offset beyond the available scrollback after switch | Low | Low | Existing render-time clamp against live scrollback length resolves to the nearest valid position |

## Open Questions

- [ ] None outstanding from SPEC.md (Open Questions: None; no `status: tbd` requirements in sdd.yaml).

## Success Metrics

- [ ] FR1, FR2, FR3 implemented and covered by automated tests where feasible and by documented manual scenarios otherwise.
- [ ] Pane/window switch restores per-unit scroll position and scrollback; no residual rows on switch.
- [ ] Non-mux tab and single-window mux scroll/render unaffected; scroll-pin preserved (NFR1).
- [ ] On-demand snapshot transfer comparable to the reattach path (NFR2).
- [ ] `cargo test` (default features) and CLI-only `cargo check` pass.
