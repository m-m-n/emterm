# Feature: relocate-wrap-cursor-clamp

> Requirements document (Japanese, authoritative for requirement content):
> `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`

## Overview

`relocate_widened_base_via_wrap` in `crates/term_core/src/print_handler.rs` ends with an
unconditional `self.cursor.col = 2;` (line 524). When `cols <= 2` that column is outside the grid,
so the next printed character is silently dropped. This feature replaces that unconditional
assignment with a `cols`-boundary clamp that is textually identical in shape to the one in
`widen_after_merge` (print_handler.rs:423-431), and pins the resulting cursor contract with unit
tests for cols=1 and cols=2. Behaviour for `cols >= 3` is unchanged.

## Objectives

- Remove the existing defect where the unconditional `self.cursor.col = 2;` at the end of
  `relocate_widened_base_via_wrap` points outside the grid for `cols <= 2` and causes the next
  printed character to be silently dropped.
- Align the cursor contract between the non-final-column path of the same VS16 widening logic
  (`widen_after_merge`, print_handler.rs:423-431) and the final-column path
  (`relocate_widened_base_via_wrap`, print_handler.rs:524-525), so that one path is no longer off
  contract.
- Correct the state where the existing cols=1 test (print_handler/tests.rs:1589-1601) only asserts
  the absence of a panic, never checks the cursor position, and is nonetheless recorded as
  "covered".
- Close follow-up finding `3e769a761d85d839` (relocate-wrap-overflow-cleanup review round1,
  reviews/round1.yaml:129-152, severity medium, confidence 80), which was filed unresolved.

## User Stories

Not applicable. The resolved requirements define no user-facing story: the change is confined to
internal cursor bookkeeping inside `crates/term_core` and is observable only through cursor
position, `wrap_pending`, and cell contents (see Success Criteria).

## Technical Requirements

### Functional Requirements

- **FR1 — Replace the cursor update at the end of `relocate_widened_base_via_wrap` with a clamp
  textually identical in shape to `widen_after_merge`:** Replace the unconditional
  `self.cursor.col = 2;` at the end of `relocate_widened_base_via_wrap` in
  `crates/term_core/src/print_handler.rs` (currently line 524) with a branch textually identical in
  shape to `widen_after_merge` (lines 423-431 of the same file). That is, compare `new_col`
  (= 0 + 2), based on the relocated base cell's column 0, against `self.cols`, in the form
  `if new_col >= self.cols as u32 { if self.get_mode(MODE_AUTO_WRAP) { self.cursor.col = self.cols - 1; self.wrap_pending = true; } } else { ... }`.
  The inner `if self.get_mode(MODE_AUTO_WRAP)` guard must be included, not elided
  (answer: mirror-verbatim).
- **FR2 — Clamp the cursor to the last column and raise `wrap_pending` when `cols <= 2`:** When
  `new_col >= self.cols` (i.e. `cols <= 2`), set `self.cursor.col = self.cols - 1;` and
  `self.wrap_pending = true;`. The cursor after relocation then always points inside the grid, and
  the next print is carried to the next row through wrapping.
- **FR3 — Preserve the existing `cols >= 3` behaviour (col=2 / wrap_pending=false):** When
  `new_col < self.cols` (`cols >= 3`), keep `self.cursor.col = 2;` together with the current
  `self.wrap_pending = false;` (currently line 525). The internal helpers `carriage_return`
  (terminal_core.rs:869-871) and `line_feed` (terminal_core.rs:874-887) both leave `wrap_pending`
  untouched, so a `wrap_pending` that was set on entry to relocation is cleared only by this one
  line. `widen_after_merge`'s else branch has no such line because that path already has
  `wrap_pending` false; the shape alignment therefore applies to the cursor-column branch
  structure, and this line stays.
- **FR4 — Leave every non-cursor post-step of relocation unchanged:** Nothing in
  `relocate_widened_base_via_wrap` other than the cursor update changes — including
  `self.last_write = Some((0, new_row));` (currently line 526), cell content transfer, overflow-table
  consistency, the `ring_wrapped` assignment, and dirty marking.
- **FR5 — Add cursor-contract assertions to the existing cols=1 test:** In
  `test_relocate_widened_base_via_wrap_no_panic_when_column_one_does_not_exist`
  (crates/term_core/src/print_handler/tests.rs:1589-1601), add assertions that the cursor column is 0
  (= `cols - 1`) and that `wrap_pending` is true, alongside the existing cell-character and
  cursor-row assertions. Update the test name and comments so they express pinning the cursor
  contract rather than only "does not panic".
