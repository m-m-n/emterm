# Implementation Plan: mouse-report-reset-active-gesture

## Overview

Repair the ghost-drag regression in the mouse-report decision layer by
narrowing what a tracking-inactive reset erases and by broadening the local
left-release arm, and guarantee that focus loss terminates a live selection
drag. The DEC mouse-report protocol surface is preserved byte-for-byte; only
the local-ownership path changes.

## Technology Stack

- **Language**: Rust (existing `src-tauri` crate, `gui` feature-gated modules).
- **Test framework**: the language's built-in `#[test]` harness only — inline
  `#[cfg(test)] mod tests` beside the code under test, plus the existing
  `window_host` module test file.
- **New dependencies**: none. No crate is added to `src-tauri/Cargo.toml`, so
  no new license enters the project; `project.license` (MIT) is unaffected and
  the license-compatibility check has no new input.

## Layer Structure

| Layer | Files | Responsibility | May depend on |
|---|---|---|---|
| L1 event arms | `src-tauri/src/window_host/event_loop.rs` | winit event dispatch: focus transitions, PTY-output driven refreshes | L2, L3, host/app state |
| L2 pointer routing | `src-tauri/src/window_host/pointer_routing.rs` | gather plain values, call the decision layer, apply the outcome, perform the named local arm | L3, host/app state |
| L3 decision layer | `src-tauri/src/window_host/mouse_report.rs` | pure decisions over plain values; the single place record updates are applied | nothing above it |
| host/app state | `src-tauri/src/window_host/mod.rs`, `src-tauri/src/app.rs` | the drag flag, the held-button record, the gesture-ownership record, the pending selection anchor | — |

Dependency direction is strictly downward. **L3 stays window-free (NFR1)**: no
signature in the decision layer takes or returns a winit type, an egui handle,
a GPU surface, a PTY, or a `term_core` mode type. Anything L3 needs to know
about host state travels as plain booleans or the existing plain held-button
value.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Local-drag-in-flight signal | One agreed definition of "a local selection drag is currently live", derived at the host layer | **Value**: true exactly when the host's drag flag is set OR the app's pending selection anchor is present. **Pre**: read before any terminator runs for the event being handled. **Post**: read-only — computing it mutates nothing. Both tasks derive it independently from the same two fields; neither exports it to the other | task0001 (feeds the release decision), task0002 (feeds the focus-loss predicate) |
| Held-button-aware outcome application (`apply_outcome_with_held`) | The companion entry point of the existing outcome-application step, additionally taking the plain held-button value | **Pre**: the held-button value reflects the physical button state at the moment the event was received (the host updates it before any decision is taken). **Post**: identical to the existing entry point in every respect EXCEPT the gesture-slot half of a reset — see "Reset narrowing" below. The existing entry point is retained as a thin wrapper that delegates with an all-released held-button value, so its present semantics (clear every slot) are unchanged for every existing caller | task0001 (defines it, moves the three production call sites onto it) |
| Local drag terminator, publish half (`terminate_local_drag_and_publish`) | The steps a left release performs to end a local drag: clear the drag flag, consume the pending selection anchor, publish a materialized selection | **Pre**: the caller has established that a local drag is in flight. **Post**: the drag flag is false; the pending selection anchor is consumed; when a selection exists its resolved text is written to PRIMARY, and additionally to CLIPBOARD when the copy-on-select setting is on and the text is non-empty; when no selection exists, neither destination is written. **Excludes** the fold-click toggle, which stays exclusive to the release path | task0002 (extracts it and raises its visibility; the focus-loss arm calls it). task0001 must leave the release path's observable behaviour identical |

No task compiles against a placeholder for another task's component: task0001
never calls the terminator, and task0002 never calls the companion entry
point. The one shared datum — the drag-in-flight signal — is derived
independently on both sides from fields that already exist.

## Conventions

- **Naming**: the three identifiers pinned above (`apply_outcome_with_held`,
  `terminate_local_drag_and_publish`, and the button-decision input field
  `local_drag_in_flight`) are fixed, because a structural test in
  `src-tauri/src/window_host/tests.rs` asserts the pointer-routing source
  literally mentions each delegate the handlers are required to use.
- **Test naming**: `<subject>_<scenario>_<expected>`, one construction per
  test, no shared global fixture (NFR3).
- **Error handling**: no new error type, code or message. Every condition this
  feature addresses is a state-machine condition resolved by choosing a
  disposition, never by returning an error.
- **Logging**: none added. The repair sits on the pointer hot path.

## Cross-task Design Decisions

### D1 — The two reset triggers are distinguished (A3)

The record-update value currently carries a single "reset observed" flag that
folds two independent observations together: no tracking mode is active, and
the active tab differs from the tab the records were built for. Only the
first is narrowed. The plain value the application step consumes must
therefore make the two triggers distinguishable, so that:

