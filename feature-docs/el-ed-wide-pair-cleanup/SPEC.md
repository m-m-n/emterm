# Feature: el-ed-wide-pair-cleanup

## Overview

EL (CSI K) and ED (CSI J) in `crates/term_core` currently leave orphaned width-2 base or width-0 spacer cells at the edges of a `clear_line_range` erase, breaking the grid width invariant that ECH/DCH/ICH and the print path have upheld since PR #30 (feature wide-pair-overwrite-cleanup). This feature adds wide-pair partner cleanup to the EL/ED cursor-row erase paths so the surviving partner is blanked to a width-1 space, matching reference-implementation behavior (xterm / Alacritty / WezTerm). Full requirement text: `feature-docs/el-ed-wide-pair-cleanup/REQUIREMENTS.md`.

## Objectives

- Eliminate the remaining wide-pair orphan-cell bug class in term_core's erase path: EL (CSI K) and ED (CSI J) currently leave orphaned width-2 base or width-0 spacer cells at the edges of a `clear_line_range` erase, breaking the grid's width invariant that ECH/DCH/ICH and the print path already uphold since PR #30 (feature wide-pair-overwrite-cleanup).
- Match reference-implementation behavior (xterm / Alacritty / WezTerm all blank the surviving partner on erase), closing review round1 finding 221c2569b17a55f9 (severity: medium).

## User Stories

### US1: EL/ED erase leaves no orphaned wide-pair half

As an application that emits EL / ED escape sequences to the terminal, I want the surviving half of a wide pair to be blanked when its partner is erased, so that the grid keeps its width invariant after the erase.

**Acceptance Criteria:**

- [ ] AC1: With the cursor on a width-0 spacer, `ESC[K` (EL 0) blanks the width-2 base at col-1 to a width-1 space (FR1).
- [ ] AC2: With the cursor on a width-2 base, `ESC[1K` (EL 1) blanks the width-0 spacer at col+1 to a width-1 space (FR2).
- [ ] AC3: `ESC[J` and `ESC[1J` (ED 0/1) apply the same rule on the cursor row (FR3).
- [ ] AC4: Full-line clears (EL 2 / ED 2) are treated as no-ops for edge cleanup — no behavioral change (FR4).

### US2: The fixed paths are guarded against regression

As a term_core maintainer, I want the proven failure paths captured as unit tests, so that the fixed behavior is guarded against regression.

**Acceptance Criteria:**

- [ ] AC5: The reproduction paths above exist as term_core unit tests and pass, acting as regression guards (FR6); the full term_core `--lib` suite passes.
- [ ] AC6: Any surviving out-of-scope items are recorded in SPEC / IMPLEMENTATION as known remaining work (FR7).

## Technical Requirements

### Functional Requirements

