# mux-strip-open-string-body-closure: Decisions (DECISIONS.md)

Records for task0001 (FR9): the closure-byte decision, the before and after value of every expectation changed under FR8, the rename with the test-docs record update, and the resolution of Residual 1 of mux-strip-join-escape-closure.

Notation: ESC = 0x1B, CAN = 0x18, DEL = 0x7F, BEL = 0x07, CR = 0x0D, ST = ESC followed by a backslash. Spaces inside a byte string are separators only. "c0" is the C0 bytes a removed query re-emits (empty for every construct but the CSI query with an embedded CR, whose c0 is CR). "D1" is `Written::close_before_removal` in `src-tauri/src/mux/scrollback_filter.rs`. All test paths below are relative to the crate (`mux::scrollback_filter::tests::<test>` or `mux::ipc::pty_spawn::tests::<module>::<test>`).

## Decisions

### D1: The closure byte (FR1, FR2)

A construct the strip removes together with its opening ESC while the written stream is inside an open OSC body or an open DCS / APC body is replaced, at its position, by `STRING_BODY_CLOSING`: ESC then CAN, written through the per-byte writer (`Written::extend`), so the reported state ends in Ground. The closure is written before the re-emitted C0 bytes (a re-emitted BEL would end an open OSC body before it is closed). The states that were already closed keep their closure: DEL for a CSI and for a written lone ESC; nothing for Ground and for a pending charset designator.

- **Why ESC then CAN.** It is the closure the write filter already writes at a cut in the same states (mux-write-filter-overflow-open-string-cut D2). `term_core` takes the ESC into the string's escape state and the CAN as the byte that aborts the string; the escape completes and the parser returns to ground without a character, a cursor move or a response. The client's parser did the same at the removed construct's ESC, so a replay of the ring stands in ground and a later `?` BEL is never absorbed into the string.
- **One definition.** `STRING_BODY_CLOSING` is defined once, in `scrollback_filter.rs`, with the visibility of `CSI_CLOSING` (`pub(in crate::mux)`), and has no dependency on the IPC layer. `write_filter.rs` drops its definition and re-exports the shared constant under the same name, so `closure_for` and every existing test use the name unchanged.
- **One closure per removal run.** The closure leaves the written stream in Ground, so further constructs removed in the same run write nothing, and a cut after the strip adds nothing. The closure is neither a strip target (ESC followed by CAN opens none) nor an ST (ESC followed by CAN is not ESC followed by a backslash), and `vt100_replay_copy` copies it unchanged (it contains no DEL).
- **Unchanged.** `closure_for` (the cut closure), the boundary scan, the overflow branch and `term_core` are unchanged. Remap needs no change: an offset at a removed construct's first byte maps before whatever the removal writes, and the others map after it.

### D2: Test inputs

The new tests (`strip_open_string_body_closure` module and the `open_body_closure_*` tests of `scrollback_filter::tests`) use the eight removed constructs of the existing target tables, the body heads `ESC]0;t`, `ESC]11;`, `ESC Px` and `ESC _Xnot-kitty`, the color heads `ESC]11;`, `ESC]10;`, `ESC]12;` and `ESC]4;1;`, and the continuations `n`, `?` BEL, plain text and ST. Replays go through the themed client for every "no color response" assertion, each with a control showing that the client does answer the closure-less join `ESC]11;?` BEL.

## Changed expectations (FR8)

Every pinned expectation below is a construct removed while the written stream is inside an open body. Each keeps the name of its test unless the table says it was renamed. The rows for Ground and for a completed string keep their no-closure expectation.

### Item 1: the renamed AC-11 test

`mux::ipc::pty_spawn::tests::overflow_open_string_cut::overflow_open_string_cut_a_non_overflow_strip_then_cut_closes_the_open_osc_body` is renamed to `mux::ipc::pty_spawn::tests::overflow_open_string_cut::overflow_open_string_cut_a_non_overflow_strip_closes_the_osc_body_at_the_removal_and_the_cut_adds_nothing`. The old name no longer describes it: the strip now closes the body, and the cut adds nothing.

| Item | Before | After |
| --- | --- | --- |
| Bytes written by call 1 (`ESC]0;x` + `ESC[6n`) | `ESC]0;x` | `ESC]0;x ESC CAN` |
| Written state after call 1 | OscBody | Ground |
| Bytes written by a cut at fed 0 of the next call | `ESC CAN` + text | text only |
| Bytes written by the reader's fallback closing | `ESC CAN` | none |

