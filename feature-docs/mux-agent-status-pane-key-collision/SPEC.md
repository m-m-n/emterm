# Feature: mux-agent-status-pane-key-collision

## Overview

The GUI keys mux agent-status state by the daemon-local wire `pane_id` alone
(`PaneKey::MuxPane(u32)`, `src-tauri/src/agent_status_model.rs:32-40`), so one
eMterm process attached to two mux daemons collides two unrelated panes on one
key and cross-wires their status badges. This feature scopes the mux key by a
GUI-local mux connection identity (`Tab::stable_id`) paired with the wire
`pane_id`, and propagates that scope through every consumer of the key: badge
aggregation, the `pane_id -> public_pane_id` map, the notification rate-limit
key, notification tab-title resolution, and pane-close discard. The fix stays
entirely inside the GUI process — no mux wire-protocol change — so an unchanged
and an older daemon keep working.

Requirements source: `feature-docs/mux-agent-status-pane-key-collision/REQUIREMENTS.md`.

## Objectives

- Agent-status badges stay truthful when one eMterm process is attached to
  several mux daemons at once: a pane's state reaches only the tab whose own mux
  connection reported it.
- Remove every consequence of the shared wire-`pane_id` key at once (badge
  aggregation, public-pane-id map, notification rate limiting, notification
  tab-title resolution, pane-close discard) so no residual cross-daemon bleed is
  left behind.
- Fix entirely inside the GUI process: no mux wire-protocol change, so an
  unchanged daemon (and an older daemon) keeps working.

## User Stories

### US1: Per-connection badge truth with two daemons

As an eMterm user with tab 1 attached to server 1's mux daemon and tab 2 attached
to server 2's mux daemon (both on their daemon's window 1, i.e. both holding wire
`pane_id` 1), I want each tab's agent-status badge to reflect only its own
server's pane, so that driving Claude Code on tab 1 never moves tab 2's badge.

**Acceptance Criteria:**
- [ ] AC-1: Driving Claude Code on tab 1 through working / done changes tab 1's
  badge only; tab 2's badge continues to reflect only its own pane.
- [ ] AC-2: Two panes that share a wire `pane_id` but differ in connection scope
  occupy two independent entries in `AgentStatusModel`: applying a daemon update
  to one leaves the other's state, name, revision and unseen flag untouched.
- [ ] AC-4: A notification raised for a transition on scope A's pane carries
  scope A's tab title, even when another tab holds a pane with the same wire
  `pane_id`.

### US2: No residual cross-daemon bleed behind the same key

As a maintainer, I want every consequence of the shared key removed together
(public-pane-id map, rate-limit key, tab-title resolution, close discard), so
that no latent cross-daemon defect is left behind the fixed badge symptom.

**Acceptance Criteria:**
- [ ] AC-3: Learning `public_pane_id` for scope A's pane 1 does not overwrite or
  remove scope B's `public_pane_id` for its own pane 1; each scope's rate-limit
  key resolves to its own daemon's `"{incarnation}-{pane_id}"` string, and the
  two keys differ.
- [ ] AC-5: Closing (PtyExited) scope A's pane 1 discards scope A's model entry,
  its `mux_public_pane_ids` entry, and its rate-limit bookkeeping, and leaves
  scope B's pane-1 entries fully intact.
- [ ] AC-7: A regression test exists that fails on the pre-fix code and passes
  after: it drives two panes sharing a wire `pane_id` under different connection
  scopes and asserts no cross-scope bleed.

### US3: Unchanged daemon and unchanged plain tabs

As an eMterm user running a fixed GUI against an unmodified — or older — mux
daemon, and as a user of plain (non-mux) tabs, I want no behavioral change at
all, so that the fix is safe to deploy on the GUI side alone.

**Acceptance Criteria:**
- [ ] AC-8: `crates/mux_ipc` is unmodified: the wire types and the
  `PublicPaneId` format are byte-for-byte the same as before the change.
- [ ] AC-6: Plain-tab (`PaneKey::Tab`) status handling — set/clear, unseen flag,
  revision minting, latch-driven inferred clear, discard-on-close — is unchanged;
  the existing plain-tab tests pass without modification of their expectations.

## Technical Requirements

