# Implementation Plan: mouse-report-reset-active-gesture

## Overview

Repair the ghost-drag regression in the mouse-report decision layer by
narrowing what a tracking-inactive reset erases, broadening the local
left-release arm, and terminating a live selection drag on focus loss. The DEC
mouse-report protocol surface is preserved byte-for-byte; only the
local-ownership path changes.

**This feature is a single task (`task0001`).** SPEC.md's "Implementation
Phases" section rules out splitting: the repair is one coherent change across
the decision layer, the pointer-routing arms and the focus-loss arm, and any
split that lands one part before the other leaves the drag flag unterminated
in an intermediate state. There are therefore no cross-task contracts; this
document records the feature-wide decisions that the task plan and
VERIFICATION.md both depend on, and the task plan carries the per-step detail.

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
| host/app state | `src-tauri/src/window_host/mod.rs`, `src-tauri/src/app/mod.rs` | the drag flag, the held-button record, the gesture-ownership record, the pending selection anchor | — |

Dependency direction is strictly downward. **L3 stays window-free (NFR1)**: no
signature in the decision layer takes or returns a winit type, an egui handle,
a GPU surface, a PTY, or a `term_core` mode type. Anything L3 needs to know
about host state travels as plain booleans or the existing plain held-button
value.

`src-tauri/src/app` is a directory module; the pending selection anchor is
declared in `src-tauri/src/app/mod.rs`. There is no `src-tauri/src/app.rs`.

## Shared Components

The three components below are shared between the layers above, not between
tasks. Their contracts are pinned here because VERIFICATION.md's scenarios and
the review perspectives both read them.

| Component | Responsibility | Contract (pre/postcondition) | Used by |
|---|---|---|---|
| Local-drag-in-flight signal | The single definition of "a local selection drag is currently live" | **Defined exactly once**, in the pointer-routing layer, as: the host's drag flag is set OR the app's pending selection anchor is present. **Pre**: evaluated before any terminator runs for the event being handled. **Post**: read-only — evaluating it mutates nothing. Both consumers call that one definition; neither re-derives it | L2 (feeds the button decision inputs, FR5), L1 (feeds the focus-loss guard, FR3) |
| Held-button-aware outcome application (`apply_outcome_with_held`) | The companion entry point of the existing outcome-application step, additionally taking the plain held-button value | **Pre**: the held-button value reflects the physical button state at the moment the event was received (the host updates it before any decision is taken). **Post**: identical to the existing entry point in every respect EXCEPT the gesture-slot half of a reset — see D1. The existing entry point is retained as a thin wrapper that delegates with an all-released held-button value, so its present semantics (clear every slot) are unchanged for its roughly thirty existing inline test call sites | L2 (all three production call sites: the motion path, the button path, the wheel path) |
| Local drag terminator, publish half | The steps that end a local drag: clear the drag flag, consume the pending selection anchor, publish a materialized selection | **Pre**: the caller has established that a local drag is in flight. **Post**: the drag flag is false; the pending anchor is consumed; when a selection exists its resolved text is written to PRIMARY, and additionally to CLIPBOARD when the copy-on-select setting is on and the text is non-empty; when no selection exists, neither destination is written. **Excludes** the fold-click toggle, which stays exclusive to the release path (D4) | L2 (the release arm composes it with the fold branch), L1 (the focus-loss arm calls it alone) |

## Conventions

- **Name pinning — what is actually enforced.** The structural test
  `pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam`
  (`src-tauri/src/window_host/tests.rs:1782-1792`) checks only
  `mouse_report::`-prefixed delegate names, each with a trailing open
  parenthesis. So exactly one name in this feature is structurally pinned: the
  companion application entry point, because its name appears in that
  delegate list. The terminator helper and the drag-in-flight input field are
  NOT in that list and nothing enforces their names — any clear name is
  acceptable for them, and they are referred to descriptively throughout these
  documents.
- **The delegate-list entry is REPLACED, not extended.** That test asserts the
  routing source contains the literal needle `mouse_report::apply_outcome(`.
  Moving the three production call sites to `apply_outcome_with_held(` makes
  that exact substring absent — the trailing `(` blocks any prefix match — so
  the test turns red unless the list entry itself is replaced with the
  companion's name. Adding a second entry while keeping the old one leaves a
  guaranteed-red assertion with no owner.
- **Test naming**: `<subject>_<scenario>_<expected>`, one construction per
  test, no shared global fixture (NFR3).
- **Error handling**: no new error type, code or message. Every condition this
  feature addresses is a state-machine condition resolved by choosing a
  disposition, never by returning an error.
- **Logging**: none added. The repair sits on the pointer hot path.

## Cross-task Design Decisions

Recorded as feature-wide decisions (the feature is one task). Each is a
decision the implementer must not re-litigate and the reviewer should check
against.

### D1 — The two reset triggers are distinguished (A3)

