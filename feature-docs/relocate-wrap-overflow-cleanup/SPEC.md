# Feature: relocate-wrap-overflow-cleanup

> Requirements document (Japanese, normative source of the requirements
> rendered here): `feature-docs/relocate-wrap-overflow-cleanup/REQUIREMENTS.md`

## Overview

`relocate_widened_base_via_wrap` writes relocated content into a new row but
never deletes the overflow-table entries that belonged to the cells it
overwrites. That breaks the invariant the two ASCII writers' `was_overflow`
gate (introduced by task0004) implicitly depends on: an overflow-table entry
exists at `(col, abs)` only while that cell's `char_len == 0xFF`. This feature
adds the two missing deletions, keeps the reverse index consistent, and states
the invariant and the obligation it creates at both ASCII writers' gates. The
defect is memory-retention only — every reader is gated on
`cell.is_overflow()` — so no observable behavior changes.

## Objectives

- Close the overflow-entry deletion gap in `relocate_widened_base_via_wrap` so
  that the invariant the two ASCII writers' `was_overflow` gate implicitly
  depends on actually holds in code.
- Make that invariant explicit at the point of dependence, so a future author
  adding a path that clears the marker knows the ASCII overwrite no longer
  cleans up after them.
- Keep the per-byte cost recovery task0004 bought for the ASCII fast path (do
  not regress to the ring-wide `!self.overflow.is_empty()` self-healing gate).
- Restore contract consistency inside term_core's print subsystem:
  `write_grapheme_to_grid`, `try_retroactive_merge`, `widen_after_merge`,
  `blank_wide_pair_half` and `set_cell_ascii` all delete by table lookup; only
  the two ASCII writers became marker-dependent.

## User Stories

### US1: The relocation path cleans up after itself

As a term_core implementer, I want `relocate_widened_base_via_wrap` to delete
the overflow-table entry of every cell whose overflow marker it clears, so that
"an entry exists at `(col, abs)` only when that cell's `char_len == 0xFF`" holds
in code and the ASCII writers' marker-based gate is sound.

**Acceptance Criteria:**
- [ ] AC1: Approach (a) is implemented — `relocate_widened_base_via_wrap`
      deletes the overflow entry at both the relocated base write and the
      spacer write, in the same shape as `write_grapheme_to_grid`, so
      "entry exists ⟹ that cell has `char_len == 0xFF`" holds in code.
      (traces: FR1, FR2, FR3, FR5)
- [ ] AC3: A unit test covers the case where `line_feed` descends into an
      existing row without scrolling and that row's col 0 / col 1 were
      overflow-bound, proving no marker-less entry survives. (traces: FR6)

### US2: The invariant is readable where it is depended on

As a term_core implementer adding a new path that clears an overflow marker, I
want both ASCII writers' `if was_overflow` blocks to state the invariant they
depend on and the deletion obligation it creates, so that I see the obligation
at the point of dependence without external references.

**Acceptance Criteria:**
- [ ] AC2: Both ASCII writers' `if was_overflow` blocks state the invariant
      they depend on and the "a write that clears the marker owns deleting the
      entry" obligation. (traces: FR4, NFR5)
- [ ] AC5: If (a) proves infeasible during implementation, (b) is taken
      instead — both ASCII writers' gates return to the self-healing form and
      the record states how NFR1's cost is then covered. This branch is the
      fallback, not the plan. (traces: FR5)

### US3: Nothing else moves

As a term_core implementer, I want the fix to change no observable behavior and
to leave the existing suites green, so that the repair is provably confined to
unreachable internal state.

**Acceptance Criteria:**
- [ ] AC4: The existing term_core tests and the src-tauri tests still pass, and
      `cargo check --no-default-features` still succeeds. (traces: FR7, FR8)

## Technical Requirements

### Functional Requirements

- **FR1 — Relocated base write removes a stale overflow entry:** In
  `relocate_widened_base_via_wrap` (`crates/term_core/src/print_handler.rs`, the
  col-0 write of the new row at lines 468-480), when the relocated content fits
  inline so `cell.set_char` clears the overflow marker, the existing
  overflow-table entry at `(0, new_abs)` is removed, in the same
  `if cell.is_overflow() { insert } else { remove }` shape
  `write_grapheme_to_grid` uses at print_handler.rs:163-170. When the content
  does not fit inline, the existing insert branch is kept unchanged.
- **FR2 — Relocated spacer write removes a stale overflow entry:** In the same
  function's col-1 spacer write (print_handler.rs:484-493), the overflow-table
  entry at `(1, new_abs)` is removed, mirroring the placeholder branch of
  `write_grapheme_to_grid` (print_handler.rs:198-203) and of
  `widen_after_merge` (print_handler.rs:405-408). The spacer write always
  clears the marker (`char_len = 0`), so the removal is unconditional for that
  cell.
