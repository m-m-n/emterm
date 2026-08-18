# Feature: ring-push-blank-row-scope-test

## Overview

The existing survivor test `test_ring_push_blank_clears_recycled_row_overflow_entries`
(`crates/term_core/src/ring_buffer/tests.rs:444`) observes only that *some* overflow
side-table clearing happened, not that the clearing is scoped to the evicted absolute
row. This feature adds a second, non-recycled viewport row ("survivor row") to that
fixture and asserts that its overflow entries survive the scroll, so the row-scoped
clear is pinned by the test. The change is test-only; `crates/term_core/src/ring_buffer.rs`
is unchanged in the final diff.

Requirements document: `feature-docs/ring-push-blank-row-scope-test/REQUIREMENTS.md`.

## Objectives

- Make the existing survivor test actually observe `ring_push_blank`'s row scope, so
  that the overflow side-table clearing is pinned to the evicted absolute row rather
  than merely "some clearing happened" (OBJ-1).
- Close the observation gap recorded as finding `821776efcf3c8be9`: a regression that
  replaced the row-scoped `overflow_clear_row` / `overflow_ridx_clear_row` pair with a
  whole-table clear would leave every current assertion green (OBJ-2).
- Keep the change test-only — no behavioral change to `crates/term_core/src/ring_buffer.rs`
  production code (OBJ-3).

## User Stories

### US1: Row-scoped clear is pinned by the test suite

As a `term_core` developer, I want the `ring_push_blank` test to observe which row's
overflow entries were cleared, so that a whole-table clear regression fails the suite
instead of passing silently.

**Acceptance Criteria:**
- [ ] AC1: The fixture contains a survivor row distinct from the row recycled by the
      scroll, holding overflow-bound content in at least one column, and `abs_survivor`
      is captured via `viewport_abs` before the scroll.
- [ ] AC2: Pre-assertions confirm that both the to-be-recycled row's and the survivor
      row's entries genuinely exist in `overflow` and `overflow_ridx` before the scroll.
- [ ] AC3: Post-assertions confirm the recycled row's `overflow` entries are gone and its
      `overflow_ridx` key is absent (existing behavior, preserved).
- [ ] AC4: Post-assertions confirm the survivor row's `overflow` entries and its
      `overflow_ridx` key (with the expected column set) are still present.
- [ ] AC6: With `ring_push_blank`'s row-scoped clear temporarily replaced by a whole-table
      clear, the extended test fails on the survivor-row assertions; the mutation is
      reverted afterwards and no diff of it remains.

### US2: The redundancy limitation is recorded where it is read

As a `term_core` developer, I want the test body to state that removing only one of the
two clear sites leaves the test green, so that I do not read a passing test as proof that
both sites are individually covered.

**Acceptance Criteria:**
- [ ] AC5: A comment in the test body records the single-site-removal /
      `new_bottom_abs == evicted_abs` redundancy.
- [ ] AC7: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      passes with no other test regressions in the crate.
- [ ] AC8: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean.

## Technical Requirements

### Functional Requirements

- **FR1 — Survivor row added to the fixture** *(status: resolved)*:
  `test_ring_push_blank_clears_recycled_row_overflow_entries` gains a second,
  non-recycled viewport row holding overflow-bound cells (the "survivor row"), populated
  through `handle_print` in the same style as the existing row-0 cells so the fixture
  shape stays consistent. Its absolute row key is captured via `viewport_abs` before the
  scroll. The ticket's concrete proposal: after writing `'e'` / `'f'` to row 0,
  `core.set_cursor(0, 1);` and print survivor content (`'g'` plus the same 8 combining
  marks), then `let abs_survivor = core.viewport_abs(1) as u32;`.

- **FR2 — Survivor-row post-assertions** *(status: resolved)*:
  After the line feed that drives the full-screen scroll path through `ring_push_blank`,
  the test asserts that the survivor row's entries are STILL present:
  `assert!(core.overflow.contains_key(&(0u32, abs_survivor)));` and `overflow_ridx` still
  holds the survivor absolute row key with its column set intact. The key is unchanged
  across the scroll because the row's ring slot / absolute id is stable even though it
  moves to viewport row 0.

- **FR3 — Anti-vacuity pre-assertions extended** *(status: resolved)*:
  The existing anti-vacuity pre-assert block is extended to cover the survivor row
  (`assert!(core.overflow.contains_key(&(0u32, abs_survivor)));`), so a fixture that fails
  to push the survivor cells past the inline cap cannot make the new assertions vacuously
  true.

- **FR4 — Existing recycled-row assertions preserved** *(status: resolved)*:
  The current recycled-row post-assertions (`!overflow.contains_key(&(0, abs0))`,
  `!overflow.contains_key(&(1, abs0))`, `!overflow_ridx.contains_key(&abs0)`) remain
  unchanged; the feature only adds observation, never removes it.