### Item 2: the test-docs record update

In `test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` the AC-11 entry lists the new name and carries a supersede YAML comment that names `feature-docs/mux-strip-open-string-body-closure/SPEC.md` FR1/FR5; its `red_reason` is unchanged. The other tests that record lists keep their names, so no other entry and no other predecessor record changes (`.claude/rules/test-docs-records.md`). The new name resolves in `cargo test --lib -- --list`.

### Item 3: `scrollback_filter::tests::strip_concat_a_construct_removed_in_ground_adds_no_closing`

The name is kept. Its `ESC]0;t` row left the table (Ground and completed strings keep the no-closure expectation) and is covered by `open_body_closure_every_entry_point_closes_an_open_body_at_every_removed_construct`; the sentence about a kept string body is gone from its doc comment.

| Input | Before | After |
| --- | --- | --- |
| `ESC]0;t` + construct + `n` | `ESC]0;t` + c0 + `n` | `ESC]0;t ESC CAN` + c0 + `n` |

### Item 4: `strip_concat_query::strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing`

The name is kept. Its `body_head` loop (`ESC]0;t`, `ESC_Xnot-kitty`) left the test and is covered by the same new test, with the heads `ESC]0;t`, `ESC]11;`, `ESC Px` and `ESC _Xnot-kitty`; its doc comment now says "completed string" and points to the new module.

| Input | Before | After |
| --- | --- | --- |
| body_head + construct + `n` | body_head + c0 + `n` | body_head + `ESC CAN` + c0 + `n` |

### Item 5: `scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes`

The name is kept. Each row is: start state + input, output / reported state.

| Start state + input | Before | After |
| --- | --- | --- |
| OscBody + `ESC[6 BEL n` | `BEL` / Ground | `ESC CAN BEL` / Ground |
| OscBody + `ESC[6 CR n` | `CR` / OscBody | `ESC CAN CR` / Ground |
| StBody + `ESC[6 BEL n` | `BEL` / StBody | `ESC CAN BEL` / Ground |
| Ground + `ESC]0;t ESC[6 BEL n` | `ESC]0;t BEL` / Ground | `ESC]0;t ESC CAN BEL` / Ground |
| Ground + `ESC]0;t ESC[6n` | `ESC]0;t` / OscBody | `ESC]0;t ESC CAN` / Ground |
| Ground + `ESC Px ESC[6n` | `ESC Px` / StBody | `ESC Px ESC CAN` / Ground |
| Ground + `ESC Px ESC[6n ab` | `ESC Px ab` / StBody | `ESC Px ESC CAN ab` / Ground |
| OscBody or StBody + each construct (loop) | empty / the body state | `ESC CAN` / Ground |
| Ground + `ESC]0;t` + each construct (loop) | `ESC]0;t` / OscBody | `ESC]0;t ESC CAN` / Ground |
| Ground + `ESC Px` + each construct (loop) | `ESC Px` / StBody | `ESC Px ESC CAN` / Ground |

### Item 6: `scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip`

The name is kept. The identity for a carried OscBody or StBody changes.

| Start state | Before | After |
| --- | --- | --- |
| OscBody, StBody (no designator) | the output equals the designator form's output (neither closing is written) | the output equals the Ground-started write-path strip of the bytes entering the body (`ESC ]0;` or `ESC _x`) + input, minus those entering bytes; the same identity every other carried state has |

The doc comment (`From a carried string body neither closing is written`) now states the new identity.

### Other changed tests

Tests outside items 1 to 6 whose expectation changed because a removal inside an open written body now writes ESC CAN: none.

## Residual 1 of mux-strip-join-escape-closure: resolved

Residual 1 of mux-strip-join-escape-closure (the kept-string-body join) is resolved by this feature. `ESC]11;` + a removed construct + `?` BEL, fed in one cut-free call, writes `ESC]11;ESC CAN` + c0 + `?` BEL, not `ESC]11;?` BEL. The replay of that ring stands in ground, the themed client does not answer (the `ESC]11;?` BEL join is answered only by the closure-less control), and the same holds for a live `?` BEL after a resume snapshot that appends nothing after the scrollback. The two expectations that residual pinned are changed in items 3 and 4 above (the `ESC]0;t` row and the `body_head` loop). The predecessor features' DECISIONS.md files and test-docs records, apart from the AC-11 entry of item 2, are not modified by this feature.
