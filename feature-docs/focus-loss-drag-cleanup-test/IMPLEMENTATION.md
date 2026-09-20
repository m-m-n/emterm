# Implementation Plan: focus-loss-drag-cleanup-test

## Overview

The focus-loss local-drag cleanup path is split into a window-free state core,
a pure destination predicate and an injectable selection-write sink so that
bare tests can drive it with no winit window; the prior feature's test-mapping
rows are then corrected to point at those tests. Production behaviour is
unchanged (NFR3) and no dependency is added.

## Technology Stack

- **Language**: Rust — the existing `src-tauri` crate, GUI feature default-on.
  All work lands inside the `window_host` module; no new module is created.
- **Test harness**: the crate's built-in test harness. Unit tests live in the
  inline test module at `src-tauri/src/window_host/tests.rs` and run under the
  library test target (`--lib`); the binary target yields zero tests.
- **New dependencies**: none. This feature adds no crate, no package and no
  external service, so there is no license to check against the project's
  `MIT`; `project.license` stays `MIT` and no new dependency license needs
  recording.

## Layer Structure

```
event_loop.rs focus-loss arm      <- call-site layer (frozen text, no edit)
        | guard call, terminator call
        v
pointer_routing.rs terminator     <- adapter layer (owns every side effect)
        | gathers plain inputs from host and app state
        v
window-free state core            <- state layer (no window type named)
        | consumed anchor + destination decision
        v
destination predicate             <- decision layer (pure, total)
        | destination set
        v
selection-write sink              <- effect layer (two implementations)
        +-- production: delegates to the existing host write primitives
        +-- recording double: captures destination and payload per call
```

Allowed dependency direction is downward only. The adapter layer may name the
window type; the state, decision and effect-abstraction layers must not, so a
test can reach them without constructing a window. The fold-click toggle is
deliberately absent from this chain and stays on the release-path composition
(FR7).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Focus-loss call site | Invokes the drag-in-flight guard, then the terminator | Pre: the two call-site expressions in the focus-loss arm stay character-for-character as they are today. Post: the guard and terminator keep their current names, visibilities, parameter order, parameter types and return types; the string naming the fold-click toggle never appears in that file | task0001 |
| Window-free drag-termination state core | Performs the two state effects and reports the decision | Pre: receives only owned state and plain values (drag flag, pending anchor, resolved selection text, copy-on-select flag); never receives or names a window type. Post: the drag flag reads false, the pending anchor slot is emptied, and the returned value is the anchor exactly as it stood before the call (empty when there was none) | task0001 |
| Destination predicate | Maps (selection present, copy-on-select, text empty) to the destination set | Pre: three plain boolean-shaped values. Post: total over all eight inputs, result exactly as the preserved write rule (FR6) states; no mutation, no I/O, no global access | task0001 |
| Selection-write sink | Performs the two selection writes on behalf of the publish path | Pre: called once per destination the predicate selected. Post: PRIMARY is written before CLIPBOARD when both are selected; each call carries the resolved selection text verbatim; the production implementation delegates to the host's existing primary and clipboard write primitives with today's arguments | task0001 |
| Pinned test identifiers | The test names the prior-feature documents cite | The end-to-end window-free test is named `focus_loss_cleanup_publishes_selection_without_a_window` and lives in `src-tauri/src/window_host/tests.rs`; every destination-predicate truth-table test name begins with `selection_publish_targets_`. task0001 creates identifiers matching this contract; task0002 cites them without reading task0001's plan | task0001, task0002 |

## Conventions

- **Naming (NFR4)**: no new identifier may be spelled `drag_in_flight`, and no
  new binding may shadow the existing free function, struct field or test-local
  bindings of that name. New names are chosen so that each of the four concepts
  (guard predicate, state core, destination predicate, sink) reads distinctly.
- **Error handling**: no new error path and no new error code. The two
  no-write conditions (no selection, active-tab lookup failure) return without
  writing to either destination and surface nothing to the user, exactly as
  today.
