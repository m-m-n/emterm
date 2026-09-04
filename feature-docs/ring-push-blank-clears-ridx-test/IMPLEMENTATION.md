# Implementation Plan: ring-push-blank-clears-ridx-test

## Overview

Test-only hardening inside the `term_core` crate: the existing unit test
`test_ring_push_blank_clears_ridx` gains a survivor-row fixture so a
row-scope-loss regression in the scrollback-enabled compress branch of
`ring_push_blank` becomes observable, and its doc comment pins the test's
provable and unprovable scope. The feature decomposes into a single task
(`task0001`); this document therefore carries only the invariants that hold
across the whole feature — the per-test detail lives in `tasks/task0001.md`.

## Technology Stack

- **Language**: Rust — the `term_core` crate (`crates/term_core/`).
- **Test framework**: the language's built-in unit-test harness, used through
  the crate's existing inline `#[cfg(test)]` module. No harness change.
- **Key libraries**: none added. This feature introduces **no new dependency
  and no new dev-dependency** (NFR2), so there is no new license to record;
  `project.license` (MIT) is unaffected and stays as declared in
  `workflow.yaml`.

## Layer Structure

Two layers are involved, with a one-directional, read-only dependency:

| Layer | Location | Role in this feature |
|---|---|---|
| Production (unit under test) | `crates/term_core/src/ring_buffer.rs` | Read-only reference. Holds `ring_push_blank` and its three clear sites. Not modified by the deliverable. |
| Test | `crates/term_core/src/ring_buffer/tests.rs` | The only file the deliverable changes. Observes the production layer through its crate-visible surface. |

The test layer depends on the production layer; the production layer never
depends on the test layer. No layer above `term_core` (renderer, tabs, mux,
GUI) is reached by this feature.

## Shared Components

This feature has exactly one task, so there is no cross-task component use and
no contract that two tasks must independently implement against. The table is
kept for structure and is intentionally empty.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

The surfaces the single task reads are pre-existing and unchanged by this
feature: the crate-visible `overflow` and `overflow_ridx` tables of the
terminal core, the absolute-row accessor `viewport_abs`, the cell writer
`set_cell`, and the blank-row push entry point `ring_push_blank`. Their
contracts are owned by the production code, not by this plan.

## Conventions

- **Assertion identity**: every assertion about a row addresses that row by its
  **absolute** row number, captured before the push that rotates the ring head.
  Viewport-relative indices are never reused across a push — after a push they
  no longer denote the same absolute row.
- **Table independence**: the `overflow` table and the `overflow_ridx` table are
  asserted separately, never one standing in for the other. A single assertion
  covering both would hide a regression that drops only one of them.
- **Anti-vacuity first**: any assertion of the form "entry X is gone" is
  preceded, in the same test, by an assertion that entry X existed. A fixture
  that silently fails to exceed the inline cap must fail the test, not pass it
  trivially.
- **Naming and shape**: mirror the sibling test
  `test_ring_push_blank_clears_recycled_row_overflow_entries` — capture absolute
  row keys, pre-assert non-vacuity, operate, then post-assert removal and
  survival — and mirror its doc-comment phrasing (NFR2).
- **Test placement**: tests stay in the crate's inline `#[cfg(test)]` module,
  per the project's `test/README.md` convention. No new test file, no new
  integration-test target, no new dev-dependency.
- **Documents in English, report to the user in Japanese** — as elsewhere in
  this feature's artifacts.

## Cross-task Design Decisions

### D1: No production diff in the deliverable, with a bounded temporary-mutation exception

**Decision.** The deliverable's change set is exactly one file,
`crates/term_core/src/ring_buffer/tests.rs`. `crates/term_core/src/ring_buffer.rs`
is deliberately absent from every task's declared `files`.

The mutation required to prove the test is not vacuous (FR8: replace the
compress branch's row-scoped clear with a clear-everything form, observe the
test turn red) is a **verification action, not a deliverable change**. It is
explicitly authorized for the duration of that observation and MUST be reverted
immediately afterwards, so that the file's committed content is byte-identical
to its pre-task state. Restoration is itself an acceptance criterion, checked by
inspecting the working tree for any diff against the task's base commit.

