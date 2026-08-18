# Feature: relocate-wrap-ec1-scroll-test

Requirements document: `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md`

## Overview

The EC1 scroll-path test in `crates/term_core/src/print_handler/tests.rs:1559`
claims — through its name, its leading comment and two assertions — that it
pins an overflow-cleanup property, but `core.overflow` is empty for that
test's entire run, so those assertions are vacuous. This feature corrects
the test so it claims only what it proves, adds a new test that genuinely
pins `ring_push_blank`'s eviction-time overflow clearing, records in this
SPEC why the relocation deletion branches are unreachable on the scroll
path, and reconciles the test-docs records. No production code changes.

Origin: review finding `532f5e5cbe0763e7` (severity medium, confidence 65)
of feature `relocate-wrap-overflow-cleanup`, recorded in
`feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`.

## Objectives

- Make the EC1 scroll-path test's name, leading comment and assertions claim
  only what the test actually proves, so no reader is misled into believing
  an overflow-cleanup property is pinned there.
- Pin `ring_push_blank`'s eviction-time overflow clearing with a test that
  genuinely fails when that clearing is removed, so the property EC1 appeared
  to cover is covered for real.
- Record in SPEC, as a stated fact with its mechanism, that the relocation
  deletion branches at `print_handler.rs:493` / `518` are unreachable on the
  scroll path by construction, so this finding is not re-raised by a future
  reader.
- Keep the test-docs records consistent with reality: own this feature's
  record and correct the stale AC-6 claim in the
  `relocate-wrap-overflow-cleanup` record.
- Change nothing about production behavior and weaken no existing test, in
  particular TS1
  (`test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`).

## User Stories

### US1: A reader can trust what a test's name claims

As a term_core developer reading `print_handler/tests.rs`, I want the EC1
scroll-path test's name, comment and assertions to describe only the
property it can observe, so that I do not conclude that overflow cleanup is
pinned on the scroll path when it is not.

**Acceptance Criteria:**

- [ ] AC-1: the test is named
      `test_relocate_widened_base_via_wrap_scrolls_without_panic` and its
      leading comment claims only no-panic / no out-of-range access plus
      correct placement of the relocated base and spacer on the resolved
      row. Neither name nor comment asserts any overflow-entry property.
- [ ] AC-2: the two `!core.overflow.contains_key(...)` assertions and the
      `abs1` binding are gone from that test, and no remaining assertion in
      it is vacuous under an empty `core.overflow`. The placement assertions
      (cursor row 1, `"5\u{FE0F}"`, widths 2 and 0) remain.

### US2: The eviction-time overflow clearing is actually pinned

As a term_core developer, I want a test whose failure follows from removing
`ring_push_blank`'s overflow clearing, so that the property EC1 appeared to
cover is covered by a test that can actually detect its loss.

**Acceptance Criteria:**

- [ ] AC-3: `test_ring_push_blank_clears_recycled_row_overflow_entries`
      exists, pre-asserts that `overflow` holds `(0,abs0)` and `(1,abs0)` and
      that `overflow_ridx[&abs0]` holds columns 0 and 1, scrolls via a plain
      line feed with no relocation involved, and post-asserts both keys
      absent from `overflow` and `abs0` absent from `overflow_ridx`. It
      passes on unmodified code.
- [ ] AC-4: red is confirmed for AC-3's test by removing
      `ring_push_blank`'s `overflow_clear_row` / `overflow_ridx_clear_row`
      calls at both sites (`ring_buffer.rs:196-199` and `221-224`) and
      observing a failing assertion; the observed failure message is
      recorded with `red_confirmed: true`. The record also states that
      removing only one of the two sites leaves the test green, because
      `new_bottom_abs == evicted_abs` makes the two clears redundant for a
      single push.
- [ ] AC-5: `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
      (TS1, `tests.rs:1454`) is unchanged and green,
      `test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
      is unchanged and green, and `git diff` shows no change to any
      non-`#[cfg(test)]` source line.

### US3: The unreachability finding is not re-raised

As a reviewer, I want the SPEC to state why the relocation deletion branches
cannot fire on the scroll path, and the test-docs records to match reality,
so that this finding is resolved once instead of being rediscovered.

**Acceptance Criteria:**

- [ ] AC-6: SPEC states, with the three-part mechanism of FR6 and explicit
      file:line evidence, that `print_handler.rs:493` / `518` are unreachable
      on the scroll path by construction, and notes the `shift_rows_up`
      scroll-region path as a distinct, out-of-scope clearing site.