- **FR6 — Add a new cols=2 regression test:** Add a new test for the cols=2 case, where the spacer
  column (col 1) exists but `new_col == cols`. Print 'A' and '5' to place the base cell in the last
  column (col 1), trigger relocation with VS16, and verify the relocated cell, the spacer, the
  cursor column, and `wrap_pending`.
- **FR7 — Pin in tests that the next character is not dropped after the clamp:** For both cols=1 and
  cols=2, assert that printing one more character immediately after relocation makes that character
  appear on the grid (rather than being silently dropped because `cell_index` returns None, as
  before the fix).

### Non-Functional Requirements

- **NFR1 — Confine the change to the term_core cursor update and its tests:** The only production
  code change is the cursor update at the end of `relocate_widened_base_via_wrap` in
  `crates/term_core/src/print_handler.rs`. Test changes and additions are confined to
  `crates/term_core/src/print_handler/tests.rs`. No other module, crate, or public API is touched.
- **NFR2 — A width=2 base cell without a spacer at cols=1 is out of scope:** At cols=1 the spacer
  column cannot be reserved, so a width=2 value is written to the base cell. This follows from the
  `col < self.cols` boundary check in `viewport_cell_offset` (ring_buffer.rs:93-106), which skips the
  write to col 1 entirely; it is existing degraded-mode behaviour, and the auto-wrap-off widening
  branch (print_handler.rs:385-387) and the existing wide-character write path take the same form.
  This feature does not change it and records it as out of scope (answer: cursor-only).
- **NFR3 — Introduce no externally observable behaviour change for `cols >= 3`:** For `cols >= 3`,
  cursor position, `wrap_pending`, cell contents, and the overflow table all produce results
  identical to the current ones. The existing test
  `test_retroactive_widen_at_last_column_wraps_with_autowrap` (tests.rs:662-685) stays green
  unmodified.
- **NFR4 — Follow the existing test and formatting conventions:** Tests go in the inline test module
  in `crates/term_core/src/print_handler/tests.rs`, per term_core's existing convention (no new
  separate `tests/` directory). Formatting follows rustfmt (style_edition 2024) and targets only the
  changed and added lines, without reformatting unrelated existing lines.
- **NFR5 — Out of scope for E2E:** This project has no E2E infrastructure (`resolved_input_paths.e2e`
  is empty, and every component's `e2e_test_command` in the referenced workflow.yaml is an empty
  string), and this feature introduces none. Verification is complete with term_core unit tests.

## Implementation Approach

### Architecture

The change lives entirely in the VS16 widening path of `crates/term_core`:

```
print_handler.rs
├── widen_after_merge (423-431)              # non-final-column path — reference shape
│     └── if self.get_mode(MODE_AUTO_WRAP)   # (379-380) sole call site of the relocation below
│           └── relocate_widened_base_via_wrap
│                 ├── cell transfer / overflow table / ring_wrapped / dirty marking   [unchanged, FR4]
│                 ├── cursor update (524-525)                                         [CHANGED, FR1-FR3]
│                 └── self.last_write = Some((0, new_row)) (526)                      [unchanged, FR4]
└── auto-wrap-off widening branch (385-387)                                           [unchanged, NFR2]
```

Supporting code that constrains the change (read-only context):

- `terminal_core.rs:869-871` (`carriage_return`) and `terminal_core.rs:874-887` (`line_feed`) — neither
  touches `wrap_pending`, which is why FR3 keeps `self.wrap_pending = false;`.
- `ring_buffer.rs:93-106` (`viewport_cell_offset`) — its `col < self.cols` boundary check is the origin
  of the out-of-scope cols=1 behaviour in NFR2.

### Data Flow

```
print('5') → print(VS16)
  → widen_after_merge
      → base cell is in the last column, auto-wrap on
          → relocate_widened_base_via_wrap
              → relocate base cell to (col 0, new_row)
              → new_col = 0 + 2
              → new_col >= cols ?
                   yes (cols <= 2) → cursor.col = cols - 1 ; wrap_pending = true    [FR2]
                   no  (cols >= 3) → cursor.col = 2        ; wrap_pending = false   [FR3]
  → next print
      → cols <= 2: carried to the next row via wrap; the character appears on the grid   [FR7]
      → cols >= 3: unchanged from today's placement                                      [NFR3]
```