- **FR5 — Regression-detection demonstrated** *(status: resolved)*:
  The extended test must fail when `ring_push_blank`'s row-scoped clear is temporarily
  replaced by a whole-table clear (`self.overflow.clear()` / `overflow_ridx.clear()`),
  demonstrated once during implementation and reverted before the change is finalized.

- **FR6 — Redundancy comment in the test body** *(status: resolved)*:
  The test body carries a comment stating that removing only ONE of the two clear sites
  leaves the test green, because `new_bottom_abs == evicted_abs` makes the two sites
  redundant. This knowledge is currently recorded only in
  `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` AC-4.

- **FR7 — Sibling test `test_ring_push_blank_clears_ridx` scope** *(status: excluded)*:
  `test_ring_push_blank_clears_ridx` (`crates/term_core/src/ring_buffer/tests.rs:417`)
  shares the same blindness — it asserts only `overflow.is_empty()` /
  `overflow_ridx.is_empty()`, which a whole-table clear satisfies trivially. It is NOT
  touched by this feature.
  *Exclusion reason:* Out of scope by answered question `requirement.sibling-test-scope`
  (packet create-spec-q0001, option `separate_task`, resolved by Codex consultation in
  batch mode). Recorded as an explicit follow-up so the observation is not lost — a
  separate task should decide whether that sibling assertion is strengthened or
  deliberately left as an emptiness check.

### Non-Functional Requirements

- **NFR1 — Change containment:** Test-only change. No edit to production code under
  `crates/term_core/src/` outside the inline `tests` module; in particular
  `ring_buffer.rs`'s `ring_push_blank` is unchanged (the mutation in FR5 is a temporary
  verification step, reverted).
- **NFR2 — Conventions:** Follow the crate's existing test conventions: inline
  `#[cfg(test)]` module, `test_*` function naming, fixture kept in
  `crates/term_core/src/ring_buffer/tests.rs` adjacent to the test it extends, explanatory
  comment block above the test kept accurate.
- **NFR3 — Dependencies:** No new dependencies. `crates/term_core` keeps `mux_ipc` as its
  only dev-dependency; no proptest, criterion, or other test framework is introduced.
- **NFR4 — Determinism:** Deterministic under the standard cargo test harness — no timing,
  ordering, or parallelism sensitivity, so the test is stable without `--test-threads=1`.
- **NFR5 — Formatting:** `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
  reports clean; fmt is never run crate-wide in rewrite mode.

## Implementation Approach

### Architecture

No architectural change. The feature edits one inline test module inside the
`crates/term_core` library crate.

```
crates/term_core
└── src/ring_buffer.rs            # production: ring_push_blank (UNCHANGED)
    └── src/ring_buffer/tests.rs  # inline #[cfg(test)] module (the only edited file)
        └── test_ring_push_blank_clears_recycled_row_overflow_entries  (extended)
```

**Component Diagram:**

```
TerminalCore
 ├── ring buffer slots ── absolute row ids (stable across scroll)
 ├── overflow        : map keyed by (col: u32, abs_row: u32)
 └── overflow_ridx   : map keyed by abs_row -> column set

ring_push_blank(evicted_abs)
 └── overflow_clear_row(evicted_abs) + overflow_ridx_clear_row(evicted_abs)
     ^ row scope under test
```

### Data Flow

```
handle_print(row 0 cells: 'e'/'f' + marks)  -> overflow[(col, abs0)]
set_cursor(0, 1) + handle_print('g' + marks) -> overflow[(0, abs_survivor)]
abs0 / abs_survivor captured via viewport_abs(...) before the scroll
pre-assert both rows present in overflow / overflow_ridx
line feed -> full-screen scroll -> ring_push_blank(abs0)
post-assert: abs0 entries gone; abs_survivor entries intact
```

### API Design

No API change. `overflow` is keyed by `(col: u32, abs_row: u32)` and `overflow_ridx` by
`abs_row`, so per-row survival is directly observable through those two maps without new
accessors (A2).

### Database Schema

Not applicable — no persisted data.

### Dependencies

**Internal Dependencies:**
- `crates/term_core` `ring_buffer` module: the `ring_push_blank` code path under
  observation; not modified.

**External Dependencies:**
- None added. `crates/term_core` keeps `mux_ipc` as its only dev-dependency (NFR3).

### File Structure

```
crates/term_core/
└── src/
    ├── ring_buffer.rs            # unchanged in the final diff
    └── ring_buffer/
        └── tests.rs              # extended fixture + assertions + comment
