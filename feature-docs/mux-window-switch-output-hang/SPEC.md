# Feature: mux-window-switch-output-hang

## Overview

Fix a hang in eMterm's mux daemon: while a pane is producing very high-volume
output (e.g. `seq 1 10000000`), switching mux windows/tabs causes the daemon
connection to stop responding — no more input is accepted for any pane on
that connection, and after a client detach/reattach the affected tab (or
other tabs sharing the connection) can remain stuck.

## Objectives

- Eliminate the daemon-side self-deadlock that occurs when a pane-snapshot
  request is handled while the shared pane-output channel is at capacity.
- Preserve the existing FIFO ordering guarantee between a pane's snapshot
  chunk and already-queued PTY output chunks for that pane.
- Preserve existing backpressure characteristics (no unbounded memory
  growth) for high-volume PTY output.

## User Stories

### US1: Switch windows while a pane is producing massive output
As an eMterm mux user, I want to switch to another window/tab while one pane
is running a command that produces a huge amount of output, so that I can
keep working without the whole client freezing.

**Acceptance Criteria:**
- [ ] Switching windows/tabs while a pane emits sustained high-volume output
      does not hang the client or the daemon connection.
- [ ] Input to other panes on the same connection continues to be processed
      while the high-volume pane is producing output.

## Technical Requirements

### Functional Requirements
- **FR1:** The daemon connection task MUST NOT self-deadlock when
  `handle_request_pane_snapshot` is invoked while the pane's shared PTY
  output channel (`pane_output_tx`, capacity `PTY_CHANNEL_CAPACITY = 256`,
  `src-tauri/src/mux/session/pane.rs`) is at or near capacity due to
  sustained high-volume PTY output.
- **FR2:** While a snapshot for one pane is pending delivery, the same
  connection's `select!` loop (`src-tauri/src/mux/ipc/connection.rs`) MUST
  continue to process incoming client messages (`framed.next()` arm,
  including `PtyInput` for other panes) and continue to drain/forward
  queued PTY output for all panes on the connection
  (`pane_output_rx.recv()` arm).
- **FR3:** The snapshot chunk for a given pane MUST still be delivered to
  the client, and MUST NOT be silently dropped or starved indefinitely while
  the pane's PTY reader thread continues producing (task0003 rework, AC-3),
  **except for the bounded-backlog carve-out below (task0004 rework,
  G3/AC-3 option (a))**: when the connection-owned deferred-output backlog
  (`DeferredOutputQueue`, `src-tauri/src/mux/session/pane.rs`) already holds
  `MAX_DEFERRED_ITEMS` (8) DISTINCT panes' snapshot chunks pending delivery,
  admitting a 9th distinct pane's snapshot MAY evict the OLDEST queued
  distinct-pane chunk (never the one just enqueued). This is the ONLY
  permitted exception to the "MUST be delivered" / "MUST NOT be dropped"
  clause above — it exists solely to keep the backlog's memory bound (FR4)
  finite without an unbounded per-pane byte budget, and it never applies to
  fewer than `MAX_DEFERRED_ITEMS` DISTINCT panes pending at once. Recovery
  is client-driven and not part of this feature: the evicted pane's tab
  simply stays as last-rendered until the user next switches to it, which
  re-issues `RequestPaneSnapshot` and receives a fresh snapshot normally.
  Ordering relative to PTY output chunks already queued for that same pane
  BEFORE the snapshot was requested, and relative to other items deferred
  through this same connection, IS preserved (FIFO). Ordering relative to
  live PTY output the pane's reader thread (a separate OS thread sending
  directly to `pane_output_tx`, `src-tauri/src/mux/ipc/pty_spawn.rs`)
  produces AFTER the snapshot was requested is NOT guaranteed — a
  concurrently-produced live chunk MAY still reach the client ahead of a
  deferred snapshot if it wins a freed channel permit first. Closing that
  residual would require either routing the reader thread's sends through
  the same connection-owned queue, or a client-observable generation number
  so a stale snapshot can be discarded on arrival — both out of this
  feature's scope (see `DeferredOutputQueue`'s doc,
  `src-tauri/src/mux/session/pane.rs`).
- **FR4:** The fix MUST NOT introduce unbounded memory growth as a
  replacement backpressure mechanism (e.g. an unbounded channel is not an
  acceptable unconditional replacement for the bounded channel without an
  equivalent bound elsewhere).

### Non-Functional Requirements
- **NFR1 - Reliability:** No regression in existing mux/tabs test suite
  (`cargo test --lib`, run with `--test-threads=1` per project convention
  for `tabs.rs` off-thread-worker tests).
- **NFR2 - Compatibility:** Client-side off-thread snapshot replay
  (`src-tauri/src/tabs.rs`, `dispatch_offthread_replay` /
  `apply_offthread_swap`) is unaffected — this feature is daemon-side only.

## Implementation Approach

### Root Cause (confirmed via code investigation)

Each daemon-side client connection is driven by a single `tokio::select!`
loop in `src-tauri/src/mux/ipc/connection.rs` (loop starts around line 277).
Two of its arms share one bounded `tokio::mpsc::channel::<PtyOutputChunk>`
(`pane_output_tx` / `pane_output_rx`, capacity 256, created once per
connection and shared across every pane in the session):

