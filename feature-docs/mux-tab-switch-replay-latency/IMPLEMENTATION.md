# Implementation Plan: mux-tab-switch-replay-latency

## Overview

Restore bypass-equivalent snapshot replay latency for mux tab switches by
fixing the bypass split gate's behavior under a resize-marker-dense
scrollback tail (task0001), removing the upstream cause of that marker
accumulation (task0002), and closing two independent switch-time race
conditions that also defeat bypass or duplicate work (task0003).

## Technology Stack

- Rust, existing crates only (`term_core`, `src-tauri`). No new dependency
  is introduced by this feature.

## Layer Structure

- `crates/term_core` — pure replay/decision logic: the bypass split gate
  (`stable_target_suffix_start`, `bypass_split`/`bypass_engaged`
  computation inside `TerminalCore::build_from_snapshot_inner`) and its
  bench/regression coverage. No I/O, no GUI/mux awareness.
- `src-tauri/src/mux` — daemon-facing scrollback storage
  (`ScrollbackRingBuffer::attribute_write` / `write_resize_marker`), which
  is what produces the marker segments `term_core` later has to replay.
- `src-tauri/src/tabs.rs` — GUI-side orchestration: applying incoming
  `MuxMessage` snapshots, dispatching off-thread replay, reacting to a
  local grid resize (including the group-wide `Resize` control-frame
  broadcast to daemon-side panes).
- `src-tauri/src/window_host.rs` / `src-tauri/src/ui/status_bar.rs` —
  compute the GUI grid size from window size + status bar panel height;
  the source of the resize events task0002 constrains.

Dependency direction: `tabs.rs` calls into `term_core` (replay) and into
`mux/scrollback_buffer.rs` (storage) and is called by `window_host.rs`
(grid size changes). None of task0001's `term_core` changes may introduce
a dependency from `term_core` back onto `tabs.rs` or `window_host.rs`.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|-------------------------------|---------------|
| Bypass split decision (`stable_target_suffix_start` + the `bypass_split`/`bypass_engaged` computation in `TerminalCore::build_from_snapshot_inner`) | Given target `(cols, rows)`, the decoded replay segments, and the `bypass` request flag, decide whether replay engages the bypass (suffix-only) path or falls back to a full non-bypass drain | Precondition: `segments` reflects the scrollback's recorded resize markers in offset order. Postcondition: the decision is a pure function of its inputs (no hidden state); its caller-visible outputs (`bypass_engaged`, resulting viewport/cursor, `scrollback_populated`) keep their existing meaning — task0001 may change *which* inputs produce `bypass_engaged: true`, but must not change what `bypass_engaged: true`/`false` each imply downstream | task0001 (owns the decision logic), task0003 (calls it indirectly via `dispatch_offthread_replay`/`build_from_snapshot`, without needing to know its internals — task0003 only supplies correct, consistently-captured target dims and segments as input) |
| `Tab::dispatch_offthread_replay` / `Tab::apply_mux_message`'s `Snapshot`/`SnapshotRestore` handling | Entry points that decode an incoming wire snapshot and start (or supersede) a replay for a target pane | Precondition: `payload`/`segments` are the decoded content for `target_pane`. Postcondition: at most one in-flight replay per tab is active at a time (a new dispatch supersedes an older one); the tab's displayed core eventually reflects the *latest* dispatch's target, never a stale one | task0003 (owns this path; may add coalescing/dedup logic here without needing task0001/task0002 to change) |
| `Tab::resize`'s group-wide `MessageType::Resize` broadcast (push to every pane in `self.mux_group`) | Informs the daemon's per-pane PTYs of a new local grid size | Precondition: called whenever the GUI's own grid size changes. Postcondition (unchanged by this feature): every daemon-side pane in the group ends up at the same `(cols, rows)` as the GUI's core | task0002 (constrains *when* `Tab::resize` is invoked from `window_host.rs` during startup/reattach settling — task0002 does not change what the broadcast does once `Tab::resize` is called with a settled size) |

## Conventions

- No new `log::debug!`/`log::info!`/`console.debug` diagnostics may be left
  in the merged code — per the project's release logging convention, only
  `warn!`/`error!` persist in release builds, and ad-hoc `[foo-diag ...]`
  prefixed instrumentation (of the kind used during this bug's
  investigation) must be removed before a task is considered done, even if
  a task's own TDD process finds it useful temporarily.
- Every new or changed constant/threshold in the bypass split gate keeps
  the existing doc-comment convention of citing the review-round finding
  or design decision that motivated it (see `BYPASS_PREFIX_MAX_BYTES` /
  `BYPASS_SUFFIX_MIN_BYTES` for the established style) — this file's
  "Constraint" bullet in each task exists precisely to keep that history
  legible for the next round.

## Cross-task Design Decisions

### D1: task0001 owns the bypass gate; task0002/task0003 treat it as fixed input/output shape

task0002 and task0003 do not need to understand the bypass split gate's
internals. task0002 only needs to reduce how often `Tab::resize`'s
broadcast fires (fewer/absent spurious resize markers reaching
`term_core`); task0003 only needs to ensure the target dims and segments
it hands to `term_core` are captured consistently and not fetched twice.
Neither may change `stable_target_suffix_start`'s signature or the meaning
of `bypass_split`/`bypass_engaged` — that is task0001's exclusive surface,
pinned above under Shared Components.

### D2: NFR1 constraint applies only to task0001

The existing `BYPASS_PREFIX_MAX_BYTES` / `suffix_len >= split_at` gates
exist to prevent the 2nd-pass worker
(`tabs.rs::apply_offthread_swap`/`build_scrollback_only_from_snapshot`)
from re-paying a large prefix's non-bypass replay cost a second time.
task0001's fix for the marker-cluster shape (FR1) must preserve this
property: whatever new condition lets a marker-dense-but-otherwise-normal
tail engage bypass, it must not newly let a genuinely-large, expensive
prefix engage a split that then gets redone by the 2nd-pass worker.
task0002/task0003 do not touch this gate and are not subject to this
constraint directly (though task0002's fix reduces how often the
marker-dense shape occurs at all).

### D3: settling threshold for task0002 is an implementation decision, not a spec value

SPEC.md's FR6 requires that the initial `visible_row_count` 0 → 1
transition not broadcast `Resize` to every pane before the status bar
settles, but does not mandate a specific debounce mechanism or timing
constant — task0002 designs the settling detection (e.g. requiring N
consecutive stable frames, or a fixed grace window after mux attach) and
records the chosen mechanism's rationale in its own task plan, not here
(it is single-task content).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| task0001's gate change reintroduces the double 2nd-pass replay cost (NFR1) | Medium | High (perf regression as bad as the bug it fixes) | task0001's Acceptance Criteria include an explicit large-prefix regression test; see D2 |
| task0002's settling fix changes when `Tab::resize` runs for OTHER (non-startup) resize causes, regressing ordinary interactive resize | Low-Medium | Medium | task0002's Acceptance Criteria require ordinary (post-startup) resize behavior to be unchanged, verified by existing resize-path tests |
| task0003's dedup logic for FR8 accidentally drops a legitimate distinct snapshot (not a true duplicate) | Low | Medium (would show stale/wrong content) | task0003's Acceptance Criteria require the dedup condition to be scoped to same-pane, same-target, near-simultaneous frames only |

## Open Questions

None — SPEC.md carries no `tbd` requirements for this feature.
