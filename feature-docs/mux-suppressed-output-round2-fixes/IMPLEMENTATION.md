# Implementation Plan: mux-suppressed-output-round2-fixes

## Overview

Fix the eight medium findings left `unresolved` in review round 2 of
mux-suppressed-output-fixes so that the side-effect handling of a suppressed
chunk (write-filter decisions and replacement-output assembly) reaches the
same parse state, in the same order, as the client would have reached from
the raw stream, and record a decision per finding. Four tasks run in
parallel; this document pins only what two or more of them share.

## Technology Stack

- **Language / crate**: Rust, the existing `src-tauri` crate. All touched
  modules are CLI-shared (`crate::mux`, not gated on the `gui` feature).
- **Client-parity reference**: `crates/term_core` parser transitions and the
  GUI theme's color-query routing (`src-tauri/src/render/theme.rs`), used as
  the rule basis (SPEC as-01) and as the test oracle.
- **New dependencies**: none. `project.license` (MIT) is unaffected; no
  dependency license needs recording.

## Layer Structure

| Layer | Location | Responsibility | May depend on |
|-------|----------|----------------|---------------|
| Shared OSC identification (new leaf) | `src-tauri/src/mux/osc_identify.rs` | OSC number recovery and viewer-launch identification (FR7) | `crate::viewer_kinds` only |
| Snapshot trailing-construct decider (new leaf) | `src-tauri/src/mux/snapshot_tail.rs` | Classify the incomplete construct a snapshot payload leaves the client parser in (FR8) | nothing inside `crate::mux` |
| Scrollback strip | `src-tauri/src/mux/scrollback_filter.rs` | Strip selection on ring write and snapshot assembly | shared OSC identification |
| Boundary record | `src-tauri/src/mux/session/pane/output_capture.rs` | Per-destination suppression boundary plus the covering snapshot's trailing construct | nothing new |
| Snapshot paths | `session/pane/output_target.rs` (visibility restore), `ipc/reattach.rs` (visible reattach), `ipc/handlers/mod.rs` + `session/pane/output_queue.rs` + `ipc/handlers/attach.rs` (on-demand, immediate and deferred) | Build the snapshot, decide its trailing construct, record it with the boundary | snapshot trailing-construct decider |
| Reader pipeline | `src-tauri/src/mux/ipc/pty_spawn/` (`mod.rs`, `write_filter.rs`, `client_parity_scan.rs`, `suppressed_output.rs`) | Write filter, suppression decision, replacement assembly | all of the above |

Dependency direction: the two new leaves depend on nothing in `mux::ipc` or
`mux::session`; `mux::session` never depends on `mux::ipc` (the existing
rule that made `snapshot_bytes.rs` a leaf). Neither leaf may use a
`gui`-gated item (NFR6).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Delivery-side OSC classification entry points (`client_parity_scan.rs`: `reconstruct_osc_number_and_data`, `is_color_query`, `is_viewer_launch`) | Decide whether a complete OSC body is a color query or a deliverable viewer launch | Names, visibility and contracts stay as they are today. Recovery: digits before the first `;` accumulate in base 10, the first `;` is dropped, every other byte is data, the number is absent on u16 overflow, and recovery never panics. `is_viewer_launch` is true exactly for OSC 777 `emterm;<kind>` with `<kind>` a replayable viewer kind other than `image`, and for OSC 9999 `emterm-md` / `emterm-md;…`. task0001 may re-implement their bodies on the shared layer but must not rename, remove or change them. | task0001 (re-implements), task0003 (classifies the carried-over completion with them) |
| Suppressed-chunk replacement request (`suppressed_output.rs`) | One value carrying every input of replacement assembly for one suppressed chunk | Base fields: the chunk's raw bytes; the ring-written ranges (chunk coordinates, ascending, exactly the spans fed to the write filter for this read); the write filter's pending bytes after this read; the retained window (up to 256 bytes preceding the chunk). Field **carried-over completion** (owner task0003): absent, or the complete bytes of a sequence whose opening ESC was held in pending before this read and that completed in this read, plus its end position (exclusive) in chunk coordinates. Field **snapshot trailing construct** (owner task0004): absent, or the bytes of the incomplete construct the destination's covering snapshot left the client in. Postcondition: output is the replacement bytes (possibly empty) in delivery order, meaning queries and viewer launches in raw-stream order, then the tail. With both new fields absent, the result equals the pre-feature assembly except for the FR1/FR2/FR5/FR7 behavior changes. The existing four-parameter `build_suppressed_replacement` stays, with unchanged parameters, as the "both new fields absent" form so existing unit tests keep compiling. | task0003, task0004 (task0002 changes item assembly only, not the request) |
| Suppressed pipeline invocation (`pty_spawn/mod.rs`: `ReaderForward::Suppressed`, `run_suppressed_pipeline`) | Carry the suppression decision's facts to replacement assembly | `run_suppressed_pipeline` builds the request from: the chunk; the ring-written ranges (`live_spans`); the write filter's pending bytes; the retained window; for task0003, the carried-over completion that this read's feed reported, mapped to chunk coordinates; for task0004, the snapshot trailing construct obtained with the suppression decision. The replacement is sent only to the destination captured at the suppression decision; an empty replacement is never sent. | task0003, task0004 |
| Regression test registry (below) | Pin one regression test per finding so the decision table can reference it before the tests exist in the table owner's worktree | Each owner task creates a test with exactly the pinned name in the pinned file; it fails on the pre-fix code and passes after the fix (SPEC AC-1). Additional tests are free to use any name. | task0001–task0004 (create), task0002 (references in DECISIONS.md) |

