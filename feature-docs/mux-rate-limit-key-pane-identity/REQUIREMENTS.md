---
title: "mux-rate-limit-key-pane-identity"
created_date: 2026-09-09
status: draft
---

# mux-rate-limit-key-pane-identity - Requirements Document

## 1. Overview

### 1.1 Background

The mux daemon sits across a trust boundary. The agent-notification rate-limit
key for a mux pane currently embeds a daemon-supplied `public_pane_id`, so a
hostile or buggy daemon can (a) alias two panes onto one throttle bucket (most
easily by sending an empty string), (b) mint a fresh bucket per update by
rotating the id, and (c) strand unreachable entries in an unpruned `HashMap`.

Origin: PR [https://github.com/m-m-n/emterm/pull/55](https://github.com/m-m-n/emterm/pull/55)
review round 1, medium findings `0810c3130449c0dc` (comprehensive) and
`0247047963c1c329` (security).

### 1.2 Purpose

Derive a mux pane's rate-limit identity only from code-owned values, and bound
the rate limiter's live-entry count over a long-running session.

### 1.3 Scope

In scope:

- The `agent_notification_rate_limit_key` derivation and its four production
  call sites.
- Expiry eviction inside `AgentNotificationRateLimiter::record`.
- The doc comments that describe the derivation and the limiter field.
- The app-layer and `notifications.rs` tests that pin both.

Out of scope (recorded as follow-up candidates only):

- Keying `AgentNotificationRateLimiter` by `PaneKey` instead of a formatted
  `String`.
- A hard entry cap and a global token bucket against adversarial bursts.
- Removal of `App::mux_public_pane_ids` or `App::mux_public_pane_id`.

## 2. Business Requirements

### 2.1 Business Objectives

- A mux pane's agent-notification rate-limit identity is derived only from
  code-owned values, so no daemon-supplied string can participate in throttling
  identity or be rotated to evade a cooldown.
- The rate limiter's live-entry count stays bounded over a long-running session,
  instead of growing with every pane that ever fired a notification.
- The existing test suite states what the code actually does, so the
  pane-identity contract is pinned rather than merely described.

### 2.2 Target Users

| User type | Description |
|----------------|------|
| eMterm user running mux panes | Receives agent notifications whose throttling must not be aliased, evaded, or starved by the daemon on the other side of the trust boundary. |
| eMterm maintainer | Reads the derivation's doc comments and tests as the statement of the pane-identity contract. |

### 2.3 Expected Effects

- A daemon that re-mints or rotates a `public_pane_id` cannot change the key its
  pane throttles under.
- A learned id byte-identical to another pane's or tab's key cannot reach that
  key.
- Rate-limiter entries that can no longer affect any decision are evicted.

## 3. Use Cases

No user-facing interaction changes. This feature alters an internal
key-derivation function, a rate limiter's eviction policy, and the tests pinning
them. No UI, no layout, no design token, no visual asset is touched, and the
notification body / badge rendering are unaffected.

The affected internal flows are the four production call sites of the derivation:

| ID | Flow | Site |
|----|------|------|
| UC01 | Closed-mux-pane loop | `app/agent_status.rs:339` |
| UC02 | Transition-drain loop | `app/agent_status.rs:370` |
| UC03 | `close_tab` | `app/tab_lifecycle.rs:148` |
| UC04 | Reaped-exited-tab loop | `app/mod.rs:1488` |

## 4. Functional Requirements

### 4.1 Functional Requirement List

| ID | Title | Status |
|----|--------|--------|
| FR1 | Mux pane keys are always the code-owned form | resolved |
| FR2 | The derivation stops consulting the learned-id map | resolved |
| FR3 | Rotation-evasion remediation | resolved |
| FR4 | The learned-id map itself is retained | resolved |
| FR5 | The derive-before-removal ordering constraint is retired | resolved |
| FR6 | Expiry eviction in the rate limiter | resolved |
| FR7 | Discard-on-close is kept alongside expiry | resolved |
| FR8 | Doc comments describing the three-form derivation are corrected | resolved |
| FR9 | Existing tests are updated to the option (a) shape | resolved |
| FR10 | The already-weak detach test is corrected | resolved |

### 4.2 Functional Requirement Details

#### FR1: Mux pane keys are always the code-owned form

`agent_notification_rate_limit_key` returns `mux:<scope>:<pane_id>` for every
`PaneKey::MuxPane`, unconditionally. The `muxpub:<scope>:<learned>` branch is
removed; `muxpub:` becomes a retired prefix that the code no longer emits
anywhere. `PaneKey::Tab(id)` keeps returning `tab:<id>` unchanged.

#### FR2: The derivation stops consulting the learned-id map

`agent_notification_rate_limit_key` no longer takes the
`mux_public_pane_ids: &HashMap<(ConnectionScope, u32), String>` parameter; its
only input is the `&PaneKey`. All four production call sites are updated to the
new signature: `app/agent_status.rs:339` (closed-mux-pane loop),
`app/agent_status.rs:370` (transition-drain loop), `app/tab_lifecycle.rs:148`
(`close_tab`), `app/mod.rs:1488` (reaped-exited-tab loop).

#### FR3: Rotation-evasion remediation

The chosen remediation is option (a) `always_code_owned_key`: because a live
pane's key is a pure function of `(ConnectionScope, wire pane_id)`, a daemon that
re-mints or rotates a `public_pane_id` cannot change the key its pane throttles
under, so a cooldown can no longer be escaped by supplying a new learned id.
Superseded-entry eviction (option (b)) is explicitly NOT adopted: evicting the
old entry removes history without transferring the cooldown timestamp, leaving
the new key immediately allowed.

#### FR4: The learned-id map itself is retained

`App::mux_public_pane_ids` and the public accessor
`App::mux_public_pane_id(scope, pane_id)` are kept unchanged - they serve
`ui::mux_sidebar`'s copy-to-clipboard row (task0006 AC-5), which is an
API-facing identifier concern, not a throttling one. Learning on
`AgentStatusUpdateMsg` (`app/agent_status.rs:321`) and removal on pane close /
tab close / tab reap (`app/agent_status.rs:340`, `app/tab_lifecycle.rs:150`,
`app/mod.rs:1490`) all stay.

#### FR5: The derive-before-removal ordering constraint is retired

Because the derivation no longer reads `mux_public_pane_ids`, the three discard
sites no longer need to derive the rate-limit key BEFORE removing the scoped map
entry (the CD-2 ordering of mux-agent-status-pane-key-collision FR4/FR6). The map
removal itself is kept; only the ordering obligation and every code comment
asserting it (`app/agent_status.rs:333-339`, `app/tab_lifecycle.rs:138-148`,
`app/mod.rs:1484-1488`) are corrected so the source stops documenting a
constraint that no longer exists.

#### FR6: Expiry eviction in the rate limiter

`AgentNotificationRateLimiter::record` additionally drops every entry whose
`last_fired` is older than `AGENT_NOTIFICATION_RATE_LIMIT` (30s) relative to the
`now` passed in, before/while inserting the new entry. `is_within_limit` keeps
its `&self` read-only signature and is not the pruning site, so a suppressed
attempt never refreshes or evicts a timestamp.

#### FR7: Discard-on-close is kept alongside expiry

`AgentNotificationRateLimiter::discard` and every
`App::discard_agent_notification_state` call site are retained unchanged. Expiry
eviction is additive: a pane key reused shortly after its pane closed must still
not inherit an unexpired cooldown, which only the close-time discard guarantees.

#### FR8: Doc comments describing the three-form derivation are corrected

The `agent_notification_rate_limit_key` doc comment
(`app/agent_status.rs:83-108`) is rewritten from three mutually disjoint forms to
two (`tab:` / `mux:`), and the `App::agent_notification_rate_limiter` field doc
(`app/mod.rs:434-443`) stops describing a daemon-learned id wrapped behind a
namespace prefix, stating instead that no daemon-supplied byte ever reaches the
key.

#### FR9: Existing tests are updated to the option (a) shape

Every app-layer test that asserts the `muxpub:` form or passes the learned-id map
is updated; the two tests whose entire premise (a daemon-controlled string
reaching the derivation) disappears are restated as guards that the derivation
cannot be influenced by any learned id at all, rather than deleted silently.

#### FR10: The already-weak detach test is corrected

`ac3_detach_releases_model_entry_public_id_and_rate_limit_identity`
(`src-tauri/src/app/tests/agent_status.rs:1540`) currently arms/probes the raw
string "xyz-7" (lines 1571, 1589) while production armed `muxpub:<scope>:xyz-7`,
so its rate-limit assertions passed vacuously on an unrelated key. It is
corrected to obtain the key from
`agent_notification_rate_limit_key(&PaneKey::MuxPane(scope, 7))` so it genuinely
exercises the identity it names.

## 5. Non-Functional Requirements

### 5.1 Performance Requirements

**NFR1 - Pruning is decision-neutral**: Expiry eviction uses exactly the same
threshold and comparison as `is_within_limit`
(`now.duration_since(prev) >= AGENT_NOTIFICATION_RATE_LIMIT`), so a pruned entry
is by construction one that would have answered `true` anyway. No observable
throttling decision changes as a result of pruning.

**NFR3 - Accepted limits of the chosen pruning shape**: Pruning is lazy (it runs
only inside `record`, so an entirely idle limiter keeps its entries until the
next fire) and `HashMap::remove` does not shrink capacity (so expiry bounds live
entries, not the allocation high-water mark). Both are accepted, not defects to
be worked around in this feature.

### 5.2 Security Requirements

- Input validation: no string the daemon supplies can appear in any rate-limit
  key. The derivation's only inputs are the `PaneKey`'s own scope and wire pane
  id, and the function does not accept the learned-id map at all - a compile-time
  property, not a runtime check (FR1, FR2, AC-3).
- Data protection: the daemon-learned `public_pane_id` is retained solely as an
  API-facing identifier for the sidebar's copy-to-clipboard row (FR4), never as
  throttling identity.

### 5.3 Availability Requirements

Not applicable - no service-level surface is affected.

### 5.4 Maintainability Requirements

- Documentation: the derivation's doc comment and the limiter field doc are
  corrected to state the two remaining key forms and the absence of any
  daemon-supplied byte (FR8).
- The source stops documenting the derive-before-removal ordering constraint,
  which no longer exists (FR5).

### 5.5 Compatibility Requirements

**NFR2 - No migration surface**: Rate-limit keys are process-local: never
serialized, never sent over the mux wire, never persisted to settings or
snapshot. Changing the key shape therefore requires no migration, no
compatibility window, and no daemon-side change.

**NFR4 - Build and test surface unchanged**: No new dependency, no change to the
public CLI surface, and no change to the `gui` / `--no-default-features` feature
split. `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
src-tauri/Cargo.toml --no-default-features` must keep passing.

## 6. UI/UX Requirements

Not applicable. No UI, no layout, no design token, and no visual asset is
touched; the notification body and badge rendering are unaffected. The design
step is skipped for this feature (gate `create-spec.design-step`, option
`decide_autonomously`).

## 7. Data Requirements

### 7.1 Data Model Overview

| Structure | Shape after this feature |
|-----------|--------------------------|
| Rate-limit key | `tab:<id>` for `PaneKey::Tab`, `mux:<scope>:<pane_id>` for `PaneKey::MuxPane`. `muxpub:` is retired. |
| `AgentNotificationRateLimiter` map | key `String` -> `last_fired` instant, pruned inside `record` at the `AGENT_NOTIFICATION_RATE_LIMIT` threshold. |
| `App::mux_public_pane_ids` | `HashMap<(ConnectionScope, u32), String>`, retained unchanged for the sidebar's copy-to-clipboard row. |

### 7.2 Data Retention

| Data kind | Retention |
|------------|----------|
| Rate-limit entries | Process-local. Dropped on close-time discard (FR7) and on expiry inside `record` (FR6). Never serialized, never on the wire, never persisted. |
| Learned `public_pane_id` | Process-local. Removed on pane close / tab close / tab reap (FR4). |

## 8. External Integration

### 8.1 Integrated Systems

| System | Integration | Data |
|------------|----------|--------|
| mux daemon | Across a trust boundary; supplies `AgentStatusUpdateMsg` including `public_pane_id` | After this feature the supplied string reaches only the sidebar identifier path, never the rate-limit key |

### 8.2 API Requirements

`App::mux_public_pane_id(scope, pane_id)` keeps its current behaviour: the
daemon's learned string verbatim for a learned pane, `None` for an unlearned or
released one. No daemon-side change is required (NFR2).

## 9. Constraints

### 9.1 Technical Constraints

- Pruning runs only inside `record`; an entirely idle limiter keeps its entries
  until the next fire (NFR3).
- `HashMap::remove` does not shrink capacity, so expiry bounds live entries, not
  the allocation high-water mark (NFR3).
- `is_within_limit` keeps its read-only `&self` signature and is not the pruning
  site (FR6).
- No new dependency; the `gui` / `--no-default-features` feature split is
  unchanged (NFR4).

### 9.2 Business Constraints

- Superseded-entry eviction (option (b)) is not adopted (FR3).
- `App::mux_public_pane_ids` and `App::mux_public_pane_id` are not removed (FR4).
- `AgentNotificationRateLimiter::discard` and its call sites are not removed
  (FR7).

### 9.3 Schedule Constraints

None recorded.

### 9.4 Declared Change Set

The feature-specific paths are not enumerated by hand here; they are derived at
create-plan from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them):

