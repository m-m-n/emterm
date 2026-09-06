# Feature: ring-push-blank-note-scope

## Overview

The NOTE attached to `test_ring_push_blank_clears_recycled_row_overflow_entries`
in `crates/term_core/src/ring_buffer/tests.rs` ends with a conclusion that reads
as a claim about the whole Step 3 block of `ring_push_blank`. Only Step 3's
`overflow` / `overflow_ridx` clear pair is actually redundant with the
eviction-time clear; the cell fill and the `ring_wrapped` reset have no
eviction-side counterpart. This feature narrows the conclusion and records the
non-redundant part, as a comment-only change.

Requirements source: `feature-docs/ring-push-blank-note-scope/REQUIREMENTS.md`.

## Objectives

- Keep the redundancy NOTE scoped to what the test actually pins, so a future
  maintainer cannot read "Step 3 is always a no-op" as licence to delete the
  whole Step 3 block of `ring_push_blank`.
- Preserve the NOTE's correct core claim (`new_bottom_abs == evicted_abs` on
  every push of at least one row) while removing the over-broad conclusion drawn
  from it.

## User Stories

### US1: A maintainer reads the NOTE before touching Step 3

As a `term_core` maintainer, I want the NOTE to attribute each Step 3 action
correctly, so that I do not delete required work while removing what is truly
redundant.

**Acceptance Criteria:**
- [ ] AC1: The NOTE's "always a no-op" statement is limited to the `overflow` /
      `overflow_ridx` clearing, and no longer reads as a claim about the whole
      Step 3 block.
- [ ] AC2: The NOTE states that the cell fill and the
      `ring_wrapped[new_bottom_abs] = false` reset have no eviction-side
      counterpart and are therefore necessary.

## Technical Requirements

### Functional Requirements

- **FR1 — Narrow the "always a no-op" conclusion to the overflow clear pair:**
  The NOTE's concluding sentence (currently
  `crates/term_core/src/ring_buffer/tests.rs:595-597`, "The new-bottom-row clear
  is therefore always a no-op within a single push") must state that only the
  Step 3 `overflow` / `overflow_ridx` clear pair is redundant with the
  eviction-time clear, not the Step 3 block as a whole. *(status: resolved)*
- **FR2 — Record that the cell fill and the `ring_wrapped` reset are not
  redundant:** Add 1-2 lines stating that Step 3's cell fill
  (`slice.fill(Cell::EMPTY)` / BCE, `ring_buffer.rs:211-217`) and
  `self.ring_wrapped[new_bottom_abs] = false` (`ring_buffer.rs:219`) have no
  counterpart on the eviction side and are therefore required, not redundant.
  *(status: resolved)*
- **FR3 — Preserve the NOTE's still-correct reasoning:** The existing text about
  the two clearing sites, the evaluation order (`evicted_abs` captured before
  Step 1, `ring_head` rotated in Step 2, `new_bottom_abs` derived in Step 3), and
  the resulting same-ring-slot identity stays as-is; only the conclusion is
  narrowed and the new sentences appended. *(status: resolved)*
- **FR4 — Comment-only change, confined to the test file:** The change touches
  only comment text inside `crates/term_core/src/ring_buffer/tests.rs`. No
  production code (`ring_buffer.rs`), no assertion, no fixture, and no test name
  is modified, and no test is added or removed. *(status: resolved)*

### Non-Functional Requirements

- **NFR1 — Formatting:** `cargo fmt --manifest-path crates/term_core/Cargo.toml
  --check` stays clean; the new comment lines follow the surrounding NOTE's wrap
  width (~72 columns inside the indented `//` block).
- **NFR2 — Wording register:** Wording is English and matches the surrounding
  comment register in `ring_buffer/tests.rs` (declarative, referring to Step 1 /
  Step 2 / Step 3 by the names used in `ring_buffer.rs`).
- **NFR3 — Behavioural neutrality:** Zero behavioural change: the crate's
  observable behaviour and the test's assertion set are identical before and
  after.

## Implementation Approach

### Architecture

No architectural change. The edit is confined to comment text in a single Rust
test module.

**Component Diagram:**
```
crates/term_core/src/ring_buffer.rs        (read-only reference for the NOTE)
  └── ring_push_blank
        Step 1  eviction  -> clears overflow / overflow_ridx for evicted_abs
        Step 2  ring_head rotation
        Step 3  new bottom -> overflow / overflow_ridx clear (redundant)
                           -> cell fill (required)
                           -> ring_wrapped[new_bottom_abs] = false (required)

crates/term_core/src/ring_buffer/tests.rs  (the only file edited)
  └── test_ring_push_blank_clears_recycled_row_overflow_entries
        └── NOTE (lines 584-597; conclusion at 595-597)
```

