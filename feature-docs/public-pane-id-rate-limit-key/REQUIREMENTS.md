---
title: "public-pane-id-rate-limit-key"
created_date: 2026-09-05
status: draft
---

# public-pane-id-rate-limit-key - Requirements Document

## 1. Overview

### 1.1 Background

The mux daemon is an external process on the far side of a trust boundary (it
commonly runs on a remote host reached over SSH). Today its verbatim
`public_pane_id` string is used as a key in the single shared
agent-notification rate-limit key space, which lets a daemon force a
cross-tab / cross-connection rate-limit collision (BO-1, SC-1).

The code's own documentation also claims a collision property it does not
have: the doc comment states that the existing `"mux:"` prefix keeps the key
from ever colliding with a plain-tab key, which holds only for the fallback
branch and not for the learned-id branch (BO-3, FR5).

### 1.2 Purpose

- BO-1: Remove the cross-tab / cross-connection rate-limit collision a mux
  daemon can currently force.
- BO-2: Keep the fix confined to an internal derived value, so that no wire
  format, no persisted state, and no user-visible identifier changes.
- BO-3: Make the code's own documentation match the collision property it
  claims, so the next reader is not misled into believing the `"mux:"` prefix
  protects the learned-id branch.

### 1.3 Scope

In scope:

- The learned-id branch of the rate-limit key derivation (FR1).
- Preservation of the unlearned-pane fallback key and the plain-tab key
  (FR2, FR3).
- Preservation of the single derivation point across all four call sites
  (FR4).
- Correction of the two stale doc comments (FR5).

Out of scope:

- Ingest-time validation of `public_pane_id`. The chosen fix approach
  (`namespace_learned_key`) places the entire fix in the derivation and none
  of it at ingest, so `mux_public_pane_ids` keeps learning the raw daemon
  string and `App::mux_public_pane_id` keeps returning it (FR6, a3).
- A daemon evading its **own** rate limit by minting a fresh
  `public_pane_id` on every update. Namespacing prevents one pane from
  reaching another pane's bucket; it does not bound how many buckets a daemon
  can create for itself (a4).

## 2. Business Requirements

### 2.1 Business Goals

| ID | Goal |
|----|------|
| BO-1 | Remove the cross-tab / cross-connection rate-limit collision a mux daemon can currently force. The daemon is an external process on the far side of a trust boundary (it commonly runs on a remote host over SSH), and its verbatim `public_pane_id` string is used today as a key in the single shared agent-notification rate-limit key space. |
| BO-2 | Keep the fix confined to an internal derived value, so that no wire format, no persisted state, and no user-visible identifier changes. |
| BO-3 | Make the code's own documentation match the collision property it claims, so the next reader is not misled into believing the `"mux:"` prefix protects the learned-id branch. |

### 2.2 Target Users

| User type | Description |
|-----------|-------------|
| (not defined by the resolved requirements) | The resolved requirements define no user-type breakdown. The feature alters one internal derived string and two doc comments; it has no user-visible surface (`design_step.skipped_reason`), and the rate-limit key is never displayed in the UI (NFR1). |

### 2.3 Expected Effects

- One pane can neither suppress another pane's notifications (by consuming
  its bucket) nor clear another pane's rate-limit state (by triggering
  `discard_agent_notification_state` on its key) (SC-2).
- No migration and no compatibility obligation, because the key is
  internal-only and process-local (NFR1, a1).
- The doc comments in the touched region remain the accurate description of
  the code after the change (NFR5, AC-7).

## 3. Use Cases

### 3.1 Use Case List

| ID | Use case | Actor | Priority |
|----|----------|-------|----------|
| UC01 | Derive the notification rate-limit key for a mux pane whose public id has been learned | eMterm GUI (agent-status transition drain) | (not assigned by the resolved requirements) |
| UC02 | Discard a pane's rate-limit state when the pane closes, is reaped, or its tab is closed | eMterm GUI (close / reap / pane-exit paths) | (not assigned by the resolved requirements) |
| UC03 | Ingest an agent-status batch carrying a daemon-supplied `public_pane_id` | mux daemon (untrusted, outside the trust boundary) | (not assigned by the resolved requirements) |

### 3.2 Use Case Details

#### UC01: Derive the notification rate-limit key for a learned mux pane

