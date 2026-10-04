# Verification Document: mux-snapshot-strip-can-abort

## Overview
**Feature**: mux-snapshot-strip-can-abort / **SPEC.md**: `feature-docs/mux-snapshot-strip-can-abort/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-snapshot-strip-can-abort/IMPLEMENTATION.md`

Run every command from the project root (the integration worktree root), with the quick-check target directory and the manifest path. Never `cd` into `src-tauri/` (`.claude/rules/core-build-location.md`).

## Build Verification
- Command (workflow.yaml `project.components.src-tauri.build_command`): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only build (NFR3): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, for both commands.

## Test Verification
- Command (workflow.yaml `project.components.src-tauri.test_command`): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: src-tauri has no coverage tool configured. Coverage is judged by scenario: every TS-n below is exercised by at least one named test, recorded in `test-docs/mux-snapshot-strip-can-abort/task0001.tests.yaml`.
- Known failures with causes outside this feature, none touched by it. Re-run each one alone before it counts as a regression:
  - The `tabs.rs` replay tests are nondeterministic in parallel. They are stable with `-- --test-threads=1`.
  - A `tmux_sockets` discover test rarely fails in parallel runs.
  - A wall-clock budget test can fail under machine load. The record `test-docs/mux-write-filter-overflow-open-string-cut/task0001.tests.yaml` notes one in `overflow_lone_esc.rs`.

