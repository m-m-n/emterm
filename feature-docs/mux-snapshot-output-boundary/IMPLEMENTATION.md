# Implementation Plan: mux-snapshot-output-boundary

## Overview

Give every mux pane a per-pane output sequence number and a per-pane capture exclusion so that shadow-parser updates, scrollback-ring writes and snapshot reads are cut at one output boundary, and record each delivered snapshot's boundary against its destination sender so the PTY reader never delivers an already-captured chunk to that destination after the snapshot (SPEC FR1-FR11, NFR1-NFR5).

## Task Decomposition

The feature is planned as a single task (`task0001`). Every part of it — the reader's capture and forward steps, the four snapshot paths, `MuxPane::resize`, the deferred on-demand snapshot flush, and the suppressed-chunk handling — both creates and consumes the same new per-pane state. Tasks run fully in parallel from the same base, so splitting a provider of that state from its consumers would leave each side unable to compile or test on its own. The decisions below are recorded here because any rework task appended later by review or verify must preserve them.

## Technology Stack

- **Language**: Rust (`src-tauri` crate, mux daemon code only; everything must build under `--no-default-features`).
- **Concurrency primitives**: the existing standard mutexes for per-pane state, the existing bounded per-connection output channel, the existing async session-manager mutex.
- **Blocking slot reservation on the reader thread**: the already-present `futures` dependency's executor. It blocks the native reader thread on a reservation of a slot on the output channel.
- **New dependencies**: none. `project.license` (MIT) is unaffected; no license entry to record.

## Layer Structure

| Layer | Modules | Responsibility in this feature |
|-------|---------|--------------------------------|
| Session (per-pane state) | `mux::session::pane` | Owns the new output capture state and suppression boundary record next to the existing output target, shadow parser and scrollback ring; hosts the visibility-resume paths (`resume_pane_with_permit`, `evaluate_output_target`) and `MuxPane::resize`. |
| IPC | `mux::ipc::pty_spawn`, `mux::ipc::reattach`, `mux::ipc::handlers` | Reader thread (capture step, forward decision, suppression, replacement payload); visible reattach; on-demand snapshot and deferred-output flush. |
| Shared byte filters | `mux::scrollback_filter`, `pty::passthrough_scanner` | Single source of the "answered device query" predicate; passthrough scanner partial-state discard. |

The dependency direction does not change: `mux::ipc` depends on `mux::session`, both depend on the shared filters, and `mux::session` has no code dependency on `mux::ipc`. The behavior of `mux::ipc::connection` (the channel drain) does not change. Only its doc comments that name the reader's blocking send as the fair-queue waiter are reworded, because the reader now waits through a slot reservation in the same queue.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Output capture state (per pane; suggested field `MuxPane::output_capture`) | Holds the capture exclusion (捕捉の排他) and the last assigned output sequence number. | Starts at "nothing captured" when a pane is constructed. This covers panes restored by hot-upgrade: they count from the start again (ASM-3). While the exclusion is held, the reader assigns exactly one new number to each non-empty read, and that number is the last-captured value once the reader's shadow update and ring write finish. A snapshot reader holding it reads ring contents, shadow-parser state and the last-captured number that describe the same set of chunks. The reader's updates, `MuxPane::resize` and the four snapshot reads take the ring and shadow-parser locks only inside it, and never nest those two locks with each other. Read-only consumers that are not snapshot paths need not take it. | task0001 |
| Suppression boundary record (per pane) | Records, for each destination sender, the highest sequence number covered by a snapshot delivered to that sender. It has its own small exclusion, the boundary exclusion. | Recording for a sender keeps the maximum of the old and new values; it never lowers them. Recording for one sender never removes or lowers another sender's entry. A chunk is covered for a sender only if an entry for that same channel holds a value at or above the chunk's number. Entries for closed senders may be pruned. Every read and write happens under the boundary exclusion. | task0001 |
| Deferred boundary commit (daemon-internal, never serialized) | Travels with an on-demand snapshot that had to be deferred because the channel was full. | Committed exactly once, when that snapshot actually enters the channel, under the boundary exclusion and in the same step as the insertion. Discarded without effect if the snapshot is coalesced away, evicted, or dropped on a closed channel. Existing chunk constructors and the existing `DeferredOutputItem::Chunk` variant shape stay usable unchanged. | task0001 |
| Replacement payload for a suppressed chunk | Bytes sent in place of a suppressed chunk (terminal queries for FR9, then the incomplete trailing sequence for FR10). | Sent as an ordinary `PtyOutput` chunk to the suppressed chunk's destination, after the snapshot and before the reader's next chunk. Never sent when empty, because an empty `PtyOutput` chunk means PTY exit. Never itself subject to suppression. | task0001 |