**Actor**: the agent-status transition-drain loop
(`src-tauri/src/app/agent_status.rs:359`).

**Preconditions**:

- A public id has been learned for `(scope, pane_id)` in
  `mux_public_pane_ids` (FR6).

**Basic flow**:

1. The caller obtains the key by calling
   `agent_notification_rate_limit_key` — it never constructs the string
   itself (FR4).
2. The `MuxPane` branch finds the learned id and returns
   `format!("muxpub:{}:{}", scope.0, id)` (FR1).
3. The learned string is embedded as-is: neither parsed, validated, escaped,
   nor truncated (FR1).

**Alternative flows**:

- No public id has been learned for `(scope, pane_id)`: the function returns
  `format!("mux:{}:{pane_id}", scope.0)` exactly as today (FR2).
- The pane key is `PaneKey::Tab(id)`: the function returns
  `format!("tab:{id}")` (FR3).

**Postconditions**:

- The derived key equals `"muxpub:" + scope.0 + ":" + the learned string`,
  and never equals the learned string itself (AC-1).
- Repeated derivation for the same scope and pane is stable — identical
  input yields an identical key — so the per-pane rate limit still suppresses
  the second notification within the window (AC-4).

#### UC02: Discard a pane's rate-limit state on close / reap / pane exit

**Actor**: the closed-mux-pane loop
(`src-tauri/src/app/agent_status.rs:328`), the reaped-exited-tab loop
(`src-tauri/src/app/mod.rs:1484`), and `close_tab`
(`src-tauri/src/app/tab_lifecycle.rs:148`).

**Preconditions**:

- A rate-limit window may be armed for the pane.

**Basic flow**:

1. The site derives the key via `agent_notification_rate_limit_key` (FR4).
2. The closed-pane and reaped-tab sites derive the key **before** removing
   the pane's `mux_public_pane_ids` entry (FR4).
3. `discard_agent_notification_state` is applied to the derived key.

**Alternative flows**:

- The map entry was already removed before derivation: the derivation falls
  through to the FR2 fallback and would discard the wrong bucket — which is
  exactly what the ordering constraint in FR4 prevents (EC-5).

**Postconditions**:

- The key derived at notification time for a pane equals the key derived for
  that same pane at close/reap/pane-exit time, so
  `discard_agent_notification_state` still reopens the correct window and
  never reopens another pane's (AC-5).

#### UC03: Ingest an agent-status batch carrying a daemon-supplied public id

**Actor**: the mux daemon (untrusted input, SC-1), via
`apply_agent_status_batch` (`src-tauri/src/app/agent_status.rs:310-311`).

**Preconditions**:

- A mux connection is delivering agent-status updates.

**Basic flow**:

1. `apply_agent_status_batch` inserts `update.public_pane_id` into
   `mux_public_pane_ids` verbatim (FR6).
2. No `mux_ipc::protocol::PublicPaneId::parse` call and no rejection path is
   added by this feature (FR6).
3. `App::mux_public_pane_id`
   (`src-tauri/src/app/agent_status.rs:155-163`) keeps returning that stored
   string unchanged (FR6).

**Alternative flows**:

- The supplied string does not satisfy `PublicPaneId::parse` (for example
  the fixtures `"xyz-7"`, `"daemon-a-1"`, `"daemon-b-1"`,
  `"daemon-a-pane1"`, `"daemon-b-pane1"`): it is still stored and still
  returned verbatim (FR6, AC-6).

**Postconditions**:

- Every reader of the public id — including the mux sidebar — observes
  exactly today's value (FR6, NFR3).

## 4. Functional Requirements

### 4.1 Function List

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| FR1 | Namespace the learned public_pane_id in the derived rate-limit key | resolved | (not assigned by the resolved requirements) |
| FR2 | Unlearned-pane fallback key unchanged | resolved | (not assigned by the resolved requirements) |
| FR3 | Plain-tab key unchanged | resolved | (not assigned by the resolved requirements) |
| FR4 | Single derivation point preserved across every call site | resolved | (not assigned by the resolved requirements) |
| FR5 | Correct the two stale doc comments | resolved | (not assigned by the resolved requirements) |
| FR6 | Ingest-time learning and the public-id query surface stay unchanged | resolved | (not assigned by the resolved requirements) |

