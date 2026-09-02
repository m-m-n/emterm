---
title: "mux-agent-status-pane-key-collision"
created_date: 2026-09-02
status: draft
---

# mux-agent-status-pane-key-collision - Requirements Document

## 1. Overview

### 1.1 Background

The GUI keys mux agent-status state by the daemon-local wire `pane_id` alone
(`PaneKey::MuxPane(u32)`, `src-tauri/src/agent_status_model.rs:32-40`). When one
eMterm process is attached to two different mux daemons at once, both daemons
number their panes from their own local counter, so two unrelated panes collide
on the same key.

The reported repro: tab 1 is attached to server 1's daemon and tab 2 to server
2's daemon, both sitting on their daemon's window 1 (wire `pane_id` 1). Driving
Claude Code on tab 1 through working / done also moves tab 2's agent-status
badge, because both tabs read the same model entry.

### 1.2 Purpose

- Agent-status badges stay truthful when one eMterm process is attached to
  several mux daemons at once: a pane's state reaches only the tab whose own mux
  connection reported it.
- Remove every consequence of the shared wire-`pane_id` key at once (badge
  aggregation, public-pane-id map, notification rate limiting, notification
  tab-title resolution, pane-close discard) so no residual cross-daemon bleed is
  left behind.
- Fix entirely inside the GUI process: no mux wire-protocol change, so an
  unchanged daemon (and an older daemon) keeps working.

### 1.3 Scope

**In scope**: the GUI-side key derivation for mux agent status and every path
that consumes it — `AgentStatusModel`'s mux key (FR1), badge/aggregation reads
(FR2), the `pane_id -> public_pane_id` map (FR3), the notification rate-limit
key (FR4), notification tab-title resolution and pane visibility (FR5), and
pane-close discard (FR6).

**Out of scope**: `crates/mux_ipc` and the daemon-side pane-id allocator (NFR1);
the plain-tab (`PaneKey::Tab`) path, which must behave identically (NFR3); badge
rendering, the mux sidebar and the status bar, which consume only the already
defined aggregated value.

**Design step**: skipped. This is a defect fix in internal Rust state-keying
with no user-visible interface change — no new screen, dialog, setting,
keybinding or layout, and no design token consumption.

## 2. Business Requirements

### 2.1 Business Goals

See 1.2. The three goals are, in short: truthful per-connection badges, a
complete removal of every consequence of the shared key, and a GUI-only fix that
keeps unchanged and older daemons working.

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| eMterm user attached to several mux daemons at once | Runs one eMterm process with tabs attached to two or more mux daemons (e.g. two servers) and reads agent-status badges per tab |
| eMterm user with a single mux daemon | Sees no behavioral change at all (see 6) |

### 2.3 Expected Effects

- Each tab's badge, each mux-sidebar entry's badge, the `next-agent-window`
  cycle, and each notification's tab title refer to the pane on that tab's own
  server.
- Four latent cross-daemon defects behind the same key (public-pane-id map,
  rate-limit key, tab-title resolution, close discard) are removed together with
  the reported badge symptom.
- A fixed GUI works against an unmodified — and an older — daemon.

## 3. Use Cases

### 3.1 Use Case List

| ID | Use case | Actor | Priority |
|----|----------|-------|----------|
| UC01 | Read agent-status badges with two tabs attached to two daemons | eMterm user attached to several mux daemons | High |

### 3.2 Use Case Detail

#### UC01: Read agent-status badges with two tabs attached to two daemons

**Actor**: eMterm user attached to several mux daemons at once.

**Preconditions**:
- Tab 1 is attached to server 1's mux daemon, tab 2 to server 2's mux daemon.
- Both tabs sit on their own daemon's window 1, i.e. both hold wire `pane_id` 1.

**Basic flow**:
1. The user drives Claude Code on tab 1, which transitions working then done.
2. Server 1's daemon sends `AgentStatusUpdate` for its pane 1 over tab 1's mux
   connection.
3. Tab 1's badge changes to reflect working, then done.
4. Tab 2's badge continues to reflect only its own pane's state.

**Alternative flow**:
- A transition raises a notification: the notification body names the tab whose
  connection actually reported the transition (see FR5).

**Postconditions**:
- Each tab's badge reflects only the panes reported over that tab's own mux
  connection (AC-1).

