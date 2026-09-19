# Implementation Plan: mouse-report-guard-local-arms

## Overview

Replace the single uniform rejection answer the SC-8 grid-ownership guard gives
today with an explicit per-region decision that names the pre-regression local
behaviour wherever one existed, and pin each region's decided disposition in the
test suite so the arm identity can no longer be lost silently.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate, `gui` feature. The change
  is confined to `src-tauri/src/window_host/`.
- **Test harness**: the crate's built-in unit-test harness, in the inline test
  module that already lives beside the decision units. No test framework crate is
  introduced.
- **New dependencies**: none. No crate is added, removed or upgraded by this
  feature, so the dependency-license set is unchanged and the project license
  (MIT) gains no new obligation. There is consequently no new-dependency license
  line to record beyond this statement.

## Layer Structure

Three layers, with dependencies flowing downward only.

| Layer | Module | Responsibility |
|---|---|---|
| Routing | `window_host::pointer_routing` | Owns the windowing-event handlers, the early guards (whose order is frozen by NFR3), the construction of the decision layer's plain-value inputs, and the perform step that executes a named arm. |
| Decision | `window_host::mouse_report` | Pure decision units. Given plain values, each names exactly one disposition and a bundle of record updates. Never mutates a record at decision time. |
| Translation helpers | `window_host::input_translate` | Pure, stateless mappings the decision layer reuses (in this feature: the wheel-consumer helper). |

Invariants on the decision layer (NFR1, SC-10 property 3):

- No windowing type, terminal-core type, window handle, GPU surface or PTY
  appears in any signature.
- Every record change travels in the record-update bundle, applied later by the
  routing layer's apply step — never written during the decision.
- Every decision is reachable from a bare unit test with nothing constructed.

The routing layer may call downward into the decision layer and the translation
helpers; the decision layer may call downward into the translation helpers; no
call goes upward.

## Shared Components

These contracts span the decision layer, the routing layer and the unchanged
perform step. They are the boundary this feature must satisfy, and the reference
the review and verify phases check against.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Grid-ownership input record (`GridOwnershipInputs`) | Carries one membership flag per guard region for a single pointer event | **Pre**: built once per pointer event by the routing layer, from geometry it already computes; no new geometry is derived. **Post**: the single combined top-area flag is gone, replaced by two independent flags — a CSD title-bar-band flag and a tab-bar-band flag (see the region-boundary contract below). Every other region flag keeps its current meaning. | task0001 |
| Region-boundary contract (FR9) | Fixes where the two new flags are true | **Post**: the title-bar-band flag is true exactly when the pointer's logical vertical position is above the CSD title-bar height. The tab-bar-band flag is true exactly when that position is at or below the title-bar height and above the title-bar height plus the effective tab-bar height. The effective tab-bar height is the same value the routing layer's existing tab-bar wheel guard uses, and is zero when the tab bar is hidden — so the two can never disagree, and with the tab bar hidden the tab-bar band is empty. | task0001 |
| Grid-ownership predicate (`point_belongs_to_grid`) | Answers "does the terminal grid own this point" | **Pre**: takes the grid-ownership input record. **Post**: meaning and truth table unchanged — false when any region flag is true or the profile selector is visible, true otherwise. Only what callers do with a false answer changes (NFR3). | task0001 |
| Wheel-consumer helper (`wheel_consumer`) | Names the pre-feature wheel consumer from five boolean conditions | **Pre**: reused exactly as it is — not modified, not re-derived, not duplicated. The rejected-position wheel branch supplies its tracking-active input as a fixed false. **Post**: with tracking-active false it ignores the shift condition entirely and answers "translate to arrows" only when the alternate screen, the alternate-scroll mode bit and the alternate-scroll setting are all true, and "scroll scrollback" otherwise; the report-to-application answer is unreachable from this branch. | task0001 |
| Rejected-position disposition table | The single source of truth for what a guard-rejected position decides | See the table below. | task0001 |
| Record-update invariance (FR5, FR8) | What a rejected event is allowed to change | **Post**: a rejected event's record updates stay at their default bundle — no reset, no tab rebuild marker, no cached-cell advance, and no gesture owner — on all three paths. Restoring a local arm changes the disposition field and nothing else. | task0001 |
| Perform step (routing layer) | Executes a named arm | **Pre**: unchanged by this feature; not reordered, not extended. **Post**: a named local arm runs the behaviour it names; the "nothing" disposition falls through to no action, as today. The scrollback-scroll arm restored from a guard region enters the same branch structure a grid-owned notch enters, so the wheel-accumulator bookkeeping needs no change (assumption A4). | task0001 |

