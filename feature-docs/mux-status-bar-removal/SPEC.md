# Feature: mux-status-bar-removal

> Requirements source: `feature-docs/mux-status-bar-removal/REQUIREMENTS.md`.
> Every requirement, acceptance criterion and assumption below is rendered
> from that document; this file is its implementation-facing view.

## Overview

The mux status bar has become a duplicate of the mux sidebar — session name,
window, pane and agent status are all shown there — so this feature removes
the mux-sourced status bar entirely: the daemon-side `StatusBarEngine`, the
`StatusUpdate` / `RequestStatusUpdate` protocol messages, the GUI-side
`mux_status_state` and OSC-row mux branch, and the `mux.statusbar` settings
schema on all mirrors. Removing it also eliminates the design in which the
terminal grid loses rows only while a mux session is attached (observed rows
49⇄51), a contributing cause of the mux→tmux tab-switch XTWINOPS response
leak documented in `tmp/discussion-mux-tab-switch-leak.md`. The general
(non-mux) app status bar is out of scope and is preserved unchanged.

## Objectives

- Remove the mux status bar UI, which became a duplicate of the mux sidebar
  (session name / window / pane / agent status are all shown there),
  eliminating a redundant UI surface.
- Eliminate the "terminal grid loses rows only while a mux session is
  attached" design (observed rows 49⇄51), which is a contributing cause of
  the mux→tmux tab-switch XTWINOPS response leak documented in
  `tmp/discussion-mux-tab-switch-leak.md`.

## User Stories

### US1: Remove the redundant mux status bar surface

As an eMterm mux user, I want the mux status bar removed, so that session
name / window / pane / agent status are presented only once — in the mux
sidebar, which already shows them and is unchanged.

**Acceptance Criteria:**
- [ ] No mux status bar rendering code, state management, or settings items
      remain — repository-wide searches for `MuxStatusbarSettings`,
      `StatusUpdateMsg`, `mux_status_state`, and `StatusBarEngine` return no
      hits outside reserved-opcode comments and historical docs. (AC-1)
- [ ] Functions not covered by the sidebar are dispositioned: pane cwd
      tracking is retained (FR7), user-configurable mux statusbar templates /
      commands are intentionally retired with no replacement (assumption A2).
      (AC-2)

### US2: Terminal grid rows independent of mux attach state

As an eMterm mux user, I want the terminal grid to keep the same number of
rows whether or not a mux session is attached, so that the rows 49⇄51
mux-conditional difference — a contributing cause of the mux→tmux tab-switch
XTWINOPS response leak — is gone.

**Acceptance Criteria:**
- [ ] Terminal grid rows are identical between mux-attached and non-mux
      states (the rows 49⇄51 delta is gone); status-bar row count is provably
      independent of mux attach state in unit tests. (AC-3)
- [ ] A settings.json containing a populated `mux.statusbar` section loads
      without error. (AC-4)

## Technical Requirements

### Functional Requirements

- **FR1 — Remove GUI-side mux status bar state and rendering path:** Delete
  the mux-sourced status bar path in the GUI: the
  `Tab::mux_status_state: Option<StatusUpdateMsg>` field and its
  `MessageType::StatusUpdate` latch/clear (src-tauri/src/tabs.rs:264,
  ~1862-1869, ~2306); the projection of `mux_status` into the status bar view
  model (src-tauri/src/app.rs:2467-2470 and `App::status_bar_state()`); the
  `mux_status` parameter and mux branch of `build_view_model` /
  `build_osc_row` (src-tauri/src/status_bar/runtime.rs:173-213, 269-271); and
  the mux-specific rendering and tests in src-tauri/src/ui/status_bar.rs
  (TS-25/TS-26 and related). Any GUI sender of `RequestStatusUpdate` is also
  removed.
- **FR2 — Remove the daemon-side mux status bar engine:** Delete
  `StatusBarEngine` and its supporting machinery in
  src-tauri/src/mux/ipc/statusbar.rs (settings loading, template resolution,
  command execution/caching, periodic StatusUpdate generation,
  `SharedActivePaneId`, the statusbar-only `SharedPaneCwdMap` registry) and
  its wiring in src-tauri/src/mux/ipc/connection.rs (construction ~370-390,
  render ticks ~804-844, force-render arms ~1338-1359, the
  `MessageType::RequestStatusUpdate` handler ~1377-1379, and
  `register_session_pane_cwds` ~1491-1507). Update the doc comment in
  src-tauri/src/windows_exec.rs, which references "mux statusbar commands".