### Test Scenarios from SPEC.md
TS-1 to TS-9 come from SPEC.md. TS-10 and TS-11 are added by the plan: they cover FR7's comment updates and a third old-behavior test that planning found.

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Strip-level regression (AC-1): a ring of leading text, an overflowed unterminated Kitty APC body (and, separately, a SIXEL DCS body), `ESC` + CAN, plain text, then `ESC ]0;title ESC \`, stripped with `strip_replayable_rich_content` and replayed by term_core | The output is exactly the leading text, `ESC` + CAN, the plain text and the OSC 0 bytes. The replay shows the plain text. Red before the fix | Unit |
| TS-2 | Write filter to snapshot round trip (AC-2): a Kitty APC / SIXEL DCS held at `SCROLLBACK_FILTER_PENDING_CAP` and flushed open past it, a cut, plain text, an ST-terminated OSC 0. The ring is stripped and replayed | The stripped ring is `ESC` + CAN, the text and the OSC 0. The replay shows the text, and its view equals the raw stream with each 47 / 1047 / 1049 pair in place of the cut | Integration |
| TS-3 | Aborting continuations (AC-3) after a short Kitty APC / SIXEL DCS body: `ESC [ 31m`, `ESC ]0;t ESC \`, `ESC ESC \`, `ESC` + CAN, `ESC _ G...ESC \`, `ESC P q...ESC \`, `ESC ]777;emterm;markdown;x BEL`, `ESC [6n` | The body is removed. Kept continuations stay whole, and strip-target continuations are removed. The surrounding text stays | Unit |
| TS-4 | Kept cases (AC-4): bodies with no ESC, a lone trailing ESC, `ESC P 1;0 ESC [ m`, `ESC P x ESC [ m`, `ESC _ x ESC [ m`, BEL with no ESC. Also BEL then ST | Every kept case is kept byte-for-byte. BEL then ST is removed whole | Unit |
| TS-5 | Closure and remap (AC-5): an aborted Kitty body after `ESC [ 6`, after a lone ESC, and in ground. Watch offsets at the introducer, inside the body, at the aborting ESC and after it | One DEL inside a CSI or after a lone ESC, none in ground. Offsets map as AC-5 states, are non-decreasing, and stay within the output | Unit |
| TS-6 | Entry-point identity (AC-6) for the TS-1 / TS-3 / TS-4 inputs | Write path equals snapshot path. The state form from Ground returns the same bytes and the end state `client_written_state` gives. The existing identity and state tests pass | Unit |
| TS-7 | Single pass (AC-7): the updated expectation of `strip_unterminated_introducers_complete_in_single_pass`, plus a timed run of at least 20,000 aborted Kitty APC / SIXEL DCS units | The output equals the last `ESC P 1;0;0q plain`. The timed run gives its exact expected output within the module's existing 2-second bound | Unit |
| TS-8 | Closure test update (AC-8 a): `overflow_open_string_cut_neither_strip_reads_the_closure_as_a_target_or_as_st` | `ESC` + CAN alone is kept. Kitty / SIXEL streams with the closure strip to `before` + `ESC` + CAN. Non-target streams stay whole. Targets after the closure are removed for every stream | Unit |
| TS-9 | Full suite and CLI-only check (AC-10) | `--lib` passes, `--no-default-features` compiles, and the `write_filter.rs` diff touches comment lines only | Integration |
| TS-10 | Doc-comment check (AC-9, planner-added): comment text of `write_filter.rs`, `scrollback_filter.rs` and `overflow_open_string_cut.rs` | None of the stale phrases appear. `scrollback_filter.rs` says "aborting `ESC`". The predecessor's doc-comment contract test passes | Unit |
| TS-11 | Escape-closure test update (AC-8 b, planner-added): part (d) of `escape_carry_the_escape_closure_has_no_effect_in_term_core` | `ESC` + the Escape closure inserted inside a Kitty APC / SIXEL DCS aborts it: the output is `head`, `ESC` + CAN, the construct's bytes from the insertion point, and `tail` | Unit |

## Code Quality Verification
- Format: workflow.yaml has no `format_command`. Do not run the formatter over the whole crate. Only the files in task0001's file set may be formatted.
- Static analysis: the two `cargo check` commands above report no new warnings in the changed files.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | All functional requirements are implemented and tested | Functional Requirements Coverage below: every FR has a passing TS |
| SC-2 | All test scenarios pass | TS-1 to TS-11 pass in the `--lib` run |
| SC-3 | Performance meets specified goals (NFR1) | TS-7 |
| SC-4 | Documentation is complete (FR7) | TS-10, plus a read of the updated comments against task0001's "Comments to update" list |
| SC-5 | Code review is completed | The review phase records no residual critical / high finding |
| SC-6 | Goal completion: the reproduction steps no longer show the defect, and a test detects a recurrence | TS-1 and TS-2 (recurrence detection), MT-1 (reproduction) |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2, TS-3 |
| FR2 | task0001 | TS-1, TS-2, TS-3 |
| FR3 | task0001 | TS-1, TS-2, TS-3, TS-4, TS-8, TS-11 |
| FR4 | task0001 | TS-4 |
| FR5 | task0001 | TS-5 |
| FR6 | task0001 | TS-2, TS-6 |
| FR7 | task0001 | TS-7, TS-8, TS-10, TS-11 |
| NFR1 | task0001 | TS-7 |
| NFR2 | task0001 | TS-9 |
| NFR3 | task0001 | TS-9 |

## Manual Testing (E2E Not Possible)
- [ ] MT-1 (goal reproduction): needs a release build and a restarted mux daemon, both started by the user. In a mux pane:
  1. Output a Kitty APC (`ESC _ G ...`), or a SIXEL DCS (`ESC P q ...`), with an unterminated body longer than 512 KiB.
  2. On the next read, output a screen switch (`ESC[?1049h` ... `ESC[?1049l`).
  3. Output main-buffer plain text, then `ESC ]0;title ESC \`.
  4. Detach and reattach to restore the snapshot.

  Expected: the plain text is displayed and the title is applied.

## Performance / Security Verification
- NFR1 (linear single pass): TS-7. The timed run finishes within the 2-second bound of the module's existing designator-chain test.
- TM-1 (removal ends at the aborting ESC; the bytes after it are kept): checked by TS-1 and TS-2. The plain text and the OSC 0 written after `ESC` + CAN are in the stripped output and on the replayed rows.
- TM-2 (an aborted Kitty APC / SIXEL DCS body, and a strip target starting at the aborting ESC, are removed): checked by TS-3, TS-8 and TS-11. Neither the aborted body nor an OSC 777 viewer launch, Kitty APC, SIXEL DCS or answered device query starting at the aborting ESC remains in the stripped output.
- TM-3 (no quadratic rescan on many aborted / unterminated introducers): checked by TS-7. Both the 20,000-unit input and the timed aborted-unit input finish within the 2-second bound.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios | 11 | 11 | 0 | 0 |
| Code quality | 2 | 1 | 0 | 1 |
| Success criteria | 6 | 4 | 0 | 2 |
| Performance (NFR1) | 1 | 1 | 0 | 0 |
| Security (TM-1, TM-2, TM-3) | 3 | 3 | 0 | 0 |
| Manual reproduction | 1 | 0 | 0 | 1 |