- `feature-docs/mux-rate-limit-key-pane-identity/**`
- `test-docs/mux-rate-limit-key-pane-identity/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces; the generating owners are the phase documents
and `references/phase-state.md` (cited, not restated).

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`; the
generating owner is `implement-phase.md` (cited, not restated).

**Semantics**:

- Default members are part of the declaration unless the SPEC author explicitly
  removes them; removal is a deliberate narrowing, never an omission by silence.
- The declaration is a SUPERSET assertion: the actual change set must be
  CONTAINED IN the declared set. A declared path that never materializes is not a
  violation.

## 10. Anticipated Issues and Risks

### 10.1 Technical Issues

| Issue | Impact | Mitigation |
|------|--------|------------|
| Lazy pruning leaves entries in an idle limiter | Low | Accepted as stated in NFR3; close-time discard (FR7) still removes closed panes' entries. |
| `HashMap` capacity is not shrunk by eviction | Low | Accepted as stated in NFR3. |
| A fifth derivation site may exist in the Detached-frame handler | Medium | AS-7 records this as inferred, not confirmed from the handler's own source; if a separate derivation exists there it needs the same FR2 signature update. |

### 10.2 Business Risks

| Risk | Likelihood | Impact | Mitigation |
|--------|----------|--------|------------|
| A test whose premise disappears is deleted silently, losing a security guard | Low | Medium | FR9 requires the two affected tests to be restated as guards that the derivation cannot be influenced by any learned id. |

