# Feature: mux Scrollback Retention

## Overview

Convert the mux daemon's per-pane buffer from a detach-only ring (`DetachRingBuffer`, 64 MB pre-allocated on detach) into a permanent scrollback buffer (`ScrollbackRingBuffer`, 2 MB pre-allocated at pane creation). The buffer is written on every PTY read regardless of attach state, allowing reattach to restore scrollback that accumulated during the previous attach as well as during detach.

## Objectives

- Restore pre-detach scrollback on reattach (currently lost)
- Reduce daemon memory footprint (64 MB → 2 MB per pane)
- Make memory usage predictable (`pane_count × 2 MB`, independent of attach state)
- Preserve existing attach/detach/reattach semantics and IPC protocol

## User Stories

### US1: Scrollback preserved across detach
As a mux user, I want my terminal scrollback to remain after reattaching, so that I can scroll back to logs produced before I detached.

**Acceptance Criteria:**
- [ ] After producing N pages of output while attached, detach, then reattach: at least the last 2 MB worth of bytes is recoverable as scrollback in the GUI
- [ ] Output produced during detach is also recoverable

### US2: Stable daemon memory footprint
As a mux user running 10–20 panes, I want the daemon's memory footprint to stay bounded and predictable, so that long-running sessions do not balloon.

**Acceptance Criteria:**
- [ ] 10 panes consume ≤ 20 MB of scrollback memory regardless of attach state
- [ ] No 64 MB allocation spike happens at the moment of detach

## Technical Requirements

### Functional Requirements

- **FR1: ScrollbackRingBuffer type** — Rename `DetachRingBuffer` to `ScrollbackRingBuffer`. Rename file `src-tauri/src/mux/ring_buffer.rs` to `src-tauri/src/mux/scrollback_buffer.rs`. Algorithm (wrap-around `write`, `read_all`, `clear`, `len`, `is_empty`, `capacity`) is unchanged.

- **FR2: Capacity constant 2 MB** — Replace `DEFAULT_RING_CAPACITY: usize = 64 * 1024 * 1024` with `DEFAULT_SCROLLBACK_CAPACITY: usize = 2 * 1024 * 1024`. The constant lives in `scrollback_buffer.rs` and is not exposed to user settings.

- **FR3: Pane-resident buffer** — Add `scrollback: Arc<StdMutex<ScrollbackRingBuffer>>` (or equivalent type alias) as a direct field of `MuxPane`. Allocate the buffer at pane creation time. Remove the `ring: DetachRingBuffer` field from `PaneOutputTarget::Detached`.

- **FR4: Always-on write** — The PTY reader path writes every chunk of PTY output to `pane.scrollback` regardless of the current `PaneOutputTarget` state. When `PaneOutputTarget::Connected`, the chunk is additionally sent to the GUI channel; when `PaneOutputTarget::Detached`, no channel send occurs but the scrollback write still happens.

- **FR5: Reattach send order** — On reattach, daemon builds the resume snapshot as the concatenation of, in order:
  1. `ESC [ H ESC [ 2 J` (clear screen + cursor home)
  2. `scrollback.read_all()` (the accumulated raw PTY history, up to 2 MB)
  3. `shadow.contents_formatted()` (final-screen state from the vt100 shadow parser)
  4. `passthrough_data` (raw_passthrough buffer drained as today — image/Markdown OSC accumulated during detach)

- **FR6: Buffer lifetime equals pane lifetime** — On reattach the daemon calls `scrollback.read_all()` but **does not** call `scrollback.clear()`. The buffer continues accumulating after reattach. The buffer is dropped only when the pane is destroyed.

- **FR7: No ESC-boundary trimming** — When `scrollback` has wrapped, its head may begin mid-ESC-sequence. No trimming is performed; the WASM parser may misparse the leading bytes, but the subsequent `shadow.contents_formatted()` (item 3 in FR5) overwrites the visible screen to a known good state.

