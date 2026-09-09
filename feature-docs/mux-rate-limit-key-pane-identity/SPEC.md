# Feature: mux-rate-limit-key-pane-identity

## Overview

The agent-notification rate-limit key for a mux pane currently embeds a
daemon-supplied `public_pane_id`, which lets a daemon across the trust boundary
alias two panes onto one throttle bucket, mint a fresh bucket per update by
rotating the id, or strand unreachable entries in an unpruned `HashMap`. This
feature makes a mux pane's key always `mux:<scope>:<pane_id>`, derived only from
code-owned values, and adds expiry eviction inside the rate limiter's `record`.
Requirement definitions live in
`feature-docs/mux-rate-limit-key-pane-identity/REQUIREMENTS.md`.

## Objectives

- A mux pane's agent-notification rate-limit identity is derived only from
  code-owned values, so no daemon-supplied string can participate in throttling
  identity or be rotated to evade a cooldown.
- The rate limiter's live-entry count stays bounded over a long-running session,
  instead of growing with every pane that ever fired a notification.
- The existing test suite states what the code actually does, so the
  pane-identity contract is pinned rather than merely described.

## User Stories

### US1: Throttling identity that a daemon cannot influence

As an eMterm user running mux panes, I want a pane's notification throttling
identity to come only from code-owned values, so that a hostile or buggy daemon
cannot alias two panes onto one bucket or escape a cooldown by rotating its
`public_pane_id`.

**Acceptance Criteria:**
- [ ] AC-1: `agent_notification_rate_limit_key(&PaneKey::MuxPane(ConnectionScope(s), p)) == format!("mux:{s}:{p}")`
      for any scope `s` and wire pane id `p`, whether or not a public id has ever
      been learned for that pane.
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

### US2: A rate limiter that stays bounded

As an eMterm user in a long-running session, I want rate-limiter entries that can
no longer affect any decision to be evicted, so that the limiter's live-entry
count does not grow with every pane that ever fired a notification.

**Acceptance Criteria:**
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

### US3: Source and tests that state the real contract

As an eMterm maintainer, I want the doc comments and the tests to state what the
derivation actually does, so that the pane-identity contract is pinned rather
than merely described.

**Acceptance Criteria:**
- [ ] AC-9: `App::mux_public_pane_id(scope, pane_id)` keeps returning the
      daemon's learned string verbatim for a learned pane and `None` for an
      unlearned or released one - the sidebar's copy-to-clipboard row is
      unaffected by this feature.
- [ ] AC-10: No source file under `src-tauri/src/` emits or asserts the `muxpub:`
      prefix any more, and `CARGO_TARGET_DIR=src-tauri/target cargo test
      --manifest-path src-tauri/Cargo.toml --lib` passes.

## Technical Requirements

### Functional Requirements

- **FR1 - Mux pane keys are always the code-owned form:**
  `agent_notification_rate_limit_key` returns `mux:<scope>:<pane_id>` for every
  `PaneKey::MuxPane`, unconditionally. The `muxpub:<scope>:<learned>` branch is
  removed; `muxpub:` becomes a retired prefix that the code no longer emits
  anywhere. `PaneKey::Tab(id)` keeps returning `tab:<id>` unchanged.
- **FR2 - The derivation stops consulting the learned-id map:**
  `agent_notification_rate_limit_key` no longer takes the
  `mux_public_pane_ids: &HashMap<(ConnectionScope, u32), String>` parameter; its
  only input is the `&PaneKey`. All four production call sites are updated to the
  new signature: `app/agent_status.rs:339` (closed-mux-pane loop),
  `app/agent_status.rs:370` (transition-drain loop), `app/tab_lifecycle.rs:148`
  (`close_tab`), `app/mod.rs:1488` (reaped-exited-tab loop).
- **FR3 - Rotation-evasion remediation:** The chosen remediation is option (a)
  `always_code_owned_key`: because a live pane's key is a pure function of
  `(ConnectionScope, wire pane_id)`, a daemon that re-mints or rotates a
  `public_pane_id` cannot change the key its pane throttles under, so a cooldown
  can no longer be escaped by supplying a new learned id. Superseded-entry
  eviction (option (b)) is explicitly NOT adopted: evicting the old entry removes
  history without transferring the cooldown timestamp, leaving the new key
  immediately allowed.