## 11. Success Criteria

### 11.1 Acceptance Criteria

- [ ] AC-1: For any `ConnectionScope(s)` and wire `pane_id p`,
      `agent_notification_rate_limit_key(&PaneKey::MuxPane(ConnectionScope(s), p)) == format!("mux:{s}:{p}")`,
      whether or not a public id has ever been learned for that pane.
- [ ] AC-2: `agent_notification_rate_limit_key(&PaneKey::Tab(id)) == format!("tab:{id}")`,
      unchanged from today.
- [ ] AC-3: No string the daemon supplies can appear in any rate-limit key: the
      derivation's only inputs are the `PaneKey`'s own scope and wire pane id, and
      the function does not accept the learned-id map at all (a compile-time
      property, not a runtime check).
- [ ] AC-4: Two connections holding the same wire pane id derive different keys
      (`mux:<scopeA>:1` vs `mux:<scopeB>:1`), and a learned id byte-identical to
      another pane's or tab's key cannot reach that key, because the key never
      depends on any learned id.
- [ ] AC-5: A pane's derived key is identical before and after the daemon
      re-mints its `public_pane_id`, so an armed cooldown survives a rotation
      attempt for the pane's whole lifetime.
- [ ] AC-6: After a fire is recorded for key K at time T, a `record` for any key
      at time >= T + AGENT_NOTIFICATION_RATE_LIMIT leaves K absent from the
      limiter's map, while a `record` at time < T + AGENT_NOTIFICATION_RATE_LIMIT
      leaves K present with its original timestamp.
