# Implementation Plan: survivor-assert-key-stability

## Overview

Add two observations to the post-scroll survival block of an existing
`term_core` unit test so the test actually observes the claim its own comment
makes — the survivor row's ring slot key stays stable across a viewport shift —
and so a survivor row whose content was blanked is no longer accepted as a
pass. The change is additive assertions inside one existing test function; no
module, type, or public API is introduced or altered.

## Technology Stack

- **Language**: Rust — the `term_core` crate (`crates/term_core/`), registered in
  workflow.yaml as `project.components.term_core`.
- **Test harness**: the language's built-in unit-test harness, used through the
  crate's inline test module. No test framework is introduced.
- **Formatter**: rustfmt with style edition 2024, invoked crate-scoped.
- **New dependencies**: none. This feature adds no crate, no dev-dependency and
  no tool, so no new license enters the project. `project.license` is `MIT` and
  stays `MIT`; the license-compatibility check has no new input to evaluate.

## Layer Structure

Exactly one crate is touched, and within it only the test layer changes.

| Layer | Location | Role in this feature | Write access |
|---|---|---|---|
| Production (core-domain) | `crates/term_core/src/ring_buffer.rs` | Holds the blank-push routine whose Step 3 fill target is the behaviour under observation. It is the *subject*, never the *object*, of this feature. | Read-only for the permanent change. Temporarily mutated during the red-check procedure, then restored. |
| Test | `crates/term_core/src/ring_buffer/tests.rs` | Holds the test whose survival block gains the new observations. | The only file whose content changes permanently. |

Allowed dependency direction: test layer observes the production layer through
in-crate accessors. There is no reverse direction, and no new module boundary
is created.

## Shared Components

This feature decomposes into a single task, so no component is shared *between*
tasks and no inter-task contract needs pinning. The table below instead records
the two in-crate accessors the task observes through, so the observation surface
and its postconditions are pinned in one place.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Viewport-to-absolute row lookup (`TerminalCore::viewport_abs`) | Resolve the ring slot key currently backing a given viewport row | Pre: the row index is inside the viewport. Post: returns the absolute ring slot key backing that row. Visibility is crate-internal, and the test lives in the same crate, so no visibility change is needed. The same test already calls it, so its availability is established. | task0001 |
| Cell grapheme accessor (`TerminalCore::get_cell_char`) | Read the grapheme cluster stored at a (column, row) cell, addressed column-first | Pre: the coordinates are inside the grid. Post: returns the stored cluster for a populated cell; returns a halfwidth space for an emptied cell, because an emptied cell stops reporting itself as an overflow cell. That difference is exactly what distinguishes a surviving row from a blanked one. | task0001 |

## Conventions

- **Test style** (NFR1): follow the crate's existing test conventions —
  an inline test module, one explicitly constructed terminal core per test, no
  shared fixtures, and assertions written against the observable contract rather
  than against internal shape. Add no test crate and no dependency.
- **Additive-only assertions** (FR3): existing assertions are neither removed
  nor rewritten. New observations are inserted alongside them. A change that
  replaces an existing assertion with a "better" one violates this convention
  even when the replacement is stronger.
- **Reference style for the content observation** (FR2): mirror how the cell
  grapheme accessor is already used by the full-screen-scroll test in the same
  file, including its column-first argument order.
- **Formatting** (NFR2): the crate-scoped format check must report no diff.
- **Error-handling policy**: not applicable beyond assertion failure. An
  assertion failure is the intended and only signal this feature produces; no
  error type, no logging and no recovery path is introduced.
- **Scope discipline** (FR5): the permanent diff is confined to the test file.
  Any edit to production code made during verification is transient and must be
  restored before the task is considered done.

## Cross-task Design Decisions

### D1: The survivor-content observation is mandatory, not optional

The originating task description marked the survivor row's content observation
as optional. It is treated here as mandatory (assumption A1). The reason is that
the reproduction mutation deliberately leaves the overflow-clear side pointed at
the correct row, so the pre-existing entry-presence assertions and the new key
stability observation all pass unchanged under it. The content observation is
the only one of the three that can turn red. Dropping it would leave the
feature's own definition of done unreachable.

Affected: task0001, verification scenario TS2.

### D2: Mutation injection is a verification procedure, not a deliverable

The red-check (FR4) deliberately breaks production code to prove the enlarged
test can fail. It is executed locally, observed, and then reverted (assumption
A4). It is never committed and never left in the working tree. Consequently:

- The production file appears in task0001's predicted file set, because the task
  does touch it during the procedure — the prediction is honest about what the
  implementer will open and edit.
- The task's acceptance criteria require that same file to be byte-identical to
  its pre-task content when the task finishes, which is what keeps the honest
  prediction from becoming a licence to change production behaviour.

Affected: task0001, verification scenarios TS2 and TS3.

### D3: The expected survivor grapheme is derived, not hard-guessed

The expected content is the base character the fixture prints followed by the
combining marks the same fixture applies, in the fixture's own order (assumption
A2). It is read off the fixture inside the test rather than transcribed from
this document, so a future change to the fixture's mark set surfaces as a test
failure at the fixture rather than as a silently stale literal.

Affected: task0001.

### D4: No coverage instrumentation is introduced

The project configures no coverage tool, and this feature adds none. Adequacy is
argued through the requirement-to-scenario mapping in VERIFICATION.md and
through the red-check, not through a coverage percentage.

Affected: VERIFICATION.md.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The red-check mutation is left in the working tree and reaches the commit, silently changing production behaviour | Low | High | D2 makes restoration an acceptance criterion; the production file is inside the task's declared file set so review scoping covers it; TS3 re-checks that no mutation remains |
| The expected grapheme is written with the wrong mark count or order, making the content observation fail spuriously or (worse) pass for the wrong reason | Medium | Medium | D3 derives the expectation from the fixture's own mark sequence; TS2 proves the assertion can still turn red, which a mis-specified constant would typically break |
| The key stability observation alone is treated as sufficient and the content observation is skipped | Medium | High | D1 records why it is insufficient; TS2 is the gate that fails if only the key observation was added |
| Both new observations are added but neither can actually fail (vacuous strengthening) | Low | Medium | TS2 is the anti-vacuity gate for the pair; the pre-existing pre-scroll anti-vacuity assertions are retained by FR3 |
| Reformatting or reordering the surrounding block while inserting the new lines produces diff noise outside the survival block | Low | Low | FR3 confines the edit to insertion; NFR2's format check runs crate-scoped so it cannot mask an unrelated reformat |

## Open Questions

- [ ] None. Every requirement in workflow.yaml carries `status: ok`; there are no
      `tbd` items, no unresolved assumptions, and no design step output to
      reconcile (the design step is recorded as skipped because the feature
      touches no UI surface and no design token).
