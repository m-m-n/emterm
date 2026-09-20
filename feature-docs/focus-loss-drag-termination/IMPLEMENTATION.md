# Implementation Plan: focus-loss-drag-termination

## Overview

Narrow the winit focus-loss arm so it terminates a live local left selection
drag only when the left button is not held, and extract that termination
decision as a pure, window-free predicate whose truth table is provable from a
bare unit test. Termination of a drag preserved across a focus steal is
delegated to the existing no-owner release fallback in the decision layer.

- SPEC: `feature-docs/focus-loss-drag-termination/SPEC.md`
- Requirements: `feature-docs/focus-loss-drag-termination/REQUIREMENTS.md`
- Verification: `feature-docs/focus-loss-drag-termination/VERIFICATION.md`

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate, GUI-gated modules only
  (`gui` feature, default-on). Tests are the crate's own inline test modules.
- **Key libraries**: none added. The touched modules sit above winit but the
  feature introduces no call into any new library.
- **New dependencies**: none. The feature adds no crate, no test framework
  (`proptest` / `criterion` remain absent) and no platform-specific API.
  Consequently no license question arises: `project.license` stays `MIT` and
  the license review perspective has no new dependency line to cross-check.

## Layer Structure

| Layer | Module | Responsibility | May depend on |
|---|---|---|---|
| Event arms | `src-tauri/src/window_host/event_loop.rs` | Translate windowing-system events into calls on the layers below; owns the statement order inside one arm | Pointer routing, decision layer, host/app state |
| Pointer routing | `src-tauri/src/window_host/pointer_routing.rs` | Local selection arms and the pure drag-state helpers, including this feature's new termination predicate | Decision layer, host/app state |
| Decision layer | `src-tauri/src/window_host/mouse_report.rs` | Window-free gesture-ownership and held-button bookkeeping, release disposition | Plain data only |
| Host / app state | `src-tauri/src/window_host/mod.rs`, `src-tauri/src/app.rs` | Drag flag, held-button record, gesture-ownership record, pending selection anchor | — |

**Allowed direction**: downward only. The decision layer and the new predicate
never take or return a winit type, an egui window handle, a GPU surface, a PTY
or a `term_core` mode type (NFR1) — that invariant is what keeps every unit
drivable from a bare unit test with no window.

## Shared Components

Only one task exists, so this table pins the contracts the task must implement
against rather than an inter-task interface. It is the authority on the new
predicate's shape; the task plan does not restate it.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| Focus-loss termination predicate (new, pointer-routing layer) | Decide whether losing window focus terminates the live local left drag | **Inputs**: plain bools only — the drag-in-flight signal (or the two bools it is composed from) and the left-button-held bool. **Pre**: the left-held input is the value read BEFORE the held-button record is cleared. **Post**: returns "terminate" exactly when a local left drag is in flight AND the left button is not held; returns "do not terminate" in every other combination. **Purity**: evaluating it mutates nothing and performs no I/O. **Surface**: module-internal visibility (visible to the sibling modules and the crate's test module, not exported from the crate); no winit / egui / GPU / PTY / `term_core` type in the signature | task0001 |
| `drag_in_flight` / `local_drag_in_flight` (existing) | Report whether a local left drag is in flight | Signatures and meaning unchanged; the new predicate composes with them instead of replacing them | task0001 |
| `clear_all` (existing, decision layer) | Empty both the gesture-ownership record and the held-button record | Signature unchanged; its position in the focus-loss arm is unchanged; it runs AFTER the held-button read | task0001 |
| No-owner release fallback in `decide_release` (existing, decision layer) | Terminate a drag preserved across a focus steal | Unchanged: a left release with no recorded owner takes the local completion arm when the drag-in-flight input is true. This is the ONLY terminator introduced for the preserved-drag path | task0001 |
| `publish_local_drag` (existing, pointer-routing layer) | Clear the drag flag, consume the pending selection anchor, publish the materialized selection to PRIMARY (and to CLIPBOARD when copy-on-select is on) | Signature and behaviour unchanged; only the condition guarding its call in the focus-loss arm changes | task0001 |

## Conventions

- **Test placement and naming** (NFR3): new tests are bare unit tests inside the
  existing inline test modules next to the code under test, named
  `<subject>_<scenario>_<expected>`, each constructing its own inputs with no
  shared global fixture. No new test framework.
- **Structural source-text assertions**: a needle pinned against the event-loop
  source must be an exact substring of the rewritten arm, and must be specific
  enough that a stale prefix of the old composition cannot satisfy it. When the
  arm's composition changes, the needle set is REPLACED, never appended to.
