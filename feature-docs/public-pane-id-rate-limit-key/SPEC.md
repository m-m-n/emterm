# Feature: public-pane-id-rate-limit-key

## Overview

The `MuxPane` branch of `agent_notification_rate_limit_key` currently returns
the mux daemon's `public_pane_id` string verbatim as a key in the single
shared agent-notification rate-limit key space, which lets an untrusted
daemon force a cross-tab / cross-connection collision. This feature
namespaces that learned string as `"muxpub:{scope.0}:{learned id}"` so the
three produced key forms (`"tab:"`, `"mux:"`, `"muxpub:"`) are mutually
disjoint by construction, and corrects the two doc comments that assert a
collision property the code does not have. The change is confined to an
internal derived value — no wire format, no persisted state and no
user-visible identifier changes.

Requirements source: `REQUIREMENTS.md` in this directory. Every FR/NFR ID
below is the same ID that document carries.

**Design step: skipped.** No user-visible surface changes. The feature alters
one internal derived string inside a single existing function plus two doc
comments; it touches no UI layout, no design token, no CSS, no egui widget
and no WebView. The design-step recommendation was skip, and the
`create-spec.design-step` gate resolved to `decide_autonomously`
(batch-decision-table), accepting it.

## Objectives

- **BO-1:** Remove the cross-tab / cross-connection rate-limit collision a
  mux daemon can currently force. The daemon is an external process on the
  far side of a trust boundary (it commonly runs on a remote host over SSH),
  and its verbatim `public_pane_id` string is used today as a key in the
  single shared agent-notification rate-limit key space.
- **BO-2:** Keep the fix confined to an internal derived value, so that no
  wire format, no persisted state, and no user-visible identifier changes.
- **BO-3:** Make the code's own documentation match the collision property it
  claims, so the next reader is not misled into believing the `"mux:"` prefix
  protects the learned-id branch.

## User Stories

### US1: A daemon-supplied pane id cannot reach another pane's rate-limit bucket

As an eMterm user whose mux panes receive agent-status notifications, I want
the rate-limit key derived for a mux pane to be namespaced, so that an
untrusted daemon cannot suppress or clear another pane's notification state.

**Acceptance Criteria:**
- [ ] AC-1: For a pane whose public id has been learned, the derived key
      equals `"muxpub:" + scope.0 + ":" + the learned string`, and never
      equals the learned string itself.
- [ ] AC-2: A daemon that sends `public_pane_id = "tab:5"` cannot reach the
      plain tab with `stable_id` 5: the derived key is
      `"muxpub:<scope>:tab:5"`, which is not equal to `"tab:5"`.
- [ ] AC-3: A daemon that sends `public_pane_id = "mux:1:7"` cannot reach the
      unlearned-pane bucket of scope 1 pane 7: the derived key is
      `"muxpub:<scope>:mux:1:7"`, which is not equal to `"mux:1:7"`.
- [ ] AC-4: Two connections that both learn the identical public id string
      for their own pane derive different keys, because the scope component
      differs. Same-scope same-pane repeated derivation stays stable
      (identical input yields an identical key), so the per-pane rate limit
      still suppresses the second notification within the window.
- [ ] AC-5: Arm and discard agree: the key derived at notification time for a
      pane equals the key derived for that same pane at close/reap/pane-exit
      time, so `discard_agent_notification_state` still reopens the correct
      window and never reopens another pane's.

### US2: The public id surface and its documentation stay truthful

As a developer reading `src-tauri/src/app/agent_status.rs`, I want the public
id to keep flowing to readers unchanged and the doc comments to describe what
the code actually does, so that I am not misled about which branch the
`"mux:"` prefix protects.

**Acceptance Criteria:**
- [ ] AC-6: `App::mux_public_pane_id` still returns the raw learned string
      for every pane, unchanged by this feature, including for the five
      fixtures that fail `PublicPaneId::parse`.