## 4. Functional Requirements

### 4.1 Functional Requirement List

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| FR1 | Scope the mux agent-status key by GUI-local mux connection identity | resolved | High |
| FR2 | Badge aggregation reads only the querying tab's own scope | resolved | High |
| FR3 | Connection-scope the `pane_id -> public_pane_id` map | resolved | High |
| FR4 | Notification rate-limit key stays collision-free across daemons | resolved | High |
| FR5 | Notification tab-title resolution picks the owning tab, not the first match | resolved | High |
| FR6 | Pane-close discard removes only the closing daemon's entry | resolved | High |

### 4.2 Functional Requirement Detail

#### FR1: Scope the mux agent-status key by GUI-local mux connection identity

`AgentStatusModel`'s mux key MUST identify a pane by (GUI-local mux connection
identity, wire `pane_id`) instead of the wire `pane_id` alone — i.e.
`PaneKey::MuxPane(u32)` at `src-tauri/src/agent_status_model.rs:32-40` gains a
connection-scope component.

The concrete GUI-side value that serves as that connection identity is
`Tab::stable_id` (`src-tauri/src/tabs/mod.rs:249`, minted from the
process-lifetime `NEXT_TAB_STABLE_ID` counter at `src-tauri/src/tabs/mod.rs:43`):
a mux-attached tab owns exactly one mux connection — its own PTY runs
`emterm mux attach`, and every mux frame is extracted from that tab's stream and
routed through `Tab::apply_mux_message` (`src-tauri/src/tabs/mux_link.rs:56`).
There is no other connection/client/session object on the GUI side;
`Tab::mux_session_name` and `MuxWindowGroup` (`src-tauri/src/mux/window_group.rs:39`)
carry no daemon-distinguishing identity. Every attached pane therefore has a
scope from attach time onward, including a pane that has never reported an agent
status, and the scope is available without any `mux_ipc` change.

Enabling this requires restoring per-tab attribution at the drain: `App::pump_all`
currently flattens `tab.take_pending_agent_status_updates()` and
`tab.take_closed_agent_status_panes()` into untagged vectors
(`src-tauri/src/app/mod.rs:1091-1092`), unlike the plain-tab events and latch
inputs which are already tagged with `tab_stable_id` at
`src-tauri/src/app/mod.rs:1086` / `:1089`; both mux drains MUST carry the
originating tab's `stable_id` into `App::apply_agent_status_batch`
(`src-tauri/src/app/agent_status.rs:235`).

#### FR2: Badge aggregation reads only the querying tab's own scope

Every badge/aggregation read path MUST query connection-scoped keys so a tab's
badge can never include another tab's panes:

- `agent_status_keys_for_tab` (`src-tauri/src/app/agent_status.rs:15`) builds
  `PaneKey::MuxPane` from `group.pane_ids()` and MUST pair each with the owning
  tab's scope.
- `App::agent_status_badge_for` (`src-tauri/src/app/agent_status.rs:110`,
  consumed at `src-tauri/src/render/mod.rs:267`) MUST aggregate only that scoped
  set.
- `App::agent_status_pane_badge` (`src-tauri/src/app/agent_status.rs:120`,
  consumed for mux-sidebar entries at `src-tauri/src/render/mod.rs:311`) MUST
  take the sidebar's owning tab scope alongside the wire `pane_id`.
- `AgentStatusModel::any_pane_has_reported_state`
  (`src-tauri/src/agent_status_model.rs:315`, called by the `next-agent-window`
  action at `src-tauri/src/app/mux_ui.rs:501`) MUST match within the acting
  tab's scope only.

#### FR3: Connection-scope the `pane_id -> public_pane_id` map

`App::mux_public_pane_ids: HashMap<u32, String>` (`src-tauri/src/app/mod.rs:225`)
MUST be keyed by the same connection-scoped identity as FR1, so a learn/refresh
from one daemon's `AgentStatusUpdate` (`src-tauri/src/app/agent_status.rs:276-277`)
can no longer overwrite another daemon's entry for the same wire `pane_id`. The
`App::mux_public_pane_id` accessor (`src-tauri/src/app/agent_status.rs:132`) MUST
be scoped in the same way, and the removal at
`src-tauri/src/app/agent_status.rs:291` MUST remove only the scoped entry.