- a tracking-inactive reset resets the cell-change cache and clears only the
  gesture slots of buttons that are NOT currently held;
- a tab-change reset resets the cell-change cache and clears EVERY gesture
  slot, held or not;
- when both fire for the same event, the tab-change rule wins (clear all).

The representation (two flags, or one scope value) is task0001's choice — no
other task reads the record-update value. What is fixed is that the
distinction is visible in the plain value, because the decision functions must
not mutate records (NFR2) and cannot see held-button state (A4).

Affected tasks: task0001.

### D2 — The held-button exclusion lives in the application step, not in the decision inputs (A4)

Held-button state reaches the exclusion through the companion application
entry point, not by threading held flags into the button or wheel input
bundles. This keeps the decision functions pure functions of their own inputs
(NFR2), keeps the roughly thirty existing inline call sites of the original
entry point compiling unchanged, and keeps the exclusion in the one place
records are mutated (SC-11's single-application property).

Affected tasks: task0001.

### D3 — The no-owner left release is gated on the drag-in-flight signal (FR2 + FR5)

Broadening the local arm to cover a left release with no recorded owner would,
on its own, turn a chrome click into a selection re-publish whenever a
selection from an earlier drag still exists — a new side effect FR5 forbids.
The decision therefore consults the drag-in-flight signal, delivered as a
plain boolean on the button-decision input bundle (window-free, NFR1):

- recorded owner is local, button is left → the selection-completion arm
  (unchanged from today);
- no recorded owner, button is left, drag-in-flight true → the
  selection-completion arm (new);
- no recorded owner, button is left, drag-in-flight false → nothing, with
  today's reset observations still carried;
- recorded owner is report-owned → unchanged in every respect (FR4).

A chrome press records no owner and starts no drag, so its release reads
drag-in-flight false and stays a no-op. A press whose slot a genuine tab
change cleared mid-drag reads drag-in-flight true, so its release terminates
the drag — which is exactly the guarantee FR2 restores.

Affected tasks: task0001 (gathers and consults it), task0002 (uses the same
definition for the focus-loss predicate).

### D4 — Focus loss shares the publish half of the terminator, not the fold toggle (FR3)

Focus loss ends a drag under the same publish rules a release applies, so both
paths must run one shared step rather than two drifting copies. The fold-click
toggle is deliberately NOT shared: folding is a click gesture, and making a
focus change toggle a fold would be new behaviour, not a repair. The
extraction must leave the release path's observable behaviour — including the
order in which the fold toggle short-circuits the publish — identical.

Affected tasks: task0002 (owns the extraction), task0001 (must not disturb the
release path's behaviour).

### D5 — The drag lifecycle invariant both tasks uphold

Every path that sets the drag flag true has a guaranteed terminator: the left
release (task0001's decision plus the existing local arm) and focus loss
(task0002). No task may introduce a third path that sets the flag, and no task
may add a condition under which a left release or a focus loss leaves the flag
set. The pointer-state-gated features (the resize hint, the link-hover
refresh, and the PTY-output link re-detection) are the observable consequence
of this invariant holding.

Affected tasks: task0001, task0002.

### D6 — Invariance perimeter (FR4, NFR5)

No task may change: the report byte composition or encoding, the release's
targeting of the tab its press recorded, the motion gate, the cell-change
filter's behaviour on the paths that reach it, the wheel-consumer matrix, or
the bounded wheel-report duplication cap. These are verified by the existing
test suite passing unchanged in meaning.

Affected tasks: task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Broadening the left-release arm re-publishes a stale selection on a chrome click | medium | medium | D3's drag-in-flight gate; a dedicated test for the chrome press/release pair asserting no PRIMARY write and no fold toggle |
| The reset narrowing suppresses a clear that a genuine tab change needs | medium | medium | D1 keeps the tab-change trigger clearing every slot, with its own test |
| The two tasks both edit the pointer-routing file and conflict at merge | medium | low | The edits sit in different regions (the gather/apply step versus the terminator helper); the implementer's parent-side-adoption protocol resolves the rest |
| The focus-loss terminator is only reachable through a winit window, leaving its behaviour untested | medium | medium | D4 plus a pure applicability predicate over plain booleans, tested bare; the state effects are asserted at the record level as the existing focus-loss test already does |
| A signature change to the application step breaks the structural delegate-list test | high | low | The delegate list is extended in the same task that introduces the companion (A7), and the companion's name is pinned above |

## Open Questions

- [ ] A6 (impact low, reversible): the pre-rework behaviour named in the bug
      report — an unconditional left-release drag terminator in the base code
      — is taken as the intended target semantics for FR2. The pre-rework
      revision is outside the supplied inputs and this was not independently
      verified.