- [ ] AC-7: `test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml`
      exists and maps every AC above to its tests, and
      `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` AC-6
      carries the renamed test and a `red_reason` that describes the vacuity
      and its removal. No other entry in that file is altered.
- [ ] AC-8: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      is green and
      `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is
      clean.

## Technical Requirements

### Functional Requirements

- **FR1 — Rename the EC1 scroll-path test to a claim it proves:** Rename
  `test_relocate_widened_base_via_wrap_scrolls_without_panic_or_stale_entries`
  (`crates/term_core/src/print_handler/tests.rs:1559`) to
  `test_relocate_widened_base_via_wrap_scrolls_without_panic`. The
  `_or_stale_entries` suffix is dropped because the test cannot observe any
  overflow-entry property: `core.overflow` is empty for the whole test — it
  writes only `'A'..'D'` and the 4-byte VS16 merge, none of which exceed the
  16-byte inline cap. *(status: resolved)*

- **FR2 — Rewrite the EC1 leading comment:** Replace the leading comment at
  `tests.rs:1551-1557` so it states (a) what the test proves: the
  relocation's line feed may itself scroll the viewport, and the relocated
  base + spacer writes must land on the resolved row without panicking or
  reading out of range; and (b) why it proves nothing about overflow
  cleanup: `ring_push_blank` blanks the recycled slot's overflow keys before
  the relocated writes run, so the deletion branches cannot fire on this
  path. The comment must not assert or imply that stale overflow entries are
  checked here. *(status: resolved)*

- **FR3 — Drop the vacuous assertions from the EC1 test:** Remove the two
  vacuous assertions at `tests.rs:1579-1580`
  (`!core.overflow.contains_key(&(0u32, abs1))` / `&(1u32, abs1)`) and the
  now-unused `let abs1 = core.viewport_abs(1) as u32;` binding at
  `tests.rs:1578`. Keep every assertion that is genuinely observable: cursor
  row pinned to 1 after the scroll, `get_cell_char(0,1) == "5\u{FE0F}"`,
  `get_cell_width(0,1) == 2`, `get_cell_width(1,1) == 0`, plus the pre-scroll
  cursor position assertions. No assertion may remain in this test whose
  truth is independent of the code under test. *(status: resolved)*

- **FR4 — Add a test that pins `ring_push_blank`'s eviction-time overflow
  clearing:** Add `test_ring_push_blank_clears_recycled_row_overflow_entries`
  in `crates/term_core/src/ring_buffer/tests.rs`, involving no relocation:
  construct `TerminalCore::new(5, 2, 0)`; write viewport row 0 col 0 and
  col 1 each as a base char plus the 8 combining marks `0x0301..0x0308`
  (17 UTF-8 bytes > the 16-byte inline cap, the same fixture shape TS1 uses
  at `tests.rs:1457-1472`) so `overflow` and `overflow_ridx` genuinely hold
  entries; capture `let abs0 = core.viewport_abs(0) as u32;`; pre-assert
  `overflow.contains_key(&(0,abs0))`, `overflow.contains_key(&(1,abs0))` and
  that `overflow_ridx[&abs0]` contains both 0 and 1; then place the cursor on
  the last row and emit a plain line feed (no DECSTBM scroll region, no VS16,
  no relocation) so the full-screen scroll path calls `ring_push_blank`; then
  assert `!overflow.contains_key(&(0,abs0))`,
  `!overflow.contains_key(&(1,abs0))` and `!overflow_ridx.contains_key(&abs0)`.
  Placed beside the existing `test_ring_push_blank_clears_ridx`
  (`ring_buffer/tests.rs:417`) because the subject under test is
  `ring_push_blank`, not the print handler. *(status: resolved)*

- **FR5 — Red criterion for the new test, retargeted to the eviction path:**
  The red criterion is satisfied by removing `ring_push_blank`'s overflow
  clearing and observing FR4's test fail. Because
  `new_bottom_abs == evicted_abs` always holds
  (`new_bottom = ((evicted+1) + rows - 1) % rows = evicted`),
  `ring_buffer.rs`'s two clearing sites — the eviction-time clear for the
  exercised scrollback-disabled branch (`ring_buffer.rs:196-199`) and the
  new-bottom clear (`ring_buffer.rs:221-224`) — target the same ring slot and
  are mutually redundant. The red confirmation therefore removes BOTH sites
  in one mutation; the record must state that a single-site removal leaves
  the test green, and why. The pre-existing relocation deletion branches
  (`print_handler.rs:493` / `518`) stay pinned by TS1 on the no-scroll path;
  this feature must not modify, remove or weaken any assertion in
  `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`.
  *(status: resolved)*

- **FR6 — SPEC records the unreachability of the deletion branches on the
  scroll path:** SPEC must state, as fact rather than conjecture, that the
  deletion branches at `print_handler.rs:493` and `print_handler.rs:518`
  cannot fire on the scroll path, with the mechanism: (i)
  `viewport_abs(row) = (ring_head + row) % rows` (`ring_buffer.rs:75-82`)
  returns a ring slot index, so the row key the relocated writes use is
  exactly the slot `ring_push_blank` just recycled; (ii) `ring_push_blank`
  clears that slot's `overflow` / `overflow_ridx` keys at eviction time in
  all three scrollback branches (`ring_buffer.rs:147-148`, `178-179`,
  `197-198`) and again when blanking the new viewport bottom
  (`ring_buffer.rs:222-223`); (iii) the relocated writes run after
  `line_feed()` returns (`print_handler.rs:464-521`), so their
  `!self.overflow.is_empty()` guard short-circuits or the key is simply
  absent. SPEC also records that the DECSTBM scroll-region path via
  `shift_rows_up` is a distinct clearing site (`terminal_rows.rs:125-126`,
  `164-165`, `189-190`, `226-227`, `256-257`) and is out of this feature's
  scope. *(status: resolved; discharged by the "Unreachability of the
  deletion branches on the scroll path" section below)*

- **FR7 — test-docs reconciliation (decided):** This feature owns its own
  record `test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml` (NNNN
  assigned by the plan phase), following the existing per-task record
  convention. In addition, correct the stale AC-6 entry of
  `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml` (lines
  59-74) in place: update the first listed test name to the FR1 name, and
  restate `red_reason` to say that the scroll case's overflow assertions were
  vacuous (`core.overflow` empty for that test's whole run) and have been
  removed, that the test now claims only no-panic + correct placement, and
  that `ring_push_blank`'s eviction-time clearing is pinned by this feature's
  new test. `red_confirmed: false` for that entry stays correct. The second
  listed test (`..._no_panic_when_column_one_does_not_exist`) and every other
  AC entry in that file are left untouched. *(status: resolved)*

- **FR8 — No production code change:** The diff touches test code,
  SPEC/feature-docs and test-docs only. No non-`#[cfg(test)]` code path
  changes: `print_handler.rs`, `ring_buffer.rs`, `terminal_rows.rs` and every
  other production module stay byte-identical. *(status: resolved)*