- **FR3 — Retire the StatusUpdate / RequestStatusUpdate protocol messages:**
  Remove `StatusUpdateMsg` and the `StatusUpdate` (0x16) /
  `RequestStatusUpdate` (0x17) message types from
  crates/mux_ipc/src/protocol.rs and their round-trip tests. The opcode values
  0x16 and 0x17 are reserved (never reused for a new message) so
  mixed-version GUI/daemon pairs cannot misinterpret frames.
- **FR4 — Remove the mux statusbar settings schema on all mirrors:** Delete
  `MuxStatusbarSettings` / `MuxStatusbarCommand` and the `mux.statusbar` field
  from crates/app_settings/src/settings.rs (~645, 707-730) and their tests
  (~808-852); the native mirror `MuxStatusbarSettings` / `RawMuxStatusbar` and
  loader tests in src-tauri/src/settings.rs (~216, 231, 251, 1370-1377,
  2288-2335); and the TypeScript `MuxStatusbarSettings` /
  `MuxStatusbarCommand` interfaces plus `statusbar` field on the mux settings
  interface in src-tauri/web-shared/settings/types.ts (~129-147) with the
  corresponding fixture data in
  src-tauri/web-shared/settings/sections/mux-section.test.ts (~52). Note:
  mux-section.ts itself renders no statusbar fields, so no settings-UI
  controls need removal — only types/fixtures.
- **FR5 — Terminal grid height identical with and without mux:** After
  removal, attaching/detaching a mux session must not change the status bar's
  `visible_row_count` and therefore not change the bottom inset
  (`panel_height_logical` → `WindowHost::refresh_status_bar_insets`,
  src-tauri/src/window_host.rs:1238-1243) or the terminal grid rows. The
  observed rows 49⇄51 mux-conditional difference is gone; grid rows depend
  only on window size and non-mux status bar state.
- **FR6 — Preserve the general (non-mux) status bar:** The app status bar
  remains fully functional: App Line 1/2 templates (`{time}` / `{cwd}` /
  custom commands), the OSC `777;statusbar` dispatcher route
  (src-tauri/src/status_bar/osc_dispatcher.rs, src-tauri/src/callbacks.rs),
  the top-level `statusbar_*` settings, and
  src-tauri/web-shared/settings/sections/status-bar-section.ts are untouched.
  Only the mux-sourced OSC-row branch is removed; the dispatcher-sourced OSC
  row keeps working. The ResizeSettler / status-bar-inset machinery in
  window_host.rs stays (it still serves the general status bar's dynamic row
  count).
- **FR7 — Preserve per-pane cwd tracking (relocate detect_osc7_cwd):**
  `detect_osc7_cwd` (src-tauri/src/mux/ipc/statusbar.rs:474) is consumed by
  the pane PTY reader (src-tauri/src/mux/ipc/pty_spawn.rs:1003) to maintain
  `Pane.cwd`, which is persisted across daemon hot-upgrade
  (src-tauri/src/mux/upgrade.rs:587) and pane restoration
  (src-tauri/src/mux/session/pane.rs) — it is NOT statusbar-only. Relocate
  this function (and its tests) out of statusbar.rs rather than deleting it;
  per-pane cwd tracking behavior is unchanged. Only the statusbar's own
  `pane_cwd_map` registry and `active_pane_id` tracker are deleted with the
  engine.
- **FR8 — Tolerate stale peers and stale settings:** (a) A GUI built after
  this change must gracefully ignore a `StatusUpdate` (0x16) frame pushed by
  an older, still-running daemon (long-lived daemons and the hot-upgrade path
  make mixed versions a real scenario), and a new daemon must gracefully
  ignore `RequestStatusUpdate` (0x17) from an older GUI — at most a warn log,
  never an error/disconnect. (b) Existing user settings.json files still
  containing a `mux.statusbar` object must continue to deserialize without
  error (obsolete keys ignored).