### 4.2 Function Details

#### FR1: Namespace the learned public_pane_id in the derived rate-limit key

**Description**: The `MuxPane` branch of `agent_notification_rate_limit_key`
(`src-tauri/src/app/agent_status.rs:98-113`) MUST NOT return the
daemon-supplied string verbatim. When a public id has been learned for
`(scope, pane_id)`, the function MUST return
`format!("muxpub:{}:{}", scope.0, id)` — the `"muxpub:"` literal, the
`ConnectionScope`'s inner `u64`, and then the learned string. The learned
string is embedded as-is; it is neither parsed, validated, escaped, nor
truncated.

**Input**:

- `scope`: `ConnectionScope` — the owning mux connection's scope.
- `pane_id`: `u32` — the pane identifier.
- learned id: `String` — the daemon-supplied `public_pane_id` stored in
  `mux_public_pane_ids`.

**Output**:

- rate-limit key: `String` — `"muxpub:{scope.0}:{learned id}"`.

**Business rules**:

- The learned string is never returned unwrapped (FR5's corrected doc text).
- `"muxpub:"` is a safe namespace because `':'` cannot appear in a
  `ConnectionScope`'s `u64` rendering, and the three produced forms are
  distinguished by their literal prefix before any daemon-controlled bytes
  appear (a2).

**Validation**:

| Item | Rule | Error message |
|------|------|---------------|
| learned `public_pane_id` | No validation is performed; the string is embedded as-is (FR1, FR6) | (none — there is no rejection path) |

**Error cases**:

| Error | Condition | Handling |
|-------|-----------|----------|
| (none defined) | The derivation has no failure mode: an empty learned string still produces `"muxpub:<scope>:"` — no panic, no fallthrough (EC-3) | — |

#### FR2: Unlearned-pane fallback key unchanged

**Description**: When no public id has been learned for `(scope, pane_id)`,
the function MUST keep returning `format!("mux:{}:{pane_id}", scope.0)`
exactly as today. This branch is already scope-qualified and already disjoint
from the plain-tab form; this feature does not touch it.

#### FR3: Plain-tab key unchanged

**Description**: The `PaneKey::Tab` branch MUST keep returning
`format!("tab:{id}")`.

#### FR4: Single derivation point preserved across every call site

**Description**: Every site that needs a rate-limit key MUST continue to
obtain it by calling `agent_notification_rate_limit_key`, so the arm site and
the discard site can never disagree about a pane's key. The four call sites
are `src-tauri/src/app/agent_status.rs:328` (closed-mux-pane loop),
`src-tauri/src/app/agent_status.rs:359` (transition-drain loop),
`src-tauri/src/app/mod.rs:1484` (reaped-exited-tab loop) and
`src-tauri/src/app/tab_lifecycle.rs:148` (`close_tab`). No call site may
construct the string itself.

**Business rules**:

- The existing ordering constraint is preserved: the closed-pane and
  reaped-tab sites MUST derive the key BEFORE removing the pane's
  `mux_public_pane_ids` entry, otherwise the derivation falls through to the
  FR2 fallback and discards the wrong bucket.

#### FR5: Correct the two stale doc comments

**Description**: The doc comment on `agent_notification_rate_limit_key`
(`src-tauri/src/app/agent_status.rs:83-97`) MUST be corrected. Its current
text states that mux panes "prefer the daemon-learned public_pane_id" and
that "the existing 'mux:' prefix keeps that fallback from ever colliding with
a plain-tab key" — a claim that holds only for the fallback branch and
describes exactly the learned-id behaviour this feature changes. The
corrected text MUST state that all three produced forms (`"tab:"`, `"mux:"`,
`"muxpub:"`) are mutually disjoint by construction and that the learned
daemon string is never returned unwrapped. The field doc on
`App::agent_notification_rate_limiter`
(`src-tauri/src/app/mod.rs:434-439`) MUST likewise stop describing a mux key
as "a mux pane's public_pane_id" and instead describe it as a key derived by
`agent_notification_rate_limit_key`.

#### FR6: Ingest-time learning and the public-id query surface stay unchanged

**Description**: This feature adds NO ingest-time validation.
`apply_agent_status_batch` (`src-tauri/src/app/agent_status.rs:310-311`) MUST
keep inserting `update.public_pane_id` into `mux_public_pane_ids` verbatim,
with no `mux_ipc::protocol::PublicPaneId::parse` call and no rejection path.
`App::mux_public_pane_id` (`src-tauri/src/app/agent_status.rs:155-163`) MUST
keep returning that stored string unchanged, so every reader of the public id
— including the mux sidebar — observes exactly today's value. Consequently
the five existing test fixtures whose `public_pane_id` values do not satisfy
`PublicPaneId::parse` (`"xyz-7"`, `"daemon-a-1"`, `"daemon-b-1"`,
`"daemon-a-pane1"`, `"daemon-b-pane1"`) remain valid fixtures and MUST NOT be
rewritten to parseable forms.

**Note**: this requirement was previously `tbd`, blocked on
`requirement.fix-approach`; it was resolved by the `namespace_learned_key`
answer, which places the entire fix in the derivation and none of it at
ingest.

### 4.3 Edge Cases

| ID | Case and expected handling |
|----|----------------------------|
| EC-1 | A daemon sends `public_pane_id = "tab:<stable_id>"` of a live plain tab. Post-change the derived key is `"muxpub:<scope>:tab:<stable_id>"`, which shares no bucket with that tab. |
| EC-2 | A daemon sends `public_pane_id = "mux:<scope>:<pane_id>"` matching another connection's unlearned pane. Post-change the derived key carries the `"muxpub:"` prefix and cannot equal that fallback key. |
| EC-3 | A daemon sends an empty `public_pane_id`. The pane is still "learned" (an empty string is stored), so the key is `"muxpub:<scope>:"` — distinct per scope and distinct from every other form. No panic, no fallthrough. |
| EC-4 | Two connections learn byte-identical public ids (e.g. two daemons that minted the same incarnation token). The scope component keeps the keys distinct; this is the case TS-4 (`ts5`) already covers. |
| EC-5 | A pane's public id is learned between the arm and the discard, or the map entry is removed before derivation. The existing ordering rule (FR4: derive before removing the map entry) is what keeps discard aligned; the format change does not alter this hazard, and the call-site comments at `agent_status.rs:322-329` and `mod.rs:1480-1487` already record it. |
| EC-6 | A transition drains for a pane whose owning tab has already closed. The title resolves to `None` and the caller falls back to an empty title; the key derivation still succeeds via the FR2 fallback because the map entry is gone. |
| EC-7 | A `public_pane_id` containing `':'` or arbitrary Unicode. It is embedded verbatim after the `"muxpub:<scope>:"` prefix; ambiguity inside the suffix is harmless because disjointness is established by the prefix before any daemon bytes, and the key is compared only for equality, never parsed back. |

## 5. Non-Functional Requirements

### 5.1 Performance Requirements

- NFR2: The derivation stays O(1) per call with at most one additional
  `String` allocation relative to today's cloned learned id. It runs once per
  drained transition and once per discarded pane, never per frame, so it
  introduces no render-path cost.
- Response time / throughput / concurrent-connection targets: none are
  specified by the resolved requirements.

### 5.2 Security Requirements

- SC-1: The mux daemon is outside the trust boundary (it commonly runs on a
  remote host reached over SSH). Any string it supplies is untrusted input
  and must not be able to name another pane's or tab's internal resource.
- SC-2: The shared rate-limit key space must remain partitioned so that one
  pane can neither suppress another pane's notifications (by consuming its
  bucket) nor clear another pane's rate-limit state (by triggering
  `discard_agent_notification_state` on its key).
