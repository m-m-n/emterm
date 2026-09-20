# Implementation Plan: mouse-drag-latch-regression

## Overview

This feature adds seam-level regression tests that pin the already-fixed
ghost-drag latch behaviour of the pointer path, together with the deliberate
focus-loss semantics that surround it. No production behaviour changes
anywhere (FR4) — the deliverable is test coverage only.

## Technology Stack

- **Language**: Rust — the existing `emterm` crate under `src-tauri/`, inside
  the GUI-gated `window_host` module.
- **Test harness**: the language's built-in test harness, reached through the
  existing inline test module `src-tauri/src/window_host/tests.rs` (NFR2).
- **New dependencies**: none. No library enters the project, so
  `project.license` (MIT) needs no compatibility decision and is unchanged.
  License record for this feature: no new dependency, therefore no new
  license obligation.

## Layer Structure

Three layers, named here so every task states which layer it attaches to:

1. **Decision layer** (window-free, pure) — maps one described input event
   plus the current mouse-report records to a disposition and a set of record
   updates. Mutates nothing.
2. **Apply layer** (window-free, record-owning) — folds a decision's record
   updates into the records, taking the held-button value that was live when
   the applied event arrived.
3. **Composition layer** (window-owning) — the pointer-routing and event-loop
   code that reads real window and app state and calls layers 2 and 1.

Allowed dependency direction: 3 → 2 → 1, never upward. **Tests introduced by
this feature attach to layers 1 and 2 only** (NFR1). Layer 3 cannot be
constructed in a unit test in this project; the existing source-scan test is
the only pin on it and stays exactly as it is (FR5).

## Shared Components

Feature-wide contract surface. Every row is an EXISTING seam whose behaviour
the new tests assert and whose production form must not change (FR4) — this
table is the invariance surface, not a build list.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|---|---|---|---|
| `decide_button_event` | decide one press or one release | **Pre**: the caller supplies the event description, the current records and the live drag-in-flight signal. **Post**: pure. A left release with no recorded gesture owner names the selection-completion arm exactly when drag-in-flight is true, and names "nothing" otherwise; a middle or right release with no owner names "nothing" in every case. | task0001 |
| `decide_wheel_event` | decide one wheel notch | **Pre**: as above, for a wheel event. **Post**: pure; a notch decided while tracking is inactive carries the tracking-inactive reset observation. | task0001 |
| `apply_outcome_with_held` | fold a decision's record updates into the records | **Pre**: receives the held-button value that was live when the applied event was received. **Post**: a tracking-inactive reset clears only the slots of buttons NOT reported held; a tab-change reset clears every slot; either flag also resets the cell-change cache and the report accumulator. | task0001 |
| `apply_wheel_report_step` | the wheel path's apply-and-fold unit | **Pre**: the same held-button obligation as the row above. **Post**: delegates the record update to the held-aware apply exactly once, then folds the wheel delta only for a report disposition. | task0001 |
| gesture-ownership record (`record_press` / `peek` / `clear_all` / `clear_unheld`) | own one gesture slot per button | **Post**: the slot read is the observable contract — a surviving slot is what "this gesture is still owned" means to a test. | task0001 |
| `should_terminate_drag` | focus-loss termination predicate | **Pre**: two plain booleans — drag in flight, and left physically held. **Post**: answers "terminate" exactly for in-flight AND not-held; every other combination answers "do not terminate" (A1 — intended semantics, not a defect). | task0001 |
| `consume_drag_termination` | the window-free half of the local drag terminator | **Post**: the drag flag reads false, the pending selection anchor is emptied and handed back, and the publish-destination decision is produced. | task0001 |
| `publish_to_targets` with a recording selection sink | issue one write request per selected destination | **Post**: PRIMARY before CLIPBOARD when both are selected. A recorded call is evidence of a write REQUEST, never of an OS-level selection update. | task0001 |

## Conventions

- **Test naming**: `<subject>_<scenario>_<expected>`, the repository's
  dominant pattern (NFR3).
- **Mechanism label**: every new test carries a doc comment naming the single
  mechanism it guards, drawn from exactly these four labels (FR6):
  reset-clears-held-gesture-slot, held-unaware-apply-entry-point,
  ungated-no-owner-left-release, focus-loss-gate-inversion.
- **Assertion policy**: assert observable contracts only — a disposition
  value, a gesture-slot read, the drag flag, the pending anchor, and recorded
  sink calls (NFR5). Every assertion carries a message naming the mechanism
  and the requirement it protects, so a future failure is diagnosable without
  re-deriving this investigation.
- **Fixture policy**: each test constructs its own records and state inline;
  no shared or global fixture, and no ordering dependency between tests
  (NFR4).
- **Reference style**: documents and comments cite symbol names, never line
  numbers, because the original report's line references are already stale
  against the base revision (A6).

## Cross-task Design Decisions

### D1: Production code is frozen

No file under `src-tauri/src/` changes except test-module content inside
`src-tauri/src/window_host/tests.rs`. The three mechanisms the original
report named are already repaired at the base revision (A2), so any
production edit inside this feature would be new behaviour rather than a
repair. Affected: task0001; enforced by the change-set check in
VERIFICATION.md.

### D2: Assertions attach to window-free seams

"The drag has ended" and "the selection was published" are asserted against
the termination seam and the publish seam with a recording sink double, never
against a real window host, which a unit test in this project cannot
construct (A5, NFR1). Affected: task0001.

### D3: Mutation sensitivity is the acceptance standard

A regression test that merely restates today's output is worthless here. Each
new test is designed so that its own named mutation — a reset that clears
held slots, an apply entry point that ignores held buttons, an ungated
no-owner left release, an inverted focus-loss gate — makes it fail. Acceptance
criteria are phrased against the mutation, not against the current output.
Affected: task0001.

### D4: Focus-loss semantics are intended behaviour

Losing window focus while the left button is still physically down PRESERVES
the drag; the later release is its terminator. Losing focus with left not
held terminates the drag immediately. Tests pin both arms and must never
assert unconditional termination on focus loss (A1, FR3). Affected: task0001.

### D5: One task, one file

The whole change set is a single file, so the feature is planned as a single
task and no cross-task interface exists to coordinate. Feature-wide coverage
accounting therefore lives in VERIFICATION.md rather than being split across
task plans.

### D6: New coverage is layered on top of existing coverage

The apply layer's reset rule already has unit coverage inside the
mouse-report module's own test module, and the focus-loss gate already has a
truth-table test plus a source-scan test. What none of them cover is the full
press → interruption → release → terminate → publish sequence, which is what
the new tests add. Existing tests are neither edited nor weakened (FR5).
Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A new test asserts today's output yet survives the regression it claims to guard | Medium | High | D3; acceptance criteria are phrased as "fails under mutation X", and the task plan prescribes a temporary local mutation probe that is reverted before commit |
| An incidental production edit slips in while reading the seams | Low | High | D1; the change-set check (TS-6) and AC-7 |
| A later reader deletes a new test as a duplicate of existing coverage | Low | Medium | D6 plus mechanism-label doc comments that state the distinct scenario each test owns |
| A new test introduces order dependence and destabilizes the suite | Low | Medium | Per-test inline fixtures (NFR4); TS-9 runs the new tests without a single-thread constraint |
| The focus-loss semantics get "corrected" later by someone reading the original report literally | Medium | Medium | D4 pins both arms, and the doc comment records that the preserved arm is deliberate |

## Open Questions

- [ ] None. Every requirement is `status: ok` in workflow.yaml, and
      assumptions A1-A6 were confirmed at create-spec.
