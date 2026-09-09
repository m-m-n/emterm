# Implementation Plan: mux-rate-limit-key-pane-identity

## Overview

A mux pane's agent-notification rate-limit identity becomes a pure function of
code-owned values (connection scope + wire pane id), and the rate limiter gains
expiry eviction inside its record operation so its live-entry count stays
bounded. The daemon-learned identifier is retained, but only as the sidebar's
copy-to-clipboard value — it never reaches throttling identity again.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate. Both affected layers are
  already in the `gui`-gated part of the crate; nothing moves across the feature
  gate.
- **Key libraries**: none added.
- **New dependencies and their licenses**: none. No dependency is introduced, so
  there is nothing to check against `project.license` (MIT) and no license line
  to record. The license surface is unchanged by this feature.

## Layer Structure

| Layer | Location | Responsibility after this feature |
|---|---|---|
| app | `src-tauri/src/app/` | Owns pane identity, derives the rate-limit key from a pane identity value, owns the learned-id map and every discard call site |
| notifications | `src-tauri/src/notifications.rs` | Owns the rate limiter and the cooldown constant; generic over its key type and deliberately ignorant of any key shape |
| ui | `src-tauri/src/ui/mux_sidebar` | Sole consumer of the daemon-learned identifier; untouched by this feature |

Allowed dependency directions: app depends on notifications; ui depends on app.
The notifications layer never learns the app layer's key vocabulary, and the ui
layer never reaches the limiter. No new direction is introduced.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| `agent_notification_rate_limit_key` | Derive throttling identity from a pane identity value | Pre: its only parameter is a reference to the pane identity value; it accepts no map and no daemon-supplied value. Post (total, deterministic): a tab pane yields the literal prefix `tab:` followed by the tab id; a mux pane yields the literal prefix `mux:`, the connection scope's numeric value, a colon, and the wire pane id. No other form is ever produced | task0001 owns it; task0002 treats every key as opaque and must not special-case any prefix |
| `AgentNotificationRateLimiter::is_within_limit` | Read-only throttle question | Pre: shared (read-only) access to the limiter. Post: answers true when the key has never fired or when the elapsed time since its last fire is at least the cooldown constant; the limiter's contents are identical before and after the call — no insertion, no refresh, no eviction | task0002 owns it (unchanged); task0001's app-layer arm/probe tests depend on this read-only property |
| `AgentNotificationRateLimiter::record` | Arm the cooldown window for one key and bound the map | Pre: exclusive access; the passed instant is monotonic and not earlier than any recorded instant in practice. Post: the recorded key is present with the passed instant; every other key whose elapsed time relative to that instant is at least the cooldown constant is absent; every key whose elapsed time is below the constant is present with its original instant, unchanged | task0002 owns it; task0001 depends on it through the app-layer arm/discard tests |
| `AgentNotificationRateLimiter::discard` | Close-time removal for a pane that went away | Pre: exclusive access. Post: the named key is absent; every other key is unchanged. Unchanged by this feature | task0002 owns it (unchanged); task0001 exercises it from the three app-layer discard sites |
| `App::mux_public_pane_id` | Sidebar-facing accessor for the daemon-learned identifier | Pre/post unchanged: the learned string verbatim for a learned pane, nothing for an unlearned or released one | task0001 must preserve it byte-for-byte in behaviour; its consumer in the ui layer is out of scope |

## Conventions

- **Key vocabulary**: exactly two live prefixes, `tab:` and `mux:`. `muxpub:` is
  retired — no file under `src-tauri/src/` may emit it, assert it, or describe
  it as a producible form. Removing it from source and from tests is part of the
  same deliverable.
- **Trust-boundary rule**: no byte supplied by the mux daemon may participate in
  a rate-limit key. This is enforced structurally, by what the derivation
  accepts as input, not by a runtime check or a sanitising step.
- **Doc comments are deliverables**: every doc comment that describes the
  derivation or the limiter field must state the two remaining forms and the
  absence of any daemon-supplied input. A doc comment that still describes three
  forms, or a learned id behind a namespace prefix, is a defect.
- **Retired constraint**: no comment anywhere may assert the
  derive-before-removal ordering obligation. Where such a comment exists it is
  rewritten to state the current reason the code is ordered the way it is, or
  removed if there is no longer a reason.
- **Test policy**: a test whose premise disappears is restated as a guard for
  the stronger property that replaced it. Silent deletion is not permitted.
- **Surface policy (NFR4)**: no new dependency, no new public item, no change to
  the `gui` / `--no-default-features` split. Both project build commands and the
  library test command must pass from each task's own worktree.