- SC-3: The fix must not create a new information-disclosure or resource
  path: the key stays internal, is never logged as an identifier the daemon
  could use to probe other tabs, and is never rendered.
- Input validation: the daemon string is embedded as-is and is neither
  parsed, validated, escaped, nor truncated (FR1); no ingest-time validation
  is added (FR6). Partitioning is achieved by the literal prefix that
  precedes any daemon-controlled bytes (a2).

### 5.3 Availability Requirements

- No availability targets are specified by the resolved requirements. The
  rate-limit key space is process-local and ephemeral, rebuilt from scratch
  each run (a1).

### 5.4 Maintainability Requirements

- NFR5: The doc comments in the touched region must remain the accurate
  description of the code after the change — no comment may keep asserting a
  collision property the code no longer implements.
- SC-3 constrains logging: the key is never logged as an identifier the
  daemon could use to probe other tabs.

### 5.5 Compatibility Requirements

- NFR1: The rate-limit key is internal-only. It is never serialized to the
  mux wire protocol, never written to `settings.json` or any on-disk state,
  and never displayed in the UI, so the format change carries no
  compatibility obligation and needs no migration.
- NFR3: Behaviour outside the key derivation is unchanged: badge aggregation
  (`App::agent_status_badge_for`, `App::agent_status_pane_badge`), sidebar
  public-id display (`App::mux_public_pane_id`), tab-title resolution
  (`agent_status_pane_tab_title`) and visibility resolution
  (`agent_status_pane_visible`) all keep their current results.