- **FR4 - The learned-id map itself is retained:** `App::mux_public_pane_ids` and
  the public accessor `App::mux_public_pane_id(scope, pane_id)` are kept
  unchanged - they serve `ui::mux_sidebar`'s copy-to-clipboard row (task0006
  AC-5), which is an API-facing identifier concern, not a throttling one.
  Learning on `AgentStatusUpdateMsg` (`app/agent_status.rs:321`) and removal on
  pane close / tab close / tab reap (`app/agent_status.rs:340`,
  `app/tab_lifecycle.rs:150`, `app/mod.rs:1490`) all stay.
- **FR5 - The derive-before-removal ordering constraint is retired:** Because the
  derivation no longer reads `mux_public_pane_ids`, the three discard sites no
  longer need to derive the rate-limit key BEFORE removing the scoped map entry
  (the CD-2 ordering of mux-agent-status-pane-key-collision FR4/FR6). The map
  removal itself is kept; only the ordering obligation and every code comment
  asserting it (`app/agent_status.rs:333-339`, `app/tab_lifecycle.rs:138-148`,
  `app/mod.rs:1484-1488`) are corrected so the source stops documenting a
  constraint that no longer exists.
- **FR6 - Expiry eviction in the rate limiter:**
  `AgentNotificationRateLimiter::record` additionally drops every entry whose
  `last_fired` is older than `AGENT_NOTIFICATION_RATE_LIMIT` (30s) relative to the
  `now` passed in, before/while inserting the new entry. `is_within_limit` keeps
  its `&self` read-only signature and is not the pruning site, so a suppressed
  attempt never refreshes or evicts a timestamp.
- **FR7 - Discard-on-close is kept alongside expiry:**
  `AgentNotificationRateLimiter::discard` and every
  `App::discard_agent_notification_state` call site are retained unchanged.
  Expiry eviction is additive: a pane key reused shortly after its pane closed
  must still not inherit an unexpired cooldown, which only the close-time discard
  guarantees.
- **FR8 - Doc comments describing the three-form derivation are corrected:** The
  `agent_notification_rate_limit_key` doc comment (`app/agent_status.rs:83-108`)
  is rewritten from three mutually disjoint forms to two (`tab:` / `mux:`), and
  the `App::agent_notification_rate_limiter` field doc (`app/mod.rs:434-443`)
  stops describing a daemon-learned id wrapped behind a namespace prefix, stating
  instead that no daemon-supplied byte ever reaches the key.
- **FR9 - Existing tests are updated to the option (a) shape:** Every app-layer
  test that asserts the `muxpub:` form or passes the learned-id map is updated;
  the two tests whose entire premise (a daemon-controlled string reaching the
  derivation) disappears are restated as guards that the derivation cannot be
  influenced by any learned id at all, rather than deleted silently.
- **FR10 - The already-weak detach test is corrected:**
  `ac3_detach_releases_model_entry_public_id_and_rate_limit_identity`
  (`src-tauri/src/app/tests/agent_status.rs:1540`) currently arms/probes the raw
  string "xyz-7" (lines 1571, 1589) while production armed
  `muxpub:<scope>:xyz-7`, so its rate-limit assertions passed vacuously on an
  unrelated key. It is corrected to obtain the key from
  `agent_notification_rate_limit_key(&PaneKey::MuxPane(scope, 7))` so it
  genuinely exercises the identity it names.

### Non-Functional Requirements

- **NFR1 - Correctness (pruning is decision-neutral):** Expiry eviction uses
  exactly the same threshold and comparison as `is_within_limit`
  (`now.duration_since(prev) >= AGENT_NOTIFICATION_RATE_LIMIT`), so a pruned entry
  is by construction one that would have answered `true` anyway. No observable
  throttling decision changes as a result of pruning.
