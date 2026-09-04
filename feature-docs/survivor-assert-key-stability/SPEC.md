# Feature: survivor-assert-key-stability

## Overview

The survivor assertion in `test_ring_push_blank_clears_recycled_row_overflow_entries` does not actually observe the design claim its own comment makes — that the ring slot key stays stable even when the viewport position shifts. This feature adds assertions to that test so the claim is observed, so that orphan overflow entries left behind (the inverse of over-clear) are not treated as a pass, and so that a regression confined to `ring_push_blank` Step 3's fill target is detected. See `REQUIREMENTS.md` for the Japanese requirements document.

## Objectives

- Make the survivor assertion in `test_ring_push_blank_clears_recycled_row_overflow_entries` actually observe the design claim stated by the test's comment: the key is stable even when the viewport position shifts.
- Given that this test's sole purpose is pinning the lifetime and scope of the overflow-side tables, stop treating orphan-entry retention (leakage) — the inverse of over-clear — as a pass.
- Let this test, whose purpose is observing row scope, detect a regression in which only `ring_push_blank` Step 3's fill target is broken.

## User Stories

### US1: Detect a fill-target regression through the unit test
As a `term_core` developer, I want `test_ring_push_blank_clears_recycled_row_overflow_entries` to fail when only `ring_push_blank` Step 3's fill slice index is wrong, so that a row-scope regression is caught by the `term_core` `--lib` suite.

**Acceptance Criteria:**
- [ ] AC4: Injecting a mutation that only corrupts the slice index in `ring_push_blank` Step 3 makes `test_ring_push_blank_clears_recycled_row_overflow_entries` fail, and reverting the mutation makes it pass.
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` is green.

### US2: Assert what the test comment claims
As a `term_core` developer, I want the survival block to assert both the survivor row's ring slot key and the survivor row's content, so that the test observes the claim its comment makes instead of passing vacuously.

**Acceptance Criteria:**
- [ ] AC1: The survival block after LF contains `assert_eq!(core.viewport_abs(0) as u32, abs1);`.
- [ ] AC2: The survival block observes the survivor row's content through `get_cell_char(0, 0)` and asserts it matches the grapheme the fixture printed.
- [ ] AC3: The existing removal post-assertions and the existing survivor-presence assertions remain.
- [ ] AC7: The permanent diff is confined to `crates/term_core/src/ring_buffer/tests.rs`.
- [ ] AC6: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean.

## Technical Requirements

### Functional Requirements

- **FR1 — Add an assertion observing survivor key stability:** In the post-LF survival block of `test_ring_push_blank_clears_recycled_row_overflow_entries` in `crates/term_core/src/ring_buffer/tests.rs`, add `assert_eq!(core.viewport_abs(0) as u32, abs1);` so that the ring slot key `abs1`, captured for the survivor row before the scroll, still corresponds to viewport row 0 after the scroll.
- **FR2 — Add an assertion observing that the survivor row's content survives:** In the same survival block, observe that the survivor row's content itself remains, using `core.get_cell_char(0, 0)`. Assert it matches the `'g'` + U+0301..U+0308 combining-mark sequence the fixture printed, so that a blanked row is detected (a blanked row becomes `Cell::EMPTY`, `is_overflow()` turns false, and `get_cell_char` therefore returns a halfwidth space). Follow the reference style of `get_cell_char(col, row)` as used by `test_scroll_up_internal_full_screen_no_scrollback_capacity` in the same file.
- **FR3 — Keep the existing assertions:** Keep the pre-scroll anti-vacuity assertions, the recycled-row removal post-assertions, and the existing survivor overflow / overflow_ridx presence assertions — none are removed; the new assertions are additive.
- **FR4 — Confirm the test goes red under mutation injection:** Temporarily introduce a mutation into `ring_push_blank` Step 3 in `crates/term_core/src/ring_buffer.rs` that corrupts only the fill-target slice index (for example, deriving `new_base` from the post-rotation `ring_head` while leaving the `new_bottom_abs` passed to `overflow_clear_row` / `overflow_ridx_clear_row` correct), confirm that `test_ring_push_blank_clears_recycled_row_overflow_entries` goes red, then revert the mutation.
- **FR5 — Confine the change scope to the test file:** Confine the permanent change to `crates/term_core/src/ring_buffer/tests.rs` and do not change production-code behaviour (including `crates/term_core/src/ring_buffer.rs`). FR4's mutation is for local verification only and is not committed.

### Non-Functional Requirements

- **NFR1 - Conformance to the existing test style:** Follow the conventions of `test/README.md` (inline `#[cfg(test)] mod tests`, an explicitly constructed `TerminalCore` per test, no shared fixtures, assertions against the observable contract). Add no new test crate or dependency.
- **NFR2 - Formatting consistency:** `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean (rustfmt style_edition 2024).
- **NFR3 - No impact on test runtime:** The addition is only a few assertion lines and adds no substantive runtime to the `term_core` `--lib` suite. It spawns no process and performs no I/O or sleep.
- **NFR4 - Determinism:** The added assertions are deterministic under parallel execution. The test touches only its own `TerminalCore` and depends on no global state, filesystem, or clock.

## Assumptions

Every assumption below is carried over from the resolved requirements; none originates in this document.

- **A1** (impact: medium, reversible): The survivor row content assertion (FR2), marked "(optional)" in the task description, is treated as mandatory. Rationale: it is the only observation that satisfies the definition of done's "the test goes red under the reproduction mutation". The mutation leaves the overflow-clear side row key correct, so both FR1 and the existing assertions pass through unaffected under the mutation.
- **A2** (impact: low, reversible): The expected survivor cell string is the grapheme formed by `'g'` (0x67) printed by the fixture followed by the eight combining marks U+0301..U+0308. Rationale: it follows from the `marks` array and the `handle_print(0x67)` ordering inside the test. A blanked row makes `get_cell_char` return a halfwidth space, which distinguishes the two.
- **A3** (impact: low, reversible): `viewport_abs`, used by FR1, is `pub(crate)`, but the test lives in the same crate, so no additional visibility change is required. Rationale: the same test already calls `core.viewport_abs(0)` / `core.viewport_abs(1)`.
- **A4** (impact: low, reversible): FR4's mutation verification is performed as a temporary local edit and is not left in the repository. Rationale: the reproduction procedure deliberately breaks production code and is not an artifact to commit.

## Implementation Approach

### Architecture

**Component Diagram:**
```
crates/term_core/
├── src/ring_buffer.rs        # ring_push_blank (Step 3 fill target) — NOT modified
└── src/ring_buffer/tests.rs  # test_ring_push_blank_clears_recycled_row_overflow_entries
                              #   ├─ pre-scroll anti-vacuity assertions      (FR3, kept)
                              #   ├─ LF
                              #   ├─ survival block                          (FR1, FR2, added)
                              #   └─ recycled-row removal assertions         (FR3, kept)