All functional requirements are `resolved`; none carries a `tbd` status.

### Non-Functional Requirements

- **NFR1 - CLI-only build remains green:**
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
  src-tauri/Cargo.toml --no-default-features` still compiles (mux and settings
  code are touched; feature gates must not break).
- **NFR2 - Full test suites remain green:** Rust `--lib` suites for src-tauri
  and affected workspace crates (app_settings, mux_ipc), the integration tests
  (including mux_hot_upgrade with `--test-threads=1`), `bun test`, and
  `bun run typecheck` all pass after removal.
- **NFR3 - No behavior change to tab bar / sidebar / agent status:**
  `mux_session_name`, `mux_group`, `AgentStatusUpdate` handling, the mux
  sidebar, and tab-bar mux window grouping are unrelated to the status bar and
  must be byte-for-byte unaffected.

All non-functional requirements are `resolved`; none carries a `tbd` status.

## Implementation Approach

### Architecture

The removal cuts one end-to-end path through the existing layers. The
mux-sourced path (marked ✗) goes away; the general status bar path (marked ✓)
stays.

```
                daemon                     mux IPC                 GUI
  ✗  StatusBarEngine ───────► StatusUpdate (0x16) ──► Tab::mux_status_state
  ✗       ▲  settings/templates/commands              ──► build_view_model(mux_status)
  ✗       └───────────────── RequestStatusUpdate (0x17) ◄── GUI sender
                                                        ──► ui/status_bar.rs mux render

  ✓  application ── OSC 777;statusbar ──► osc_dispatcher ──► OSC row (kept)
  ✓  settings statusbar_* ─────────────► App Line 1/2  (kept)
  ✓  shell ── OSC 7 ──► detect_osc7_cwd (relocated) ──► Pane.cwd (kept)
```

**Component disposition:**

| Component | Disposition | Requirement |
|---|---|---|
| GUI `Tab::mux_status_state`, `MessageType::StatusUpdate` latch/clear | removed | FR1 |
| GUI `mux_status` projection, `build_view_model` / `build_osc_row` mux branch, mux rendering + tests | removed | FR1 |
| GUI sender of `RequestStatusUpdate` | removed | FR1 |
| daemon `StatusBarEngine`, `SharedActivePaneId`, statusbar-only `SharedPaneCwdMap`, connection.rs wiring, `register_session_pane_cwds` | removed | FR2 |
| `windows_exec.rs` doc comment referencing "mux statusbar commands" | updated | FR2 |
| `StatusUpdateMsg`, opcodes 0x16 / 0x17 and round-trip tests | removed; opcodes reserved | FR3, A3 |
| `MuxStatusbarSettings` / `MuxStatusbarCommand` / `RawMuxStatusbar` / `mux.statusbar` on Rust, native and TypeScript mirrors + fixtures | removed | FR4 |
| `detect_osc7_cwd` and its tests | relocated out of statusbar.rs | FR7, A4 |
| ResizeSettler / `refresh_status_bar_insets` machinery | retained (mux-specific comments/tests updated) | FR6, A5 |
| General status bar: `statusbar_*` settings, App Line 1/2, OSC 777 dispatcher, status-bar-section.ts | untouched | FR6, A1 |
| Tab bar / sidebar / agent status: `mux_session_name`, `mux_group`, `AgentStatusUpdate`, mux window grouping | untouched | NFR3 |

### Data Flow

Grid sizing after removal (FR5):

```
window size ──┐
              ├──► status bar visible_row_count ──► panel_height_logical
non-mux SB ───┘        (no mux input)              ──► WindowHost::refresh_status_bar_insets
                                                   ──► terminal grid rows
mux attach state ──✗ (no longer an input)
```

Per-pane cwd after relocation (FR7):

```
shell ── OSC 7 ──► pane PTY reader (pty_spawn.rs)
                     └─► detect_osc7_cwd (relocated) ─► Pane.cwd
                                                        ├─► daemon hot-upgrade handoff (upgrade.rs)
                                                        └─► pane restoration (session/pane.rs)
