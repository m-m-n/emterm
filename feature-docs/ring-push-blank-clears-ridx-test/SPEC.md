# Feature: ring-push-blank-clears-ridx-test

## Overview

`test_ring_push_blank_clears_ridx` in `crates/term_core/src/ring_buffer/tests.rs`
currently cannot observe whether the scrollback-enabled compress branch of
`ring_push_blank` clears overflow entries belonging to rows other than the
evicted one. This feature strengthens that test with a survivor-row fixture so
row-scope loss becomes detectable, and pins the test's provable and unprovable
scope in a doc comment. Requirements are defined in
`feature-docs/ring-push-blank-clears-ridx-test/REQUIREMENTS.md`.

## Objectives

- Strengthen `test_ring_push_blank_clears_ridx` so it observes that the
  scrollback-enabled compress branch of `ring_push_blank` does not clear
  overflow entries other than the evicted row's, removing the last place in
  term_core where a row-scope-loss regression goes undetected.
- Apply the survivor-row approach already introduced in the sibling test
  `test_ring_push_blank_clears_recycled_row_overflow_entries` (feature
  `ring-push-blank-row-scope-test` / PR #48) to the scrollback-enabled branch
  so coverage matches.
- Pin what the test can and cannot prove (its structural ceiling) in a doc
  comment, so the unsatisfiable demand to "directly pin the compress branch's
  clear site firing" is not raised again.

## User Stories

### US1: Detect row-scope loss in the compress branch
As a term_core developer, I want `test_ring_push_blank_clears_ridx` to fail
when `ring_push_blank`'s compress branch clears overflow entries beyond the
evicted row, so that a row-scope-loss regression is caught by the test suite.

**Acceptance Criteria:**
- [ ] AC1: The fixture gains a survivor row, and after a single push the test
      asserts that the recycled row's `overflow` / `overflow_ridx` entries are
      gone while the survivor row's entries remain — asserted separately for
      each of the two tables.
- [ ] AC2: Before the push, the test asserts via both `overflow` and
      `overflow_ridx` that the recycled row and the survivor row genuinely have
      overflow-bound content.
- [ ] AC5: With the compress branch's row-scoped clear replaced by a
      clear-everything mutation, `test_ring_push_blank_clears_ridx` is confirmed
      red; the result is recorded as verification evidence and the mutation is
      reverted so no production-code diff remains.

### US2: Keep the existing cycle-exhaustion property and pin the coverage ceiling
As a term_core developer, I want the existing "everything empties out after a
full cycle" assertions kept and the test's structural ceiling documented, so
that neither property is lost and no unsatisfiable acceptance condition is
proposed later.

**Acceptance Criteria:**
- [ ] AC3: The total push count stays at 5 and the existing
      `core.overflow.is_empty()` / `core.overflow_ridx.is_empty()` assertions
      remain, neither deleted nor replaced.
- [ ] AC4: The test's doc comment states what is provable (nothing beyond the
      evicted row is cleared), what is not (that the compress branch's clear
      site fired), and the structural reason — `new_bottom_abs == evicted_abs`
      makes it indistinguishable from Step 3's unconditional clear.
