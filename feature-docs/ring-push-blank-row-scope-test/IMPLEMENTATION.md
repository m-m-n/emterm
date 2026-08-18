# Implementation Plan: ring-push-blank-row-scope-test

## Overview

Extend one existing inline unit test in `crates/term_core` so that it observes the *row
scope* of `ring_push_blank`'s overflow side-table clearing, instead of only observing that
some clearing happened. The change is confined to the crate's inline test layer; the
production module under observation is unchanged in the final diff.

## Technology Stack

- **Language**: Rust — the `crates/term_core` library crate (pure logic, no GUI feature gate).
- **Test harness**: the standard cargo test harness driving inline `#[cfg(test)]` modules.
- **Key libraries**: none added. The crate keeps its single existing dev-dependency; no
  property-testing, snapshot, benchmarking or mutation-testing framework is introduced.

### Dependency licensing

New dependencies introduced by this plan: **none**. There is therefore no license
evaluation to perform against the project license (MIT). Any future need for a new
dependency in this feature would be a plan deviation and must be reported rather than
absorbed.

## Layer Structure

Two layers are involved, and only one of them is edited.

| Layer | Location | Role in this feature | Edited |
|---|---|---|---|
| Production ring-buffer logic | `crates/term_core/src/ring_buffer.rs` | The behavior under observation (`ring_push_blank` and its row-scoped clearing of the overflow table and its reverse index) | No — byte-identical at task completion |
| Inline test layer | `crates/term_core/src/ring_buffer/tests.rs` | The observer: fixture construction, pre-scroll anti-vacuity assertions, post-scroll survival/removal assertions, explanatory comments | Yes — the only edited source file |

Allowed dependency direction: test layer observes the production layer. The reverse
direction is never introduced — no production symbol is added, widened, or reshaped for
the test's convenience, and no test-only hook is planted in production code.

## Shared Components

This feature decomposes into a single task, so no component is produced by one task and
consumed by another.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (none) | — | — | — |

For reference, the **existing, unchanged** observation surfaces the task relies on — these
are contracts the task depends upon, not contracts it creates:

| Surface | Shape | Property relied upon |
|---|---|---|
| Overflow side table | keyed by (column, absolute row) | Per-row survival is directly readable: an entry's presence or absence for a given absolute row answers "was this row cleared?" without any new accessor |
| Overflow reverse index | keyed by absolute row, valued by a set of columns | Same, at row granularity, including which columns a surviving row still claims |
| `ring_push_blank` | Recycles exactly one ring slot per call | Postcondition under observation: the two side tables lose exactly the evicted absolute row's entries, and nothing else |
| Ring slot / absolute row identity | Stable across a scroll | A row that is not recycled keeps its absolute row key even though its viewport position shifts — this is what makes the survivor row observable with a key captured *before* the scroll |

## Conventions

- **Test placement**: the fixture stays in the inline `#[cfg(test)]` module adjacent to the
  test it extends. No new test file, no new module, no relocation of the test.
- **Naming**: the existing `test_*` function name is kept; the feature adds observation to
  an existing test rather than introducing a differently named one.
- **Comment language and style**: English, matching the surrounding module. The explanatory
  comment block above the test is a maintained artifact — when the test's claim widens, the
  block is updated in the same change so it never overstates or understates what the test
  proves.
- **Assertion discipline (anti-vacuity)**: every "still present after the scroll" assertion
  is paired with a "genuinely present before the scroll" assertion on the same key. A
  survival assertion without its pre-assertion is not acceptable, because a fixture that
  silently stops producing overflow-bound content would make it vacuously true.
- **Additive observation**: existing assertions are preserved, never relaxed or replaced.
  Observation is only ever added.
- **Command surface**: exactly the three commands registered for the `term_core` component
  in `workflow.yaml` are used (build/check, test, format). No new command string is
  introduced; formatting is only ever run in check mode, never as a crate-wide rewrite.

## Cross-task Design Decisions

### D1 — Row scope is observed through the two side tables' keys, not through new production surface

The clearing's row scope is made observable purely by reading the overflow table and its
reverse index with a *survivor* absolute row key alongside the *recycled* one. No accessor,
no visibility widening, no test hook, and no instrumentation is added to production code.
Affected: the whole feature (task0001). Rationale: the keys already carry the row identity,
so any added production surface would be net-new API for zero observational gain and would
break the test-only containment requirement.