```

### Protocol Compatibility

| Opcode | Message | After this change |
|---|---|---|
| 0x16 | `StatusUpdate` | Removed from the protocol; opcode reserved and never reused. A new GUI receiving it from an older daemon ignores it — at most a warn log, never an error/disconnect (FR8a). |
| 0x17 | `RequestStatusUpdate` | Removed from the protocol; opcode reserved and never reused. A new daemon receiving it from an older GUI ignores it — at most a warn log, never an error/disconnect (FR8a). |

Long-lived daemons and the hot-upgrade path make mixed GUI/daemon versions a
real scenario, which is why both opcodes stay reserved rather than being
recycled (FR3, assumption A3).

### Settings Compatibility

Existing `settings.json` files that still contain a `mux.statusbar` object
must continue to deserialize without error; the obsolete key is ignored
(FR8b, AC-4). `mux.statusbar` left / right / commands are retired with no
sidebar replacement — an intentional feature retirement, not a migration gap
(assumption A2).

### File Structure

Files touched by this feature:

```
crates/
├── mux_ipc/src/protocol.rs                              # FR3 remove StatusUpdateMsg + 0x16/0x17
└── app_settings/src/settings.rs                         # FR4 remove MuxStatusbarSettings/Command
src-tauri/
├── src/
│   ├── tabs.rs                                          # FR1 mux_status_state + StatusUpdate latch/clear
│   ├── app.rs                                           # FR1 mux_status projection, status_bar_state()
│   ├── status_bar/runtime.rs                            # FR1 mux_status param + mux branch
│   ├── status_bar/osc_dispatcher.rs                     # FR6 untouched (dispatcher OSC row kept)
│   ├── callbacks.rs                                     # FR6 untouched
│   ├── ui/status_bar.rs                                 # FR1 mux rendering + TS-25/TS-26 tests
│   ├── window_host.rs                                   # FR5/FR6 insets kept; mux comments/tests updated
│   ├── settings.rs                                      # FR4 native mirror + loader tests
│   ├── windows_exec.rs                                  # FR2 doc comment update
│   └── mux/
│       ├── ipc/statusbar.rs                             # FR2 engine removed; FR7 detect_osc7_cwd relocated
│       ├── ipc/connection.rs                            # FR2 wiring removed
│       ├── ipc/pty_spawn.rs                             # FR7 consumer of relocated detect_osc7_cwd
│       ├── upgrade.rs                                   # FR7 Pane.cwd hot-upgrade persistence
│       └── session/pane.rs                              # FR7 pane restoration
└── web-shared/settings/
    ├── types.ts                                         # FR4 TS interfaces + statusbar field
    ├── sections/mux-section.test.ts                     # FR4 fixture data
    └── sections/status-bar-section.ts                   # FR6 untouched
```

### Dependencies

**Internal dependencies:**

- mux sidebar: precondition — its implementation is already complete and is
  the surface that already shows session name / window / pane / agent status.
  It is unchanged by this feature.
- `crates/mux_ipc`, `crates/app_settings`: protocol and settings schema
  mirrors that must be edited together with the src-tauri mirrors (FR3, FR4).

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR5, FR6) — runtime.rs: `build_view_model` no longer
      takes/uses mux status; OSC row renders only from the OSC 777 dispatcher;
      row count is unchanged by mux attach.
- [ ] **TS2** (FR1, FR8) — tabs.rs / app.rs: receiving a raw frame with
      retired opcode 0x16 from a stale daemon is ignored with at most a warn
      log and does not disturb the tab (replaces the current
      `on_mux_message_status_update_caches_payload_on_tab` test).
- [ ] **TS3** (FR4, FR8) — app_settings + src-tauri/src/settings.rs: JSON with
      a `mux.statusbar` object deserializes; the obsolete key is ignored.
- [ ] **TS4** (FR5, FR6) — window_host.rs: status-bar inset / grid-size
      candidates are driven only by general status-bar visibility; no
      mux-conditional path remains.
- [ ] **TS5** (FR7) — mux/ipc: relocated `detect_osc7_cwd` tests still pass;
      pane cwd still updates from OSC 7 and survives hot-upgrade (the
      mux_hot_upgrade integration test remains green).

### Integration Tests

- [ ] **TS5** (FR7) — mux_hot_upgrade remains green (run with
      `--test-threads=1`), confirming `Pane.cwd` survives the daemon
      hot-upgrade handoff after `detect_osc7_cwd` is relocated.

### TypeScript Tests

- [ ] **TS6** (FR4) — `bun test` and `bun run typecheck` pass with
      `MuxStatusbarSettings` removed from types.ts and fixtures.

### Manual Verification

- [ ] **TS7** (FR5) — manual/user, not run in automated verify (record as a
      manual verification note): in a 3-tab mux/tmux/plain setup, switching
      mux→tmux no longer resizes the inactive tmux tab's PTY and no XTWINOPS
      response text leaks into the tmux screen
      (`tmp/discussion-mux-tab-switch-leak.md` scenario); full root-cause
      elimination of per-tab grid coupling stays in the separate follow-up
      task.

### Build Checks

- [ ] NFR1 — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
      src-tauri/Cargo.toml --no-default-features` compiles.
