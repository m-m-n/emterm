# Implementation Plan: mux-suppressed-output-fixes

## Overview
This feature makes the suppressed-chunk replacement and the scrollback write
filter classify PTY bytes the way the client does (term_core parser and GUI
theme). It also makes the visibility-resume snapshot restore the screen mode,
removes the unreachable `ResumeWithSnapshot` branch, fixes and adds tests, and
records a verdict for each of the 18 open review findings.

## Technology Stack
- **Language**: Rust. All work is in the existing `src-tauri` crate (mux daemon
  code under `src-tauri/src/mux/`).
- **Reference (test oracle only)**: `term_core` (`crates/term_core`),
  already a dependency; its parser is crate-private, so production mux code
  mirrors it rather than calling it. The GUI theme (`src-tauri/src/render/theme.rs`) is
  gui-gated and is used only by tests gated on the `gui` feature.
- **New dependencies**: none. Nothing to check against `project.license` (MIT).

## Layer Structure
- **Reader** (`mux/ipc/pty_spawn/mod.rs`): per-read capture step, forward
  decision, and the suppressed-chunk pipeline. It owns reader-thread-local state.
- **Suppressed-chunk replacement** (`mux/ipc/pty_spawn/suppressed_output.rs`) on
  top of a new **client-parity classifier** (`mux/ipc/pty_spawn/client_parity_scan.rs`).
  Existing SSOTs are reused unchanged: the CSI device-query predicate in
  `mux/scrollback_filter.rs` and the viewer-kind list in `viewer_kinds`.
- **Scrollback write filter** (`mux/ipc/pty_spawn/write_filter.rs`): the capture
  step feeds it main-buffer bytes, and it holds incomplete strings in pending.
- **Visibility resume**: `mux/ipc/handlers/attach.rs` →
  `mux/session/pane/output_target.rs` (`resume_pane_with_permit`) →
  `mux/snapshot_bytes.rs` (resume layout).

Dependency direction is unchanged: `ipc` → `session/pane` → `snapshot_bytes` →
`scrollback_filter`. No non-test mux code may reference a gui-gated module.

