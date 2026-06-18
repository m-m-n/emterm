# Feature: mux Off-Thread Snapshot Replay (案a)

## Overview

When switching mux panes/windows, the client currently reparses the target pane's
entire scrollback (up to ~2 MiB) synchronously on the winit event loop (the UI/render
thread). Release measurement shows this blocks the UI thread for 30–230 ms per switch,
proportional to history size. This feature moves the heavy VT reparse to a worker
thread and swaps the completed `TerminalCore` in on the main thread, so the UI thread
no longer blocks during pane/window switches.

The data model is unchanged (1 tab = 1 core, snapshot swap-in, inactive-pane output
retained by the daemon). Only the heavy parse is moved off the UI thread.

## Objectives

- Keep the UI/render thread responsive during pane/window switches regardless of the
  target pane's scrollback size.
- Preserve all invariants established by `mux-scroll-isolation` (FR2 no residual rows,
  FR3 per-pane scroll restore) and marks/folds/selection consistency.
- Avoid the wider redesigns rejected in the design memo (no per-pane resident cores =
  案c; no LRU cache of recent panes).

## Background / Decision Basis

- `mux-scroll-isolation` FR1 (commit `e8e538f`) made the on-demand `RequestPaneSnapshot`
  response carry the pane's scrollback. The client replays it via
  `reset_and_replay` → `process_pty_data_fully`, parsing every payload byte synchronously
  inside `App::pump_all` (driven by the winit event loop).
- Release measurement (lto, 2026-06-18,
  `doc/tasks/mux-snapshot-reparse-offthread/VERIFICATION_RESULT.md`):
  256 KiB = 30.1 ms, 1 MiB = 116.6 ms, 2 MiB = 232.5 ms (≈ 8.6 MiB/s).
- These exceed the 50 ms+ threshold from the design memo §4, so the off-thread replay
  (案a) is **GO**. Rapid `Ctrl+B n n n` switching across history-heavy panes accumulates
  visible jank.

## Scope

**In scope:** Off-thread VT reparse of the snapshot payload, with main-thread swap-in,
live-output ordering, marks/folds/selection reconciliation, and per-pane scroll restore.

**Out of scope:**
- LRU cache of the most-recent K panes (explicitly not adopted in this feature).
- 案c (per-pane resident cores) — full redesign, rejected.
- Any change to the WebView (`src/`) build.

## User Stories

### US1: Responsive switching across large-history panes
As a user running history-heavy panes, I want pane/window switches to stay responsive,
so that `Ctrl+B n n n` does not stutter while the new pane's scrollback is reparsed.

**Acceptance Criteria:**
- [ ] On pane/window switch, the render thread does not block proportionally to
      scrollback size.
- [ ] Rapid switching does not accumulate UI stalls.

### US2: No visible blanking during the parse gap
As a user, I want the previous pane to remain visible until the new pane is ready,
so that switching shows no blank flicker.

**Acceptance Criteria:**
- [ ] Between switch request and parse completion, the outgoing pane stays displayed.
- [ ] When the parsed core is ready, the view swaps to the new pane with no blanking
      (or minimal).

### US3: No data loss for live output arriving during the parse
As a user, I want output produced by the incoming pane while its snapshot is parsing to
appear in order, so that nothing is dropped or reordered.

**Acceptance Criteria:**
- [ ] Live `PtyOutput` for the target pane that arrives during the pending switch is
      applied, in order, after the swap.
- [ ] No bytes are lost or reordered relative to the snapshot.

## Technical Requirements

### Functional Requirements

- **FR1 — Off-thread reparse with main-thread swap:** On a pane/window switch whose
  snapshot payload is at or above the size threshold (see FR4), the client copies the
  payload once and hands it to a one-shot worker thread. The worker builds a new
  `TerminalCore` initialized at the current grid size (cols/rows) and runs
  `process_pty_data_fully(payload)`, returning the completed core over a channel. The
  main thread does **not** reset/replay the current core synchronously for that switch.
  On a subsequent `pump_all`, when the completed core is available, the main thread
  swaps `tab.core` to the new core.

- **FR2 — Pending-switch display (keep outgoing pane):** From the switch request until
  the parsed core is swapped in, the outgoing pane remains displayed (a "pending switch"
  state). No blank frame is shown during the gap.

- **FR3 — Live-output ordering:** Live `PtyOutput` for the target pane that arrives
  during the pending switch is queued on the main thread (the displayed core is not
  mutated). After the swap, the queued bytes are applied in arrival order on top of the
  replayed snapshot core, then rendering proceeds.

- **FR4 — Size-threshold fast path:** Switches whose snapshot payload is below a byte
  threshold (`OFFTHREAD_REPLAY_THRESHOLD_BYTES`, default 64 KiB ≈ ~7 ms on the measured
  machine, well under one 60 fps frame) are reparsed synchronously on the main thread
  exactly as today (instant, no pending-switch gap). Only payloads at or above the
  threshold take the off-thread path. The threshold is a named constant.

