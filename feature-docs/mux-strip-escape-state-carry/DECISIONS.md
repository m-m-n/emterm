# Decisions: mux-strip-escape-state-carry

Decisions for the medium finding `a879a02de382209f`, left unresolved in review round 1 of the predecessor feature mux-cut-csi-post-strip-closure, and the splices that remain outside this fix (SPEC FR6).

## Decision table

| stable_id | requirement | verdict | rationale | regression test |
|-----------|-------------|---------|-----------|-----------------|
| a879a02de382209f | FR1, FR2, FR3, FR4 | resolved | Cause: the stateful strip narrowed the end state of the written bytes to the CSI sub-state, so an Escape end state was carried as ground. The strip writes an `ESC` and then removes a construct that follows it: the OSC 777 viewer launch, OSC 9999 emterm-md, agent-status reports, Kitty APC, SIXEL DCS and answered CSI device queries such as `ESC[6n`. The written stream then ends in Escape, and the next call started from ground. With a first call `ESC` + a removed construct and a second call `[6` + a cut, the ring ended in an unclosed `ESC[6` and a later `n` completed a cursor-position query on replay, while the same bytes in one call got the closure. A cut right after the written `ESC` left the ring ending in a lone `ESC`. The closure at a cut therefore depended on the read split. Fix (IMPLEMENTATION.md D1): the write filter carries the full written end state (Ground, Escape, Designator, Csi(Entry), Csi(Param)) and starts the next call's strip from it; that state decides the closure at the in-call cut, at the empty-segment cut and fallback closing, and at the cut after an overflow flush: DEL for a CSI, the Escape closure for Escape, the designator `ESC` for Designator, nothing for ground. The awaiting-designator flag stays decided by the boundary scan alone, so a carried Designator never changes a removal decision. The Escape closure is CAN (IMPLEMENTATION.md D2). The shared strip's output is unchanged (NFR1). | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output`<br>`mux::scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes`<br>`mux::scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_reproduction_closes_the_csi_and_replays_no_query`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_cut_after_a_written_escape_writes_the_escape_closure`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_replays_like_the_raw_stream`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_has_no_effect_in_term_core`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_splice_made_designator_wait_closes_with_the_designator_esc`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_an_overflow_flush_ending_in_a_written_escape_carries_or_closes_it`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_reader_restore_matches_the_raw_stream_reference`<br>`mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget` |

## Residuals (FR6)

Two splices remain outside this fix. The shared strip's output is unchanged (NFR1), so they are recorded here.

### Residual 1: the string-introducer splice

Occurrence condition: the strip writes an `ESC`, removes a construct that follows it, and the next written byte is `]`, `P` or `_`; the ring then holds `ESC` followed by that byte, which opens an OSC, DCS or APC string the raw stream never opened, and the written state treats the string body as ground (SPEC EC-6).

Out of scope because: SPEC as-04 limits this fix to carrying the end state of the written stream (Ground / Escape / Designator / Csi) and the closure at a cut, and names the string-introducer splice as not handled.

### Residual 2: a query completed through cut-free concatenation

Occurrence condition: the three cut-free concatenation residuals recorded in `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md`, section "Residuals (FR6)", still apply, including after a written Escape: a viewer launch removed between an open CSI and its final byte within one call (Residual 1 there), a snapshot taken while the ring ends in an open CSI (Residual 2 there), and a CSI device query written across calls (Residual 3 there). No cut occurs in these cases, so no closure is written.

Out of scope because: SPEC as-04 limits this fix to the closure at a cut and the carried state, and names the cut-free concatenation as not handled.

## Behavior-changing tests

One existing test changes the meaning of its expectation (IMPLEMENTATION.md D4): `mux::scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes` in `src-tauri/src/mux/scrollback_filter/tests.rs`. Two rows change, both from ground: `ESC[6 ESC` now reports Escape (was none) and `ESC[6 ESC(` now reports Designator (was none). The row `ESC(ESC` keeps ground.

No predecessor test-docs record was updated: no test was renamed.

## Predecessor records

`feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` and `feature-docs/mux-cut-csi-post-strip-closure/reviews/round1.yaml` are not modified by this feature.
