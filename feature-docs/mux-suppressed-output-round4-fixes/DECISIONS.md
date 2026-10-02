# mux-suppressed-output-round4-fixes: Decisions (DECISIONS.md)

Decisions for the three medium findings left unresolved in review round 1 of mux-suppressed-output-round3-fixes (PR #112), and the outcome of the two adjacent paths this feature checked.

## Decision table

| stable_id | requirement | verdict | rationale | regression test |
|-----------|-------------|---------|-----------|-----------------|
| 3e2024dce619ed9f | FR1 | pending (task0001) | pending (task0001) | pending (task0001) |
| 989ec5c588abce06 | FR2 | pending (task0003) | pending (task0003) | pending (task0003) |
| 48caec6f5b0b5810 | FR3 | pending (task0002) | pending (task0002) | pending (task0002) |

## Adjacent paths

### FR4: in-progress CSI at a cut

Outcome: reproduced and fixed.

Regression test: `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs`: `mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_in_progress_csi_at_a_cut_matches_the_raw_stream_reference`

On the pre-change code the test failed. With `ESC[6`, a removed 47/1047/1049 `h`/`l` pair and `n` in a later read, through the production visibility restore, the client answered a cursor-position report (`ESC[1;1R`) that the raw-stream reference does not produce. The write filter stepped over `ESC [` as a complete escape and wrote the CSI's bytes to the ring, while the live parser aborted that CSI at the removed switch's ESC, so a later `n` completed it on replay.

Fix, in `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`: the boundary scan follows term_core's CSI transitions (entry and parameter states, C0 controls that execute inside the CSI, bytes that cancel it, an ESC that aborts it) inside its existing pass. The filter carries the O(1) CSI sub-state across feeds and holds no CSI byte in `pending`. At a cut, when the emitted stream ends inside a CSI, it writes one DEL (0x7f) after the emitted bytes and clears the state. This covers a cut inside a call, a cut at an empty segment (including the reader's fallback closing), and an overflow flush followed by a cut. term_core cancels a CSI on DEL without dispatching it and returns to ground, and neither strip treats DEL as the start of a strip target.

The closing is also written when the construct a cut drops starts right after the CSI's bytes (`ESC[6`, `ESC]0;t`, then the switch): that construct's ESC would have aborted the CSI, but it is not written. IMPLEMENTATION.md D2 states that the CSI closing is not written when the cut dropped a chain; the filter instead carries the CSI state while `pending` is non-empty.

Residual: none at a cut. Outside a cut, the strip removes a complete viewer launch that follows an open CSI together with the ESC that aborted it, so `ESC[6`, a launch and `n` fed in one call are written as `ESC[6n`. The strip is outside this feature's change set (NFR1) and is unchanged.

### FR5: switch sequence in the designator slot

Outcome: reproduced and fixed. One residual remains outside the fix boundary (below).

Regression test: `src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_slot.rs`: `mux::ipc::pty_spawn::tests::round4_designator_slot::round4_fr5_switch_in_the_designator_slot_matches_the_raw_stream_reference`

On the pre-change code the test failed. With `ESC (` directly followed by `ESC[?47l` (`pre`, `ESC (`, `ESC[?47l`, `TEXT` in one read, `later` in the next), the ring held `pre ESC ( ESC TEXT later` where the raw stream holds `pre ESC ( ESC [?47l TEXT later`: the sequence's `[?47l` bytes were removed as a screen switch although term_core consumes the ESC as the charset designator and prints them as text. An exploratory probe on the same code showed the client's rows and cursor differing from the raw-stream reference at the restore points, and in one layout (a color query after the sequence) the responses too.

Fix, in `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` and `src-tauri/src/mux/ipc/pty_spawn/mod.rs`: main-span extraction (`extract_main_buffer`, formerly `extract_main_buffer_bytes`) follows the charset-designator slot inside its existing pass. An ESC directly after `ESC (` / `ESC )` is the designator, so the sequence that starts there is not a switch; extraction keeps it as text. The reader carries a `DesignatorSlot` (ground, ESC seen, awaiting a designator) from one read to the next, so an `ESC (` that ends a read and the ESC that opens the next are handled as within one read. The state is reader-local and O(1); no lock or blocking wait is added and the lock order is unchanged. The ring write, the cuts derived from the span list and the live spans (the OSC 133 gating) all follow the corrected span list. Extraction also returns `shadow_alt`, the state a parser that aborts on ESC reaches. The reader's cross-check compares that with the shadow parser's state: comparing term_core's final state would make the two disagree on a designator-slot `h` form and send the read to the fallback, which drops it from the ring.

Alternate-screen state per case, term_core against the vt100 shadow parser:

| sequence in the slot | term_core | vt100 shadow parser |
|----------------------|-----------|---------------------|
| `?47h`, `?1049h` | text, stays on the main screen | aborts on the ESC and enters the alternate screen |
| `?47l`, `?1049l` | text, stays on the main screen | no change (already on the main screen) |
| `?1047h`, `?1047l` | text, stays on the main screen | no action (an existing unrecognized form) |

Residual: an `h` form of 47 or 1049 in the designator slot that nothing closes. The shadow parser is in the alternate screen while term_core is on the main screen. The snapshot assembly then lays the pane out as an alternate-screen pane, so the client's rows and cursor differ from the raw-stream reference. The next read's extraction starts in the alternate screen (`alt_before` comes from the shadow parser), so that read's text is not written to the ring: for `pre ESC ( ESC[?47h TEXT` followed by `later`, the ring holds 15 of the 20 raw bytes and `later` is missing. Responses equal the reference's in these cases. The cause is the shadow parser's own state and the snapshot assembly's alternate-screen layout, both outside the fix boundary (D4, NFR1); neither is changed. The task reports a plan deviation. For `h` forms that nothing closes, the registry test compares the ring (one read each) replayed through term_core with the reference instead of a snapshot. An `h` form closed by a switch that is one leaves the shadow parser on the main screen at the end of that read, and the registry test compares the whole client view there; a restore taken between the `h` form and its closing read is not compared (the shadow parser is in the alternate screen then).

## Behavior-changing tests

### FR1

pending (task0001)

### FR2

pending (task0003)

### FR3

pending (task0002)

### FR4

| old name | new name | file | reason |
|----------|----------|------|--------|
| `fallback_path_keeps_the_whole_chunk_gate` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR4: the earlier read leaves the ring inside the CSI `ESC[?10`, and the fallback's cut closes it with one DEL, so the expected ring is `a\x1b[?10` followed by that DEL |

Predecessor `test-docs/` records updated: None (the test keeps its name).

### FR5

| old name | new name | file | reason |
|----------|----------|------|--------|
| `round4_48caec6f_awaiting_designator_at_a_cut_writes_the_consumed_esc` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_cut.rs` | FR5: in the task example the ESC of the `enter` form directly after the waiting `ESC (` is the designator, so only the `leave` form is removed and the ring keeps the designator ESC and the text of `enter`; the expected ring is `X ESC ( <enter> ESC ( ESC ]11;?` instead of `X ESC ( ESC ESC ( ESC ]11;?` |
| `round4_many_straddling_switches_after_a_waiting_designator_stay_within_the_budget` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_cut.rs` | FR5: in the recognized form the first ESC of the read after `x ESC (` is the designator, so the ring keeps it and `[?1049h` and removes only the `leave` form; the expected ring per round is `x ESC ( ESC [?1049h` instead of `x ESC ( ESC` |

Predecessor `test-docs/` records updated: None (both tests keep their names).

## Review round 1 record

`feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is not modified by this feature.