- NFR4: The change stays inside GUI-gated code (`src-tauri/src/app/` is
  behind `#[cfg(feature = "gui")]` per `core-architecture.md`), so the
  `--no-default-features` CLI-only build is unaffected and must keep
  compiling.

## 6. UI/UX Requirements

### 6.1 Screen Requirements

None. The design step is **skipped**: no user-visible surface changes. The
feature alters one internal derived string inside a single existing function
plus two doc comments; it touches no UI layout, no design token, no CSS, no
egui widget and no WebView. The design-step recommendation was skip, and the
`create-spec.design-step` gate resolved to `decide_autonomously`
(batch-decision-table), accepting it.

### 6.2 Screen Transitions

None — no screen is added or altered.

### 6.3 Responsive Behaviour

Not applicable — no rendered surface is involved (NFR1: the key is never
displayed in the UI).

## 7. Data Requirements

### 7.1 Data Model Overview

- `mux_public_pane_ids`: a map from `(ConnectionScope, u32)` to the
  daemon-supplied `public_pane_id` `String`, written verbatim at ingest
  (FR6).
- `App::agent_notification_rate_limiter`:
  `AgentNotificationRateLimiter<String>` (`src-tauri/src/app/mod.rs:439`),
  keyed by the string `agent_notification_rate_limit_key` derives (a1, FR5).

### 7.2 Data Items

| Entity | Item | Type | Required | Description |
|--------|------|------|----------|-------------|
| `mux_public_pane_ids` | key | `(ConnectionScope, u32)` | yes | Scope and pane id of the learned pane (FR1). |
| `mux_public_pane_ids` | value | `String` | yes | The daemon-supplied `public_pane_id`, stored verbatim, never validated (FR6). |
| rate-limit key | value | `String` | yes | One of `"tab:{id}"` (FR3), `"mux:{scope.0}:{pane_id}"` (FR2), `"muxpub:{scope.0}:{learned id}"` (FR1). |

### 7.3 Data Retention

| Data | Retention |
|------|-----------|
| Rate-limit key space | Process-local and ephemeral: it lives only in `App::agent_notification_rate_limiter` and is rebuilt from scratch each run, so changing the derived format needs no migration and cannot invalidate stored data (a1). |
| Derived rate-limit key | Never serialized to the mux wire protocol, never written to `settings.json` or any on-disk state (NFR1). |

## 8. External Integration

### 8.1 Integrated Systems

| System | Integration | Data |
|--------|-------------|------|
| mux daemon (outside the trust boundary; commonly remote over SSH — SC-1) | agent-status batch ingest via `apply_agent_status_batch` (`src-tauri/src/app/agent_status.rs:310-311`) | `update.public_pane_id`, stored verbatim (FR6) |

### 8.2 API Requirements

No wire-protocol change. The rate-limit key is never serialized to the mux
wire protocol (NFR1), and the ingest path is unchanged (FR6).

## 9. Constraints

### 9.1 Technical Constraints

- The change stays inside GUI-gated code (`src-tauri/src/app/` behind
  `#[cfg(feature = "gui")]`); the `--no-default-features` CLI-only build must
  keep compiling (NFR4).
- The fix must be confined to an internal derived value: no wire format, no
  persisted state, no user-visible identifier may change (BO-2, NFR1).
- Every call site must keep obtaining the key from
  `agent_notification_rate_limit_key`, and the closed-pane and reaped-tab
  sites must derive before removing the map entry (FR4).