## Conventions

- **Lock order (NFR2)**:
  - Paths that hold several locks acquire them as `output_target` → capture exclusion → {ring, shadow parser}, and as `output_target` → boundary exclusion.
  - The capture exclusion and the boundary exclusion are never held at the same time.
  - The reader always releases the capture exclusion before taking `output_target`.
  - No path takes `output_target` while holding the capture exclusion.
  - The async session-manager mutex, where a path already holds it, stays outermost as today.
- **No blocking under locks**: nothing waits for channel capacity while holding `output_target`, the capture exclusion or the boundary exclusion. This keeps the G1 discipline and its EOF regression test.
- **Lock scope (NFR3)**:
  - Snapshot paths hold the capture exclusion only while reading ring, shadow-parser state and the last-captured number. Assembly, encoding, the wrapped-ring probe replay, size checks and sending happen outside it.
  - No assembly, encoding, probe replay or ring copy moves into any `output_target` critical section on a tokio worker.
  - The only additions inside existing `output_target` sections are taking the capture exclusion around reads that already happen there (`resume_pane_with_permit`, `evaluate_output_target`), plus an O(1) boundary record.
- **Reader normal-path cost (NFR4)**: for a chunk that is not suppressed, the added work is limited to one counter increment, one comparison and two uncontended exclusion acquisitions. The FR6/FR9/FR10 work runs only for suppressed chunks.
- **Wire and bytes (NFR1)**: sequence numbers, boundaries and commit tokens are daemon-internal. Snapshot byte layout, the `mux_ipc` wire format and `Snapshot`/`SnapshotRestore` frames stay unchanged.
- **Platform (NFR5)**: no platform-specific API; Linux and Windows share one code path.
- **Test-only hooks**: reader pause points exist only in test builds (`cfg(test)`) and cost nothing otherwise.
- **Logging**: anomalies use `warn` or higher (release builds keep only `warn`+). Never log payload bytes.

## Cross-task Design Decisions

### D1: Sequence-number boundary, not one critical section (ASM-1)
A per-pane monotonically increasing output sequence number is assigned inside the capture exclusion. Update-to-send is not merged into one critical section. The backpressure send must stay outside `output_target` (G1), and on-demand assembly must stay off the lock (NFR3).

### D2: Two exclusions (capture vs boundary)
The capture exclusion orders shadow and ring updates against snapshot reads (FR2). A separate boundary exclusion orders "boundary record + snapshot insertion" against "boundary check + reader insertion" (FR11). Keeping them separate means the on-demand path can insert its snapshot atomically with the boundary record without sending under the capture exclusion (NFR3) and without taking `output_target`.

### D3: Ordered insertion invariant (resolves the SPEC's open item on FR4/FR11 ordering)
- **Reader**: for chunk *k*, the covered check against the destination S and the insertion of *k* into S's channel happen while holding `output_target` → boundary exclusion.
- **Snapshot for S with captured boundary B**:
  - `resume_pane_with_permit` / `evaluate_output_target`: record (S, B) inside the existing `output_target` critical section, before the snapshot is sent or returned and before the swap to `Connected`.
  - On-demand, immediate: under the boundary exclusion, try a non-blocking insertion; record (S, B) only if it succeeded.
  - On-demand, deferred: record nothing at defer time. Commit when the flush or fair-permit path actually inserts it (D5).
  - Visible reattach: record (S, B) before the pane's target becomes `Connected(S)`. The `SnapshotRestore` frame is admitted by the connection task before that same task drains S again, as today.
- **Consequence**: any chunk with number ≤ B that reaches S's channel either precedes the snapshot (the snapshot overwrites it) or is suppressed.

### D4: Reader backpressure with a reserved slot
- **On a full channel**: the reader releases all locks and blocks until a slot on that same channel is reserved. This uses the same fair waiter queue the old blocking send joined. The reader then re-takes `output_target` → boundary exclusion and suppresses if S covers the chunk; otherwise it inserts through the slot.
- **Fixed destination**: the destination stays S, the sender first observed for this chunk, even if the target changed meanwhile. The pre-fix blocking send behaved the same way, and FR11 forbids the new sender's boundary from suppressing a chunk bound for the old one.
- **Closed channel**: `capture_passthrough` runs once, and the target switches to `Detached(NetworkDetach)` only if it still points at S. A newer owner's `Connected` target is left intact.

### D5: Deferred on-demand snapshots commit at insertion
The deferred snapshot carries a deferred boundary commit. Evicted or coalesced-away snapshots therefore never suppress anything. The SPEC 4.3 case "a chunk hidden until the client re-requests" does not arise.