- [ ] AC-7: The doc comment on `agent_notification_rate_limit_key` and the
      field doc on `agent_notification_rate_limiter` both describe the
      post-change behaviour; neither retains the claim that the `"mux:"`
      prefix protects the learned-id branch.

## Technical Requirements

### Functional Requirements

- **FR1:** *Namespace the learned public_pane_id in the derived rate-limit
  key.* The `MuxPane` branch of `agent_notification_rate_limit_key`
  (`src-tauri/src/app/agent_status.rs:98-113`) MUST NOT return the
  daemon-supplied string verbatim. When a public id has been learned for
  `(scope, pane_id)`, the function MUST return
  `format!("muxpub:{}:{}", scope.0, id)` — the `"muxpub:"` literal, the
  `ConnectionScope`'s inner `u64`, and then the learned string. The learned
  string is embedded as-is; it is neither parsed, validated, escaped, nor
  truncated.
- **FR2:** *Unlearned-pane fallback key unchanged.* When no public id has
  been learned for `(scope, pane_id)`, the function MUST keep returning
  `format!("mux:{}:{pane_id}", scope.0)` exactly as today. This branch is
  already scope-qualified and already disjoint from the plain-tab form; this
  feature does not touch it.
- **FR3:** *Plain-tab key unchanged.* The `PaneKey::Tab` branch MUST keep
  returning `format!("tab:{id}")`.
- **FR4:** *Single derivation point preserved across every call site.* Every
  site that needs a rate-limit key MUST continue to obtain it by calling
  `agent_notification_rate_limit_key`, so the arm site and the discard site
  can never disagree about a pane's key. The four call sites are
  `src-tauri/src/app/agent_status.rs:328` (closed-mux-pane loop),
  `src-tauri/src/app/agent_status.rs:359` (transition-drain loop),
  `src-tauri/src/app/mod.rs:1484` (reaped-exited-tab loop) and
  `src-tauri/src/app/tab_lifecycle.rs:148` (`close_tab`). No call site may
  construct the string itself. The existing ordering constraint is also
  preserved: the closed-pane and reaped-tab sites MUST derive the key BEFORE
  removing the pane's `mux_public_pane_ids` entry, otherwise the derivation
  falls through to the FR2 fallback and discards the wrong bucket.
- **FR5:** *Correct the two stale doc comments.* The doc comment on
  `agent_notification_rate_limit_key`
  (`src-tauri/src/app/agent_status.rs:83-97`) MUST be corrected. Its current
  text states that mux panes "prefer the daemon-learned public_pane_id" and
  that "the existing 'mux:' prefix keeps that fallback from ever colliding
  with a plain-tab key" — a claim that holds only for the fallback branch and
  describes exactly the learned-id behaviour this feature changes. The
  corrected text MUST state that all three produced forms (`"tab:"`,
  `"mux:"`, `"muxpub:"`) are mutually disjoint by construction and that the
  learned daemon string is never returned unwrapped. The field doc on
  `App::agent_notification_rate_limiter`
  (`src-tauri/src/app/mod.rs:434-439`) MUST likewise stop describing a mux
  key as "a mux pane's public_pane_id" and instead describe it as a key
  derived by `agent_notification_rate_limit_key`.
- **FR6:** *Ingest-time learning and the public-id query surface stay
  unchanged.* This feature adds NO ingest-time validation.
  `apply_agent_status_batch`
  (`src-tauri/src/app/agent_status.rs:310-311`) MUST keep inserting
  `update.public_pane_id` into `mux_public_pane_ids` verbatim, with no
  `mux_ipc::protocol::PublicPaneId::parse` call and no rejection path.
  `App::mux_public_pane_id` (`src-tauri/src/app/agent_status.rs:155-163`)
  MUST keep returning that stored string unchanged, so every reader of the
  public id — including the mux sidebar — observes exactly today's value.
  Consequently the five existing test fixtures whose `public_pane_id` values
  do not satisfy `PublicPaneId::parse` (`"xyz-7"`, `"daemon-a-1"`,
  `"daemon-b-1"`, `"daemon-a-pane1"`, `"daemon-b-pane1"`) remain valid
  fixtures and MUST NOT be rewritten to parseable forms. (Previously `tbd`,
  blocked on `requirement.fix-approach`; resolved by the
  `namespace_learned_key` answer, which places the entire fix in the
  derivation and none of it at ingest.)

