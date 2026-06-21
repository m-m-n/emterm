# Implementation Plan: snapshot-replay-daemon-routing

## Overview

Re-route the daemon's `RequestPaneSnapshot` reply from `MessageType::PtyOutput` to `MessageType::Snapshot` so the client dispatches it through `apply_mux_message::Snapshot|SnapshotRestore` and the existing `build_from_snapshot` + `scrollback_bypass` fast path (delivered by `doc/tasks/snapshot-replay-perf/`) becomes effective for real tab-switch traffic.

## Objectives

- Make `handle_request_pane_snapshot` emit the assembled snapshot as `MessageType::Snapshot` while preserving FIFO ordering against PTY chunks already queued on `pane_output_tx`.
- Keep the daemon-side scrollback assembly, the snapshot byte stream, and the client-side `Snapshot|SnapshotRestore` handling behaviorally unchanged.
- Preserve forward / backward compatibility across one daemon × client version skew step.

## Prerequisites

### Development Environment

- Rust toolchain (per `rust-toolchain.toml`).
- `cargo` available with the project's `--manifest-path src-tauri/Cargo.toml` flow.
- `cargo xwin` + `x86_64-pc-windows-msvc` target installed (per `make setup`) for the Windows cross-check (NFR4).

### Dependencies

- Predecessor task `doc/tasks/snapshot-replay-perf/` is merged. `TerminalCore::build_from_snapshot` + `scrollback_bypass` + `dispatch_offthread_replay` exist on the client.
- `[mux-perf]` instrumentation (5 sites in `src-tauri/src/tabs.rs`) is already in place on `refactor/promote-native-poc`.
- No external dependency changes.

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **IPC framing**: `MuxCodec` over a Unix-domain stream (`tokio_util::codec::Framed`)
- **Channel**: `tokio::sync::mpsc` with bounded capacity (`PTY_CHANNEL_CAPACITY = 256`)
- **Protocol**: `crates/mux_ipc/src/protocol.rs::MessageType` (no enum change; pre-existing `Snapshot = 0x0C` and `SnapshotRestore = 0x0D` reused)

### Design Decisions

**Decision 1 — Reply opcode: `MessageType::Snapshot` (0x0C).**
The client `apply_mux_message` arm at `src-tauri/src/tabs.rs:900` already matches `Snapshot | SnapshotRestore`, so either opcode reaches the same `dispatch_offthread_replay` / `reset_frame_for_replay` branch.
- `SnapshotRestore` (0x0D) has no daemon producer at this revision (grep across `src-tauri/` and `crates/`).
- `Snapshot` (0x0C) is already constructed by existing test fixtures (`tabs.rs:3622`, `tabs.rs:4290`) and semantically reads as "the daemon's current state for this pane, replay it into the live core."
- Picking the opcode that already has constructor sites in tests minimises the diff in assertions (TS-1 surface in `handlers.rs:725-925`).

Rationale: lower test-update churn, no semantic difference at the dispatch site, no protocol changes (NFR2).

**Decision 2 — Send path: Option A, minimal variant (extend `PtyOutputChunk` with a `kind` discriminator).**
- The ordering invariant in FR5 / SPEC `handlers.rs:395-414` requires that bytes already queued on `pane_output_tx` for the same pane appear *before* the snapshot, and bytes queued *after* appear after. The only way to honour this without re-implementing the per-pane FIFO is to route the snapshot reply through the same `pane_output_tx` channel.
- A pure sibling channel + direct `framed.send` (Option B) loses ordering: snapshot frames bypass the `pane_output_rx` drain + `merge_consecutive_chunks` batch and arrive interleaved with whatever was mid-flight on the PTY path. Recovering ordering would require draining `pane_output_rx` before each snapshot write, which is what putting the snapshot on the channel already gives us for free.
- Full Option A (channel element becomes a tagged enum `PtyChunk(...) | SnapshotReply { ... }`) is correct but touches all 4 producers (`handle_request_pane_snapshot`, `resume_pane_with_permit`, `pty_spawn` reader thread, `evaluate_output_target`) and the consumer drain in `connection.rs:314-401`.
- Chosen variant: keep `PtyOutputChunk` as the channel element and add a single `kind: ChunkKind` field (`PtyOutput` / `Snapshot`). The default at construction (`PtyOutputChunk::pty_output(pane_id, data)`) is `PtyOutput`. Only `handle_request_pane_snapshot` constructs a `Snapshot` chunk. The drain in `connection.rs` matches on `kind` and emits the appropriate `MessageType` via the existing `MuxMessage::pty_output(...)` or a new `MuxMessage` constructor for the snapshot payload.