### Regression test registry

| stable_id | Requirement | Owner | File | Test name |
|-----------|-------------|-------|------|-----------|
| `ecc48041b65a5380` | FR1 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs` (tests module) | `round2_ecc48041_designator_slot_esc_is_never_a_window_restart_position` |
| `dd56f3984c74cde1` | FR2 | task0002 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_dd56f398_main_screen_color_query_is_answered_once_after_the_snapshot` |
| `ae48e7cd98084c19` | FR3 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_ae48e7cd_awaiting_designator_is_carried_across_feeds` |
| `b600645f1fa94686` | FR4 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_b600645f_removed_screen_switch_closes_the_pending_string` |
| `f8b600bcc0ed55da` | FR5 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_f8b600bc_pending_exclusion_keeps_alt_screen_queries` |
| `03ccd5c7702db8db` | FR6 | task0003 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_03ccd5c7_carried_over_viewer_launch_is_delivered_once` |
| `a93dffe30438a693` | FR7 | task0001 | `src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs` (tests module) | `round2_a93dffe3_leading_zero_viewer_launch_reaches_the_client_once` |
| `3eccc254dd278b33` | FR8 | task0004 | `src-tauri/src/mux/ipc/pty_spawn/tests.rs` | `round2_3eccc254_visibility_restore_does_not_resend_a_tail_the_snapshot_carried` |

Registry tests drive entry points whose parameters this feature keeps (the
reader harness, the four-parameter replacement builder, the scrollback strip
functions, the existing write-filter feed), so that "fails on the pre-fix
code" can be demonstrated by running the same test body against the pre-fix
sources.

## Conventions

- **Coordinate spaces.** *Chunk coordinates*: byte offsets into the raw
  read (`data`). *Fed coordinates*: offsets into the concatenation of the
  ring-written ranges, i.e. what the write filter is fed. *Combined
  coordinates*: offsets into the client-parity scan's `window[s..] ++ chunk`.
  Every position passed between modules (request fields, scan exclusion,
  dedup keys) is in chunk coordinates; conversion happens at the producer.
- **Client parity, fail toward misses (TM-1).** Every scan and state
  decision follows the term_core parser transitions and the previous
  feature's transition table (a)–(h) (SPEC as-01). When the client's state
  at a position cannot be decided, the position is treated as "not a start"
  — the result may miss a query or launch, never fabricate one.
- **Bounded and panic-free (TM-2).** Every scan and decision added or
  changed is a single forward pass bounded by its input length, with no
  panicking arithmetic or indexing. An empty replacement is never turned
  into a `PtyOutput` chunk. The 512 KiB pending cap, its strip-filtered
  flush and the 256-byte retained window are unchanged.
- **Locks (NFR2).** Order `output_target` → capture exclusion → ring /
  shadow parser; the boundary exclusion is never held together with the
  capture exclusion; no `blocking_send` while `output_target` is held. New
  work (trailing-construct decision, identification, mapping) runs outside
  the capture exclusion.
- **Reader hot path (NFR3).** No new scan on the non-suppressed path. State
  the write filter must expose is recorded inside its existing boundary
  scan; identification, mapping and assembly run only for suppressed chunks.
