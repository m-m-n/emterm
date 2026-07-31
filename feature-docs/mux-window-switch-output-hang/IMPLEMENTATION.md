# Implementation Plan: mux-window-switch-output-hang

## Overview

Fix a daemon-side self-deadlock in the mux connection task: a pane-snapshot
enqueue can block on the same channel that only the deadlocked task itself
drains, freezing the whole connection during high-volume PTY output. This is
a single-task fix (one coherent concurrency change); no cross-task shared
components are introduced.

## Technology Stack

- **Language**: Rust (tokio async runtime, `tokio::select!`, `tokio::sync::mpsc`)
- **Affected subsystem**: `src-tauri/src/mux/ipc/` (daemon-side per-connection
  task) and `src-tauri/src/mux/session/pane.rs` (pane output channel)

## Layer Structure

Single layer: daemon-side mux IPC connection handling. No new layer or
component boundary is introduced. The existing per-connection `select!` loop
(`connection.rs`) remains the single owner of a connection's I/O; the fix
changes how pane-snapshot delivery interacts with that loop's own channel,
not the loop's overall responsibility.

## Shared Components

Not applicable — this feature is a single task with no cross-task component
sharing.

## Conventions

- Preserve existing logging conventions for backpressure conditions (the
  existing `"Pane {} backpressure: channel full, blocking"`-style log
  lines in `pty_spawn.rs` establish the pattern; any new backpressure-visible
  path introduced by this fix should log at the same level with an equally
  specific message).
- No new error types: this is a concurrency/scheduling fix, not a new
  fallible operation — existing error handling for `pane_output_tx` send
  failures (already handled where the channel closes) must continue to be
  handled the same way.

## Cross-task Design Decisions

Not applicable (single task).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Fix reorders snapshot vs. PTY output chunks for the same pane (breaks FR3 FIFO guarantee) | Medium | High | Acceptance criteria require an explicit ordering test; task plan mandates preserving delivery order relative to already-queued chunks for the same pane |
| Fix removes backpressure entirely (unbounded growth) | Low | High | Acceptance criteria require the channel/queue bound to remain finite; task plan forbids introducing an unconditionally unbounded channel as the sole mitigation |
| Fix only relieves the observed symptom for the snapshot path but leaves other send-from-within-select-loop patterns in the same connection task self-blockable | Medium | Medium | Task plan requires an audit note in the task's own file: confirm no other `route_message`-reachable code path performs a blocking send on `pane_output_tx` from within this same task without also being able to progress the drain arm |

## Open Questions

- [ ] None — the resolution mechanism is decided within the task (SPEC.md
  Implementation Approach lists FIFO-preserving candidates; the task plan
  commits to one, documented in the task's own Design section, not decided
  here since it is task-local, not cross-task).
