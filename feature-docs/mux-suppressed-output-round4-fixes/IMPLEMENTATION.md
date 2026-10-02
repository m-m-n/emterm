# Implementation Plan: mux-suppressed-output-round4-fixes

## Overview

This feature resolves the three medium findings left unresolved in review
round 1 of mux-suppressed-output-round3-fixes (FR1-FR3). It checks two
adjacent paths with term_core-based regression tests and fixes them if they
reproduce (FR4, FR5), and it records every outcome (FR6). Five tasks run
fully in parallel. This document pins only what two or more of them share.

## Technology Stack

- **Language / crate**: Rust, the existing `src-tauri` crate. Every touched
  module is CLI-shared (`crate::mux`, not gated on the `gui` feature).
- **Client-parity reference**: the `term_core` parser transitions (SPEC
  as-01). They are the rule basis and the test oracle.
- **New dependencies**: none. `project.license` (MIT) is unaffected; no
  dependency license needs recording.

## Layer Structure

| Layer | Location | Responsibility in this feature | May depend on |
|-------|----------|--------------------------------|---------------|
| Main-span extraction | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` (`extract_main_buffer_bytes`, the toggle list) | Main-buffer spans of a raw chunk. Designator-slot handling if FR5 reproduces | nothing new |
| Write filter | `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs` (`ScrollbackWriteFilter`, `scan_boundary`, `CarriedCompletion`) | Holds incomplete constructs and chains. Owns the cut, drain and overflow paths (FR1, FR3, FR4) | the shared strip in `src-tauri/src/mux/scrollback_filter.rs` (unchanged) |
| Reader capture step | `src-tauri/src/mux/ipc/pty_spawn/mod.rs` (capture closure, `cuts_from_main_spans`) | Cut derivation, the fallback closing and the feed (FR3 comment, FR5) | extraction, write filter |
| Suppressed path | `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs`, `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs` | Client-parity scan (FR2). Tail and excluded pieces (FR1 alignment) | the write filter's `pending()` (read-only) |

The dependency direction is unchanged. No task changes the shared strip
(`scrollback_filter.rs`), `osc_identify.rs`, `output_capture.rs`, the
snapshot assembly (`src-tauri/src/mux/snapshot_bytes.rs`) or anything under
`crates/mux_ipc/` (NFR1). Needing to change one of them is a reportable
plan deviation.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Boundary scan (`write_filter.rs`: `scan_boundary` and its result) | Classify the end of a run, for the hold and for the cut | One forward pass from a given start position and start state (the designator skip; the carried CSI state, FR4 only). It never panics, and its work is linear in the bytes walked. It reports: (a) the hold boundary, which is the chain head of the single incomplete construct at the end of the run, or the run end when nothing is incomplete (FR1); (b) the construct start, the opening ESC of that incomplete construct (FR1); (c) whether the run ends awaiting a designator (meaning unchanged); (d) the carried-completion end of the construct at the given carried start position (meaning unchanged; that position is now the stored construct start); (e) FR4 only, if it reproduces: whether the run ends inside a CSI, with the O(1) CSI sub-state needed to continue in the next feed. "Incomplete construct", (c) and (e) are mutually exclusive end states. The string, `ESC ESC`, `ESC (` / `ESC )` and lone-ESC rules are unchanged except where D1 and D3 extend them. | task0001 (owns a, b, d), task0004 (owns e), task0002 (reads c, including on the overflow path) |
| Write filter feed (`ScrollbackWriteFilter`: `feed`, `feed_with_cuts`, `pending`, `FeedOutcome`) | Bytes safe for the ring, the held run, the carried-over completion | Signatures, the `FeedOutcome` shape and the dims attribution are unchanged. After every call the filter is in exactly one end state: (1) ground; (2) holding a chain, where `pending` is non-empty and holds exactly the chain that ends in the single incomplete construct at the end of the fed stream after its last cut (FR1 postcondition); (3) awaiting a designator, with `pending` empty; (4) FR4 only: inside a CSI, with `pending` empty. At a cut, closing follows D2. The overflow escape hatch keeps the 512 KiB cap, and a held chain counts toward it. The flushed run still goes through the strip. Afterwards `pending` is empty, no chain head or construct start is stored, and the awaiting flag (and FR4's CSI state) equals the client-parity state at the end of the flushed run. | task0001, task0002, task0004 (each its own region, see Conventions); consumers: the capture step and the suppressed pipeline |
| Carried-over completion (`CarriedCompletion`: sequence bytes, fed end) | Report a held construct that completes in this call before any cut | Accessor names and meanings are unchanged. The sequence bytes are exactly the construct, from its own opening ESC (the construct start, never the chain head) through its terminator. The fed end is the fed offset just past the terminator. When a completion is reported, the whole held run, chain included, settles and is emitted. Nothing is reported for an aborted chain link, a construct closed by a cut, a still-incomplete construct, or a call that took the overflow flush. | task0001 (owner); consumers: the capture step and `suppressed_output.rs`, at unchanged call sites |
| Held run as the suppressed tail (`pending()` read by the suppressed pipeline; `prepare_suppressed_replacement`) | Re-deliver the held run and keep it out of item scanning | `pending()` returns the whole held chain. The re-delivered tail and the excluded pieces are derived from its length by today's rule; the chain is a contiguous suffix of the fed stream after the last cut. No byte is both a replacement item and part of the re-delivered tail. | task0001 (owner); task0003 (its scan receives the excluded pieces unchanged) |
| Capture step (`mod.rs` capture closure, `cuts_from_main_spans`) | Per-chunk extraction, cut derivation, fallback closing and feed | Per chunk it still yields the title change, the ring-written live spans and the carried-over completion. On the non-fallback path, cuts come from the main-span list (rule unchanged unless FR5 reproduces). On the fallback path with the alternate screen involved, it feeds an empty range with one cut at fed 0, so D2 applies: the held chain is dropped, at most one closing write reaches the ring, the live spans are empty and no completion is reported. | task0002 (owns the fallback comment), task0005 (owns the extraction call, the shadow-parser cross-check and the cut derivation) |
| Main-span extraction (`extract_main_buffer_bytes`) | Main-buffer bytes, final alternate-screen state, span list | The result shape is unchanged. The borrowed result for a chunk without toggles is kept. Ordinary toggles, which sit outside a designator slot, are removed exactly as today. FR5 only, if it reproduces: a toggle whose ESC term_core consumes as a designator is not a toggle. This also holds when `ESC (` / `ESC )` ended the previous read; the reader carries that O(1) state. | task0005 (owner); task0002 and task0004 tests rely on today's extraction of ordinary switches |
| Client-parity scan (`client_parity_scan.rs`: `scan`) | Items and tail of a suppressed chunk | The signature and result shape are unchanged. Behavior outside the as-05 fallback condition is unchanged. Each reading is a single bounded pass. Inside the fallback, items and the tail follow the FR2 intersection rule. | task0003 (owner); task0001's suppressed-path tests call it unchanged |
| Reader-level oracle harness (`src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` helpers) | Drive the production reader through a visibility restore and compare with term_core | The helpers listed under Conventions are widened to parent-module visibility, so sibling test modules can use them. Behavior, signatures and names do not change. Every task that needs them makes the identical edit (only the visibility qualifier changes), so parallel edits merge cleanly. | task0001, task0002, task0004, task0005 |
| Decision record (`feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md`) | FR6 record | Skeleton pinned in D5. Each task fills only its own regions. | all tasks |

## Conventions

### Region ownership in shared files

| File | Region | Owner |
|------|--------|-------|
| `write_filter.rs` | String, `ESC ESC` and lone-ESC handling in `scan_boundary`; the chain-head and construct-start results; the hold rule on the normal drain and the drop point at a cut; `CarriedCompletion`; the chain-state reset on overflow; the postcondition docs of `feed_with_cuts` and `pending()` | task0001 |
| `write_filter.rs` | The closing ESC write in the cut path, the empty-segment path and the overflow-then-cut path; the awaiting-designator paragraph of the `feed_with_cuts` doc | task0002 |
| `write_filter.rs` | The `ESC [` arm of `scan_boundary` (the CSI walk), the carried CSI state, the CSI closing write | task0004 |
| `write_filter.rs` | `extract_main_buffer_bytes` and the toggle list | task0005 |
| `mod.rs` | The comment on the alternate-screen fallback branch of the capture step | task0002 |
| `mod.rs` | The extraction call, the cross-check against the shadow parser, `cuts_from_main_spans`, any reader-local designator-slot state | task0005 |
| `suppressed_output.rs` | all | task0001 |
| `client_parity_scan.rs` | all | task0003 |
| `pty_spawn/tests.rs` | Expectation updates of the four FR1 tests in the registry below; one module line directly after the line `mod round3_write_path;` | task0001 |
| `pty_spawn/tests.rs` | Expectation updates of the two FR3 tests in the registry below; one module line directly after the line `mod round3_as05;` | task0002 |
| `pty_spawn/tests.rs` | One module line directly before the comment line containing "mux-suppressed-output-round2-fixes task0004 (FR8, finding" | task0003 |
| `pty_spawn/tests.rs` | One module line at the end of the file, after the line `mod round3_send_recheck;` | task0004 |
| `pty_spawn/tests.rs` | One module line directly before the comment line containing "mux-suppressed-output-round2-fixes task0003: write-filter parity," | task0005 |
| `tests/round3_write_path.rs` | The harness visibility widening | the identical edit by task0001, task0002, task0004, task0005 |
| `tests/round3_write_path.rs` | The EC-2 parts of `a_cut_drops_the_construct_from_its_opening_esc_on` | task0002 |
| `DECISIONS.md` | The regions named in D5 | each task its own |

New tests live in new child files under
`src-tauri/src/mux/ipc/pty_spawn/tests/`, one per task. Conflicts in
`write_filter.rs` are expected, because the cut path is shared. The later
merger resolves them by parent-side adoption and re-expresses its own region
against D1, D2 and the Shared Components contracts.

### Harness helpers widened to parent-module visibility

`DIMS`, `switch_pairs`, `QueryResponder`, `new_core`, `client_view`,
`reference_view`, `assert_client_equals_reference`,
`spawn_reader_keeping_target`, `RestoreRun` and its two fields,
`run_visibility_restore_at`, `run_reader_without_owner` and
`osc_introducers`. Nothing else in that file changes for this purpose.

### Test naming and rename policy

- Registry tests use `round4_<first 8 hex of stable_id>_<behavior>` for
  FR1-FR3, and `round4_fr4_<behavior>` / `round4_fr5_<behavior>` for the
  adjacent paths.
- An existing test is renamed only when its name states the superseded
  expectation. When a test listed in a predecessor's
  `test-docs/*/taskNNNN.tests.yaml` is renamed or its expectation inverted,
  that record is updated per `.claude/rules/test-docs-records.md` (SPEC
  as-06). The supersede note names
  `feature-docs/mux-suppressed-output-round4-fixes/SPEC.md` and the FR ID,
  and `red_reason` stays unchanged. Every updated name resolves in the
  libtest listing (the rule's resolution check). Rewriting a call site for a
  signature change is not a behavior change.

### Reader-level oracle convention

The reference is term_core fed the concatenated chunks once, with the same
responder. The client applies the snapshot through
`reset_and_replay_segments` with its responses discarded, then processes
every `PtyOutput` chunk fully. Responses are always compared. For FR1, FR4
and FR5 cases on the visibility-restore layout (ring not wrapped), screen,
cursor and displayed characters are compared too. When an FR3 case's switch
sits in the designator slot, task0002 compares responses and the parse of
the bytes after the closing, but not screen differences that come only from
FR5's path (SPEC AC-2, EC-5).

### Logging, errors, hot path

- No new logging on the reader's normal path; the existing overflow warning
  stays. No new error type. Every changed pass is panic-free on any input
  (NFR5).
- No new pass over a chunk on the reader's normal path (NFR3). Chain-head,
  construct-start and CSI tracking live inside the existing boundary scan.
  Designator-slot tracking lives inside the existing extraction pass.
  Closing writes are O(1).

## Cross-task Design Decisions

### D1: A chain is held from its head (FR1)

A chain is a run of consecutive constructs, each closed by the ESC that
opens the next one (SPEC as-03). Two kinds of link exist:

- an OSC, DCS or APC string aborted by `ESC <byte other than \>`, whose
  aborting ESC opens another string or a lone ESC;
- the superseded first ESC of `ESC ESC`.

The chain ends, and the bytes before the closing ESC settle, when a
construct is closed by an ESC that starts something else: a complete
non-string escape, or an `ESC (` / `ESC )` designation.

`pending` holds the bytes from the chain head on. This applies on the
normal drain path as well as at a cut, so the chain head is never written
on the read that opened it. When the final construct completes, the whole
held run settles. The scan of a held run resumes at the stored construct
start, because the chain prefix before it is already fixed. Per-call work
is therefore the same as for a single held construct.

Affects task0001 (owner). task0002 and task0004 rely on "a non-empty
`pending` excludes the awaiting and inside-CSI states".

### D2: Closing order at a cut

This order applies wherever a cut closes the filter's state:

- a cut inside a call;
- a cut at an empty segment;
- a cut that follows an overflow flush in its segment;
- the `mod.rs` fallback closing (empty fed range, cut at 0).

The filter does, in order:

1. Emits the settled bytes before the chain head through the strip, with
   the awaiting-designator flag in effect at the run's start (unchanged
   rule). Nothing from the chain head on is emitted (FR1).
2. Emits at most one closing write after those bytes. It writes one ESC
   byte when the emitted stream ends awaiting a designator (FR3). If FR4
   reproduces, it writes the CSI closing when the stream ends inside a CSI.
   The two conditions never hold together, and neither holds when step 1
   dropped a chain.
3. Resets to ground: `pending` empty, no chain head or construct start, the
   awaiting flag and the CSI state cleared. The next fed byte starts from
   ground.

After an overflow flush in the last segment, no closing is written because
no cut follows; the end state is carried instead.

Affects task0001 (step 1 and the chain-state part of step 3), task0002 (the
ESC in step 2 on every path) and task0004 (the CSI closing in step 2).

### D3: An open CSI is closed, never held (FR4)

If FR4 reproduces, its fix writes closing bytes at the cut. It does not
hold CSI bytes in `pending`, so the pending contract stays task0001's chain
contract. task0004 chooses the closing bytes. They must meet three
conditions:

- term_core cancels the CSI without dispatch and returns to ground, with no
  displayed character, cursor move or response;
- they start no strip target in either strip;
- the term_core oracle confirms both.

### D4: Adjacent paths are test-first and bounded (FR4, FR5)

task0004 and task0005 first write the regression test against the
pre-change code and record whether it reproduces.

- If it does not reproduce, they keep the test, make no production change
  and record "not reproducing".
- If it reproduces, they fix it within the task's fix boundary and record
  "reproduced and fixed". The boundary for FR4 is the write filter. For FR5
  it is extraction, cut derivation and the capture step.

If parity needs a change outside the boundary (for example the shadow
parser's alternate-screen state, the snapshot assembly or the wire format),
the task stops at the boundary. It records the residual and its cause in
DECISIONS.md and reports a plan deviation.

### D5: Decision record skeleton (FR6)

Every task writes the same skeleton and fills only its own regions; the
first branch to merge creates the file. An unfilled region reads
`pending (taskNNNN)` with its owner's ID. In order:

1. The title `# mux-suppressed-output-round4-fixes: Decisions (DECISIONS.md)`
   and this intro paragraph: "Decisions for the three medium findings left
   unresolved in review round 1 of mux-suppressed-output-round3-fixes
   (PR #112), and the outcome of the two adjacent paths this feature
   checked."
2. `## Decision table`: one table with the columns stable_id, requirement,
   verdict, rationale and regression test. Rows, in this order:
   3e2024dce619ed9f / FR1 (task0001), 989ec5c588abce06 / FR2 (task0003),
   48caec6f5b0b5810 / FR3 (task0002). The verdict is `resolved` or
   `no change needed`. The regression test cell equals the registry path
   below, character for character.
3. `## Adjacent paths`, with `### FR4: in-progress CSI at a cut` (task0004)
   and `### FR5: switch sequence in the designator slot` (task0005). Each
   states "reproduced and fixed" or "not reproducing", plus any residual per
   D4, and its registry test path.
4. `## Behavior-changing tests`, with one subsection per requirement in the
   order FR1 (task0001), FR2 (task0003), FR3 (task0002), FR4 (task0004),
   FR5 (task0005). Each subsection holds a table with the columns old name,
   new name, file and reason, or the word None. A line naming the
   predecessor `test-docs/` records it updated (or None) follows the table.
5. `## Review round 1 record`, with one fixed sentence:
   "`feature-docs/mux-suppressed-output-round3-fixes/reviews/round1.yaml` is
   not modified by this feature."

### Regression test registry

| Finding / path | Requirement | Owner | File | Test path |
|----------------|-------------|-------|------|-----------|
| 3e2024dce619ed9f | FR1 | task0001 | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_chain.rs` | `mux::ipc::pty_spawn::tests::round4_chain::round4_3e2024dc_a_chain_closed_by_a_cut_is_never_completed_by_a_later_read` |
| 989ec5c588abce06 | FR2 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_as05.rs` | `mux::ipc::pty_spawn::tests::round4_as05::round4_989ec5c5_as05_fallback_never_fabricates_after_a_designator_chain_inside_the_chunk` |
| 48caec6f5b0b5810 | FR3 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_cut.rs` | `mux::ipc::pty_spawn::tests::round4_designator_cut::round4_48caec6f_awaiting_designator_at_a_cut_writes_the_consumed_esc` |
| adjacent: in-progress CSI at a cut | FR4 | task0004 | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs` | `mux::ipc::pty_spawn::tests::round4_cut_csi::round4_fr4_in_progress_csi_at_a_cut_matches_the_raw_stream_reference` |
| adjacent: switch in the designator slot | FR5 | task0005 | `src-tauri/src/mux/ipc/pty_spawn/tests/round4_designator_slot.rs` | `mux::ipc::pty_spawn::tests::round4_designator_slot::round4_fr5_switch_in_the_designator_slot_matches_the_raw_stream_reference` |

Each owner creates a test at exactly the pinned path. The FR1-FR3 tests
fail on the pre-fix code and pass after the fix (SPEC AC-1). The FR4 and
FR5 tests fail on the pre-change code exactly when the path reproduces
(SPEC AC-6). Other tests may use any name.

### Changed-expectation registry (SPEC AC-7)

| Old name | New name | File | Reason | Owner |
|----------|----------|------|--------|-------|
| `write_filter_closes_esc_aborted_strings_and_holds_only_incomplete_tail` | `write_filter_holds_an_esc_aborted_chain_from_its_head` | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the aborted OSC and DCS chain into the incomplete APC. The whole run is held from the OSC instead of the aborted strings being flushed | task0001 |
| `write_filter_double_esc_treats_the_second_esc_as_the_introducer` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the superseded first ESC is held with the OSC and emitted with it on completion | task0001 |
| `write_filter_boundaries_are_split_position_independent_and_match_term_core` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the allowed pending shapes now include a chain that starts with a superseded ESC. Split invariance and the term_core open/closed oracle are unchanged | task0001 |
| `write_filter_hostile_aborted_introducers_stream_is_linear_and_holds_only_the_final_pair` | `write_filter_hostile_aborted_introducers_stream_is_linear_and_held_as_one_chain` | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR1: the 64 KiB stream is one chain below the cap and is held whole. Linearity is unchanged | task0001 |
| `a_cut_clears_the_awaiting_designator_flag` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR3: one ESC follows the awaiting `ESC (` at the cut, also for a wait carried in from an earlier read | task0002 |
| `pending_after_a_cut_equals_a_fresh_scan_of_the_bytes_after_the_last_cut` | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | FR3: the expected emitted bytes gain one ESC after each segment that ends awaiting a designator and is followed by a cut. The pending expectation is unchanged | task0002 |
| `a_cut_drops_the_construct_from_its_opening_esc_on` (EC-2 parts only) | unchanged | `src-tauri/src/mux/ipc/pty_spawn/tests/round3_write_path.rs` | FR3: the awaiting `ESC (` at a cut is followed by one ESC, in the same call and when carried from an earlier read | task0002 |

Predecessor records to update:

- **task0001**, in `test-docs/mux-suppressed-output-fixes/task0002.tests.yaml`:
  - AC-2 and AC-6 get the new names, each with a supersede note naming FR1.
  - AC-3 gets a supersede note naming FR1; its test names are kept.
  - AC-5 is unchanged, because its expectation is not inverted.
- **task0002**: AC-2 of
  `test-docs/mux-suppressed-output-round2-fixes/task0003.tests.yaml` and
  AC-3 of `test-docs/mux-suppressed-output-round3-fixes/task0002.tests.yaml`
  each get a supersede note naming FR3. The names are kept.

Some other test may change its expectation too, for example one that pins
the pending content of an aborted chain, the designator state on the
overflow path, or bytes at a cut that FR4 or FR5 change. The owning task
adds it to its Behavior-changing tests subsection in DECISIONS.md and
reports a plan deviation.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflicts in the cut path of `write_filter.rs` (task0001, task0002, task0004, task0005) | high | medium | D2 fixes the order of the three steps. The region table is pinned. Parent-side adoption re-expresses each region against the contracts, and each task re-runs its own tests after adoption |
| Holding chains changes more existing expectations than the registry lists | medium | medium | The deviation rule above. Split-invariance and fresh-filter oracles are self-referential and adapt without edits |
| Rescanning a held chain on every tiny read becomes quadratic | medium | high | D1: the scan resumes at the stored construct start. TS-10 feeds the chains byte by byte within a time budget |
| FR5 parity is out of reach within extraction, because the vt100 shadow parser enters the alternate screen on a designator-slot switch that term_core prints as text, and the reader cross-checks extraction against that state | medium | medium | D4: stop at the boundary, record the residual, report a deviation. FR3's ESC still prevents fabrication on that path (EC-5) |
| FR4's closing bytes are misread by the strip or the snapshot side | low | high | D3 conditions. The term_core oracle and both strips are checked over the closing bytes |
| A construct or chain longer than 512 KiB is flushed open, and a later cut cannot drop it | certain (pre-existing escape hatch) | low | Accepted: SPEC keeps the cap and the strip-filtered flush (NFR5). The post-flush state is defined and tested |
| The FR2 intersection drops a real query in the fallback | certain (by design) | low | Accepted (SPEC FR2: misses only, never fabrication). Tests assert no fabrication and that agreed items are kept |
| The decision record drifts from the tests actually written | medium | low | Names are pinned above. Verification resolves every registry path in the libtest listing |

## Open Questions

- [ ] FR5 may not be fixable inside extraction and cut derivation: the
      shadow parser's alternate-screen state disagrees with term_core on a
      designator-slot switch. The task follows D4.
- [ ] Reading the code, FR4 is expected to reproduce. The ring can hold
      `ESC[6` followed, across a removed switch, by a later `n`. The
      decision record states what the test showed.
