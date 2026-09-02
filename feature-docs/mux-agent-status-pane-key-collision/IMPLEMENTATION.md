# Implementation Plan: mux-agent-status-pane-key-collision

## Overview

The GUI-side agent-status key for mux panes gains a connection-scope component,
so two panes that share a daemon-local wire `pane_id` on two different mux
daemons stop collapsing onto one entry. The change is confined to the GUI
process; the mux wire and the daemon are untouched.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate, `gui` feature (default-on).
- **Key libraries**: none added. This feature introduces **no new dependency**,
  so `project.license: MIT` is unaffected and no dependency-license entry needs
  recording.
- **Test framework**: the crate's built-in test harness, inline `#[cfg(test)]`
  modules next to the code (repository convention; SPEC assumption A3).

## Layer Structure

Layers this feature touches, and the dependency directions it must preserve
(no new direction is introduced):

| Layer | Location | Responsibility in this feature |
|---|---|---|
| wire | `crates/mux_ipc` | daemon message types — READ-ONLY, byte-for-byte unmodified (NFR1) |
| connection | `src-tauri/src/tabs/` | one mux connection per tab; routes daemon frames, latches pending status updates and closed panes |
| application state | `src-tauri/src/app/` | drains the per-tab latches, derives keys, applies status, resolves badges / notification titles / rate-limit keys, discards on close |
| model | `src-tauri/src/agent_status_model.rs` | the keyed store of per-pane agent status |
| render | `src-tauri/src/render/`, `src-tauri/src/ui/` | consumes already-aggregated badge values only |

Allowed directions: render → application state → model, and application state →
connection. `crates/mux_ipc` stays a read-only dependency of the connection and
application-state layers. The connection layer never reads application state.

## Shared Components

This feature is a single task (decision D1 below), so no component is handed
from one task to another. The table records the contracts every touched consumer
implements against, so review and verify have one normative statement of them.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Scoped mux pane key | Identify one agent-status pane by (connection scope, wire `pane_id`) | **Pre**: the scope is available for every attached pane from attach time onward, including a pane that has never reported a status. **Post**: equal scope AND equal wire `pane_id` is the same key; equal wire `pane_id` with differing scope is two distinct keys; the plain-tab key is unchanged; lookup remains a single keyed hash lookup | task0001 |
| Connection scope value | Name the GUI-local mux connection a pane belongs to | **Pre**: `Tab::stable_id` of the tab whose PTY runs the mux attach that delivered the frame. **Post**: constant for the tab's whole lifetime, including across detach and re-attach; never transmitted on the wire; never rendered to the user | task0001 |
| Tagged mux drain | Carry the originating tab's scope out of the per-tab latches | **Pre**: each drained element came from exactly one tab. **Post**: every drained pending update and every drained closed pane reaches the batch apply paired with its originating tab's scope; the batch apply derives no key from an untagged wire `pane_id` | task0001 |
| Scoped public-pane-id lookup | Map (connection scope, wire `pane_id`) to the daemon-minted public pane id | **Pre**: the value is learned from an update already tagged with its scope. **Post**: learn, refresh and removal affect only the queried scope; another scope's entry for the same wire `pane_id` is never read, overwritten or removed | task0001 |
| Rate-limit key derivation | Produce one notification rate-limit key per pane | **Pre**: the caller supplies the pane's connection scope and wire `pane_id`. **Post**: derived from the scoped lookup above; when no public id was ever learned, the fallback key still distinguishes scopes; the same pane yields the same key at every call site; two scopes never yield the same key | task0001 |
| Scoped tab resolution | Resolve the tab that owns a transition, for the notification title and for visibility | **Pre**: the transition carries its connection scope. **Post**: resolves to the tab whose own connection reported it, or to no tab at all when that tab is gone — never to another tab that merely holds the same wire `pane_id` | task0001 |

## Conventions

- **Naming**: name the added component after the *connection* it identifies, not
  after the tab that currently supplies it, so the name survives if a dedicated
  mux client object later replaces the tab as the connection owner.
- **Error handling**: no new error path. Absence stays absence — when a
  transition's owning tab is gone, the resolution yields no tab and the caller
  keeps its existing empty-title fallback (EC-4). Never substitute a different
  scope's tab as a fallback.
- **Logging**: unchanged. If the scope is logged at all, log it only next to the
  wire `pane_id` as a diagnostic pair; it is a process-local counter value and
  is never a user-facing identifier.