### API Design

Not applicable. No public API surface changes (NFR1).

### Database Schema

Not applicable. The feature touches no persisted data.

### Dependencies

**Internal Dependencies:**

- `crates/term_core` (`print_handler.rs`): the module that owns the changed cursor update.
- `crates/term_core` (`terminal_core.rs`, `ring_buffer.rs`): read-only context that constrains the
  change (FR3, NFR2).

**External Dependencies:**

- rustfmt (style_edition 2024) for formatting (NFR4).

### File Structure

```
crates/term_core/src/
├── print_handler.rs              # production change: cursor update at the end of
│                                 # relocate_widened_base_via_wrap (FR1-FR3); nothing else (FR4)
└── print_handler/
    └── tests.rs                  # existing cols=1 test extended (FR5), new cols=2 test (FR6),
                                  # next-character assertions (FR7); inline test module (NFR4)
```

## Test Scenarios

### Unit Tests

- [ ] **TS1 — cols=1 existing test extension** (FR2, FR5, FR7): add assertions for cursor column 0 and
      `wrap_pending == true` to `test_relocate_widened_base_via_wrap_no_panic_when_column_one_does_not_exist`
      (tests.rs:1589-1601). Expected before the fix: red (`cursor.col` stays 2).
- [ ] **TS2 — cols=2 new regression test** (FR2, FR6): print 'A', '5', VS16 with
      `TerminalCore::new(2, 3, 0)` and verify each value in AC2. Expected before the fix: red
      (`cursor.col` is 2, out of range, and `wrap_pending` is false).
- [ ] **TS3 — next character is not dropped at cols=2** (FR7): continuing from TS2, print 'X' and
      verify 'X' appears on the grid (col 0 of the wrapped-to row). Expected before the fix: red
      (`cell_index` returns None and 'X' disappears).
- [ ] **TS4 — next character is not dropped at cols=1** (FR7): continuing from TS1, print 'X' and
      verify 'X' appears on the grid (col 0 of the wrapped-to row). Expected before the fix: red
      ('X' disappears).
- [ ] **TS5 — cols=5 existing behaviour is unchanged** (FR3, NFR3): run
      `test_retroactive_widen_at_last_column_wraps_with_autowrap` (tests.rs:662-685) unmodified and
      confirm it stays green. Expected before the fix: green (green after the fix as well).

### Integration Tests

Not applicable. Verification is complete with the term_core unit tests above (NFR5).

### E2E Tests

**Existing E2E tests**: None (`resolved_input_paths.e2e` is empty, and every component's
`e2e_test_command` in the referenced workflow.yaml is an empty string).
**Run command**: Not detected.

No E2E tests are introduced (NFR5).

### Edge Cases

- [ ] cols=1 — column 1 does not exist, so `new_col >= cols`; the cursor is clamped to column 0 and
      `wrap_pending` is raised (FR2, TS1).
- [ ] cols=2 — the spacer column (col 1) exists but `new_col == cols`; the cursor is clamped to
      column 1 and `wrap_pending` is raised (FR2, FR6, TS2).
- [ ] cols=1 with a width=2 base cell and no spacer — existing degraded-mode behaviour, left
      unchanged and out of scope (NFR2).

### Component Commands

- [ ] **TS6 — component commands** (NFR1, NFR4): run term_core's `build_command` / `test_command` /
      `format_command` and confirm they succeed.

## Security Considerations

Not applicable. The change introduces no authentication, authorization, external input handling, or
data-protection surface (NFR1).

## Error Handling

No error codes or error responses are introduced. The behavioural correction is that a printed
character that was previously dropped silently for `cols <= 2` is now carried to the next row via
wrapping (FR2, FR7).

## Performance Optimization

No performance goals or optimization strategies are defined for this feature.

## Success Criteria

- [ ] **AC1:** With cols=1 (`TerminalCore::new(1, 3, 0)`), printing VS16 after '5' leaves cursor row 1,
      cursor column 0, `wrap_pending` true, and cell (0,1) containing `"5\u{FE0F}"`.
- [ ] **AC2:** With cols=2 (`TerminalCore::new(2, 3, 0)`), printing VS16 after 'A' and '5' leaves cursor
      row 1, cursor column 1, `wrap_pending` true, cell (0,1) containing `"5\u{FE0F}"` with width 2, and
      cell (1,1) with width 0 (spacer).