- **Tests.** Client-behavior assertions use a term_core client: snapshots
  are applied with `reset_and_replay_segments` using the real assembly
  function's output and their responses are discarded (as the GUI does);
  the reference feeds the raw stream once. Existing tests pass unchanged
  (SPEC AC-8); rewriting a call site for a changed parameter list is not a
  behavior change. The three behavior-changing tests SPEC AC-8 expects all
  belong to task0002, which renames them to describe the new behavior and
  lists them in DECISIONS.md. Any other existing test whose expectation
  would have to change is a plan deviation: the task that hits it reports it
  instead of editing the expectation.

## Cross-task Design Decisions

### D1: Decision table location and ownership (FR9)

The decision table is `feature-docs/mux-suppressed-output-round2-fixes/DECISIONS.md`,
written by task0002. It has one row per finding (all eight, verdict
"resolved" — each of FR1–FR8 fixes exactly one finding) with the rationale
and the registry test (file and name) from the table above, and it lists the
behavior-changing tests (SPEC AC-8). Affected: task0002 writes it;
task0001/task0003/task0004 must honour the registry names.
`feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml` is never
touched by any task.

### D2: Replacement request is introduced additively

task0003 and task0004 both extend replacement assembly and the pipeline
invocation. Each introduces the replacement request (if it does not yet
exist on its branch) with the base fields plus only the field it owns, and
keeps the four-parameter form as the "no new fields" wrapper. When the
second of the two merges, the request carries the union of the fields; its
own field and its own pipeline wiring are added to what the first one
introduced. The FR8 tail rule (task0004) applies only when the tail comes
from the client-parity scan, never when it is the write filter's pending run
(task0003's domain), so the two rules never compete for the same tail.

### D3: as-07 check result (term_core)

Checked in `crates/term_core/src/parser/ground.rs`: a byte that is not a
continuation byte while a UTF-8 sequence is incomplete clears the partial
bytes without printing U+FFFD and is then processed from ground — this holds
for ESC (as-07 confirmed) and also for a new lead byte. A continuation byte
arriving in ground prints U+FFFD. `escape.rs`: the byte after `ESC (` /
`ESC )` is always consumed and dispatched as a charset designation (`0`
selects line drawing, every other value selects ASCII; there is no no-op
designator). `osc.rs`: an OSC aborted by ESC followed by a non-`\` byte is
dispatched as `Unterminated`. Effect on tests: re-sending a UTF-8 or
incomplete-CSI tail the client already holds is invisible in term_core,
while re-sending `ESC (` prints the `(`; FR8's registry test therefore has
to include the designator case (task0004).

### D4: Ring content under FR3/FR4 versus NFR1

FR3 and FR4 stop the write filter from holding bytes the client has already
closed or consumed as a designator, so in those streams the bytes reach the
ring earlier; the concatenated ring byte stream stays the concatenation of
the main-buffer spans. Snapshot byte assembly (every layout in
`snapshot_bytes.rs`) and the `mux_ipc` wire format are untouched; NFR1 /
AC-6 are judged on the assembly for a given ring and shadow state, with
FR7's strip-target change as the only content exception. Affected: task0003
(produces the change), task0001 (owns the exception), task0004 (must keep
assembly byte-identical).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Merge conflicts in `suppressed_output.rs`, `pty_spawn/mod.rs`, `client_parity_scan.rs` and `pty_spawn/tests.rs` among task0001–task0004 | High | Medium | Contracts pinned above (D2, entry points kept stable); each task keeps its change inside the functions it owns |
| FR8 coexistence with an `ESC (` tail needs a filler designator that briefly selects a charset until the next chunk's real designator arrives | Medium | Low | Only in that one shape; nothing is printed in between (queries and launches only); the next chunk restores the raw-stream charset (task0004) |
| Two snapshots with the same boundary race for one destination and leave conflicting trailing constructs | Low | Low | Conflicting information collapses to "absent", which falls back to the pre-feature re-send (task0004) |
| FR7 now strips leading-zero and non-digit-prefixed launches that the ring used to keep | Medium | Low | Stated NFR1 exception; strip tests pin kept vs stripped kinds (task0001) |
| A second client-parity state machine (the snapshot trailing-construct decider) drifts from the reader-side scan | Medium | Medium | Both are tested against term_core as the oracle; the decider reports only three narrow constructs and falls back to "absent" otherwise (task0004) |
| Pre-existing nondeterministic tests in the `--lib` run (e.g. `tabs` replay tests in parallel) mask or mimic regressions | Medium | Low | Re-run the failing test single-threaded before treating it as a failure (VERIFICATION.md) |

## Open Questions

- [ ] None blocking. SPEC.md has no `tbd` requirement and no DESIGN.md
      exists (design step skipped; no UI change).