### D6: Suppressed-chunk handling (FR6, FR8, FR9, FR10)
- **Unchanged processing (FR8)**: everything besides forwarding runs as for any chunk: shadow update, ring write, title, agent-status and OSC 133, OSC 7 cwd, osc-probe.
- **Steps outside `output_target`**:
  1. Run `capture_passthrough` once.
  2. Discard the passthrough scanner's incomplete in-progress sequence. Its remainder then cannot fire later or stitch with a later Detached period.
  3. Build the replacement payload and send it to S, if it is non-empty.

### D7: Terminal-query set (resolves the SPEC's open item on OSC 10/11/12/4)
- **Snapshot strip set**: OSC 4/10/11/12 color queries are not added. NFR1 forbids changing snapshot bytes.
- **FR9 re-delivery set**:
  - Every complete CSI device query matched by the snapshot strip predicate. The same predicate is reused as the single source of truth, and these queries never survive into a snapshot.
  - Every complete OSC 4/10/11/12 color query located in the chunk's bytes that were not written toward the ring (alternate-screen spans), since only those are absent from the snapshot.
- **Color queries that reached the ring** stay in the snapshot and are not re-delivered, following FR9's wording.

### D8: FR10 method and upper bound (resolves the SPEC's open item on FR10)
- **Method**: re-deliver the incomplete trailing sequence verbatim after the snapshot, so that its continuation in the next chunk completes it once on the client. It starts with ESC or a UTF-8 lead byte, which restarts the client parser's sequence state whatever the snapshot's own tail left behind; the reference tests in task0001 must confirm this.
- **Source of the tail**:
  - If the reader's scrollback write filter holds a pending run after the chunk, the tail is that whole run, even if it started in an earlier chunk.
  - Otherwise it is the incomplete CSI, ESC-intermediate, OSC, DCS or APC sequence, or the partial UTF-8 character, whose introducer lies inside the suppressed chunk.
- **Upper bound**: the filter's existing pending cap (512 KiB) for pending runs. Otherwise the suppressed chunk's own extent (at most one 64 KiB read). These are the known gaps (FR10):
  - An introducer that lies before the suppressed chunk and is not held in pending.
  - The filter's overflow escape hatch.

### D9: Cases that record no boundary (FR5)
- On-demand size rejection.
- `resume_pane_with_permit` returning `NoChange`.
- `evaluate_output_target` returning `Unchanged`.
- Hidden reattach.
- A deferred snapshot that is never inserted.
- Visible reattach of a pane whose snapshot `send_reattach_data` would skip for size. The decision uses the same shared size policy. In the common case it must not delay the `Connected` swap by snapshot assembly: decide from a conservative upper bound first, and fall back to the exact encoded length only when the bound does not fit.

### D10: `evaluate_output_target`'s `ResumeWithSnapshot` branch
This branch is unreachable in production. It keeps its existing caller contract: the returned snapshot must be sent before any later reader chunk. FR3 suppression holds regardless. FR9/FR10 "after the snapshot" ordering on this branch holds only when a caller honours that contract.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| New lock introduces a deadlock with `output_target` / session manager | Medium | High (pane or connection freeze) | Fixed lock order and no-blocking-under-locks conventions above; TS-12 stress test; G1 EOF regression test kept. |
| Snapshot bytes change accidentally while restructuring reads (breaks apt-progress constraint) | Medium | High | Reads only move under the capture exclusion; assembly helpers are unchanged. TS-13 runs the existing byte-shape tests unmodified. |
| Reserved-slot backpressure changes fairness or throughput under saturation | Low | Medium | Reservation joins the same waiter queue as the previous blocking send; the existing connection starvation tests stay green. |
| Chunks already queued in a connection's own channel before that same connection re-attaches are drained after the `SnapshotRestore` (pre-existing, outside the in-flight window this feature closes) | Low | Low (transient) | Documented residual; see Open Questions. |
| Replacement payload re-delivers a partial rich-content sequence, so a viewer opens after the switch | Low | Low | Allowed by FR10 ("may reach the client once"); never twice because the passthrough scanner partial is discarded. |

## Open Questions

- [ ] A connection that re-attaches while chunks for the same panes are still queued in its own channel will still deliver those queued chunks after the `SnapshotRestore`. This is pre-existing and needs channel-side filtering. It is kept out of scope unless review requires it.
- [ ] Color queries that stay inside a snapshot are neither answered by the client's replay (replay discards responses) nor re-delivered (FR9 wording). A program waiting on such a query relies on its own timeout.
- [x] ASM-3 basis: `mux::upgrade` re-establishes restored panes' readers through the same `pty_reader_loop`, so restored panes restart numbering with a fresh capture state.
