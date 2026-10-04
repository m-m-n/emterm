# Feature: mux-write-filter-overflow-lone-esc

## Overview

When the scrollback write filter (`ScrollbackWriteFilter`) flushes its pending run because it grew past `SCROLLBACK_FILTER_PENDING_CAP` (512 KiB), and the flushed run ends in a live lone ESC, the next PTY read's strip sees the remainder (for example `[6n`) without that ESC. The bytes are written verbatim and `ESC[6n` forms in the scrollback ring as an executable cursor-position report query. This feature holds that final ESC so the next read strips it together with its continuation, as the non-overflow path already does. Requirements document: `feature-docs/mux-write-filter-overflow-lone-esc/REQUIREMENTS.md`.

## Objectives

- On the overflow flush path of the scrollback write filter (pending past `SCROLLBACK_FILTER_PENDING_CAP` = 512 KiB), a strip target whose opening ESC is the last byte of the flushed run and whose remainder arrives in a later PTY read is never left in the scrollback ring in executable form (for example `ESC[6n` as a cursor-position report query).
- The overflow flush path classifies such a split strip target the same way the non-overflow path already does (the non-overflow path holds a lone trailing ESC in pending, so the next read's strip sees the whole sequence and removes it).

## User Stories

### US1: No executable query in the ring after an overflow flush ending in a lone ESC
As a mux user, I want a strip target split across an overflow flush ending in a lone ESC and the next read to be removed from the scrollback ring, so that replaying the ring gives no response to a query the client did not initiate.

**Acceptance Criteria:**
- [ ] AC-1 (FR1, FR6): Reproduction at filter level. An OSC is held at the cap (`osc_held_at_the_cap`). Call 1 is its continuation (more body, BEL, `abc`, ESC); it overflows. Call 2 is `[6n`. The two calls together write no `ESC[6n`, and term_core fed the written bytes gives no cursor-position report.
- [ ] AC-2 (FR1, NFR2): For each strip-target continuation that follows the lone ESC in a later call (`[6n`, `[5n`, `[c`, `]777;emterm;markdown;...BEL`, `]9999;emterm-md;...BEL`, `]777;emterm;agent-status;...BEL`, `_G...ESC\\`, `P...q...ESC\\`), the construct is absent from the written bytes. The total output equals the output of feeding the same stream so that the final ESC and its continuation arrive in one non-overflowing call.
- [ ] AC-7 (FR1, FR6): Production reader. Chunks that grow an OSC past the cap end in a lone ESC, and the next chunk is `[6n`. The pane's scrollback ring holds no executable `ESC[6n`.

### US2: Overflow path keeps its other behavior
As a mux user, I want every other part of the overflow flush, and every non-strip continuation, to keep its current output, so that no byte is lost and existing behavior stays as pinned.

**Acceptance Criteria:**
- [ ] AC-3 (FR2, FR3, FR5): Right after the overflowing call of AC-1: `pending()` equals `[ESC]` (`pending_len() == 1`). The written bytes equal the strip of the run without its final ESC. `written_state()` is the end state of those bytes (Ground for a run ending in `abc` + ESC). `awaiting_designator()` is false. `outcome.carried` is None. The attribution dims of a later flush of the held ESC are that call's `current_dims`.
- [ ] AC-4 (FR4): A non-strip continuation in call 2 (`x`, `[H`) is written as ESC followed by the continuation, with no byte lost.
- [ ] AC-5 (FR4): After the overflowing call of AC-1, a cut at fed 0 with an empty range (the reader fallback), or a cut at the start of call 2, writes nothing for the held ESC, leaves pending empty and the state Ground. term_core replaying the ring matches the raw-stream reference that has a 47 / 1047 / 1049 h / l pair in place of the cut.
- [ ] AC-6 (FR3, NFR3): Overflow cases outside FR1 keep their current bytes and state: a run ending in ESC + a complete removed construct (R9 form), a run ending in a charset-designator ESC (`ESC ( ESC`), a run ending in `ESC[6` + removed construct, an overflow followed by a cut in the same call (stripped run + one closure), and runs ending in plain bytes.
- [ ] AC-8 (NFR1, NFR3, NFR5): The full `--lib` test suite passes, including the existing budget tests. `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds.

## Technical Requirements

### Functional Requirements
- **FR1:** Overflow flush ending in a lone ESC: the ESC is classified together with the next read. When `ScrollbackWriteFilter::feed_with_cuts` takes the overflow flush in the last segment of a call (no cut follows it in that call) and the flushed run's last byte is a live lone ESC (an ESC that the strip writes and that leaves the written stream in `WrittenState::Escape`, not a charset designator byte after `ESC (` / `ESC )`), that ESC is not written in that call. It is held so the next call strips it together with the bytes that follow it. A strip target whose opening ESC is that byte and whose remainder arrives in later calls (CSI device queries such as `ESC[6n` / `ESC[5n` / `ESC[c`, OSC 777 viewer launch, OSC 9999 emterm-md, OSC 777 agent-status report, Kitty APC, SIXEL DCS) is removed from the ring, exactly as when the same bytes arrive without an overflow.
- **FR2:** Memory bound after the overflow flush. After an overflow flush, pending holds at most that one ESC byte (FR1). In every other case it stays empty as it is today. The pending cap keeps bounding memory.
- **FR3:** Rest of the overflow flush unchanged. Every byte of the flushed run before the held ESC still goes through the strip and is written in that call. The carried written state is the end state of those written bytes (started from the state carried in). The awaiting-designator flag is the client-parity state at the end of the run (false for a run ending in a lone ESC). The call reports no carried completion, as today.
- **FR4:** Behavior of the held ESC in later calls. The held ESC behaves as a lone trailing ESC held by the non-overflow path: a non-strip continuation (for example plain text or `ESC[H`) is written together with the ESC, with nothing dropped. A cut, or the reader's fallback closing (an empty fed range with a cut at 0), drops the held ESC unwritten (round3 FR1) and writes the one closure that the written state before it decides (`closure_for`). A held ESC whose continuation completes an OSC / DCS / APC string is reported as a carried completion under the existing rules.
- **FR5:** Dims attribution of the held ESC. The held ESC is attributed to the dims of the read that produced it (`pending_started_dims` set to that call's `current_dims`, per the existing D7''' rule for a tail left by a call that drained bytes).
- **FR6:** Regression tests. Tests reproduce the reported scenario: an OSC held at the cap is continued past the cap by a call ending in a lone ESC, and a later call carries `[6n`. They confirm that the ring holds no executable `ESC[6n`, and that term_core replaying the ring gives no cursor-position report. They cover each strip-target continuation and a non-strip continuation, and include a production-reader-level case.

### Non-Functional Requirements
- **NFR1 - Performance:** No additional pass over the fed bytes. The extra work on the overflow path is O(1) (inspecting the run's last byte and the strip's reported end state). The existing budget tests (BUDGET, 10 s) still pass.
- **NFR2 - Output independence from read splitting:** For the same byte stream, the bytes written across calls equal those written when the final ESC and its continuation arrive in one call that does not overflow.
- **NFR3 - Unchanged elsewhere:** Bytes forwarded live to the connected client, the snapshot-time strip (`strip_replayable_rich_content`), the non-overflow path, and the strip functions in `scrollback_filter.rs` keep their current behavior. Existing tests pass without changing their expectations, in particular `escape_carry_an_overflow_flush_ending_in_a_written_escape_carries_or_closes_it` (R9), `post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi`, the round3 overflow designator test, `an_osc_held_near_the_cap_is_dropped_at_a_cut_and_at_the_fallback`, and `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`.
- **NFR4 - Documentation:** Doc comments that state the overflow postcondition ("pending is always empty right after the overflow flush") are updated to the new postcondition (empty, or exactly the one held lone ESC).
- **NFR5 - Platform / build:** Builds on Linux and Windows. The `--no-default-features` (CLI-only) check still compiles, because the mux code is shared by the CLI build.

## Implementation Approach

### Architecture

**System Architecture:**
```
PTY read (pane reader)
        |
        v
ScrollbackWriteFilter::feed_with_cuts   (write_filter.rs)
        |   overflow branch: pending > SCROLLBACK_FILTER_PENDING_CAP
        v
strip_pass                              (scrollback_filter.rs, unchanged)
        |
        v
scrollback ring
```

**Component Diagram:**
```
write_filter.rs
  ScrollbackWriteFilter
    feed_with_cuts        -- overflow branch: change site (FR1-FR3, FR5)
    pending / pending_len / written_state / awaiting_designator
scrollback_filter.rs
  strip_pass / scan_csi_device_query   -- unchanged (NFR3)
```

### Data Flow

```
Call N (overflow, last segment, run ends in live lone ESC):
  run[..len-1] -> strip -> written to ring
  run[len-1] (ESC) -> held in pending (1 byte)

Call N+1:
  pending ESC + fed bytes -> strip -> written to ring
    strip target  -> removed
    non-strip     -> ESC + continuation written
  cut / fallback  -> held ESC dropped, one closure_for closure written
```

Implementation direction (assumption A2): keep that final ESC in pending (one byte, `held_construct_start` `Some(0)`), mirroring the non-overflow lone-ESC hold. The plan may choose an equivalent mechanism provided FR1-FR5 hold.

### API Design

Not applicable. No public interface changes are part of the requirements; the observable accessors (`pending()`, `pending_len()`, `written_state()`, `awaiting_designator()`, `outcome.carried`) keep their existing meaning, with the overflow postcondition widened per FR2 / NFR4.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/scrollback_filter.rs` (`strip_pass`, `scan_csi_device_query`, `strip_replayable_rich_content`): used as-is; behavior unchanged (NFR3).
- `run_suppressed_pipeline` / `prepare_suppressed_replacement`: `run_suppressed_pipeline` passes `scrollback_filter.pending()` to `prepare_suppressed_replacement`; after this change the held ESC is seen there exactly as a non-overflow held lone ESC is seen today, with no separate handling (assumption A6).
- term_core: used by tests to replay the ring and check for cursor-position reports.

**External Dependencies:**
- None.

### File Structure

```
src-tauri/src/mux/
├── ipc/pty_spawn/
│   └── write_filter.rs      # ScrollbackWriteFilter::feed_with_cuts overflow branch, SCROLLBACK_FILTER_PENDING_CAP
└── scrollback_filter.rs     # strip_pass / scan_csi_device_query (unchanged)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-write-filter-overflow-lone-esc/**`
- `test-docs/mux-write-filter-overflow-lone-esc/**`

`feature-docs/mux-write-filter-overflow-lone-esc/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/mux-write-filter-overflow-lone-esc/**` covers `test-docs/mux-write-filter-overflow-lone-esc/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/mux-write-filter-overflow-lone-esc/` directory at all; the declared
`test-docs/mux-write-filter-overflow-lone-esc/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1; FR1, FR6): Filter unit test: hold an OSC at the cap, overflow with a continuation ending in a lone ESC, then feed `[6n`. Assert no `ESC[6n` in the concatenated output and no term_core response on replay.
- [ ] TS-2 (AC-2; FR1, NFR2): Loop over all strip targets (`post_strip_cut_csi::all_targets` plus the CSI query forms) as continuations after the overflow-ending ESC. Assert removal and equality with the non-overflow feeding.
- [ ] TS-3 (AC-3, AC-4; FR2, FR3, FR4, FR5): Assert pending / written_state / awaiting_designator / carried after the overflowing call. Assert that non-strip continuations keep ESC + bytes intact.
- [ ] TS-4 (AC-5; FR4): Fallback closing and a cut after the overflow-held ESC, compared against term_core fed the raw stream with a screen-switch pair in place of the cut (`view_after_a_cut` oracle).
- [ ] TS-5 (AC-6; FR3, NFR3): Overflow-path regression matrix for runs not ending in a live lone ESC. Expect bytes and state unchanged. The existing R9, post_strip R4 and round3 overflow tests serve as the baseline.

### Integration Tests
- [ ] TS-6 (AC-7; FR1, FR6): Production reader test (`run_reader_without_owner` or the visibility-restore helpers in `round3_write_path`): ring content contains no `ESC[6n`.
- [ ] TS-7 (AC-8; NFR1, NFR3, NFR5): Run the full `--lib` suite and the CLI-only cargo check.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression (none detected)

### Edge Cases
- [ ] Non-strip continuation after the held ESC (`x`, `[H`): ESC + continuation written, no byte lost (AC-4, FR4).
- [ ] Cut or reader fallback after the held ESC: held ESC dropped unwritten, one `closure_for` closure written, pending empty, state Ground (AC-5, FR4).
- [ ] Continuation of the held ESC that completes an OSC / DCS / APC string: reported as a carried completion under the existing rules (FR4).
- [ ] Run ending in ESC + a complete removed construct (R4 / R9 form): out of scope, current bytes and state kept (AC-6, assumption A1).
- [ ] Run ending in a charset-designator ESC (`ESC ( ESC`): not a live lone ESC, current bytes and state kept (AC-6, FR1).
- [ ] CSI query split across calls (EC-7 of mux-strip-escape-state-carry): out of scope, behavior kept as pinned by existing tests (assumption A1).
- [ ] Overflow followed by a cut in the same call: stripped run plus one closure from `closure_for` (the Escape closure for a run ending in a written ESC) (AC-6, assumption A4).

### Performance Tests
- [ ] Budget: the existing budget tests (BUDGET, 10 s), including `escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget`, still pass (NFR1, NFR3).

## Security Considerations

- **Authentication:** Not applicable.
- **Authorization:** Not applicable.
- **Input Validation:** PTY output reaching the scrollback ring on the overflow flush path does not form a query the client did not initiate (FR1). The snapshot-time strip already removes a contiguous `ESC[6n` from the ring on reattach and stays as defense in depth (assumption A5).
- **Data Protection:** Not applicable.
- **XSS Prevention:** Not applicable.
- **SQL Injection Prevention:** Not applicable.
- **CSRF Protection:** Not applicable.

## Error Handling

### Error Codes

Not applicable.

### Error Flow

Not applicable.

## Performance Optimization

### Performance Goals
- No additional pass over the fed bytes; the extra work on the overflow path is O(1) (NFR1).
- After an overflow flush, pending holds at most one byte (FR2).

### Optimization Strategies
- Inspect only the flushed run's last byte and the strip's reported end state (NFR1).

### Caching Strategy
- Not applicable.

## Assumptions

- **A1:** Scope is the overflow flush in the last segment of a call whose flushed run ends in a live lone ESC (written state Escape at that final byte). Runs whose written stream ends in Escape because a written ESC is followed by a construct the strip removes (the R4 / R9 form) are not part of this fix. The same holds for a CSI query split across calls (EC-7 of mux-strip-escape-state-carry). Both remain as pinned by existing tests.
- **A2:** Implementation direction: keep that final ESC in pending (one byte, `held_construct_start` `Some(0)`), mirroring the non-overflow lone-ESC hold. The plan may choose an equivalent mechanism provided FR1-FR5 hold.
- **A3:** The task description's root-cause line reference (`scrollback_filter.rs:121`, "Escape is not reported as a CSI state") predates mux-strip-escape-state-carry. The filter now carries `WrittenState::Escape` after such a flush; R9 pins this. In current code the remaining cause is different: the next call's `strip_pass` sees `[6n` without the already-written ESC, so `scan_csi_device_query` never runs, and the bytes are written verbatim, forming `ESC[6n` in the ring. This was confirmed by reading `write_filter.rs` `feed_with_cuts` (overflow branch, lines 485-522) and `scrollback_filter.rs` `strip_pass` (lines 393-397, 433-444).
- **A4:** An overflow followed by a cut in the same call keeps its current output: the stripped run plus one closure from `closure_for` (the Escape closure for a run ending in a written ESC).
- **A5:** The snapshot-time strip already removes a contiguous `ESC[6n` from the ring on reattach. This feature fixes the write path, and the snapshot strip stays as defense in depth.
- **A6:** `run_suppressed_pipeline` passes `scrollback_filter.pending()` to `prepare_suppressed_replacement`. After this change, the held ESC is seen there exactly as a non-overflow held lone ESC is seen today. No separate handling is required.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] The full `--lib` test suite and the `--no-default-features` cargo check pass (AC-8)

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None. Every requirement is resolved.

## References

- Requirements document: `feature-docs/mux-write-filter-overflow-lone-esc/REQUIREMENTS.md`
- `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`: `ScrollbackWriteFilter::feed_with_cuts` (overflow branch), `SCROLLBACK_FILTER_PENDING_CAP`
- `src-tauri/src/mux/scrollback_filter.rs`: `strip_pass`, `scan_csi_device_query`, `strip_replayable_rich_content`
