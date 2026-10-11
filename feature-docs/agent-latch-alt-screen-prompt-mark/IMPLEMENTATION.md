# Implementation Plan: agent-latch-alt-screen-prompt-mark

## Overview

Keep alternate-screen OSC 133 candidates out of the plain tab's agent-status
latch feed at their source, so the existing forward-walk reconciliation hands
main-screen marks to the inferred-clear latch at their true positions relative
to OSC 777 Set / Clear. The feature is one task (task0001).

## Technology Stack

- **Language / Framework**: Rust, `src-tauri` crate (`gui` feature path: native
  callbacks, tab output pipeline, agent-status model)
- **New dependencies**: none (no license check required; `project.license`
  MIT is unaffected)

## Layer Structure

1. term_core — parses PTY bytes, tracks alternate-screen mode, captures live
   OSC 133 marks (main screen only), delivers each OSC through the
   screen-aware notification.
2. Native callbacks — candidate sources: the latch feed and the OSC 7501 feed.
3. Tab output pipeline — per pump, drains both feeds and the live marks and
   reconciles the feeds against the live marks.
4. Agent-status model — per-tab inferred-clear latch and the Clear path.
5. App pump — applies each tab's events and latch inputs in order.

Dependencies point downward only (unchanged). This feature changes only the
candidate source rule in layer 2; layers 3 and 4 keep their behavior and get
documentation updates.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Latch feed (per-pump ordered log of Set / Clear / OSC 133 mark candidates) | Carries agent-status reports and prompt-mark candidates from the callbacks to the pipeline in call order | Pre: every mark candidate was dispatched on the main screen (screen-aware notification with the main screen active, or the screen-less notification); Set / Clear enter on either screen, in call order. Post: drained every pump; discarded for mux-attached pumps. | task0001 |
| `reconcile_latch_feed` | Turns one pump's latch feed plus that pump's live marks into true-order, live-only latch inputs | Pre: the latch-feed precondition above; live marks are the same pump's main-screen marks in order. Post: every Set / Clear in original order; each candidate the live marks confirm (front to back, kind match) at its own position; unconfirmed candidates dropped without consuming a live mark; one pass, no blocking, no I/O. Signature and behavior unchanged by this feature. | task0001 |

## Conventions

- Test renames follow `.claude/rules/test-docs-records.md`: a test is renamed
  only when its name states the superseded behavior; the predecessor record
  lists the new name, and an AC whose expectation inverts gets a supersede
  note naming this feature's SPEC FR IDs.
- Cargo commands run from the project root with
  `CARGO_TARGET_DIR=src-tauri/target` and `--manifest-path src-tauri/Cargo.toml`.
- No new logging.

## Cross-task Design Decisions

### D1: Exclude at the candidate source, not in the reconciliation

- **Decision**: alternate-screen exclusion is applied where candidates enter
  the latch feed (callbacks layer), using the same main-screen rule the OSC
  7501 feed already follows; `reconcile_latch_feed` keeps its contract.
- **Rationale**: the screen-aware notification carries the alternate-screen
  state read at dispatch, which agrees with term_core's live-mark gate; the
  reconciliation has no screen information, and kind-only matching cannot
  tell the screens apart. One rule for both feeds keeps one invariant.
- **Affected tasks**: task0001, and any later rework task touching the latch
  feed.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| An existing test beyond the three identified callback tests pins the superseded "alternate-screen candidate stays in the latch feed" behavior | Low | Low | Full `--lib` suite (TS-7); any further inverted expectation follows the same rename / record rule |
| Known non-deterministic tests (tabs replay, tmux_sockets discover) fail under parallel load and blur the result | Medium | Low | Judge against the task record's `baseline_failures` |

## Open Questions

- None
