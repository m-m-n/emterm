# Feature: mux-detach-agent-status-cleanup

## Overview

When a tab leaves mux mode via a daemon-confirmed `Detached` frame, the agent
status entries that its mux group's wire panes held are never discarded, so a
subsequent attach on the same tab shows the previous connection's state on the
tab badge and in the mux sidebar. This feature makes `Tab::handle_detached`
queue the group's pane ids into the existing `closed_panes` teardown chain
before clearing `mux_group`, releasing the model entry, the scoped
public-pane-id mapping, and the notification rate-limit state for each pane. It
also corrects the `ConnectionScope` doc comment, which currently asserts a
lifetime property the implementation does not provide.

Requirements document:
`feature-docs/mux-detach-agent-status-cleanup/REQUIREMENTS.md`.

## Objectives

- A tab that leaves mux mode stops carrying the previous connection's agent
  state, so a re-attach (to the same or a different daemon) shows only the new
  connection's panes on the tab badge and mux sidebar.
- Agent-status bookkeeping stops growing monotonically across repeated detach
  cycles: model entries, scoped public-pane-id mappings and notification
  rate-limit state are released at detach rather than at process exit.
- The `ConnectionScope` doc comment stops asserting a lifetime property that the
  implementation does not provide, so future readers do not build on a false
  invariant.

## User Stories

### US1: Detach releases the connection's agent state
As an eMterm mux user, I want a tab that leaves mux mode to drop the departing
connection's agent state, so that the tab badge and mux sidebar do not keep
displaying panes that no longer exist.

**Acceptance Criteria:**
- [ ] AC-1: Attach a tab to a mux session, drive a pane to a reported state,
      deliver a `Detached` frame, pump: the tab's aggregated badge reports no
      state from that pane, and `App::mux_public_pane_id` for its (scope, wire
      pane_id) returns `None`.
- [ ] AC-3: A detach releases the notification rate-limit identity of every
      discarded pane, so the same public pane id reported by a new daemon is not
      suppressed by the previous connection's rate-limit entry.
- [ ] AC-4: A detach on a tab whose own plain-tab agent status was set leaves
      the `PaneKey::Tab(stable_id)` entry and its inferred-clear latch intact.
- [ ] AC-5: A detach on one of two tabs whose groups hold the SAME wire pane id
      leaves the other tab's entry, public-pane-id mapping and rate-limit state
      untouched.

### US2: Re-attach shows only the new connection
As an eMterm mux user, I want a fresh attach on a previously detached tab to
show only the new connection's state, so that a re-attach — including one to a
different host — never displays the old connection's panes.

**Acceptance Criteria:**
- [ ] AC-2: After detach and a fresh attach on the SAME tab, and before any new
      `AgentStatusUpdate` arrives, the tab badge and the mux sidebar pane badges
      are empty — no state or agent name from the previous connection is
      displayed.
- [ ] AC-6: The `ConnectionScope` doc comment no longer claims entries survive
      detach and re-attach.
- [ ] AC-7: The full `--lib` suite passes, including the existing
      mux-agent-status-pane-key-collision scoping tests (TS-5, TS-6, TS-7 in
      `src-tauri/src/app/tests/agent_status.rs`) and the detach-driving overlay
      tests in `src-tauri/src/app/tests/mux_ui.rs`.

## Technical Requirements

### Functional Requirements

- **FR1 — Every mux-exit path discards the group's pane entries:** Every
  transition that takes a tab out of mux mode by clearing `Tab::mux_group`
  discards the agent-status entries for the wire pane ids that group held — not
  only the daemon-confirmed `Detached` frame. Investigation found exactly two
  assignments of `mux_group = None` (`src-tauri/src/tabs/mux_link.rs:823` in
  `handle_pty_exited` when the group empties, and
  `src-tauri/src/tabs/mux_link.rs:878` in `handle_detached`) plus the tab-death
  route where `mux_group` stays `Some` and `Tab::exited` drives the reap. Of
  these, only `handle_detached` currently discards nothing; `handle_pty_exited`
  already pushes each removed pane id at
  `src-tauri/src/tabs/mux_link.rs:821`, and bridge death / connection loss / PTY
  close reach `App::pump_all`'s reaped-tab loop
  (`src-tauri/src/app/mod.rs:1473-1490`) or `App::close_tab`
  (`src-tauri/src/app/tab_lifecycle.rs:147-154`), both of which expand the group
  via `agent_status_keys_for_tab`. The requirement is that all of these paths
  satisfy the discard obligation; the change needed to reach that state is
  confined to `handle_detached`.

