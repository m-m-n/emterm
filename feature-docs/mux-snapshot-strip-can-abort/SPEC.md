# Feature: mux-snapshot-strip-can-abort

## Overview

The mux strip pass (`strip_pass` in `src-tauri/src/mux/scrollback_filter.rs`) ends a Kitty APC (`ESC _ G`) or DCS (`ESC P`) body scan at the first ESC in the body: `ESC \` completes it, and ESC followed by any other byte (including `ESC` + `CAN`) aborts it. An aborted Kitty APC / SIXEL DCS body is removed up to the aborting ESC, and strip judgement resumes at the aborting ESC with the existing rules. Requirements: `feature-docs/mux-snapshot-strip-can-abort/REQUIREMENTS.md`.

## Objectives

- A snapshot replay of a pane ring shows the bytes written after a Kitty APC / SIXEL DCS body that a cut closed with `ESC` + `CAN` (plain text, ST-terminated OSC). The result matches what the client displayed.

## User Stories

### US1: Snapshot replay after an aborted Kitty APC / SIXEL DCS body
As a mux client, I want a snapshot replay of a pane ring to show the bytes written after a Kitty APC / SIXEL DCS body that a cut closed with `ESC` + `CAN`, so that the result matches what the client displayed.

**Acceptance Criteria:**
- [ ] AC-1 (FR1, FR2, FR3): For a ring made of plain text, a Kitty APC (`ESC _ G ...`) body longer than 512 KiB with no terminator, `ESC` + `CAN`, main-buffer plain text and an ST-terminated OSC 0 (`ESC ]0;title ESC \`), `strip_replayable_rich_content` returns the leading text, `ESC` + `CAN`, the plain text and the OSC 0 bytes. term_core replaying that output displays the plain text. The same holds for a SIXEL DCS (`ESC P q ...`) body. Before the fix the plain text and the OSC are missing.
- [ ] AC-2 (FR1, FR2, FR3, FR6): Through `ScrollbackWriteFilter`, a Kitty APC / SIXEL DCS held at the cap and flushed open past it, a cut (a 47 / 1047 / 1049 h / l pair), plain text and an ST-terminated OSC 0 produce a ring. Passed through `strip_replayable_rich_content` and replayed by term_core, that ring displays the plain text. Its view (rows, cursor, responses) equals term_core fed the raw stream with the switch pair in place of the cut, for each pair.
- [ ] AC-3 (FR1, FR2, FR3): A Kitty APC / SIXEL DCS body aborted by ESC followed by a byte other than `\` (`ESC [`, `ESC ]`, `ESC _`, `ESC P`, `ESC ESC`, `ESC CAN`) is removed up to the aborting ESC. The bytes from the aborting ESC on are kept. A strip target starting at the aborting ESC (e.g. an OSC 777 markdown launch or another Kitty APC) is also removed. In `ESC _ G body ESC ESC \`, the body is removed and `ESC ESC \` is kept.
- [ ] AC-4 (FR3, FR4): These are kept byte-for-byte: a Kitty APC / SIXEL DCS body with no ESC, a body ending in a lone trailing ESC, a DCS aborted before reaching `q` (e.g. `ESC P 1;0 ESC [ m`), a DCS whose pre-abort final byte is not `q`, and a non-Kitty APC aborted by ESC. A BEL inside a Kitty APC / SIXEL DCS body neither ends nor aborts it.
- [ ] AC-5 (FR5): When an aborted body is removed while the written stream is inside a CSI or right after a written lone ESC, one CSI_CLOSING (DEL) is written first. In ground nothing is added. Through `strip_rich_content_and_remap`, a watch offset at the introducer maps before what the removal writes. Offsets strictly inside the removed body, or at the aborting ESC, map to the output position of the aborting ESC. Remapped offsets stay non-decreasing and within the output.
- [ ] AC-6 (FR6): For the AC-1 / AC-3 / AC-4 inputs, `strip_pty_output_for_scrollback_write` equals `strip_replayable_rich_content`. `strip_pty_output_for_scrollback_write_with_written_state` from Ground returns the same bytes and the end state the term_core end-state oracle (`client_written_state`) gives for them. The existing identity and state-agreement tests in `scrollback_filter/tests.rs` pass.
- [ ] AC-7 (FR7, NFR1): `strip_unterminated_introducers_complete_in_single_pass` runs 20,000 repetitions of `ESC _ G`, `ESC P 1;0;0q` and `plain`. Its output equals the last `ESC P 1;0;0q plain` (every earlier aborted Kitty APC and SIXEL DCS is removed, the last unterminated DCS is kept). It completes as a single linear pass. A long input of repeated aborted Kitty APC / SIXEL DCS bodies completes within the existing time budget.
- [ ] AC-8 (FR3, FR7): In `overflow_open_string_cut_neither_strip_reads_the_closure_as_a_target_or_as_st`, `ESC` + `CAN` alone is still kept by both strips. Each open Kitty APC / SIXEL DCS stream followed by `ESC` + `CAN` strips to `before` followed by `ESC` + `CAN`. The open OSC / DCS / APC non-target streams followed by `ESC` + `CAN` are kept whole. A strip target written after the closure is removed for every open stream, Kitty and SIXEL included.
- [ ] AC-9 (NFR2, NFR3): The full `--lib` suite passes, the existing write-filter tests in `overflow_open_string_cut.rs` (AC-2 .. AC-11 of the predecessor feature, with the AC-8 update above) pass, and the `--no-default-features` check compiles.

## Technical Requirements

### Functional Requirements
- **FR1:** Abort recognition in the APC / DCS body scan. The strip pass in `src-tauri/src/mux/scrollback_filter.rs` (`strip_pass`, used by `strip_replayable_rich_content`, `strip_pty_output_for_scrollback_write` and their remap / designator / written-state forms) scans a Kitty APC (`ESC _ G`) or DCS (`ESC P`) body for its end. The scan ends at the first ESC in the body. ESC followed by `\` is the ST terminator (complete). ESC followed by any other byte, including ESC and CAN (0x18), aborts the body at that ESC. A body with no ESC, or whose only ESC is the last byte of the input, is unterminated.
- **FR2:** Bytes after the abort keep the existing judgement. The aborting ESC and every byte after it are not part of the aborted body. Strip judgement resumes at the aborting ESC with the existing rules, so plain text, kept OSC (e.g. OSC 0 / OSC 8) and kept escapes after the abort stay in the output. A strip target that starts at or after the aborting ESC is removed as its own construct.
- **FR3:** Removal of the aborted Kitty APC / SIXEL DCS body. An aborted Kitty APC body is removed: the bytes from its introducer ESC up to, but not including, the aborting ESC. An aborted DCS body is removed the same way when its pre-abort range (the bytes between `ESC P` and the aborting ESC) is SIXEL by the existing final-byte rule (`dcs_is_sixel`: final byte `q`). An aborted DCS whose pre-abort range never reached `q` is kept. A non-Kitty APC is kept, as before.
- **FR4:** Unterminated and BEL cases stay kept. A Kitty APC / SIXEL DCS body with no ESC, or whose body ends in a lone trailing ESC at the end of the input, is kept byte-for-byte as an unterminated sequence. BEL inside a DCS / APC body is body data and neither ends nor aborts it.
- **FR5:** Existing strip rules unchanged. The removal of complete strip targets (Kitty APC, SIXEL DCS, OSC 777 viewer launch / agent-status, OSC 9999 emterm-md, answered CSI device queries), the D1 CSI_CLOSING written before a removal inside a CSI or after a written lone ESC, the D2 rewrite of a completing device query, the watch-offset remap rules, the designator handling and the rule that the carried WrittenState decides no removal all apply unchanged. They apply to the aborted-body removal of FR3 the same way they apply to any removal.
- **FR6:** Write path and snapshot path stay identical. `strip_pty_output_for_scrollback_write` and `strip_replayable_rich_content` keep returning identical output for every input. The state-reporting form reports the end state of the stripped bytes as term_core classifies it.
- **FR7:** Tests and comments that state the old behavior are updated. These tests now expect the FR1-FR4 behavior. In `overflow_open_string_cut_neither_strip_reads_the_closure_as_a_target_or_as_st`, an open Kitty APC / SIXEL DCS stream followed by `ESC` + `CAN` strips to the bytes before the introducer followed by `ESC` + `CAN`, and the Kitty / SIXEL streams are no longer excluded from the "strip target right after the closure is removed" loop. In `strip_unterminated_introducers_complete_in_single_pass`, the expected output follows FR3/FR4. The doc comments that describe the ST search or the closure are updated to the new behavior: `scrollback_filter.rs` module / function docs and the `find_st_terminator` / `st_search_from` docs, the `write_filter.rs` `ESCAPE_CLOSING` / `STRING_BODY_CLOSING` docs ("nor ends an APC / DCS body at it") and the `find_st` doc, and the `overflow_open_string_cut.rs` comment at 651-655.

### Non-Functional Requirements
- **NFR1 - Linear single pass:** The strip stays one O(n) pass with no per-introducer rescan of the tail, including on input made of many aborted or unterminated APC / DCS introducers.
- **NFR2 - Write filter cut logic unchanged:** The write filter's boundary scan (`scan_boundary` / `find_st` / `find_osc_end`), `closure_for`, `STRING_BODY_CLOSING`, `ESCAPE_CLOSING`, `CSI_CLOSING` and `DESIGNATOR_CLOSING` keep their behavior. The OSC terminator scan and the CSI device-query scan keep their behavior.
- **NFR3 - CLI-only build:** The change compiles in the `--no-default-features` (CLI + mux) build.

## Implementation Approach

### Architecture

**Component Diagram:**
```
scrollback_filter.rs
  strip_pass  (single O(n) pass; changed: APC / DCS body end scan, FR1-FR4)
    <- strip_replayable_rich_content                 (snapshot path)
    <- strip_pty_output_for_scrollback_write         (write path)
    <- remap / designator / written-state forms
       (strip_rich_content_and_remap,
        strip_pty_output_for_scrollback_write_with_written_state)

write_filter.rs  (ScrollbackWriteFilter: scan_boundary / find_st / find_osc_end,
                  closure_for, *_CLOSING constants; behavior unchanged, NFR2;
                  doc comments updated, FR7)
```

### Data Flow

```
ScrollbackWriteFilter -> ring -> strip_replayable_rich_content -> term_core replay
```

### Body end scan (FR1-FR4)

| Body content from the introducer | Classification | Strip result |
|---|---|---|
| First ESC followed by `\` | Complete (ST) | Existing complete-target rule (FR5) |
| First ESC followed by any other byte (incl. ESC, CAN) | Aborted at that ESC | Kitty APC: removed up to the aborting ESC. DCS: removed when the pre-abort range is SIXEL (`dcs_is_sixel`), otherwise kept. Non-Kitty APC: kept. Judgement resumes at the aborting ESC (FR2) |
| No ESC | Unterminated | Kept byte-for-byte |
| Only ESC is the last byte of the input | Unterminated | Kept byte-for-byte |
| BEL | Body data | Neither ends nor aborts |

### API Design

Not applicable.

### Database Schema

Not applicable.

### Dependencies

**Internal Dependencies:**
- term_core: replay of the stripped output and the end-state oracle (`client_written_state`) used in AC-2 / AC-6.
- `ScrollbackWriteFilter`: produces the ring in AC-2.

**External Dependencies:**
- None.

### File Structure

Files named in the requirements:

```
src-tauri/src/mux/scrollback_filter.rs   # strip_pass body end scan; module / function docs,
                                         # find_st_terminator / st_search_from docs
src-tauri/src/mux/scrollback_filter/tests.rs                       # identity and state-agreement tests
src-tauri/src/mux/ipc/pty_spawn/write_filter.rs                    # ESCAPE_CLOSING / STRING_BODY_CLOSING / find_st docs
src-tauri/src/mux/ipc/pty_spawn/tests/overflow_open_string_cut.rs  # test expectations; comment at 651-655
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-snapshot-strip-can-abort/**`
- `test-docs/mux-snapshot-strip-can-abort/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers `test-docs/{feature}/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1 (AC-1): Strip-level regression. Build a ring of an overflowed unterminated Kitty APC (and, separately, a SIXEL DCS) followed by `ESC` + `CAN`, plain text and `ESC ]0;title ESC \`. Pass it through `strip_replayable_rich_content`, assert the exact output bytes, then replay through term_core and assert the plain text is on a row. Red before the fix.
- [ ] TS-3 (AC-3): Table of aborting continuations after a short Kitty APC / SIXEL DCS body: `ESC [ 31m`, `ESC ]0;t ESC \`, `ESC _ G...ESC \`, `ESC P q...ESC \`, `ESC ESC \`, `ESC CAN`, `ESC ]777;emterm;markdown;x BEL`. Assert the exact output of `strip_replayable_rich_content`.
- [ ] TS-4 (AC-4): Kept-case table: Kitty / SIXEL body without ESC, body with a trailing lone ESC, `ESC P 1;0 ESC [ m`, `ESC P x ESC [ m`, `ESC _ x ESC [ m`, and a Kitty / SIXEL body containing BEL then ST (removed whole) or BEL with no ESC (kept).
- [ ] TS-5 (AC-5): Closure and remap. An aborted Kitty body after `ESC [ 6` and after a lone ESC writes one DEL. Run `strip_rich_content_and_remap` with watch offsets at the introducer, inside the body, at the aborting ESC and after it, and assert the remapped positions.
- [ ] TS-6 (AC-6): Run the TS-1 / TS-3 / TS-4 inputs through every strip entry point and compare outputs. Check that the state-form end state equals `client_written_state` of the written bytes.
- [ ] TS-8 (AC-8): Update `overflow_open_string_cut_neither_strip_reads_the_closure_as_a_target_or_as_st`: the Kitty / SIXEL closed-stream expectation, and remove the Kitty / SIXEL skip in the target-after-closure loop together with its comment.

### Integration Tests
- [ ] TS-2 (AC-2): Write filter to snapshot round trip. Hold a Kitty APC / SIXEL DCS at `SCROLLBACK_FILTER_PENDING_CAP`, overflow it, cut with each 47 / 1047 / 1049 pair, then write plain text and an ST-terminated OSC 0. Strip the ring with `strip_replayable_rich_content`, replay through term_core, and compare against `view_after_a_cut` of the raw stream. Uses the payloads that neither answer nor place an image (`a=d`, `q#0;2;0;0;0`).
- [ ] TS-9 (AC-9): From the project root, run `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` and `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`.

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Body ending in a lone trailing ESC at the end of the input: kept byte-for-byte (FR4, TS-4).
- [ ] `ESC _ G body ESC ESC \`: the body is removed and `ESC ESC \` is kept (AC-3, TS-3).
- [ ] DCS aborted before reaching `q`: kept (FR3, TS-4).
- [ ] BEL inside a Kitty APC / SIXEL DCS body: body data (FR4, TS-4).

### Performance Tests
- [ ] TS-7 (AC-7): Update the expectation of `strip_unterminated_introducers_complete_in_single_pass`. Add a timed run of repeated aborted Kitty APC / SIXEL DCS units.

## Security Considerations

Not applicable.

## Error Handling

Not applicable.

## Performance Optimization

### Performance Goals
- NFR1: one O(n) pass with no per-introducer rescan of the tail.

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Documentation is complete
- [ ] Code review is completed

## Assumptions

- **A-1** (impact: medium, reversible; question `requirement.aborted-body-handling`, create-spec-q0001, option `remove_aborted_body`, resolved under batch policy `codex_consultation`, recorded as an assumption): Aborted Kitty APC / SIXEL DCS bodies are removed: from the introducer up to, but not including, the aborting ESC. Normal strip judgement resumes at the aborting ESC. A lone trailing ESC and a body with no ESC stay kept. BEL is body data. SIXEL detection uses only the pre-abort range, so a DCS that never reached `q` is kept. The existing remap rules and O(n) are kept. The existing AC-8 Kitty/SIXEL expectation and the `strip_unterminated_introducers_complete_in_single_pass` expectation change accordingly.
- **A-2** (impact: medium, reversible): The change applies to the write-path strip as well as the snapshot strip, because both are the single `strip_pass`. Their identity (`strip_pty_output_for_scrollback_write_matches_snapshot_strip_for_everything` and the post_strip identity tests) is preserved.
- **A-3** (impact: low, reversible): The write filter's boundary scan and cut closures (`find_st`, `scan_boundary`, `closure_for`, `STRING_BODY_CLOSING`) are not changed. Only their doc comments that describe the strip's handling of `ESC` + `CAN` are updated.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- None.

## References

- Requirements: `feature-docs/mux-snapshot-strip-can-abort/REQUIREMENTS.md`
- `src-tauri/src/mux/scrollback_filter.rs`
