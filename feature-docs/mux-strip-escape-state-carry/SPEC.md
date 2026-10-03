# Feature: mux-strip-escape-state-carry

## Overview

In the mux daemon's write filter, carry the end state of the bytes written after the strip as Ground / Escape / Designator / Csi(phase), so that output and state are independent of where reads are split, and decide the closure at a cut from that state. The feature also records the verdict, reason and regression tests for the review round 1 medium finding `a879a02de382209f`. See [REQUIREMENTS.md](REQUIREMENTS.md) for the requirements in detail.

## Objectives

- In the mux daemon's write filter, carry the end state of the bytes written after the strip, distinguishing Ground / Escape / awaiting a designator / Csi(phase), so that output and state equal the result of a single call regardless of where reads are split.
- At a cut where the written stream ends in Escape (or awaiting a designator), keep ring replay from completing a query (CPR and the like) or a reset (RIS) the client did not start.
- Decide whether the review round 1 medium finding `a879a02de382209f` is resolved or needs no action, and record the reason and the regression tests.

## User Stories

N/A. The acceptance criteria are listed under "Success Criteria".

## Technical Requirements

### Functional Requirements

- **FR1:** Carry the end state of the written stream. The write filter keeps the end state of the bytes it wrote after the strip, distinguishing Ground / Escape / Designator (awaiting a designator) / Csi(Entry) / Csi(Param), and starts the next call's strip from that state. The stateful strip (`strip_pty_output_for_scrollback_write_with_csi_state`, or its successor) takes and returns this state. When the strip removes a construct that immediately follows an ESC it wrote, the resulting end state Escape is carried as is, not narrowed to None (Ground). This holds on the normal path, after a call without a cut, and for the last segment of an overflow flush.
- **FR2:** A carried Designator does not change the strip's removal decisions. When a `(` / `)` right after a construct the strip removed leaves the written stream awaiting a designator (Designator), that state is carried too. Whether the next call copies its first byte verbatim (instead of reading it as the start of a strip target) is decided, as before, by the boundary scan's `awaiting_designator` alone. The carried Designator is used only for state transitions and does not change removal decisions. As a result, the strip's output for the same bytes is the same whether they are passed in one call or split.
- **FR3:** Decide the closure at a cut from the end state of the written stream. At a cut (an in-call cut, an empty-segment cut, the reader's fallback closure (an empty range + fed 0 cut), or a cut following an overflow flush), write at most one closure according to the end state of the bytes written after the strip: Csi(Entry|Param) writes CSI_CLOSING (DEL), Escape writes the FR4 closure, Designator writes the designator ESC (the same single byte as predecessor round4 FR3), and Ground writes nothing. The state after the cut is Ground. For the reproduction input (call 1 `ESC ESC]777;emterm;markdown;begin;id=x BEL`, call 2 `[6` + a trailing cut), the output is `ESC` + `[6` + DEL. The same holds for `ESC` + `ESC[6n`, `ESC` + Kitty APC, `ESC` + SIXEL DCS, `ESC` + OSC 9999 emterm-md and `ESC` + agent-status.
- **FR4:** Closure at a position ending in Escape. The closure written at a cut where the written stream ends in Escape satisfies all of the following. (a) When term_core reads it right after the preceding written ESC, it completes the escape and returns to ground. (b) It causes no character display, cursor movement, response, or change of mode or character set. (c) It is not ESC (0x1B). (d) It does not form ST (`ESC \`) with the preceding ESC. (e) It is not written together with DEL or the designator ESC at the same cut. The closure is written after the strip, so it is never read as the start of a strip target. The concrete byte is chosen at create-plan from term_core's escape-state transitions and confirmed by term_core raw-stream comparison (as-03).
- **FR5:** Add regression tests. Add to the split-invariance corpus in `src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs` (part (c) of `post_strip_the_carried_csi_state_follows_the_stripped_output`) the inputs `ESC ESC` + each held target (HELD_TARGETS: OSC 777 launch, OSC 9999 emterm-md, agent-status, Kitty APC, SIXEL DCS) and the same inputs followed by `[6`. Verify every split position and byte-at-a-time feeding, both without a cut and with a trailing cut. Also add: (1) for an in-call cut and the fallback closure, the output is the FR3 / FR4 closure; (2) in the term_core raw-stream comparison, `[6n`, `n` and `c` after the cut get the same interpretation and responses as the baseline (neither CPR nor RIS appears); (3) the reproduction itself (call 1, then call 2 `[6` + a cut, then a later `n`). The CSI query (`ESC ESC[6n`) goes into the single-call case and the cut cases; following predecessor D2, it is excluded from the split and byte-at-a-time comparisons.
- **FR6:** Record the decision. For stable_id `a879a02de382209f`, write the verdict (resolved / no action needed), the reason and the regression tests to a decision record under `feature-docs/mux-strip-escape-state-carry/`. Leave the predecessor's `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` and `reviews/round1.yaml` unchanged. The splices that remain outside this fix (the strip removes a construct after a written ESC and a following `]` / `P` / `_` leaves a string open, or a query completes through cut-free concatenation) are written to the decision record as residuals, with their trigger conditions and the reason they are out of scope.

### Non-Functional Requirements

- **NFR1 - Compatibility:** The mux_ipc wire format, the Snapshot / SnapshotRestore frame shape, the snapshot byte layout, and the output of the shared strips (`strip_rich_content_and_remap_with_designator` / `strip_replayable_rich_content` / `strip_pty_output_for_scrollback_write(_with_designator)`) do not change. The ring content changes only when an FR3 / FR4 cut adds one closure byte (DEL for a CSI opened after a carried Escape, the Escape closure, or the designator ESC for a splice-made designator wait), and when the boundary scan is awaiting a designator but the written stream ends in Ground, so the designator ESC is no longer written.
- **NFR2 - Reader normal-path load:** Add no byte-scanning pass to the reader's normal path. The end state is derived as O(1) state inside the existing strip pass.
- **NFR3 - Security (TM-1):** On the cut path and its state-carry path, ring replay never completes a query or reset the client did not start.
- **NFR4 - Security (TM-2):** Scanning and stripping are bounded and do not panic. The 512 KiB pending cap and the overflow flush through the strip are kept.
- **NFR5 - Build and platform:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` pass. Behavior is the same on Linux and Windows.

## Implementation Approach

### Architecture

The change is confined to the mux daemon's write filter and the state reporting of the shared strip. No UI is touched.

Carried end state of the written stream (FR1):

| State | Meaning |
|-------|---------|
| Ground | The written stream is not inside an escape |
| Escape | The written stream ends in an ESC |
| Designator | The written stream ends awaiting a designator |
| Csi(Entry) / Csi(Param) | The written stream ends in an unfinished CSI |

Closure at a cut (FR3). At most one closure per cut; the state after the cut is Ground.

| End state of the written stream | Closure written at the cut |
|---------------------------------|----------------------------|
| Csi(Entry) / Csi(Param) | CSI_CLOSING (DEL) |
| Escape | The FR4 closure |
| Designator | The designator ESC (same single byte as predecessor round4 FR3) |
| Ground | Nothing |

The cuts covered are the in-call cut, the empty-segment cut, the reader's fallback closure (empty range + fed 0) and a cut following an overflow flush (FR3).

### Data Flow

- The stateful strip takes the carried end state and returns the new end state (FR1). An Escape end state is carried, not narrowed to None.
- The verbatim copy of the next call's first byte is decided by the boundary scan's `awaiting_designator` alone; a carried Designator affects only state transitions (FR2).
- On an overflow flush, the last segment's end state is carried; a segment followed by a cut gets its closure from its own end state (FR1, FR3).

### API Design

N/A. The mux_ipc wire format and the Snapshot / SnapshotRestore frame shape do not change (NFR1).

### Database Schema

N/A.

### Dependencies

**Internal Dependencies:**
- Shared strips (`strip_rich_content_and_remap_with_designator` / `strip_replayable_rich_content` / `strip_pty_output_for_scrollback_write(_with_designator)`): output unchanged (NFR1).
- Predecessor features mux-cut-csi-post-strip-closure and mux-suppressed-output-round4-fixes: their invariants carry over (as-01).
- term_core: oracle for raw-stream comparison and replay checks (FR4, FR5, AC-2, AC-3).

**External Dependencies:**
- None.

### Reference Impact

| Symbol | Change | Affected paths |
|--------|--------|----------------|
| `strip_pty_output_for_scrollback_write_with_csi_state` (`csi_in: Option<CsiPhase>` and the returned `Option<CsiPhase>` become the full written state) | Signature change or a successor form | `src-tauri/src/mux/scrollback_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/scrollback_filter/tests.rs` |
| `WrittenState` (private enum), `WrittenState::start`, `WrittenState::csi` | Visibility and/or constructor change: `start` takes a carried state separately from the `pending_designator` flag (FR2); `csi()` is no longer the carried value | `src-tauri/src/mux/scrollback_filter.rs` |
| `ScrollbackWriteFilter.csi` field (`Option<CsiPhase>`) and the test accessor `csi_phase()` | The field's type becomes the full written state; `csi_phase()` keeps its meaning (as-05) and a new accessor exposes the full state | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/round4_chain.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs` |
| `CSI_CLOSING` (and a companion closure constant for Escape) | A new closure constant may be added next to it; `CSI_CLOSING` itself is unchanged | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs`, `src-tauri/src/mux/ipc/pty_spawn/tests/round4_chain.rs` |
| `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` (read by a test) | Must stay unmodified | `src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi_record.rs`, `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` |
| Test names listed in `test-docs/mux-cut-csi-post-strip-closure/task0001.tests.yaml` | Update only if a listed test is renamed (`.claude/rules/test-docs-records.md`) | `test-docs/mux-cut-csi-post-strip-closure/task0001.tests.yaml`, `test-docs/mux-cut-csi-post-strip-closure/task0002.tests.yaml` |
| Test module registry | A new test module, if one is added, is declared next to the existing ones | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` |

Notes:

- The field doc of `ScrollbackWriteFilter.csi` says it is "never set together with awaiting_designator"; that changes.
- `WrittenState::start` and the strip pass's `verbatim = usize::from(start == WrittenState::Designator)`: a carried Designator from a splice must not set verbatim (FR2).
- Tests that strip a trailing CSI_CLOSING (for example `strip_closing` in `post_strip_cut_csi.rs`) may need the Escape closure too when their input ends in a written ESC.
- `post_strip_the_decision_record_states_the_verdict_and_the_residuals` pins the predecessor DECISIONS.md headings, the single `4c0ad9058a983648` row, three residual subsections and the "None." behavior-changing statement.
- The listed tests include `post_strip_the_carried_csi_state_follows_the_stripped_output`, `post_strip_the_closing_follows_the_stripped_output`, `post_strip_state_form_reports_the_csi_state_of_the_written_bytes` and `post_strip_state_form_output_equals_the_write_path_strip`. Extending a test in place needs no record update.

### File Structure

```
src-tauri/src/mux/scrollback_filter.rs                      # stateful strip returns the full written state (FR1, FR2)
src-tauri/src/mux/ipc/pty_spawn/write_filter.rs             # carried state and closure at a cut (FR1, FR3, FR4)
src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs # split-invariance corpus and cut cases (FR5)
src-tauri/src/mux/scrollback_filter/tests.rs                # stateful-strip table rows (TS-6)
feature-docs/mux-strip-escape-state-carry/DECISIONS.md      # decision record (FR6, as-02)
```

The implementation's changed files are derived at create-plan from every task's `files` entries in `workflow.yaml`.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-strip-escape-state-carry/**`
- `test-docs/mux-strip-escape-state-carry/**`

`feature-docs/mux-strip-escape-state-carry/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-strip-escape-state-carry/**` covers `test-docs/mux-strip-escape-state-carry/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-strip-escape-state-carry/` directory at all; the declared
`test-docs/mux-strip-escape-state-carry/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (FR1, FR3, AC-1): Run the reproduction through the write filter for each held target and for the CSI query. The output is `ESC` + `[6` + DEL, and replaying a later `n` produces no CPR.
- [ ] TS-2 (FR1, FR2, FR5, AC-2): Add `ESC ESC` + each held target, `ESC ESC` + each held target + `[6`, and `ESC ESC` + a held target + `(` to the split-invariance corpus in `post_strip_cut_csi.rs`. For every split position and byte-at-a-time feeding, without a cut and with a trailing cut, the State (emitted / pending / awaiting / carried state) equals the single-call result. Check the state after each call with a term_core oracle that can also report Escape and Designator.
- [ ] TS-3 (FR3, FR4, AC-3): For a cut after `ESC` + each removed construct (in-call cut, three cuts at the same position, fallback closure, and a second closure that writes nothing), the output is `ESC` + the closure. In the term_core raw-stream comparison, `[6n`, `n` and `c` after the cut equal the baseline.
- [ ] TS-4 (FR2, FR3, AC-4): A cut at a splice-made designator wait writes only the designator ESC, and a carried Designator does not change the next call's strip output. When the boundary scan is awaiting a designator but the written stream ends in Ground (`ESC ESC]777...BEL ( ESC (`), the cut writes nothing.
- [ ] TS-5 (FR1, FR3, AC-5): An OSC held up to near the cap is flushed and the run ends in `ESC` + a complete removed construct. With a following cut, the Escape closure is written; without a cut, Escape is carried.
- [ ] TS-6 (FR1): In the stateful-strip table in `scrollback_filter/tests.rs`, check rows that return the end state Escape / Designator (`ESC[6 ESC`, `ESC ESC` + a removed construct, `ESC[6 ESC(` and so on). Also check that the output is identical to the existing strip (the R9 identity check).

### Integration Tests
- [ ] TS-7 (FR3, NFR3): With the production reader and the visibility-restore harness (`run_visibility_restore_at`), feed `ESC` + a removed construct + a screen switch, then `[6n` / `c` in a later read. The client's responses, screen and cursor equal the raw-stream baseline.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] EC-1: A splice that interrupts a CSI. `ESC[6 ESC ESC]777...BEL` writes `ESC[6 ESC` and the end state is Escape (not Csi). The cut writes the Escape closure, not DEL.
- [ ] EC-2: An unfinished construct held after a written Escape and dropped by the cut (`ESC ESC]777...BEL ESC]0;ti` + cut). The held chain is not written; the Escape closure is written.
- [ ] EC-3: The call after a carried Escape starts with ESC. The written ESC ESC stays Escape. A following removed construct is removed as before.
- [ ] EC-4: A splice-made designator wait (`ESC ESC]777...BEL (`). The next call's first byte is not copied verbatim (FR2). The cut writes the designator ESC.
- [ ] EC-5: The boundary scan is awaiting a designator but the written stream ends in Ground (`ESC ESC]777...BEL ( ESC (`). The cut writes nothing (FR3).
- [ ] EC-6: A splice where `]` / `P` / `_` follows a written Escape and leaves a string open on the ring. WrittenState treats it as Ground. Out of scope for this fix; recorded as a residual (FR6).
- [ ] EC-7: A CSI query spanning calls is written as it arrives (predecessor D2), so it is excluded from the split and byte-at-a-time comparisons.
- [ ] EC-8: At most one closure per cut. DEL, the Escape closure and the designator ESC never overlap. A second fallback closure writes nothing.
- [ ] EC-9: In comparisons against the raw-stream baseline, the effects of the removed construct itself (the legitimate CPR to `ESC[6n`, Kitty responses and placements) are excluded (the method of predecessor EC-5).

### Performance Tests
- [ ] TS-8 (NFR2, NFR4, AC-9): Feed a long input alternating `ESC` + a removed construct (below and above the cap) in one call, two calls and byte at a time, with and without a cut. It finishes within budget, does not panic, and the output is equal for every feeding.

### Record Checks
- [ ] TS-9 (FR6, AC-7): The decision record has the `a879a02de382209f` row (verdict, reason, regression tests) and a residuals section. The predecessor decision record is unchanged.

### Build Checks
- [ ] TS-10 (NFR5): `--no-default-features` cargo check passes.

## Security Considerations

- **Authentication:** N/A.
- **Authorization:** N/A.
- **Input Validation:** Bounded and panic-free; the 512 KiB cap and the overflow flush through the strip are kept (NFR4, TM-2).
- **Data Protection:** On the cut path and its state-carry path, ring replay never completes a query (CPR, DA and the like) or a reset (RIS) the client did not start (NFR3, TM-1).
- **Closure constraint (FR4 (d)):** The closure never forms `ESC \`. The snapshot-time strip (`find_st_terminator`) searches for `ESC \` from an unterminated Kitty APC / SIXEL DCS introducer, so a new `ESC \` could change the snapshot's removal range.
- **XSS Prevention:** N/A.
- **SQL Injection Prevention:** N/A.
- **CSRF Protection:** N/A.

## Error Handling

### Error Codes

N/A.

### Error Flow

N/A. Scanning and stripping do not panic (NFR4).

## Performance Optimization

### Performance Goals
- No byte-scanning pass is added to the reader's normal path (NFR2).
- A long input alternating removed constructs and written ESCs finishes within budget (AC-9, TS-8).

### Optimization Strategies
- The end state is derived as O(1) state inside the existing strip pass (NFR2).

### Caching Strategy
- N/A.

## Success Criteria

- [ ] AC-1 (FR1, FR3, NFR3): With the reproduction (call 1 `ESC ESC]777;emterm;markdown;begin;id=x BEL` without a cut, then call 2 `[6` + a trailing cut, then a later `n`), the ring is `ESC[6` + DEL. Replaying it in term_core produces no CPR response and `n` is displayed as a character. The same holds for each held target and for the CSI query.
- [ ] AC-2 (FR1, FR2, FR5): In the split-invariance corpus extended with `ESC ESC` + each held target (+ `[6`), the output bytes, pending, designator wait and carried state for every split position and byte-at-a-time feeding equal the single-call result, both without a cut and with a trailing cut. The carried state after each cut-free call equals term_core's state for the written bytes.
- [ ] AC-3 (FR3, FR4, NFR3): At a cut right after `ESC` + each removed construct (both an in-call cut and the fallback closure), the ring ends in `ESC` + the FR4 closure. A second fallback closure writes nothing. Replaying that ring followed by `[6n`, `n` and `c` after the cut in term_core gives the same subsequent interpretation, responses, screen and cursor as the raw-stream baseline with the screen switch in between (the effects of the removed construct itself are excluded from the comparison).
- [ ] AC-4 (FR2, FR3): When a `(` / `)` right after a construct the strip removed leaves the written stream awaiting a designator, the cut writes the designator ESC exactly once, without overlapping DEL or the Escape closure. The carried designator wait does not change the next call's strip output.
- [ ] AC-5 (FR1, FR3): When an overflow-flush run ends in `ESC` + a complete removed construct and a cut follows, the Escape closure is written once. Without a cut, the last segment's end state (Escape) is carried.
- [ ] AC-6 (FR5): The added regression tests fail on the code before the fix and pass after it.
- [ ] AC-7 (FR6): The decision record under `feature-docs/mux-strip-escape-state-carry/` has the verdict, reason and regression tests for `a879a02de382209f`, and the out-of-scope residuals (trigger conditions and the reason they are out of scope). The predecessor DECISIONS.md and reviews/round1.yaml are unchanged.
- [ ] AC-8 (NFR1): Existing tests pass unchanged. Tests whose expectations are changed on purpose are listed in the decision record. If a test is renamed, the predecessor test-docs are updated per `.claude/rules/test-docs-records.md`.
- [ ] AC-9 (NFR5, NFR2, NFR4): `--lib` cargo test and `--no-default-features` cargo check pass. A long input alternating removed constructs and written ESCs finishes within budget and does not panic.

## Assumptions

- as-01: The invariants of the predecessor features (mux-cut-csi-post-strip-closure, mux-suppressed-output-round4-fixes) carry over: lock order, the mux_ipc wire format, the 512 KiB pending cap, DEL (CSI_CLOSING) as the closure byte, never writing the designator ESC and DEL together, adding no byte-scanning pass to the reader's normal path, leaving the shared strips' output unchanged, and holding no CSI bytes (D2).
- as-02: The decision record lives at `feature-docs/mux-strip-escape-state-carry/DECISIONS.md`; the predecessor's `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` and `reviews/round1.yaml` are not changed.
- as-03: The closure byte for a position ending in Escape is chosen at create-plan from term_core's escape-state transitions (`crates/term_core/src/parser/escape.rs`) to satisfy FR4's properties, and is confirmed by term_core raw-stream comparison.
- as-04: The scope is carrying the written stream's end state (Ground / Escape / Designator / Csi) and the closure at a cut. The string-introducer splice (EC-6) and predecessor FR6's three residuals (cut-free concatenation) are not handled; they are written to the decision record as residuals.
- as-05: The test accessor `ScrollbackWriteFilter::csi_phase()` keeps its meaning of returning the CSI sub-state (None for anything other than Csi). The full end state is read through a separate accessor. Existing `csi_phase()` expectations in `round4_chain.rs` / `round4_cut_csi.rs` / `post_strip_cut_csi.rs` do not change.
- as-06: In the table of `post_strip_state_form_reports_the_csi_state_of_the_written_bytes` in `scrollback_filter/tests.rs`, rows ending in a written ESC or `ESC (` (`ESC[6 ESC`, `ESC[6 ESC(`, `ESC(ESC`) may change their expectation from None to Escape / Designator once the stateful strip returns the full state. If they change, they are listed under behavior-changing tests in the decision record.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None.

## Implementation Phases (if applicable)

N/A.

## References

- Requirements document: `feature-docs/mux-strip-escape-state-carry/REQUIREMENTS.md`
- Predecessor SPEC: `feature-docs/mux-cut-csi-post-strip-closure/SPEC.md`
- Predecessor decision record: `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`
- Predecessor review record: `feature-docs/mux-cut-csi-post-strip-closure/reviews/round1.yaml`
- test-docs record rule: `.claude/rules/test-docs-records.md`
- Regression test location: `src-tauri/src/mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs`