### Non-Functional Requirements

- **NFR1 — Behavior preservation:** Zero runtime behavior change. The full
  term_core lib suite passes before and after, with the same set of passing
  tests plus the one added by FR4.
- **NFR2 — No new dependencies:** The new test uses only in-crate APIs
  already used by neighboring tests (`TerminalCore::new`, `handle_print`,
  `process_pty_data`, `viewport_abs`, `overflow`, `overflow_ridx`). No new
  dev-dependency is added to `crates/term_core/Cargo.toml`.
- **NFR3 — Suite determinism:** The new test constructs its own
  `TerminalCore` and touches no process-global state, so it remains
  parallel-safe under the default test harness; its runtime addition is
  negligible.
- **NFR4 — Convention conformance:** Test names follow the crate's
  `test_<subject>_<behavior>` convention, and each test carries a leading
  comment naming the AC / requirement IDs it covers, matching the surrounding
  style in `print_handler/tests.rs` and `ring_buffer/tests.rs`.
- **NFR5 — Format cleanliness:** The touched Rust files pass
  `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`.

## Implementation Approach

### Architecture

This is a test-only change inside the pure-Rust `term_core` crate. No UI
surface, no visual artifact, no user-visible behavior and no public API
change; the design step is therefore skipped. The modules involved:

```
crates/term_core/src/
├── print_handler.rs          # production — UNCHANGED (relocation writes 464-521,
│                             #              deletion branches 493 / 518)
├── print_handler/tests.rs    # EC1 test renamed / comment rewritten /
│                             # vacuous assertions removed  (FR1, FR2, FR3)
├── ring_buffer.rs            # production — UNCHANGED (viewport_abs 75-82,
│                             #              eviction clears 147-148 / 178-179 /
│                             #              196-199, new-bottom clear 221-224)
├── ring_buffer/tests.rs      # new test added beside test_ring_push_blank_clears_ridx
│                             # (line 417)                            (FR4)
└── terminal_rows.rs          # production — UNCHANGED, out of scope
                              # (shift_rows_up clears 125-126 / 164-165 /
                              #  189-190 / 226-227 / 256-257)
```

