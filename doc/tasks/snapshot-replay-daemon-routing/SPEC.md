# Feature: snapshot-replay-daemon-routing — route pane-snapshot replies through `MessageType::Snapshot`

## Overview

The mux daemon currently replies to `RequestPaneSnapshot` by sending the
~2 MiB snapshot payload as `MessageType::PtyOutput`. On the client side this
arrives at the same branch as live PTY output and is fed to `TerminalCore`
through `process_pty_data_fully`, which exercises the SlimCell intern +
`scrollback_slim` push/pop hot loop in `ring_push_blank`. The
`TerminalCore::build_from_snapshot` fast path with `scrollback_bypass`
(implemented in `doc/tasks/snapshot-replay-perf/`) is therefore never
entered by real tab-switch traffic.

This feature re-routes the daemon's `RequestPaneSnapshot` reply so it is
delivered as `MessageType::Snapshot` (or `MessageType::SnapshotRestore`),
which `tabs.rs::apply_mux_message` already dispatches to the
`build_from_snapshot` + `scrollback_bypass` path (off-thread when payload
>= 64 KiB, synchronous otherwise). After this change, the existing
`snapshot-replay-perf` machinery becomes effective on the real tab-switch
flow.

## Objectives

- Reduce wall-clock latency of a 2 MiB tab-switch reattach from **~3000 ms
  (observed in production)** to **< 1000 ms (MUST)**, ideally **< 200 ms
  (SHOULD)** or **< 100 ms (STRETCH)**.
- Keep the daemon-side scrollback assembly, the snapshot byte-stream
  format, and the client-side `Snapshot` / `SnapshotRestore` handling
  unchanged in behavior.
- Maintain forward and backward compatibility across one daemon/client
  version skew step so users can upgrade without a forced restart sequence.

## User Stories

### US1: Switching to a heavy mux tab is sub-second in production

As a developer running mux with one tab full of `seq 1 10000000` output
(~2 MiB scrollback), I want a tab switch into that pane to feel
sub-second on the real binary (not just in micro-benches), so that mux
is competitive with tmux for AI-heavy workflows.

**Acceptance Criteria:**
- [ ] `[mux-perf]` log line `RECEIVED → swap DONE` delta for a 2 MiB
      snapshot is **< 1000 ms (MUST)** on the user's local machine.
- [ ] The instrumentation confirms the `apply_mux_message::Snapshot |
      SnapshotRestore` branch is entered (it currently is not), and the
      `dispatch_offthread_replay` worker logs `build_from_snapshot
      START / DONE`.

### US2: Mixed daemon/client versions do not break mux

As a user upgrading the binary while a daemon from the previous build is
still running, I want mux to keep working (functionally) regardless of
which side is upgraded first, so that I do not have to coordinate a
restart.

**Acceptance Criteria:**
- [ ] **New daemon × old client:** the old client decodes the new reply
      via its existing `Snapshot` branch and replays correctly. Tab
      switching works; performance is at the old client's level.
- [ ] **New client × old daemon:** the new client receives `PtyOutput`
      replies and processes them through the live-input path; tab
      switching works (functional only, no perf improvement until daemon
      is also upgraded).

## Technical Requirements

### Functional Requirements

- **FR1 — Daemon replies to `RequestPaneSnapshot` with a snapshot-typed
  message:** `handle_request_pane_snapshot` in
  `src-tauri/src/mux/ipc/handlers.rs` MUST send its assembled snapshot
  payload as `MessageType::Snapshot` or `MessageType::SnapshotRestore`,
  not as `MessageType::PtyOutput`. The send must preserve the existing
  ordering guarantee with respect to already-queued PTY chunks for the
  same pane that the current `PtyOutputChunk` channel provides (see
  `handlers.rs:395-414` doc-comments and `handlers.rs:725-925` ordering
  tests). If the snapshot-typed reply cannot reuse the
  `pane_output_tx: mpsc::Sender<PtyOutputChunk>` channel, a separate
  send path (channel or direct connection write) MUST be added and
  serialized against `pane_output_tx` such that any PTY bytes captured
  *before* the snapshot was assembled appear *before* the snapshot
  reply on the wire, and bytes captured *after* appear *after*.