### D2 — The regression demonstration is a transient production mutation, proven reverted

The feature's regression-detection evidence (whole-table clear ⇒ the extended test fails on
a survivor-row assertion) is produced by temporarily editing the production module during
implementation, observing the failure, and then restoring the file. Three rules govern it:

1. The mutation is a **verification step**, not a deliverable. It is expected, and
   performing it is explicitly in scope for the owning task.
2. The restoration must be **proven**, not asserted: the production file is byte-identical
   to its pre-task state at task completion, and the failure observed under the mutation is
   recorded verbatim (test name plus the exact assertion-failure message) in the task's test
   record.
3. Because of rule 2, the production module is **deliberately absent** from the task's
   declared file set and from the SPEC's declared change set. Touching it transiently is
   therefore not a plan deviation; leaving it changed at completion is a defect.

Affected: task0001. Rationale: without the mutation the extended test is green both before
and after the change, so nothing else would demonstrate that the new assertions actually
discriminate.

### D3 — The two clear sites are mutually redundant; that limitation is recorded, not fixed

Within a single push, the evicted absolute row and the new bottom absolute row coincide, so
the two clearing sites are redundant for this fixture: removing only one of them leaves the
test green. This feature does **not** attempt to build a fixture that separates the two
sites. Instead the limitation is written into the test body as a comment, so a future reader
cannot mistake a passing test for proof that each site is independently covered. A
single-site removal that leaves the test green is a known, documented outcome — not a failed
regression demonstration.

Affected: task0001. Rationale: this knowledge currently exists only in a sibling feature's
test record, which is not where a reader of the test will look.

### D4 — The test's "red" step is the mutation, not a pre-existing failure

The behavior being pinned is already correct in production, so the extended test passes
immediately once written. The TDD contract for this feature is therefore satisfied by the D2
mutation: the new assertions must be demonstrated to fail against deliberately broken
production behavior before the task is considered done. A task that only shows green has not
shown that its new assertions observe anything.

Affected: task0001.

### D5 — Fixture geometry may widen, but only if a clearer survivor row requires it

The current fixture shape (a narrow, two-row viewport with scrollback disabled) is expected
to yield exactly one recycled row and one survivor row per line feed. Widening the fixture's
row or column count is permitted if that assumption does not hold in practice, provided the
scrollback-disabled eviction branch is still the branch exercised. Changing the fixture so
that a different clearing branch runs would silently retarget the test and is not permitted.

Affected: task0001.

### D6 — The sibling test with the same blind spot stays untouched

The sibling test that asserts only emptiness of the two side tables shares this observation
gap, and a whole-table clear satisfies it trivially. It is deliberately out of scope
(requirement FR7, excluded) and must not be edited, renamed, or re-asserted as a
convenience while nearby lines are being changed.

Affected: task0001 (as a prohibition).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The survivor row's content never reaches the overflow table, making the new survival assertions vacuously true | Medium | High — the feature would ship a test that proves nothing, recreating the very gap it closes | D-anti-vacuity convention: mandatory pre-scroll existence assertions on the survivor key (FR3), which fail loudly instead of passing silently |
| The survivor row is itself recycled by the scroll, so "survival" is asserted on the wrong row | Low | High | The survivor's absolute row key is captured before the scroll and the recycled row's key is asserted absent in the same test; if the two coincided, the survival and removal assertions would contradict each other and the test would fail |
| The transient production mutation is left in the final diff | Low | High — violates the test-only containment requirement (NFR1) | D2 rule 2: byte-identity of the production file is proven at task completion and re-confirmed at verification by inspecting the integrated diff |
| A reader takes the green test as proof that each clearing site is individually covered | Medium | Medium | D3: the redundancy is recorded as a comment in the test body (FR6) |
| Formatting drift outside the edited region is introduced by a crate-wide rewrite | Low | Medium | Formatting is run in check mode only; edits stay inside the one test function and its comment block |

## Open Questions

- [ ] FR7 (excluded): whether the sibling emptiness-only test is strengthened or deliberately
      left as an emptiness check is deferred to a separate feature. It is recorded here so the
      observation is not lost; it is not resolvable inside this feature's scope.