#### FR4: Notification rate-limit key stays collision-free across daemons

`agent_notification_rate_limit_key` (`src-tauri/src/app/agent_status.rs:83`) MUST
derive the per-pane rate-limit key from the FR3 scoped map, so the daemon-minted
`public_pane_id` (`"{incarnation}-{pane_id}"`,
`crates/mux_ipc/src/protocol.rs:961-978`) it prefers is always the one belonging
to the pane's own daemon. Its `"mux:{pane_id}"` fallback (used when a pane is
discarded before its public id was ever learned) MUST also include the connection
scope so two daemons' pane 1 do not share a fallback key. Its four call sites
(`src-tauri/src/app/agent_status.rs:290` and `:321`,
`src-tauri/src/app/tab_lifecycle.rs:143`, `src-tauri/src/app/mod.rs:1460`) MUST
all derive the same key.

#### FR5: Notification tab-title resolution picks the owning tab, not the first match

`agent_status_pane_tab_title` (`src-tauri/src/app/agent_status.rs:54`) currently
returns the first tab whose `mux_group.pane_ids()` contains the wire `pane_id`,
which with two daemons can name the wrong tab in the notification body. It MUST
resolve the tab by the transition's connection scope, so the notification names
the tab whose connection actually reported the transition.

The same rule applies to `agent_status_pane_visible`
(`src-tauri/src/app/agent_status.rs:35`), which decides visibility by membership
in `agent_status_keys_for_tab(active_tab)` and would otherwise treat another
daemon's pane as visible whenever the active tab happens to hold the same wire
`pane_id`.

#### FR6: Pane-close discard removes only the closing daemon's entry

Discarding on pane/tab close MUST be scoped so a pane exiting on one daemon never
drops another daemon's same-`pane_id` state: the `closed_panes` loop in
`App::apply_agent_status_batch` (`src-tauri/src/app/agent_status.rs:286-294`, fed
from `Tab::pending_closed_agent_status_panes` latched at
`src-tauri/src/tabs/mux_link.rs:821`) MUST discard the scoped `PaneKey`, the
scoped `mux_public_pane_ids` entry, and the scoped rate-limit key only.

The two whole-tab discard paths — `App::close_tab`
(`src-tauri/src/app/tab_lifecycle.rs:142-146`) and the reaped-exited-tab loop
(`src-tauri/src/app/mod.rs:1453-1463`) — already iterate
`agent_status_keys_for_tab`, so they inherit the scope from FR2 and MUST NOT
discard any key outside the closing tab's own scope.

## 5. Non-Functional Requirements

### 5.1 Performance (NFR2 - Per-frame read paths stay cheap)

`agent_status_badge_for` and `agent_status_pane_badge` run once per tab / per
sidebar entry on every rendered frame (`src-tauri/src/render/mod.rs:267`,
`:311`), and `next-agent-window` resolves its qualify list at key-event time with
no polling. The scoped key MUST keep these as O(1) hash lookups — no per-frame
scan across all tabs and no new per-frame allocation beyond what
`agent_status_keys_for_tab` already does.

### 5.2 Security

- The daemon-minted `public_pane_id` remains an opaque string handled only as a
  map value and rate-limit key; no new parsing of it is introduced on the GUI
  side, so `PublicPaneId::parse`'s malformed-input surface is not newly
  exercised.
- The connection scope is a process-local counter value, never transmitted over
  the mux wire and never rendered to the user, so it introduces no new
  information exposure between servers.

### 5.3 Maintainability (NFR4 - Documented invariants stay accurate)

The doc comments that pin the current semantics — `PaneKey::MuxPane`'s "keyed by
the wire `pane_id`" note (`src-tauri/src/agent_status_model.rs:36-39`) and
`agent_notification_rate_limit_key`'s "unique across concurrent panes" claim
(`src-tauri/src/app/agent_status.rs:71-82`) — MUST be updated to state the new
scoping rule rather than left describing the buggy contract.

### 5.4 Compatibility

- **NFR1 - No mux wire-protocol change**: the fix stays inside the GUI process.
  `crates/mux_ipc` (including `AgentStatusUpdateMsg` at
  `crates/mux_ipc/src/protocol.rs:885` and `PublicPaneId` at `:967`) and the
  daemon-side pane-id allocator (`src-tauri/src/mux/session/manager.rs:76`) are
  unchanged, so an unmodified — and an older — daemon keeps working with a fixed
  GUI.