- **FR2 — Detach reuses the existing closed_panes teardown, no new path:**
  Before clearing `mux_group`, `Tab::handle_detached`
  (`src-tauri/src/tabs/mux_link.rs:872-912`) queues the group's `pane_ids()`
  into `Tab::pending_closed_agent_status_panes`
  (`src-tauri/src/tabs/mod.rs:421`). `App::pump_all` drains it through the
  existing `Tab::take_closed_agent_status_panes`
  (`src-tauri/src/tabs/mod.rs:858-860`) -> `agent_status_closed_panes`
  (`src-tauri/src/app/mod.rs:1108-1112`) -> `App::apply_agent_status_batch`'s
  `closed_panes` loop (`src-tauri/src/app/agent_status.rs:321-332`) chain. No
  parallel teardown routine is introduced.

- **FR3 — All three pieces of per-pane state are released:** For each discarded
  pane the existing `closed_panes` loop releases: the `AgentStatusModel` entry
  (`AgentStatusModel::discard`, `src-tauri/src/agent_status_model.rs:264-269`),
  the scoped `mux_public_pane_ids` entry keyed `(ConnectionScope, wire pane_id)`
  (`src-tauri/src/app/mod.rs:228-229`), and the notification rate-limit state
  keyed by `agent_notification_rate_limit_key`
  (`src-tauri/src/app/agent_status.rs:98-113`). The existing ordering constraint
  is preserved: the rate-limit key is resolved from the still-present public-id
  mapping BEFORE that mapping entry is removed
  (`src-tauri/src/app/agent_status.rs:326-331`).

- **FR4 — ConnectionScope keeps its stable_id derivation; its doc is
  corrected:** `ConnectionScope` remains `ConnectionScope(tab.stable_id)` at
  every derivation site (`src-tauri/src/app/agent_status.rs:25`, `:306`, `:326`,
  `src-tauri/src/app/mux_ui.rs:499`, `src-tauri/src/render/mod.rs:317`). No
  attach-generation counter is added. The doc comment at
  `src-tauri/src/agent_status_model.rs:43-44`, currently reading "Constant for
  the tab's whole lifetime, including across detach and re-attach", is corrected
  so it describes the implemented behaviour (the scope value is constant, but
  the entries it keys are discarded at detach and re-minted on re-attach).

- **FR5 — Only MuxPane entries are discarded at detach:** Detach discards
  `PaneKey::MuxPane(scope, pane_id)` entries only. The tab's own
  `PaneKey::Tab(tab.stable_id)` entry and the per-tab inferred-clear latch that
  `AgentStatusModel::discard` removes alongside a `PaneKey::Tab` key
  (`src-tauri/src/agent_status_model.rs:264-269`) both survive, since the tab
  reverts to a plain tab that keeps reporting OSC 777 status on its own key.
  This follows from FR2's use of the `closed_panes` loop, which only ever
  constructs `PaneKey::MuxPane` (`src-tauri/src/app/agent_status.rs:327`).

- **FR6 — Post-re-attach badge reflects only the new connection:** After a
  detach followed by an attach on the same tab, and before the new daemon has
  pushed any `AgentStatusUpdate`, `App::agent_status_badge_for` for that tab
  (`src-tauri/src/app/agent_status.rs:128-134`) and
  `App::agent_status_pane_badge` for its sidebar entries
  (`src-tauri/src/app/agent_status.rs:141-148`,
  `src-tauri/src/render/mod.rs:314-322`) report no state carried over from the
  previous connection, and `App::mux_public_pane_id` returns `None` for the new
  connection's wire pane ids until the new daemon supplies them.

### Non-Functional Requirements

- **NFR1 - Bounded state growth:** Repeated detach/re-attach cycles on one tab
  leave `AgentStatusModel::entries`, `App::mux_public_pane_ids` and the
  notification rate-limit map bounded by the currently-live panes rather than
  growing once per cycle.
- **NFR2 - Compatibility:** No change to the mux wire protocol
  (`crates/mux_ipc`), to any daemon behaviour, or to any user-visible setting;
  the change is confined to GUI-side state lifecycle in `src-tauri/src`.