- **FR5 — Supersession on rapid re-switch:** If a new switch arrives while an
  off-thread parse for an earlier target is still in flight, only the most recent target
  is applied. Results for superseded intermediate targets are discarded (not swapped in),
  and the queued live-output buffer follows the latest target. The outgoing pane stays
  displayed until the latest target's core is ready. Intermediate panes are never briefly
  shown. A **grid resize** during a pending switch also supersedes the in-flight parse:
  the stale core (built at the old cols/rows) is discarded and the snapshot is
  re-dispatched at the new grid.

- **FR6 — Reconciliation split:** The replay work is split into (1) byte → core parsing
  on the worker, and (2) marks/folds/selection/anchor reconciliation and per-pane scroll
  restore on the main thread after the swap. The main-thread reconciliation reproduces
  the existing `reset_frame_for_replay` semantics (prompts/folds reset, marks backfill /
  `pending_frame_reset` latch, selection/anchor drop) and integrates with the existing
  FR3 per-pane scroll restore path from `mux-scroll-isolation`. The marks and
  `evicted_baseline` consumed by the backfill are taken from the **worker-built core's**
  drained values (its eviction counter starts at 0), exactly as the synchronous path
  derives them from a freshly-reset core — not from the displayed (outgoing) core. The
  swap + marks-latch run per owning tab; the selection drop and per-pane scroll restore +
  full redraw are applied for the active tab (background tabs defer them to activation).

- **FR7 — Synchronous fallback on worker failure:** If the worker fails or panics, the
  client falls back to a synchronous main-thread reparse for that switch (the legacy
  path), accepting the one-off block, so correctness is preserved.

### Non-Functional Requirements

- **NFR1 — Invariant preservation:** `mux-scroll-isolation` FR2 (no residual rows from
  the outgoing unit) and FR3 (per-tab / per-pane scroll restore), plus marks/folds/
  selection consistency, are maintained. Non-mux tabs and single-window mux are
  unaffected.

- **NFR2 — Deterministic, non-flaky testability:** The worker-side parse is exposed as a
  pure function (bytes + grid size → `TerminalCore`) and unit-tested directly, without
  going through the flaky `pump_all` path. No new `pump_all`-driven async tests are added
  that worsen existing flakiness.

- **NFR3 — Portability:** `term_core` + `src-tauri` changes keep Linux/Windows GUI builds
  and the CLI-only (`--no-default-features`) build green. The off-thread path is GUI-only.

- **NFR4 — No memory regression:** The parsed core remains 1 per tab; the off-thread path
  holds at most the in-flight worker's core plus the displayed core transiently. No
  per-pane resident cores, no LRU cache.

## Implementation Approach

### Architecture

```
                 switch request
                       │
        ┌──────────────┴───────────────┐
   payload < threshold            payload ≥ threshold
        │ (FR4)                         │ (FR1)
   sync reparse on             copy payload → one-shot worker thread
   main thread (legacy)        ┌────────────────────────────────┐
        │                      │ build TerminalCore @ cols/rows  │
        │                      │ process_pty_data_fully(payload) │
        │                      └───────────────┬─────────────────┘
        │                          channel: completed core
        ▼                                      │
   swap + reconcile        meanwhile: outgoing pane stays shown (FR2)
                           live PtyOutput for target → main-thread queue (FR3)
                                               │
                       next pump_all: core ready (latest target only, FR5)
                                               ▼
                       swap tab.core → apply queued live bytes in order
                       → marks/folds/selection reconcile + scroll restore (FR6)
                       (worker failure → sync fallback, FR7)
```

### Data Flow

1. Switch: send `request_pane_snapshot(pane_id)` to the daemon (unchanged). The client
   does **not** immediately reset the current core; it records a pending switch for the
   target pane.
2. Snapshot received: if payload < threshold (FR4), reparse synchronously and swap now.
   Otherwise copy the payload and dispatch a one-shot worker (FR1); enter/refresh the
   pending-switch state.
3. During the pending switch: live `PtyOutput` for the target pane is queued on the main
   thread; the displayed (outgoing) core is not mutated (FR2, FR3).
4. A newer switch supersedes the pending target (FR5): the live-queue is re-targeted and
   any earlier completed/in-flight core is discarded.
5. Next `pump_all`: if the latest target's core is ready, swap `tab.core`, apply the
   queued live bytes in order, run marks/folds/selection reconciliation, restore per-pane
   scroll, and request a full redraw (FR6). On worker failure, fall back to synchronous
   reparse (FR7).

### Key Locations

- `crates/term_core/src/terminal_core.rs:433-435` — `reset_and_replay` /
  `process_pty_data_fully`. The worker-side pure parse is built here (bytes + grid size
  → `TerminalCore`). Confirm `TerminalCore` is `Send` (no GUI / thread-local deps) so it
  can be constructed on a worker and moved to the main thread.
- `src-tauri/src/tabs.rs` — `reset_frame_for_replay`, `apply_mux_message`
  (Snapshot / SnapshotRestore), `Tab::core`. Pending-switch state, the live-output queue,
  the swap, and the main-thread reconciliation live here.