- **NFR3 - Plain-tab behavior unchanged**: `PaneKey::Tab(u64)` and the whole
  plain-tab path (OSC 777 parsing, the inferred-clear latch, revision minting,
  `AgentStatusModel::discard`'s latch cleanup) MUST behave identically before and
  after the change; only the mux key gains a scope.

## 6. UI/UX Requirements

- No visible UI change in the single-daemon case: badge glyphs, ranking
  (blocked > unseen done > working > seen done > idle), sidebar entries and
  notification bodies are all unchanged.
- In the multi-daemon case the only user-visible change is correctness: each
  tab's badge, each mux-sidebar entry's badge, the `next-agent-window` cycle, and
  each notification's tab title now refer to the pane on that tab's own server.
- No new setting, no new keybinding, no new dialog.

## 7. Data Requirements

### 7.1 Keyed State Overview

| State | Current key | Required key |
|-------|-------------|--------------|
| `AgentStatusModel` mux entry (`PaneKey::MuxPane`) | wire `pane_id` (`u32`) | (GUI-local connection scope, wire `pane_id`) — FR1 |
| `App::mux_public_pane_ids` | wire `pane_id` (`u32`) | the same connection-scoped identity — FR3 |
| Notification rate-limit key | `public_pane_id`, or `"mux:{pane_id}"` fallback | scope-derived `public_pane_id`, or a scope-including fallback — FR4 |
| `AgentStatusModel` plain-tab entry (`PaneKey::Tab`) | `Tab::stable_id` (`u64`) | unchanged — NFR3 |

### 7.2 Identity Sources

| Item | Source |
|------|--------|
| GUI-local connection scope | `Tab::stable_id` (`src-tauri/src/tabs/mod.rs:249`), minted from `NEXT_TAB_STABLE_ID` (`src-tauri/src/tabs/mod.rs:43`) |
| Wire `pane_id` | daemon-local allocator (`src-tauri/src/mux/session/manager.rs:76`), delivered in `AgentStatusUpdateMsg` |
| `public_pane_id` | daemon-minted `"{incarnation}-{pane_id}"` (`crates/mux_ipc/src/protocol.rs:961-978`) |

## 8. External Integration

### 8.1 Integrated Systems

| System | Integration | Data |
|--------|-------------|------|
| mux daemon | mux wire protocol via `crates/mux_ipc`, over the tab's own `emterm mux attach` PTY | `AgentStatusUpdateMsg`, `PublicPaneId` |

### 8.2 API Requirements

None. The wire types and the `PublicPaneId` format are unchanged (NFR1, AC-8);
the connection scope is never transmitted.

## 9. Constraints

### 9.1 Technical Constraints

- No `mux_ipc` wire-shape change; no daemon-side change (NFR1).
- The scope must come from an existing GUI-side identity, because every attached
  pane needs a scope from attach time — including panes that have never reported
  an agent status (A1).
- Per-frame read paths must remain O(1) hash lookups (NFR2).
- The plain-tab path must be bit-for-bit equivalent in behavior (NFR3).

### 9.2 Business Constraints

- The fix must cover all five consequences of the shared key, not only the
  reported badge symptom (A2).

### 9.3 Schedule Constraints

None recorded.

### 9.4 Declared Change Set

Feature-specific paths are not enumerated by hand here; they are derived at
create-plan from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):

- `feature-docs/mux-agent-status-pane-key-collision/**`
- `test-docs/mux-agent-status-pane-key-collision/**`

**Semantics**: the declaration is a SUPERSET assertion — the actual change set
must be CONTAINED IN the declared set. A declared path that never materializes is
not a violation.

## 10. Issues and Risks

### 10.1 Edge Cases