```

The change is additive assertions inside one existing test function. No module, type, or public API is introduced or altered.

### Data Flow

```
fixture prints 'g' + U+0301..U+0308 → row captured with ring slot key abs1
LF → ring_push_blank rotates and fills Step 3's target rows
survival block → core.viewport_abs(0)   ⇒ compared against abs1        (FR1)
               → core.get_cell_char(0,0) ⇒ compared against the grapheme (FR2)
```

### API Design

No API surface is added or changed. The assertions use existing in-crate accessors:

| Accessor | Visibility | Use |
|---|---|---|
| `TerminalCore::viewport_abs(row)` | `pub(crate)` (A3) | FR1 — resolve the ring slot key of viewport row 0 |
| `TerminalCore::get_cell_char(col, row)` | existing test-facing accessor | FR2 — read the survivor row's leading grapheme |

### Database Schema

Not applicable — this feature touches no persistent storage.

### Dependencies

**Internal Dependencies:**
- `crates/term_core/src/ring_buffer.rs`: provides `ring_push_blank`, whose Step 3 fill target is the behaviour under observation. Read-only for the permanent change (FR5); temporarily mutated for FR4 only.
- `crates/term_core/src/ring_buffer/tests.rs`: the sole file that carries the permanent change (FR5).
- `test_scroll_up_internal_full_screen_no_scrollback_capacity` in the same test file: the reference style for `get_cell_char(col, row)` usage (FR2).

**External Dependencies:**
- None. No new test crate or dependency is added (NFR1).

### File Structure

```
crates/term_core/
├── Cargo.toml
└── src/
    ├── ring_buffer.rs         # unchanged (FR5); temporarily mutated for FR4 only
    └── ring_buffer/
        └── tests.rs           # the only permanently modified file (FR5)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/survivor-assert-key-stability/**`