**Rationale.** Listing the production file in `files` would declare production
code as changeable and weaken NFR1's guard, which is the point of the
requirement. Excluding it while naming the temporary exception explicitly keeps
the guard intact and still tells the implementer that touching the file during
verification is expected rather than a plan deviation.

**Affected tasks**: task0001.

### D2: Row-scope and cycle-exhaustion are two properties, and both are kept

**Decision.** The strengthened test asserts two distinct properties in one body,
in this order: (a) after a single push, the recycled row's entries are gone and
the survivor row's entries remain; (b) after the full cycle (five pushes in
total, the pre-existing count), both tables are empty. Neither replaces the
other, and the existing emptiness assertions are neither deleted nor rewritten.

**Rationale.** (a) proves "a single eviction does not over-clear"; (b) proves
"everything eventually empties". A regression that over-clears satisfies (b)
trivially, which is exactly the blind spot this feature closes; a regression
that under-clears at the end of the cycle would escape (a). Dropping either
loses coverage.

**Affected tasks**: task0001.

### D3: The row-scope assertions run before the survivor can be evicted

**Decision.** The row-scope assertions are placed at the one-push point, which
is strictly before the push count reaches the fixture's row count. The remaining
pushes run only after those assertions have been made.

**Rationale.** Once the push count reaches the row count, the survivor row is
itself recycled and its entries legitimately disappear — assertions made after
that point cannot distinguish legitimate eviction from over-clearing.

**Affected tasks**: task0001.

### D4: The coverage ceiling is documented, not engineered around

**Decision.** The test's doc comment states, as settled fact: what the test
proves (nothing beyond the evicted row is cleared), what it does not prove (that
the compress branch's own clear site fired), and the structural reason — within
a single push the new viewport's bottom absolute row always equals the evicted
absolute row, so the eviction-time clear and the unconditional bottom-row clear
necessarily target the same row and no fixture can tell them apart.

**Rationale.** The ceiling is a property of the production control flow, not of
the fixture. Recording it in the test body stops the unsatisfiable demand ("pin
the compress branch's clear site firing") from being raised again, and stops a
later contributor from spending effort on a fixture that cannot exist.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The fixture's content does not actually exceed the inline cell capacity, so no overflow entry is created and every post-assertion passes vacuously | Medium | High — the feature's whole purpose is lost silently | D1 conventions' anti-vacuity pre-assert (FR3), asserted through both tables and for both rows, turns this into a test failure |
| Row identity is taken from viewport-relative indices after the push, so the assertions address the wrong rows | Medium | High — false red or false green | Absolute row numbers are captured before the push and reused verbatim afterwards (FR2, Conventions) |
| The row-scope assertions are placed too late in the push sequence and the survivor has already been evicted | Low | High — the survival assertion becomes impossible to satisfy | D3 fixes the assertion point at one push |
| The temporary FR8 mutation is left behind in the deliverable | Low | High — production behavior silently changes | D1 makes restoration an acceptance criterion, verified by a working-tree diff check before the task is considered done |
| Formatting drifts beyond the changed file | Low | Medium — unrelated files are dirtied and review is polluted | Only the changed file is formatted; no crate-wide formatting sweep (NFR4) |
| The strengthened test becomes order- or thread-sensitive | Low | Medium — flaky suite | The fixture builds its own terminal core instance and shares no global state; determinism under the default parallel test run is an explicit requirement (NFR3) |

## Open Questions

- [ ] FR7 (coverage boundary recorded in the doc comment) has no automated test
      scenario. It is verified by human reading of the doc comment (a manual
      item in VERIFICATION.md), and its `tests` mapping in `workflow.yaml` stays
      empty. Automating it would mean asserting on comment text, which is worse
      than the manual check it replaces.
- [ ] NFR2 (follow the existing test style; add no dev-dependency) likewise has
      no automated test scenario. The dev-dependency half is mechanically
      observable as "no manifest file appears in the change set", the style half
      is a review judgement; both are covered as manual items rather than by a
      test ID.