Rationale: the smallest refactor that preserves the channel-mediated FIFO ordering already exercised by `handlers.rs` ordering tests; all 4 producers continue to work via the default constructor; the drain change is a single `match` insertion. `merge_consecutive_chunks` gains `kind` as part of its merge key so a `Snapshot` chunk never collapses into adjacent `PtyOutput` chunks (snapshots are framed as a single IPC message regardless of size).

### Component Interaction

Daemon-side (`src-tauri/src/mux/ipc/`):

```
pane reader thread ─► pane_output_tx ─► pane_output_rx (drain) ─► merge ─► framed.send
                       ▲
RequestPaneSnapshot ──┤ same channel, kind=Snapshot
handler              │
                     │
SetVisibility        │
resume path          │
```

Client-side (`src-tauri/src/tabs.rs`): unchanged. `apply_mux_message` already routes `Snapshot|SnapshotRestore` to `dispatch_offthread_replay` (>= 64 KiB) or `reset_frame_for_replay` (< 64 KiB).

## Implementation Phases

### Phase 1: Channel-element discriminator

**Goal**: Extend `PtyOutputChunk` with a `kind: ChunkKind` discriminator without changing default construction behavior, so existing callsites keep working.

**Files to Modify**:
- `src-tauri/src/mux/session/pane.rs` — add `ChunkKind` enum, add `kind` field to `PtyOutputChunk`, expose constructors that default to `PtyOutput`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `ChunkKind` | Tag a channel element as a live PTY chunk or a snapshot reply | – | Variants exhaustively cover the two send-path origins (FR1, FR3) |
| `PtyOutputChunk::pty_output(...)` | Default constructor for the reader thread / resume path | – | Resulting chunk has `kind == ChunkKind::PtyOutput` |
| `PtyOutputChunk::snapshot(...)` | Constructor for the new snapshot reply path | – | Resulting chunk has `kind == ChunkKind::Snapshot` |

**Implementation Steps**:
1. Introduce `ChunkKind` with two variants (PtyOutput / Snapshot).
2. Add `kind` field to `PtyOutputChunk` and provide named constructors so all existing producers compile via the default.
3. Update existing producers (`pty_spawn`, `evaluate_output_target`, `resume_pane_with_permit`, `handle_set_visibility` indirectly through the resume path) to use the `pty_output(...)` constructor — semantic no-op.

**Dependencies**: none. **Blocks**: Phase 2, Phase 3.

**Testing Approach**:
- Unit: round-trip ergonomics — chunk constructed via `pty_output(...)` reports `kind == PtyOutput`; via `snapshot(...)` reports `kind == Snapshot`.
- No behavior change yet — existing tests in `handlers.rs`, `connection.rs::merge_*`, `tabs.rs` MUST stay green with no edits (proves the default-constructor migration is faithful).

**Acceptance Criteria**:
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` green.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` green (no test edits in this phase).

**Estimated Effort**: small

---

### Phase 2: Drain-side dispatch by `kind`

**Goal**: Teach the connection drain loop to emit `MessageType::Snapshot` for `kind == Snapshot` chunks while keeping `MessageType::PtyOutput` for the default.

