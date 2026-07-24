# Implementation Plan: mux Agent Status & Agent-Facing API

## Overview

Agent panes report state via an `agent-status` OSC; daemon and GUI hold the
state, the GUI visualizes and notifies, and the mux socket gains
read / send / wait requests for agents. Eight parallel tasks share the
contracts below.

## Technology Stack

- **Language**: Rust (native stack), TypeScript (settings panel mirror)
- **New external dependencies**: none (license check: no additions against MIT)

## Layer Structure

- `src-tauri/src/agent_status.rs` — build-agnostic core (types, wire
  grammar, parsing, name sanitization). CLI-shared: compiled WITHOUT the
  `gui` feature; depends only on std. Everything else depends on it;
  it depends on nothing feature-gated.
- `crates/mux_ipc` — transport message shapes (status update, API
  request/response). No business logic.
- `src-tauri/src/mux/*` (daemon side) — state ownership for mux panes,
  replay stripping, API handlers.
- GUI (`callbacks.rs` / `tabs.rs` / `app.rs` + `agent_status_model.rs`) —
  state ownership for plain tabs, merged view, seen tracking.
- `src-tauri/src/ui/*` — rendering only; reads the model, never mutates
  semantic state (except mark-seen on foreground display).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| `agent_status` core module (`src-tauri/src/agent_status.rs`) | AgentState enum (Idle/Working/Blocked/Done); AgentStatusEvent (Set{state, name} / Clear); parse the OSC payload; build the wire string; sanitize names | Parse input: the OSC 777 payload string starting `emterm;agent-status;…`. Returns Set/Clear event or None (whole-sequence rejection on: missing/invalid state, duplicate key, bad percent-encoding). Name postcondition: percent-decoded UTF-8, control characters stripped, truncated to 80 chars. Unknown keys ignored. Build output: the exact FR1 wire strings (set includes `v=1`). Module has no `gui`-gated dependencies | task0001, task0003, task0005 |
| mux_ipc protocol additions (`crates/mux_ipc/src/protocol.rs`) | New MessageTypes + serde payloads: `AgentStatusUpdate{pane_id: u32, public_pane_id: String, state: Option<AgentState>, name: Option<String>, revision: u64, replay_derived: bool}` (daemon→GUI, unsolicited); `ReadPane{public_pane_id, lines}` → `ReadPaneResult{text}`; `SendText{public_pane_id, bytes}` → `SendTextResult{revision_watermark}`; `WaitAgentState{public_pane_id, states, timeout_ms, after_revision}` → `WaitAgentStateResult{state, revision}`; shared `AgentApiError{kind: unknown_pane / not_mux_pane / timeout / pane_gone / invalid_input, message}` | Existing message encodings, StatusUpdate, and Snapshot payload bytes are byte-identical to today. PROTOCOL_VERSION is bumped; version mismatch at handshake fails cleanly (existing handshake rules). All new payloads are serde-encoded like existing JSON-payload messages | task0002 (defines), task0003, task0004, task0005 |
| Public pane ID format | Stable non-reusable API-facing pane identifier | Opaque string `"{incarnation}-{pane_id}"`: `incarnation` = lowercase-hex token generated once at daemon start (derived from start time + random); `pane_id` = the existing wire u32. Postconditions: never re-used across daemon restarts; encodes no window/tab position or name. Daemon is the only minter; clients treat it as opaque | task0002, task0003, task0004, task0006 |
| GUI `AgentStatusModel` (`src-tauri/src/agent_status_model.rs`) | Single merged store of per-pane AgentStatus (+ GUI-local seen flag) for both plain tabs and mux panes | API surface: apply a daemon update (with replay_derived flag); apply a plain-tab OSC event (tab-local, revision minted by the model); discard on tab/pane close; `aggregate(tab)` → highest-priority (blocked > unseen done > working > seen done > idle) state + unseen flag; per-state counts over semantic state; `mark_seen` for a tab shown in the foreground window; drains a queue of real-transition events `{pane, old_state, new_state, name}` for the notification layer (replay_derived and same-state re-reports never enqueue) | task0005 (owns + wires into app), task0006 (render reads), task0007 (drains transitions) |
| Agent notification setting | `agent_status_notifications: bool` (default true) in settings.json | Rust field in app_settings with serde default true; mirrored in the TS `AppSettings` interface; read at notification time (no restart needed) | task0007 |