- **FR8: Migration of existing tests** — All existing tests in `ring_buffer.rs` (`test_empty_buffer`, `test_simple_write_read`, `test_multiple_writes`, `test_wrap_around`, `test_overflow_large_write`, `test_exact_capacity`, `test_clear`, `test_write_after_clear`, `test_capacity`, `test_repeated_small_writes_overflow`) are carried over to `scrollback_buffer.rs` unchanged in semantics.

- **FR9: passthrough_data unchanged** — The `raw_passthrough` buffer continues to accumulate only while the pane is in `Detached` state, as today. On reattach it is drained and cleared. No change to this code path.

### Non-Functional Requirements

- **NFR1 - Memory:** Steady-state memory for scrollback is `pane_count × 2 MB`. For 20 panes that is 40 MB. No 64 MB-per-pane allocation spike on detach.
- **NFR2 - Latency:** Per-byte scrollback write cost matches the existing detach-time ring write (memcpy into a fixed buffer). The reattach payload is bounded to `2 MB + screen + passthrough` per pane.
- **NFR3 - Compatibility:** Mux IPC frame format, OSC handshake, and WASM-side parser are unchanged. Existing E2E specs (`mux.e2e.js`, `mux-reattach.e2e.js`, `mux-multi-session.e2e.js`) must keep passing.

## Implementation Approach

### Architecture

**Daemon-side data model after the change:**

```
MuxPane
├─ id: PaneId
├─ shadow_parser: SharedShadowParser           // unchanged
├─ scrollback: Arc<StdMutex<ScrollbackRingBuffer>>  // NEW, always present
├─ raw_passthrough: ...                        // unchanged
├─ output_target: Arc<StdMutex<PaneOutputTarget>>
│   ├─ Connected(Sender<PtyOutputChunk>)
│   └─ Detached { reason, owner }              // no more `ring`
└─ ...
```

**Write path (per PTY chunk):**

```
PTY read N bytes
  ├─→ pane.scrollback.lock().write(&bytes)     // always
  ├─→ shadow_parser.process(&bytes)            // always (existing)
  └─→ if PaneOutputTarget::Connected(tx): tx.send(bytes)
      if PaneOutputTarget::Detached:           // do not send; do collect passthrough as today
```

**Reattach path:**

```
collect_reattach_data() for each pane:
  combined = ESC[H ESC[2J
  combined += pane.scrollback.lock().read_all()     // do NOT clear
  combined += shadow_parser.contents_formatted()
  combined += raw_passthrough.lock().read_all() then clear()
  flip output_target to Connected(new_tx)
  emit PaneCreated + PtyOutput(combined)
```

### Data Flow

```
[Steady state - attached or detached]
PTY → pane.scrollback.write(...)
     → shadow_parser.process(...)
     → (if Connected) sender → bridge → client

[Reattach]
client → AttachMsg → daemon
daemon → for each pane: build combined bytes (FR5 order)
       → PaneCreated + PtyOutput(combined)
client → WASM parse → grid + scrollback
```

### File Structure

```
src-tauri/src/mux/
├── scrollback_buffer.rs        # renamed from ring_buffer.rs
│   ├── DEFAULT_SCROLLBACK_CAPACITY: usize = 2 * 1024 * 1024
│   └── pub struct ScrollbackRingBuffer { ... }
├── mod.rs                       # `pub mod scrollback_buffer;` (replaces ring_buffer)
├── session/
│   └── pane.rs                  # MuxPane.scrollback field, PaneOutputTarget::Detached no longer holds ring
└── ipc/
    └── reattach.rs              # build_resume_snapshot uses scrollback before shadow
```

### Dependencies

**Internal Dependencies:**
- `mux::session::pane`: `MuxPane`, `PaneOutputTarget` (modified)
- `mux::ipc::reattach`: `collect_reattach_data`, `build_shadow_parser_snapshot` (modified)
- `mux::session::manager`: pane construction / destruction sites
- All call sites of `DetachRingBuffer` / `DEFAULT_RING_CAPACITY` (renamed)

**External Dependencies:**
- No new dependencies.

## Test Scenarios

### Unit Tests

In `scrollback_buffer.rs` (carried over from `ring_buffer.rs`):