### Unreachability of the deletion branches on the scroll path (FR6)

The deletion branches at `print_handler.rs:493` and `print_handler.rs:518`
**cannot fire on the scroll path**. This is a structural fact, not a
conjecture, and it follows from three mechanisms:

1. **The row key is the slot that was just recycled.**
   `viewport_abs(row) = (ring_head + row) % rows` (`ring_buffer.rs:75-82`)
   returns a *ring slot index*, not a monotonically growing absolute line
   number. The row key the relocated writes use is therefore exactly the slot
   `ring_push_blank` just recycled.

2. **That slot's overflow keys are cleared at eviction time.**
   `ring_push_blank` clears the slot's `overflow` / `overflow_ridx` keys in
   all three scrollback branches (`ring_buffer.rs:147-148`, `178-179`,
   `197-198`), and clears them again when blanking the new viewport bottom
   (`ring_buffer.rs:222-223`).

3. **The relocated writes run afterwards.** The relocated base + spacer
   writes run after `line_feed()` returns (`print_handler.rs:464-521`), so by
   the time the deletion branches are evaluated, either their
   `!self.overflow.is_empty()` guard short-circuits, or the key is simply
   absent.

Consequently, a test that scrolls through `ring_push_blank` can never observe
the deletion branches removing anything — which is precisely why the EC1
test's overflow assertions were vacuous.

**Out of scope:** the DECSTBM scroll-region path via `shift_rows_up` is a
*distinct* clearing site (`terminal_rows.rs:125-126`, `164-165`, `189-190`,
`226-227`, `256-257`). This feature covers only the full-screen scroll path
(no DECSTBM scroll region); the scroll-region path is not addressed here.

### Redundancy of `ring_push_blank`'s two clearing sites (FR5)

`new_bottom_abs == evicted_abs` always holds, because
`new_bottom = ((evicted + 1) + rows - 1) % rows = evicted`. The eviction-time
clear for the exercised scrollback-disabled branch
(`ring_buffer.rs:196-199`) and the new-bottom clear
(`ring_buffer.rs:221-224`) therefore target the same ring slot and are
mutually redundant. No externally observable test can pin them
independently, so the red criterion removes both sites in one mutation, and
the test record must state that a single-site removal leaves the test green
and why.

### Data Flow (FR4's new test)

```
TerminalCore::new(5, 2, 0)
  → write row 0 col 0 / col 1 : base char + 0x0301..0x0308  (17 bytes > 16-byte inline cap)
  → abs0 = viewport_abs(0)
  → pre-assert  overflow{(0,abs0),(1,abs0)}  and  overflow_ridx[abs0] ⊇ {0,1}
  → cursor to last row → plain line feed
  → full-screen scroll → ring_push_blank(recycles slot abs0)
  → post-assert  overflow ∌ (0,abs0),(1,abs0)   and   overflow_ridx ∌ abs0
```

### API Design

Not applicable. No API surface is added or changed.

### Database Schema

Not applicable. No persistent data model is involved.

### Dependencies

**Internal Dependencies:**

- `crates/term_core` — in-crate test APIs only: `TerminalCore::new`,
  `handle_print`, `process_pty_data`, `viewport_abs`, `overflow`,
  `overflow_ridx` (NFR2).

**External Dependencies:**

- None. No dev-dependency is added to `crates/term_core/Cargo.toml` (NFR2).

### File Structure

```
crates/term_core/src/
├── print_handler/tests.rs                       # modified (FR1, FR2, FR3)
└── ring_buffer/tests.rs                         # modified (FR4)

feature-docs/relocate-wrap-ec1-scroll-test/
├── REQUIREMENTS.md                              # this feature's requirements
└── SPEC.md                                      # this document (FR6)

test-docs/
├── relocate-wrap-ec1-scroll-test/
│   └── taskNNNN.tests.yaml                      # new; NNNN assigned by plan phase (FR7)
└── relocate-wrap-overflow-cleanup/
    └── task0001.tests.yaml                      # AC-6 entry (lines 59-74) corrected in place (FR7)
```

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (FR1, FR2, FR3 → AC-1, AC-2) —
      `print_handler::tests::test_relocate_widened_base_via_wrap_scrolls_without_panic`:
      5x2 terminal, no scrollback; cursor on the last row, `'A'..'D'` then
      `'5'` at the last column, then VS16. The relocation's line feed scrolls
      the viewport. Asserts no panic, cursor pinned to row 1, and the
      relocated base/spacer placement. Carries no overflow assertions.
      *Red expectation:* not a defect pin — green before and after
      (robustness / no-regression). Verified by review that its claim now
      matches its assertions.

