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

pending (task0005)

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

pending (task0005)

## Review round 1 record

`feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is not modified by this feature.
