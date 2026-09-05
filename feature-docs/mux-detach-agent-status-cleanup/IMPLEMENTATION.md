# Implementation Plan: mux-detach-agent-status-cleanup

## Overview

A tab that leaves mux mode through a daemon-confirmed detach currently keeps
the departing connection's agent-status entries, scoped public-pane-id
mappings and notification rate-limit state alive until process exit, so the
tab badge and mux sidebar show the previous connection after a re-attach.
This plan releases that state at detach by feeding the departing group's wire
pane ids into the teardown chain the tab-close and pane-exit paths already
use, and corrects the connection-scope doc comment that asserts a lifetime
property the implementation does not provide.

## Technology Stack

- **Language**: Rust — the GUI-gated modules of the `emterm` binary crate
  (`src-tauri/`). Toolchain and edition are unchanged.
- **Key libraries**: none. The change touches only in-crate state structures
  that already exist; the mux wire types are consumed, not modified.
- **New dependencies**: none. No crate is added, so no license review is
  triggered and `project.license` (MIT) is unaffected. Every dependency this
  work relies on is already declared and already reviewed.

## Layer Structure

| Layer | Responsibility | May depend on |
|-------|----------------|---------------|
| render / ui (`render`, `ui::tab_bar`, `ui::mux_sidebar`) | Read-only per-frame projection of badge state | App |
| App (`app::mod`, `app::agent_status`, `app::mux_ui`, `app::tab_lifecycle`) | Owns the three per-pane state stores (model, scoped public-pane-id map, notification rate-limit map); drains every tab's latches once per pump and performs the actual release | Tab, AgentStatusModel |
| Tab (`tabs::mod`, `tabs::mux_link`) | Decodes mux frames for one tab and records the consequences on per-tab latches | AgentStatusModel types only (for key/latch value shapes) |
| AgentStatusModel (`agent_status_model`) | Owns entries keyed by pane key, the per-tab inferred-clear latch, and the transition queue | — |

Allowed dependency direction is downward only. The tab layer has **no**
mutable App access at mux-frame handling time; it must never acquire one for
this feature (NFR4). Anything the tab layer needs the App layer to do is
expressed by pushing onto a per-tab latch that the pump drains.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Closed-pane latch (`Tab::pending_closed_agent_status_panes` + its drain `Tab::take_closed_agent_status_panes`) | Carry "these wire pane ids have left this tab's mux group" from the tab layer to the pump | **Pre**: the caller pushes wire pane ids belonging to THIS tab's group, while the group is still reachable (i.e. before the group is cleared). **Post**: one drain returns every queued id in push order and leaves the latch empty; a second drain in the same pump returns nothing. Pushing an id that no longer has state is permitted. | task0001 |
| Closed-panes teardown (the `closed_panes` arm of the App batch apply) | Release all three pieces of per-pane state for one (owning tab, wire pane id) pair | **Pre**: each pair names the tab that OWNS the pane; the scoped public-pane-id mapping has not yet been removed for that pair. **Post**: the notification rate-limit identity is resolved FIRST from the still-present mapping, then the mapping entry, the rate-limit entry, and the model entry for the mux-pane key in the owning tab's scope are all removed. Repeating the operation for an already-released pane, or for a pane that never had a mapping, is a no-op and never an error. | task0001 |
| Connection scope value | Distinguish two daemons' identically-numbered wire panes | **Pre/Post**: derived from the owning tab's stable id at every derivation site, unchanged by this feature. The value is constant for a tab; the ENTRIES it keys are not — they are discarded when the tab leaves mux mode and re-minted on the next attach. | task0001 |
| Per-tab agent-status key set (`agent_status_keys_for_tab`) | Enumerate the keys a tab occupies (its own plain-tab key plus one mux-pane key per group pane) | **Pre**: the tab's group, if any, is intact. **Post**: keys are derived from this tab's own scope only, so two tabs' key sets are disjoint even when their groups hold the same wire pane id. Used unchanged by the tab-close and reaped-tab paths. | task0001 |

## Conventions

- **No parallel teardown**: releasing per-pane agent-status state has exactly
  one implementation — the closed-panes teardown above. Any new mux-exit
  path feeds it rather than reimplementing the release.
- **Latch, never reach upward**: a tab-layer handler that needs App-owned
  state changed records it on a latch. Introducing a model reference or an
  App borrow into the tab layer is out of bounds.
- **Idempotent release**: discarding a pane that has no entry, no mapping, or
  no rate-limit record is a silent no-op. Callers are not required to
  de-duplicate the ids they queue.
- **Ordering rule**: the rate-limit identity of a pane is resolved BEFORE its
  public-pane-id mapping is removed. Any code motion that reorders these two
  steps silently changes which rate-limit record is released.
- **Error handling**: this feature adds no error type, no error code and no
  user-visible failure surface. The two edge conditions (double release,
  never-learned public id) are no-ops by contract, not error paths.