- **Error handling / logging**: no new error paths and no new log statements.
  Elapsed-time comparison relies on the monotonic clock's saturating behaviour
  for an earlier instant (assumption AS-6), so no guard against a negative
  interval is added.

## Cross-task Design Decisions

### CD-1: Option (a) `always_code_owned_key` is the adopted remediation

Settled in the SPEC (FR3) and not open for re-litigation during implementation.
A live pane's key is a pure function of its connection scope and wire pane id,
so re-minting or rotating the daemon-supplied identifier cannot move the pane to
a different bucket. Superseded-entry eviction (option (b)) is rejected because
evicting the old entry discards the cooldown history without transferring the
timestamp, which would leave the rotated identity immediately allowed — the
exact evasion the feature exists to close. Affected: task0001.

### CD-2: The parameter list is the enforcement mechanism

The derivation stops accepting the learned-id map at all. Two consequences that
the implementation must not weaken into a runtime check:

1. "No daemon-supplied byte reaches the key" becomes a property the compiler
   enforces, which is what makes the security criterion checkable by reading the
   signature rather than by auditing every branch.
2. Every call site is found mechanically: a site that is not updated does not
   compile. This is what closes assumption AS-7 (a possibly-unknown fifth
   derivation site) — the task does not need to enumerate call sites correctly
   in advance, it needs the crate to compile.

Affected: task0001.

### CD-3: Eviction lives only in the record operation, at the read check's own threshold

The pruning predicate is the same threshold and the same comparison the
read-only check uses. Two properties follow and both are required (NFR1):

- Any entry pruned would have answered "allowed" anyway, so no observable
  throttling decision changes.
- A suppressed attempt — one where a gate other than the rate limit rejected the
  notification, so nothing is recorded — neither refreshes nor evicts anything,
  because the read-only check performs no mutation and keeps shared access.

Pruning must not be placed in the read path, and must not use a different
threshold, a grace multiplier, or a size trigger. Affected: task0002.

### CD-4: Expiry and close-time discard coexist

Expiry is additive, not a replacement. Close-time discard remains the only thing
that prevents a key reused shortly after its pane closed from inheriting an
unexpired cooldown; expiry alone would leave that window armed. Neither
mechanism may be removed in favour of the other (FR7). Affected: task0002 owns
the limiter side, task0001 owns the app-layer call sites.

### CD-5: The limiter stays key-shape agnostic

The limiter is generic over its key type and must remain so. It may not inspect,
parse, or branch on the textual shape of a key — including for pruning. This
keeps the two tasks genuinely independent: task0001 may change the key shape
without touching the limiter, and task0002 may change the eviction policy
without knowing what a key looks like. Affected: both tasks.

### CD-6: Parallel decomposition and file ownership

The two tasks have disjoint file sets, so they run fully in parallel with no
ordering and no expected merge conflict. Task0001 owns everything under
`src-tauri/src/app/`; task0002 owns `src-tauri/src/notifications.rs`. Neither
task may edit the other's files; a change that appears to require it is a plan
deviation to report, not to absorb. Their only coupling is the record/discard
contract pinned in Shared Components above, which both implement against
independently.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| An unknown additional derivation call site is missed (AS-7) | Low | Medium | CD-2: the narrowed parameter list makes any missed site a compile error. Both tasks must build cleanly before completion |
| Expiry eviction silently changes an observable throttling decision | Low | High | CD-3 pins the predicate to the read check's own threshold; the decision-neutrality test asserts a still-unexpired sibling survives a record |
| A test whose premise disappears is deleted, losing a security guard | Medium | Medium | The test policy convention and task0001's acceptance criteria require restatement as a guard, with the same count of derivation-focused tests before and after |
| The eviction tests need to observe the limiter's internal map and provoke a new public accessor | Low | Medium | The limiter's tests live beside it, so entry presence is observable without widening the public surface; the surface policy forbids adding a public accessor for test convenience |
| A doc comment is left describing the retired form or the retired ordering | Medium | Low | The doc-comment and retired-constraint conventions make docs a graded deliverable; task0001's criteria include the absence of the retired prefix anywhere it owns |
| The two tasks' branches both touch the same test run and mask each other | Low | Low | Disjoint file sets (CD-6); the integrated library test run in VERIFICATION.md is the joint gate |

## Open Questions

- [ ] NFR2 (no migration surface) has no test scenario: it is verified by
      inspection that a rate-limit key is never serialized, never written to the
      mux wire, and never persisted. Recorded as a coverage gap, not a defect.
- [ ] NFR3 (accepted limits of lazy pruning and unshrunk capacity) has no test
      scenario by design — it records accepted behaviour rather than a
      requirement to verify.
- [ ] NFR4 (build and surface unchanged) is verified by the two project build
      commands rather than by a test scenario id.
