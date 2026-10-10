# Implementation Plan: osc7501-alt-screen-prompt-mark

## Overview

OSC 133 marks dispatched while the alternate screen is active stop entering the
plain tab's ordered OSC 7501 feed. The feed's prompt-mark candidates then line
up with `term_core`'s live main-screen marks, and a main-screen OSC 133 A that
follows a report in the same processing unit discards the working / blocked /
idle record (FR1, FR2). The feature is one task (task0001); this document holds
the decisions that span the `term_core` and `src-tauri` layers.

## Technology Stack

- **Language**: Rust — existing crates `crates/term_core` and `src-tauri`
- **New dependencies**: none (no license to record; `project.license` MIT is
  unchanged)

## Layer Structure

| Layer | Location | Responsibility in this feature |
|-------|----------|--------------------------------|
| Parser core | `crates/term_core/src/callbacks.rs`, `crates/term_core/src/osc_handler.rs` | Notifies the host once per dispatched OSC, now together with the alternate-screen state at that moment (SC-1). Names no GUI concept. |
| Host callbacks | `src-tauri/src/callbacks.rs` | Builds the ordered OSC 7501 feed and the agent-status latch feed from the notifications; keeps alternate-screen OSC 133 candidates out of the 7501 feed (SC-2). |
| Tab output pipeline | `src-tauri/src/tabs/output_pipeline.rs` | Applies the 7501 feed to the tab's record table against the live marks; matching logic unchanged (D3). |

Dependency direction: `src-tauri` depends on `term_core`, never the reverse.
`term_core` gains no knowledge of OSC 7501, the feeds or the record table.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| SC-1: screen-aware OSC notification (provided method of `term_core`'s terminal-callbacks trait) | Tells the host, for each dispatched OSC, whether the alternate screen was active when it was dispatched | Pre: `term_core` dispatches an OSC that today produces exactly one OSC notification to the installed callbacks. Post: the callbacks receive exactly one screen-aware notification carrying the same action id and payload as today plus one boolean, "alternate screen active", equal to the `MODE_ALT_SCREEN` value that `term_core`'s own OSC 133 live-mark gate reads in the same dispatch. Its position in the synchronous call order is unchanged. Default behavior (callbacks that do not override it): forward to the existing OSC notification with the same action id and payload, so such callbacks observe exactly the sequence they observe today. No allocation, no I/O, constant time (NFR1). | task0001 |
| SC-2: main-screen invariant of the plain tab's 7501 feed | Defines what a prompt-mark item in the ordered OSC 7501 feed means | Every prompt-mark item in the 7501 feed comes from an OSC 133 dispatched on the main screen. A call of the existing OSC notification without screen information counts as the main screen. Reports and resets enter the feed exactly as today. The agent-status latch feed is outside this invariant and keeps receiving one candidate per recognized OSC 133 on either screen. | task0001 |

## Conventions

- Existing tests keep their names and expectations (A3); regression detection
  uses new tests. Pipeline-level tests use the `osc7501_` prefix of their
  neighbors.
- No new log lines on the OSC dispatch path or the feed path.
- Alternate-screen detection uses `term_core`'s `MODE_ALT_SCREEN` only (A2); the
  host keeps no mirror of the mode.

## Cross-task Design Decisions

### D1: Classify at dispatch time in term_core

The alternate-screen state travels with each OSC notification (SC-1) and is
read at the dispatch itself. It is stateless: callbacks moved between cores
(off-thread snapshot swap) or modes set wholesale cannot leave a stale screen
state behind. Options not adopted: host-driven parse rounds that insert screen
markers into the feed (duplicates `term_core`'s single resume loop); a
mode-change notification mirrored into host state (the mirror can go stale);
alternate-screen candidates kept as a separate feed item that the pipeline
skips (data nothing consumes). Affects: task0001.

### D2: The agent-status latch keeps its current input

The latch feed and `reconcile_latch_feed` are out of scope (A4). An
alternate-screen OSC 133 still pushes its latch candidate exactly as today.
Affects: task0001.

### D3: Pipeline matching stays as it is

`apply_program_status_feed` keeps its rules: reports and resets apply in feed
order, prompt-mark candidates are confirmed against the live marks by a forward
walk, and candidates ahead of the last reset are skipped. Under SC-2 the walk
sees main-screen candidates only; only its documentation changes. The
mux-attached discard (FR3) and the query responder (NFR2) are untouched.
Affects: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A main-screen OSC 133 is classified as alternate screen, so records are never discarded | Low | Medium | SC-1 reads the same mode bit, in the same dispatch, as `term_core`'s live-mark gate; tests cover `?1049`, `?1047`, `?47`, a return within the same parse call, and RIS issued on the alternate screen |
| The host handles one OSC twice (its own branch plus the forwarded notification), duplicating feed items | Low | Medium | task0001 AC-3 asserts exactly one item per feed per mark |
| The agent-status latch changes behavior | Low | Medium | D2; the existing latch tests run unchanged (TS-5) |
| Other implementors of the callbacks trait (test doubles in `term_core` and `src-tauri`) stop compiling | Low | Low | SC-1 is a provided method with a forwarding default |

## Open Questions

None.