### Non-Functional Requirements

- **NFR1 - Compatibility:** The rate-limit key is internal-only. It is never
  serialized to the mux wire protocol, never written to `settings.json` or
  any on-disk state, and never displayed in the UI, so the format change
  carries no compatibility obligation and needs no migration.
- **NFR2 - Performance:** The derivation stays O(1) per call with at most one
  additional `String` allocation relative to today's cloned learned id. It
  runs once per drained transition and once per discarded pane, never per
  frame, so it introduces no render-path cost.
- **NFR3 - Behavioural stability:** Behaviour outside the key derivation is
  unchanged: badge aggregation (`App::agent_status_badge_for`,
  `App::agent_status_pane_badge`), sidebar public-id display
  (`App::mux_public_pane_id`), tab-title resolution
  (`agent_status_pane_tab_title`) and visibility resolution
  (`agent_status_pane_visible`) all keep their current results.
- **NFR4 - Build gating:** The change stays inside GUI-gated code
  (`src-tauri/src/app/` is behind `#[cfg(feature = "gui")]` per
  `core-architecture.md`), so the `--no-default-features` CLI-only build is
  unaffected and must keep compiling.
- **NFR5 - Documentation accuracy:** The doc comments in the touched region
  must remain the accurate description of the code after the change — no
  comment may keep asserting a collision property the code no longer
  implements.

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────────────┐
│  mux daemon (untrusted, often remote over SSH — SC-1)   │
├─────────────────────────────────────────────────────────┤
│  Ingest: apply_agent_status_batch                       │
│    agent_status.rs:310-311 — stores public_pane_id      │
│    verbatim, no validation (FR6)                        │
├─────────────────────────────────────────────────────────┤
│  State: mux_public_pane_ids                             │
│    (ConnectionScope, u32) -> String                     │
├─────────────────────────────────────────────────────────┤
│  Derivation: agent_notification_rate_limit_key          │
│    agent_status.rs:98-113 — the ONLY producer of a key  │
│      Tab(id)             -> "tab:{id}"          (FR3)   │
│      MuxPane, learned    -> "muxpub:{scope}:{id}" (FR1) │
│      MuxPane, unlearned  -> "mux:{scope}:{pane}"  (FR2) │
├─────────────────────────────────────────────────────────┤
│  Consumer: App::agent_notification_rate_limiter         │
│    AgentNotificationRateLimiter<String>, mod.rs:439     │
│    process-local, ephemeral (a1, NFR1)                  │
└─────────────────────────────────────────────────────────┘
```

**Component Diagram:**

```
agent_notification_rate_limit_key   <- single derivation point (FR4)
  ^          ^            ^                    ^
  |          |            |                    |
agent_status.rs:328   agent_status.rs:359   mod.rs:1484   tab_lifecycle.rs:148
(closed mux pane)     (transition drain)    (reaped tab)  (close_tab)
  |                       |                   |               |
discard                 arm                 discard         discard
```

The closed-pane (`agent_status.rs:328`) and reaped-tab (`mod.rs:1484`) sites
derive the key BEFORE removing the pane's `mux_public_pane_ids` entry (FR4);
the call-site comments at `agent_status.rs:322-329` and `mod.rs:1480-1487`
already record this hazard (EC-5).

### Data Flow

```
daemon update -> apply_agent_status_batch -> mux_public_pane_ids[(scope,pane)] = raw string   (FR6)
                                                    |