- **FR1 — EL 0 blanks the orphaned base left of the erase range:** When CSI K / CSI 0 K executes with the cursor on a width-0 spacer cell at col > 0, after the `[col, cols)` range is BCE-cleared, the width-2 base cell at col-1 is blanked to a width-1 space via the wide-pair partner cleanup (`blank_wide_pair_split` semantics: character content and width change only; fg/bg/flags/hyperlink preserved). Evidence: `crates/term_core/src/csi_screen.rs:49` currently calls `clear_line_range` with no cleanup; the required predicate is identical to ECH's `start_is_spacer` at `csi_screen.rs:79,84-88`.
- **FR2 — EL 1 blanks the orphaned spacer right of the erase range:** When CSI 1 K executes with the cursor on a width-2 base cell, after the `[0, col+1)` range is BCE-cleared, the width-0 spacer cell at col+1 is blanked to a width-1 space with attributes preserved. Evidence: `crates/term_core/src/csi_screen.rs:53`; the required predicate is identical to ECH's `last_is_base` at `csi_screen.rs:80,89-93` (end = col+1).
- **FR3 — ED 0 / ED 1 apply the same cleanup on the cursor row:** CSI J (ED 0) and CSI 1 J (ED 1) issue the same cursor-row `clear_line_range` calls as EL 0 / EL 1 respectively (`crates/term_core/src/csi_screen.rs:14` and `:25`) and receive the same partner cleanup as FR1 / FR2. Non-cursor rows are cleared whole by `clear_line` and need no edge cleanup.
- **FR4 — Full-line clears remain no-ops for edge cleanup:** EL 2 (`csi_screen.rs:57`) and ED 2 (`csi_screen.rs:30-32`), and every full-row `clear_line` call inside ED 0/1, clear the whole line and therefore perform no partner cleanup — a wide pair cannot straddle a full-row erase boundary. No behavioral change to these paths.
- **FR5 — Partner blanking preserves cell attributes:** The partner cell blanked outside the erase range keeps its own fg/bg/flags/hyperlink; only character content and width change, consistent with `blank_wide_pair_split`'s documented contract (`crates/term_core/src/csi_edit.rs:155-179`). Cells inside the erase range keep receiving the BCE cell exactly as today.
- **FR6 — Regression unit tests in term_core:** The three proven failure paths (spacer-cursor EL 0, base-cursor EL 1, ED 0/1 cursor-row equivalents) plus the EL 2 / ED 2 no-op are added as inline `#[cfg(test)]` unit tests in `crates/term_core`, following the crate's `<subject>_<scenario>_<expected>` naming convention, guarding against regression.
- **FR7 — Remaining out-of-scope items documented:** Items deliberately kept out of scope (partner-cleanup primitive consolidation / chokepoint refactor, overflow-path tests, ECH/DCH/ICH/print already handled by PR #30) are recorded in SPEC / IMPLEMENTATION as known remaining work where still applicable.

### Non-Functional Requirements

- **NFR1 — Dependencies:** No new dependencies in `crates/term_core` (crate currently depends only on serde/bincode/log/unicode-width).
- **NFR2 — Behavioral compatibility:** No change to erase semantics inside the cleared range: BCE fill, overflow clearing, and dirty-row marking behave exactly as before; only the boundary partner cells gain the blanking step.
- **NFR3 — Performance:** Negligible performance impact on the erase hot path: at most two width lookups and two conditional single-cell writes per EL/ED cursor-row call (same cost profile ECH already pays).
- **NFR4 — Boundary safety:** Out-of-bounds boundary columns (col 0 for the left edge, cols for the right edge) are safe: `blank_wide_pair_split` is a no-op for out-of-bounds columns and for cells that are not a spacer/base half.

## Implementation Approach

### Architecture

The change is confined to the escape-sequence erase path of the `term_core` crate. No process boundary, UI surface, or persisted data is involved.

**Component Diagram:**

```
crates/term_core
├── csi_screen.rs
│   ├── handle_erase_in_display (:10)   ED 0/1/2
│   │   └── clear_line_range (:14, :25) cursor row  ── partner cleanup added (FR3)
│   ├── handle_erase_in_line (:45)      EL 0/1/2
│   │   └── clear_line_range (:49, :53)             ── partner cleanup added (FR1, FR2)
│   └── ECH partner predicates (:79-93) start_is_spacer / last_is_base (reference pattern)
└── csi_edit.rs
    └── blank_wide_pair_split (:155-179, def :161, pub(crate))  ── partner blanking primitive
```

### Data Flow

```
EL / ED sequence → handle_erase_in_line / handle_erase_in_display
                 → clear_line_range (BCE fill of the erase range; unchanged, NFR2)
                 → boundary check (spacer at range start / base at range end)
                 → blank_wide_pair_split on the surviving partner outside the range (FR5)
```

### Implementation approach decision (deferred to the plan phase)

Two approaches both satisfy the acceptance criteria; the choice is left to the planning step:

- **(a)** Replicate ECH's local pre-capture pattern at the four EL/ED cursor-row call sites. Only the "out of scope" clause of the existing comment at `crates/term_core/src/csi_screen.rs:74-78` needs refreshing.
- **(b)** Fold partner cleanup into `clear_line_range` with a full-row exception. The existing comment at `crates/term_core/src/csi_screen.rs:74-78` — which explicitly states the cleanup "is never folded into `clear_line_range` itself" because it is shared with ED/EL — must be updated.

### Out of Scope / Known Remaining Work (FR7)

- Partner-cleanup primitive consolidation / chokepoint refactor.
- Overflow-path tests.
- ECH / DCH / ICH / print paths — already handled by PR #30 (wide-pair-overwrite-cleanup), which is merged into the integration base; they need no changes.

### API Design

Not applicable. This feature changes in-crate grid cell state only; no external API, endpoint, or protocol surface is added or modified.

### Database Schema

Not applicable. No persisted data is added or changed; the affected state is grid cell content (character, width, fg/bg/flags/hyperlink).

### Dependencies

**Internal Dependencies:**

- `crates/term_core/src/csi_edit.rs` — `blank_wide_pair_split` (pub(crate), same crate, already called from `csi_screen.rs`) is the partner-blanking primitive.
- PR #30 (feature wide-pair-overwrite-cleanup) — already merged into the integration base; establishes the width-invariant contract this feature extends to EL/ED.

**External Dependencies:**

- None. Per NFR1, no new dependency is added to `crates/term_core` (current dependencies: serde, bincode, log, unicode-width).

### File Structure

```
crates/term_core/src/
├── csi_screen.rs      # EL/ED handlers + cursor-row clear_line_range call sites + inline #[cfg(test)] tests
└── csi_edit.rs        # blank_wide_pair_split (partner blanking primitive; unchanged)
```

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (FR1, FR5, FR6): Print a wide character so its base sits at col-1 and spacer at col; move cursor onto the spacer; send `ESC[K`; assert the base cell at col-1 is now a width-1 space with its original attributes and the range `[col, cols)` is BCE.
- [ ] **TS-2** (FR2, FR5, FR6): Move the cursor onto a wide character's base at col (spacer at col+1); send `ESC[1K`; assert the spacer at col+1 is now a width-1 space and `[0, col+1)` is BCE.
- [ ] **TS-3** (FR3, FR5, FR6): Repeat both scenarios with `ESC[J` (ED 0) and `ESC[1J` (ED 1); assert identical cursor-row results plus correct full clears of the rows below/above.
- [ ] **TS-6** (FR4, FR6): No-op — EL 2 and ED 2 over a row containing wide pairs produce a fully BCE-cleared row with no additional writes attributable to partner cleanup.

### Integration Tests

Not applicable. The behavior is fully observable at the `term_core` unit-test level; the full term_core `--lib` suite must pass (AC5).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] **TS-4** (FR1, FR2): Negative — cursor on the base with `ESC[K` (both halves inside the range) leaves no orphan and triggers no extra blanking; cursor on the spacer with `ESC[1K` (base also inside the range) likewise.
- [ ] **TS-5** (NFR4): Boundary — cursor at col 0 with `ESC[K` (full-row range, no left partner) and cursor such that col+1 == cols with `ESC[1K` (right partner check out of bounds) are safe no-ops for the cleanup step.

