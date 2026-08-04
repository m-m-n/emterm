# Implementation Plan: mux-status-bar-removal

## Overview

Remove the mux-sourced status bar end-to-end — daemon `StatusBarEngine`, the
`StatusUpdate` (0x16) / `RequestStatusUpdate` (0x17) protocol messages, the
GUI `mux_status_state` / OSC-row mux branch, and the `mux.statusbar` settings
schema on all mirrors — while preserving the general app status bar, per-pane
cwd tracking (`detect_osc7_cwd` relocated, not deleted), and tolerance for
stale peers and stale settings files.

## Technology Stack

- **Rust** (existing): GUI terminal stack, mux daemon, `crates/mux_ipc`
  protocol crate, `crates/app_settings` settings crate.
- **TypeScript** (existing): `src-tauri/web-shared` settings type mirror.
- **New dependencies: none.** This is a pure removal; `project.license` (MIT)
  is unaffected and there are no dependency licenses to record.

## Layer Structure

| Layer | Disposition | Owning task |
|---|---|---|
| GUI state/render (tabs, app, status_bar/runtime, ui/status_bar, window_host) | mux-sourced path removed; general status bar and inset machinery preserved | task0001 |
| Daemon engine (mux/ipc/statusbar, connection, windows_exec comment) | `StatusBarEngine` and wiring removed; `detect_osc7_cwd` relocated | task0001 |
| IPC protocol (crates/mux_ipc) | `StatusUpdateMsg` + opcodes 0x16/0x17 removed; opcodes reserved-not-reused | task0001 |
| Settings schema, Rust mirrors (crates/app_settings, src-tauri/src/settings.rs) | `MuxStatusbarSettings` / `MuxStatusbarCommand` / `RawMuxStatusbar` and `mux.statusbar` removed; obsolete key tolerated on load | task0001 |
| Settings schema, TypeScript mirror (web-shared/settings) | interfaces + `statusbar` field + fixtures removed | task0002 |

Dependency direction is unchanged: GUI and daemon depend on `mux_ipc` and
`app_settings`; the TS mirror shadows the Rust settings shape with no build
coupling to Rust.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Settings mirror shape (mux section) | Keep the Rust and TypeScript settings schemas structurally identical | Postcondition on every mirror: the mux settings object has **no** statusbar member and no `MuxStatusbarSettings` / `MuxStatusbarCommand` type. Rust loaders additionally satisfy: given stored JSON that still contains a `mux.statusbar` object, loading succeeds and the obsolete key is ignored (the TS mirror receives already-normalized settings and needs no tolerance logic of its own). | task0001 (Rust mirrors), task0002 (TS mirror) |

## Conventions

- **Tolerance log floor**: every stale-peer / stale-settings tolerance path
  emits at most one warn-level log and never an error, disconnect, or state
  mutation. (Project rule: release builds persist only warn and higher, so
  warn is also the minimum level at which the event is observable in the
  field.)
- **Reserved-opcode comment**: the protocol module keeps a comment reserving
  opcode values 0x16 and 0x17 (never to be reassigned). That comment — and
  historical docs — are the only places the retired names
  (`StatusUpdate` / `RequestStatusUpdate` / `StatusUpdateMsg`) may appear
  (AC-1).
- **No named references to retired variants**: no code path (including
  tests) may name the retired protocol message types. Incoming message kinds
  that a receiver does not handle are covered by wildcard/default handling,
  never by a named match arm — this is what keeps each side compiling
  regardless of whether the peer-facing type still exists.

## Cross-task Design Decisions

### D1: The entire Rust-side removal is one task (compile atomicity)

Tasks run fully in parallel and each task's worktree must compile and pass
tests standalone. `StatusUpdateMsg` is referenced by GUI code (tabs → app →
status_bar/runtime) and daemon code (statusbar, connection);
`MuxStatusbarSettings` is referenced by the daemon engine and both Rust
settings mirrors. Deleting a definition in a worktree where any consumer
still exists breaks that worktree's build, and there is no task-ordering
mechanism to sequence "remove usages first, delete definitions second".
Therefore definition deletion and all usage removal must land in the same
task: task0001 carries the whole Rust side (FR1–FR8). The TypeScript mirror
has no Rust compile coupling and is split out as task0002. Affected tasks:
task0001, task0002.