- **FR2 — Client routes the reply through the existing
  `Snapshot | SnapshotRestore` branch:** `tabs.rs::apply_mux_message`
  MUST dispatch the reply to its existing `MessageType::Snapshot |
  MessageType::SnapshotRestore` arm (`tabs.rs:900`), which already
  selects between `dispatch_offthread_replay` (payload ≥ 64 KiB) and
  the synchronous `reset_frame_for_replay` path. No new branch is
  added on the client; the routing change is purely upstream.

- **FR3 — `PtyOutput`-as-snapshot fallback is removed for the
  `RequestPaneSnapshot` reply path on the daemon side:** the daemon MUST
  NOT emit a `RequestPaneSnapshot` reply through the `PtyOutput`
  channel after this change. If `handle_request_pane_snapshot` or any
  test in `src-tauri/src/mux/ipc/` still composes the reply as a
  `MessageType::PtyOutput`-typed `PtyOutputChunk`, it MUST be updated
  to use the new snapshot-typed path. **Out of scope for this task:**
  the resume snapshot emitted by `resume_pane_with_permit`
  (`SetVisibility(true)` transition) and the reattach snapshot emitted
  by `send_reattach_data` are explicitly NOT re-routed by this
  feature; both continue to flow through `PtyOutput`. They are
  recorded as follow-ups in §Out of Scope.