### Rejected-position disposition table (FR1-FR4, FR6, FR10)

Read only when the grid-ownership predicate answers false. Overlapping regions
resolve by the precedence rule below, not by row order.

| Region | Wheel | Middle press | Left / right press | Motion |
|---|---|---|---|---|
| CSD title-bar band | local: wheel-consumer arm | nothing | nothing | nothing |
| Tab-bar band | nothing | nothing | nothing | nothing |
| Bottom status strip | local: wheel-consumer arm | local: paste-primary | nothing | nothing |
| Scrollbar overlay | local: wheel-consumer arm | local: paste-primary | nothing | nothing |
| Mux sidebar | nothing | local: paste-primary | nothing | nothing |
| CSD resize hot zone | local: wheel-consumer arm | local: paste-primary (only where it does not overlap the top area) | nothing | nothing |
| Profile selector visible | nothing | nothing | nothing | nothing |

- "wheel-consumer arm" means the arm the wheel-consumer helper names under its
  contract above: scroll-scrollback, or translate-to-arrow-bytes on the alternate
  screen with the mode bit and the setting both on.
- Every middle-press cell becomes "nothing" when middle-click paste is disabled.
- No cell on any path produces a report disposition (FR7). The report side of the
  answer is unchanged by this feature.

## Conventions

- **Test placement and naming**: every automated scenario is a unit test in the
  inline test module that already sits beside the decision units. Names follow
  `<subject>_<scenario>_<expected>`. Tests build their inputs from the existing
  base-input helpers rather than constructing anything windowed.
- **Requirement identifiers**: SPEC identifiers (`FR1`…`FR10`, `NFR1`…`NFR6`,
  `TS1`…`TS11`, `MS1`…`MS4`) are used literally in test names, comments and
  verification records. They are compared as strings by the traceability checks.
- **Logging policy**: no log line is added anywhere on the pointer path, at any
  level (NFR4). The regression this feature repairs is diagnosed by tests, not by
  logging.
- **Error handling policy**: no error type, error code or fallible path is
  introduced. The only "no action" condition is the suppressing disposition
  itself, which the perform step already handles.
- **Hot-path cost policy**: the region dispatch reads only booleans already
  present in the input record. No allocation, no lock acquisition and no extra
  geometry computation is introduced per pointer event (NFR4).

## Cross-task Design Decisions

### D1 — One implementation task, deliberately

The feature is decomposed into a single task. The three candidate seams were each
evaluated and rejected:

- *Split by decision unit (wheel vs. press/motion)*: the feature's central
  anti-regression test (TS9, satisfying AC16/BO2) asserts one expected table
  covering every region × event kind × button identity. Split across two parallel
  worktrees, each side could only assert its own kinds, both sides would edit the
  same test construct, and the union would depend on a merge adoption going
  perfectly — putting the feature's own acceptance criterion at risk.
- *Split the region-flag decomposition (FR9) off as its own task*: the wheel
  answer is the only consumer that needs the two flags distinguished, and a
  parallel dispatch task could not compile without performing the same
  decomposition itself. The split would produce duplicated work, not parallelism.
- *Split tests off from behaviour*: the tests assert dispositions the same change
  produces, so a test-only worktree would be red by construction and would
  contradict the test-first discipline.

