# Implementation Plan: mux-strip-escape-state-carry

## Overview
The mux daemon's scrollback write filter carries the full end state of the bytes it wrote
after the strip (Ground / Escape / Designator / Csi(Entry) / Csi(Param)) and writes the
closure at every cut from that state (task0001); the verdict on review finding
`a879a02de382209f` is recorded in this feature's DECISIONS.md (task0002).

## Technology Stack
- **Language**: Rust, the existing `src-tauri` crate, `mux` module tree (built with and
  without the `gui` feature; nothing here is `gui`-gated).
- **Test oracle**: term_core's `TerminalCore`, already used by the mux tests.
- **New dependencies**: none. No license to record; `project.license` (MIT) is unaffected.

## Layer Structure
- **Shared strip** (`mux::scrollback_filter`): owns the single strip pass and the
  written-state transitions (term_core's, byte by byte). It never depends on the IPC layer.
- **Write filter** (`mux::ipc::pty_spawn::write_filter`): owns `pending` and the held chain,
  the boundary scan and its awaiting-designator flag, the carried written state and the
  closure at cuts. It depends on the shared strip, never the reverse.
- **Tests**: `mux::scrollback_filter::tests` and the `mux::ipc::pty_spawn::tests::*` modules.
- **Records**: `feature-docs/mux-strip-escape-state-carry/DECISIONS.md`.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Regression test identifiers (table below) | Name every regression test of this feature once, so the record can cite them | task0001 defines each test at exactly the module path and name listed; existing tests are extended in place and keep their names. task0002 cites the fully qualified names verbatim in DECISIONS.md and pins them in its record test. Neither task renames an existing test. | task0001, task0002 |
| Behavior-changing test list (D4) | The only existing test expectations whose meaning changes | task0001 changes the meaning of no existing expectation outside D4; task0002 lists exactly D4 in DECISIONS.md. | task0001, task0002 |
| Escape closure (D2) | The byte written at a cut where the written stream ends in Escape | A constant next to `CSI_CLOSING` in the write filter, value CAN (0x18); `CSI_CLOSING` is unchanged. task0001 implements it; task0002 cites D2 in the record's rationale. | task0001, task0002 |
| Test module registry entries (D6) | Declare the two new test modules in `src-tauri/src/mux/ipc/pty_spawn/tests.rs` without a merge conflict | Each task adds one module declaration (with a one-line comment) at its own anchor; neither touches the other's anchor or any other line. | task0001, task0002 |

### Regression test identifiers

Fully qualified as the libtest listing prints them (crate-relative module path).

| ID | Test | Kind | Covers |
|----|------|------|--------|
| R1 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output` | existing, extended | TS-2 |
| R2 | `mux::scrollback_filter::tests::post_strip_state_form_reports_the_csi_state_of_the_written_bytes` | existing, extended | TS-6 |
| R3 | `mux::scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip` | existing, extended | TS-6 (output identity), NFR1 |
| R4 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_reproduction_closes_the_csi_and_replays_no_query` | new | TS-1 |
| R5 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_cut_after_a_written_escape_writes_the_escape_closure` | new | TS-3 (filter level), EC-1, EC-2, EC-3, EC-8 |
| R6 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_replays_like_the_raw_stream` | new | TS-3 (raw-stream comparison), EC-9 |
| R7 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_the_escape_closure_has_no_effect_in_term_core` | new | TS-3 (FR4 properties) |
| R8 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_a_splice_made_designator_wait_closes_with_the_designator_esc` | new | TS-4, EC-4, EC-5 |
| R9 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_an_overflow_flush_ending_in_a_written_escape_carries_or_closes_it` | new | TS-5 |
| R10 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_reader_restore_matches_the_raw_stream_reference` | new | TS-7 |
| R11 | `mux::ipc::pty_spawn::tests::escape_state_carry::escape_carry_alternating_written_escapes_and_strip_targets_finish_within_the_budget` | new | TS-8 |
| R12 | `mux::ipc::pty_spawn::tests::escape_state_carry_record::escape_carry_the_decision_record_states_the_verdict_and_the_residuals` | new (task0002) | TS-9 |

R1-R11 belong to task0001 and R12 to task0002. DECISIONS.md cites R1-R11 as the regression
tests for `a879a02de382209f`.

## Conventions
- New test functions in the new module start with `escape_carry_`. Existing tests are
  extended in place and keep their names, so no record under
  `test-docs/mux-cut-csi-post-strip-closure/` needs an update
  (`.claude/rules/test-docs-records.md`).
- No task modifies `feature-docs/mux-cut-csi-post-strip-closure/` (its DECISIONS.md and
  `reviews/round1.yaml`) or `test-docs/mux-cut-csi-post-strip-closure/`.
- cargo runs from the project root with `CARGO_TARGET_DIR=src-tauri/target` and
  `--manifest-path src-tauri/Cargo.toml` (`.claude/rules/core-build-location.md`).
- Code, comments, tests and DECISIONS.md are written in English.
- Raw-stream comparisons follow the predecessor's oracle convention: the reference is
  term_core fed the raw stream with a 47 / 1047 / 1049 `h` / `l` pair in place of the cut,
  and the removed construct's own effects (the answer to `ESC[6n`, Kitty responses and
  placements) are kept out of the comparison (EC-9).