- **Logging**: none added. A refactor that is observationally neutral must not
  introduce new log output either.
- **Test placement**: every new test is a bare test in
  `src-tauri/src/window_host/tests.rs`, reachable under the library test target
  with no window, no GPU surface, no event loop and no display server.
- **Formatting**: keep formatting scoped to the touched files. A crate-wide
  format run is forbidden by project rules and would pollute unrelated files.
- **Visibility**: new items are introduced at the narrowest visibility that
  lets the inline test module reach them; nothing new becomes part of the
  crate's outward-facing surface.

## Cross-task Design Decisions

### D1: A test-identifier contract replaces task ordering

FR8 says the document backfill applies "once the new tests land and pass", but
tasks are implemented fully in parallel with no ordering mechanism. The
schedule constraint is therefore satisfied at the feature level — both tasks
merge into the same integration branch before the verify phase runs — and the
cross-task coupling is reduced to the pinned test identifiers in Shared
Components. task0002 writes those identifiers into the prior feature's rows
without reading task0001's plan; task0001 is required by its own acceptance
criteria to produce exactly those identifiers. Affected tasks: task0001,
task0002.

### D2: Observational neutrality is the acceptance bar for the refactor

Because NFR3 forbids any production behaviour change, the refactor is accepted
only when the whole existing suite passes unmodified — including the three
source-scanning tests — and the CLI-only feature check still compiles. No
existing assertion may be weakened, relaxed or deleted to make the new
structure fit; if an existing test blocks a shape, the shape changes, not the
test. Affected tasks: task0001.

### D3: The sink seam is bounded to the pointer-routing publish path

Only the publish path in `src-tauri/src/window_host/pointer_routing.rs` writes
through the sink. The copy-chord clipboard write in the key-routing module
keeps its current literal form and its current adjacency to the
selection-clearing call, because an existing source-scanning test pins both.
Generalising the seam across the module is out of scope for this feature.
Affected tasks: task0001.

### D4: A recorded sink call is evidence of a write request only

The recording double observes that the publish path asked for a write with a
given payload. It does not observe an OS-level selection or clipboard update.
Every document row this feature writes or corrects must state that
distinction, and the prior feature's real-hardware acceptance criterion stays
mapped to a manual scenario rather than being marked unit-verified. Affected
tasks: task0001 (test wording and test notes), task0002 (document rows).

### D5: No new dependency, license unchanged

No new library is introduced, so no license compatibility question arises and
`project.license` stays `MIT`. Any implementer finding they want a new crate
must stop and report a plan deviation instead of adding one. Affected tasks:
task0001, task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Test names drift from the pinned identifiers, so the backfilled rows cite tests that do not exist | Medium | Medium | Identifiers pinned in Shared Components; task0001 and task0002 each carry an acceptance criterion on them; the verify phase checks the citations resolve |
| The seam changes an observable: destination set, payload, write ordering, state mutation or return value | Low | High | Sink contract fixes ordering and payload; the write rule is preserved verbatim; the full existing suite must pass unmodified |
| The production sink implementation lands in a file whose reference impact is only partially known | Medium | Medium | The implementation is additive: the existing write primitives keep their current form, signatures and callers; nothing existing is renamed or removed |
| A new identifier collides with or shadows the already-overloaded drag-in-flight name | Low | Medium | Naming convention above; acceptance criterion in task0001 |
| The prior feature's rows do not have the shape the SPEC describes | Low | Low | task0002 reports a plan deviation rather than inventing or restructuring rows |
| The perceived defect in the write rule (PRIMARY written for empty text) gets "fixed" during the refactor | Low | High | The rule is preserved byte-for-byte and pinned by the new tests as current behaviour |

## Open Questions

- [ ] Whether the prior feature's VERIFICATION.md SC-C row is spelled exactly
      as the SPEC describes is confirmed by task0002 at implementation time;
      the planner did not open that document.