- **Logging**: the mux-exit paths already log at info level. Diagnostics for
  the discarded pane ids belong on the existing detach log statement; no new
  log site is introduced, and no log line is added below warn level with the
  expectation of being readable in a release build.
- **Documentation as a contract**: a doc comment that states a lifetime
  property must state the property the code provides. When a lifetime claim
  and the implementation disagree, the doc is corrected here rather than the
  claim being made true by new machinery.

## Cross-task Design Decisions

### D1: Detach feeds the existing teardown chain

The daemon-confirmed detach handler queues the departing group's wire pane
ids onto the closed-pane latch instead of gaining its own release routine.
The pump then drains it exactly as it drains the pane-exit path's queue.

Flow, in order:

1. The daemon-confirmed detach frame reaches the tab's mux-frame handler.
2. The handler reads the wire pane ids of the group it is about to drop and
   pushes each onto the closed-pane latch.
3. The handler clears the group and the session name and performs the
   existing screen-restoration steps, unchanged.
4. The next pump drains the latch and tags each id with the owning tab's
   stable id.
5. The batch apply's closed-panes arm resolves each pane's rate-limit
   identity, then removes the mapping entry, the rate-limit entry and the
   model entry.

**Rationale**: the release semantics, the scoping guarantee and the ordering
rule already live in one place and are already covered by the tab-close and
reaped-tab paths. A second implementation would have to re-derive all three.

**Affected**: task0001.

### D2: Queue before clearing, not after

Step 2 above must precede step 3. Once the group is cleared, the pane ids it
held are unrecoverable and the release obligation is silently lost — which is
exactly the defect being fixed.

**Affected**: task0001.

### D3: Connection scope derivation is unchanged; its documentation is corrected

The scope stays derived from the owning tab's stable id at every derivation
site. No attach-generation counter is introduced: with D1 in place, the
entries a re-attach could collide with no longer exist by the time the new
connection reports. The doc comment is corrected to describe the two facts
that are actually true — the scope VALUE is constant for the tab, and the
entries it keys are released when the tab leaves mux mode.

**Rationale**: a generation counter would change every derivation site and
add a second identity to keep synchronized, for a collision that D1 removes
at its source.

**Affected**: task0001.

### D4: Only mux-pane keys are released at detach

Detach releases mux-pane keys in the detaching tab's scope. The tab's own
plain-tab key and the per-tab inferred-clear latch that the model removes
alongside a plain-tab key both survive, because the tab reverts to a plain
tab that keeps reporting its own status. This follows structurally from D1:
the closed-panes teardown only ever constructs mux-pane keys, so no
additional guard is needed — but the property is asserted by test rather than
assumed.

**Affected**: task0001.

### D5: The pane-exit path is not touched

The pane-exit handler already queues each removed pane id, and the
group-empties branch inside it reaches its own clearing of the group without
going through the detach handler. Adding the queueing to the detach handler
therefore leaves the pane-exit path pushing each id exactly once. Any
refactor that routes the pane-exit path through the detach handler would
double-push and must not be undertaken here.

**Affected**: task0001.

### D6: Scope of the change set

Only the GUI-gated modules under `src-tauri/src` change. The mux wire
protocol crate, daemon behaviour and user-visible settings are untouched, so
the CLI-only build is unaffected by construction rather than by inspection.

**Affected**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The same pane id reaches the teardown twice in one pump (a pane exit and a detach coinciding) | Medium | Low | The release is idempotent by contract (Conventions); the second pass is a no-op. Asserted by the drain-once test rather than by de-duplicating at the queueing site. |
| The release reaches the tab's own plain-tab entry, blanking a plain tab's status after detach | Low | High | D4: the teardown constructs mux-pane keys only. Covered by a dedicated scenario that sets the plain-tab status before detaching. |
| The rate-limit identity is resolved after the mapping is removed, releasing the fallback key instead of the learned one | Low | Medium | The ordering rule is stated in Conventions and the existing teardown already satisfies it; the plan forbids reordering rather than rewriting the arm. |
| Queueing placed after the group is cleared, making the fix a silent no-op | Low | High | D2 states the ordering explicitly; the tab-level scenario asserts the drain returns the group's ids, which fails if the order is wrong. |
| A detach on one tab reaches another tab's identically-numbered wire pane | Low | High | Every key is derived from the detaching tab's own scope (Shared Components). Covered by the two-tab scenario. |

## Open Questions

- [ ] NFR2 (no wire-protocol / daemon / user-visible-setting change) and NFR5
      (CLI-only build unaffected) have no scenario-level test in the SPEC's
      TS set. They are verified by the change-set containment check and by
      the CLI-only build command in VERIFICATION.md, so their requirement
      `tests` arrays stay empty rather than being padded with an unrelated
      scenario id.