### Performance Tests

Not applicable as a separate test. NFR3 bounds the added cost to at most two width lookups and two conditional single-cell writes per EL/ED cursor-row call — the same cost profile ECH already pays.

## Security Considerations

Not applicable. The change is a behavioral fix inside term_core's grid state; no authentication, authorization, input parsing surface, or data protection boundary is added or altered.

## Error Handling

No new error paths. Boundary columns that fall out of bounds, and cells that are not a spacer/base half, are handled by `blank_wide_pair_split` as no-ops (NFR4).

## Performance Optimization

### Performance Goals

- At most two width lookups and two conditional single-cell writes per EL/ED cursor-row call (NFR3).
- No change to BCE fill, overflow clearing, or dirty-row marking inside the erase range (NFR2).

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and tested
- [ ] All test scenarios (TS-1 – TS-6) pass
- [ ] The full term_core `--lib` suite passes (AC5)
- [ ] NFR1–NFR4 are satisfied
- [ ] Existing EL/ED tests (`csi_screen.rs` tests module, e.g. `test_handle_erase_in_line_to_end`) keep passing unchanged
- [ ] Remaining out-of-scope items are recorded as known remaining work (AC6 / FR7)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every requirement (FR1–FR7, NFR1–NFR4) is resolved. The implementation-approach choice (a) vs (b) recorded above is a planning-step decision, not an unresolved requirement.

## References

- Requirements document (Japanese): `feature-docs/el-ed-wide-pair-cleanup/REQUIREMENTS.md`
- `crates/term_core/src/csi_screen.rs`: `handle_erase_in_display` (:10), `handle_erase_in_line` (:45), `clear_line_range` call sites (:14 / :25 / :49 / :53), ECH partner predicates (:79-93), scope comment (:74-78)
- `crates/term_core/src/csi_edit.rs`: `blank_wide_pair_split` (:155-179, definition at :161)
- PR #30 (feature wide-pair-overwrite-cleanup): wide-pair partner cleanup for ECH/DCH/ICH and the print path
- Review round1 finding 221c2569b17a55f9 (severity: medium)
- Reference implementations for erase-time partner blanking: xterm, Alacritty, WezTerm
- Line references verified against the integration worktree at base_revision 91f3280b (task description cited PR #30 head 0d3a4ff; the references match)