- **Error handling**: no new error type, code or message. Every condition this
  feature addresses is a state-machine condition resolved by the predicate.
- **Logging**: none added — the focus-loss arm stays silent.
- **Naming**: the new predicate reads as a question about its own inputs and
  carries no windowing vocabulary, matching its sibling drag-state helpers.

## Cross-task Design Decisions

### D1 — One task, not several

The change is one coherent narrowing: the predicate, the arm that calls it and
the structural assertion that pins the arm's composed form must land together.
Splitting them would leave an intermediate state in which the arm and its
structural assertion disagree, and a worktree in which one half does not
compile. The SPEC records the same conclusion under "Implementation Phases".
Affected: task0001.

### D2 — The predicate lives in the pointer-routing layer

It is a sibling of the existing drag-state helpers, with module-internal
visibility, rather than a member of the decision layer. That is where the
drag-in-flight pair already lives and where the test module already imports
from. Rationale recorded as SPEC assumption A3. Affected: task0001.

### D3 — Ordering invariant: read the held state before the records are cleared

The left-held input must be read from the host's event-derived held-button
record before the decision layer's clear routine empties it. A read placed
after the clear would observe "not held" unconditionally and silently degrade
the feature to today's unconditional termination — the exact bug being fixed.
The clear routine itself and its position in the arm are unchanged. Affected:
task0001 (SPEC FR2, AC-4).

### D4 — Delegation, not a second terminator

A drag preserved because the button was held is terminated by the release
path's existing no-owner fallback, reachable because the clear routine has
emptied the gesture-ownership record. No focus-regained check, no
first-motion-after-refocus check and no separate selection-side press record is
added: both candidates are provably ineffective (the first reads an
already-zeroed record; the second stays "held" forever exactly on the
lost-release path). The residual lost-release leak is accepted and
self-healing — the next complete left click in the grid produces a no-owner
release that runs the fallback. Affected: task0001 (SPEC FR8, A2).

### D5 — No state-shape change

The drag flag, the held-button record, the gesture-ownership record and the
pending selection anchor keep their current declarations. The feature adds no
field and changes no field's type, so the host and app state modules are not in
any task's file set. Affected: task0001 (SPEC A5).

### D6 — Held state is event-derived, by definition

"The left button is held" means the host's event-derived held-button record,
not a query of the physical device. A press whose release was never delivered
can therefore report "held" while the button is physically up; that is the
accepted definition, not a defect to guard against. Affected: task0001 (SPEC
A1).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The held-button read is placed after the clear routine, silently restoring today's behaviour | medium | high — reintroduces the reported bug with green tests | D3 stated as an acceptance criterion; the structural assertion pins both the read and the predicate call as they are spelled in the rewritten arm |
| The replacement needle is a prefix that the old composition would also satisfy, so the assertion passes stale | medium | medium — the structural pin becomes decorative | The task's acceptance criteria require needles specific to the new composed form; the SPEC's test note calls this out explicitly |
| Old needles are appended to rather than replaced, so the test fails on literals no longer present | medium | low — caught immediately by the test run | Replacement (not extension) is an acceptance criterion; only literals present in the rewritten arm may remain |
| A left release is genuinely lost, leaving the drag flag set with no immediate terminator | low | low — bounded and self-healing on the next complete left click | Accepted by design (D4); a separate follow-up for a non-event-derived terminator is out of this feature's scope |
| Behaviour of the arm itself cannot be asserted end to end (no window in the test layer) | certain | medium — fidelity risk between predicate and arm | Predicate proven by unit tests; the arm's fidelity pinned structurally; the end-to-end confirmation is the manual scenario in VERIFICATION.md |
| Unrelated existing tests disturbed by the edit | low | medium | The full default-feature test run plus the CLI-only build check are both verification gates |

## Open Questions

- [ ] FR6 (supersession of the prior feature's FR3 / AC-3 / focus-loss
      state-table row) has no implementing task: it was satisfied at create-spec
      by SPEC.md's own Supersession section, and the prior feature's document is
      outside this feature's declared change set. Recorded as a coverage gap
      rather than planned work.
- [ ] NFR6 (no hot-path cost) has no automated test. It is a design-level
      statement — one boolean read per focus-loss event — and is verified by
      review of the diff rather than by a scenario.
- [ ] The follow-up for a non-event-derived terminator for the residual
      lost-release leak (SPEC assumption A2) is deliberately NOT planned here and
      needs a separate feature if it is ever wanted.