The change is two production files, one input record, two decision-unit rejection
branches and one inline test module — one coherent implementation session.

### D2 — The rejected-position answer uses existing decision values only

No disposition variant and no local-arm variant is added (NFR2). The reviewer's
alternative of introducing a dedicated "not grid owned" disposition is explicitly
not adopted: it would blur the decide/perform boundary and weaken the property
that one decision value names one concrete action.

### D3 — Overlap precedence: suppressing regions win

When more than one region flag is true for the same position, the answer is
decided in this order (FR10):

1. Profile selector visible — suppresses.
2. Tab-bar band, then mux sidebar — suppress.
3. Arm-bearing regions — title-bar band, bottom status strip, scrollbar overlay,
   resize hot zone — name their arm.

This makes the real overlaps deterministic (the resize band's top edge inside the
title-bar band, its bottom edge inside the status strip, its right edge inside the
scrollbar overlay) and guarantees that an event the chrome is already consuming
can never additionally move the terminal.

### D4 — The rejected wheel branch fixes tracking-active to false

A guard-rejected position is not grid-owned, so a tracking application has no
claim on the notch; the wheel-consumer helper is therefore consulted with
tracking-active fixed false, which reproduces the pre-feature wheel behaviour
table exactly. A fixed scroll-scrollback answer is rejected outright — it would
break alternate-screen arrow translation.

### D5 — Motion keeps no local arm

The motion rejection branch is left exactly as it is (FR6). The routing layer
performs motion's local work — chrome forwarding, sidebar hover, resize hint, link
hover, selection-drag extension — before the decision runs, so naming a motion arm
would double-execute work already done.

### D6 — A rejected press records no gesture owner

Even now that a press may name a local arm, a guard-rejected press records no
gesture owner (FR5). Recording a local owner would route the matching left release
into the selection-completion arm, completing a selection that was never begun.

### D7 — Guard order is frozen

No early return in the pointer-button or wheel handler is reordered, added or
removed (NFR3). Which regions a wheel reaches and which a press reaches differ,
and that asymmetry is a fact the disposition table encodes — the mux sidebar is
press-reachable but wheel-unreachable, the title-bar band is the reverse.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A construction site of the grid-ownership input record exists outside the two known ones, so the flag split does not compile | Medium | Low | The failure is a build error, never a silent behaviour change (assumption A3). Mechanical adaptation of any further site inside `window_host/` is in scope; a site outside it is reported as a plan deviation. |
| The wheel-consumer helper is not reachable from the decision unit at its current visibility | Medium | Low | A visibility widening within the module tree is permitted and pre-declared in the task's file set; the helper's behaviour stays untouched. |
| Strengthening the existing guarded-region test breaks other currently-green assertions | Low | Medium | The strengthening is additive to the existing byte-absence assertion, and the full unit suite is the completion gate (NFR6). |
| Overlap precedence implemented as row order rather than as the stated precedence, making an overlapping position answer by accident | Medium | High | The precedence is pinned as its own scenario (TS10) covering each overlapping pair explicitly. |
| The restored arm leaks a report on some tracking-mode combination | Low | High | The report prohibition is re-asserted across the whole matrix, both encodings and every tracking mode (TS2), independently of the disposition assertions. |
| Reviewers read the single-task plan as under-decomposition | Medium | Low | D1 records the evaluated seams and why each was rejected. |

## Open Questions

- [ ] NFR4 (no allocation, lock or log line added per pointer event) has no
      automated verification. The SPEC states it is satisfied structurally, so it
      is carried as a code-inspection item in the review phase rather than as a
      test scenario.
- [ ] Assumption A1 stands: a left press inside a CSD resize hot zone whose cached
      resize direction is absent bypasses the early resize guard and now answers
      "nothing" where it previously began a selection. This is the one place the
      change is not strict pre-regression parity, and it is treated as intended.
