# Feature: ascii-fast-path-wide-pair-cleanup

## Overview

The ASCII fast path in `process_pty_data` (`crates/term_core/src/terminal_dispatch.rs`) overwrites cells without performing the D2 repair, so an ASCII character that overwrites a fullwidth cell leaves an orphaned width-0 spacer visible in the terminal. This feature closes that gap inside the fast path's write step and brings `blank_wide_pair_half`'s doc comment in line with the set of paths that actually perform the repair.

Requirements document: `feature-docs/ascii-fast-path-wide-pair-cleanup/REQUIREMENTS.md`.

## Objectives

- Make the D2 invariant (no orphaned wide-pair half in the grid) hold for every print path in term_core, including the ASCII fast path in `process_pty_data`, so a fullwidth cell overwritten by ASCII never leaves a stray width-0 spacer visible in the terminal.
- Keep the codebase's own documentation truthful: the enumeration of D2-repair call sites in `blank_wide_pair_half`'s doc comment must match the set of paths that actually perform the repair.
- Preserve the reason the ASCII fast path exists — minimum per-byte cost on the ASCII common case — while closing the correctness gap.

## Acceptance Criteria

The requirements analysis defines acceptance criteria rather than user stories; the design step was skipped because the change has no user-visible surface, no new UI and no interaction flow.

- [ ] **AC1** (FR1, FR2): Approach (a) is implemented — the ASCII fast path reads `old_width` before writing and calls `blank_orphaned_neighbor_before_overwrite` when `old_width != 1`, and removes the overflow entry in the same shape as `handle_print_ascii`.
- [ ] **AC2** (FR6): A unit test proves that fullwidth output followed by a separate `process_pty_data` call carrying CR + ASCII leaves no orphaned spacer.
- [ ] **AC3** (NFR1): The performance impact on the ASCII fast path is evaluated and the evaluation is written down, showing the `old_width` read adds no cost to the width-1 common case beyond a resident-field read and a branch.
- [ ] **AC4** (FR5): `blank_wide_pair_half`'s doc comment enumerates D2-repair call sites consistently with the post-change code, so a reader cannot conclude that an uncovered print path is covered.
- [ ] **AC5** (FR7, NFR2): The existing term_core `--lib` suite and the src-tauri `--lib` suite both pass, and `cargo check --no-default-features` still succeeds.

## Technical Requirements

### Functional Requirements

- **FR1 — Fast path performs orphan-neighbor repair before overwrite:** Before writing a cell, the ASCII fast path in `crates/term_core/src/terminal_dispatch.rs` reads the target cell's existing width and, when that width is not 1, invokes the same orphan-neighbor blanking the slow path uses (`blank_orphaned_neighbor_before_overwrite`, backed by the `blank_wide_pair_half` primitive), so the surviving half of a broken wide pair is blanked rather than left orphaned.
- **FR2 — Fast path removes the overflow-table entry:** The ASCII fast path removes the overflow-table entry for each cell it overwrites, in the same shape as `handle_print_ascii` does on the slow path.
- **FR3 — Fast/slow path observable parity:** For any byte stream, the resulting grid state, cell widths and overflow-table contents are identical whether the bytes were consumed through the ASCII fast path or through the slow `handle_print_ascii` path — including when the stream is split across `process_pty_data` calls at an arbitrary boundary.
- **FR4 — Fast path is not narrowed to achieve the fix:** The fix is implemented inside the fast path's write step. `can_fast_ascii`'s admission conditions are not extended with a grid-state precondition (for example "no wide cells present"), so the set of inputs eligible for the fast path is unchanged.
- **FR5 — D2-repair call-site documentation matches reality:** `blank_wide_pair_half`'s doc comment in `crates/term_core/src/terminal_cells.rs` is updated so its enumeration of D2-repair call sites names the dispatch ASCII fast path alongside the print slow path (`handle_print_ascii` / `write_grapheme_to_grid`), ICH/DCH and range-erase, leaving no path that a reader would wrongly assume is covered or wrongly assume is not.
- **FR6 — Regression test for the reported break:** A unit test reproduces the reported sequence — emit a fullwidth character, then in a subsequent `process_pty_data` call send CR followed by an ASCII character that overwrites the wide base at col 0 — and asserts that no orphaned width-0 spacer remains at col 1.
- **FR7 — No behavior change for non-wide content:** For input that touches no wide-pair cell, the fast path's observable output (grid contents, widths, emitted callbacks) is unchanged from the pre-change behavior.

### Non-Functional Requirements