- **FR3 — Reverse index stays consistent with the table:** Every removal added
  by FR1/FR2 updates `overflow_ridx` through
  `overflow_ridx_remove(&mut self.overflow_ridx, abs, col)` whenever
  `overflow.remove` returned `Some`, exactly as the existing removal sites do;
  no `overflow_ridx` row entry survives the removal of its last column.
- **FR4 — The invariant is documented at both ASCII writers' gate:** The
  `if was_overflow` block in `handle_print_ascii`
  (`crates/term_core/src/print_handler.rs:250-256`) and the equivalent block in
  the dispatch ASCII fast path
  (`crates/term_core/src/terminal_dispatch.rs:155-165`) each state the
  invariant they depend on — an overflow-table entry exists at `(col, abs)`
  only while that cell's `char_len == 0xFF` — and state the obligation it
  creates: any write that clears a cell's overflow marker is responsible for
  removing that cell's own table entry, because the ASCII overwrite no longer
  sweeps entries it did not observe a marker for.
- **FR5 — The ASCII gate stays marker-based:** Neither ASCII writer's gate is
  reverted to the ring-wide self-healing form `!self.overflow.is_empty()`. The
  approach selected by the objective is (a) — establish the invariant in code —
  and (b) (self-healing gate plus a recorded NFR1 cost argument) is retained
  only as the documented fallback if (a) is shown infeasible during
  implementation.
- **FR6 — Regression test for the non-scrolling `line_feed` case:** A unit test
  drives `relocate_widened_base_via_wrap` through its real trigger (a
  last-column base cell widened by VS16 with auto-wrap on) so that `line_feed`
  descends into an existing row without scrolling, where that row's col 0 and
  col 1 already hold overflow-bound content, and asserts that after the
  relocation no overflow-table entry (and no `overflow_ridx` entry) remains for
  either column while both cells report `is_overflow() == false`.
- **FR7 — No observable behavior change:** Grid contents, cell widths, cursor
  position, wrap flags, scrollback, reflow output and snapshots are unchanged
  by this fix for every input: the removed entries were already unreachable
  because every reader (`get_cell_char`, `cell_content_at`, reflow, ring
  eviction, snapshot) is gated on `cell.is_overflow()`. The existing relocation
  tests (for example `test_retroactive_widen_at_last_column_wraps_with_autowrap`
  and
  `test_relocate_widened_base_via_wrap_spacer_creation_blanks_existing_pair_spacer`)
  keep passing unmodified.
- **FR8 — Existing suites stay green:** The term_core `--lib` suite and the
  src-tauri `--lib` suite both pass after the change, and the CLI-only
  `cargo check --no-default-features` still succeeds.

### Non-Functional Requirements

- **NFR1 - Performance budget preserved:** No cost is added to the ASCII common
  case. FR1/FR2's removals live in `relocate_widened_base_via_wrap`, reached
  only from a VS16 widening of a last-column base cell with auto-wrap on —
  never on a per-byte ASCII path. The at-most-two hash operations they add
  occur once per relocation. The `was_overflow` marker read in both ASCII
  writers stays where it is (before the write clears `char_len`), because a
  read placed after the write always observes `false`.
- **NFR2 - Scope:** The change is confined to `crates/term_core`:
  `src/print_handler.rs` (the two removals plus the `handle_print_ascii` gate
  comment), `src/terminal_dispatch.rs` (fast-path gate comment only), and
  `src/print_handler/tests.rs`. term_core's public API is unchanged and no
  dependency or dev-dependency is added.
- **NFR3 - Robustness:** The removals must not panic or index out of range when
  the target row is reached by a scrolling `line_feed`, when `cols` is small
  enough that col 1 does not exist (`cell_index(1, new_row)` returns `None`), or
  when no entry is present at the key (the `remove` returns `None` and the
  reverse-index update is skipped).
- **NFR4 - Test conventions:** New tests follow test/README.md: inline
  `#[cfg(test)] mod tests {}` next to the code under test (here the existing
  `crates/term_core/src/print_handler/tests.rs`), an explicitly constructed
  `TerminalCore` per test, input driven through `handle_print` /
  `process_pty_data`, and the file's local
  `test_<subject>_<scenario>_<expected>` naming. No new test framework or
  dev-dependency.
- **NFR5 - Documentation locality:** The invariant text required by FR4 sits at
  the two points that depend on it (both `if was_overflow` blocks), not only in
  a feature document, so a reader of either writer sees the obligation without
  external references.