- [ ] **AC3:** Printing one more character immediately after AC1 / AC2 makes that character observable
      as a cell on the grid (it is not silently dropped by a cursor column pointing outside the grid, as
      before the fix).
- [ ] **AC4:** In the existing cols=5 scenario, the current results are unchanged: cursor row 1, cursor
      column 2, `wrap_pending` false, and the following 'X' at cell (2,1). The existing test
      tests.rs:662-685 stays green unmodified.
- [ ] **AC5:** The implemented clamp is textually identical in shape to `widen_after_merge`
      (print_handler.rs:423-431) and includes the inner `if self.get_mode(MODE_AUTO_WRAP)` guard.
- [ ] **AC6:** term_core's build / test / format commands (each command under
      `project.components.term_core`) succeed.

## Assumptions

- **A1:** As a consequence of the mirror-verbatim answer, the inner `if self.get_mode(MODE_AUTO_WRAP)`
  guard placed on the relocation side is always true, because the sole caller of
  `relocate_widened_base_via_wrap` is inside `widen_after_merge`'s `if self.get_mode(MODE_AUTO_WRAP)`
  branch (print_handler.rs:379-380). The false side of that guard is therefore unreachable and is not
  covered by tests. This trades the cost of one unreachable branch for textual comparability against
  the acceptance criteria and the review finding.
  (Source: answer `requirement.clamp-shape.autowrap-guard = mirror-verbatim`)
- **A2:** The width=2 base cell written at cols=1 without a spacer column is treated as existing
  degraded-mode behaviour and is not changed by this feature. Scope is limited to the cursor clamp
  and the tests.
  (Source: answer `requirement.scope.cols1-width2-base = cursor-only`)
- **A3:** The shape alignment applies to "the cursor-column branch structure at the `cols` boundary",
  and the in-range side keeps `self.wrap_pending = false;` as it is today. Because neither
  `carriage_return` (terminal_core.rs:869-871) nor `line_feed` (terminal_core.rs:874-887) touches
  `wrap_pending`, dropping that line would leave `wrap_pending` true for `cols >= 3` and break both
  `assert!(!core.get_wrap_pending())` and the placement of the following 'X' in the existing test
  tests.rs:662-685. `widen_after_merge`'s else branch lacks that line because that path already has
  `wrap_pending` false.
  (Source: code fact — terminal_core.rs:869-887, print_handler.rs:423-431, tests.rs:662-685)
- **A4:** Skipping the design step was decided by automatic adoption of the analyst's recommendation
  (batch decision table); it is not a separate user judgement about whether visual artifacts are
  needed.
  (Source: answer `design-step.recommendation = skip`)
- **A5:** The component definitions (the term_core / main / cli commands) are inherited as-is from the
  preceding feature relocate-wrap-overflow-cleanup, workflow.yaml:15-33. Because no E2E
  infrastructure exists, `e2e_test_command` stays empty.
  (Source: `feature-docs/relocate-wrap-overflow-cleanup/workflow.yaml:15-33`)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1-FR7, NFR1-NFR5) has `status: ok`; there is no `tbd` requirement.

## Design Step

Skipped. The change is confined to a single cursor update inside `crates/term_core` and its unit
tests, touching no UI surface, screen layout, visual artifact, or design token; there is no visual
design question for the design step to settle.

## References

- Requirements document: `feature-docs/relocate-wrap-cursor-clamp/REQUIREMENTS.md`
- `crates/term_core/src/print_handler.rs`: `relocate_widened_base_via_wrap` (lines 524-526),
  `widen_after_merge` (lines 423-431), the sole call site (lines 379-380), the auto-wrap-off widening
  branch (lines 385-387)
- `crates/term_core/src/print_handler/tests.rs`: existing cols=1 test (lines 1589-1601), existing
  cols=5 test (lines 662-685)
- `crates/term_core/src/terminal_core.rs`: `carriage_return` (lines 869-871), `line_feed`
  (lines 874-887)
- `crates/term_core/src/ring_buffer.rs`: `viewport_cell_offset` boundary check (lines 93-106)
- `feature-docs/relocate-wrap-overflow-cleanup/reviews/round1.yaml`: follow-up finding
  `3e769a761d85d839` (lines 129-152, severity medium, confidence 80)
- `feature-docs/relocate-wrap-overflow-cleanup/workflow.yaml`: component definitions (lines 15-33)