- **NFR2 - Compatibility (no migration surface):** Rate-limit keys are
  process-local: never serialized, never sent over the mux wire, never persisted
  to settings or snapshot. Changing the key shape therefore requires no
  migration, no compatibility window, and no daemon-side change.
- **NFR3 - Accepted limits of the chosen pruning shape:** Pruning is lazy (it
  runs only inside `record`, so an entirely idle limiter keeps its entries until
  the next fire) and `HashMap::remove` does not shrink capacity (so expiry bounds
  live entries, not the allocation high-water mark). Both are accepted, not
  defects to be worked around in this feature.
- **NFR4 - Build and test surface unchanged:** No new dependency, no change to
  the public CLI surface, and no change to the `gui` / `--no-default-features`
  feature split. `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
  src-tauri/Cargo.toml --no-default-features` must keep passing.

## Implementation Approach

### Architecture

**Component relationships:**

```
mux daemon  --AgentStatusUpdateMsg(public_pane_id)-->  app/agent_status.rs
                                                        |
                    +-----------------------------------+------------------+
                    |                                                      |
        App::mux_public_pane_ids                        agent_notification_rate_limit_key(&PaneKey)
        (learned id, FR4)                               (code-owned only, FR1/FR2)
                    |                                                      |
        ui::mux_sidebar copy-to-clipboard               AgentNotificationRateLimiter
        (App::mux_public_pane_id, AC-9)                 (record: insert + expiry evict, FR6)
                                                        (discard: close-time, FR7)
```

The daemon-supplied string keeps exactly one downstream consumer - the sidebar's
copy-to-clipboard row. The throttling path no longer touches it.

### Data Flow

```
PaneKey::Tab(id)                -> "tab:<id>"                 (FR1, AC-2)
PaneKey::MuxPane(scope, pane)   -> "mux:<scope>:<pane>"       (FR1, AC-1)

is_within_limit(key, now)  -> &self, read-only, no eviction    (FR6, AC-7)
record(key, now)           -> insert(key, now) + drop every entry whose
                              last_fired is older than AGENT_NOTIFICATION_RATE_LIMIT
                              relative to now                  (FR6, AC-6)
discard(key)               -> unchanged close-time removal     (FR7, AC-8)
```

### Key Derivation Contract

| `PaneKey` variant | Derived key | Inputs |
|---|---|---|
| `Tab(id)` | `tab:<id>` | `id` (code-owned) |
| `MuxPane(scope, pane_id)` | `mux:<scope>:<pane_id>` | `scope`, wire `pane_id` (both code-owned) |
| (retired) | `muxpub:<scope>:<learned>` | no longer emitted anywhere |

The function's signature carries only `&PaneKey`; the learned-id map is not a
parameter, which makes AC-3 a compile-time property rather than a runtime check.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/app/agent_status.rs`: the derivation, its doc comment, the
  closed-mux-pane loop and the transition-drain loop.
- `src-tauri/src/app/tab_lifecycle.rs`: `close_tab` call site and its ordering
  comment.
- `src-tauri/src/app/mod.rs`: reaped-exited-tab loop, its ordering comment, and
  the `App::agent_notification_rate_limiter` field doc.
- `src-tauri/src/notifications.rs`: `AgentNotificationRateLimiter::record` /
  `is_within_limit` / `discard` and `AGENT_NOTIFICATION_RATE_LIMIT`.
- `src-tauri/src/ui/mux_sidebar`: consumer of `App::mux_public_pane_id`,
  unchanged (AC-9).

**External Dependencies:**
- None added (NFR4).

### File Structure

```
src-tauri/src/
├── app/
│   ├── agent_status.rs          # derivation + doc comment + 2 call sites (FR1/FR2/FR5/FR8)
│   ├── tab_lifecycle.rs         # close_tab call site + comment (FR2/FR5)
│   ├── mod.rs                   # reaped-tab call site + comment + field doc (FR2/FR5/FR8)
│   └── tests/
│       └── agent_status.rs      # TS-1..TS-8 (FR9/FR10)
└── notifications.rs             # record expiry eviction + TS-9..TS-11 (FR6/FR7)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths above are derived at create-plan from every task's
`files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/mux-rate-limit-key-pane-identity/**`
- `test-docs/mux-rate-limit-key-pane-identity/**`

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
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no `test-docs/{feature}/`
directory at all; the declared `test-docs/{feature}/**` entry is still correct in
that case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] TS-1 (`src-tauri/src/app/tests/agent_status.rs:656`
      `agent_notification_rate_limit_key_prefers_public_pane_id_falls_back_to_prefixed_id`,
      rewrite + rename) — Rename to reflect that there is no preference step any
      more (e.g.
      `agent_notification_rate_limit_key_is_scope_and_pane_id_for_every_mux_pane`).
      Assert `mux:1:7` for a pane whose public id WAS learned, `mux:1:8` for an
      unlearned pane, `tab:3` for a plain tab, and `mux:2:7` for a second scope's
      same-numbered pane. Requirements: FR1, FR9. Criteria: AC-1, AC-2, AC-4.