transition drained -> agent_notification_rate_limit_key(ids, PaneKey) -> "muxpub:{scope.0}:{raw}"  (FR1)
                                                    |
                                             rate limiter (arm)
pane closed / tab reaped / close_tab -> same derivation BEFORE map removal -> discard  (FR4, AC-5)
```

### API Design

No API change. The rate-limit key is never serialized to the mux wire
protocol and never leaves the process (NFR1), and the ingest path keeps its
current shape (FR6). No endpoint is added, removed or altered.

### Database Schema

Not applicable. There is no database and no persisted state: the key space
lives only in `App::agent_notification_rate_limiter`
(`AgentNotificationRateLimiter<String>`, `src-tauri/src/app/mod.rs:439`) and
is rebuilt from scratch each run (a1), so no migration is possible or needed
(NFR1).

The in-memory shapes involved:

| Structure | Key | Value | Notes |
|-----------|-----|-------|-------|
| `mux_public_pane_ids` | `(ConnectionScope, u32)` | `String` | Daemon string stored verbatim (FR6) |
| `agent_notification_rate_limiter` | `String` (derived key) | rate-limit window state | Process-local, ephemeral (a1) |

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/app/agent_status.rs`: holds the derivation, the ingest path,
  the public-id accessor and two of the four call sites (FR1, FR4, FR5, FR6).
- `src-tauri/src/app/mod.rs`: holds the rate limiter field and its doc, plus
  the reaped-exited-tab call site (FR4, FR5).
- `src-tauri/src/app/tab_lifecycle.rs`: holds the `close_tab` call site; it
  needs no edit because it calls the shared function, but it is part of the
  verification surface (FR4, a5).
- `src-tauri/src/app/tests/agent_status.rs`: the test file every unit and
  integration scenario below targets.

**External Dependencies:**
- `mux_ipc::protocol::PublicPaneId`: explicitly NOT called at ingest by this
  feature (FR6).
- The mux daemon: an external process outside the trust boundary, commonly
  reached over SSH (SC-1).

### File Structure

