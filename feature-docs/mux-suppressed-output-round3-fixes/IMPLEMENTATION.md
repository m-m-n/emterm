# Implementation Plan: mux-suppressed-output-round3-fixes

## Overview

Fix the eight medium findings left `unresolved` in review round 1 of
mux-suppressed-output-round2-fixes, so that a suppressed chunk's side
effects reach the same client parse state as the raw stream and the normal
write path pays no per-OSC copy, and record a decision per finding. Five
tasks run fully in parallel; this document pins only what two or more of
them share.

## Technology Stack

- **Language / crate**: Rust, the existing `src-tauri` crate. Every touched
  module is CLI-shared (`crate::mux`, not gated on the `gui` feature).
- **Client-parity reference**: the `term_core` parser transitions (SPEC
  as-01), used as the rule basis and as the test oracle.
- **New dependencies**: none. `project.license` (MIT) is unaffected; no
  dependency license needs recording.

## Layer Structure

| Layer | Location | Responsibility | May depend on |
|-------|----------|----------------|---------------|
| OSC identification (leaf) | `src-tauri/src/mux/osc_identify.rs` | OSC number recovery and identification; gains the allocation-free entry (FR8) | `crate::viewer_kinds` only |
| Shared strip | `src-tauri/src/mux/scrollback_filter.rs` | Strip selection for ring writes and snapshot assembly; designator-aware (FR2/FR3) | OSC identification |
| Write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` | Holds incomplete constructs across reads; cut, drain and overflow paths (FR1/FR2) | shared strip |
| Reader | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` | Capture step (FR4 fallback), forward decision, suppressed pipeline (FR6) | all layers in this table |
| Suppressed path | `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs` | Client-parity scan (FR5) and replacement assembly (FR6 split) | OSC identification, shared strip helpers |
| Boundary record | `src-tauri/src/mux/session/pane/output_capture.rs` | Per-destination boundary, construct and generation (FR6/FR7) | nothing in `mux::ipc` |

Dependency direction is unchanged: `mux::session` never depends on
`mux::ipc`, and `osc_identify` depends on neither `mux::ipc` nor a
`gui`-gated item (NFR6).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Replayable-OSC decision (`scrollback_filter.rs`: `is_replayable_osc_body`) | Decide whether an OSC body is stripped from the ring and snapshot | Name, signature and true/false meaning unchanged: true exactly when the body identifies as a viewer launch, a markdown launch or an agent-status report under the round2 recovery rule. After this feature it is answered by the allocation-free identification and allocates nothing. The strip calls it exactly where it does today. | task0001 (re-implements its body), task0002 (strip changes around the call site) |
| Allocation-free OSC identification (`osc_identify.rs`, new entry) | Identify an OSC body without heap allocation | Input: the OSC body bytes (after the introducer, terminator excluded), any byte values. Output: the same identity value as recovering then identifying with the kept reference pair (`recover_osc`, `identify_osc`) for every input. No heap allocation, one bounded pass, never panics. The reference pair stays with unchanged behavior for delivery-side callers and tests. | task0001 (owner), task0002 (reached through the strip) |
| Shared strip with an initial state (`scrollback_filter.rs`) | Strip rich content from a byte run in one pass, designator-aware | The three existing entry points (`strip_replayable_rich_content`, `strip_pty_output_for_scrollback_write`, `strip_rich_content_and_remap`) keep their signatures and mean "start in ground"; the snapshot side keeps calling them. A state-taking form accepts one flag, "byte 0 is a pending charset designator". Postconditions: at top level, `ESC (` / `ESC )` and the byte after it are copied verbatim and never start a strip target, even when that byte is ESC; with the flag set, byte 0 is copied verbatim; every copied byte keeps a one-to-one remapped offset; still a single bounded pass. | task0002 (owner), task0001 (same file, separate region) |
| Write filter feed (`write_filter.rs`: `feed_with_cuts`, `pending`, carried-over completion) | Bytes safe for the ring, the held incomplete construct, and a carried-over completion | Signatures and result shapes unchanged; the `pending` postcondition is unchanged. Changed: bytes of an OSC/DCS/APC string or held lone ESC closed at a cut, from its opening ESC on, are absent from the emitted bytes (FR1); every strip call receives the awaiting-designator state in effect at its start (FR2). A carried-over completion is reported exactly as before; its sequence bytes are still readable from it. | task0002 (owner), task0004 (pipeline reads `pending` and the completion) |
| Capture-step results (`mod.rs` capture closure) | Per-chunk facts handed to the forward decision | Still yields, per chunk: title change, ring-written live spans, carried-over completion, and the chunk number from the capture exclusion. On the fallback path with `alt_before` or `alt_after` true: the write filter has closed `pending` as at a cut at fed position 0, the live spans are empty and no carried-over completion is reported (FR4). | task0002 (owner), task0004 (consumer) |
| Client-parity scan (`client_parity_scan.rs`: `scan`, `derive_prefix_start`) | Items and tail of a suppressed chunk under client parity | Signature and result shape unchanged; the meaning of the combined buffer and the window/chunk boundary is unchanged. The FR5 rule only removes an item or tail whose construct starts at the chunk's first byte. Call-site rewrites forced by an internal signature change are allowed. | task0003 (owner), task0004 (through the builders) |
| Replacement builders (`suppressed_output.rs`: `build_suppressed_replacement`, `build_suppressed_replacement_for`) | Assemble a suppressed chunk's replacement output | Signatures and results unchanged for every input. Internally they become "prepare (scan, items, tail, outside every exclusion) then finalize (tail-omission decision from a construct)"; composing the two reproduces today's result. | task0004 (owner), task0003 (tests call them) |
| Boundary record (`output_capture.rs`) | Per-destination boundary, construct and generation | Record entry points (`record_boundary`, `record_boundary_with_construct`, guard `record`, `record_with_construct`) keep their signatures, so the snapshot call sites are untouched. The covered query keeps its construct value and additionally carries the destination's generation. Rules: insert, higher boundary and equal boundary each store the later construct (none included) and advance the generation; a lower boundary changes nothing. Generations never repeat within one pane's capture state. O(1) per record and per covered check. | task0004 (owner), task0002 and task0003 (their tests drive single snapshots through the unchanged record paths) |
| Reader-level test oracle (`pty_spawn/tests.rs` top-level helpers) | Feed client and reference cores and compare | The existing top-level helpers (reader drivers, client view, reference core, response/screen comparison, prefix-reproduction guard, sweeps) are reused unchanged from child test modules. No task edits them or the inline `fr8_snapshot_tail` module; a task that needs a visibility-restore driver writes its own in its own child module. | task0002, task0003, task0004 |