### Data Flow

Not applicable — no runtime data flow is introduced or altered.

### API Design

Not applicable — no API surface is touched.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `crates/term_core/src/ring_buffer.rs`: read as the reference the NOTE
  describes (Step 1 branches at 135-200, Step 3 at 206-224). Not modified.

**External Dependencies:**
- None.

### File Structure

```
crates/term_core/src/
├── ring_buffer.rs          # referenced only, unmodified
└── ring_buffer/
    └── tests.rs            # the sole edited file (comment text only)
```

### Design Step

Skipped. Comment-only edit inside an existing Rust test file. No UI surface, no
user-visible behaviour, no new module or public API, and no design-system file
is touched, so there is nothing for a design step to produce.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths above are derived at create-plan from every task's
`files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths above:

- `feature-docs/ring-push-blank-note-scope/**`
- `test-docs/ring-push-blank-note-scope/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`; this
section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is
a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no `test-docs/{feature}/`
directory at all; the declared `test-docs/{feature}/**` entry is still correct in
that case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS1 (covers AC3): `CARGO_TARGET_DIR=src-tauri/target cargo test
      --manifest-path crates/term_core/Cargo.toml --lib` — the whole term_core
      lib suite, including
      `test_ring_push_blank_clears_recycled_row_overflow_entries`.

### Integration Tests
- None.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Static / Formatting Checks
- [ ] TS2 (covers AC4): `cargo fmt --manifest-path crates/term_core/Cargo.toml
      --check`.

### Review Scenarios
- [ ] TS3 (covers AC1, AC2): Read the revised NOTE against `ring_push_blank`'s
      Step 1 branches (`ring_buffer.rs:135-200`) and Step 3
      (`ring_buffer.rs:206-224`) and confirm each Step 3 action is attributed
      correctly: redundant = overflow clear pair; required = cell fill and
      `ring_wrapped` reset.
- [ ] TS4 (covers FR4): The diff contains only comment lines in
      `crates/term_core/src/ring_buffer/tests.rs`.

### Edge Cases
- None beyond the reasoning the NOTE already records: for `rows >= 1`,
  `((ring_head + 1) % rows + rows - 1) % rows == ring_head`, so
  `new_bottom_abs == evicted_abs`.

### Performance Tests
- Not applicable.

## Security Considerations

Not applicable — comment-only change with no runtime effect.

## Error Handling

Not applicable — no runtime code path is introduced or altered.

## Performance Optimization

Not applicable.

## Assumptions

Carried over from the requirements analysis:

- Verified against the source: all three Step 1 branches (`scrollback_bypass`
  136-156, `scrollback_capacity > 0` 157-192, disabled 193-200) clear only
  `overflow` / `overflow_ridx` for `evicted_abs`. The task description's claim is
  accurate.
- Verified: `ring_wrapped` is written exactly once in the function, at
  `ring_buffer.rs:219`. Line 181 (`let wrapped = self.ring_wrapped[evicted_abs];`)
  is a read feeding `scrollback_wrapped`, not a reset — so the eviction side has
  no `ring_wrapped` counterpart.
- Verified: cell clearing happens only in Step 3 (`ring_buffer.rs:211-217`); no
  Step 1 branch writes `ring_cells`.
- Verified: the NOTE's core premise holds — for `rows >= 1`,
  `((ring_head + 1) % rows + rows - 1) % rows == ring_head`, so
  `new_bottom_abs == evicted_abs`. It is retained.
- Scope is the NOTE text only. The task's "実害" section observes that deleting
  Step 3 entirely would still leave the test green, but the stated expected
  behaviour asks for a comment correction, not a new test pinning the fill /
  `ring_wrapped` reset. No such test is added.
- The task's "該当箇所" line numbers (`tests.rs:528-530`) are stale; in the
  current worktree the NOTE spans lines 584-597 and its conclusion sentence is at
  595-597. Lines 528-530 are fixture-population code.

## Success Criteria

- [ ] AC1: The NOTE's "always a no-op" statement is limited to the `overflow` /
      `overflow_ridx` clearing, and no longer reads as a claim about the whole
      Step 3 block.
- [ ] AC2: The NOTE states that the cell fill and the
      `ring_wrapped[new_bottom_abs] = false` reset have no eviction-side
      counterpart and are therefore necessary.
- [ ] AC3: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path
      crates/term_core/Cargo.toml --lib` is green.
- [ ] AC4: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`
      reports no diff.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. FR1-FR4 are all `resolved`.

## References

- Requirements document: `feature-docs/ring-push-blank-note-scope/REQUIREMENTS.md`
- Edited file: `crates/term_core/src/ring_buffer/tests.rs`
- Referenced implementation: `crates/term_core/src/ring_buffer.rs`