**Files to Modify**:
- `src-tauri/src/mux/ipc/connection.rs` — in the `pane_output_rx.recv()` arm (currently lines 314-401), branch on `chunk.kind` when constructing `MuxMessage`. `Snapshot` kind → `MuxMessage { msg_type: MessageType::Snapshot, pane_id, payload: data }`; `PtyOutput` (default) → `MuxMessage::pty_output(...)`.
- `src-tauri/src/mux/ipc/connection.rs::merge_consecutive_chunks` — include `kind` in the merge key so a `Snapshot` chunk is never folded into adjacent PTY data.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| drain branch | Encode each chunk as the right `MessageType` | chunk drained from `pane_output_rx` | `framed.feed` sees `Snapshot` framing for snapshot chunks, `PtyOutput` framing for live chunks |
| `merge_consecutive_chunks` | Coalesce adjacent same-pane same-`kind` chunks; never merge across `kind` | input chunks ordered by arrival | snapshot chunks emitted as standalone frames; ordering across panes preserved (existing invariant) |

**Processing Flow**:
1. Drain up to `DRAIN_BATCH_LIMIT` chunks from `pane_output_rx`.
2. Run `merge_consecutive_chunks` (now `kind`-aware).
3. For each merged chunk:
   - kind == Snapshot → feed `MuxMessage { msg_type: Snapshot, pane_id, payload }`
   - kind == PtyOutput, data empty → feed `PtyExited` (existing behavior)
   - kind == PtyOutput, data non-empty → feed `MuxMessage::pty_output(...)`
4. flush.

**Implementation Steps**:
1. Update `merge_consecutive_chunks` to gate the same-pane merge on matching `kind`.
2. Extend the existing per-chunk loop in `connection.rs:348-367` with a `match chunk.kind` branch.
3. Add a `MuxMessage::snapshot(pane_id, payload)` helper next to `MuxMessage::pty_output` in `crates/mux_ipc/src/protocol.rs` (a one-line constructor — keeps the call site symmetric; not a protocol addition because `MessageType::Snapshot` already exists).

**Dependencies**: Requires Phase 1. **Blocks**: Phase 3, Phase 4.

**Testing Approach**:
- Unit (extends existing `merge_efficiency_*` tests): a `Snapshot` chunk surrounded by `PtyOutput` chunks on the same pane MUST surface as three separate frames; total bytes preserved.
- Unit: `merge_consecutive_chunks` on `[PtyOutput, PtyOutput, Snapshot, PtyOutput]` for one pane produces `[merged-2-bytes, snapshot, 1-byte]`.

**Acceptance Criteria**:
- [ ] `merge_consecutive_chunks` never folds across `kind`.
- [ ] Drain branch produces the right `MessageType` for each kind.

**Estimated Effort**: small

---

### Phase 3: Switch `handle_request_pane_snapshot` to the snapshot kind

**Goal**: Make the snapshot reply go out as `MessageType::Snapshot` via the same `pane_output_tx` channel, preserving FR5 ordering.

**Files to Modify**:
- `src-tauri/src/mux/ipc/handlers.rs::handle_request_pane_snapshot` (lines 415-509) — replace `PtyOutputChunk { pane_id, data: snapshot }` (line 496) with `PtyOutputChunk::snapshot(pane_id, snapshot)`. Update the doc-comment block (lines 394-414) to note that the chunk now carries `kind == Snapshot` and routes to `MessageType::Snapshot` on the wire while keeping channel-mediated ordering.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `handle_request_pane_snapshot` | Assemble snapshot bytes (unchanged) and enqueue them as a snapshot chunk | active session match + pane found | one snapshot chunk lands on `pane_output_tx` with `kind == Snapshot`; FIFO ordering against in-flight PTY chunks preserved |