## Implementation Approach

### Architecture

**Selected approach:** (a) — establish the invariant in code (FR5, A1).
Approach (b) — return both ASCII gates to the ring-wide self-healing form
`!self.overflow.is_empty()` and record how NFR1's cost is then covered — is
retained as the documented fallback only, taken if (a) proves infeasible during
implementation (AC5).

**Affected components (all inside `crates/term_core`):**

```
term_core print subsystem
├── print_handler.rs
│   ├── relocate_widened_base_via_wrap   ← FR1 (col-0 write, 468-480)
│   │                                      FR2 (col-1 spacer write, 484-493)
│   │                                      FR3 (overflow_ridx update)
│   ├── write_grapheme_to_grid            ← shape reference (163-170, 198-203)
│   ├── widen_after_merge                 ← shape reference (405-408)
│   └── handle_print_ascii                ← FR4 (gate comment, 250-256)
├── terminal_dispatch.rs
│   └── ASCII fast path                   ← FR4 (gate comment, 155-165)
└── print_handler/tests.rs                ← FR6 (new regression test)
```

**State touched:**

| Structure | Location | Role |
|---|---|---|
| `overflow` | `terminal_core.rs:123-124` (`pub(crate)`) | maps `(col, abs)` to content that does not fit in a cell |
| `overflow_ridx` | `terminal_core.rs:123-124` (`pub(crate)`) | row → column reverse index over `overflow` |
| cell `char_len` | cell representation | `0xFF` marks the cell overflow-bound (`is_overflow()`) |

### Data Flow

```
VS16 on last-column base cell (auto-wrap on)
  → relocate_widened_base_via_wrap
      → line_feed (descends into an existing row, may or may not scroll)
      → write relocated base at col 0 of new row
          ├── cell.is_overflow()  → insert into overflow            (unchanged)
          └── !cell.is_overflow() → overflow.remove((0, new_abs))   (FR1)
                                    → Some ⇒ overflow_ridx_remove(new_abs, 0) (FR3)
      → write spacer at col 1 of new row (char_len = 0, marker always cleared)
          └── overflow.remove((1, new_abs))                         (FR2)
              → Some ⇒ overflow_ridx_remove(new_abs, 1)             (FR3)
```

The ASCII writers' path is unchanged; only its gate gains the invariant
comment:

```
handle_print_ascii / dispatch ASCII fast path
  → read was_overflow (before the write clears char_len — NFR1)
  → write ASCII byte
  → if was_overflow { remove this cell's own entry + reverse index }  (unchanged)
```

### API Design

No API change. term_core's public API is unchanged (NFR2); the feature adds no
endpoint, no new public function and no new type.

### Database Schema

Not applicable. The feature touches only in-memory term_core state
(`overflow`, `overflow_ridx`, cell `char_len`).

### Dependencies

**Internal Dependencies:**
- `crates/term_core/src/print_handler.rs`: `write_grapheme_to_grid`
  (163-170, 198-203) and `widen_after_merge` (405-408) are the shape references
  the new removals mirror; `cell_content_at` (285-295) is a reader gated on
  `is_overflow()`.
- `crates/term_core/src/cell.rs`: `overflow_ridx_remove` (164-168) — already
  drops the row key when its last column is removed (EC4).
- `crates/term_core/src/terminal_cells.rs`: `get_cell_char` (111-126) — reader
  gated on `is_overflow()`.
- `crates/term_core/src/snapshot.rs`: `pub overflow` (76-77) — one of the two
  assertion surfaces available to the FR6 test (A5).
- PR #40 (`ascii-fast-path-wide-pair-cleanup`, task0004) must be merged into
  this feature's base so that `relocate_widened_base_via_wrap`,
  `handle_print_ascii`'s `was_overflow` gate (print_handler.rs:234/250) and the
  dispatch fast path's `was_overflow` gate (terminal_dispatch.rs:124/155) exist
  in the shapes cited (A2).

**External Dependencies:**
- None. No dependency or dev-dependency is added (NFR2, NFR4).

### File Structure

```
crates/term_core/src/
├── print_handler.rs            # FR1, FR2, FR3 removals; FR4 gate comment
│                               #   in handle_print_ascii
├── terminal_dispatch.rs        # FR4 gate comment in the ASCII fast path only
└── print_handler/
    └── tests.rs                # FR6 regression test (inline #[cfg(test)] mod)
```

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR2, FR3, FR6): Relocation onto an overflow-bound existing
      row leaves no stale entry — given a `TerminalCore` whose row 1 holds, at
      col 0 and col 1, content longer than 16 bytes (a base plus a long
      combining-mark run, as
      `test_retroactive_merge_long_combining_run_overflows_correctly` builds),
      and row 0 filled to the last column, when a VS16 widens the last-column
      base and relocates it via wrap onto row 1 (no scroll), then neither
      `(0, abs(row1))` nor `(1, abs(row1))` remains in the overflow table,
      neither cell reports `is_overflow()`, and `overflow_ridx` holds no entry
      for those columns.