- **FR4 — Version-skew compatibility (both directions):**
  - New daemon × old client: the old client's `Snapshot |
    SnapshotRestore` arm must accept the reply without crashing or
    desynchronizing. (The arm has existed since before this task; this
    is a no-code-change requirement that must be verified by reading
    the existing client code.)
  - New client × old daemon: the new client must continue to process
    `PtyOutput`-delivered snapshots through its live-input path
    without crashing. (Same: existing behavior verified, not changed.)
  - No change to the `MessageType` enum, no new opcode, no change to
    the `Snapshot` / `SnapshotRestore` payload schema. The choice
    between `Snapshot` and `SnapshotRestore` for the new reply is an
    implementation decision to be made in `sdd.2-create-plan`; both
    are already accepted by the client arm.

- **FR5 — Ordering invariants preserved:** the existing daemon-side
  invariants documented around `handle_request_pane_snapshot`
  (handlers.rs:395-414) and exercised by the ordering tests at
  `handlers.rs:725-925` (e.g. `snapshot chunk must appear with
  pane_output_tx already in <state>`, `pane_output_tx` capacity-1
  serialization) MUST continue to hold. Tests in that module MUST
  pass without modification, or be updated only to reflect the new
  message type while keeping the ordering assertions intact.

### Non-Functional Requirements

- **NFR1 — Performance (production wall time):**
  Tab-switch reattach of a ~2 MiB snapshot, measured end-to-end with
  the `[mux-perf]` instrumentation (`request_pane_snapshot SENT`
  timestamp → `apply_offthread_swap DONE` timestamp):
  - **MUST:** < 1000 ms
  - **SHOULD:** < 200 ms
  - **STRETCH:** < 100 ms
  Measured in a `make build` release binary on the user's local
  machine. The existing `snapshot_replay_bench_2mib_seq` bench
  (51 ms) supplies the lower bound; IPC + off-thread swap +
  `apply_queued_live_output` overhead accounts for the rest.

- **NFR2 — Protocol / wire-format stability:** No change to the
  `MessageType` enum values, no new opcode, no change to the
  snapshot byte-stream layout produced by the daemon. The only
  on-the-wire delta is the `msg_type` byte of the framing header
  for the `RequestPaneSnapshot` reply, flipping from `0x01`
  (`PtyOutput`) to `0x0C` (`Snapshot`) or `0x0D`
  (`SnapshotRestore`).

- **NFR3 — Correctness of existing test surfaces:** All tests in
  `crates/term_core`, `src-tauri/src/mux/`, `src-tauri/src/tabs.rs`,
  and `crates/mux_ipc` MUST pass. The ordering-invariant tests
  noted in FR5 MUST pass.

- **NFR4 — Portability:** Linux release build (`make build`) and
  Windows cross-build (`make win-build`) MUST succeed. CLI-only
  (`--no-default-features`) MUST type-check.

- **NFR5 — Instrumentation lifecycle:** The 5 `log::warn!("[mux-perf]
  ...")` instrumentation sites added in `src-tauri/src/tabs.rs`
  during the investigation (covering `request_pane_snapshot SENT`,
  `apply_mux_message::Snapshot|SnapshotRestore`,
  `dispatch_offthread_replay` worker START/DONE, `apply_offthread_swap`
  entry/exit, `apply_queued_live_output` entry/exit) MUST remain in
  place during implementation and `sdd.6-verify` to support the
  NFR1 measurement. The instrumentation MUST be removed (reverted)
  in a final cleanup commit after `sdd.6-verify` records the
  measured numbers.

## Implementation Approach

### Architecture

The change is local to the mux IPC layer: one daemon-side handler
function and the channel/send path it uses. No new modules, no
public API changes, no `MessageType` enum additions, no client-side
routing changes.

```
┌──────────────────────────────────────────────────────────────────┐
│  client (tabs.rs)                                                │
│   request_pane_snapshot ──────────────► daemon                   │
│                                                                  │
│  apply_mux_message                                               │
│    ├─ Snapshot | SnapshotRestore ──► dispatch_offthread_replay   │
│    │   (uses build_from_snapshot + scrollback_bypass)            │
│    └─ PtyOutput ──► live-input path                              │
│                     (no longer reached for snapshots)            │
└──────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │ msg_type = Snapshot (0x0C) or
                                  │            SnapshotRestore (0x0D)
                                  │
┌─────────────────────────────────┴────────────────────────────────┐
│  daemon (src-tauri/src/mux/ipc/handlers.rs)                      │
│   handle_request_pane_snapshot                                   │
│     ├─ build snapshot bytes (unchanged)                          │
│     └─ send as MessageType::Snapshot (was: PtyOutput)            │
│         ordered against pane_output_tx PTY chunks                │
└──────────────────────────────────────────────────────────────────┘
```

### Data Flow (post-change)

```
client: RequestPaneSnapshot(pane_id) ─────────► daemon
                                                  │
                                          assemble snapshot
                                                  │
                  MuxMessage{type=Snapshot, payload≈2MiB}
client ◄──────────────────────────────────────────┘
   │
   └─► apply_mux_message::Snapshot | SnapshotRestore
         ├─ payload ≥ 64 KiB → dispatch_offthread_replay
         │     worker thread: build_from_snapshot(... scrollback_bypass on ...)
         │     main thread:   apply_offthread_swap, then
         │                    apply_queued_live_output (any PTY bytes that
         │                    arrived during the off-thread replay)
         └─ payload <  64 KiB → reset_frame_for_replay (synchronous)
```

### Send-path options

Two implementation options, to be decided in `sdd.2-create-plan`:

**Option A — repurpose `pane_output_tx` to carry a tagged enum.**
Change the channel element type from `PtyOutputChunk` to an enum
with `PtyChunk(PtyOutputChunk)` and `SnapshotReply { pane_id, data }`
variants. The connection writer matches on the enum and emits the
appropriate `MessageType`. Preserves all existing ordering
guarantees automatically because the same channel is used.

**Option B — add a sibling snapshot send path with explicit
serialization.** Keep `pane_output_tx` typed as `PtyOutputChunk`
and add a separate sender (channel or direct connection write)
for snapshot replies. Serialize the snapshot send against
`pane_output_tx` by, e.g., draining all queued PTY chunks for the
target pane before writing the snapshot frame, or by reserving a
permit on `pane_output_tx` to flush the queue.

Option A is the leading candidate on grounds of ordering correctness
(the existing tests already cover the channel-mediated ordering);
Option B is the fallback if Option A's refactor footprint is
disproportionate. Both satisfy FR1 and FR5.

### Dependencies

**Internal:**
- `src-tauri/src/mux/ipc/handlers.rs` —
  `handle_request_pane_snapshot` body change (FR1, FR3, FR5).
- `src-tauri/src/mux/ipc/connection.rs` (or adjacent) —
  send-path plumbing if Option B is chosen (FR1).
- `src-tauri/src/tabs.rs` — no functional change; instrumentation
  remains in place during implementation, removed in cleanup
  (NFR5).

**External:** none new.

### File Structure (changes only)

```
src-tauri/src/mux/ipc/
├── handlers.rs              # handle_request_pane_snapshot reply path (FR1, FR3, FR5)
└── connection.rs (or sibling)  # send-path plumbing if Option B (FR1)

src-tauri/src/
└── tabs.rs                  # [mux-perf] instrumentation lifecycle (NFR5)
```

## Test Scenarios

### Unit Tests

- [ ] **TS-1** All existing tests in `src-tauri/src/mux/ipc/handlers.rs`
      (the ordering-invariant module around lines 725-925) pass without
      modification, or only with `msg_type` assertion updates that
      preserve the ordering semantics. Covers FR1, FR5.
- [ ] **TS-2** A new unit test asserts that a `RequestPaneSnapshot`
      reply emitted by `handle_request_pane_snapshot` carries
      `msg_type == MessageType::Snapshot` (or `SnapshotRestore`),
      not `PtyOutput`. Covers FR1, FR3.
- [ ] **TS-3** A new unit test asserts that when PTY bytes are
      enqueued on `pane_output_tx` both before and after the
      snapshot reply is sent, the on-wire order is `pre-PTY,
      snapshot, post-PTY`. Covers FR1, FR5.

### Integration Tests

- [ ] **TS-4** `cargo test --manifest-path src-tauri/Cargo.toml`
      green (covers NFR3, including all `tabs.rs` replay tests and
      all `mux/ipc/` tests).

### Cross-build / CLI Tests

- [ ] **TS-5** `cargo check --manifest-path src-tauri/Cargo.toml
      --no-default-features` green (NFR4).
- [ ] **TS-6** `make win-build` green (NFR4).

### Manual / Experiential Tests (via `[mux-perf]` instrumentation)

- [ ] **TS-7** Build a release binary (`make build`), kill any
      running daemon (`pkill -f "emterm.*mux.*daemon"`), launch the
      new client so the daemon spawns from the new binary, run
      `seq 1 10000000` in one mux tab, switch away and back, and
      grep `[mux-perf]` from
      `~/.local/share/net.laser5.app.emterm/logs/emterm.log`.
      Expected pattern:
      ```
      [mux-perf] request_pane_snapshot SENT pane=X (t=0)
      [mux-perf] snapshot RECEIVED type=Snapshot payload=~2MB
      [mux-perf] build_from_snapshot START
      [mux-perf] build_from_snapshot DONE in <Tms>
      [mux-perf] offthread swap START queued_live=<n>
      [mux-perf] offthread swap DONE total=<Tms>
      ```
      Covers NFR1 (record actual `RECEIVED → swap DONE` delta and
      compare against MUST/SHOULD/STRETCH).
- [ ] **TS-8** Version-skew functional check (new daemon × old
      client): run a prior-build client against a freshly-built
      daemon and confirm tab switch still works (functional, not
      performance). Covers FR4.
- [ ] **TS-9** Version-skew functional check (new client × old
      daemon): run the freshly-built client against an unupgraded
      daemon and confirm tab switch still works. Covers FR4.

### Edge Cases

- [ ] **EC-1** Empty snapshot payload (pane just attached, no PTY
      output yet): the new reply path must still deliver a valid
      `Snapshot`-typed message and the client must process it
      without panicking. The existing client `Snapshot` arm
      already handles empty payloads.
- [ ] **EC-2** Snapshot reply size > 64 KiB threshold for
      `dispatch_offthread_replay`: this is the main perf path; TS-7
      exercises it. No additional mitigation needed.
- [ ] **EC-3** Snapshot reply size < 64 KiB threshold: the client's
      synchronous `reset_frame_for_replay` branch is taken; the
      change to message type does not affect that branch.

## Security Considerations

Not applicable. This change does not cross any new trust boundary,
does not alter how external data is parsed or escaped, and does not
change authentication or authorization paths.

## Error Handling

No new error paths. If the new send path fails (channel closed,
connection dropped), it MUST fail with the same semantics as the
current `pane_output_tx.send(chunk).await` error path
(`handlers.rs:495`): log a warning and continue, without panicking
or desynchronizing the connection.

## Performance Goals

| Measurement                                       | Current  | Target                                                | Asserted by |
| ------------------------------------------------- | -------- | ----------------------------------------------------- | ----------- |
| Tab-switch wall time, 2 MiB snapshot (production) | ~3000 ms | < 1000 ms MUST, < 200 ms SHOULD, < 100 ms STRETCH     | TS-7 (manual, `[mux-perf]` log) |
| `snapshot_replay_bench_2mib_seq` (lower bound)    | 51 ms    | preserved (already asserted by snapshot-replay-perf)  | (already in TS-6 of that task) |

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented.
- [ ] All test scenarios TS-1..TS-9 pass.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` green.
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml
      --no-default-features` green.
- [ ] `make win-build` green.
- [ ] NFR1 MUST (< 1000 ms) achieved on the user's local machine and
      recorded in `VERIFICATION_RESULT.md`; SHOULD / STRETCH
      recorded as actual numbers.
- [ ] `[mux-perf]` instrumentation reverted in a final cleanup
      commit after `sdd.6-verify` records the numbers (NFR5).

## Out of Scope

The following items, surfaced during the predecessor task's
multi-review, are explicitly **not** addressed by this task and are
recorded as follow-ups:

- **Layer-violation refactor (codex-architecture, high):** moving
  `bypass_b_mark_texts` out of `term_core` into the mux layer.
- **Medium-priority perf items (Claude perf review):** unbounded
  HashMap, per-cell `String` allocation, `clone` vs `remove`
  micro-optimizations on the bypass path.
- **Re-routing the resume snapshot (`resume_pane_with_permit`,
  `SetVisibility(true)`):** this path also emits snapshot-shaped bytes
  through `PtyOutput`. The current feature only targets the
  tab-switch `RequestPaneSnapshot` flow that motivates NFR1; the
  resume path keeps the live-input replay route. A follow-up task
  may extend the new `kind == Snapshot` channel route to this site.
- **Re-routing the reattach snapshot (`send_reattach_data`):**
  reattach emits a `PaneCreated` frame followed by chunked
  `MessageType::PtyOutput` carrying the buffered snapshot. Same
  rationale as the resume path; deferred to a follow-up.

### Known Limitation: post-bypass scrollback is empty until live PTY refill

When the snapshot payload routed through this task's new path is ≥ 64 KiB
(off-thread `dispatch_offthread_replay` branch), `build_from_snapshot` runs
with `scrollback_bypass` on and intentionally does NOT populate
`scrollback_slim`. The displayed core therefore has `scrollback_count() == 0`
post-swap, so `render::build_cell_grid` sees no historical rows and the user
cannot scroll up through the snapshotted history until live PTY output
refills scrollback. Below the 64 KiB threshold the synchronous
`reset_frame_for_replay` path still populates scrollback as before, so the
behavior is threshold-dependent.

Recorded as a follow-up: **scrollback restoration on the off-thread bypass
path** (see `doc/tasks/snapshot-replay-scrollback-restore/`).

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

(none — Option A vs Option B for the send path and `Snapshot` vs
`SnapshotRestore` for the reply opcode are implementation decisions
deferred to `sdd.2-create-plan` and are not requirement-level
ambiguities.)

## References

- Predecessor task: `doc/tasks/snapshot-replay-perf/`
- Primary source files:
  - `src-tauri/src/mux/ipc/handlers.rs`
    (`handle_request_pane_snapshot`)
  - `src-tauri/src/mux/ipc/connection.rs` (and sibling modules
    in `src-tauri/src/mux/ipc/`)
  - `src-tauri/src/tabs.rs` (`apply_mux_message`,
    `dispatch_offthread_replay`, `apply_offthread_swap`,
    `apply_queued_live_output`, `[mux-perf]` instrumentation)
  - `crates/mux_ipc/src/protocol.rs` (`MessageType` enum)
  - `crates/term_core/src/terminal_core.rs`
    (`build_from_snapshot`, used by the off-thread replay path)