## Cross-task Design Decisions

### D1: The carried state is the full written end state
- The write filter carries the end state of the bytes it WROTE after the strip as one of
  Ground / Escape / Designator / Csi(Entry) / Csi(Param), instead of the CSI sub-state
  alone. The stateful strip takes this state and returns it; it is advanced only by written
  bytes, inside the existing single strip pass (O(1) state, NFR2).
- The awaiting-designator flag stays separate and is still decided by the boundary scan
  alone. It controls only whether the next run's first byte is copied verbatim (FR2); a
  carried Designator affects state transitions only and never a removal decision, so the
  strip's output for the same bytes does not depend on where reads are split.
- At every cut (the in-call cut, the empty-segment cut, the reader's fallback closing with
  an empty range and a cut at fed 0, and a cut after an overflow flush) the filter writes at
  most one closure, chosen from the written end state at the cut: Csi(Entry) / Csi(Param)
  write `CSI_CLOSING` (DEL), Escape writes the Escape closure (D2), Designator writes the
  designator ESC (0x1B), Ground writes nothing. After the cut the state is Ground and the
  awaiting flag is clear (FR3).
- Affected tasks: task0001 implements it; task0002 states it as the fix in the record.

### D2: The Escape closure is CAN (0x18)
FR4 requires a byte that, right after a written ESC, (a) makes term_core complete the escape
and return to ground, (b) displays nothing, moves no cursor, provokes no response and changes
no mode or charset, (c) is not ESC, (d) does not form ST with that ESC, and (e) is written
alone at its cut. Selection:
- Every byte other than `[`, `]`, `_`, `P`, `(`, `)` and ESC completes the escape and returns
  to ground in the two in-tree models of term_core's escape transitions (the shared strip's
  written-state transition and the snapshot tail decider's escape step). DEC-compatible
  parsers also cancel an escape in progress on CAN without displaying anything.