- [ ] AC6: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      crates/term_core/Cargo.toml --lib` is green.
- [ ] AC7: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is
      clean.

## Technical Requirements

### Functional Requirements

- **FR1 — Replace the fixture with one that contains a survivor row:** Extend
  the fixture of `test_ring_push_blank_clears_ridx`
  (`crates/term_core/src/ring_buffer/tests.rs:417` area), keeping
  `TerminalCore::new(10, 3, 2)` (cols=10 / rows=3 / scrollback capacity=2), so
  that before the push, overflow-bound content is placed on two rows: the row
  recycled by the first push, and a survivor row that is not recycled. The
  baseline form places `long = "👨‍👩‍👧‍👦"` via `set_cell` at row 0 col 0 and
  row 1 col 1 (`set_cell`'s argument order is col, row). Exact coordinates and
  string may be adjusted during implementation.
- **FR2 — Capture absolute row numbers before the push:** Obtain and hold the
  absolute row numbers of the recycled row and the survivor row before the push
  via `viewport_abs(0)` / `viewport_abs(1)`. Because `ring_head` rotates on
  every push, viewport-relative coordinates after the push do not point at the
  same absolute rows.
- **FR3 — Anti-vacuity pre-assert:** Before the push, assert via both
  `overflow` and `overflow_ridx` that the content of both the recycled row and
  the survivor row is genuinely overflow-bound. The existing
  `assert!(!core.overflow_ridx.is_empty())` is either folded into this
  pre-assert or strengthened into a more specific per-row-key assert. This
  prevents a fixture that cannot exceed the inline cap from silently
  trivializing the post-assertions.
- **FR4 — Row-scope assertions immediately after one push:** After calling
  `ring_push_blank(PackedColor::DEFAULT)` exactly once, assert that (a) the
  `overflow` entry for the recycled row's absolute row key is gone, (b) the same
  row key is gone from `overflow_ridx`, (c) the survivor row's `overflow` entry
  remains, and (d) the survivor row's `overflow_ridx` membership remains.
  `overflow` and `overflow_ridx` are checked independently.
- **FR5 — Timing constraint for the row-scope assertions:** FR4's assertions are
  performed before the push count reaches rows (=3), i.e. before the survivor
  row itself is evicted. Asserting at the one-push point satisfies this.
- **FR6 — Preserve the existing emptiness assertions:** After FR4, push 4 more
  times (5 in total, the same count as before) and keep the existing
  `assert!(core.overflow.is_empty())` /
  `assert!(core.overflow_ridx.is_empty())` as they are. The row-scope
  assertions verify "a single eviction does not over-clear", while the
  emptiness assertions verify "everything ends up empty once all rows have
  cycled"; these are distinct properties, so neither replaces the other.
- **FR7 — Document the coverage boundary in the doc comment:** State in the
  test's doc comment that what the test proves is "the compress branch does not
  sweep away rows other than the evicted one", not "the compress branch's clear
  site fired". Include the reason: `ring_push_blank` has three clear sites (the
  compress branch, the scrollback-disabled branch, and Step 3's unconditional
  clear of the new viewport's bottom row), and within a single push
  `new_bottom_abs == evicted_abs` always holds regardless of rows, so the
  eviction-time clear and Step 3's unconditional clear necessarily target the
  same absolute row (i.e. no fixture can distinguish them).
- **FR8 — Confirm red via mutation:** With the compress branch of
  `crates/term_core/src/ring_buffer.rs` (around lines 177-180) mutated so that
  `overflow_clear_row` / `overflow_ridx_clear_row` are replaced by
  `self.overflow.clear()` / `self.overflow_ridx.clear()`, confirm the
  strengthened `test_ring_push_blank_clears_ridx` turns red. After confirming,
  the mutation is always reverted, leaving no production-code diff.

### Non-Functional Requirements

- **NFR1 - Maintainability (no production-code change):** The only file changed
  is `crates/term_core/src/ring_buffer/tests.rs`. FR8's mutation is temporary
  and used only during verification; the final deliverable contains no diff to
  `crates/term_core/src/ring_buffer.rs`.
- **NFR2 - Maintainability (follow the existing test style):** Mirror the
  structure of the sibling test
  `test_ring_push_blank_clears_recycled_row_overflow_entries` (capture absolute
  row keys → anti-vacuity pre-assert → operate → removal / survival
  post-asserts) and its doc-comment phrasing. Per `test/README.md`'s convention,
  keep the test in the inline `#[cfg(test)] mod tests {}`, and add no new
  dev-dependency (proptest / criterion, etc.).
- **NFR3 - Performance (determinism and runtime):** The test is deterministic
  under parallel execution and does not require `--test-threads=1`. It is not
  made into a long-running path that requires `#[ignore]`.
- **NFR4 - Maintainability (formatting):** `cargo fmt --manifest-path
  crates/term_core/Cargo.toml --check` is clean. No crate-wide formatting sweep
  is performed; only the changed file is formatted.

## Implementation Approach

### Architecture

Test-only change inside the `term_core` crate. No layer above the crate is
touched.

```
crates/term_core/
├── src/ring_buffer.rs          # production: ring_push_blank (unchanged)
└── src/ring_buffer/tests.rs    # inline #[cfg(test)] mod tests — the only file changed
```

**Component Diagram:**

```
test_ring_push_blank_clears_ridx
  ├─ reads  TerminalCore::viewport_abs(row)   → absolute row key
  ├─ writes TerminalCore::set_cell(col, row, ...) → overflow-bound content
  ├─ calls  TerminalCore::ring_push_blank(PackedColor::DEFAULT)
  └─ asserts on pub(crate) fields: overflow, overflow_ridx
```

### Data Flow

```
set_cell (recycled row, survivor row)
  → anti-vacuity pre-assert on overflow / overflow_ridx      [FR3]
  → capture viewport_abs(0), viewport_abs(1)                  [FR2]
  → ring_push_blank x1
  → row-scope post-assert: recycled gone / survivor remains   [FR4, FR5]
  → ring_push_blank x4  (5 total)
  → emptiness assert: overflow / overflow_ridx both empty     [FR6]
```

### Data Structures Under Assertion

| Table | Key | Value |
|---|---|---|
| `overflow` | `(col: u32, abs_row: u32)` | overflow-bound cell content |
| `overflow_ridx` | `abs_row: u32` | set of columns |

Both are `pub(crate)` fields of `TerminalCore`; the test lives in the same
crate's `#[cfg(test)]` module and accesses them directly.