- **NFR3 - Preserved scoping guarantees:** Every existing scoping guarantee from
  the mux-agent-status-pane-key-collision work is preserved: a detach on one tab
  never touches another tab's same-numbered wire pane, because every key is
  derived from the detaching tab's own `ConnectionScope(tab.stable_id)`.
- **NFR4 - Maintainability / borrow shape:** `Tab::handle_detached` has no
  `&mut App` access; the fix keeps the existing latch-and-drain indirection
  rather than introducing a new borrow path or a direct model reference from the
  tab layer.
- **NFR5 - CLI-only build:** The CLI-only build (`--no-default-features`) is
  unaffected — all touched modules are GUI-gated.

## Implementation Approach

### Architecture

**Component layering:**

```
┌───────────────────────────────────────────────────────────┐
│ render / ui (tab_bar, mux_sidebar)   — unchanged          │
├───────────────────────────────────────────────────────────┤
│ App  (app/mod.rs, app/agent_status.rs, app/mux_ui.rs)     │
│   pump_all -> agent_status_closed_panes                   │
│            -> apply_agent_status_batch (closed_panes)     │
├───────────────────────────────────────────────────────────┤
│ Tab  (tabs/mod.rs, tabs/mux_link.rs)                      │
│   handle_detached / handle_pty_exited                     │
│   pending_closed_agent_status_panes (latch)               │
├───────────────────────────────────────────────────────────┤
│ AgentStatusModel (agent_status_model.rs)                  │
│   entries keyed by PaneKey::{Tab, MuxPane(scope, id)}     │
└───────────────────────────────────────────────────────────┘
```

**Component notes:**

- The tab layer has no `&mut App` (NFR4), so it communicates the discard
  obligation through the `pending_closed_agent_status_panes` latch rather than
  touching the model directly.
- The `App` layer owns the three pieces of per-pane state that FR3 enumerates
  and performs the actual release inside the single existing `closed_panes`
  loop.
- The render layer is untouched; its only observable change is that it stops
  finding stale entries.

### Data Flow

```
Detached frame
  → Tab::handle_detached
      → queue group.pane_ids() into Tab::pending_closed_agent_status_panes
      → clear Tab::mux_group
  → App::pump_all
      → Tab::take_closed_agent_status_panes
      → App::agent_status_closed_panes
      → App::apply_agent_status_batch  (closed_panes loop)
          → resolve agent_notification_rate_limit_key   (mapping still present)
          → AgentStatusModel::discard(PaneKey::MuxPane(scope, pane_id))
          → remove mux_public_pane_ids[(scope, pane_id)]
          → remove notification rate-limit entry
```

### Key Derivation

Every key used by the teardown is derived from the detaching tab's own scope:

| State | Key |
|---|---|
| `AgentStatusModel::entries` (discarded) | `PaneKey::MuxPane(ConnectionScope(tab.stable_id), wire pane_id)` |
| `AgentStatusModel::entries` (retained) | `PaneKey::Tab(tab.stable_id)` |
| `App::mux_public_pane_ids` | `(ConnectionScope(tab.stable_id), wire pane_id)` |
| Notification rate-limit map | result of `agent_notification_rate_limit_key` |