```

## Declared Change Set

Feature-specific paths:

- `crates/term_core/src/ring_buffer/tests.rs`

Every SPEC declares, by default, the following two workflow-generated entries in addition
to the feature-specific paths above:

- `feature-docs/ring-push-blank-row-scope-test/**`
- `test-docs/ring-push-blank-row-scope-test/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`, `IMPLEMENTATION.md`,
`workflow.yaml`, `phase-state/`, `tasks/`, `reviews/roundN.yaml`, `VERIFICATION.md`,
`retrospect.yaml`, and the design artifacts the design step produces. These are generated
and owned by the phase documents and by `references/phase-state.md`; this section cites
them and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/ring-push-blank-row-scope-test/{T}.tests.yaml`,
the per-task test record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author explicitly
removes them; their absence is never assumed by silence — removal is a deliberate,
explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at verification
time must be CONTAINED IN the declared set, not equal to it. A feature that produces no
implement tasks generates no `test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared path that never
materializes is not a violation.

`crates/term_core/src/ring_buffer.rs` is deliberately NOT declared: the FR5 mutation is
transient and reverted, so the final diff leaves that file unchanged (A4, NFR1).

## Test Scenarios

### Unit Tests

- [ ] **TS1 — Row-scoped clear observed** (FR1, FR2, FR3, FR4): Populate the recycled row
      and the survivor row with overflow-bound cells, pre-assert both, line-feed to trigger
      the full-screen scroll path, then assert recycled-row entries cleared AND survivor-row
      entries intact.
- [ ] **TS2 — Anti-vacuity guard** (FR3): Verify the pre-assertions fail loudly if the
      fixture's content no longer exceeds the inline cap (i.e. the cells are not actually
      overflow-bound).
- [ ] **TS3 — Mutation check** (FR5): Temporarily substitute a whole-table clear inside
      `ring_push_blank`; confirm the test fails, then revert.

### Integration Tests

- [ ] **TS4 — Crate-level regression run** (FR7, NFR1): Run the full `term_core` lib test
      suite to confirm no neighbouring fixture (including `test_ring_push_blank_clears_ridx`,
      untouched) regressed.

**Run command:**
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

No E2E infrastructure exists in this project.

### Edge Cases

- [ ] Survivor cells that do not exceed the inline cap: the extended anti-vacuity
      pre-assertions (FR3) fail rather than letting the survivor post-assertions pass
      vacuously.
- [ ] Both clear sites are redundant because `new_bottom_abs == evicted_abs`: removing only
      one of them leaves the test green; this limitation is recorded as a comment in the
      test body (FR6) rather than claimed as covered.

### Performance Tests

Not applicable.

## Security Considerations

Not applicable — test-only change with no input handling, no I/O, and no exposed surface.

## Error Handling

Not applicable — assertions fail the test through the standard cargo test harness.

## Performance Optimization

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are implemented; FR7 remains excluded.
- [ ] All test scenarios (TS1–TS4) pass.
- [ ] AC1–AC8 in REQUIREMENTS.md section 11.1 are satisfied.
- [ ] The final diff touches only `crates/term_core/src/ring_buffer/tests.rs` plus the
      workflow-generated entries (NFR1).
- [ ] `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean (NFR5).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None — no requirement carries `status: tbd`.

Deliberately out of scope (not open questions):

- FR7: whether `test_ring_push_blank_clears_ridx`'s emptiness assertion is strengthened or
  deliberately left as-is, deferred to a separate task.
- The DECSTBM scroll-region path (`shift_rows_up`) clear site, per the ticket.
- The `cols <= 2` cursor-out-of-range defect (finding `3e769a761d85d839`), per the ticket.

## Assumptions

- **A1:** With the fixture's `TerminalCore::new(cols, 2, 0)` shape (2 viewport rows,
  scrollback disabled), a single line feed from the last row recycles exactly one ring
  slot, leaving the other viewport row available as the survivor row. The fixture's
  row/column counts may be widened if a clearer survivor row is needed.
- **A2:** `overflow` is keyed by `(col: u32, abs_row: u32)` and `overflow_ridx` by
  `abs_row`, so per-row survival is directly observable through those two maps without new
  accessors.
- **A3:** The existing `marks` sequence (8 combining marks after a base character) is what
  pushes a cell past the inline cap; the survivor row reuses the same technique.
- **A4:** The FR5 mutation check is performed transiently during implementation and
  reverted; the final diff leaves `crates/term_core/src/ring_buffer.rs` unchanged.
- **A5:** The required explanatory comment is written in English, matching the surrounding
  test module's style.
- **A6:** All edits are confined to `crates/term_core/src/ring_buffer/tests.rs`; no new
  dev-dependency and no new test file are introduced.

## Implementation Phases (if applicable)

Single phase — the change is one extended test function.

## References

- Requirements document: `feature-docs/ring-push-blank-row-scope-test/REQUIREMENTS.md`
- Test under extension: `crates/term_core/src/ring_buffer/tests.rs:444`
  (`test_ring_push_blank_clears_recycled_row_overflow_entries`)
- Sibling test (out of scope): `crates/term_core/src/ring_buffer/tests.rs:417`
  (`test_ring_push_blank_clears_ridx`)
- Production code (unchanged): `crates/term_core/src/ring_buffer.rs`
- Existing record of the redundancy knowledge:
  `test-docs/relocate-wrap-ec1-scroll-test/task0001.tests.yaml` AC-4
- Observation-gap finding: `821776efcf3c8be9`
- Out-of-scope finding: `3e769a761d85d839`
- Project license: MIT