- `src-tauri/src/app.rs` — `pump_all` (UI-thread driven): poll the worker channel, perform
  the swap + reconcile on completion.
- `src-tauri/src/window_host.rs` — winit event loop (caller of `pump_all`).
- `src-tauri/src/mux/ipc/handlers.rs` / `reattach.rs` — snapshot construction
  (`build_snapshot_bytes`); unchanged, but the source of the payload size used for FR4.

### Difficult Points (must address)

1. **`TerminalCore` `Send`-ness** — verify it can be built on a worker and moved in; no
   GUI/thread-local dependency.
2. **Async gap display** — "pending switch" state must keep the outgoing pane shown.
3. **Live-output ordering (most critical)** — snapshot (off-thread parse) → live bytes
   (main-thread queue) → applied in order after swap.
4. **Replay is more than parse** — split byte→core parsing (worker) from marks/folds/
   selection reconciliation (main thread, post-swap), reproducing
   `reset_frame_for_replay` semantics.
5. **Grid-size consistency** — the worker's core is initialized at the current cols/rows.
6. **Test flakiness** — keep the worker parse a pure function and unit-test it; do not add
   async into `pump_all` tests.

### Dependencies

**Internal Dependencies:**
- `mux-scroll-isolation` (FR1 snapshot-with-scrollback = commit `e8e538f`; FR2/FR3
  invariants this feature must preserve).
- `mux-snapshot-reparse-offthread` (measurement + GO decision recorded in
  `VERIFICATION_RESULT.md`; lock-scope guard-rail already landed).

**External Dependencies:** none (one-shot thread via std; no new crates required).

## Test Scenarios

### Unit Tests
- [ ] Worker-side pure parse: bytes + (cols, rows) → `TerminalCore` produces a core
      byte/grid-identical to the legacy synchronous `reset_and_replay` for the same input.
- [ ] Empty / sub-threshold payload takes the synchronous path (FR4 boundary).
- [ ] Threshold boundary: payload exactly at `OFFTHREAD_REPLAY_THRESHOLD_BYTES` takes the
      off-thread path; one byte below takes the sync path.
- [ ] Live-output queue applied after swap yields the same final grid as parsing
      snapshot+live as one contiguous stream (ordering invariant, FR3).
- [ ] Supersession: given targets A→B→C with B in flight, only C's core is swapped in;
      B's result is discarded (FR5).
- [ ] Worker-failure fallback path reparses synchronously and yields the correct core
      (FR7).

### Integration Tests
- [ ] After off-thread swap, marks/folds/selection and per-pane scroll match the legacy
      synchronous path for the same snapshot (FR6, NFR1).
- [ ] FR2 (no residual rows) holds when swapping to a shorter pane via the off-thread
      path.

### E2E Tests
**Existing E2E tests**: None detected.
**Run command**: Not detected.
- [ ] (manual) `Ctrl+B n n n` across history-heavy panes stays responsive; outgoing pane
      visible until swap; no blank flicker; live output not lost.

### Edge Cases
- [ ] Switch back to the outgoing pane before the worker completes (supersede to original).
- [ ] Grid resize between switch request and swap — supersedes the in-flight parse;
      re-dispatch at the new grid (a stale-sized core is never swapped in).
- [ ] Worker panic mid-parse → synchronous fallback (FR7).

### Performance Tests
- [ ] Switch latency on a ~2 MiB pane: UI thread main-loop iteration does not block
      proportionally to scrollback size (compare against the legacy synchronous path
      measured in `mux-snapshot-reparse-offthread`).

## Performance

### Performance Goals
- Pane/window switch does not block the render thread proportionally to scrollback size;
  switch-induced UI stall is bounded and independent of history size (above the threshold,
  the heavy parse runs off-thread).
- Below `OFFTHREAD_REPLAY_THRESHOLD_BYTES` (default 64 KiB ≈ ~7 ms on the measured
  machine, well under one 60 fps frame), the synchronous path is retained to keep
  small-pane switching instant and gap-free.

## Success Criteria

- [ ] All functional requirements (FR1–FR7) implemented and tested.
- [ ] `mux-scroll-isolation` FR2/FR3 and marks/folds/selection invariants preserved (NFR1).
- [ ] `cargo test` (default, single-thread) green; CLI-only `cargo check` green.
- [ ] No new `pump_all`-driven async tests that worsen existing flakiness (NFR2).
- [ ] Release build only on explicit user request.

## Open Questions

> Note: Unresolved requirements are tracked in sdd.yaml as `status: tbd`.

- None. (Rapid-switch supersession = latest-target-only; small panes = size-threshold
  synchronous; worker failure = synchronous fallback — all confirmed.)

## References

- Follow-up task report: `tmp/mux-offthread-replay-followup.md`
- Primary design memo (§2 = primary design source): `tmp/perf-snapshot-reparse-offthread-plan.md`
- Measurement + GO decision: `doc/tasks/mux-snapshot-reparse-offthread/VERIFICATION_RESULT.md`
- Prerequisite feature (FR1 snapshot-with-scrollback, commit `e8e538f`):
  `doc/tasks/mux-scroll-isolation/`