- Rejected: printable finals 0x30-0x7E (they dispatch escape functions such as RIS, cursor
  save / restore, keypad modes, single shifts, DECID with a response, index / reverse index,
  and `\` forms ST); DEL (EC-1 requires the Escape closure to differ from `CSI_CLOSING`, and
  DEC-compatible parsers ignore DEL inside an escape); the other C0 controls (BEL rings,
  BS / HT / LF / VT / FF / CR move the cursor, SO / SI shift the charset, ENQ answers, SUB
  shows a substitution character on some terminals, NUL does not end an escape in
  DEC-compatible parsers); 0x80-0xFF (C1 / UTF-8 lead bytes).
- CAN is confirmed against term_core by task0001 AC-6 (R7). `crates/term_core/src/parser/escape.rs`
  was not among this planning pass's inputs; if R7 shows term_core does not satisfy (a) or
  (b) for CAN, task0001 stops and reports a plan deviation with the oracle evidence instead
  of choosing another byte.

### D3: Task split
- The carried field, every strip call site and the closure decision in the write filter all
  depend on the stateful strip's new contract, and the tests of both modules depend on both.
  Tasks run fully in parallel in isolated worktrees, so splitting the fix would leave one
  task unable to compile or pass its own tests. The fix and its code-level regression tests
  (FR1-FR5) are therefore one task, task0001.
- The decision record (FR6) has no compile dependency on the fix and is task0002. Its only
  couplings are the regression test identifiers, D2 and D4, all pinned here.

### D4: Behavior-changing tests
- Re-typing an expectation from the CSI sub-state to the full written state (none becomes
  Ground, a CSI phase becomes Csi of that phase) is not a change of meaning.
- The only existing expectations whose meaning changes are two rows of R2
  (`src-tauri/src/mux/scrollback_filter/tests.rs`): from ground, `ESC[6 ESC` now reports
  Escape (was none) and `ESC[6 ESC(` now reports Designator (was none) (SPEC as-06). The
  row `ESC(ESC` keeps ground.
- `csi_phase()` keeps its meaning (SPEC as-05), so its expectations in `round4_chain.rs`,
  `round4_cut_csi.rs` and `post_strip_cut_csi.rs` do not change.

### D5: Name resolution of cited tests
The record test (R12) checks that DECISIONS.md cites R1-R11 verbatim, not that they exist in
the source (task0001 creates them in parallel). The verify phase checks that every R1-R12
name appears as passed in the output of the project's `--lib` test command.

### D6: Test module registry anchors in `src-tauri/src/mux/ipc/pty_spawn/tests.rs`
- task0001 appends the declaration of the module `escape_state_carry` (with a one-line
  comment) after the file's current last line, the declaration of `post_strip_cut_csi`.
- task0002 inserts the declaration of the module `escape_state_carry_record` (with a
  one-line comment) directly after the declaration of `post_strip_cut_csi_record`, before
  the blank line that follows it.
- The two insertions are separated by unchanged lines, so the merge of both tasks is clean;
  on a conflict, the parent-side adoption protocol keeps both declarations.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| term_core does not treat CAN after ESC as FR4 requires (its escape transitions were not read at plan time) | Low | High | R7 checks (a)-(e) against term_core before the closure is relied on; a refutation is reported as a plan deviation (D2) |
| An existing test outside D4 changes meaning (a splice case not found at plan time) | Low | Medium | task0001 AC-9 requires the suite to pass otherwise unchanged; any further change is reported as a plan deviation so DECISIONS.md can be updated |
| A carried Designator wrongly sets the verbatim copy, making strip output depend on the read split | Medium | High | The verbatim flag and the start state are separate inputs (D1); R1, R2 and R8 compare split and single-call output |
| Merge conflict in the test module registry | Medium | Low | Separate anchors (D6) |
| Byte-at-a-time feeding of inputs above the 512 KiB cap exceeds the test budget | Low | Medium | Byte-at-a-time runs use a shorter input of the same shape (R11), as the predecessor's budget test does |
| The CLI-only check is not an approved `project.components` command | Certain | Low | Kept as a manual verification item (TS-10); task0001 records its own run |

## Open Questions
- [ ] TS-10's `--no-default-features` cargo check is not among the approved
      `project.components` commands; the verify phase treats it as a manual item unless the
      orchestrator approves that command.
- [ ] D2 rests on the in-tree models of term_core's escape transitions; it is confirmed only
      when R7 runs against term_core in task0001.