- [ ] AC-7: A suppressed attempt (any gate other than the rate limit returning
      false, so `record` is never called) neither refreshes an existing timestamp
      nor evicts any entry - the limiter's contents are unchanged by an
      `is_within_limit` call.
- [ ] AC-8: Closing a tab, reaping an exited tab, and a mux pane exiting each
      still leave the closed pane's key absent from the limiter, so an immediate
      re-report under the same derived key fires rather than being suppressed.
- [ ] AC-9: `App::mux_public_pane_id(scope, pane_id)` keeps returning the
      daemon's learned string verbatim for a learned pane and `None` for an
      unlearned or released one - the sidebar's copy-to-clipboard row is
      unaffected by this feature.
- [ ] AC-10: No source file under `src-tauri/src/` emits or asserts the `muxpub:`
      prefix any more, and `CARGO_TARGET_DIR=src-tauri/target cargo test
      --manifest-path src-tauri/Cargo.toml --lib` passes.

### 11.2 KPI

Not applicable.

## 12. Test Scenarios

### 12.1 Test Perspectives

| ID | Target | Change | Requirements | Acceptance criteria |
|----|--------|--------|--------------|---------------------|
| TS-1 | `src-tauri/src/app/tests/agent_status.rs:656 agent_notification_rate_limit_key_prefers_public_pane_id_falls_back_to_prefixed_id` | rewrite + rename | FR1, FR9 | AC-1, AC-2, AC-4 |
| TS-2 | `src-tauri/src/app/tests/agent_status.rs:703 agent_notification_rate_limit_key_learned_id_matching_a_tab_key_cannot_reach_that_tab` | restate (premise removed) | FR1, FR9 | AC-3, AC-4 |
| TS-3 | `src-tauri/src/app/tests/agent_status.rs:726 agent_notification_rate_limit_key_learned_id_matching_an_unlearned_fallback_cannot_reach_that_pane` | restate (premise removed) | FR1, FR9 | AC-3 |
| TS-4 | `src-tauri/src/app/tests/agent_status.rs:1171 ts5_public_pane_id_map_and_rate_limit_key_are_scoped` | update assertions | FR4, FR9 | AC-4, AC-9 |
| TS-5 | `src-tauri/src/app/tests/agent_status.rs:1540 ac3_detach_releases_model_entry_public_id_and_rate_limit_identity` | correct (currently vacuous) + update | FR10 | AC-8 |
| TS-6 | `src-tauri/src/app/tests/agent_status.rs:1720 ac6_detach_on_one_tab_leaves_a_second_tabs_identically_numbered_pane_untouched` | call-site update only | FR2, FR9 | AC-5 |
| TS-7 | `src-tauri/src/app/tests/agent_status.rs:432, 454, 528` | call-site update only | FR2, FR5, FR7 | AC-8 |
| TS-8 | `src-tauri/src/app/tests/agent_status.rs:242 maybe_notify_agent_transition_ac6_judgement_independent_of_pane_key_format` | comment update only | FR8 | AC-10 |
| TS-9 | `src-tauri/src/notifications.rs` (new test, alongside `rate_limiter_*` tests at 918-978) | new | FR6 | AC-6 |
| TS-10 | `src-tauri/src/notifications.rs` (new test) | new | FR6, NFR1 | AC-7 |
| TS-11 | `src-tauri/src/notifications.rs:970 rate_limiter_discard_drops_bookkeeping_for_closed_pane` | keep unchanged | FR7 | AC-8 |