The record-update value currently carries a single "reset observed" flag that
folds two independent observations together: no tracking mode is active, and
the active tab differs from the tab the records were built for. Only the first
is narrowed. The plain value the application step consumes must therefore make
the two triggers distinguishable, so that:

- a tracking-inactive reset resets the cell-change cache and clears only the
  gesture slots of buttons that are NOT currently held;
- a tab-change reset resets the cell-change cache and clears EVERY gesture
  slot, held or not;
- when both fire for the same event, the tab-change rule wins (clear all).

The representation (two flags, or one scope value) is the implementer's
choice. What is fixed is that the distinction is visible in the plain value,
because the decision functions must not mutate records (NFR2) and cannot see
held-button state (A4).

### D2 — The held-button exclusion lives in the application step, not in the decision inputs (A4)

Held-button state reaches the exclusion through the companion application
entry point, not by threading held flags into the button or wheel input
bundles. This keeps the decision functions pure functions of their own inputs
(NFR2), keeps the roughly thirty existing inline call sites of the original
entry point compiling unchanged, and keeps the exclusion in the one place
records are mutated.

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

### D4 — Focus loss shares the publish half of the terminator, not the fold toggle (FR3)

Focus loss ends a drag under the same publish rules a release applies, so both
paths run one shared step rather than two drifting copies. The fold-click
toggle is deliberately NOT shared: folding is a click gesture, and making a
focus change toggle a fold would be new behaviour, not a repair. The
extraction must leave the release path's observable behaviour — including the
order in which the fold toggle short-circuits the publish — identical.

### D5 — The drag lifecycle invariant

Every path that sets the drag flag true has a guaranteed terminator: the left
release (the decision plus the existing local arm) and focus loss. No third
path may set the flag, and no condition may be added under which a left
release or a focus loss leaves it set. The pointer-state-gated features — the
resize hint and link-hover refresh (`pointer_routing.rs:227`), the PTY-output
link re-detection (`event_loop.rs:649`), and the link-hover consumer at
`link_hover.rs:178-179` — are the observable consequence of this invariant
holding; none of them has a self-healing path if it does not.

### D6 — Invariance perimeter (FR4, NFR5)

Nothing in this feature may change: the report byte composition or encoding,
the release's targeting of the tab its press recorded, the motion gate, the
cell-change filter's behaviour on the paths that reach it, the wheel-consumer
matrix, or the bounded wheel-report duplication cap. These are verified by the
existing test suite passing unchanged in meaning.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The structural delegate-list assertion turns red when the call sites move | high | medium | The Conventions section above: the list entry is replaced, not extended; this is an explicit acceptance criterion in the task plan |
| Adding a field to the button-decision input bundle breaks an existing full struct literal | high | low | That bundle derives only debug/clone/copy — it has no default — so the one full literal in the focus-loss test (`tests.rs:1844-1863`) must gain the new field. The same test is extended by this feature anyway, so it has an owner |
| Broadening the left-release arm re-publishes a stale selection on a chrome click | medium | medium | D3's drag-in-flight gate; a dedicated test for the chrome press/release pair asserting no PRIMARY write and no fold toggle |
| The reset narrowing suppresses a clear that a genuine tab change needs | medium | medium | D1 keeps the tab-change trigger clearing every slot, with its own test |
| The focus-loss terminator is only reachable through a winit window, leaving its behaviour untested | medium | medium | D4 plus a pure applicability guard over plain booleans, tested bare; the state effects are asserted at the record level as the existing focus-loss test already does |
| Two definitions of the drag-in-flight signal drift apart | low | medium | One definition only (Shared Components); both consumers call it |

## Open Questions

None. Assumption A6 is resolved — see below.

### A6 is verified, not open

The pre-rework behaviour named in the bug report is present in this
repository's history, so A6 is a verified fact rather than an assumption:

- The rework commit is `34774fab` ("task0006: reduce pointer handlers to
  gather/decide/apply/perform"). In its parent revision,
  `src-tauri/src/window_host/pointer_routing.rs:621-622` is an
  **unconditional** left-release arm whose first statement clears the drag
  flag.
- In the same pre-rework file, the release short-circuit at `:406-438` returns
  early only for a report-owned gesture; its in-source comments at `:430-436`
  state that the locally-owned and no-owner cases fall through to that
  unconditional arm.
- Independent corroboration in the current tree: the drag flag is documented
  as "whether the left button is currently held"
  (`src-tauri/src/window_host/mod.rs:172-174`), it is set in exactly one place
  (`pointer_routing.rs:339`) and cleared in exactly one place (`:362`), and its
  three consumers (`pointer_routing.rs:227`, `event_loop.rs:649`,
  `link_hover.rs:178-179`) have no self-healing path.

D3's drag-in-flight gate is a deliberate narrowing of that pre-rework
unconditional arm, required by FR5 so a chrome release cannot re-publish a
stale selection. The narrowing preserves what A6 actually underwrites: a left
release arriving while a drag is live always clears the flag.