- [ ] **TS2** (FR1): Relocated content that is itself overflow keeps its entry —
      given a relocated base whose content exceeds 16 bytes, when the
      relocation lands, then the entry at `(0, new_abs)` is present and equals
      the relocated content, and the cell reports `is_overflow()`.
- [ ] **TS3** (FR7): Visible relocation behavior unchanged — the assertions of
      `test_retroactive_widen_at_last_column_wraps_with_autowrap` and
      `test_relocate_widened_base_via_wrap_spacer_creation_blanks_existing_pair_spacer`
      (cell chars, widths, cursor col/row, wrap flags, `get_line_wrapped`) hold
      unmodified after the change.
- [ ] **TS4** (FR4, FR5): ASCII writers still clean up their own cell —
      overwriting an overflow-bound cell with ASCII through `handle_print_ascii`
      and through the dispatch fast path both leave the table and reverse index
      free of that cell's entry, unchanged from the pre-change behavior.

### Integration Tests

None.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

None.

### Edge Cases

- [ ] **EC1** (NFR3): Relocation where `line_feed` does scroll (the widened base
      sits on the last row): the row scrolled in must also end free of
      marker-less entries; the eviction path must not be disturbed.
- [ ] **EC2** (NFR3): `cell_index(1, new_row)` returns `None` (no col 1
      available): the spacer-side removal is skipped without panic.
- [ ] **EC3** (FR3, NFR3): No entry exists at the removal key:
      `overflow.remove` returns `None` and `overflow_ridx_remove` is not called,
      matching every existing removal site.
- [ ] **EC4** (FR3): Removing the last column of a row from `overflow_ridx`
      drops the row key entirely (the behavior `overflow_ridx_remove` already
      implements at cell.rs:164-168).

### Suite-Level Tests

- [ ] **TS5** (FR8): term_core `--lib`, src-tauri `--lib` and the CLI-only
      `cargo check --no-default-features` all pass.

### Performance Tests

None. NFR1 is satisfied structurally: FR1/FR2's removals are reachable only
from a VS16 widening of a last-column base cell with auto-wrap on, never on a
per-byte ASCII path, and add at most two hash operations per relocation.

## Security Considerations

Not applicable. The change is an internal invariant repair inside
`crates/term_core` with no user-facing surface, no new input handling and no
API change.

## Error Handling

No new error paths or error codes. The removals must degrade silently and
safely in the three cases NFR3 names:

| Condition | Behavior |
|---|---|
| Target row reached by a scrolling `line_feed` | Removal proceeds; no panic, no index-out-of-range; eviction path undisturbed (EC1) |
| `cell_index(1, new_row)` returns `None` (no col 1) | Spacer-side removal is skipped without panic (EC2) |
| No entry present at the key | `overflow.remove` returns `None`; `overflow_ridx_remove` is not called (EC3) |

## Performance Optimization

### Performance Goals

- No cost added to the ASCII common case (per-byte path): zero additional
  operations (NFR1).
- Relocation path: at most two hash operations per relocation (NFR1).

### Optimization Strategies

- Keep the ASCII gate marker-based; do not revert to the ring-wide
  `!self.overflow.is_empty()` self-healing form (FR5, NFR1).
- Keep the `was_overflow` marker read before the write clears `char_len` — a
  read placed after the write always observes `false` (NFR1).

### Caching Strategy

Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1-FR8) are implemented and tested
- [ ] All test scenarios pass (TS1-TS5, EC1-EC4)
- [ ] Performance meets specified goals (NFR1: no per-byte ASCII cost added)
- [ ] Security requirements are satisfied (not applicable to this change)
- [ ] Documentation is complete (FR4/NFR5: the invariant and the deletion
      obligation are stated at both `if was_overflow` blocks)
- [ ] Code review is completed
- [ ] AC1: `relocate_widened_base_via_wrap` deletes the overflow entry at both
      the relocated base write and the spacer write, in the same shape as
      `write_grapheme_to_grid`
- [ ] AC2: Both ASCII writers' `if was_overflow` blocks state the invariant and
      the obligation
- [ ] AC3: A unit test covers the non-scrolling `line_feed` descent onto an
      overflow-bound row