| ID | Edge case | Handling |
|----|-----------|----------|
| EC-1 | Two tabs attached to the SAME daemon session and the same pane become two distinct model entries under a per-tab scope | Duplication, not contamination: each tab's badge still tracks that pane correctly, because the daemon pushes `AgentStatusUpdate` to each owning connection independently. The model's global `counts()` would count the pane twice; `counts()` has no non-test caller today. |
| EC-2 | Detach then re-attach in the SAME tab keeps the same `Tab::stable_id`, so the tab's scope is unchanged across the re-attach | `handle_detached` (`src-tauri/src/tabs/mux_link.rs:872-912`) clears `mux_group` without latching its panes into `pending_closed_agent_status_panes`, so that tab's mux entries are never discarded on detach (pre-existing behavior — after the clear, `agent_status_keys_for_tab` no longer yields them, so even tab close cannot reach them). With the scoped key, a re-attach that reuses wire `pane_id` 1 in that same tab can therefore surface the pre-detach state as the new pane's badge until the daemon's first update arrives. Whether to latch the pane ids at detach is a decision for the plan step; it does not change any FR above. |
| EC-3 | A mux tab whose daemon predates the window-list/`AgentStatusUpdate` protocol never installs a `mux_group` | It contributes no `PaneKey::MuxPane` keys at all; the scoped key must leave that path as a plain tab exactly as today. |
| EC-4 | A transition drained for a pane whose tab closed between the transition firing and the drain | `agent_status_pane_tab_title` returns `None` and the caller falls back to an empty title. Scoped resolution must keep returning `None` here rather than falling back to some other tab that happens to hold the same wire `pane_id`. |
| EC-5 | The daemon's `incarnation` token changes on every daemon restart (`src-tauri/src/mux/session/manager.rs:58-68`), so reconnecting to a restarted daemon on the same host yields new `public_pane_id`s for reused wire `pane_id`s | The GUI-local scope is independent of that, so this affects only the FR3/FR4 refresh path, which already overwrites on each update. |

### 10.2 Technical Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| The key change reaches five consumers, so a partially applied change leaves inconsistent scoping | High | FR3-FR6 are in scope precisely because the key change must reach `mux_public_pane_ids`, the rate-limit derivation, the tab-title resolution and the discard loops to compile and stay consistent (A2) |
| Scoping the per-frame read paths adds cost | Medium | NFR2 constrains them to O(1) hash lookups with no new per-frame allocation |
| Plain-tab regressions from the shared model change | Medium | NFR3 plus TS-10: existing plain-tab tests must pass with their original expectations |

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] **AC-1**: Following the reported repro (tab 1 attached to server 1's
  daemon, tab 2 attached to server 2's daemon, both sitting on their daemon's
  window 1 = wire `pane_id` 1), driving Claude Code on tab 1 through working /
  done changes tab 1's badge only; tab 2's badge continues to reflect only its
  own pane.
- [ ] **AC-2**: Two panes that share a wire `pane_id` but differ in connection
  scope occupy two independent entries in `AgentStatusModel`: applying a daemon
  update to one leaves the other's state, name, revision and unseen flag
  untouched.
- [ ] **AC-3**: Learning `public_pane_id` for scope A's pane 1 does not overwrite
  or remove scope B's `public_pane_id` for its own pane 1; each scope's
  rate-limit key resolves to its own daemon's `"{incarnation}-{pane_id}"` string,
  and the two keys differ.
- [ ] **AC-4**: A notification raised for a transition on scope A's pane carries
  scope A's tab title, even when another tab holds a pane with the same wire
  `pane_id`.
- [ ] **AC-5**: Closing (PtyExited) scope A's pane 1 discards scope A's model
  entry, its `mux_public_pane_ids` entry, and its rate-limit bookkeeping, and
  leaves scope B's pane-1 entries fully intact.
- [ ] **AC-6**: Plain-tab (`PaneKey::Tab`) status handling — set/clear, unseen
  flag, revision minting, latch-driven inferred clear, discard-on-close — is
  unchanged; the existing plain-tab tests pass without modification of their
  expectations.
- [ ] **AC-7**: A regression test exists that fails on the pre-fix code and
  passes after: it drives two panes sharing a wire `pane_id` under different
  connection scopes and asserts no cross-scope bleed.
- [ ] **AC-8**: `crates/mux_ipc` is unmodified: the wire types and the
  `PublicPaneId` format are byte-for-byte the same as before the change.

### 11.2 KPI

Not defined for this defect fix.

## 12. Test Scenarios

All scenarios are unit level; regression coverage is inline `#[cfg(test)]` unit
tests over `AgentStatusModel` and the `src-tauri/src/app/agent_status.rs`
helpers. No two-daemon integration test is added (A3).