- [ ] **TS-2** (FR4, FR5 → AC-3, AC-4) —
      `ring_buffer::tests::test_ring_push_blank_clears_recycled_row_overflow_entries`:
      5x2 terminal, scrollback capacity 0; row 0 cols 0 and 1 pre-filled with
      overflow-bound width-1 content, pre-asserted present in both `overflow`
      and `overflow_ridx`; a plain line feed from the last row triggers the
      full-screen scroll; the recycled slot's keys must be clear afterwards.
      *Red expectation:* red-confirmed by removing both of
      `ring_push_blank`'s clearing sites together (single-site removal stays
      green — see AC-4).

### Integration Tests

Not applicable. Every scenario is a `term_core` unit test or a documentary
review.

### E2E Tests

**Existing E2E tests:** None (no E2E inputs were resolved for this feature).
**Run command:** Not detected.

### Regression

- [ ] **TS-3** (FR5, FR8 → AC-5) —
      `print_handler::tests::test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
      (unchanged) +
      `print_handler::tests::test_relocate_widened_base_via_wrap_clamps_cursor_when_column_one_does_not_exist`
      (unchanged): the deletion branches at `print_handler.rs:493` / `518`
      stay pinned on the no-scroll path exactly as before this feature.
      *Red expectation:* green; already red-confirmed for AC-1 / AC-2 / AC-3
      of `relocate-wrap-overflow-cleanup` task0001 and not re-derived here.

- [ ] **TS-4** (NFR1, NFR3, NFR5 → AC-8) — full crate suite plus format check
      for term_core. *Red expectation:* green.

### Documentary Verification

- [ ] **TS-5** (FR6 → AC-6) — review of SPEC.md: the reviewer confirms the
      unreachability statement, its three-part mechanism and its file:line
      evidence are present and accurate. *Red expectation:* no automated
      test; manual verification only.

- [ ] **TS-6** (FR7 → AC-7) — review of both `tests.yaml` records: the
      reviewer confirms this feature's own record exists and is complete, and
      that the `relocate-wrap-overflow-cleanup` AC-6 entry now names the
      renamed test and describes the vacuity correction, with no other entry
      touched. *Red expectation:* no automated test; manual verification only.

### Edge Cases

- [ ] The EC1 scroll case itself (TS-1): the relocation's own line feed
      scrolls the viewport, and the relocated base + spacer must still land on
      the resolved row without panicking or reading out of range.

### Performance Tests

Not applicable. NFR3 states the new test's runtime addition is negligible.

## Security Considerations

Not applicable. The change touches test code and documentation records only
(FR8); no input handling, authentication, authorization or data-protection
surface is involved.

## Error Handling

Not applicable. No new error paths or error codes are introduced.

## Performance Optimization

Not applicable. Zero runtime behavior change (NFR1).

## Constraints

- **W1 — the property is not previously unpinned.**
  `ring_buffer::tests::test_ring_push_blank_clears_ridx`
  (`ring_buffer/tests.rs:417`) already partially covers the same property, so
  FR5's red confirmation will very likely turn TWO tests red. This SPEC does
  not claim the property was previously unpinned.
- **W2 — the two clearing sites cannot be pinned independently.**
  `new_bottom_abs == evicted_abs` always holds, so `ring_push_blank`'s two
  clearing sites are mutually redundant and no externally observable test can
  pin them independently. This is why FR5's red criterion removes both sites
  together.
- **Production code is frozen** (FR8): `print_handler.rs`, `ring_buffer.rs`,
  `terminal_rows.rs` and every other non-`#[cfg(test)]` path stay
  byte-identical.
- **TS1 is frozen** (FR5): no assertion in
  `test_relocate_widened_base_via_wrap_removes_stale_overflow_entries_on_target_row`
  may be modified, removed or weakened.