- [ ] TS-4 (`src-tauri/src/app/tests/agent_status.rs:1171`
      `ts5_public_pane_id_map_and_rate_limit_key_are_scoped`, update assertions) —
      Lines 1221-1222 change from `format!("muxpub:{}:daemon-a-1", scope0.0)` /
      `daemon-b-1` to `format!("mux:{}:1", scope0.0)` /
      `format!("mux:{}:1", scope1.0)`. The `assert_ne!(rate_key0, rate_key1)` and
      both `mux_public_pane_id` assertions (1209-1210) stay. Requirements: FR4,
      FR9. Criteria: AC-4, AC-9.
- [ ] TS-8 (`src-tauri/src/app/tests/agent_status.rs:242`
      `maybe_notify_agent_transition_ac6_judgement_independent_of_pane_key_format`,
      comment update only) — The test body uses free-form key strings and needs no
      change, but its header comment (lines 242-247) cites
      `public_pane_id, e.g. "xyz-7"` as the mux-shaped key. Update the example to
      `mux:<scope>:<pane_id>`. Requirements: FR8. Criteria: AC-10.
- [ ] TS-9 (`src-tauri/src/notifications.rs`, new test alongside the
      `rate_limiter_*` tests at 918-978) — Expiry eviction: record key A at `now`,
      record key B at `now + AGENT_NOTIFICATION_RATE_LIMIT`, then assert A is gone
      while B is present and throttled. Requirements: FR6. Criteria: AC-6.
- [ ] TS-10 (`src-tauri/src/notifications.rs`, new test) — Pruning is
      decision-neutral and not triggered by reads: record A at `now`, then call
      `is_within_limit` repeatedly at `now + 2 * AGENT_NOTIFICATION_RATE_LIMIT`
      and assert the limiter's contents are unchanged; also assert that a record
      within the window does NOT evict a still-unexpired sibling entry.
      Requirements: FR6, NFR1. Criteria: AC-7.
- [ ] TS-11 (`src-tauri/src/notifications.rs:970`
      `rate_limiter_discard_drops_bookkeeping_for_closed_pane`, keep unchanged) —
      Regression guard that discard-on-close survives the addition of expiry
      eviction. No edit expected; listed so the plan does not fold it into the
      expiry tests. Requirements: FR7. Criteria: AC-8.

### Integration Tests

- [ ] TS-5 (`src-tauri/src/app/tests/agent_status.rs:1540`
      `ac3_detach_releases_model_entry_public_id_and_rate_limit_identity`, correct
      (currently vacuous) + update) — Replace the two literal "xyz-7" calls to
      `maybe_notify_agent_transition` (lines 1571, 1589) with the key obtained
      from `agent_notification_rate_limit_key(&PaneKey::MuxPane(scope, 7))`, so
      the pre-detach assertion genuinely proves the window armed by `pump_all`'s
      own drain is open and the post-detach assertion genuinely proves it was
      released. Requirements: FR10. Criteria: AC-8.