- [ ] `test_empty_buffer`: new buffer has `len()==0`, `read_all()==Vec::new()`
- [ ] `test_simple_write_read`: single small write reads back identically
- [ ] `test_multiple_writes`: two writes concatenate in order
- [ ] `test_wrap_around`: writing past `capacity` keeps the tail
- [ ] `test_overflow_large_write`: a single write larger than capacity keeps only the last `capacity` bytes
- [ ] `test_exact_capacity`: writing exactly `capacity` bytes
- [ ] `test_clear`: clear empties the buffer
- [ ] `test_write_after_clear`: after clear, the next write starts at position 0
- [ ] `test_capacity`: `capacity()` returns the configured size
- [ ] `test_repeated_small_writes_overflow`: many 1-byte writes wrap correctly

Additional new unit tests:

- [ ] `test_default_capacity_is_2mb`: `DEFAULT_SCROLLBACK_CAPACITY == 2 * 1024 * 1024`
- [ ] `test_pane_has_scrollback_at_creation`: a fresh `MuxPane` exposes a non-`None` scrollback whose `len()==0` and `capacity()==2MB`
- [ ] `test_pane_scrollback_writes_in_connected_state`: when `PaneOutputTarget::Connected(_)`, writes to scrollback still accumulate
- [ ] `test_pane_scrollback_writes_in_detached_state`: when `PaneOutputTarget::Detached{..}`, writes still accumulate (covers existing detach-buffering semantics)
- [ ] `test_reattach_snapshot_order_history_then_shadow`: given a pane with mocked scrollback and shadow parser, `collect_reattach_data` emits bytes in the order `ESC[H ESC[2J → scrollback → shadow → passthrough`
- [ ] `test_reattach_does_not_clear_scrollback`: after `collect_reattach_data`, the scrollback `len()` is unchanged (no `clear()` called)
- [ ] `test_pane_output_target_detached_has_no_ring_field`: compile-time check (struct construction) that the `ring` field is removed

### Integration Tests

- [ ] Existing Rust tests covering reattach paths (`mux::ipc::reattach` callers) compile and pass after the rename and field changes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml -p emterm-lib` passes in Docker

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/mux*.e2e.js` (mux.e2e.js, mux-reattach.e2e.js, mux-multi-session.e2e.js, mux-osc-title-propagation.e2e.js)
**Run command**: `./scripts/run-e2e-docker.sh test`

- [ ] Existing E2E tests pass without regression
- [ ] No new E2E spec is added (per design decision — scrollback retention is validated by Rust unit tests and manual verification)

### Edge Cases

- [ ] Pane created while no client is attached: scrollback buffer is still allocated and accumulates
- [ ] Pane created, immediately destroyed: buffer is dropped along with the pane (no leak)
- [ ] Repeated detach/reattach cycles: each reattach reads the (cumulative) scrollback without clearing it
- [ ] PTY chunk exactly equals 2 MB: matches existing `test_exact_capacity`
- [ ] PTY chunk larger than 2 MB: matches existing `test_overflow_large_write`
- [ ] Wrap-around mid-ESC-sequence: shadow snapshot at FR5[3] corrects the visible screen; scrollback may contain a corrupt prefix but is not specially trimmed

### Performance Tests

Manual / inspection only:

- [ ] Daemon RSS measured with 10 idle panes attached is approximately `baseline + 10 × 2 MB`
- [ ] Daemon RSS does not jump by ~64 MB per pane at the moment a client detaches

## Security Considerations

- **Data exposure:** Scrollback contents are PTY output. They are memory-resident only and never persisted to disk by this feature. No new exposure surface beyond the existing detach ring buffer.
- **Multi-client model:** Identity / owner semantics of `PaneOutputTarget::Detached` are unchanged (same `DetachReason` / `owner` fields). Scrollback access is gated by the existing reattach owner check.

## Error Handling

This is an internal refactor; no new user-facing error codes. Failure modes:

- Lock poisoning on `pane.scrollback`: handled the same way as other `StdMutex` locks in `pane.rs` (panic; we do not currently recover from poisoned daemon locks).
- Allocation failure (`Vec<u8>::with_capacity(2MB)`): treated as a fatal pane-creation error (already true for the existing 64 MB ring).