- [ ] AC4: Existing term_core and src-tauri tests pass and
      `cargo check --no-default-features` succeeds
- [ ] AC5: If (a) proves infeasible, (b) is taken instead and the record states
      how NFR1's cost is then covered (fallback branch, not the plan)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every functional requirement is `status: resolved`.

## Assumptions

These are the assumptions requirements-analyst resolved; they are recorded here
and in REQUIREMENTS.md section 14.1.

- **A1** (high): Approach (a) is the selected approach; (b) is a documented
  fallback only. Evidence: the task's mandatory objective says to make the
  invariant hold in code and to document it; the background calls task0004's
  marker gate the correct change to recover NFR1's performance budget; the
  feature slug is "…-cleanup". Review suggestion 9907b4671d9f9e50 lists (a)
  first and (b) as "if you choose not to establish it".
- **A2** (high): PR #40 (ascii-fast-path-wide-pair-cleanup) is merged into this
  feature's base, so `relocate_widened_base_via_wrap`, `handle_print_ascii`'s
  `was_overflow` gate and the dispatch fast path's `was_overflow` gate exist in
  the shapes cited. Evidence: verified in the integration worktree —
  print_handler.rs:234/250 and terminal_dispatch.rs:124/155 carry the task0004
  gates; the task's constraints section states the merge as a precondition.
- **A3** (medium): Amending
  `feature-docs/ascii-fast-path-wide-pair-cleanup/SPEC.md` (its NFR2 scope
  statement, and its File Structure / Dependencies mis-location of
  `blank_orphaned_neighbor_before_overwrite`) is not part of this feature. This
  feature resolves the root cause finding 3a78522db0da4ea7 rests on — the
  broken equivalence premise — but the task's acceptance criteria contain no
  document-reconciliation item. Evidence: task_description acceptance-criteria
  list; round2.yaml:106-110 raises the doc reconciliation as part of the
  suggestion, not of this task's DoD.
- **A4** (high): The defect is memory-retention only, bounded by
  cols × ring rows × 256B, with no rendering, scrollback, reflow or snapshot
  impact, because every reader is gated on `cell.is_overflow()`. Evidence:
  verified — `get_cell_char` (terminal_cells.rs:111-126) and `cell_content_at`
  (print_handler.rs:285-295) both branch on `is_overflow()`; the task and
  round2.yaml state the same.
- **A5** (medium): The FR6 test asserts on the `overflow` table /
  `overflow_ridx` directly (in-crate `pub(crate)` access) or via
  `TerminalSnapshot.overflow`, accepting the deviation from test/README.md's
  "observable contract" guidance because the invariant has no observable
  projection by construction. Evidence: terminal_core.rs:123-124
  (`pub(crate) overflow` / `overflow_ridx`), snapshot.rs:76-77
  (`pub overflow`), test/README.md "Test Structure".

## Design Step

**Status:** skipped

**Reason:** The change has no user-visible surface: no UI, no interaction flow,
no new API. It is an internal invariant repair inside `crates/term_core` plus
code comments and one unit test; grid rendering is provably unchanged (A4). The
immediately prior feature in the same subsystem
(`ascii-fast-path-wide-pair-cleanup`) skipped the design step for the same
reason.

## Implementation Phases (if applicable)

Not applicable — the change is a single, indivisible repair confined to three
files in `crates/term_core` (NFR2).

## References

- Requirements document: `feature-docs/relocate-wrap-overflow-cleanup/REQUIREMENTS.md`
- `crates/term_core/src/print_handler.rs`: `relocate_widened_base_via_wrap`
  (468-480, 484-493), `write_grapheme_to_grid` (163-170, 198-203),
  `widen_after_merge` (405-408), `handle_print_ascii` (234, 250-256),
  `cell_content_at` (285-295)
- `crates/term_core/src/terminal_dispatch.rs`: ASCII fast path (124, 155-165)
- `crates/term_core/src/cell.rs`: `overflow_ridx_remove` (164-168)
- `crates/term_core/src/terminal_core.rs`: `pub(crate) overflow` /
  `overflow_ridx` (123-124)
- `crates/term_core/src/snapshot.rs`: `pub overflow` (76-77)
- `crates/term_core/src/terminal_cells.rs`: `get_cell_char` (111-126)
- `crates/term_core/src/print_handler/tests.rs`: existing relocation tests
- `test/README.md`: test structure conventions
- `feature-docs/ascii-fast-path-wide-pair-cleanup/`: the immediately prior
  feature in the same subsystem (task0004)
- Review suggestion 9907b4671d9f9e50; finding 3a78522db0da4ea7;
  round2.yaml:106-110