### Functional Requirements

- **FR1 — Scope the mux agent-status key by GUI-local mux connection identity:**
  `AgentStatusModel`'s mux key MUST identify a pane by (GUI-local mux connection
  identity, wire `pane_id`) instead of the wire `pane_id` alone — i.e.
  `PaneKey::MuxPane(u32)` at `src-tauri/src/agent_status_model.rs:32-40` gains a
  connection-scope component. The concrete GUI-side value that serves as that
  connection identity is `Tab::stable_id` (`src-tauri/src/tabs/mod.rs:249`,
  minted from the process-lifetime `NEXT_TAB_STABLE_ID` counter at
  `src-tauri/src/tabs/mod.rs:43`): a mux-attached tab owns exactly one mux
  connection — its own PTY runs `emterm mux attach`, and every mux frame is
  extracted from that tab's stream and routed through `Tab::apply_mux_message`
  (`src-tauri/src/tabs/mux_link.rs:56`). There is no other
  connection/client/session object on the GUI side; `Tab::mux_session_name` and
  `MuxWindowGroup` (`src-tauri/src/mux/window_group.rs:39`) carry no
  daemon-distinguishing identity. Every attached pane therefore has a scope from
  attach time onward, including a pane that has never reported an agent status,
  and the scope is available without any `mux_ipc` change. Enabling this requires
  restoring per-tab attribution at the drain: `App::pump_all` currently flattens
  `tab.take_pending_agent_status_updates()` and
  `tab.take_closed_agent_status_panes()` into untagged vectors
  (`src-tauri/src/app/mod.rs:1091-1092`), unlike the plain-tab events and latch
  inputs which are already tagged with `tab_stable_id` at
  `src-tauri/src/app/mod.rs:1086` / `:1089`; both mux drains MUST carry the
  originating tab's `stable_id` into `App::apply_agent_status_batch`
  (`src-tauri/src/app/agent_status.rs:235`).

- **FR2 — Badge aggregation reads only the querying tab's own scope:** Every
  badge/aggregation read path MUST query connection-scoped keys so a tab's badge
  can never include another tab's panes: `agent_status_keys_for_tab`
  (`src-tauri/src/app/agent_status.rs:15`) builds `PaneKey::MuxPane` from
  `group.pane_ids()` and MUST pair each with the owning tab's scope;
  `App::agent_status_badge_for` (`src-tauri/src/app/agent_status.rs:110`,
  consumed at `src-tauri/src/render/mod.rs:267`) MUST aggregate only that scoped
  set; `App::agent_status_pane_badge` (`src-tauri/src/app/agent_status.rs:120`,
  consumed for mux-sidebar entries at `src-tauri/src/render/mod.rs:311`) MUST
  take the sidebar's owning tab scope alongside the wire `pane_id`; and
  `AgentStatusModel::any_pane_has_reported_state`
  (`src-tauri/src/agent_status_model.rs:315`, called by the `next-agent-window`
  action at `src-tauri/src/app/mux_ui.rs:501`) MUST match within the acting tab's
  scope only.

- **FR3 — Connection-scope the `pane_id -> public_pane_id` map:**
  `App::mux_public_pane_ids: HashMap<u32, String>`
  (`src-tauri/src/app/mod.rs:225`) MUST be keyed by the same connection-scoped
  identity as FR1, so a learn/refresh from one daemon's `AgentStatusUpdate`
  (`src-tauri/src/app/agent_status.rs:276-277`) can no longer overwrite another
  daemon's entry for the same wire `pane_id`. The `App::mux_public_pane_id`
  accessor (`src-tauri/src/app/agent_status.rs:132`) MUST be scoped in the same
  way, and the removal at `src-tauri/src/app/agent_status.rs:291` MUST remove
  only the scoped entry.

- **FR4 — Notification rate-limit key stays collision-free across daemons:**
  `agent_notification_rate_limit_key` (`src-tauri/src/app/agent_status.rs:83`)
  MUST derive the per-pane rate-limit key from the FR3 scoped map, so the
  daemon-minted `public_pane_id` (`"{incarnation}-{pane_id}"`,
  `crates/mux_ipc/src/protocol.rs:961-978`) it prefers is always the one
  belonging to the pane's own daemon. Its `"mux:{pane_id}"` fallback (used when a
  pane is discarded before its public id was ever learned) MUST also include the
  connection scope so two daemons' pane 1 do not share a fallback key. Its four
  call sites (`src-tauri/src/app/agent_status.rs:290` and `:321`,
  `src-tauri/src/app/tab_lifecycle.rs:143`, `src-tauri/src/app/mod.rs:1460`) MUST
  all derive the same key.