```
src-tauri/src/app/
├── agent_status.rs           # derivation (98-113), doc (83-97),
│                             # mux_public_pane_id (155-163),
│                             # ingest (310-311), call sites (328, 359)
├── mod.rs                    # rate limiter field + doc (434-439),
│                             # reaped-exited-tab call site (1484)
├── tab_lifecycle.rs          # close_tab call site (148)
└── tests/
    └── agent_status.rs       # TS-1..TS-6
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from every
task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries
in addition to the feature-specific paths above:

- `feature-docs/public-pane-id-rate-limit-key/**`
- `test-docs/public-pane-id-rate-limit-key/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal
is a deliberate, explicit narrowing. Neither is removed here.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (`src-tauri/src/app/tests/agent_status.rs`; covers FR1, FR2,
      FR3, AC-1): Extend the existing
      `agent_notification_rate_limit_key_prefers_public_pane_id_falls_back_to_prefixed_id`
      (line 634) — a plain `#[test]` over a hand-built
      `HashMap<(ConnectionScope, u32), String>`, no `App` needed. With
      `ids[(ConnectionScope(1), 7)] = "xyz-7"`, assert the
      `MuxPane(scope_a, 7)` key is `"muxpub:1:xyz-7"` (today's assertion of
      `"xyz-7"` at line 647 is the one that changes). Keep asserting
      `"mux:1:8"` for the unlearned pane and `"tab:3"` for the plain tab,
      unchanged.
- [ ] **TS-2** (`src-tauri/src/app/tests/agent_status.rs`; covers FR1, AC-2):
      New `#[test]`, same hand-built-map style: insert a hostile learned id
      `"tab:5"` for `MuxPane(ConnectionScope(9), 1)` and assert the derived
      key is `"muxpub:9:tab:5"` and `assert_ne!` against
      `agent_notification_rate_limit_key(&ids, &PaneKey::Tab(5))` — i.e.
      against `"tab:5"`.
- [ ] **TS-3** (`src-tauri/src/app/tests/agent_status.rs`; covers FR1, FR2,
      AC-3): New `#[test]`: insert a hostile learned id `"mux:1:7"` for
      `MuxPane(ConnectionScope(9), 1)` and `assert_ne!` against the key an
      unlearned `MuxPane(ConnectionScope(1), 7)` derives (`"mux:1:7"`),
      proving the reserved fallback form is unreachable from a daemon string.

### Integration Tests

- [ ] **TS-4** (`src-tauri/src/app/tests/agent_status.rs`; covers FR1, AC-4):
      Update `ts5_public_pane_id_map_and_rate_limit_key_are_scoped`
      (line 1094), the end-to-end variant that drives two mux connections
      through `App::on_mux_message` + `pump_all`. Its assertions at lines
      1139-1140 become `"muxpub:<scope0.0>:daemon-a-1"` and
      `"muxpub:<scope1.0>:daemon-b-1"` (derive the expected strings from the
      tabs' `stable_id`s rather than hard-coding numbers — `stable_id` values
      are allocation-order dependent). Keep the `assert_ne!` between the two
      keys, and keep asserting `App::mux_public_pane_id` still returns the
      bare `"daemon-a-1"` / `"daemon-b-1"`.
- [ ] **TS-5** (`src-tauri/src/app/tests/agent_status.rs`; covers FR4, AC-5):
      New `#[test]` in the style of the existing discard tests (lines 427,
      448, and the closed-mux-pane variant near 468): drive a mux pane to a
      learned public id, fire a `Blocked` transition, confirm the immediate
      re-fire is suppressed, then close that pane and confirm the next
      transition fires again. This exercises arm/discard agreement through
      the real call sites without hard-coding any key string, so it stays
      valid whatever the derivation is.
- [ ] **TS-6** (`src-tauri/src/app/tests/agent_status.rs`; covers FR6, AC-6):
      Assert that after ingest, `App::mux_public_pane_id` returns the exact
      daemon string for an id that fails `PublicPaneId::parse` (e.g.
      `"daemon-a-pane1"`), pinning that this feature adds no ingest-time
      validation and no drop path. The existing fixtures at lines 1309 and
      1321 already supply such values.

### E2E Tests

**Existing E2E tests**: None — there is no E2E infrastructure in this project
(TS-7).
**Run command**: Not detected.

- [ ] No E2E scenario is proposed (TS-7).

### Review-Verified Scenarios

- [ ] **TS-7** (no new test; covers FR5, AC-7, NFR4): Doc-comment correctness
      (FR5/AC-7) is verified by review, not by a test — the project has no
      doc-drift test covering these two comments (the only drift tests are
      `ui::dialog::tests` over the design tokens, unrelated here). NFR4 is
      verified by the CLI-only `cargo check`. There is no E2E infrastructure
      in this project, so no E2E scenario is proposed.

### Edge Cases

- [ ] **EC-1:** A daemon sends `public_pane_id = "tab:<stable_id>"` of a live
      plain tab. Post-change the derived key is
      `"muxpub:<scope>:tab:<stable_id>"`, which shares no bucket with that
      tab.
- [ ] **EC-2:** A daemon sends `public_pane_id = "mux:<scope>:<pane_id>"`
      matching another connection's unlearned pane. Post-change the derived
      key carries the `"muxpub:"` prefix and cannot equal that fallback key.
- [ ] **EC-3:** A daemon sends an empty `public_pane_id`. The pane is still
      "learned" (an empty string is stored), so the key is
      `"muxpub:<scope>:"` — distinct per scope and distinct from every other
      form. No panic, no fallthrough.
- [ ] **EC-4:** Two connections learn byte-identical public ids (e.g. two
      daemons that minted the same incarnation token). The scope component
      keeps the keys distinct; this is the case `ts5` (TS-4) already covers.
- [ ] **EC-5:** A pane's public id is learned between the arm and the
      discard, or the map entry is removed before derivation. The existing
      ordering rule (FR4: derive before removing the map entry) is what keeps
      discard aligned; the format change does not alter this hazard, and the
      call-site comments at `agent_status.rs:322-329` and
      `mod.rs:1480-1487` already record it.
- [ ] **EC-6:** A transition drains for a pane whose owning tab has already
      closed. The title resolves to `None` and the caller falls back to an
      empty title; the key derivation still succeeds via the FR2 fallback
      because the map entry is gone.
- [ ] **EC-7:** A `public_pane_id` containing `':'` or arbitrary Unicode. It
      is embedded verbatim after the `"muxpub:<scope>:"` prefix; ambiguity
      inside the suffix is harmless because disjointness is established by
      the prefix before any daemon bytes, and the key is compared only for
      equality, never parsed back.

### Performance Tests

- [ ] No load or stress test is proposed. NFR2 bounds the change to O(1) per
      call with at most one additional `String` allocation, off the render
      path.

## Security Considerations

- **Trust boundary (SC-1):** The mux daemon is outside the trust boundary (it
  commonly runs on a remote host reached over SSH). Any string it supplies is
  untrusted input and must not be able to name another pane's or tab's
  internal resource.
- **Key-space partitioning (SC-2):** The shared rate-limit key space must
  remain partitioned so that one pane can neither suppress another pane's
  notifications (by consuming its bucket) nor clear another pane's
  rate-limit state (by triggering `discard_agent_notification_state` on its
  key).
- **No new disclosure or resource path (SC-3):** The fix must not create a
  new information-disclosure or resource path: the key stays internal, is
  never logged as an identifier the daemon could use to probe other tabs, and
  is never rendered.
- **Input Validation:** None is performed on the daemon string — it is
  embedded as-is, neither parsed, validated, escaped, nor truncated (FR1),
  and no ingest-time validation is added (FR6). Safety comes from
  namespacing instead: `"muxpub:"` is a safe namespace because `':'` cannot
  appear in a `ConnectionScope`'s `u64` rendering, and the three produced
  forms are distinguished by their literal prefix before any
  daemon-controlled bytes appear (a2).
- **Authentication / Authorization:** Not applicable — the resolved
  requirements define no authentication or authorization surface for this
  change.
- **Data Protection:** The key is never serialized, never persisted and never
  displayed (NFR1, SC-3).
- **XSS / SQL injection / CSRF:** Not applicable — no WebView surface, no
  database and no HTTP request is involved (design step skipped; NFR1).
- **Residual risk (a4):** A daemon evading its OWN rate limit by minting a
  fresh `public_pane_id` on every update remains out of scope. Namespacing
  prevents one pane from reaching ANOTHER pane's bucket (the cross-victim
  collision); it does not bound how many buckets a daemon can create for
  itself. A compromised daemon can already spam notifications for its own
  panes, so this is not a regression, but it is not closed by this feature
  either.

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| (none) | The derivation has no failure mode and no rejection path: no error code, no HTTP surface and no user-facing message is introduced (FR1, FR6, EC-3). | — | — |

### Error Flow

```
No error path is introduced. Every input reaches one of the three key forms:
  learned id (even empty)  -> "muxpub:{scope}:{id}"   (FR1, EC-3)
  no learned id            -> "mux:{scope}:{pane_id}" (FR2, EC-6)
  plain tab                -> "tab:{id}"              (FR3)
```

## Performance Optimization

### Performance Goals

- The derivation stays O(1) per call with at most one additional `String`
  allocation relative to today's cloned learned id (NFR2).
- It runs once per drained transition and once per discarded pane, never per
  frame, so it introduces no render-path cost (NFR2).

### Optimization Strategies

- None required beyond keeping the derivation allocation-bounded as stated in
  NFR2.

### Caching Strategy

- No caching is introduced. The learned public id map (`mux_public_pane_ids`)
  keeps its current lifetime, and the rate-limit key space stays
  process-local and ephemeral, rebuilt from scratch each run (a1).

## Assumptions

- **a1:** The rate-limit key space is process-local and ephemeral: it lives
  only in `App::agent_notification_rate_limiter`
  (`AgentNotificationRateLimiter<String>`, `src-tauri/src/app/mod.rs:439`)
  and is rebuilt from scratch each run, so changing the derived format needs
  no migration and cannot invalidate stored data.
- **a2:** `"muxpub:"` is a safe namespace because `':'` cannot appear in a
  `ConnectionScope`'s `u64` rendering, and the three produced forms are
  distinguished by their literal prefix before any daemon-controlled bytes
  appear — so no daemon string can make one form impersonate another
  regardless of its content.
- **a3:** Ingest-time validation is explicitly out of scope. The
  `requirement.fix-approach` answer selected `namespace_learned_key`, not
  `validate_on_ingest`, so `mux_public_pane_ids` keeps learning the raw
  daemon string and `App::mux_public_pane_id` keeps returning it. The answer
  to `requirement.sidebar-public-id` (`drop_unparseable`) was conditional on
  the approach not chosen and therefore constrains nothing in this feature;
  it is recorded for a possible future ingest-validation change only.
- **a4:** A daemon evading its OWN rate limit by minting a fresh
  `public_pane_id` on every update remains out of scope. Namespacing prevents
  one pane from reaching ANOTHER pane's bucket (the cross-victim collision);
  it does not bound how many buckets a daemon can create for itself. A
  compromised daemon can already spam notifications for its own panes, so
  this is not a regression, but it is not closed by this feature either.
- **a5:** The scan-target list supplied by the orchestrator omitted
  `src-tauri/src/app/tab_lifecycle.rs`, which is a fourth call site. This
  analysis assumes that call site is in scope for the change (it needs no
  edit, since it calls the shared function, but it is part of the
  verification surface).

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and tested.
- [ ] All test scenarios (TS-1 through TS-7) pass or, for TS-7, are confirmed
      by review.
- [ ] Performance meets NFR2 (O(1) derivation, no render-path cost).
- [ ] Security requirements SC-1, SC-2 and SC-3 are satisfied.
- [ ] Documentation is complete: both doc comments corrected per FR5/NFR5 and
      asserting no collision property the code does not implement (AC-7).
- [ ] Code review is completed, including the review-verified TS-7 items.
- [ ] The `--no-default-features` CLI-only build still compiles (NFR4).
- [ ] `App::mux_public_pane_id` and the other unaffected surfaces return
      today's values (NFR3, AC-6).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional and non-functional requirement is `resolved`; no
requirement carries `status: tbd`. FR6's earlier `tbd` state (blocked on
`requirement.fix-approach`) was resolved by the `namespace_learned_key`
answer.

## Implementation Phases (if applicable)

Not applicable — the resolved requirements define no phased rollout. The
change is a single derivation edit plus two doc-comment corrections and the
accompanying test updates.

## References

- Requirements document: `feature-docs/public-pane-id-rate-limit-key/REQUIREMENTS.md`
- Derivation and doc comment: `src-tauri/src/app/agent_status.rs:83-97`,
  `:98-113`
- Public-id accessor: `src-tauri/src/app/agent_status.rs:155-163`
- Ingest path: `src-tauri/src/app/agent_status.rs:310-311`
- Call sites: `src-tauri/src/app/agent_status.rs:328`, `:359`,
  `src-tauri/src/app/mod.rs:1484`, `src-tauri/src/app/tab_lifecycle.rs:148`
- Rate limiter field and doc: `src-tauri/src/app/mod.rs:434-439`
- Ordering comments: `src-tauri/src/app/agent_status.rs:322-329`,
  `src-tauri/src/app/mod.rs:1480-1487`
- Tests: `src-tauri/src/app/tests/agent_status.rs`
- GUI feature gating: `.claude/rules/core-architecture.md`