### 12.2 Test Scenario Details

- **TS-1**: Rename to reflect that there is no preference step any more (e.g.
  `agent_notification_rate_limit_key_is_scope_and_pane_id_for_every_mux_pane`).
  Assert `mux:1:7` for a pane whose public id WAS learned, `mux:1:8` for an
  unlearned pane, `tab:3` for a plain tab, and `mux:2:7` for a second scope's
  same-numbered pane.
- **TS-2**: The `muxpub:` prefix assertion (line 718) is gone. Restate as: seeding
  the learned-id map with a value byte-identical to `tab:5`'s key leaves the mux
  pane's derived key unchanged and still different from the tab's - the
  strengthened reason being that the derivation cannot read the map at all.
- **TS-3**: Same treatment as TS-2 against the `mux:<scope>:<pane_id>` fallback
  form: a learned id equal to pane 8's key cannot make pane 9 derive it.
- **TS-4**: Lines 1221-1222 change from `format!("muxpub:{}:daemon-a-1", scope0.0)`
  / `daemon-b-1` to `format!("mux:{}:1", scope0.0)` / `format!("mux:{}:1", scope1.0)`.
  The `assert_ne!(rate_key0, rate_key1)` and both `mux_public_pane_id` assertions
  (1209-1210) stay.
- **TS-5**: Replace the two literal "xyz-7" calls to `maybe_notify_agent_transition`
  (lines 1571, 1589) with the key obtained from
  `agent_notification_rate_limit_key(&PaneKey::MuxPane(scope, 7))`, so the
  pre-detach assertion genuinely proves the window armed by `pump_all`'s own
  drain is open and the post-detach assertion genuinely proves it was released.