- **Doc comments**: any doc comment that states the old keying rule is part of
  the change, not a leftover — the two comments named in NFR4 must describe the
  new scoping rule (maintainability gate VC-3).

## Cross-task Design Decisions

### D1 — The feature is one task, not several

Reshaping the mux key changes a type that every consumer names, so all
consumers stop compiling the moment the shape changes. Splitting the work would
produce task worktrees that cannot compile and cannot run their own tests,
which violates worktree independence — the decomposition rule that outranks the
per-task size heuristic. The task's acceptance criteria are therefore grouped by
requirement cluster (one criterion per FR cluster) rather than split across
tasks. Affected: task0001.

### D2 — The connection scope is the tab's stable id

Fixed by SPEC assumption A1 / FR1. Two alternatives are explicitly rejected:

- the daemon-minted public pane id — not available until the daemon's first
  status update, whereas every attached pane needs a scope from attach time;
- the mux session name or the mux window group — neither carries any
  daemon-distinguishing identity.

Affected: task0001 (every key derivation).

### D3 — Detach does not discard the tab's mux entries (EC-2 resolution)

SPEC leaves this to the plan step. **Decision: do not add a detach-time
discard in this feature.** After detach the tab's mux group is cleared, so its
pane entries become unreachable and a re-attach that reuses the same wire
`pane_id` in the same tab can surface the pre-detach state until the daemon's
first update arrives. That behavior is identical before and after the scoping
change (pre-fix the key did not depend on the tab at all, so the same entry was
reached the same way), so this feature neither introduces nor worsens it.
Adding a discard would change detach semantics beyond every FR in scope and
would put an untested behavioral change into a defect fix. Recorded as an open
question for a follow-up. Affected: task0001 (explicitly out of scope there).

### D4 — The rate-limit fallback key keeps its namespace and gains the scope

When a pane is discarded before its public pane id was ever learned, the
fallback key must stay distinguishable in two directions at once: across
connection scopes, and against the plain-tab key namespace. Keep the existing
mux namespace prefix and add the scope alongside the wire `pane_id`, so neither
collision is possible. All four rate-limit call sites derive the key through the
same single derivation — none of them re-implements it. Affected: task0001.

### D5 — Nothing crosses the wire

`crates/mux_ipc` and the daemon-side pane-id allocator are read-only for this
feature (NFR1, AC-8). The scope is minted, stored and read entirely inside the
GUI process, so a fixed GUI keeps working against an unmodified — and an
older — daemon, and no new information is exposed between servers. Affected:
task0001; verified by VC-1.

### D6 — The scoped key stays a hash key

The per-frame badge reads and the key-event-time agent-window cycle must remain
single keyed lookups (NFR2). No read path may scan across tabs to find a pane's
owner, and the per-frame path adds no allocation beyond the key-set build that
already happens. Affected: task0001; verified by VC-2.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Partially applied scoping — one consumer still keys on the bare wire `pane_id` | Medium | High | The key shape change makes the compiler reject most of them; the remaining judgment calls (fallback key, tab resolution, discard loops) each have their own acceptance criterion naming the exact path |
| Plain-tab regression from touching the shared model | Medium | Medium | NFR3 + TS-10: the existing plain-tab tests must pass with their original expectations. A changed plain-tab expectation is a defect signal, never part of the fix |
| Per-frame cost regression on the badge read paths | Low | Medium | D6 + VC-2: read paths stay keyed lookups with no cross-tab scan |
| A pre-existing non-deterministic replay test is mistaken for a regression | Medium | Low | VERIFICATION.md records the single-test-thread re-run rule before any replay-test failure is treated as caused by this change |
| Two tabs attached to the same daemon pane produce two model entries (EC-1) | Low | Low | Accepted: duplication, not contamination — each tab's badge still tracks that pane. The only affected reader is the model's global count, which has no non-test caller |

## Open Questions

- [ ] EC-2 follow-up: should a tab discard its mux pane entries at detach, so a
      re-attach starts from a clean badge and no unreachable entry is left
      behind? Decided out of scope here (D3); the behavior is unchanged by this
      feature.
- [ ] NFR1, NFR2 and NFR4 are verified by the non-test checks VC-1..VC-3 in
      VERIFICATION.md rather than by a TS scenario, so their workflow.yaml
      `tests` arrays stay empty by design rather than by omission.