- **FR5 — Notification tab-title resolution picks the owning tab, not the first
  match:** `agent_status_pane_tab_title`
  (`src-tauri/src/app/agent_status.rs:54`) currently returns the first tab whose
  `mux_group.pane_ids()` contains the wire `pane_id`, which with two daemons can
  name the wrong tab in the notification body. It MUST resolve the tab by the
  transition's connection scope, so the notification names the tab whose
  connection actually reported the transition. The same rule applies to
  `agent_status_pane_visible` (`src-tauri/src/app/agent_status.rs:35`), which
  decides visibility by membership in `agent_status_keys_for_tab(active_tab)` and
  would otherwise treat another daemon's pane as visible whenever the active tab
  happens to hold the same wire `pane_id`.

- **FR6 — Pane-close discard removes only the closing daemon's entry:**
  Discarding on pane/tab close MUST be scoped so a pane exiting on one daemon
  never drops another daemon's same-`pane_id` state: the `closed_panes` loop in
  `App::apply_agent_status_batch` (`src-tauri/src/app/agent_status.rs:286-294`,
  fed from `Tab::pending_closed_agent_status_panes` latched at
  `src-tauri/src/tabs/mux_link.rs:821`) MUST discard the scoped `PaneKey`, the
  scoped `mux_public_pane_ids` entry, and the scoped rate-limit key only. The two
  whole-tab discard paths — `App::close_tab`
  (`src-tauri/src/app/tab_lifecycle.rs:142-146`) and the reaped-exited-tab loop
  (`src-tauri/src/app/mod.rs:1453-1463`) — already iterate
  `agent_status_keys_for_tab`, so they inherit the scope from FR2 and MUST NOT
  discard any key outside the closing tab's own scope.

### Non-Functional Requirements

- **NFR1 - Compatibility (no mux wire-protocol change):** The fix stays inside
  the GUI process. `crates/mux_ipc` (including `AgentStatusUpdateMsg` at
  `crates/mux_ipc/src/protocol.rs:885` and `PublicPaneId` at `:967`) and the
  daemon-side pane-id allocator (`src-tauri/src/mux/session/manager.rs:76`) are
  unchanged, so an unmodified — and an older — daemon keeps working with a fixed
  GUI.

- **NFR2 - Performance (per-frame read paths stay cheap):**
  `agent_status_badge_for` and `agent_status_pane_badge` run once per tab / per
  sidebar entry on every rendered frame (`src-tauri/src/render/mod.rs:267`,
  `:311`), and `next-agent-window` resolves its qualify list at key-event time
  with no polling. The scoped key MUST keep these as O(1) hash lookups — no
  per-frame scan across all tabs and no new per-frame allocation beyond what
  `agent_status_keys_for_tab` already does.