- **TS-6**: Lines 1760 and 1781 lose the map argument; the before/after equality
  assertion still holds (and now holds trivially, which is the point).
- **TS-7**: `close_tab_discards_agent_notification_rate_limit_state`,
  `pump_all_reap_exited_tab_discards_agent_notification_rate_limit_state` and
  `pump_all_closed_mux_pane_discards_agent_notification_rate_limit_state` drop the
  map argument. The last of these keeps its value as the arm/discard agreement
  test for a learned pane; its derive-before-removal ordering rationale comment
  (lines 473-479, 523-525) is rewritten since that ordering is no longer
  load-bearing.
- **TS-8**: The test body uses free-form key strings and needs no change, but its
  header comment (lines 242-247) cites `public_pane_id, e.g. "xyz-7"` as the
  mux-shaped key. Update the example to `mux:<scope>:<pane_id>`.
- **TS-9**: Expiry eviction: record key A at `now`, record key B at
  `now + AGENT_NOTIFICATION_RATE_LIMIT`, then assert A is gone while B is present
  and throttled.
- **TS-10**: Pruning is decision-neutral and not triggered by reads: record A at
  `now`, then call `is_within_limit` repeatedly at
  `now + 2 * AGENT_NOTIFICATION_RATE_LIMIT` and assert the limiter's contents are
  unchanged; also assert that a record within the window does NOT evict a
  still-unexpired sibling entry.