### Dependencies

**Internal Dependencies:**
- `crates/term_core/src/ring_buffer.rs`: the `ring_push_blank` implementation
  under test (read-only; mutated only temporarily for FR8).
- Sibling test `test_ring_push_blank_clears_recycled_row_overflow_entries`
  (feature `ring-push-blank-row-scope-test` / PR #48): the structural model this
  test mirrors.

**External Dependencies:**
- None. No new dev-dependency is added (NFR2).

### File Structure

```
crates/term_core/
└── src/
    ├── ring_buffer.rs           # unchanged in the final deliverable (NFR1)
    └── ring_buffer/
        └── tests.rs             # test_ring_push_blank_clears_ridx (~line 417)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/ring-push-blank-clears-ridx-test/**`
- `test-docs/ring-push-blank-clears-ridx-test/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS1 — Row-scope observation (normal case)** (AC1, AC2; FR1, FR2, FR3,
      FR4, FR5): Place overflow content on the recycled row and the survivor row
      of `TerminalCore::new(10, 3, 2)`, capture the absolute row keys, call
      `ring_push_blank` once, and confirm only the recycled row is cleared while
      the survivor remains.
- [ ] **TS2 — Emptiness after a full cycle (existing property)** (AC3; FR6):
      Continuing from TS1, push 4 more times (5 in total) and confirm both
      `overflow` and `overflow_ridx` become empty.

### Integration Tests

None. The change is confined to one unit test in `term_core`.

### Mutation Tests

- [ ] **TS3 — Mutation detection (red confirmation)** (AC5; FR8, NFR1): Replace
      the compress branch's row-scoped clear in `ring_buffer.rs` with
      `self.overflow.clear()` / `self.overflow_ridx.clear()`, run
      `cargo test --manifest-path crates/term_core/Cargo.toml --lib`, confirm
      `test_ring_push_blank_clears_ridx` fails, then restore.

### Regression Tests

- [ ] **TS4 — Crate-wide regression** (AC6; FR1, FR2, FR3, FR4, FR5, FR6, NFR3):
      Run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      crates/term_core/Cargo.toml --lib` after restoring, and confirm every
      term_core `--lib` test is green.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Formatting

- [ ] **TS5 — Formatting** (AC7; NFR4): Run `cargo fmt --manifest-path
      crates/term_core/Cargo.toml --check` and confirm there is no diff.

### Edge Cases

- [ ] Anti-vacuity: a fixture that cannot exceed the inline cap would silently
      trivialize the post-assertions — guarded by FR3's pre-assert.
- [ ] Absolute-row drift: `ring_head` rotates on every push, so
      viewport-relative coordinates after the push do not address the same
      absolute rows — guarded by FR2's pre-push capture.
- [ ] Survivor eviction: once the push count reaches rows (=3) the survivor row
      is itself evicted — guarded by FR5's one-push timing.

## Security Considerations

Not applicable. The change is a unit test inside `term_core` with no input
handling, no network surface and no persisted data.

## Error Handling

Not applicable. The change introduces no runtime error paths; failures surface
as test assertion failures.

## Structural Coverage Ceiling

`ring_push_blank` has three clear sites: the compress branch, the
scrollback-disabled branch, and Step 3's unconditional clear of the new
viewport's bottom row. Within a single push, `new_bottom_abs == evicted_abs`
always holds regardless of `rows` (Step 2 sets
`ring_head = (ring_head + 1) % rows`; Step 3 computes
`new_bottom_abs = (ring_head + rows - 1) % rows`, which equals the old
`ring_head`, i.e. `evicted_abs`). The eviction-time clear and Step 3's
unconditional clear therefore always target the same absolute row, so no fixture
can distinguish them. An acceptance condition requiring proof that "the compress
branch's clear site fired" is consequently unsatisfiable in principle, and this
specification does not require it (FR7 records this in the test's doc comment).

## Success Criteria

- [ ] All functional requirements (FR1-FR8) are implemented.
- [ ] All acceptance criteria (AC1-AC7) hold.
- [ ] All test scenarios (TS1-TS5) pass.
- [ ] No diff remains in `crates/term_core/src/ring_buffer.rs` (NFR1).
- [ ] `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean
      (NFR4).

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every FR and NFR is `confirmed`.

## References

- Requirements document: `feature-docs/ring-push-blank-clears-ridx-test/REQUIREMENTS.md`
- Test under change: `crates/term_core/src/ring_buffer/tests.rs`
  (`test_ring_push_blank_clears_ridx`, around line 417)
- Implementation under test: `crates/term_core/src/ring_buffer.rs`
  (compress branch, around lines 177-180)
- Sibling test: `test_ring_push_blank_clears_recycled_row_overflow_entries`
  (feature `ring-push-blank-row-scope-test` / PR #48)
- Test placement convention: `test/README.md`