- the `framed.next()` arm reads an incoming client message and dispatches it
  via `route_message(...).await`, which can call
  `handle_request_pane_snapshot` (`src-tauri/src/mux/ipc/handlers.rs`)
  in response to a `RequestPaneSnapshot` message (sent by the client on
  window/tab switch);
- the `pane_output_rx.recv()` arm drains queued PTY output chunks (up to
  `DRAIN_BATCH_LIMIT = 64` per iteration) and forwards them to the client
  socket.

`handle_request_pane_snapshot` builds the snapshot payload and enqueues it
as a `ChunkKind::Snapshot` chunk onto the SAME `pane_output_tx`
(`pane_output_tx.send(...).await`) to preserve FIFO ordering with
already-queued PTY bytes for that pane. When the channel is near/at
capacity — which happens when a pane emits output faster than the socket
drains it — this `.send().await` suspends waiting for free capacity. Free
capacity can only be created by the SAME task's `pane_output_rx.recv()` arm
running again — which cannot happen while the task is suspended inside
`route_message`'s await chain. This is a single-task producer/consumer
self-block: the connection task stalls entirely, so no further client
messages are processed (explains "stops accepting input") and no queued PTY
output for ANY pane on that connection is forwarded (explains why other
tabs sharing the connection can also freeze, and why a stuck state can
survive a detach/reattach if the stall is still in effect).

### Candidate resolution directions (final selection deferred to create-plan)

The specific mechanism is left to the planning phase, constrained by
FR1-FR4. Candidates observed in the codebase's existing patterns (not a
prescription):

1. Decouple the snapshot enqueue from the connection's own select loop —
   e.g. perform `pane_output_tx.send(...)` from a spawned task /
   `try_send` with an explicit backpressure-safe fallback, so the
   originating connection task is never itself the one blocked on filling
   the channel it alone drains.
2. Split snapshot delivery onto a distinct channel while preserving FIFO
   order relative to the PTY output channel for the same pane (e.g. a
   sequence/ordering token, or draining both channels in a single combined
   step before forwarding).
3. Any other approach that satisfies FR1-FR4; the planner should record the
   chosen mechanism's rationale in IMPLEMENTATION.md.

### File Structure (existing files primarily touched)

```
src-tauri/src/mux/ipc/
├── connection.rs      # per-connection select! loop (message dispatch + PTY output drain)
├── handlers.rs         # handle_request_pane_snapshot
src-tauri/src/mux/session/
├── pane.rs             # PTY_CHANNEL_CAPACITY, pane_output_tx/rx definitions
src-tauri/src/mux/ipc/
├── pty_spawn.rs         # PTY reader thread producing into pane_output_tx
```

## Test Scenarios

### Unit Tests
- [ ] Test 1: A connection task's `select!` loop continues to process an
      incoming `PtyInput` message for pane B while pane A's snapshot send is
      pending against a full/near-full shared channel.
- [ ] Test 2: Snapshot chunk for a pane is observed by the client in FIFO
      order relative to PTY output chunks queued for that pane before the
      snapshot was requested.

### Integration Tests
- [ ] Test 1: Simulate sustained high-volume PTY output on one pane while
      issuing a `RequestPaneSnapshot`/`SwitchWindow` for another pane on the
      same connection; assert the connection continues to accept and
      process messages within a bounded time (no indefinite hang).

### E2E Tests
**Existing E2E tests**: None detected in this repository.
**Run command**: Not applicable.
- [ ] Scenario 1: Manual/tauri-driver-based repro — run `seq 1 10000000` in
      one mux pane, switch windows, confirm the client remains responsive
      and other tabs can be used.

### Edge Cases
- [ ] Edge case 1: Snapshot requested for the exact pane that is currently
      producing the high-volume output (not just a different pane on the
      same connection).
- [ ] Edge case 2: Multiple rapid window switches while output is ongoing.

### Performance Tests
- [ ] Load test: `seq 1 10000000`-scale output sustained for the duration of
      several window switches; acceptance = no observed hang (bounded
      response latency), no unbounded memory growth.

## Security Considerations

Not applicable — this is an internal daemon/client IPC concurrency fix; no
new external inputs, authentication, or data exposure surface is
introduced.

## Success Criteria

- [ ] All functional requirements (FR1-FR4) are implemented and tested
- [ ] All test scenarios pass
- [ ] No regression in existing mux/tabs test suite
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

(none — all requirements resolved via code investigation in batch mode;
the specific resolution mechanism is intentionally left open per
Implementation Approach and resolved during create-plan)

## References

- Prior related work (repository `doc/tasks/`): `mux-snapshot-reparse-offthread`,
  `mux-offthread-replay`, `mux-offthread-swap-callback-restore`,
  `snapshot-replay-perf`, `snapshot-replay-daemon-routing`,
  `snapshot-replay-scrollback-restore` — none of these address the
  shared-channel backpressure self-block between `handle_request_pane_snapshot`
  and the connection's own drain arm; this appears to be a gap left by the
  "route snapshot through the same channel for FIFO ordering" design
  decision.