- **Out of scope:** the `cols <= 2` cursor-out-of-range defect (review
  finding `3e769a761d85d839`) and the `shift_rows_up` scroll-region clearing
  path.

## Assumptions

Every assumption below is carried over from the requirements analysis; none
is originated here.

| ID | Assumption | Impact | Reversible |
|----|------------|--------|------------|
| a1 | "The deletion branch is actually executed" means the branch at `print_handler.rs:493` / `518` evaluates its `remove(...)` and that removal is observable, not merely that the guard expression is reached. | high | yes |
| a2 | The full-screen scroll path (no DECSTBM scroll region) is the path EC1 is meant to cover; the scroll-region path via `shift_rows_up` is a distinct clearing site and out of scope. | medium | yes |
| a3 | The new FR4 test lives in `crates/term_core/src/ring_buffer/tests.rs` beside `test_ring_push_blank_clears_ridx`, since its subject is `ring_push_blank`. Moving it to `print_handler/tests.rs` would work equally well and changes nothing else. | low | yes |
| a4 | The exact new names (`test_relocate_widened_base_via_wrap_scrolls_without_panic`, `test_ring_push_blank_clears_recycled_row_overflow_entries`) are proposals conforming to the crate convention; any rename must be mirrored in both `tests.yaml` records. | low | yes |
| a5 | The FR4 fixture uses `TerminalCore::new(5, 2, 0)` (scrollback disabled → `ring_buffer.rs`'s third eviction branch) and a plain line feed as the scroll trigger. Any of the three scrollback branches would do; the disabled branch is chosen because it is the simplest and matches EC1's own fixture shape. | low | yes |

## Success Criteria

- [ ] AC-1 — EC1 test renamed and its comment claims only no-panic /
      placement.
- [ ] AC-2 — the vacuous assertions and the `abs1` binding are gone; the
      placement assertions remain.
- [ ] AC-3 — `test_ring_push_blank_clears_recycled_row_overflow_entries`
      exists and passes on unmodified code.
- [ ] AC-4 — red confirmed by removing both clearing sites; failure message
      recorded with `red_confirmed: true`, together with the single-site
      redundancy note.
- [ ] AC-5 — TS1 and `..._no_panic_when_column_one_does_not_exist` unchanged
      and green; no non-`#[cfg(test)]` source line changed.
- [ ] AC-6 — SPEC's unreachability statement present with mechanism and
      file:line evidence, plus the `shift_rows_up` out-of-scope note.
- [ ] AC-7 — this feature's `tests.yaml` record exists and maps every AC; the
      `relocate-wrap-overflow-cleanup` AC-6 entry is corrected and no other
      entry is altered.
- [ ] AC-8 —
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      green and
      `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` clean.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional requirement (FR1–FR8) is `resolved`; there is no
`tbd` requirement.

The only value deferred by design is the test-docs task number `NNNN` in
`test-docs/relocate-wrap-ec1-scroll-test/taskNNNN.tests.yaml`, which FR7
assigns to the plan phase.

## Design Step

Skipped. Test-only change inside the pure-Rust `term_core` crate: no UI
surface, no visual artifact, no user-visible behavior and no public API
change. The diff touches test code and documentation records only, so there
is nothing for the design step to decide.

## References

- Requirements document: `feature-docs/relocate-wrap-ec1-scroll-test/REQUIREMENTS.md`
- Origin finding `532f5e5cbe0763e7` (medium, confidence 65):
  `feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`
- Existing test record: `test-docs/relocate-wrap-overflow-cleanup/task0001.tests.yaml`
  (AC-6 entry at lines 59-74)
- EC1 test: `crates/term_core/src/print_handler/tests.rs:1551-1580`
- TS1 test and fixture shape: `crates/term_core/src/print_handler/tests.rs:1454`,
  `1457-1472`
- `test_ring_push_blank_clears_ridx`: `crates/term_core/src/ring_buffer/tests.rs:417`
- `viewport_abs`: `crates/term_core/src/ring_buffer.rs:75-82`
- `ring_push_blank` eviction clears: `crates/term_core/src/ring_buffer.rs:147-148`,
  `178-179`, `196-199`; new-bottom clear: `221-224`
- Relocation writes and deletion branches:
  `crates/term_core/src/print_handler.rs:464-521`, `493`, `518`
- `shift_rows_up` clearing sites (out of scope):
  `crates/term_core/src/terminal_rows.rs:125-126`, `164-165`, `189-190`,
  `226-227`, `256-257`