- [ ] TS-6 (`src-tauri/src/app/tests/agent_status.rs:1720`
      `ac6_detach_on_one_tab_leaves_a_second_tabs_identically_numbered_pane_untouched`,
      call-site update only) — Lines 1760 and 1781 lose the map argument; the
      before/after equality assertion still holds (and now holds trivially, which
      is the point). Requirements: FR2, FR9. Criteria: AC-5.
- [ ] TS-7 (`src-tauri/src/app/tests/agent_status.rs:432, 454, 528`, call-site
      update only) — `close_tab_discards_agent_notification_rate_limit_state`,
      `pump_all_reap_exited_tab_discards_agent_notification_rate_limit_state` and
      `pump_all_closed_mux_pane_discards_agent_notification_rate_limit_state` drop
      the map argument. The last of these keeps its value as the arm/discard
      agreement test for a learned pane; its derive-before-removal ordering
      rationale comment (lines 473-479, 523-525) is rewritten since that ordering
      is no longer load-bearing. Requirements: FR2, FR5, FR7. Criteria: AC-8.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      src-tauri/Cargo.toml --lib` passes (AC-10).
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
      src-tauri/Cargo.toml --no-default-features` keeps passing (NFR4).

### Edge Cases

- [ ] TS-2 (`src-tauri/src/app/tests/agent_status.rs:703`
      `agent_notification_rate_limit_key_learned_id_matching_a_tab_key_cannot_reach_that_tab`,
      restate (premise removed)) — The `muxpub:` prefix assertion (line 718) is
      gone. Restate as: seeding the learned-id map with a value byte-identical to
      `tab:5`'s key leaves the mux pane's derived key unchanged and still
      different from the tab's - the strengthened reason being that the derivation
      cannot read the map at all. Requirements: FR1, FR9. Criteria: AC-3, AC-4.
- [ ] TS-3 (`src-tauri/src/app/tests/agent_status.rs:726`
      `agent_notification_rate_limit_key_learned_id_matching_an_unlearned_fallback_cannot_reach_that_pane`,
      restate (premise removed)) — Same treatment as TS-2 against the
      `mux:<scope>:<pane_id>` fallback form: a learned id equal to pane 8's key
      cannot make pane 9 derive it. Requirements: FR1, FR9. Criteria: AC-3.

### Performance Tests

Not applicable. The only performance-relevant property is that expiry bounds the
limiter's live-entry count; its accepted limits are stated in NFR3.

## Security Considerations

- **Trust boundary:** The mux daemon is across a trust boundary. Every value it
  supplies is untrusted.
- **Input Validation:** No string the daemon supplies can appear in any
  rate-limit key. The derivation's only inputs are the `PaneKey`'s own scope and
  wire pane id, and the function does not accept the learned-id map at all - a
  compile-time property, not a runtime check (FR1, FR2, AC-3).
- **Aliasing:** Two connections holding the same wire pane id derive different
  keys, and a learned id byte-identical to another pane's or tab's key cannot
  reach that key, because the key never depends on any learned id (AC-4).
- **Cooldown evasion:** A pane's derived key is identical before and after the
  daemon re-mints its `public_pane_id`, so an armed cooldown survives a rotation
  attempt for the pane's whole lifetime (FR3, AC-5).
- **Unbounded growth:** Expiry eviction inside `record` (FR6) bounds the
  limiter's live entries; close-time discard (FR7) remains for keys reused
  shortly after a pane closed.
- **Data Protection:** The daemon-learned `public_pane_id` is retained solely as
  an API-facing identifier for the sidebar's copy-to-clipboard row (FR4, AC-9).

## Error Handling

No new error paths. `Instant::duration_since` saturates rather than panics for an
earlier instant, so pruning inside `record` is safe for the monotonic `Instant`
values these call sites pass (AS-6).

## Performance Optimization

### Performance Goals

- The limiter's live-entry count stays bounded over a long-running session
  instead of growing with every pane that ever fired a notification.

### Optimization Strategies

- Expiry eviction inside `record`, using exactly the same threshold and
  comparison as `is_within_limit`, so pruning is decision-neutral (NFR1).