- **NFR1 - Performance:** The ASCII common case (overwriting a width-1 cell with a width-1 character) gains no more than an already-resident field read plus a well-predicted branch. No additional allocation, no additional pass over the input buffer, and no additional per-byte function call that is not inlinable. This is the concrete reading of the "NFR4 impact evaluation" acceptance criterion; the evaluation is recorded in the feature's documentation, not left implicit.
- **NFR2 - Scope:** The change is confined to `crates/term_core` (`terminal_dispatch.rs`, the `terminal_cells.rs` doc comment, and tests). term_core's public API is unchanged and no new dependency or dev-dependency is added.
- **NFR3 - Robustness:** The repair is safe on adversarial PTY input: a spacer at column 0 with no left neighbor, a wide base at the last column, and a width-0 cell that is a combining-mark residue rather than a wide-pair spacer must all be handled without panic, index-out-of-bounds, or blanking of a legitimate neighbor.
- **NFR4 - Convention:** New tests follow `test/README.md`: inline `#[cfg(test)] mod tests {}` next to the code under test, `<subject>_<scenario>_<expected>` naming, an explicitly constructed `TerminalCore` per test, input driven through `process_pty_data`, and assertions on observable grid contract rather than internal-only state.

## Implementation Approach

### Architecture

Affected components, all inside `crates/term_core`:

```
process_pty_data (terminal_dispatch.rs)
  ├── can_fast_ascii  ── admission conditions (unchanged, FR4)
  ├── ASCII fast path write step  ── gains the D2 repair (FR1) + overflow removal (FR2)
  └── slow path: handle_print_ascii / write_grapheme_to_grid  ── already repairs

terminal_cells.rs
  ├── blank_wide_pair_half                       ── primitive; doc comment updated (FR5)
  └── blank_orphaned_neighbor_before_overwrite   ── shared repair entry point
```

**Component relationships:** the fast path and the slow path both reach the same repair primitive, which is what makes FR3's parity a property of shared code rather than of duplicated logic.

### Data Flow

```
PTY bytes → process_pty_data → can_fast_ascii?
    ├─ yes → fast path write step:
    │          read old_width → old_width != 1 ? blank_orphaned_neighbor_before_overwrite : —
    │          → write cell → remove overflow entry
    └─ no  → handle_print_ascii / write_grapheme_to_grid (existing repair)

Both branches converge on identical grid + width + overflow-table state (FR3).
```

### API Design

Not applicable. term_core's public API is unchanged (NFR2); the change is internal to the print path.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `blank_wide_pair_half` (`terminal_cells.rs`): the D2-repair primitive the fast path's repair is backed by.
- `blank_orphaned_neighbor_before_overwrite` (`terminal_cells.rs`): the shared entry point the slow path already uses and the fast path will call.
- `handle_print_ascii` (slow path): the reference behavior for overflow-entry removal (FR2) and for parity (FR3).

**External Dependencies:**
- None. No new dependency or dev-dependency is added (NFR2).

### File Structure

```
crates/term_core/src/
├── terminal_dispatch.rs   # ASCII fast path write step: old_width read, repair call,
│                          # overflow-entry removal; inline #[cfg(test)] mod tests
└── terminal_cells.rs      # blank_wide_pair_half doc comment: D2-repair call-site enumeration
```

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR6): Orphan spacer removed when fast-path ASCII overwrites a wide base — given a fullwidth character was printed at col 0 in a prior `process_pty_data` call, when a subsequent `process_pty_data` call delivers CR followed by an ASCII byte, then col 0 holds the ASCII character at width 1 and col 1 is a blank width-1 cell, with no width-0 spacer remaining.
- [ ] **TS2** (FR3): Fast/slow path parity across chunk boundaries — given a byte stream containing a fullwidth character followed by CR and ASCII, when the same stream is delivered (i) as a single `process_pty_data` call and (ii) split so the ASCII tail lands in a fast-path-eligible chunk, then both cores end with identical grid contents, widths and overflow-table state.
- [ ] **TS3** (FR1): Overwriting the spacer half blanks the base — given a fullwidth character occupies cols 0-1, when fast-path ASCII overwrites col 1 (the width-0 spacer), then col 0's wide base is blanked to a width-1 blank and no orphan base survives.
- [ ] **TS4** (FR2): Overflow entry removed by the fast path — given a cell carrying an overflow-table entry, when fast-path ASCII overwrites that cell, then the overflow entry is gone, matching `handle_print_ascii`'s result for the same overwrite.
- [ ] **TS5** (FR7, NFR1): Pure-ASCII behavior unchanged — given a grid with no wide cells, when a pure-ASCII stream is processed through the fast path, then the resulting grid is identical to the pre-change behavior and no repair path is entered.

### Integration Tests

None defined by the requirements analysis.

### E2E Tests

**Existing E2E tests**: None recorded in the requirements analysis.
**Run command**: Not recorded.