### 9.2 Business Constraints

- The five existing fixtures whose `public_pane_id` values fail
  `PublicPaneId::parse` must not be rewritten to parseable forms (FR6).
- Ingest-time validation is explicitly excluded by the selected fix approach
  (a3).

### 9.3 Schedule Constraints

None stated by the resolved requirements.

### 9.4 Declared Change Set

Feature-specific paths are not enumerated by hand: they are derived at
create-plan from every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

**Default members** (always part of the declaration unless the SPEC author
explicitly removes them; neither is removed here):

- `feature-docs/public-pane-id-rate-limit-key/**`
- `test-docs/public-pane-id-rate-limit-key/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces; the generating owners are the phase
documents and `references/phase-state.md` (cited, not restated).

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`,
generated and owned by `implement-phase.md` (cited, not restated).

**Semantics**: the declaration is a SUPERSET assertion — the actual change
set must be CONTAINED IN the declared set, and a declared path that never
materializes is not a violation.

### 9.5 Assumptions

| ID | Assumption |
|----|------------|
| a1 | The rate-limit key space is process-local and ephemeral: it lives only in `App::agent_notification_rate_limiter` (`AgentNotificationRateLimiter<String>`, `src-tauri/src/app/mod.rs:439`) and is rebuilt from scratch each run, so changing the derived format needs no migration and cannot invalidate stored data. |
| a2 | `"muxpub:"` is a safe namespace because `':'` cannot appear in a `ConnectionScope`'s `u64` rendering, and the three produced forms are distinguished by their literal prefix before any daemon-controlled bytes appear — so no daemon string can make one form impersonate another regardless of its content. |
| a3 | Ingest-time validation is explicitly out of scope. The `requirement.fix-approach` answer selected `namespace_learned_key`, not `validate_on_ingest`, so `mux_public_pane_ids` keeps learning the raw daemon string and `App::mux_public_pane_id` keeps returning it. The answer to `requirement.sidebar-public-id` (`drop_unparseable`) was conditional on the approach not chosen and therefore constrains nothing in this feature; it is recorded for a possible future ingest-validation change only. |
| a4 | A daemon evading its OWN rate limit by minting a fresh `public_pane_id` on every update remains out of scope. Namespacing prevents one pane from reaching ANOTHER pane's bucket (the cross-victim collision); it does not bound how many buckets a daemon can create for itself. A compromised daemon can already spam notifications for its own panes, so this is not a regression, but it is not closed by this feature either. |
| a5 | The scan-target list supplied by the orchestrator omitted `src-tauri/src/app/tab_lifecycle.rs`, which is a fourth call site. This analysis assumes that call site is in scope for the change (it needs no edit, since it calls the shared function, but it is part of the verification surface). |

## 10. Anticipated Issues and Risks

### 10.1 Technical Issues

| Issue | Impact | Mitigation |
|-------|--------|------------|
| Deriving the key after removing the `mux_public_pane_ids` entry falls through to the FR2 fallback and discards the wrong bucket (EC-5) | (not rated by the resolved requirements) | Preserve the existing ordering constraint: derive before removing the map entry (FR4); the call-site comments at `agent_status.rs:322-329` and `mod.rs:1480-1487` already record it. |
| A call site constructing the key string itself would let the arm site and the discard site disagree (FR4) | (not rated) | Keep `agent_notification_rate_limit_key` the single derivation point across all four call sites (FR4), verified end-to-end by TS-5. |
| `src-tauri/src/app/tab_lifecycle.rs` was omitted from the orchestrator's scan-target list (a5) | (not rated) | Treat that call site as part of the verification surface; it needs no edit because it calls the shared function (a5, FR4). |
| Doc comments could keep asserting a collision property the code no longer implements (NFR5) | (not rated) | FR5 corrects both comments; verified by review, since no doc-drift test covers them (TS-7). |

### 10.2 Business Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| A daemon evades its own rate limit by minting a fresh `public_pane_id` per update | (not rated by the resolved requirements) | (not rated) | Out of scope and not a regression: a compromised daemon can already spam notifications for its own panes (a4). |

## 11. Success Criteria

