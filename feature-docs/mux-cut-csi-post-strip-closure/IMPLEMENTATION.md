# Implementation Plan: mux-cut-csi-post-strip-closure

## Overview
The mux daemon's scrollback write filter decides the DEL closing at a cut, and
the CSI state it carries to the next call, from the CSI state of the bytes it
actually writes after the strip, instead of the boundary scan's pre-strip
state. The verdict on review finding `4c0ad9058a983648` and the cut-free strip
concatenation residuals (SPEC FR6) are recorded in this feature's decision
record.

## Technology Stack
- **Language / crate**: Rust, crate `src-tauri` (the mux daemon is part of both
  the default build and the `--no-default-features` CLI build).
- **Test oracle**: the workspace crate `term_core`, fed the raw stream, as in
  the predecessor feature mux-suppressed-output-round4-fixes.
- **New dependencies**: none. `project.license` (MIT) is unaffected.

## Layer Structure
| Layer | Location | Responsibility |
|-------|----------|----------------|
| Shared strip | `src-tauri/src/mux/scrollback_filter.rs` | Stateless removal of rich-content launches and answered CSI device queries; used by the write path and by the snapshot path. |
| Write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | Per-pane stateful filter on the reader thread: boundary scan, held chains, cut closing, overflow flush, carried state. |
| Tests | `src-tauri/src/mux/ipc/pty_spawn/tests/`, `src-tauri/src/mux/scrollback_filter/tests.rs` | Strip-level, filter-level and reader-level regression tests; the decision-record test. |
| Decision record | `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` | Verdict on `4c0ad9058a983648` and the FR6 residuals. |

Dependency direction: the write filter depends on the shared strip. The shared
strip never depends on `mux::ipc` (the session layer's snapshot path uses it
too).

## Shared Components
| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Post-strip CSI state rule (D1) | Defines the CSI state that decides the closing and the carry | The CSI state of the emitted stream is the CSI sub-state (none / entry / parameter) a term_core parser is in after the bytes the filter wrote, that is, after the strip. It is obtained inside the strip's existing single pass, starting from the state before the stripped bytes. The cut closing (one DEL), the state carried after a cut-free call and both overflow-flush decisions use it. | task0001 (implements), task0002 (records it as the rationale) |
| Regression test identifiers | The names the decision record cites and verify resolves | The table below. task0001 creates R1-R7 and R9 under exactly these paths; task0002 creates R8. Each appears as a test in the `--lib -- --list` output. | task0001, task0002 |
| Existing-test expectations | What the decision record states under behavior-changing tests | No existing test's expectation changes. R1 gains three CLOSING_CASES rows, with a row-specific byte-by-byte expectation for the CSI-query row (D2); no existing row changes. A change task0001 cannot avoid is reported as a plan deviation, and the record is updated in review. No test is renamed, so no predecessor test-docs record changes. | task0001, task0002 |
| Test module registry | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | Each task appends one module declaration (with a one-line comment naming this feature and the task) at the end of the file. On merge, both declarations are kept. | task0001, task0002 |

### Regression test identifiers
| ID | Full test path | Scenario | Owner |
|----|----------------|----------|-------|
| R1 | `mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi` (existing, extended) | TS-1 | task0001 |
| R2 | `mux::ipc::pty_spawn::tests::round4_cut_csi::post_strip_a_cut_after_a_stripped_construct_replays_like_the_raw_stream` | TS-2 | task0001 |
| R3 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_carried_csi_state_follows_the_stripped_output` | TS-3 | task0001 |
| R4 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi` | TS-4 | task0001 |
| R5 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_reader_restore_matches_the_raw_stream_reference` | TS-5 | task0001 |
| R6 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_the_closing_follows_the_stripped_output` | TS-1 (EC-2, EC-3, EC-4, EC-6) | task0001 |
| R7 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi::post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget` | TS-7 | task0001 |
| R8 | `mux::ipc::pty_spawn::tests::post_strip_cut_csi_record::post_strip_the_decision_record_states_the_verdict_and_the_residuals` | TS-6 | task0002 |
| R9 | `mux::scrollback_filter::tests::post_strip_state_form_output_equals_the_write_path_strip` | TS-9 | task0001 |

## Conventions
- New tests of this feature are named with the prefix `post_strip_`.
- Code and doc comments cite requirements as "mux-cut-csi-post-strip-closure FRn".
- No log output is added on the reader's per-call path.
- Technical documents, code comments and the decision record are in English.

## Cross-task Design Decisions

### D1: The post-strip CSI state comes from the strip pass
Decision: the shared strip gains a write-path form that, in the same single
pass, reports the CSI sub-state its output leaves the stream in, given the
state before. The write filter uses that report, not the boundary scan's CSI
state, for the closing at a cut, for the carried state and on the overflow
path.

Rationale: only the strip decides which constructs it removes (OSC identity,
Kitty APC, SIXEL DCS, answered CSI queries). Repeating those decisions in the
boundary scan would duplicate the strip's predicates and could drift from
them. Advancing O(1) state inside the existing pass satisfies NFR2.

Affected tasks: task0001 implements it; task0002 cites it as the fix.

### D2: Fix scope is cut-and-carry (SPEC as-04)
The output of every existing strip form, the snapshot path and the `mux_ipc`
wire format do not change (NFR1). The three cut-free strip concatenation cases
of SPEC FR6 are recorded, not fixed. A CSI device query split across calls is
written to the ring as it arrives (CSI bytes are never held) and only the
snapshot-time strip removes it (FR6 item 3). Tests that split the input or
feed it byte by byte therefore either avoid CSI-query strip targets or state
that expectation explicitly.

Affected tasks: task0001 (test design), task0002 (residuals).

### D3: What still comes from the boundary scan
The boundary, the held chain, the start of its last construct, the carried
completion and the awaiting-designator state keep coming from the boundary
scan. Only the CSI state used for the closing and the carry changes source.
The designator `ESC` and the DEL are never written at the same cut
(predecessor invariant, EC-6).

Affected tasks: task0001; task0002 (wording of the rationale).

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The CSI tracking in the strip pass drifts from the boundary scan's (term_core's) transitions | Medium | High (a wrong or missing DEL) | One CSI transition definition shared by both; the existing token-corpus test that compares the carried state with term_core keeps running; R3 and R6 compare with term_core |
| An existing test's expectation changes | Low | Medium (the record becomes inaccurate) | Shared Components contract: reported deviation, record updated in review |
| Extra per-byte work in the strip slows the reader or the snapshot path | Low | Medium | O(1) state; R7 and the existing budget tests |
| The removed construct's own effect (term_core's legitimate answer to `ESC[6n`, Kitty replies or placements) pollutes raw-stream comparisons | High | Low (false failures) | Compare only effects provoked after the cut, or use payloads with no effect (EC-5) |
| Merge conflict in the test module registry | High | Low | Keep both declarations |

## Open Questions
- [ ] A construct the strip removes right after a kept `ESC` (for example
  `ESC[6`, `ESC ESC`, a complete viewer launch, then a cut) leaves the emitted
  run ending in an escape state (with a following `(` or `)`, a designator
  wait) that neither the boundary scan nor the cut closing reports. On replay
  the first bytes written after the cut are then read as part of that escape
  sequence, which the live client did not do. It is not a CSI at the cut, so
  it is outside FR1-FR3 and outside SPEC FR6's list. This plan does not
  address it; whether a follow-up task takes it is undecided.