| ID | Location | Scenario | Requirements |
|----|----------|----------|--------------|
| TS-1 | `src-tauri/src/agent_status_model/tests.rs` (inline `#[cfg(test)]`) | Two connection scopes, same wire `pane_id`: apply a daemon update (Working) under scope A and none under scope B; `aggregate` over scope A's key set yields Working, over scope B's yields None. Then apply Blocked under scope B and assert scope A still reads Working. Covers AC-2. | FR1 |
| TS-2 | `src-tauri/src/agent_status_model/tests.rs` | `discard` of scope A's (pane_id 1) key leaves scope B's (pane_id 1) entry present and unchanged. Covers AC-5's model half. | FR6 |
| TS-3 | `src-tauri/src/agent_status_model/tests.rs` | `any_pane_has_reported_state` is scope-sensitive: a reported state under scope A does not make scope B's same-numbered pane qualify for the `next-agent-window` cycle. Covers FR2's cycle path. | FR2 |
| TS-4 | `src-tauri/src/app/tests/agent_status.rs` | `agent_status_keys_for_tab` over two mux-attached tab fixtures whose `MuxWindowGroup`s hold identical `pane_ids()` produces two disjoint key sets (no key appears in both). Covers FR2. | FR1, FR2 |
| TS-5 | `src-tauri/src/app/tests/agent_status.rs` | `agent_notification_rate_limit_key` returns different keys for scope A pane 1 and scope B pane 1, both when each scope has learned its own `public_pane_id` ("aaa-1" vs "bbb-1") and on the never-learned fallback path. Covers AC-3 / FR4. | FR4 |
| TS-6 | `src-tauri/src/app/tests/agent_status.rs` | `agent_status_pane_tab_title` over two tabs whose groups both contain wire `pane_id` 1 returns each tab's own title for its own scoped key (not the first-match title for both). A companion assertion covers `agent_status_pane_visible`: a non-active tab's pane is not reported visible merely because the active tab holds the same wire `pane_id`. Covers AC-4 / FR5. | FR5 |
| TS-7 | `src-tauri/src/app/tests/agent_status.rs` | `App::apply_agent_status_batch` fed updates tagged for two different tab scopes but carrying the same `update.pane_id` (and different `public_pane_id`s) stores both `mux_public_pane_ids` entries; querying each scope's public id returns its own daemon's string. Covers AC-3 / FR3. | FR3 |
| TS-8 | `src-tauri/src/app/tests/agent_status.rs` | `App::apply_agent_status_batch` with a `closed_panes` entry tagged for scope A and wire `pane_id` 1 discards scope A's model entry, `mux_public_pane_ids` entry and rate-limit bookkeeping, while scope B's pane-1 entries all survive. Covers AC-5 end to end within the batch. This is the primary AC-7 regression test. | FR6, FR3, FR4 |
| TS-9 | `src-tauri/src/app/tests/agent_status.rs` | `App::agent_status_badge_for` returns the correct, non-shared badge for each of two mux-attached tab fixtures whose groups hold the same `pane_ids()` but different reported states — the direct unit-level analogue of the reported symptom. Covers AC-1's testable core. | FR2 |
| TS-10 | `src-tauri/src/agent_status_model/tests.rs` and `src-tauri/src/app/tests/agent_status.rs` | Existing plain-tab tests (set/clear, unseen preservation, replay_derived silence, latch-driven inferred clear, discard-on-close) still pass with their original expectations, confirming `PaneKey::Tab` semantics are untouched. Covers AC-6 / NFR3. | NFR3 |

**Commands**:

- Test: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only check: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Format: `make fmt`

## 13. Glossary

| Term | Definition |
|------|------------|
| Wire `pane_id` | The daemon-local pane number carried on the mux wire, allocated by `src-tauri/src/mux/session/manager.rs:76`. Not unique across daemons. |
| Connection scope | The GUI-local identity of a mux connection; concretely `Tab::stable_id`, since a mux-attached tab owns exactly one mux connection. |
| `public_pane_id` | The daemon-minted `"{incarnation}-{pane_id}"` string (`crates/mux_ipc/src/protocol.rs:961-978`). |
| `incarnation` | A daemon-side token that changes on every daemon restart (`src-tauri/src/mux/session/manager.rs:58-68`). |
| `PaneKey` | `AgentStatusModel`'s key enum: `Tab(u64)` for plain tabs, `MuxPane(u32)` for mux panes today (`src-tauri/src/agent_status_model.rs:32-40`). |