- **TS-11**: Regression guard that discard-on-close survives the addition of
  expiry eviction. No edit expected; listed so the plan does not fold it into the
  expiry tests.

## 13. Glossary

| Term | Definition |
|------|------|
| `PaneKey` | The canonical pane identity used by the agent-status model, badge lookup, tab-title resolution and visibility resolution: `Tab(id)` or `MuxPane(scope, pane_id)`. |
| `public_pane_id` | The daemon-supplied, API-facing pane identifier, stable across daemon restarts. |
| Learned-id map | `App::mux_public_pane_ids`: `HashMap<(ConnectionScope, u32), String>`. |
| `muxpub:` | The retired key prefix that embedded the daemon-supplied learned id. |
| `AGENT_NOTIFICATION_RATE_LIMIT` | The 30s agent-notification cooldown window. |
| Option (a) `always_code_owned_key` | The adopted remediation: a mux pane's key is always `mux:<scope>:<pane_id>`. |
| Option (b) | Superseded-entry eviction; explicitly not adopted. |

## 14. Confirmations

### 14.1 Confirmed Items

- [x] AS-1: Daemon pane ids are monotonically allocated with no renumbering path,
      hot-upgrade restore preserves the pane counter and incarnation verbatim, and
      detach/reattach deliberately does not preserve limiter state - so
      `(ConnectionScope, wire pane_id)` is lifetime-stable and concurrently
      unique, which is all task0009's throttling identity required. (reversible)
- [x] AS-2: `public_pane_id`'s documented purpose is an API-facing identifier
      stable across daemon restarts; it was never load-bearing for throttling
      identity, and no other consumer of the rate-limit key exists. (reversible)
- [x] AS-3: `PaneKey::MuxPane(scope, pane_id)` is already the canonical pane
      identity used by the agent-status model, badge lookup, tab-title resolution
      and visibility resolution; the rate-limit key was the only place a
      daemon-supplied string diverged from it. (reversible)
- [x] AS-4: Rate-limit keys are process-local (not serialized, not on the wire,
      not persisted), so no migration path is needed and `muxpub:` can simply be
      retired. (reversible)
- [x] AS-5: Keying `AgentNotificationRateLimiter` by `PaneKey` instead of a
      formatted `String`, and adding a hard entry cap plus a global token bucket
      against adversarial bursts, are out of scope for this feature and recorded
      only as follow-up candidates. (reversible)
- [x] AS-6: `Instant::duration_since` saturates rather than panics for an earlier
      instant, so pruning inside `record` is safe for the monotonic `Instant`
      values these call sites pass. (reversible)
- [x] Design step: skipped. No user-visible surface changes - this feature alters
      an internal key-derivation function, a rate limiter's eviction policy, and
      the tests pinning them. Recommendation accepted via gate
      `create-spec.design-step`, option `decide_autonomously`.

### 14.2 Unconfirmed / Pending Items

- [ ] AS-7: The Detached-frame handler that releases a scope's panes reaches the
      limiter only through `apply_agent_status_batch`'s `closed_panes` loop, so
      there is no fifth call site of `agent_notification_rate_limit_key`. This was
      inferred from the scanned paths and is not confirmed from the handler's own
      source; if a separate derivation exists there it needs the same FR2
      signature update. (reversible)

## 15. References

- SPEC: `feature-docs/mux-rate-limit-key-pane-identity/SPEC.md`
- Origin review: [https://github.com/m-m-n/emterm/pull/55](https://github.com/m-m-n/emterm/pull/55)
  round 1, medium findings `0810c3130449c0dc` (comprehensive) and
  `0247047963c1c329` (security)
- Prior feature referenced by FR5: mux-agent-status-pane-key-collision (FR4/FR6,
  CD-2 ordering)
- Prior feature referenced by FR4: task0006 AC-5 (sidebar copy-to-clipboard row)
