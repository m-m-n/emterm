# mux-strip-concat-query-closure: Decisions (DECISIONS.md)

Records for task0001: the existing tests whose expectation changed (FR8) and the residuals found.

## Behavior-changing tests

Each entry gives the test's full `--lib` path, the old and the new expectation, and the FR that changes it. No test is renamed.

### 1. `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output` (FR1, FR7)

- Old: an open head followed by a strip target, in one cut-free call, writes the head only and keeps the CSI state open, and the fallback closing writes one DEL. Case (b), the target held and completed in a later call, keeps the carried-in CSI open (EC-1).
- New: the call writes the head and one DEL, the state is clear, and the fallback closing writes nothing. Case (b) is inverted (EC-1): the completing call writes the DEL, the state is clear, and the fallback closing writes nothing. Case (c) (split invariance and term_core parity) is unchanged.

### 2. `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi` (FR1, EC-7)

- Old: the stripped run ends in `abc ESC[6`, the cut writes the DEL after it, and a flush without a cut carries the parameter sub-state until the fallback closing writes the DEL.
- New: the stripped run ends in `abc ESC[6 DEL` (the strip's closing in place), the cut writes no second DEL, and a flush without a cut carries no state and the fallback closing writes nothing.

### 3. `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_closing_follows_the_stripped_output` (FR1, FR3)

- Old: EC-3 writes `ESC[6`, the embedded C0 byte, then one DEL, and the byte-by-byte ring holds the fed bytes. EC-6 writes `ESC[6`, the designation and the designator ESC. The state after a cut-free call is the parameter sub-state.
- New: EC-3 writes `ESC[6`, one DEL, then the C0 byte (the closing precedes the re-emitted C0 byte). The byte-by-byte ring holds the fed bytes with the final `n` replaced by DEL. EC-6 writes `ESC[6`, one DEL, the designation and the designator ESC. The state after a cut-free call is clear. EC-2 and EC-4 are unchanged.

### 4. `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` (FR1)

- Old: each unit (`ESC[6` + target) leaves `ESC[6` (the comparison dropped one trailing DEL).
- New: each unit leaves `ESC[6` and one DEL, compared exactly.

### 5. `mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi` (FR1, FR3)

- Old: the CLOSING_CASES rows that follow `ESC[6` with a launch, a Kitty APC or a query keep `ESC[6`, end inside a CSI, and the cut writes the DEL. BYTE_BY_BYTE_EXPECTATIONS for `ESC[6ESC[6n` is `ESC[6ESC[6n`.
- New: those rows keep `ESC[6` and one DEL, end in ground, and the cut writes nothing more (the strip wrote the DEL). BYTE_BY_BYTE_EXPECTATIONS for `ESC[6ESC[6n` is `ESC[6ESC[6` and one DEL.

### 6. `mux::scrollback_filter::tests::strip_bare_esc_in_csi_body_aborts_then_strips_following_query` (FR1)

- Old: `ESC[5 ESC[6n` is stripped to `ESC[5`.
- New: it is stripped to `ESC[5` and one DEL.

### 7. `mux::scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes` (FR1, D3)

- Old: rows with a construct removed inside an open CSI write the open head only and report the open sub-state. The C0 bytes re-emitted from a removed query execute inside the open CSI and leave it open.
- New: those rows write the head and one DEL (before any re-emitted C0 byte) and report no open CSI. The form takes the carried classification and returns the end classification, so the test passes the classification and compares its sub-state (D3).

### 8. `mux::scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip` (FR1, FR3, FR5)

- Old: the state-reporting form's output equals the write-path form's output from every start state: ground, carried entry, carried parameter and designator.
- New: the identity holds from the ground and designator start states. From a carried open CSI the output depends on the carried classification, so it equals the ground-started strip of head and input minus the head's written bytes, for input whose bytes before the first `ESC` hold no final byte. Input that completes the carried CSI is covered by `strip_concat_the_state_form_from_a_carried_head_equals_the_ground_started_strip`.

### 9. `mux::scrollback_filter::tests::post_strip_state_form_is_one_linear_pass` (FR1)

- Old: 100000 units of `ESC[6` + launch + `ESC[7` + Kitty APC give `ESC[6 ESC[7` per unit and end in the parameter sub-state.
- New: they give `ESC[6 DEL ESC[7 DEL` per unit and end with no open CSI.

### Adaptations without an expectation change

- `mux/scrollback_filter/tests.rs`: the helper `carried` builds the carried classification for the state-form tests (D3 signature).
- `mux/ipc/pty_spawn/tests/post_strip_cut_csi.rs`: `client_csi_phase` and `assert_client_equals_reference_except` are `pub(super)` so the new test module reuses them.

## Residuals

### Residual 1: an overflow flush whose run ends in a lone ESC

An overflow flush whose run ends in a lone ESC leaves the written stream in the escape state while the carried classification is none. A `[6n` in the next read then reaches the ring executable. The snapshot-time strip still removes it. The case needs a held run over the pending cap. No FR addresses it.

### Residual 2: `dcs_is_sixel` accepts a DECRQSS request

`dcs_is_sixel` skips parameter and intermediate bytes and decides on the final byte `q`, so `ESC P $ q ... ST` (a DECRQSS request) is removed as a SIXEL. The strip writes the closing at it like at any other removed DCS. The predicate is unchanged (FR5).

## Test scope notes

- AC-3 states "the pending length is 0 after every call". The filter holds a lone trailing `ESC` and `ESC ESC` chains by design (D4), and never a CSI byte. The tests assert that only a lone `ESC` may be held after a call, and that the held length stays 0 for CSI bytes, including a 300000-byte parameter run.

## Predecessor records

`feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` and the predecessor `test-docs` records are not modified by this feature.
