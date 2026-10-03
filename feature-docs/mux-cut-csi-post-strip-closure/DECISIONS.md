# mux-cut-csi-post-strip-closure: Decisions (DECISIONS.md)

Decisions for the medium architecture finding `4c0ad9058a983648`, left unresolved in review round 1 of the predecessor feature mux-suppressed-output-round4-fixes, and the three cut-free strip concatenation residuals (SPEC FR6).

## Decision table

| stable_id | requirement | verdict | rationale | regression test |
|-----------|-------------|---------|-----------|-----------------|
| 4c0ad9058a983648 | FR1, FR2, FR3 | resolved | Cause: the cut closing (one DEL when the written stream ends inside a CSI) used the boundary scan's pre-strip CSI state, at the cut and for the carry to the next call. The boundary scan clears that state at an `ESC` it steps past, but the strip removes some constructs together with that `ESC`: the OSC 777 viewer launch, OSC 9999 emterm-md, agent-status reports, Kitty APC, SIXEL DCS and answered CSI device queries such as `ESC[6n`. With `ESC[6`, such a construct and a cut, the ring ended in `ESC[6` with no DEL, and a later `n` completed a cursor-position query on replay. The cut-free path carried the same state, so a later fallback closing wrote nothing either; the overflow flush had the same gap. Fix (IMPLEMENTATION.md D1): the CSI state of the strip-applied output, obtained inside the strip's existing pass, decides the closing on the in-call cut, on the empty-segment and fallback closing, and on the overflow flush, and it is the state carried after a cut-free call. The shared strip's output is unchanged (IMPLEMENTATION.md D2). | `mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi`<br>`mux::ipc::pty_spawn::tests::round4_cut_csi::post_strip_a_cut_after_a_stripped_construct_replays_like_the_raw_stream`<br>`mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output`<br>`mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi`<br>`mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_reader_restore_matches_the_raw_stream_reference`<br>`mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_closing_follows_the_stripped_output`<br>`mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` |

## Residuals (FR6)

Three cut-free strip concatenation cases remain. The shared strip's output is unchanged (NFR1, IMPLEMENTATION.md D2), so they are recorded here and a separate task handles them.

### Residual 1: a launch between an open CSI and its final byte, in one call

Occurrence condition: `ESC[6`, a complete viewer launch and `n` are fed in one cut-free call; the strip removes the launch together with the `ESC` that aborted the CSI, and the bytes are written to the ring as `ESC[6n`.

Out of scope because: the fix is limited to the cut and the carried state (SPEC as-04, IMPLEMENTATION.md D2); this case comes from the shared strip joining the bytes around a removed construct without a cut, and changing the shared strip's output changes the snapshot path, which NFR1 excludes; a separate task handles it.

### Residual 2: a snapshot taken while the ring ends in an open CSI

Occurrence condition: the ring ends in an `ESC[6` whose following construct the strip removed, a snapshot is taken in that state with no cut to close it, and the next live `n` can complete a cursor-position query.

Out of scope because: the fix is limited to the cut and the carried state (SPEC as-04, IMPLEMENTATION.md D2); the closing is written at a cut, and this case has no cut; this case comes from the shared strip joining the bytes around a removed construct, and changing the shared strip's output changes the snapshot path, which NFR1 excludes; a separate task handles it.

### Residual 3: a CSI device query written across calls

Occurrence condition: a CSI device query such as `ESC[6n` arrives split across calls; the write filter holds no CSI byte, so each call's bytes reach the ring as written, and only the strip applied at snapshot time removes the query.

Out of scope because: the fix is limited to the cut and the carried state (SPEC as-04, IMPLEMENTATION.md D2); this case is on the cut-free path, where CSI bytes are written as they arrive and never held, and changing what the shared strip emits changes the snapshot path, which NFR1 excludes; a separate task handles it.

## Behavior-changing tests

None.

No existing test's expectation changed: `round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi` gained three table rows, with a row-specific byte-by-byte expectation for the CSI-query row, and no existing row changed.

No predecessor test-docs record was updated.

## Predecessor records

`feature-docs/mux-suppressed-output-round4-fixes/reviews/round1.yaml` and `feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md` are not modified by this feature.