`ConnectionScope` keeps the `ConnectionScope(tab.stable_id)` derivation at every
site (FR4); no attach-generation counter is introduced. Because every key
carries the detaching tab's own stable id, a detach cannot reach another tab's
same-numbered wire pane (NFR3).

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/tabs/mux_link.rs`: `Tab::handle_detached` is the single site
  that gains the queueing step (FR1, FR2).
- `src-tauri/src/tabs/mod.rs`: `Tab::pending_closed_agent_status_panes` (:421)
  and `Tab::take_closed_agent_status_panes` (:858-860) — the existing latch and
  drain used unchanged.
- `src-tauri/src/app/mod.rs`: `App::pump_all` /
  `App::agent_status_closed_panes` (:1108-1112), `App::mux_public_pane_ids`
  (:228-229), the reaped-tab loop (:1473-1490).
- `src-tauri/src/app/agent_status.rs`: `apply_agent_status_batch`'s
  `closed_panes` loop (:321-332), `agent_notification_rate_limit_key`
  (:98-113), `agent_status_badge_for` (:128-134), `agent_status_pane_badge`
  (:141-148), `agent_status_keys_for_tab` (:28).
- `src-tauri/src/agent_status_model.rs`: `AgentStatusModel::discard` (:264-269)
  and the `ConnectionScope` doc comment (:43-44).
- `src-tauri/src/app/tab_lifecycle.rs`: `App::close_tab` (:147-154), an existing
  path that already satisfies FR1.

**External Dependencies:**
- None. No new crate, and no change to `crates/mux_ipc` (NFR2).

### File Structure

```
src-tauri/src/
├── tabs/
│   ├── mux_link.rs              # handle_detached queues group.pane_ids()
│   ├── mod.rs                   # existing latch + drain (unchanged shape)
│   └── tests/mux_link.rs        # TS-5, TS-6
├── app/
│   ├── mod.rs                   # existing drain wiring
│   ├── agent_status.rs          # existing closed_panes teardown
│   └── tests/agent_status.rs    # TS-1, TS-2, TS-3, TS-4
└── agent_status_model.rs        # ConnectionScope doc comment correction
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-detach-agent-status-cleanup/**`
- `test-docs/mux-detach-agent-status-cleanup/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (AC-1, AC-3) — `src-tauri/src/app/tests/agent_status.rs`: Build
      an app with a mux group (the `app_with_mux_windows`-style construction
      used in `src-tauri/src/app/tests/mux_ui.rs:434` and the direct
      `MuxWindowGroup::seed` construction at
      `src-tauri/src/app/tests/agent_status.rs:1044-1054`), deliver an
      `AgentStatusUpdate` via `App::on_mux_message` + `App::pump_all`, assert
      the badge and `mux_public_pane_id` are populated, then deliver
      `MuxMessage { msg_type: MessageType::Detached, pane_id: 0, payload:
      vec![] }` and pump again; assert the badge is `None` and
      `mux_public_pane_id` is `None`.
- [ ] **TS-2** (AC-2, AC-6) — `src-tauri/src/app/tests/agent_status.rs`: Extend
      TS-1 with a second attach (a `Welcome` message, per `mux_welcome_message`
      at `src-tauri/src/app/tests/mux_ui.rs:434-457`) reusing the SAME wire pane
      id, and assert the badge stays empty until the new daemon's first
      `AgentStatusUpdate` — the direct regression guard for the reported repro.
- [ ] **TS-3** (AC-4) — `src-tauri/src/app/tests/agent_status.rs`: Set a
      plain-tab status on a mux-attached tab's own `PaneKey::Tab(stable_id)`
      key, detach, and assert that entry still reports its state after the pump.
- [ ] **TS-4** (AC-5) — `src-tauri/src/app/tests/agent_status.rs`: Two tabs,
      both groups seeded with wire pane id 1 (mirroring
      `src-tauri/src/app/tests/agent_status.rs:1043-1064`); detach tab 0 and
      assert tab 1's model entry, `mux_public_pane_id` and derived rate-limit
      key are unchanged.
- [ ] **TS-5** (AC-1) — `src-tauri/src/tabs/tests/mux_link.rs`: Tab-level test:
      a `Detached` frame applied to a tab with a seeded group makes
      `Tab::take_closed_agent_status_panes()` return the group's pane ids
      (mirroring the existing `PtyExited` assertion at
      `src-tauri/src/tabs/tests/mux_link.rs:157`), and a second call returns
      empty.
- [ ] **TS-6** (AC-1) — `src-tauri/src/tabs/tests/mux_link.rs`: A `PtyExited`
      sequence that empties the group still yields each pane id exactly once —
      the detach-side queueing must not double-push ids the `PtyExited` arm
      already queued.

### Integration Tests

No integration test beyond the `--lib` suite is required by
`requirements_analysis`. AC-7 requires the full `--lib` suite to pass,
including the existing mux-agent-status-pane-key-collision scoping tests (TS-5,
TS-6, TS-7 in `src-tauri/src/app/tests/agent_status.rs`) and the detach-driving
overlay tests in `src-tauri/src/app/tests/mux_ui.rs`.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] Detach in the same pump as a `PtyExited` for one of the same panes: the
      pane id can appear twice in the drained list; the second discard must be
      an idempotent no-op.
- [ ] Detach for a pane that never received an `AgentStatusUpdate`: no
      `mux_public_pane_ids` entry exists, so the rate-limit key falls back to
      `mux:<scope>:<pane_id>`; the discard must be a safe no-op.
- [ ] Detach on a background (non-active) tab: teardown must run for it too,
      since `pump_all` drains every tab's latches.
- [ ] Detach while the group holds multiple windows: every pane in the group is
      torn down, not just the active one.

### Performance Tests

No load or stress test is required. NFR1's bound (state bounded by currently
live panes rather than growing once per detach cycle) is asserted structurally
by TS-1 through TS-4 rather than by a performance harness.

## Security Considerations

`requirements_analysis` states no security requirement for this feature. The
change is confined to GUI-side in-process state lifecycle; it adds no input
parsing, no new external surface, and no change to the mux wire protocol
(NFR2).

## Error Handling

The feature introduces no new error code or error surface. The failure modes it
must tolerate are the idempotency cases listed under Edge Cases: a repeated
discard of the same pane id, and a discard of a pane that has no
`mux_public_pane_ids` entry. Both are no-ops rather than errors.

## Performance Optimization

### Performance Goals

- NFR1: `AgentStatusModel::entries`, `App::mux_public_pane_ids` and the
  notification rate-limit map stay bounded by the currently-live panes across
  repeated detach/re-attach cycles on one tab.

### Optimization Strategies

- Release at detach rather than at process exit, reusing the existing
  `closed_panes` loop (FR2) so no additional traversal is added to the pump.

## Design Step

Skipped. Resolved via gate `create-spec.design-step`, option
`decide_autonomously` — the analyst's own recommendation to skip was accepted.
The feature is an internal state-lifecycle bug fix with no new or changed
user-facing surface: no new widget, no layout change, no new colour/typography/
spacing decision, and no design-token consumption. The only visible effect is
that an existing badge and sidebar entry stop showing stale data, rendered by
the unchanged `render/mod.rs` / `ui::tab_bar` / `ui::mux_sidebar` path.

## Assumptions

- **A-1** (`answers[fr.detach-paths]`, batch-codex-consultation): The
  requirement covers every transition out of mux mode that clears `mux_group` —
  confirmed detach, bridge death, connection loss, PTY close — not only the
  daemon-confirmed `Detached` frame. Recorded as an assumption because it was
  resolved by batch policy rather than by the user. Investigation confirms all
  paths other than `handle_detached` already satisfy it.
- **A-2** (`answers[fr.scope-generation]`, batch-codex-consultation): Fix option
  (a) only: queue the group's pane ids at detach and reuse the existing
  `closed_panes` teardown. `ConnectionScope` stays
  `ConnectionScope(tab.stable_id)`; no attach-generation counter is introduced,
  and its doc comment is corrected to match implemented behaviour. Recorded as
  an assumption per batch policy.
- **A-3** (`answers[ec.plain-tab-entry]`, batch-codex-consultation): Detach
  discards `PaneKey::MuxPane` entries only; the tab's own
  `PaneKey::Tab(stable_id)` entry and its inferred-clear latch survive. Recorded
  as an assumption per batch policy.
- **A-4** (investigation): The repro's cross-host symptom ("re-attach to a
  different host shows the old host's state") and the single-tab
  detach->re-attach symptom share one root cause and one fix; no separate
  cross-host handling is required.
- **A-5** (investigation): `MuxWindowGroup::pane_ids()` is available on the
  group at the point `handle_detached` runs (it is already used by
  `agent_status_keys_for_tab` at `src-tauri/src/app/agent_status.rs:28` and by
  `src-tauri/src/app/mux_ui.rs:503`), so no new accessor is needed.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested
- [ ] All test scenarios (TS-1 through TS-6) pass
- [ ] All acceptance criteria (AC-1 through AC-7) are satisfied
- [ ] Non-functional requirements NFR1-NFR5 hold
- [ ] AC-7: the full `--lib` suite passes, including the existing
      mux-agent-status-pane-key-collision scoping tests and the detach-driving
      overlay tests in `src-tauri/src/app/tests/mux_ui.rs`
- [ ] AC-6: the `ConnectionScope` doc comment matches implemented behaviour

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional requirement is `status: resolved`.

## References

- Requirements document (Japanese):
  `feature-docs/mux-detach-agent-status-cleanup/REQUIREMENTS.md`
- Prior scoping work: mux-agent-status-pane-key-collision (its scoping tests
  TS-5, TS-6, TS-7 live in `src-tauri/src/app/tests/agent_status.rs`)