## Conventions

### Region ownership in shared files

| File | Region | Owner |
|------|--------|-------|
| `src-tauri/src/mux/scrollback_filter.rs` | `is_replayable_osc_body` and the `osc_identify` import line | task0001 |
| `src-tauri/src/mux/scrollback_filter.rs` | everything else (strip entry points, the single-pass strip) | task0002 |
| `src-tauri/src/mux/ipc/pty_spawn/mod.rs` | the capture closure (main-span extraction, fallback, write-filter feed) | task0002 |
| `src-tauri/src/mux/ipc/pty_spawn/mod.rs` | the forward decision, both suppressed call sites, the reader-forward enum, the suppressed pipeline and its imports | task0004 |
| `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | expectation updates of the FR1/FR4 tests listed below; one module line right after the comment "END round2 task0003 tests: pre-existing entry points." | task0002 |
| `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | one module line right after the comment "END round2 task0003 tests: new entry points." | task0003 |
| `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | one module line at the end of the file | task0004 |

New tests live in new child files under
`src-tauri/src/mux/ipc/pty_spawn/tests/`, one per task, so that parallel
tasks do not edit the same lines.

### Test naming and rename policy

- Registry tests use `round3_<first 8 hex of stable_id>_<behavior>`.
- An existing test is renamed only when its name states the superseded
  expectation. A rename of a test listed in a predecessor's
  `test-docs/*/taskNNNN.tests.yaml` updates that record per
  `.claude/rules/test-docs-records.md`, with a supersede note when the
  expectation is inverted. Rewriting call sites for signature changes is not
  a behavior change.

### Reader-level oracle convention

Responses are always compared with the raw-stream reference. Screen, cursor
and displayed characters are compared unconditionally for every FR1 case,
which runs on the visibility-restore layout. For other cases the existing
prefix-reproduction guard may skip the screen comparison only when the
mismatch lies in the stand-in snapshot itself; each use is stated in the
task's test record.

### Logging and errors

No new logging on the reader's normal path. No new error type; every
changed scan stays panic-free on any input (NFR5).

### Test-only allocation counting

The FR8 allocation counter is a test-build-only global allocator wrapper
that counts only while a thread-local flag is armed on the calling thread.
It is active for the whole lib test binary, so it must add no allocation,
no lazy initialization and no destructor of its own, and must not change
any other test's behavior.

## Cross-task Design Decisions

### D1: One shared strip for ring and snapshot

FR2 and FR3 are met by one change in the shared strip, so the ring write
and the snapshot strip cannot diverge. The write filter supplies the
initial designator state; the snapshot side always starts in ground.
Affects task0002 (owner) and task0001 (same file).

### D2: Closed constructs are dropped, not terminated

A construct closed at a cut, or by the FR4 fallback closing (a cut at fed
position 0), is not written to the ring at all, and no terminator is
appended (SPEC FR1, as-05). An awaiting-designator `ESC (` at a cut keeps
its round2 handling (as-02). Affects task0002 (owner) and task0004 (reads
`pending` and the carried-over completion, whose contracts do not change).

### D3: The tail decision is re-checked at send time

The suppressed pipeline secures its send slot outside every exclusion,
then re-takes `output_target` and the boundary exclusion, re-checks the
destination's record, and finalizes and sends under that hold (SPEC FR6
data flow). Work under the hold is limited to the record comparison,
selecting or assembling the final bytes, and the non-blocking send.
Affects task0004 (owner); task0002 and task0003 rely on the pipeline
contract only through their reader-level tests.

### Regression test registry

| stable_id | Requirement | Owner | File | Test path |
|-----------|-------------|-------|------|-----------|
| 973af79f520bdaa5 | FR1 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` | `mux::ipc::pty_spawn::tests::round3_write_path::round3_973af79f_osc_closed_by_a_removed_switch_is_never_completed_by_a_later_bel` |
| 195916fd94088018 | FR2 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` | `mux::ipc::pty_spawn::tests::round3_write_path::round3_195916fd_write_filter_output_is_split_invariant_across_a_designator_esc` |
| 76342d8d941e6416 | FR3 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` | `mux::ipc::pty_spawn::tests::round3_write_path::round3_76342d8d_designator_esc_never_starts_a_strip_target_in_either_strip` |
| 2a9d929f4fafb7cb | FR4 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` | `mux::ipc::pty_spawn::tests::round3_write_path::round3_2a9d929f_straddling_switch_fallback_closes_the_held_construct` |
| eaf83fe08869d5e6 | FR5 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_as05.rs` | `mux::ipc::pty_spawn::tests::round3_as05::round3_eaf83fe0_as05_fallback_never_fabricates_after_a_trailing_esc_paren` |
| 6230e66b979e312e | FR6 | task0004 | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_send_recheck.rs` | `mux::ipc::pty_spawn::tests::round3_send_recheck::round3_6230e66b_tail_omission_uses_the_record_current_at_send_time` |
| 240031761bfdf695 | FR7 | task0004 | `src-tauri/src/mux/session/pane/tests.rs` | `mux::session::pane::tests::round3_24003176_equal_boundary_takes_the_later_record` |
| b3e644c5e2d31809 | FR8 | task0001 | `src-tauri/src/mux/osc_identify.rs` (tests module) | `mux::osc_identify::tests::round3_b3e644c5_osc_identification_is_allocation_free_and_matches_the_reference` |

Each owner creates a test with exactly the pinned path. It fails on the
pre-fix code and passes after the fix (SPEC AC-1). Other tests may use any
name.

### Changed-expectation registry (SPEC AC-8)

| Old name | New name | File | Reason | Owner |
|----------|----------|------|--------|-------|
| `a_cut_closes_an_in_progress_osc_and_pending_holds_no_closed_osc` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the closed OSC's bytes are no longer emitted | task0002 |
| `a_cut_closes_a_held_lone_esc` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the closed lone ESC is no longer emitted | task0002 |
| `an_empty_fed_range_with_a_cut_still_closes_pending` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the closed pending run is no longer emitted | task0002 |
| `pending_after_a_cut_equals_a_fresh_scan_of_the_bytes_after_the_last_cut` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: emitted bytes before the cut lose the closed construct; the pending expectation is unchanged | task0002 |
| `reader_closes_pending_at_a_chunk_that_starts_with_a_switch_to_the_alt_screen` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the ring no longer holds the closed OSC | task0002 |
| `removed_switch_between_main_ranges_delivers_the_alt_query_once_and_sends_no_tail` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the ring holds only the bytes after the closed OSC | task0002 |
| `fr4_removed_switch_stream_matches_the_reference_over_split_positions` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the expected ring bytes lose the closed OSC | task0002 |
| `fallback_path_derives_no_cuts_and_keeps_the_whole_chunk_gate` | `fallback_path_keeps_the_whole_chunk_gate` | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR4: the alt-involved fallback now derives a cut; the test's assertion is unchanged | task0002 |
| `ac2_equal_boundary_with_a_different_construct_yields_none` | `ac2_equal_boundary_with_a_different_construct_takes_the_later_record` | `src-tauri/src/mux/session/pane/tests.rs` | FR7: an equal boundary keeps the later record's construct | task0004 |

The four names kept from SPEC AC-8 still describe the new behavior, so
they are not renamed and no predecessor record changes for them. Only the
FR7 rename touches a predecessor record
(`test-docs/mux-suppressed-output-round2-fixes/task0004.tests.yaml`,
AC-2). A test whose expectation changes but is not in this table is a
reportable deviation.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflicts in `scrollback_filter.rs`, `mod.rs`, `pty_spawn/tests.rs` | medium | low | Region ownership table; new tests in per-task child files; module lines at distinct anchors |
| Strip and write-filter boundary scan disagree on designator handling | medium | high | Both follow the term_core transition; task0002's split-invariance sweep compares the filter's emitted bytes against the single-call result |
| The test-only global allocator disturbs other tests | low | medium | Counts only while armed on the calling thread; no allocation, lazy init or destructor of its own; full lib suite must pass |
| Deadlock or a blocking wait under `output_target` / the boundary exclusion | low | high | Slot secured before any lock; task0004 has a full-channel test proving both locks are takeable while the reader waits |
| Decision table drifts from the tests actually written | medium | low | Names pinned above; verification resolves each pinned path with the libtest list |
| vt100 shadow parser and term_core disagree on a switch form | low | medium | Reader-level tests compare against term_core fed the raw stream |
| FR5 misses a real query after an even `ESC (` chain | certain (by design) | low | Accepted miss (SPEC FR5: misses only, never fabrication); tests assert no fabrication and do not assert parity for the even chain |

## Open Questions

- [ ] None.