## 14. Confirmations

### 14.1 Confirmed Facts

- [x] **A1 — Pane identity source** (answer to `requirement.pane-identity-source`,
  option `gui_local_connection_scope`; batch-codex-consultation): The
  agent-status state is keyed on a GUI-local mux connection scope paired with the
  wire `pane_id`, not on a new daemon-supplied identifier; no `mux_ipc`
  wire-shape change is made. Rationale: every attached pane has a connection
  scope from attach time, including panes that have never reported an agent
  status — so badge, map, rate-limit and discard paths are all keyable
  immediately, without waiting for the daemon's first `AgentStatusUpdate` to
  deliver a `public_pane_id`. Investigation confirms the scope exists today as
  `Tab::stable_id`: a mux-attached tab owns exactly one mux connection (its own
  PTY runs `emterm mux attach`, and all mux frames arrive through
  `Tab::apply_mux_message`), and `Tab::stable_id` is already the identity used for
  `PaneKey::Tab` and for tagging plain events at the `pump_all` drain. Keeping the
  daemon untouched also means an unchanged (or older) daemon works with a fixed
  GUI.
- [x] **A2 — Fix scope** (answer to `requirement.fix-scope`, option
  `all_root_effects`; batch-codex-consultation): The fix covers every consequence
  of the shared key — badge aggregation, the `pane_id -> public_pane_id` map, the
  notification rate-limit key, notification tab-title resolution, and pane-close
  discard — rather than only the reported badge symptom. Rationale: all five
  effects share one root cause (the `u32`-only key). Fixing only the badge would
  leave four latent cross-daemon defects behind the same key change, and the key
  change itself must reach `mux_public_pane_ids`, the rate-limit derivation, the
  tab-title resolution and the discard loops anyway to compile and stay
  consistent. FR3-FR6 are therefore in scope.
- [x] **A3 — Regression test level** (answer to `testing.regression-test-level`,
  option `unit_only`; batch-codex-consultation): Regression coverage is inline
  `#[cfg(test)]` unit tests over `AgentStatusModel` and the
  `src-tauri/src/app/agent_status.rs` helpers, driving two panes that share a wire
  `pane_id` but differ in connection scope. No two-daemon integration test is
  added. Rationale: the defect is entirely GUI-side key derivation; the collision
  is fully reproducible in-process by constructing two scopes with the same wire
  `pane_id`, so a two-daemon integration test would add real-process/real-PTY cost
  without covering anything the unit tests miss. This also matches the repo's
  stated convention (`test/README.md`: unit tests inline next to the code,
  `src-tauri/tests/` only when a separate compilation unit is genuinely needed)
  and the existing coverage location for this subsystem.

### 14.2 Open / Deferred Items

- No requirement is `tbd`; FR1-FR6 and NFR1-NFR4 are all resolved.
- [ ] Whether to latch the pane ids at detach (EC-2) is a decision for the plan
  step; it does not change any requirement above.

## 15. References

- `SPEC.md` (this feature, English implementation-facing rendering)
- `src-tauri/src/agent_status_model.rs` — `PaneKey`, `AgentStatusModel`
- `src-tauri/src/app/agent_status.rs` — key derivation, badge reads, rate-limit
  key, tab-title resolution, batch apply
- `src-tauri/src/app/mod.rs` — `pump_all` drain, `mux_public_pane_ids`, reaped
  exited-tab loop
- `src-tauri/src/app/tab_lifecycle.rs` — `close_tab` discard path
- `src-tauri/src/tabs/mux_link.rs` — mux frame routing, close latch, detach
- `src-tauri/src/mux/window_group.rs` — `MuxWindowGroup`
- `src-tauri/src/render/mod.rs` — per-frame badge consumers
- `crates/mux_ipc/src/protocol.rs` — `AgentStatusUpdateMsg`, `PublicPaneId`
- `src-tauri/src/mux/session/manager.rs` — daemon pane-id allocator, incarnation