## Shared Components
| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| FR2 transition rule set (normative text: SPEC.md "走査の遷移規則（FR2）" table) | Defines where the client parser starts, continues, completes, aborts or cancels a sequence | Each consumer implements the rules independently (see D1). Post: for any byte stream, the consumer's sequence boundaries equal those term_core's parser produces for the same bytes. Proven by tests that use term_core as the oracle over a shared corpus, at minimum the TS-2 byte strings plus split positions. | task0001, task0002 |
| Incomplete vs aborted string (FR1) | Tells an unfinished OSC/DCS/APC apart from one ended by ESC | Pre: an OSC/DCS/APC string is open. Post: an ESC that is the last available byte leaves the string **incomplete**, and the next byte decides. An ESC followed by a backslash completes it with ST. An ESC followed by any other byte **aborts** (closes) it, and that byte is processed as the start of a new escape sequence. | task0001, task0002 |
| Write-filter pending (D4) | The bytes held by the filter after each feed, readable through its existing read-only accessor | Post (after every feed, except right after an overflow flush, when it is empty): pending is either empty or starts at the ESC that opens the single OSC/DCS/APC string, or the lone trailing ESC, still incomplete at the end of the fed main-buffer stream. It contains that sequence's bytes and nothing else: no completed or aborted sequence and no text after one. The accessor's shape, the 512 KiB cap and the overflow flush are unchanged. | task0002 (producer), task0001 (consumer) |
| Suppressed-chunk delivery contract | What a sibling task's test may assume about a suppressed chunk | A suppressed chunk produces at most one PtyOutput-kind chunk, sent only to the covering snapshot's destination, after that snapshot and before the next reader chunk. An empty replacement sends nothing. For a chunk whose only special content is one complete CSI device query in the main buffer (for example "A", ESC [ 6 n, "B"), the replacement is exactly that query's bytes both before and after this feature. Sibling tests that go through the reader use only such inputs. | task0001 (owner), task0003, task0004, task0005 |
| Resume snapshot builder (`snapshot_bytes.rs` resume variants) | Byte layout of the visibility-resume Snapshot chunk | Signatures and the main-pane layout stay unchanged (FR8, NFR1). The alt-pane layout may change (task0003). Other tasks must not pin alt-pane resume bytes in new tests. Reattach / on-demand layouts do not change. | task0003 (owner), task0004 |
| Existing test helpers in `pty_spawn/tests.rs` and `pane/tests.rs` | Scripted readers, session/reader spawners, segment converters, snapshot decode, poison helper | Existing helper signatures and behavior stay unchanged. A task may add new helpers next to its own tests. | task0001, task0002, task0003, task0004, task0005 |
| Test-side client model | Stand-in for the GUI client in reference comparisons | A Snapshot-kind chunk is applied as core reset plus segment replay. PtyOutput is applied as full-drain processing. This is the existing pattern. Only task0003 may extend it with buffer-switch routing, to mirror the GUI behavior it establishes. | task0001, task0002, task0003, task0004 |
| Regression-test registry (D7) | Maps each stable_id to the regression tests that detect it | The owning task creates each listed test under exactly the listed name. task0005 writes the decision table from this registry. | all tasks |

## Conventions
- **Lock discipline (NFR2)**: take locks in the order output_target, then the
  capture/boundary exclusion, then ring/shadow. Never block on a channel send
  while holding output_target. New code adds no lock.
- **No empty PtyOutput (TM-2)**: an empty payload is never turned into a
  PtyOutput chunk, because the client reads that as PTY exit.
- **Logging**: anomalies are logged at warn level (release logs keep warn and
  above). Log metadata (lengths, offsets, pane id) only, never payload bytes.
- **Feature gating**: all production code in this feature builds under
  `--no-default-features`. Tests that use the GUI theme as an oracle are gated
  on the `gui` feature.
- **Shared test files**: new tests go next to the related existing tests, never
  at the end of `pty_spawn/tests.rs` or `pane/tests.rs`, so parallel merges stay
  clean.
- **Behavior-changing tests (AC-8)**: every existing test whose expectation a
  task changes, deletes or ports is listed in that task's completion report,
  with the reason. Any other existing test must pass unmodified.
- **Formatting**: no format command is configured. Format only the files the
  task touches; never reformat the whole crate.

## Cross-task Design Decisions

### D1: Client parity is proven by oracle tests, not by sharing code
The classifier (task0001) and the write-filter boundary scan (task0002) each
implement the FR2 rule set against the table in SPEC.md. They run in parallel,
so neither can depend on the other's code. Both are held to the same oracle,
term_core, over the same corpus; color-query decisions are also checked against
the GUI theme in gui-gated tests. Moving the filter onto the classifier is out
of scope. Affected: task0001, task0002.

### D2: Retained window N = 256 bytes (FR6, NFR3)
The reader keeps the last up to 256 bytes of the PTY stream that came before the
current read, as a sliding window over previous reads. On the normal path it
only copies into this window. 256 bytes covers the unfinished part of any CSI
device query, an ESC plus designator (up to 2 bytes), a UTF-8 lead plus
continuations (up to 3 bytes), long SGR parameter lists, and ordinary color-query
OSCs. The per-read copy is negligible next to the read size.

Sequences whose start lies more than N bytes back are a known gap. They are
recorded by task0005:
- a string outside the ring-written spans (as-05);
- a viewer launch that completes in the suppressed chunk.

Affected: task0001 (implements), task0005 (records).

### D3: OSC 777 emterm image is treated as a viewer launch (FR7)
The codebase's viewer-kind SSOT already groups `image` with markdown, json and
yaml as viewer kinds opened in child WebView windows. So a complete OSC 777
emterm markdown, json, yaml or image, or an OSC 9999 emterm-md, in a suppressed
chunk is delivered once. Kitty APC and SIXEL DCS are inline images: not
delivered, known gap. The OSC 777 agent-status kind is handled by the daemon and
is not delivered.

task0001 confirms this against the GUI's handling of the image kind. If that
kind draws into the terminal grid instead of opening a window, it moves to the
inline-image side and task0001 reports the change. Affected: task0001
(implements and confirms), task0005 (records in the 9e6a468b3a45ceeb rows).

### D4: Where the replacement's tail comes from (FR4, FR5, FR6)
- **Ring-written end with pending**: if the suppressed chunk's last byte lies in
  a ring-written span and the write filter's pending (after this chunk) is
  non-empty, the tail is that pending run. Nothing is extracted from the chunk
  bytes that pending re-delivers, i.e. its chunk-contributed suffix of the
  ring-written spans. Pending excludes nothing else (FR4).
- **Otherwise**: the tail is the classifier's in-progress sequence at the chunk
  end, from its start, which may lie in the retained window. For a CSI, C0 bytes
  are removed (FR3). If the classifier ends in ground, there is no tail.
- **Pending belongs to an aborted sequence**: when the chunk ends outside the
  ring-written spans (alternate screen), pending is never re-sent. In the client,
  the screen-switch ESC already aborted that sequence.

Affected: task0001 (implements), task0002 (guarantees the pending contract).

### D5: FR8 client-apply determination belongs to task0003
How the GUI applies a Snapshot-kind payload decides the alt-pane resume layout.
The questions are whether it starts from the main screen and a ground parser,
and whether a buffer switch inside the payload takes effect with later bytes
drawn on the switched-to screen. task0003 establishes this before changing the
layout and reports the finding. task0005 records the 830f950f39e499fa verdict
as addressed, citing the resume-mode regression tests in D7. Affected:
task0003, task0005.

### D6: Known-gap register (recorded by task0005)
- Inline images (Kitty APC, SIXEL DCS) in a suppressed chunk are not delivered
  (FR7).
- A sequence that started more than N bytes before a suppressed chunk is not
  reconstructed (D2, as-05), unless it is a ring-region string, which pending
  covers.
- The OSC-number u16 overflow in term_core (as-06) is not fixed. The classifier
  treats such an OSC as not-a-query, so it can only miss one.

### D7: Regression-test registry (FR13, AC-1, AC-7)
| stable_id | Test name(s) | TS | Owner |
|-----------|--------------|----|-------|
| 19209420de72b144 | suppressed_alt_osc_color_query_split_inside_st_is_delivered_once; write_filter_holds_string_ending_in_trailing_esc_until_completed | TS-1 | task0001; task0002 |
| 61d33252f22b6fd2 | suppressed_alt_osc_color_query_split_inside_st_is_delivered_once; write_filter_holds_string_ending_in_trailing_esc_until_completed | TS-1 | task0001; task0002 |
| 8d069c589dc21784 | replacement_never_extracts_a_query_consumed_as_charset_designator; replacement_matches_client_reference_for_transition_corpus | TS-2 | task0001 |
| 66d05376ff960d53 | color_query_predicate_matches_client_osc_number_and_theme_rules; replacement_matches_client_reference_for_transition_corpus | TS-2 | task0001 |
| 001161ab9fa3c20b | incomplete_csi_tail_is_resent_without_executed_c0_controls | TS-3 | task0001 |
| c8aa5052b1a02acd | pending_does_not_drop_alt_region_queries_viewer_launches_or_own_tail; viewer_launch_in_suppressed_chunk_is_delivered_exactly_once; write_filter_closes_esc_aborted_strings_and_holds_only_incomplete_tail | TS-4, TS-8, TS-5 | task0001; task0002 |
| 66170217dc5057ce | write_filter_closes_esc_aborted_strings_and_holds_only_incomplete_tail; suppressed_chunk_after_aborted_strings_does_not_resend_delivered_query_or_text | TS-5 | task0002 |
| 3bc1e21fdd8702ef | query_split_across_reads_is_answered_once_at_every_split_position; utf8_split_across_reads_never_prints_a_replacement_character; consecutive_suppressed_chunks_leave_next_forwarded_chunk_intact | TS-6, TS-7 | task0001 |
| 30ee5a7036a7fc6e | query_split_across_reads_is_answered_once_at_every_split_position | TS-6 | task0001 |
| 9e6a468b3a45ceeb (viewer launch) | viewer_launch_in_suppressed_chunk_is_delivered_exactly_once | TS-8 | task0001 |
| 9e6a468b3a45ceeb (inline image) | inline_images_in_suppressed_chunk_are_not_delivered | TS-8 | task0001 |
| 830f950f39e499fa | visibility_resume_restores_alt_screen_mode_and_content; visibility_resume_after_hidden_alt_exit_shows_main_screen; visibility_resume_keeps_main_pane_progress_bar_layout | TS-9, TS-10 | task0003 |
| 9a548939524b405a | suppressed_queries_arrive_after_snapshot_in_order_once_each | TS-11 | task0001 |
| 29ff65b6c01032dc | evaluate_output_target_never_resumes_a_detached_pane; production_visible_resume_delivers_snapshot_before_replacement_and_next_chunk | TS-12 | task0004 |
| 39267160fcf1bb2a | split_osc9_across_two_suppressed_chunks_never_fires_more_than_once (existing, reordered) | TS-13 | task0005 |
| 692921cdd030612d | existing tests located by task0005 (already addressed) | — | task0005 |
| 9a7dc7697c6af992 | existing tests located by task0005 (already addressed) | — | task0005 |
| 5988c2406aa06a7b | existing paused visible-reattach and paused production-resume tests, located by task0005 (already addressed) | — | task0005 |
| aca2b1d612ab97e0 | snapshot_paths_run_concurrently_with_reader_and_resize_without_deadlock | TS-14 | task0005 |

Other test names used in task plans (not tied to a stable_id):
- retained_window_holds_last_n_bytes_across_reads (TS-17, task0001)
- hostile_chunk_replacement_is_linear_and_never_empty (TS-19, task0001)
- resume_pane_with_permit_recovers_a_poisoned_shadow_parser (TS-12, task0004)
- resume_pane_with_permit_hide_show_roundtrip_on_same_connection (TS-12, task0004)

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| The two FR2 implementations (classifier and write filter) drift apart | Medium | High | D1: same oracle, same corpus, both proven against term_core |
| The GUI does not honor a buffer switch in the middle of a Snapshot payload | Medium | High | D5: task0003 establishes the behavior before choosing a layout. If no single-chunk layout works, it reports a plan deviation instead of changing the client |
| The client routes OSC numbers to the theme differently from the daemon's mirror | Medium | Medium | task0001 derives the routing from the client code. An OSC it cannot route counts as not-a-query (a miss, never a fabrication) |
| Merge conflicts in the large shared test files | High | Low | Tests placed next to related tests (Conventions); helpers unchanged |
| Sibling-task tests break when the builder changes | Medium | Medium | Suppressed-chunk delivery contract restricts their inputs to builder-invariant cases |
| The retained-window copy adds reader latency (NFR3) | Low | Medium | At most 256 bytes copied per read, and no scan on the normal path |

## Open Questions
- [ ] D3: confirm that the OSC 777 emterm image kind opens a viewer window (task0001).
- [ ] D5: how the GUI applies a buffer switch inside a Snapshot-kind payload (task0003).
