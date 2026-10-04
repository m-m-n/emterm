# mux-strip-join-escape-closure: Decisions (DECISIONS.md)

Records for task0001 (FR4, FR5): how the three join attack scenarios map to mux-strip-concat-query-closure D1, and the residual found. This feature makes no production change.

Notation: ESC = 0x1B, DEL = 0x7F, BEL = 0x07, ST = ESC followed by a backslash. Spaces inside a byte string are separators only. "mux-strip-concat-query-closure D1" is `Written::close_before_removal`: it writes one DEL (`CSI_CLOSING`) before a construct removed while the written stream is inside a CSI or right after a written lone ESC. The decisions below are numbered D1 to D4 within this feature and always name the predecessor in full.

## Decisions

### D1: Scenario mapping (FR4)

Each scenario is defeated by mux-strip-concat-query-closure D1. The write strip writes the bytes below, and the named test pins the scenario. All test paths are `mux::ipc::pty_spawn::tests::<module>::<test>`.

| Scenario | Fed in one cut-free call | Bytes the write strip writes | Pinned by |
| --- | --- | --- | --- |
| 1 | `ESC[6` + removed construct + `n` | `ESC[6 DEL n` (a C0 byte a removed query re-emits follows the DEL) | `strip_concat_query::strip_concat_one_call_closes_an_open_csi_at_every_removed_construct`, the existing DoD-1 test, unchanged |
| 2 | `ESC ESC ESC[6n[6n[5n` | `ESC ESC DEL [6n[5n` | `strip_join_escape_closure::join_closure_two_stage_strip_gives_zero_responses` |
| 3 | `ESC` + removed construct + `c` | `ESC DEL c` | `strip_join_escape_closure::join_closure_a_removal_after_a_lone_esc_neither_resets_nor_switches_g0_nor_answers` |
| 3 | `ESC` + removed construct + `(0` + probe text | `ESC DEL (0` + probe text | the same test |

- **Scenario 2.** The snapshot strip leaves `ESC ESC DEL [6n[5n` unchanged, and term_core shows `[6n[5n` as text, with no response. The test asserts the ring, the unchanged stripped scrollback and the empty response buffer of the replay.
- **Scenario 3.** The test covers the eight removed construct kinds of the DoD-1 test table (`TARGETS`) against both continuations. Each case compares rows, cursor and responses of the replay with the raw stream, with the construct's own effect excluded. It also asserts that the prefix text stays on screen (no reset) and that the continuation is displayed as typed (G0 not switched).
- **Controls.** Three tests in the same module show that the assertions detect the attacks: `join_closure_control_the_replay_core_answers_the_joined_query` (term_core answers `ESC[5n`), `join_closure_control_a_closure_less_join_with_c_resets_the_terminal` (prefix text followed by `ESC c` replays with the prefix gone) and `join_closure_control_a_closure_less_join_with_a_designation_switches_g0` (`ESC(0` followed by the probe text displays line-drawing glyphs).

### D2: Why DEL is inert in term_core (FR4)

- Inside an open CSI, DEL is not a parameter, intermediate, final or C0 byte, so term_core cancels the CSI and returns to ground (`crates/term_core/src/parser/csi.rs`).
- Right after a lone ESC, DEL is an unknown escape final: the parser dispatches it and returns to ground, and the ESC handler ignores it (`parser/escape.rs`, `esc_handler.rs`). It produces no character, cursor move, response, reset or charset change.
- In ground, DEL is ignored (`parser/ground.rs`).

### D3: Superseded mitigation (FR4)

The mitigation the task proposed, CAN after ESC and moving `closure_for` into `scrollback_filter`, is superseded by mux-strip-concat-query-closure D1. `closure_for` stays the cut-only closure: it decides what a cut writes, and the strip writes the DEL at a removed construct.

### D4: Scope

No production code changes. The diff contains test code only, the registration of the new test module in `src-tauri/src/mux/ipc/pty_spawn/tests.rs`, and this file.

## Adaptations without an expectation change

- `src-tauri/src/mux/ipc/pty_spawn/tests/strip_concat_query.rs`: `Target` (struct and fields) and `TARGETS` are `pub(super)` so the new module reuses the removed-construct table of the DoD-1 test.

## Residuals

### Residual 1: the kept-string-body join (out of scope, FR5)

The join inside an unterminated kept OSC, DCS or APC body is not closed. It is out of scope for this feature and no production change is made.

- **Mechanism.** `WrittenState` treats a string body as ground, so a construct removed inside it writes no closing, and the bytes after the construct join the body.
- **Reproduction.** `ESC]11;` + a removed construct (for example `ESC]777;emterm;markdown;x BEL` or `ESC[6n`) + `?BEL`, fed in one cut-free call, writes `ESC]11;?BEL`. The snapshot strip keeps it, and term_core dispatches OSC 11 with `?` and a BEL terminator, which the color responder answers. The raw stream aborts OSC 11 at the construct's ESC (Unterminated, empty data) and gets no answer.
- **Impact.** An OSC 11 response can still be induced. Responses produced during snapshot replay are discarded by the client (`src-tauri/src/tabs/replay.rs`), so the PTY-reply path is a later live continuation: a ring ending in `ESC]11;` + a removed construct leaves the replayed parser inside the OSC body, and a live `?BEL` is answered.
- **Condition.** That path applies only when the main-buffer resume snapshot appends nothing after the scrollback. On ring wrap, `append_wrapped_dump_block_if_applicable` (`src-tauri/src/mux/snapshot_bytes.rs:490`) appends a screen-restore dump block.
- **Pinned expectations, unchanged.**
  - The `ESC]0;t` row of `scrollback_filter::tests::strip_concat_a_construct_removed_in_ground_adds_no_closing`.
  - The `body_head` loop of `strip_concat_query::strip_concat_a_construct_removed_in_ground_or_a_kept_string_adds_no_closing`.

## Predecessor records

The predecessor features' DECISIONS.md files and test-docs records are not modified by this feature.