### D2: Opcodes 0x16/0x17 are reserved-not-reused, with two-sided tolerance

Long-lived daemons and the hot-upgrade path make mixed GUI/daemon versions a
real scenario (SPEC, A3). Decision: the opcode values are never reassigned;
a receiver that gets a frame carrying a retired opcode discards it under the
tolerance log floor above (GUI receiving 0x16 from an older daemon; daemon
receiving 0x17 from an older GUI). Tolerance is verified with tests that
construct **raw frames** and assert the observable outcome (state
undisturbed, connection alive) without naming the retired types — such tests
stay valid before and after the type deletion. Affected tasks: task0001.

### D3: `detect_osc7_cwd` relocates into the pane PTY reader module

`detect_osc7_cwd` is not statusbar-only: it feeds `Pane.cwd`, which persists
across daemon hot-upgrade and pane restoration (FR7). Decision: relocate the
function and its unit tests into `src-tauri/src/mux/ipc/pty_spawn.rs` — its
sole call site — with behavior and tests unchanged. This avoids creating a
new module (no module-roster registration risk) and lets `statusbar.rs` be
deleted outright, together with its module declaration in the mux/ipc module
root. Only the statusbar's own `pane_cwd_map` registry and `active_pane_id`
tracker are deleted with the engine; `Pane.cwd` maintenance in the PTY
reader stays. Affected tasks: task0001.

### D4: Preservation boundary

The following are out of bounds for every task and must be byte-for-byte
unaffected (FR6, NFR3): the general app status bar (top-level `statusbar_*`
settings, App Line 1/2, the OSC `777;statusbar` dispatcher route in
`status_bar/osc_dispatcher.rs` / `callbacks.rs`,
`web-shared/settings/sections/status-bar-section.ts`); the ResizeSettler /
`refresh_status_bar_insets` machinery in `window_host.rs` (only mux-specific
comments/tests there are updated); and the tab bar / mux sidebar / agent
status surfaces (`mux_session_name`, `mux_group`, `AgentStatusUpdate`, mux
window grouping). Affected tasks: task0001, task0002.

### D5: Batch-mode decision record (no user gate needed)

- TBD requirements: none — all FR/NFR entries are `status: ok` in
  workflow.yaml; no resolution needed.
- License: no new dependencies, so no compatibility check beyond recording
  that fact (Technology Stack above); `project.license: MIT` unchanged.
- Existing files: first planning pass — no pre-existing IMPLEMENTATION.md /
  VERIFICATION.md / tasks/ to merge or overwrite.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Older daemon keeps pushing 0x16 after GUI upgrade (or older GUI sends 0x17) and the new peer errors/disconnects | Medium | High | D2 tolerance contract; raw-frame tolerance tests on both receive paths (TS2 + daemon-side test) |
| A `StatusBarEngine`-adjacent consumer is missed and the CLI-only feature-gated build breaks | Medium | Medium | NFR1 `--no-default-features` check is an explicit acceptance criterion; AC-1 repository-wide symbol search |
| Relocating `detect_osc7_cwd` regresses `Pane.cwd` hot-upgrade handoff or pane restoration | Low | High | Tests move with the function unchanged; mux_hot_upgrade integration test (`--test-threads=1`) is an explicit acceptance criterion (TS5) |
| Grid-height parity not actually achieved (a hidden mux-conditional row path remains) | Low | High | TS1/TS4 unit tests assert row count and inset candidates are driven only by general status-bar state |
| Stored settings.json with `mux.statusbar` fails to load after schema removal | Low | High | Shared settings-mirror contract; TS3 fixture tests on both Rust mirrors |
| Pre-existing tabs.rs replay-test flakiness under parallel execution muddies verification | Medium | Low | Integration suite runs with `--test-threads=1` per workflow.yaml command; flakiness is documented as pre-existing, not introduced |

## Open Questions

- [ ] None blocking. Traceability note (intentional, not an oversight): FR2
      and FR3 have no SPEC test-scenario mapping — they are verified by the
      AC-1 repository-wide symbol search plus the build/test gates; NFR1–NFR3
      are verified by the build/check commands and existing suites staying
      green (see VERIFICATION.md). Their `tests` arrays in workflow.yaml
      stay empty by design.