- [ ] NFR2 — Rust `--lib` suites for src-tauri, app_settings and mux_ipc, plus
      the integration tests, `bun test` and `bun run typecheck`, all pass.

## Assumptions

- **A1:** "mux status bar" means exactly the mux-sourced pieces — daemon
  StatusBarEngine, the StatusUpdate/RequestStatusUpdate protocol, the GUI
  mux_status_state / OSC-row mux branch, and the `mux.statusbar` settings on
  all mirrors. The general app status bar (top-level `statusbar_*` settings,
  App Line 1/2, OSC 777;statusbar dispatcher) is out of scope and preserved.
- **A2:** User-configurable mux statusbar templates/commands (mux.statusbar
  left/right/commands) are removed without a sidebar replacement — intentional
  feature retirement, not a migration gap.
- **A3:** Protocol opcodes 0x16/0x17 are reserved-not-reused, and both sides
  tolerate receiving the retired messages from an older peer (FR8).
- **A4:** `detect_osc7_cwd` and per-pane cwd tracking are kept (relocated),
  since Pane.cwd feeds hot-upgrade handoff and pane restoration beyond the
  status bar.
- **A5:** The ResizeSettler / refresh_status_bar_insets machinery in
  window_host.rs is retained for the general status bar; only mux-specific
  comments/tests there are updated.
- **A6:** tmp/discussion-mux-tab-switch-leak.md exists in the repository and
  is background context only; the grid-parity AC is the extent to which this
  task addresses the leak, with the per-tab grid size task explicitly out of
  scope.
- **A7:** design step is skipped — pure removal/cleanup of an existing UI
  surface; no new UI, no visual or layout design decisions. The replacement UI
  (mux sidebar) already exists and is unchanged.

## Constraints and Scope Exclusions

**Premises:**

- The sidebar implementation is already complete (precondition).
- The rows 49⇄51 grid delta is resolved by this task.
- The cols 171⇄207 grid delta (from sidebar persistent mode) is explicitly
  NOT resolved by this task.

**Out of scope:**

- 「タブごとに独立したグリッドサイズを保持する」 (per-tab independent grid
  size) is a separate follow-up task; per-tab grid coupling root-cause
  elimination is deferred there.

## Success Criteria

- [ ] **AC-1:** No mux status bar rendering code, state management, or
      settings items remain — repository-wide searches for
      `MuxStatusbarSettings`, `StatusUpdateMsg`, `mux_status_state`, and
      `StatusBarEngine` return no hits outside reserved-opcode comments and
      historical docs.
- [ ] **AC-2:** Functions not covered by the sidebar are dispositioned: pane
      cwd tracking is retained (FR7), user-configurable mux statusbar
      templates/commands are intentionally retired with no replacement
      (assumption A2).
- [ ] **AC-3:** Terminal grid rows are identical between mux-attached and
      non-mux states (the rows 49⇄51 delta is gone); status-bar row count is
      provably independent of mux attach state in unit tests.
- [ ] **AC-4:** A settings.json containing a populated `mux.statusbar` section
      loads without error.
- [ ] **AC-5:** All builds and test suites in NFR1/NFR2 are green.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — every functional (FR1-FR8) and non-functional (NFR1-NFR3) requirement
is `resolved`.

## References

- Requirements document: `feature-docs/mux-status-bar-removal/REQUIREMENTS.md`
- Background context (assumption A6): `tmp/discussion-mux-tab-switch-leak.md`
