# Verification Document: mux-suppressed-output-fixes

## Overview
**Feature**: mux-suppressed-output-fixes / **SPEC.md**: `feature-docs/mux-suppressed-output-fixes/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-suppressed-output-fixes/IMPLEMENTATION.md`

This document covers the integrated verification run by the verify phase. Task-level acceptance criteria live in `tasks/task0001.md` through `tasks/task0005.md`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors (the CLI + mux build without the gui feature)

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, all tests pass
- Coverage target: no line-coverage tool is configured. Every task Acceptance Criterion and every TS below maps to at least one named test (IMPLEMENTATION.md D7).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | FR1: on the alternate screen, an OSC 11;? whose ST is split between ESC and backslash, with the first half suppressed; plus the write-filter side with a trailing ESC in a main-buffer OSC | The query is answered once, and the backslash of the continuation is not displayed. The filter holds the OSC until it completes and never treats the trailing ESC as an abort. | Unit + Integration |
| TS-2 | FR2: a suppressed chunk containing each of: ESC ( ESC [ 6 n / ESC X … ESC [ 6 n / ESC P q ESC [ 6 n ESC backslash / ESC ESC [ c / OSC 010;? / OSC 10;?;? / OSC 10;#fff;? / OSC 4;1;?;2;#000000 / an aborted OSC 11;? | Whether each query is extracted, and how many responses result, matches the term_core + theme reference | Unit + Integration |
| TS-3 | FR3: a chunk cut inside an incomplete CSI that contains an LF, including the case carried over from the previous read | The LF is executed exactly once, and screen and cursor equal the reference | Unit + Integration |
| TS-4 | FR4: pending is non-empty, and the suppressed chunk's alternate-screen region holds a query, a viewer launch and an incomplete tail | None of them is dropped | Unit + Integration |
| TS-5 | FR5: in the previous read, an ESC-aborted OSC and an ESC-aborted DCS are followed by CSI 6n and text; the next chunk is suppressed | The filter holds nothing, the 6n and text are not re-sent, and the ring content equals the strip of the reference stream | Integration |
| TS-6 | FR6: ESC [ 6 is delivered, then a chunk starting with n is suppressed; a UTF-8 lead byte is delivered, then its continuation is suppressed; the split position is swept byte by byte; a sequence longer than N | One response; no U+FFFD; nothing fabricated and no panic past N | Unit + Integration |
| TS-7 | FR6: two consecutive suppressed chunks; the first ends in an incomplete SGR that the second completes | The next forwarded chunk's first byte is not consumed as a CSI final | Integration |
| TS-8 | FR7: a complete OSC 777 emterm markdown inside a suppressed chunk, also overlapping a pending re-send; a Kitty APC in the same kind of chunk | The viewer launch arrives exactly once after the snapshot, and the Kitty APC is not delivered | Integration |
| TS-9 | FR8: a chunk containing ESC[?1049h is suppressed right before a visibility resume; a pane goes back to main via ESC[?1049l while hidden; a pane stays on the alternate screen | In every case the client's screen mode and content equal the shadow parser's | Integration |
| TS-10 | FR8: hide then show a main-screen pane showing an apt-style progress bar (DECSTBM, DECSC/DECRC) | Cursor, scroll region and display equal those before the hide | Integration |
| TS-11 | FR9: a suppressed chunk containing A ESC[6n B ESC[c | Both queries arrive after the snapshot, in order, once each; exactly 2 responses | Integration |
| TS-12 | FR10: the visible path through resume_pane_with_permit; a poisoned shadow parser; a hide/show round trip | Delivery order is snapshot, then replacement, then the next chunk; poison is recovered; the round trip ends Connected | Integration |
| TS-13 | FR11: in split_osc9_across_two_suppressed_chunks_never_fires_more_than_once, p2.arm happens before the release | The ordering is corrected and the original assertions still hold | Integration |
| TS-14 | FR12: collect_reattach_data, handle_request_pane_snapshot and resume_pane_with_permit run repeatedly in parallel with the reader and resize | Finishes within the time budget; no deadlock; each path delivers at least once | Concurrency |
| TS-15 | FR13 (planner-added): DECISIONS.md decision table | All 18 stable_ids present, with 9e6a468b3a45ceeb split by target; each row has a verdict, a rationale and regression tests per the D7 registry; round1.yaml unchanged | Document check |
| TS-16 | NFR1 (planner-added): reattach and on-demand snapshot layouts, the wire format, and the replacement chunk kind | The existing reattach/on-demand layout tests pass unmodified; crates/mux_ipc is unchanged; the replacement is a PtyOutput-kind chunk | Unit + Integration |
| TS-17 | NFR3 (planner-added): the reader's retained window over reads of varying size | The window equals the last min(N, total) bytes before each read, and the normal path does no scan | Unit + Review |
| TS-18 | NFR4 (planner-added): TM-1 fabrication-negative corpus and TM-3 destination check | Nothing is extracted where the client parser starts no sequence; the replacement goes only to the covering snapshot's destination | Unit + Integration |
| TS-19 | NFR5 (planner-added): TM-2 hostile input and the pending cap | Linear scan without panic; no empty PtyOutput; the 512 KiB pending cap and overflow flush are unchanged | Unit + Integration |
| TS-20 | NFR6 (planner-added): CLI-only build and platform neutrality | The Build Verification command passes; no production code is platform-specific; new tests use scripted readers | Build + Review |

## Code Quality Verification
- Format: none configured (`format_command` is empty). Only the touched files are formatted, never the whole crate.
- Static analysis: covered by the Build Verification command, since `cargo check` without the gui feature compiles every production mux path.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | Every addressed finding has a regression test that fails on the pre-fix code and passes after | The D7 registry tests exist. For each addressed stable_id, the test fails on the implement base commit (390d9eff0b40d930db2d50f3d40e028c8d7c9b1f) and passes on the integration head (task test records). |
| AC-2 | Feeding snapshot + replacement + next chunk into term_core matches the reference fed the raw stream once | TS-1, TS-3, TS-4, TS-6, TS-7 reference comparisons (screen, cursor, response bytes, displayed characters) |
| AC-3 | Queries arrive exactly once, in order, after the snapshot; delivered queries, non-sequence positions and aborted OSCs are not delivered | TS-2, TS-11, TS-18 |
| AC-4 | Viewer launches arrive exactly once, never duplicated; inline images are not delivered | TS-8 |
| AC-5 | After a visibility resume, screen mode and content equal the shadow parser's; the apt progress-bar display is unchanged | TS-9, TS-10 |
| AC-6 | EvalResult::ResumeWithSnapshot does not exist; on the real resume path the snapshot arrives before the replacement | TS-12, plus a source search for the variant name returning nothing |
| AC-7 | All 18 stable_ids are recorded with verdict, rationale and regression tests; round1.yaml unchanged | TS-15 |
| AC-8 | Existing tests pass unmodified, except the listed behavior-changing ones | Test Verification command, plus a diff of existing test bodies against the base commit, checked against the lists in each task's completion report. Expected list (SPEC.md): the suppressed_output.rs tests for a bare trailing ESC, an embedded-ESC abort, and the pending cases; the two pty_spawn/tests.rs DCS-query tests; the snapshot_bytes.rs resume-layout test; the pane/tests.rs ResumeWithSnapshot tests (deleted or ported). |
| AC-9 | Both approved commands pass | Build Verification and Test Verification commands |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS-1 |
| FR2 | task0001 | TS-2 |
| FR3 | task0001 | TS-3 |
| FR4 | task0001 | TS-4 |
| FR5 | task0002 | TS-5 |
| FR6 | task0001 | TS-6, TS-7 |
| FR7 | task0001 | TS-8 |
| FR8 | task0003 | TS-9, TS-10 |
| FR9 | task0001 | TS-11 |
| FR10 | task0004 | TS-12 |
| FR11 | task0005 | TS-13 |
| FR12 | task0005 | TS-14 |
| FR13 | task0005 | TS-15 |
| NFR1 | task0001, task0003 | TS-16 |
| NFR2 | task0001, task0004, task0005 | TS-14 |
| NFR3 | task0001 | TS-17 |
| NFR4 | task0001 | TS-18 |
| NFR5 | task0001, task0002 | TS-19 |
| NFR6 | task0001, task0002, task0003, task0004, task0005 | TS-20 |

## Manual Testing (E2E Not Possible)
- [ ] M-1 (FR8): In a real mux session, run a full-screen TUI on the alternate screen (e.g. less). Hide and show its pane: it comes back on the alternate screen with its content. Then exit the TUI while its pane is hidden and show the pane again: it comes back on the main screen with the shell history.
- [ ] M-2 (FR8, TS-10): In a real mux session, hide and show a pane while an apt-style progress bar is running. The bar and the log lines render as they did before the change.
- [ ] M-3 (NFR6): Windows parity. Review that no production change is platform-specific. The user may optionally run the Windows cross-build.
- [ ] M-4 (FR13): Read DECISIONS.md and check that each verdict and rationale is consistent with round1.yaml's finding text.

## Performance / Security Verification
- NFR3: the reader's normal path adds only the covered-sequence comparison and a copy of at most 256 bytes (IMPLEMENTATION.md D2). Checked by TS-17 and a code review of the reader loop: no classifier call outside the suppressed pipeline.
- NFR5: bounded work. Checked by TS-19.
- TM-1: the classifier mirrors term_core's transitions, and extraction only happens at positions where the client starts a sequence. Checked by TS-2 and TS-18: the fabrication-negative corpus (designator ESC, 8-bit C1, aborted OSC, undecidable retained-window start) produces no extracted item, and the extraction and response counts match the reference.
- TM-2: single bounded pass, window capped at N, non-panicking OSC-number arithmetic, no empty PtyOutput, pending cap unchanged. Checked by TS-19 (a 64 KiB hostile chunk for the builder and for the write filter, an empty-replacement case, unmodified cap tests).
- TM-3: the replacement is delivered only to the destination captured when the chunk was judged covered. Checked by TS-18 (the existing destination-takeover test passes unmodified, and TS-11 asserts a single destination).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build Verification | 1 | 1 | 0 | 0 |
| Test Scenarios (TS-1 to TS-20) | 20 | 19 | 0 | 1 |
| Success Criteria (AC-1 to AC-9) | 9 | 8 | 0 | 1 |
| Manual Testing (M-1 to M-4) | 4 | 0 | 0 | 4 |
| Performance / Security (NFR3, NFR5, TM-1, TM-2, TM-3) | 5 | 5 | 0 | 0 |