## Performance Optimization

### Performance Goals

- Memory: ≤ `pane_count × 2 MB` of scrollback storage
- Write overhead: O(N) memcpy per PTY chunk (matches existing detach ring path)
- No incremental reallocation: buffer is pre-allocated at pane creation

### Optimization Strategies

- **Pre-allocation:** Single `vec![0u8; 2 MB]` per pane, no growth — keeps the hot write path lock-friendly and predictable.
- **No per-byte work:** Use the existing `write()` method (slice copies, no parsing) for hot path.
- **Reattach payload bound:** scrollback contribution to reattach payload is hard-capped at 2 MB, which is 32× smaller than today.

## Success Criteria

- [x] All FR1–FR9 are implemented
- [x] All unit tests listed above pass
- [x] `cargo test --manifest-path src-tauri/Cargo.toml` is green (252/252 mux tests)
- [x] `bun run typecheck` and `bun test` not affected (no TS files touched in this change)
- [x] No remaining references to `DetachRingBuffer`, `DEFAULT_RING_CAPACITY`, or `ring_buffer.rs`
- [~] Existing E2E specs run without regression: `./scripts/run-e2e-docker.sh test`
      — `mux.e2e.js` / `mux-reattach.e2e.js` / `mux-multi-session.e2e.js`
      currently fail, but reproduce identically on `main @ 2a0d903` with
      this branch reverted, so the failure is a pre-existing regression
      tracked separately (not caused by this feature).
- [ ] Manual: after `attach → big output → detach → reattach`, scrollback in the GUI reaches back into the pre-detach output (requires `bun tauri dev` — pending user verification)

## Open Questions

None at SPEC time. All ambiguities were resolved during `sdd.1-create-spec`.

## Implementation Phases

Delivered in three phased commits on `feat/mux-scrollback-retention` (plus
one chore commit for incidental fmt drift). See `IMPLEMENTATION.md` for
per-phase detail and `tasks.yaml` for the commit-to-phase mapping.

**Phase A — Cap the existing ring at 2 MiB** (commit `7b668f3`)
- Single-line constant change in `mux/ring_buffer.rs`. Behavior otherwise
  identical to today (still detach-only buffering). Shrinks the
  worst-case detach memory spike from 64 MiB to 2 MiB per pane.

**Phase B — Move the buffer to MuxPane.scrollback** (commit `dbe85b0`)
- Rename `DetachRingBuffer` → `ScrollbackRingBuffer` and
  `ring_buffer.rs` → `scrollback_buffer.rs`. Drop the `ring` field from
  `PaneOutputTarget::Detached` and add `scrollback: SharedScrollback` to
  `MuxPane`. Write timing and reattach send order are intentionally
  preserved at this step (still detach-only writes, shadow-first order)
  to keep the structural refactor small and reviewable.

**Phase B chore — cargo-fmt drift** (commit `fda9e26`)
- Mechanical import-order and line-wrap fixes picked up by
  `cargo fmt --edition 2024` over the mux crate.

**Phase C — Always-on write, FR5 reorder, no-clear** (commit `a1321a7`)
- Hoist `scrollback.write` to the top of `pty_reader_loop`, before the
  `output_target` match, so attach-time bytes are captured. Remove the
  three per-arm `scrollback.write` calls that would now double-write.
- Reorder the resume snapshot composer in `collect_reattach_data`,
  `resume_pane_with_permit`, and the Detached → Connected branch of
  `evaluate_output_target` to FR5: `ESC[H ESC[2J → scrollback → shadow
  → passthrough`. Drop the `scrollback.clear()` call so the buffer
  lives for the lifetime of the pane (FR6).

## References

- Memo / decision log: `tmp/daemon-log.md`
- Prior mux protocol design: `doc/tasks/mux-osc-handshake/SPEC.md` (FR2 daemon-side grid, FR8 reattach restoration)
- Implementation: `src-tauri/src/mux/scrollback_buffer.rs`, `src-tauri/src/mux/ipc/reattach.rs`, `src-tauri/src/mux/ipc/pty_spawn.rs`, `src-tauri/src/mux/session/pane.rs`