**Processing Flow** (post-change):
1. Resolve `(shadow_parser, scrollback)` under the session lock; refuse cross-session requests (unchanged).
2. Copy scrollback under the scrollback lock (unchanged scoped block).
3. Assemble snapshot via `build_shadow_parser_snapshot` (unchanged).
4. `pane_output_tx.send(PtyOutputChunk::snapshot(pane_id, snapshot)).await` (was: chunk with default kind).
5. On channel-closed error, log warn + continue (unchanged).

**Implementation Steps**:
1. Replace the `PtyOutputChunk { ... }` struct-literal construction with the `snapshot(...)` constructor.
2. Refresh the doc-comment block to reflect the new on-the-wire `msg_type`.

**Scope note (FR3)**: only the `RequestPaneSnapshot` reply is re-routed. Other snapshot-shaped producers — `resume_pane_with_permit` (`SetVisibility(true)` resume path; `src-tauri/src/mux/session/pane.rs:378`) and `send_reattach_data` (reattach buffered output; `src-tauri/src/mux/ipc/reattach.rs:285`) — continue to use `MessageType::PtyOutput`. They are explicitly OUT OF SCOPE per SPEC §Out of Scope and produce their snapshots via `PtyOutputChunk::pty_output(...)` (the default `kind == PtyOutput`) so the merge / drain paths handle them identically to live bytes.

**Dependencies**: Requires Phase 1, Phase 2. **Blocks**: Phase 4 (test updates).

**Testing Approach**:
- Existing `snapshot_bytes_unchanged_after_lock_scope_guardrail` (handlers.rs:736) MUST still pass — the snapshot byte assembly is byte-identical.
- The error-path doc-comment for FR3 in `handlers.rs:495` should still hold (same `pane_output_tx.send().await` semantics).
- Existing `handle_set_visibility_*` tests (handlers.rs:812-1027) and `test_resume_pane_with_permit_*` tests (pane.rs:867-) MUST stay green unchanged — those exercise the out-of-scope resume path, which keeps `kind == PtyOutput`.

**Acceptance Criteria**:
- [ ] No `MessageType::PtyOutput` `RequestPaneSnapshot` replies emitted by this handler.
- [ ] Snapshot bytes byte-identical to predecessor task's output.
- [ ] `resume_pane_with_permit` / `send_reattach_data` continue to emit `kind == PtyOutput` (no behavior change to the out-of-scope paths).

**Estimated Effort**: small

---

### Phase 4: New unit tests (TS-2, TS-3) and ordering-test updates (TS-1)

**Goal**: Add behavioral tests that pin the new opcode (FR1, FR3) and the FIFO ordering of pre-PTY → snapshot → post-PTY (FR1, FR5). Update assertions in the existing `handlers.rs:725-925` test block where they reference the (now-stale) `msg_type`.

**Files to Modify**:
- `src-tauri/src/mux/ipc/handlers.rs` — extend the `#[cfg(test)] mod tests` block with TS-2 and TS-3 cases. The existing resume-path tests (`handle_set_visibility_*`) exercise an OUT-OF-SCOPE path (resume snapshot stays on `PtyOutput`); they MUST stay green with no edits. Ordering assertions on the channel (chunk order, `pane_id`, capacity-1 invariant) MUST stay intact in any case.

Note: scanning the existing tests in `handlers.rs:712-1028` shows they assert on chunks via `rx.try_recv()` and inspect `chunk.pane_id` / `chunk.data` payload markers (`b"\x1b[H\x1b[2J"`), not on `MuxMessage::msg_type`. The chunk-level assertions stay valid because both `PtyOutput` and `Snapshot` chunks reach the receiver identically — the `msg_type` only differs at the drain-side `MuxMessage` construction, which the unit tests do not exercise. TS-1 should therefore be a "no changes required" pass once the drain branch is in place; the only test that may need a `kind` assertion update is one that explicitly inspects the chunk emitted by `handle_request_pane_snapshot` (the in-scope path), which is exactly what new TS-2 covers.

**Key Test Scenarios**:

| ID | Scenario | Verifies |
|----|----------|----------|
| TS-2 | Construct the same setup as `snapshot_bytes_unchanged_after_lock_scope_guardrail`, invoke `handle_request_pane_snapshot`, drain `pane_output_rx`, observe the resulting chunk has `kind == ChunkKind::Snapshot` | FR1, FR3 |
| TS-3 | On a single pane, enqueue a "pre" `PtyOutputChunk` (pty_output), call `handle_request_pane_snapshot`, enqueue a "post" `PtyOutputChunk`. Drain `pane_output_rx` in order; assert the three chunks come out as `[pre(PtyOutput), snapshot(Snapshot), post(PtyOutput)]` and that `pre.data` precedes the snapshot bytes which precede `post.data` | FR1, FR5 |
| TS-1 review | Re-run the existing 725-925 block unchanged; only update `msg_type` assertions if any | FR1, FR5 |

**Implementation Steps**:
1. Add a `handle_request_pane_snapshot_emits_snapshot_kind` test that mirrors the guardrail test's setup, then asserts `chunk.kind == ChunkKind::Snapshot`.
2. Add a `handle_request_pane_snapshot_preserves_fifo_ordering` test that pushes pre/post `pty_output(...)` chunks around the snapshot call and asserts the on-channel order.
3. Re-run the existing `mod tests`; touch assertions only where compilation fails or behavior actually diverged.

**Dependencies**: Requires Phase 1-3. **Blocks**: Phase 5.

**Acceptance Criteria**:
- [ ] TS-2 and TS-3 added and green.
- [ ] Existing handlers.rs ordering tests green with minimal edits (msg_type only).

**Estimated Effort**: small-medium

---

### Phase 5: Cross-target verification

**Goal**: Confirm all target builds and the test matrix stay green; capture the production NFR1 measurement using the existing `[mux-perf]` instrumentation; do NOT revert the instrumentation yet.

**Files to Modify**: none in this phase (verification only).

**Implementation Steps**:
1. `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml` (NFR3, TS-4) — note `tabs.rs` replay tests may need `--test-threads=1` (existing project-wide caveat).
2. `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` (NFR4, TS-5).
3. `make win-build` (NFR4, TS-6).
4. `make build` — release binary for the manual measurement (TS-7).
5. Kill old daemon (`pkill -f "emterm.*mux.*daemon"`), launch new client, produce ~2 MiB scrollback via `seq 1 10000000`, switch tabs, grep `[mux-perf]` from the log, record `RECEIVED → swap DONE` delta.
6. Functional version-skew check pair (TS-8, TS-9): run prior-build client × new daemon, and new client × prior-build daemon; confirm tab switching does not crash.

**Acceptance Criteria**:
- [ ] All NFR1 / NFR3 / NFR4 commands green.
- [ ] `[mux-perf]` log shows the new path is entered: `snapshot RECEIVED type=Snapshot` → `build_from_snapshot START / DONE` → `offthread swap START / DONE`.
- [ ] Measured `RECEIVED → swap DONE` delta meets the MUST threshold (< 1000 ms) on the user's machine; SHOULD / STRETCH recorded.

**Dependencies**: Requires Phase 1-4.

**Estimated Effort**: small (execution) + medium (measurement)

---

### Phase 6 (deferred to sdd.6 cleanup): Revert `[mux-perf]` instrumentation

**NOT IN SCOPE FOR `sdd.4-implement`.** Per NFR5 and SPEC §Success Criteria, the 5 `log::warn!("[mux-perf] ...")` sites in `src-tauri/src/tabs.rs` remain in place during implementation and `sdd.6-verify` so the production measurement can be captured. They MUST be removed in a final cleanup commit AFTER `sdd.6-verify` has recorded the numbers in `VERIFICATION_RESULT.md`.

This phase is listed here only to record the lifecycle intent. VERIFICATION.md tracks the revert as a post-verify item.

---

## Complete File Structure (changes only)