### 11.1 Acceptance Criteria

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
- [ ] AC-6: `App::mux_public_pane_id` still returns the raw learned string
      for every pane, unchanged by this feature, including for the five
      fixtures that fail `PublicPaneId::parse`.
- [ ] AC-7: The doc comment on `agent_notification_rate_limit_key` and the
      field doc on `agent_notification_rate_limiter` both describe the
      post-change behaviour; neither retains the claim that the `"mux:"`
      prefix protects the learned-id branch.

### 11.2 KPI

| Metric | Target | Measurement |
|--------|--------|-------------|
| (none defined) | — | The resolved requirements define no KPI. |

## 12. Test Scenarios

### 12.1 Test Perspectives

| ID | File | Covers | Scenario |
|----|------|--------|----------|
| TS-1 | `src-tauri/src/app/tests/agent_status.rs` | FR1, FR2, FR3, AC-1 | Extend the existing `agent_notification_rate_limit_key_prefers_public_pane_id_falls_back_to_prefixed_id` (line 634) — a plain `#[test]` over a hand-built `HashMap<(ConnectionScope, u32), String>`, no `App` needed. With `ids[(ConnectionScope(1), 7)] = "xyz-7"`, assert the `MuxPane(scope_a, 7)` key is `"muxpub:1:xyz-7"` (today's assertion of `"xyz-7"` at line 647 is the one that changes). Keep asserting `"mux:1:8"` for the unlearned pane and `"tab:3"` for the plain tab, unchanged. |
| TS-2 | `src-tauri/src/app/tests/agent_status.rs` | FR1, AC-2 | New `#[test]`, same hand-built-map style: insert a hostile learned id `"tab:5"` for `MuxPane(ConnectionScope(9), 1)` and assert the derived key is `"muxpub:9:tab:5"` and `assert_ne!` against `agent_notification_rate_limit_key(&ids, &PaneKey::Tab(5))` — i.e. against `"tab:5"`. |
| TS-3 | `src-tauri/src/app/tests/agent_status.rs` | FR1, FR2, AC-3 | New `#[test]`: insert a hostile learned id `"mux:1:7"` for `MuxPane(ConnectionScope(9), 1)` and `assert_ne!` against the key an unlearned `MuxPane(ConnectionScope(1), 7)` derives (`"mux:1:7"`), proving the reserved fallback form is unreachable from a daemon string. |
| TS-4 | `src-tauri/src/app/tests/agent_status.rs` | FR1, AC-4 | Update `ts5_public_pane_id_map_and_rate_limit_key_are_scoped` (line 1094), the end-to-end variant that drives two mux connections through `App::on_mux_message` + `pump_all`. Its assertions at lines 1139-1140 become `"muxpub:<scope0.0>:daemon-a-1"` and `"muxpub:<scope1.0>:daemon-b-1"` (derive the expected strings from the tabs' `stable_id`s rather than hard-coding numbers — `stable_id` values are allocation-order dependent). Keep the `assert_ne!` between the two keys, and keep asserting `App::mux_public_pane_id` still returns the bare `"daemon-a-1"` / `"daemon-b-1"`. |
| TS-5 | `src-tauri/src/app/tests/agent_status.rs` | FR4, AC-5 | New `#[test]` in the style of the existing discard tests (lines 427, 448, and the closed-mux-pane variant near 468): drive a mux pane to a learned public id, fire a `Blocked` transition, confirm the immediate re-fire is suppressed, then close that pane and confirm the next transition fires again. This exercises arm/discard agreement through the real call sites without hard-coding any key string, so it stays valid whatever the derivation is. |
| TS-6 | `src-tauri/src/app/tests/agent_status.rs` | FR6, AC-6 | Assert that after ingest, `App::mux_public_pane_id` returns the exact daemon string for an id that fails `PublicPaneId::parse` (e.g. `"daemon-a-pane1"`), pinning that this feature adds no ingest-time validation and no drop path. The existing fixtures at lines 1309 and 1321 already supply such values. |
| TS-7 | (no new test) | FR5, AC-7, NFR4 | Doc-comment correctness (FR5/AC-7) is verified by review, not by a test — the project has no doc-drift test covering these two comments (the only drift tests are `ui::dialog::tests` over the design tokens, unrelated here). NFR4 is verified by the CLI-only `cargo check`. There is no E2E infrastructure in this project, so no E2E scenario is proposed. |

Coverage by category:

- [ ] Happy path: TS-1 (learned / unlearned / plain-tab forms), TS-5
      (arm and discard agree through the real call sites).
- [ ] Adversarial input: TS-2 (`"tab:5"`), TS-3 (`"mux:1:7"`).
- [ ] Boundary: TS-4 (two connections learning ids for their own panes;
      scope component keeps them distinct), EC-3 (empty `public_pane_id`),
      EC-7 (`':'` or arbitrary Unicode in the id).
- [ ] Security: TS-2, TS-3 (SC-1, SC-2 — a daemon string cannot name another
      pane's or tab's bucket).
- [ ] Performance: no performance test is proposed; NFR2 keeps the
      derivation O(1) off the render path.
- [ ] Build gating: NFR4 is verified by the CLI-only `cargo check` (TS-7).

## 13. Glossary

| Term | Definition |
|------|------------|
| `public_pane_id` | The daemon-supplied string a mux connection reports for a pane; stored verbatim in `mux_public_pane_ids` at ingest (FR6). |
| `ConnectionScope` | The per-connection scope whose inner `u64` (`scope.0`) is rendered into the derived key (FR1, FR2). |
| rate-limit key | The internal `String` returned by `agent_notification_rate_limit_key` and used as the key of `App::agent_notification_rate_limiter` (a1, NFR1). |
| `"muxpub:"` | The literal prefix introduced by FR1 for the learned-id form; a safe namespace because `':'` cannot appear in a `ConnectionScope`'s `u64` rendering (a2). |
| mux daemon | The external process that supplies agent-status updates; outside the trust boundary, commonly running on a remote host reached over SSH (SC-1). |
| arm / discard | Arming is registering a pane's rate-limit window at notification time; discarding is `discard_agent_notification_state` on that pane's key at close/reap/pane-exit time (AC-5, FR4). |

## 14. Confirmations

### 14.1 Confirmed Items

- [x] `requirement.fix-approach`: `namespace_learned_key` — the entire fix
      lives in the derivation and none of it at ingest (a3, FR6).
- [x] `requirement.sidebar-public-id`: `drop_unparseable` — conditional on
      the approach that was not chosen, so it constrains nothing in this
      feature; recorded for a possible future ingest-validation change only
      (a3).
- [x] `create-spec.design-step`: `decide_autonomously`
      (batch-decision-table), accepting the skip recommendation; the design
      step is skipped because there are no user-visible surface changes
      (`design_step`).

### 14.2 Unconfirmed / Deferred Items

- All functional and non-functional requirements are `resolved`; none is
  `tbd`.
- Deliberately excluded, not deferred: ingest-time validation (a3) and a
  daemon evading its own rate limit by minting fresh ids (a4).

## 15. References

- `src-tauri/src/app/agent_status.rs:83-97` — doc comment on
  `agent_notification_rate_limit_key` (FR5).
- `src-tauri/src/app/agent_status.rs:98-113` — the derivation itself (FR1,
  FR2, FR3).
- `src-tauri/src/app/agent_status.rs:155-163` — `App::mux_public_pane_id`
  (FR6).
- `src-tauri/src/app/agent_status.rs:310-311` —
  `apply_agent_status_batch` ingest (FR6).
- `src-tauri/src/app/agent_status.rs:322-329`, `:328` — closed-mux-pane loop
  and its ordering comment (FR4, EC-5).
- `src-tauri/src/app/agent_status.rs:359` — transition-drain loop (FR4).
- `src-tauri/src/app/mod.rs:434-439` — `App::agent_notification_rate_limiter`
  field doc and type (FR5, a1).
- `src-tauri/src/app/mod.rs:1480-1487`, `:1484` — reaped-exited-tab loop and
  its ordering comment (FR4, EC-5).
- `src-tauri/src/app/tab_lifecycle.rs:148` — `close_tab` call site (FR4, a5).
- `src-tauri/src/app/tests/agent_status.rs` — the test file all of TS-1..TS-6
  target.
- `.claude/rules/core-architecture.md` — the `#[cfg(feature = "gui")]`
  gating NFR4 relies on.
- `SPEC.md` — the implementation-facing rendering of these requirements.