### Edge Cases

- [ ] **TS6** (NFR3): Boundary safety — given a spacer at column 0, and a wide base at the last column of a row, when fast-path ASCII overwrites each in turn, then no panic or out-of-range access occurs and the neighbor rule is applied only within the row.
- [ ] A width-0 cell may be a combining-mark residue rather than a wide-pair spacer; the repair keys off the wide-pair relationship rather than `width == 0` alone (NFR3, assumption A5).

### Suite-Level Verification

- [ ] **TS7** (FR7, NFR2): Existing suites stay green — given the change is applied, when the term_core `--lib` suite, the src-tauri `--lib` suite and the CLI-only `cargo check` are run, then all pass.

### Performance Tests

- [ ] NFR1 impact evaluation (AC3): show that the `old_width` read adds no more than an already-resident field read plus a well-predicted branch on the width-1 common case, with no added allocation, no added pass over the input buffer, and no added non-inlinable per-byte call. The evaluation is written down in the feature's documentation.

## Security Considerations

- **Input Validation:** PTY input is treated as adversarial (NFR3): a spacer at column 0 with no left neighbor, a wide base at the last column, and a width-0 combining-mark residue must all be handled without panic, index-out-of-bounds, or blanking of a legitimate neighbor.
- Authentication, authorization, data protection, XSS, SQL injection and CSRF are not applicable to this change.

## Error Handling

No new error codes. The repair must not panic or index out of range on any of the NFR3 inputs; the neighbor rule is applied only within the row.

## Performance Optimization

### Performance Goals

- ASCII common case (width-1 cell overwritten by a width-1 character): at most one already-resident field read plus one well-predicted branch of added cost.
- No additional allocation, no additional pass over the input buffer, no additional non-inlinable per-byte function call.

### Optimization Strategies

- Implement the repair inside the fast path's write step rather than narrowing `can_fast_ascii` (FR4), so the set of inputs eligible for the fast path stays unchanged.
- Gate the repair on `old_width != 1`, so the ASCII common case takes the not-taken branch.

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and tested
- [ ] All test scenarios (TS1–TS7) pass
- [ ] Performance meets NFR1 and the evaluation is recorded (AC3)
- [ ] Robustness requirement NFR3 is satisfied
- [ ] `blank_wide_pair_half`'s doc comment matches the post-change code (AC4)
- [ ] The change stays confined to `crates/term_core` with no public API or dependency change (NFR2)
- [ ] New tests follow `test/README.md` (NFR4)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1–FR7, NFR1–NFR4) has `status: resolved`.

## Assumptions

Traced from the requirements analysis; not originated by this document.

- **A1** (confidence: high): Approach (a) — fix the fast path — is the selected approach, not (b) documenting the gap. Evidence: the feature slug is "…-cleanup" (a repair, not a documentation note); the task classifies the item as 種別: バグ; option (b) is phrased as the fallback ("直ちに直さない場合"); and the constraints section pre-answers (a)'s only stated objection by noting `old_width` already sits in the touched cell's cache line. FR5 additionally folds in (b)'s underlying concern (doc accuracy).
- **A2** (confidence: medium): If the NFR1 evaluation ever demonstrates a real regression on the ASCII common case, the fallback is approach (b) — restrict `blank_wide_pair_half`'s doc to the print slow path and record the fast path as a known exception. Recorded as an assumption rather than a conditional requirement, so the spec stays deterministic on (a).
- **A3** (confidence: medium): The "NFR4" cited in the task's acceptance criteria means the prior spec's requirement that the ASCII fast path minimize per-byte cost; it is restated as this feature's NFR1 because the originating document was outside read scope.
- **A4** (confidence: medium): PR #37 (wide-pair-blank-primitive-unification) is already merged into the base this feature branches from, so `blank_wide_pair_half` and `blank_orphaned_neighbor_before_overwrite` exist as described.
- **A5** (confidence: medium): A width-0 cell is not always a wide-pair spacer (combining marks also produce width-0 cells), so the repair must key off the wide-pair relationship rather than "width == 0" alone. Captured as NFR3 and TS6.

## References

- Requirements document: `feature-docs/ascii-fast-path-wide-pair-cleanup/REQUIREMENTS.md`
- `crates/term_core/src/terminal_dispatch.rs`: `process_pty_data`, `can_fast_ascii`, the ASCII fast path write step
- `crates/term_core/src/terminal_cells.rs`: `blank_wide_pair_half`, `blank_orphaned_neighbor_before_overwrite`
- `test/README.md`: test placement, naming and authoring conventions (NFR4)
- PR #37 (wide-pair-blank-primitive-unification): introduced the shared blanking primitives (assumption A4)