```
src-tauri/src/mux/session/
└── pane.rs                  # ChunkKind enum, PtyOutputChunk { kind } field + constructors (FR1)

src-tauri/src/mux/ipc/
├── handlers.rs              # handle_request_pane_snapshot uses snapshot(...) constructor (FR1, FR3, FR5)
│                            # + TS-2, TS-3 tests added to #[cfg(test)] mod
└── connection.rs            # drain dispatch by ChunkKind; merge_consecutive_chunks keys on kind (FR1, FR5)

crates/mux_ipc/src/
└── protocol.rs              # MuxMessage::snapshot(...) helper (no MessageType change; NFR2)

src-tauri/src/
└── tabs.rs                  # NO CHANGE in implement phase; [mux-perf] sites reverted in post-verify cleanup (NFR5)
```

## Testing Strategy

- **Unit**: TS-2 (snapshot reply has `kind == Snapshot`), TS-3 (FIFO ordering pre-PTY → snapshot → post-PTY), `merge_consecutive_chunks` `kind`-aware coverage extension.
- **Regression**: existing `handlers.rs:725-925` ordering tests, `snapshot_bytes_unchanged_after_lock_scope_guardrail`, all `merge_efficiency_*` tests must stay green.
- **Integration**: full `cargo test --manifest-path src-tauri/Cargo.toml` (NFR3 / TS-4); `tabs.rs` replay tests may require `--test-threads=1`.
- **Cross-build**: `make win-build`, `cargo check --no-default-features`.
- **Manual (production wall-time)**: TS-7 via `[mux-perf]` log; covers NFR1.
- **Manual (version-skew)**: TS-8 / TS-9, functional only.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| – | – | No new external dependencies. All changes use existing `tokio::sync::mpsc`, `MuxCodec`, `MessageType`. |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `merge_consecutive_chunks` regression — `kind` not added to merge key, snapshot bytes get folded into adjacent PTY data | low | high (corrupted snapshot frame; wrong `MessageType` on wire) | Dedicated unit test in Phase 2 covering `[PtyOutput, Snapshot, PtyOutput]` interleaving |
| Hidden assertion in existing tests on the snapshot `msg_type` reaching the client as `PtyOutput` (none found in scan, but cfg-gated paths exist) | low | medium (false-negative test failure during Phase 4) | Phase 4 step is "re-run first, edit only on failure" — preserves intent |
| Version skew: new daemon × very old client lacking the `Snapshot|SnapshotRestore` arm | very low | medium (client crash) | The arm has existed since before `snapshot-replay-perf` (predecessor task) — verified by reading `tabs.rs:900`; no daemon revision predates the arm |
| `[mux-perf]` instrumentation accidentally reverted before measurement | low | medium (cannot evaluate NFR1) | Phase 6 explicitly deferred to post-`sdd.6-verify`; VERIFICATION.md tracks |

## Open Questions

None. Both decisions documented in §Design Decisions are intentional and require no follow-up clarification.

## Success Metrics

- [ ] FR1 — daemon reply carries `MessageType::Snapshot` (verified by TS-2).
- [ ] FR2 — client routes through existing arm (verified by TS-7 log presence of `build_from_snapshot START / DONE`).
- [ ] FR3 — no daemon-side `PtyOutput` snapshot emissions (verified by TS-2 + code review).
- [ ] FR4 — version-skew functional (verified by TS-8, TS-9).
- [ ] FR5 — FIFO ordering preserved (verified by TS-3 + existing ordering tests).
- [ ] NFR1 — `RECEIVED → swap DONE` < 1000 ms MUST on the user's machine.
- [ ] NFR2 — no `MessageType` enum / opcode / payload-schema change.
- [ ] NFR3 — `cargo test` green.
- [ ] NFR4 — `make win-build`, `cargo check --no-default-features` green.
- [ ] NFR5 — instrumentation retained through `sdd.6-verify`; reverted in a final cleanup commit afterward.