- `test-docs/survivor-assert-key-stability/**`

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
- [ ] TS1 — Baseline green: run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` with no mutation in place. Expected: every test passes, including `test_ring_push_blank_clears_recycled_row_overflow_entries`. (FR1, FR2, FR3, NFR1, NFR3, NFR4)
- [ ] TS2 — Red under fill-index mutation: corrupt only the slice index in `ring_buffer.rs` Step 3 (leaving the overflow-clear side correct) and run the same `--lib` command. Expected: the survivor row is blanked, so FR2's content assertion fails and the test goes red. (FR2, FR4)
- [ ] TS3 — Green after reverting the mutation: revert TS2's mutation and run `--lib` again. Expected: every test passes and no mutation remains in the working tree. (FR4, FR5)

### Integration Tests
Not applicable — the change is confined to a `term_core` unit test.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases
- [ ] Survivor row blanked: `Cell::EMPTY` makes `is_overflow()` false, so `get_cell_char` returns a halfwidth space; FR2's assertion distinguishes this from the fixture's grapheme.
- [ ] Mutation leaves the overflow-clear side row key correct: FR1 and the existing assertions pass through unaffected, which is why FR2 is mandatory (A1).

### Performance Tests
- [ ] TS4 — Formatting check: run `cargo fmt --manifest-path crates/term_core/Cargo.toml --check`. Expected: it exits with no diff. (NFR2)
- [ ] No runtime impact: the addition is a few assertion lines with no process spawn, I/O, or sleep (NFR3).

## Security Considerations

Not applicable. The change adds assertions to an existing in-crate unit test; it introduces no input surface, no data handling, and no production-code behaviour change (FR5).

## Error Handling

The only failure mode is an assertion failure, which is the intended signal:

| Condition | Signal |
|---|---|
| Viewport row 0's ring slot key no longer equals `abs1` | FR1 assertion fails; test red |
| Survivor row blanked (halfwidth space instead of the fixture grapheme) | FR2 assertion fails; test red |
| Recycled-row overflow entries not removed, or survivor entries missing | Existing FR3 assertions fail; test red |

### Error Flow

```
Assertion fails → cargo test reports the failing test → developer inspects the row scope of ring_push_blank Step 3
```

## Performance Optimization

### Performance Goals
- No substantive increase in the `term_core` `--lib` suite runtime (NFR3).

### Optimization Strategies
- Keep the addition to assertion lines only; spawn no process and perform no I/O or sleep (NFR3).

### Caching Strategy
Not applicable.

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented and observed.
- [ ] All test scenarios (TS1–TS4) pass.
- [ ] AC1: The survival block after LF contains `assert_eq!(core.viewport_abs(0) as u32, abs1);`.
- [ ] AC2: The survival block observes the survivor row's content through `get_cell_char(0, 0)` and asserts it matches the grapheme the fixture printed.
- [ ] AC3: The existing removal post-assertions and the existing survivor-presence assertions remain.
- [ ] AC4: Injecting the Step 3 slice-index-only mutation makes `test_ring_push_blank_clears_recycled_row_overflow_entries` fail, and reverting it makes the test pass.
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` is green.
- [ ] AC6: `cargo fmt --manifest-path crates/term_core/Cargo.toml --check` is clean.
- [ ] AC7: The permanent diff is confined to `crates/term_core/src/ring_buffer/tests.rs`.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every FR and NFR has `status: ok`.

## Implementation Phases (if applicable)

Not applicable — the change is a single additive edit to one test function.

## References

- Requirements document (Japanese): `feature-docs/survivor-assert-key-stability/REQUIREMENTS.md`
- Test under change: `crates/term_core/src/ring_buffer/tests.rs`
- Production code observed (unchanged): `crates/term_core/src/ring_buffer.rs`
- Test conventions (NFR1): `test/README.md`