Integration wiring owner: task0005 wires `AgentStatusModel` into the app
state and both ingestion paths; task0006/task0007 compile against the
contract above and read the model from app state.

## Conventions

- `emterm mux read/send/wait` exit codes: 0 success; 2 usage/invalid input;
  3 wait timeout (dedicated); 4 unknown pane / pane gone; 5 not_mux_pane;
  1 all other errors (connection failure etc.).
- Diagnostics use `log::warn!`+ (release log persists warn+ only).
- GUI strings via inline `t(ja, en)` (`crate::i18n`).
- UI colors via `ui::md3` role accessors only — state mapping: blocked →
  `on_error_container`, working → `primary`, done →
  `on_secondary_container`, idle → `on_surface_variant`. Badge: 8px dot,
  6px gap before title; unseen = filled, seen = 1.5px ring (blocked/done
  only). Status-bar summary: right-aligned in the app row, dot + count per
  state (order blocked/working/done/idle), label-extra-small, zero counts
  omitted, hidden when empty. No animation.
- Rejected input (OSC or API) leaves ALL state untouched — no partial
  application.

## Cross-task Design Decisions

### Revision semantics (task0003, task0004, task0005)
`revision: u64` starts at 0 per pane; every ACCEPTED report (set, clear,
same-state re-report) increments it. SendText returns the pane's revision
as observed immediately before the successful PTY write (watermark).
WaitAgentState is level-triggered over (state-in-set AND revision >
after_revision when given). Plain tabs mint their own revisions inside the
model (API never targets plain tabs, so scopes cannot mix).

### Replay separation (task0003, task0005)
The daemon strips agent-status OSC bytes from scrollback storage and
snapshot replay via the existing strip mechanism. After a
snapshot/reattach, the daemon sends one AgentStatusUpdate per stateful
pane with `replay_derived: true`; the model applies these silently (no
transition events). Snapshot payload format is unchanged.

### Wait implementation (task0004)
Waiters are registered per pane with an optional deadline; every accepted
state change re-evaluates them (level-triggered). Client disconnect and
pane destruction discard/fail the waiter. No busy polling.

### Notification gating (task0007)
Notify only when: a drained transition event's new state is blocked or
done, AND the pane is not visible in the foreground OS window, AND both
the agent-notification setting and the existing global notification
switch are on, AND the per-pane rate limit (minimum interval between
notifications for one pane) is not exceeded. Bodies use the sanitized
name (sanitization guaranteed by the core module).

### Pane-ID copy placement (task0006; resolves the DESIGN.md open item)
The tab bar has no context-menu infrastructure, so the copy affordance
lives in the mux window/pane UI (`mux_sidebar.rs` / `mux_dialogs.rs`): a
click-to-copy of the pane's public ID, labeled via t(ja, en).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Concurrent edits to `mux/ipc/handlers.rs` and `daemon.rs` by task0003/task0004 | High | Merge conflicts | Parent-side-adoption protocol; both tasks follow the same shared contracts so re-implementation is mechanical |
| Wait/waiter lifecycle races (disconnect vs state change vs pane destroy) | Medium | Hung CLI or leaked waiters | Contract fixed above; task0004 tests cover disconnect/destroy paths |
| Replay-derived updates leaking notifications | Medium | Spurious notifications on attach | replay_derived flag is part of the message contract; model enqueues transitions only for non-replay accepted reports |
| Old GUI × new daemon pairing | Low | Misparse | PROTOCOL_VERSION bump; handshake rejects cleanly |

## Open Questions

- [ ] None blocking. (DESIGN.md's context-menu open item is resolved above.)