- **NFR3 - Compatibility (plain-tab behavior unchanged):** `PaneKey::Tab(u64)`
  and the whole plain-tab path (OSC 777 parsing, the inferred-clear latch,
  revision minting, `AgentStatusModel::discard`'s latch cleanup) MUST behave
  identically before and after the change; only the mux key gains a scope.

- **NFR4 - Maintainability (documented invariants stay accurate):** The doc
  comments that pin the current semantics — `PaneKey::MuxPane`'s "keyed by the
  wire `pane_id`" note (`src-tauri/src/agent_status_model.rs:36-39`) and
  `agent_notification_rate_limit_key`'s "unique across concurrent panes" claim
  (`src-tauri/src/app/agent_status.rs:71-82`) — MUST be updated to state the new
  scoping rule rather than left describing the buggy contract.

## Implementation Approach

### Architecture

The change is confined to the GUI process. The daemon side and the wire are
untouched (NFR1).

```
mux daemon (server 1)                     mux daemon (server 2)
  wire pane_id 1                            wire pane_id 1
        │  AgentStatusUpdateMsg                    │  AgentStatusUpdateMsg
        │  (crates/mux_ipc — UNCHANGED)            │  (UNCHANGED)
        ▼                                          ▼
 Tab A PTY (`emterm mux attach`)          Tab B PTY (`emterm mux attach`)
 Tab::apply_mux_message                   Tab::apply_mux_message
   tabs/mux_link.rs:56                      tabs/mux_link.rs:56
        │                                          │
        │ per-tab pending queues                   │
        ▼                                          ▼
   App::pump_all  (app/mod.rs:1091-1092) — drains MUST be tagged with
                   the originating tab's `stable_id` (FR1)
                            │
                            ▼
   App::apply_agent_status_batch  (app/agent_status.rs:235)
        ├─ AgentStatusModel        key: (scope, wire pane_id)   FR1
        ├─ App::mux_public_pane_ids key: (scope, wire pane_id)  FR3
        └─ notification rate-limit key derived from the scoped map  FR4
                            │
                            ▼
   read paths: agent_status_keys_for_tab / agent_status_badge_for /
               agent_status_pane_badge / any_pane_has_reported_state  FR2
               agent_status_pane_tab_title / agent_status_pane_visible FR5
                            │
                            ▼
   render/mod.rs:267 (tab badge), :311 (mux sidebar entry) — UNCHANGED consumers
```

**Component notes:**

- The connection scope is `Tab::stable_id` (`src-tauri/src/tabs/mod.rs:249`),
  because a mux-attached tab owns exactly one mux connection (FR1). It is a
  process-local counter value, never transmitted and never rendered.
- `MuxWindowGroup` (`src-tauri/src/mux/window_group.rs:39`) and
  `Tab::mux_session_name` carry no daemon-distinguishing identity and are
  therefore not usable as the scope.
- Badge rendering (`src-tauri/src/ui/tab_bar/badge.rs`), the mux sidebar
  (`src-tauri/src/ui/mux_sidebar.rs`) and the status bar consume only the already
  defined aggregated value and are untouched by the key change. The design step
  is skipped for this reason.

### Data Flow

```
daemon update ─► tab stream ─► per-tab queue ─► pump_all (tagged with stable_id)
              ─► apply_agent_status_batch ─► scoped model / scoped map / scoped
                 rate-limit key ─► scoped read paths ─► per-tab badge
pane close    ─► mux_link latch ─► pump_all (tagged) ─► scoped discard only
```

### Keyed State

| State | Current key | Required key | Requirement |
|-------|-------------|--------------|-------------|
| `AgentStatusModel` mux entry (`PaneKey::MuxPane`) | wire `pane_id` (`u32`) | (connection scope, wire `pane_id`) | FR1 |
| `App::mux_public_pane_ids` | wire `pane_id` (`u32`) | the same scoped identity | FR3 |
| Notification rate-limit key | `public_pane_id`, else `"mux:{pane_id}"` | scope-derived `public_pane_id`, else a scope-including fallback | FR4 |
| `AgentStatusModel` plain-tab entry (`PaneKey::Tab`) | `Tab::stable_id` (`u64`) | unchanged | NFR3 |

### Dependencies

**Internal:**
- `src-tauri/src/agent_status_model.rs` — `PaneKey`, `AgentStatusModel`
  (`aggregate`, `discard`, `any_pane_has_reported_state`, `counts`).
- `src-tauri/src/app/agent_status.rs` — key derivation, badge reads, rate-limit
  key, tab-title resolution and visibility, `apply_agent_status_batch`.
- `src-tauri/src/app/mod.rs` — `pump_all` drain tagging, `mux_public_pane_ids`,
  reaped-exited-tab discard loop.
- `src-tauri/src/app/tab_lifecycle.rs` — `close_tab` discard path.
- `src-tauri/src/tabs/mux_link.rs` — mux frame routing, pane-close latch, detach.
- `src-tauri/src/app/mux_ui.rs` — `next-agent-window` action.
- `src-tauri/src/render/mod.rs` — per-frame badge consumers (call sites adapt to
  the scoped signatures; rendering semantics unchanged).

**External:**
- `crates/mux_ipc` — read-only dependency; MUST remain unmodified (NFR1, AC-8).

### File Structure

```
src-tauri/src/
├── agent_status_model.rs          # PaneKey::MuxPane gains a connection scope (FR1, NFR4)
├── agent_status_model/
│   └── tests.rs                   # TS-1, TS-2, TS-3, TS-10
├── app/
│   ├── agent_status.rs            # FR2-FR6, NFR4
│   ├── mod.rs                     # tagged drains (FR1), scoped map (FR3), reap loop (FR6)
│   ├── tab_lifecycle.rs           # scoped close discard (FR4, FR6)
│   ├── mux_ui.rs                  # scoped next-agent-window (FR2)
│   └── tests/
│       └── agent_status.rs        # TS-4 .. TS-10
├── tabs/mux_link.rs               # per-tab attribution at the source (FR1, FR6)
└── render/mod.rs                  # scoped badge call sites (FR2, NFR2)
crates/mux_ipc/                    # UNMODIFIED (NFR1, AC-8)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths above are derived at create-plan from every task's
`files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/mux-agent-status-pane-key-collision/**`
- `test-docs/mux-agent-status-pane-key-collision/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it.

## Test Scenarios

Regression coverage is inline `#[cfg(test)]` unit tests over `AgentStatusModel`
and the `src-tauri/src/app/agent_status.rs` helpers, driving two panes that share
a wire `pane_id` but differ in connection scope. No two-daemon integration test
is added.

### Unit Tests

- [ ] **TS-1** (`src-tauri/src/agent_status_model/tests.rs`, inline
  `#[cfg(test)]`) — FR1: Two connection scopes, same wire `pane_id`: apply a
  daemon update (Working) under scope A and none under scope B; `aggregate` over
  scope A's key set yields Working, over scope B's yields None. Then apply
  Blocked under scope B and assert scope A still reads Working. Covers AC-2.
- [ ] **TS-2** (`src-tauri/src/agent_status_model/tests.rs`) — FR6: `discard` of
  scope A's (pane_id 1) key leaves scope B's (pane_id 1) entry present and
  unchanged. Covers AC-5's model half.
- [ ] **TS-3** (`src-tauri/src/agent_status_model/tests.rs`) — FR2:
  `any_pane_has_reported_state` is scope-sensitive: a reported state under scope
  A does not make scope B's same-numbered pane qualify for the
  `next-agent-window` cycle. Covers FR2's cycle path.
- [ ] **TS-4** (`src-tauri/src/app/tests/agent_status.rs`) — FR1, FR2:
  `agent_status_keys_for_tab` over two mux-attached tab fixtures whose
  `MuxWindowGroup`s hold identical `pane_ids()` produces two disjoint key sets
  (no key appears in both). Covers FR2.
- [ ] **TS-5** (`src-tauri/src/app/tests/agent_status.rs`) — FR4:
  `agent_notification_rate_limit_key` returns different keys for scope A pane 1
  and scope B pane 1, both when each scope has learned its own `public_pane_id`
  ("aaa-1" vs "bbb-1") and on the never-learned fallback path. Covers AC-3.
- [ ] **TS-6** (`src-tauri/src/app/tests/agent_status.rs`) — FR5:
  `agent_status_pane_tab_title` over two tabs whose groups both contain wire
  `pane_id` 1 returns each tab's own title for its own scoped key (not the
  first-match title for both). A companion assertion covers
  `agent_status_pane_visible`: a non-active tab's pane is not reported visible
  merely because the active tab holds the same wire `pane_id`. Covers AC-4.
- [ ] **TS-7** (`src-tauri/src/app/tests/agent_status.rs`) — FR3:
  `App::apply_agent_status_batch` fed updates tagged for two different tab scopes
  but carrying the same `update.pane_id` (and different `public_pane_id`s) stores
  both `mux_public_pane_ids` entries; querying each scope's public id returns its
  own daemon's string. Covers AC-3.
- [ ] **TS-8** (`src-tauri/src/app/tests/agent_status.rs`) — FR6, FR3, FR4:
  `App::apply_agent_status_batch` with a `closed_panes` entry tagged for scope A
  and wire `pane_id` 1 discards scope A's model entry, `mux_public_pane_ids`
  entry and rate-limit bookkeeping, while scope B's pane-1 entries all survive.
  Covers AC-5 end to end within the batch. **This is the primary AC-7 regression
  test.**
- [ ] **TS-9** (`src-tauri/src/app/tests/agent_status.rs`) — FR2:
  `App::agent_status_badge_for` returns the correct, non-shared badge for each of
  two mux-attached tab fixtures whose groups hold the same `pane_ids()` but
  different reported states — the direct unit-level analogue of the reported
  symptom. Covers AC-1's testable core.
- [ ] **TS-10** (`src-tauri/src/agent_status_model/tests.rs` and
  `src-tauri/src/app/tests/agent_status.rs`) — NFR3: Existing plain-tab tests
  (set/clear, unseen preservation, replay_derived silence, latch-driven inferred
  clear, discard-on-close) still pass with their original expectations,
  confirming `PaneKey::Tab` semantics are untouched. Covers AC-6.

### Integration Tests

None. The defect is entirely GUI-side key derivation and the collision is fully
reproducible in-process, so no two-daemon integration test is added; this also
matches the repo convention of inline unit tests next to the code.

### E2E Tests

**Existing E2E tests**: None (no E2E infrastructure exists in this repository).
**Run command**: Not detected.

### Edge Cases

- [ ] **EC-1**: Two tabs attached to the SAME daemon session and the same pane:
  with a per-tab connection scope these become two distinct model entries for one
  daemon pane. Each tab's badge still tracks that pane correctly (the daemon
  pushes `AgentStatusUpdate` to each owning connection independently), so this is
  duplication, not contamination — but the model's global `counts()` would count
  the pane twice. `counts()` has no non-test caller today.
- [ ] **EC-2**: Detach then re-attach in the SAME tab keeps the same
  `Tab::stable_id`, so the tab's scope is unchanged across the re-attach.
  `handle_detached` (`src-tauri/src/tabs/mux_link.rs:872-912`) clears `mux_group`
  without latching its panes into `pending_closed_agent_status_panes`, so that
  tab's mux entries are never discarded on detach (pre-existing behavior — after
  the clear, `agent_status_keys_for_tab` no longer yields them, so even tab close
  cannot reach them). With the scoped key, a re-attach that reuses wire `pane_id`
  1 in that same tab can therefore surface the pre-detach state as the new pane's
  badge until the daemon's first update arrives. Whether to latch the pane ids at
  detach is a decision for the plan step; it does not change any FR above.
- [ ] **EC-3**: A mux tab whose daemon predates the window-list/
  `AgentStatusUpdate` protocol never installs a `mux_group`, so it contributes no
  `PaneKey::MuxPane` keys at all; the scoped key must leave that path as a plain
  tab exactly as today.
- [ ] **EC-4**: A transition drained for a pane whose tab closed between the
  transition firing and the drain: `agent_status_pane_tab_title` returns `None`
  and the caller falls back to an empty title. Scoped resolution must keep
  returning `None` here rather than falling back to some other tab that happens
  to hold the same wire `pane_id`.
- [ ] **EC-5**: The daemon's `incarnation` token changes on every daemon restart
  (`src-tauri/src/mux/session/manager.rs:58-68`), so reconnecting to a restarted
  daemon on the same host yields new `public_pane_id`s for reused wire
  `pane_id`s. The GUI-local scope is independent of that, so this affects only
  the FR3/FR4 refresh path, which already overwrites on each update.

### Performance Tests

No dedicated load or stress test. NFR2 is satisfied structurally: the scoped key
must remain an O(1) hash lookup on the per-frame read paths, with no per-frame
scan across all tabs and no new per-frame allocation beyond what
`agent_status_keys_for_tab` already does.

## Security Considerations

- **Input Validation:** The daemon-minted `public_pane_id` remains an opaque
  string handled only as a map value and rate-limit key; no new parsing of it is
  introduced on the GUI side, so `PublicPaneId::parse`'s malformed-input surface
  is not newly exercised.
- **Data Protection / information exposure:** The connection scope is a
  process-local counter value, never transmitted over the mux wire and never
  rendered to the user, so it introduces no new information exposure between
  servers.
- Authentication, authorization, XSS, SQL injection and CSRF are not applicable:
  this feature adds no network surface, no web surface and no data store.

## Error Handling

No new error codes or user-facing error paths. The one absence-handling rule is
FR5 / EC-4: when a transition's owning tab no longer exists,
`agent_status_pane_tab_title` returns `None` and the caller falls back to an
empty title, rather than resolving some other tab that happens to hold the same
wire `pane_id`.

## Performance Optimization

### Performance Goals

- `agent_status_badge_for` and `agent_status_pane_badge`: O(1) hash lookups per
  key, once per tab / per sidebar entry per rendered frame (NFR2).
- No per-frame scan across all tabs; no new per-frame allocation beyond what
  `agent_status_keys_for_tab` already does (NFR2).
- `next-agent-window` resolves its qualify list at key-event time, with no
  polling (NFR2).

### Caching Strategy

None added. `App::mux_public_pane_ids` remains the learned-value map; it only
changes key shape (FR3).

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested
- [ ] All non-functional requirements (NFR1-NFR4) hold
- [ ] All test scenarios (TS-1 .. TS-10) pass
- [ ] All acceptance criteria (AC-1 .. AC-8) are met
- [ ] AC-7's regression test fails on pre-fix code and passes after
- [ ] `crates/mux_ipc` is byte-for-byte unmodified (AC-8)
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` passes
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` passes
- [ ] `make fmt` applied
- [ ] The doc comments named in NFR4 describe the new scoping rule
- [ ] Code review is completed

## Open Questions

> **Note**: Unresolved requirements are tracked in workflow.yaml as
> `status: tbd`. Resolve them before running the plan phase.

None. FR1-FR6 and NFR1-NFR4 are all `status: resolved`; no requirement carries a
`tbd_reason`. One plan-step decision is recorded as an edge case rather than an
open requirement: whether to latch the pane ids at detach (EC-2).

## Assumptions

- **A1** (answer to `requirement.pane-identity-source`, option
  `gui_local_connection_scope`; batch-codex-consultation): The agent-status state
  is keyed on a GUI-local mux connection scope paired with the wire `pane_id`,
  not on a new daemon-supplied identifier; no `mux_ipc` wire-shape change is
  made.
- **A2** (answer to `requirement.fix-scope`, option `all_root_effects`;
  batch-codex-consultation): The fix covers every consequence of the shared key —
  badge aggregation, the `pane_id -> public_pane_id` map, the notification
  rate-limit key, notification tab-title resolution, and pane-close discard —
  rather than only the reported badge symptom.
- **A3** (answer to `testing.regression-test-level`, option `unit_only`;
  batch-codex-consultation): Regression coverage is inline `#[cfg(test)]` unit
  tests over `AgentStatusModel` and the `src-tauri/src/app/agent_status.rs`
  helpers, driving two panes that share a wire `pane_id` but differ in connection
  scope. No two-daemon integration test is added.

Full rationale for each assumption is recorded in REQUIREMENTS.md section 14.1.

## References

- Requirements document: `feature-docs/mux-agent-status-pane-key-collision/REQUIREMENTS.md`
- `src-tauri/src/agent_status_model.rs` — `PaneKey`, `AgentStatusModel`
- `src-tauri/src/app/agent_status.rs` — key derivation, badge reads, rate-limit
  key, tab-title resolution, batch apply
- `src-tauri/src/app/mod.rs` — `pump_all` drain, `mux_public_pane_ids`,
  reaped-exited-tab loop
- `src-tauri/src/app/tab_lifecycle.rs` — `close_tab` discard path
- `src-tauri/src/tabs/mod.rs` — `Tab::stable_id`, `NEXT_TAB_STABLE_ID`
- `src-tauri/src/tabs/mux_link.rs` — mux frame routing, close latch, detach
- `src-tauri/src/mux/window_group.rs` — `MuxWindowGroup`
- `src-tauri/src/app/mux_ui.rs` — `next-agent-window`
- `src-tauri/src/render/mod.rs` — per-frame badge consumers
- `crates/mux_ipc/src/protocol.rs` — `AgentStatusUpdateMsg`, `PublicPaneId`
- `src-tauri/src/mux/session/manager.rs` — daemon pane-id allocator, incarnation