### Accepted Limits

- Pruning is lazy: an entirely idle limiter keeps its entries until the next fire.
- `HashMap::remove` does not shrink capacity: expiry bounds live entries, not the
  allocation high-water mark.

Both are accepted, not defects to be worked around in this feature (NFR3).

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] All non-functional requirements (NFR1-NFR4) are satisfied
- [ ] All test scenarios (TS-1 through TS-11) pass
- [ ] All acceptance criteria (AC-1 through AC-10) are met
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      src-tauri/Cargo.toml --lib` passes
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path
      src-tauri/Cargo.toml --no-default-features` passes
- [ ] No source file under `src-tauri/src/` emits or asserts the `muxpub:` prefix
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every FR and NFR is `status: resolved`.

One reversible assumption is recorded as unconfirmed rather than as an open
requirement:

- AS-7: The Detached-frame handler that releases a scope's panes reaches the
  limiter only through `apply_agent_status_batch`'s `closed_panes` loop, so there
  is no fifth call site of `agent_notification_rate_limit_key`. This was inferred
  from the scanned paths and is not confirmed from the handler's own source; if a
  separate derivation exists there it needs the same FR2 signature update.

## Assumptions

- **AS-1:** Daemon pane ids are monotonically allocated with no renumbering path,
  hot-upgrade restore preserves the pane counter and incarnation verbatim, and
  detach/reattach deliberately does not preserve limiter state - so
  `(ConnectionScope, wire pane_id)` is lifetime-stable and concurrently unique,
  which is all task0009's throttling identity required. (reversible)
- **AS-2:** `public_pane_id`'s documented purpose is an API-facing identifier
  stable across daemon restarts; it was never load-bearing for throttling
  identity, and no other consumer of the rate-limit key exists. (reversible)
- **AS-3:** `PaneKey::MuxPane(scope, pane_id)` is already the canonical pane
  identity used by the agent-status model, badge lookup, tab-title resolution and
  visibility resolution; the rate-limit key was the only place a daemon-supplied
  string diverged from it. (reversible)
- **AS-4:** Rate-limit keys are process-local (not serialized, not on the wire,
  not persisted), so no migration path is needed and `muxpub:` can simply be
  retired. (reversible)
- **AS-5:** Keying `AgentNotificationRateLimiter` by `PaneKey` instead of a
  formatted `String`, and adding a hard entry cap plus a global token bucket
  against adversarial bursts, are out of scope for this feature and recorded only
  as follow-up candidates. (reversible)
- **AS-6:** `Instant::duration_since` saturates rather than panics for an earlier
  instant, so pruning inside `record` is safe for the monotonic `Instant` values
  these call sites pass. (reversible)
- **AS-7:** The Detached-frame handler that releases a scope's panes reaches the
  limiter only through `apply_agent_status_batch`'s `closed_panes` loop, so there
  is no fifth call site of `agent_notification_rate_limit_key`. This was inferred
  from the scanned paths and is not confirmed from the handler's own source; if a
  separate derivation exists there it needs the same FR2 signature update.
  (reversible)

## Design Step

Skipped. No user-visible surface changes: this feature alters an internal
key-derivation function, a rate limiter's eviction policy, and the tests pinning
them. No UI, no layout, no design token, no visual asset is touched, and the
notification body / badge rendering are unaffected. Recommendation accepted via
gate `create-spec.design-step`, option `decide_autonomously`.

## References

- Requirements document:
  `feature-docs/mux-rate-limit-key-pane-identity/REQUIREMENTS.md`
- Origin review: [https://github.com/m-m-n/emterm/pull/55](https://github.com/m-m-n/emterm/pull/55)
  round 1, medium findings `0810c3130449c0dc` (comprehensive) and
  `0247047963c1c329` (security)
- Prior feature referenced by FR5: mux-agent-status-pane-key-collision (FR4/FR6,
  CD-2 ordering)
- Prior feature referenced by FR4: task0006 AC-5 (sidebar copy-to-clipboard row)
