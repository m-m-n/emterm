# Implementation Plan: mux Off-Thread Snapshot Replay (案a)

## Overview

Move the heavy VT reparse of a mux pane's snapshot scrollback off the winit/UI thread
onto a one-shot worker, then swap the completed `TerminalCore` into the tab on a later
`pump_all`, so pane/window switches stay responsive regardless of scrollback size.

## Objectives

- Off-thread the snapshot reparse (≥ a size threshold) and swap the result on the main thread.
- Keep the outgoing pane displayed during the parse gap; apply target-pane live output in order after the swap.
- Preserve all `mux-scroll-isolation` invariants (no residual rows; per-pane scroll restore) and marks/folds/selection consistency.

## Prerequisites

### Development Environment
- Rust toolchain pinned by the repo (`rust-toolchain`), `cargo` with `CARGO_TARGET_DIR` per project build-location rules.

### Dependencies
- Internal: `mux-scroll-isolation` FR1 (snapshot carries scrollback, commit `e8e538f`); `mux-snapshot-reparse-offthread` (GO decision + lock-scope guard-rail already landed).
- External: none. The worker is a one-shot OS thread from the standard library; no new crates.

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Key components**: `term_core::TerminalCore` (VT parse + grid), `src-tauri` `Tab` / `App` (mux client, winit-driven `pump_all`).

### Design Approach

The replay is split into two halves:

1. **Parse half (worker thread):** build a fresh `TerminalCore` at the switch-time grid size and replay the snapshot payload to completion. Pure, no GUI/thread-local state. Returns the built core plus the mode-actions and pending marks drained during the replay.
2. **Reconcile half (main thread, after swap):** swap `Tab::core` to the built core, apply any queued target-pane live bytes in arrival order, then reproduce the existing `reset_frame_for_replay` main-thread effects (prompt/fold reset already done at dispatch time, marks backfill, `pending_frame_reset` latch, alt-screen reseed) and the `mux-scroll-isolation` per-pane scroll restore + full redraw.

A per-tab **pending-switch** state tracks the target pane, a non-blocking completion handoff from the worker, and a **live-output queue** for the target pane. The synchronous path is retained below a byte threshold so small panes switch instantly with no gap.

### Component Interaction

```
switch_to (app.rs) ── send SwitchWindow + request_pane_snapshot ──▶ daemon
        │  (does NOT reset core; records pending-switch target)
        ▼
Tab::pump → apply_mux_message(Snapshot)  (tabs.rs)
        │  payload < threshold → synchronous reset_frame_for_replay (legacy, swap now)
        │  payload ≥ threshold → copy payload, dispatch worker, enter pending-switch
        ▼
worker thread: build TerminalCore @ grid, full-drain replay → completion handoff
        │   meanwhile: target-pane PtyOutput → main-thread live queue (displayed core untouched)
        │   newer switch → supersede: discard in-flight, retarget queue
        ▼
App::pump_all (app.rs): poll completion handoff (non-blocking) for active tab
        │  ready → swap Tab::core → apply queued live bytes in order
        │        → reconcile marks/folds/selection (pending_frame_reset latch)
        │        → restore per-pane scroll + full redraw   (FR6, NFR1)
        │  worker failed → synchronous reparse fallback     (FR7)
```

## Implementation Phases

### Phase 1: Off-thread-safe pure parse in term_core

**Goal**: A pure builder produces a fresh `TerminalCore` from a payload at a given grid size, byte/grid-identical to the synchronous `reset_and_replay`, suitable for construction on a worker thread and move to the main thread.

**Files to Modify**:
- `crates/term_core/src/terminal_core.rs` — add the pure builder alongside `reset_and_replay` / `process_pty_data_fully`; confirm `TerminalCore` thread-safety.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| pure snapshot-replay builder | Construct a fresh core at (cols, rows, scrollback_lines), full-drain replay the payload, return the core + drained mode-actions + drained marks | cols/rows > 0; payload is daemon snapshot bytes | Returned core's grid/scrollback equals a synchronously `reset_and_replay`'d core for the same input; drained marks/actions match |
| thread-safety guarantee | `TerminalCore` carries no GUI / thread-local state and can be moved across threads | — | A compile-time assertion documents that the core is movable to another thread |

**Processing Flow**:
1. Create a fresh core sized to the current grid.
2. Full-drain replay the payload (the existing resume loop, so buffer-switch sequences inside the snapshot are not dropped).
3. Drain mode-actions and pending marks accumulated during replay.
4. Return (core, actions, drained marks) to the caller.

**Implementation Steps**:
1. **Pure builder** — factor the "fresh core + full-drain replay + drain" sequence into a function that owns and returns the core (no `&mut self`), reusing the existing replay/drain helpers.
2. **Equivalence guarantee** — ensure the builder's result is observably identical to the in-place `reset_and_replay` + `drain_marks` outcome.
3. **Thread-safety check** — document and assert the core is movable to another thread.

**Dependencies**: Blocks Phase 2/3.

**Testing Approach**:
- Unit: builder output (grid, scrollback, marks) equals legacy `reset_and_replay` for a representative payload and for an empty payload.
- Unit: compile-time movable-to-thread assertion.

**Acceptance Criteria**:
- [ ] Pure builder exists and is grid/marks-identical to the synchronous path.
- [ ] Core is statically confirmed movable across threads.

**Estimated Effort**: small

---

### Phase 2: Pending-switch state, size-threshold dispatch, live-output queue (tabs.rs)

**Goal**: On a snapshot apply, dispatch the parse off-thread for large payloads while keeping the displayed core intact; queue target-pane live output during the pending switch; supersede the pending target on a newer switch. Small payloads keep the synchronous path.

**Files to Modify**:
- `src-tauri/src/tabs.rs` — pending-switch state on `Tab`; threshold constant; branch in the `Snapshot` / `SnapshotRestore` arm of `apply_mux_message`; live-output queueing in the `PtyOutput` arm; supersession.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `OFFTHREAD_REPLAY_THRESHOLD_BYTES` | Named constant, default 64 KiB (≈ ~7 ms), deciding sync vs off-thread | — | payload `< threshold` → sync; `≥ threshold` → off-thread |
| pending-switch state | Hold target pane id, completion handoff, live-output queue; lifecycle enter/clear | A switch dispatched an off-thread parse | Cleared on swap or supersede |
| snapshot dispatch | For ≥ threshold, copy payload once, do the prompt/fold reset portion, start the worker, enter pending-switch; do NOT reset the displayed core | snapshot received for the active/target pane | Displayed core unchanged until swap |
| live-output queue | During pending-switch, append target-pane `PtyOutput` to the queue instead of the displayed core | pending-switch active for that pane | Bytes retained in arrival order for post-swap apply |
| supersession | A newer switch replaces the pending target, discards the in-flight result, retargets the queue | a second switch arrives mid-parse | Only the latest target will swap in; intermediate cores discarded |

**Processing Flow**:
1. Snapshot arm: branch on payload size.
   - `< threshold` → existing synchronous `reset_frame_for_replay` (swap now). No pending-switch.
   - `≥ threshold` → reset prompts/folds (frame-discard portion), copy payload, dispatch worker, enter/refresh pending-switch for the target pane.
2. PtyOutput arm: if a pending-switch is active for that pane, append bytes to the live queue and return (displayed core untouched); otherwise current behavior.
3. New switch while pending: set pending target to the latest, drop the prior completion handoff, clear the live queue and re-key it to the latest target.

**Implementation Steps**:
1. **Threshold constant** — introduce the named byte constant.
2. **Pending-switch state** — add the state to `Tab` with explicit enter/clear.
3. **Size-branch dispatch** — split the snapshot arm into sync (< threshold) and off-thread (≥ threshold) paths.
4. **Live-output queueing** — divert target-pane live output to the queue while pending.
5. **Supersession** — replace target + discard in-flight on a newer switch.

**Dependencies**: Requires Phase 1. Blocks Phase 3.

**Testing Approach**:
- Unit: threshold boundary (exactly threshold → off-thread; one byte below → sync).
- Unit: supersession yields only the latest target.

**Acceptance Criteria**:
- [ ] Large-payload switch enters pending-switch without mutating the displayed core.
- [ ] Live output for the target pane is queued, not lost.
- [ ] A newer switch supersedes the in-flight one.

**Estimated Effort**: medium

---

### Phase 3: Worker-completion swap + main-thread reconciliation + fallback (app.rs, tabs.rs)

**Goal**: In `pump_all`, non-blockingly poll the pending switch; on completion swap the core, apply queued live bytes in order, reconcile marks/folds/selection, restore per-pane scroll, and full-redraw. On worker failure, fall back to synchronous reparse.

**Files to Modify**:
- `src-tauri/src/app.rs` — poll the active tab's pending-switch in `pump_all`; integrate the swap with the existing `active_pane_switch_from` / `active_frame_reset` post-loop reconciliation (FR3 scroll restore, selection drop on frame reset).
- `src-tauri/src/tabs.rs` — swap + post-swap apply helper that reproduces the `reset_frame_for_replay` main-thread half (marks backfill, `pending_frame_reset` latch, alt-screen reseed) using the worker's returned core/actions/marks.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| completion poll | Non-blocking check of the pending-switch handoff, run per owning tab each pump (not gated to the active tab) | pending-switch active on that tab | Returns ready core, still-pending, or worker-failed |
| swap + apply | Replace `Tab::core` with the built core, then apply queued live bytes in arrival order | ready core for the latest target | Displayed core = snapshot + queued live, in order |
| main-thread reconcile (per tab) | Backfill marks and set `evicted_baseline` **from the worker-built core's drained values** (eviction counter starts at 0, exactly as the synchronous `reset_frame_for_replay`), latch `pending_frame_reset`, reseed alt-screen | swap completed on that tab | marks/folds consistent with the legacy synchronous path |
| active-tab reconcile (post-loop) | Drive the existing selection drop (`pending_frame_reset` latch) + per-pane scroll restore + full redraw, for the active tab only | the swap happened on the active tab | selection + per-pane scroll consistent with legacy synchronous path |
| sync fallback | On worker failure/panic, synchronously reparse that switch via the legacy path | completion poll reports failure | Target pane displayed correctly; one-off main-thread block accepted |

**Processing Flow**:
1. Per owning tab (inside the existing per-tab pump pass), poll its pending-switch handoff (non-blocking).
   - still pending → keep showing the outgoing pane, continue.
   - ready → swap core; apply queued live bytes in order; run the per-tab reconcile half (marks/`evicted_baseline` from the worker-built core, `pending_frame_reset` latch, alt-screen reseed); clear pending-switch.
   - failed → synchronous reparse fallback for the latest target; clear pending-switch.
2. The existing post-loop reconciliation runs for the **active tab only**: `pending_frame_reset` (latched by the per-tab reconcile) drops the stale selection, and the `mux-scroll-isolation` per-pane scroll restore + `needs_full_redraw` fire, so FR3/selection semantics match the synchronous path. Background tabs only swap + latch; their selection/scroll are applied later when they become active (parity with the existing background-tab bookkeeping).
3. **Resize during pending switch** → the size change supersedes the in-flight parse (FR5): discard the in-flight core and re-dispatch the snapshot at the new grid (a stale core built at the old cols/rows is never swapped in).

**Implementation Steps**:
1. **Per-tab poll** — poll each owning tab's pending-switch handoff (non-blocking) within the existing per-tab pump pass; the active tab additionally feeds the post-loop reconciliation.
2. **Swap + ordered live apply** — replace the core and replay the queued bytes in order.
3. **Reconcile half** — backfill marks and set `evicted_baseline` from the worker-built core's drained values, latch `pending_frame_reset`, reseed alt-screen from the worker-returned actions.
4. **Scroll/selection integration** — ensure the existing active-tab per-pane scroll restore + selection-on-frame-reset path fires for the off-thread swap (full redraw on the active tab).
5. **Resize supersede + fallback** — treat a grid resize during pending as a supersede (re-dispatch at the new grid); synchronous reparse on worker failure.

**Dependencies**: Requires Phase 1, 2.

**Testing Approach**:
- Unit: snapshot off-thread parse + queued live applied after swap == one contiguous parse of snapshot+live.
- Unit: worker-failure path yields the correct core via fallback.
- Integration: post-swap marks/folds/selection + per-pane scroll match the synchronous path.

**Acceptance Criteria**:
- [ ] Completed core swaps in on a later pump; UI not blocked proportional to size.
- [ ] Queued live output applied in order after swap.
- [ ] marks/folds/selection + per-pane scroll preserved (NFR1).
- [ ] Worker failure falls back to synchronous reparse (FR7).

**Estimated Effort**: large

---

### Phase 4: Invariant regression + portability hardening

**Goal**: Lock in the `mux-scroll-isolation` invariants under the off-thread path and keep all build variants green, without adding flaky `pump_all`-driven async tests.

**Files to Modify**:
- `crates/term_core/src/terminal_core.rs`, `src-tauri/src/tabs.rs`, `src-tauri/src/app.rs` — test modules only.

**Implementation Steps**:
1. **Ordering / supersession / fallback unit tests** — drive the pure builder + queue model directly (no `pump_all` async).
2. **Invariant regression tests** — FR2 (no residual rows after off-thread swap to a shorter pane) and marks/folds/selection + scroll parity vs the synchronous path.
3. **CLI-only check** — verify `--no-default-features` stays green; the off-thread path is GUI-only.

**Testing Approach**:
- Unit + Integration as above; CLI-only `cargo check`.

**Acceptance Criteria**:
- [ ] Default `cargo test` (single-thread) green; no new flaky `pump_all` async tests.
- [ ] CLI-only `cargo check` green.

**Estimated Effort**: medium

---

## Complete File Structure

```
crates/term_core/src/terminal_core.rs   # +pure snapshot-replay builder, +thread-safety assertion, +unit tests
src-tauri/src/tabs.rs                    # +pending-switch state, +threshold constant, +size-branch dispatch,
                                         #  +live-output queue, +supersession, +swap/reconcile helper, +tests
src-tauri/src/app.rs                     # +pump_all completion poll + swap integration with existing
                                         #  pane-switch/frame-reset reconciliation, +tests
doc/tasks/mux-offthread-replay/          # SPEC.md, 要件定義書.md, IMPLEMENTATION.md, VERIFICATION.md, sdd.yaml, tasks.yaml
```

## Testing Strategy
- Unit: pure builder equivalence; threshold boundary; live-queue ordering; supersession; fallback. Worker-side logic exercised as pure functions (NFR2).
- Integration: post-swap marks/folds/selection + per-pane scroll parity; FR2 no-residual-rows.
- Manual/Perf: `Ctrl+B n n n` across history-heavy panes stays responsive; outgoing pane visible until swap; no blank flicker; ~2 MiB switch does not block UI proportional to size.
- Build: default `cargo test` (single-thread) + CLI-only `cargo check`.

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | — | One-shot worker via the standard library |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Live-output ordering bug (lost/reordered bytes) | Medium | High | Strict snapshot→queue→post-swap-apply ordering; ordering unit test vs contiguous parse |
| `reset_frame_for_replay` semantics not fully reproduced on the split | Medium | High | Reuse the same marks-backfill / latch / alt-screen recipe; parity integration test |
| `TerminalCore` not movable across threads | Low | High | Phase 1 static assertion up front; if it fails, revisit design before Phase 2/3 |
| New async in `pump_all` worsens existing flakiness | Medium | Medium | Keep worker logic pure + unit-tested; poll is non-blocking try-receive, no async test added |
| Grid resize between dispatch and swap | Low | Medium | Resize supersedes the in-flight parse → re-dispatch at the new grid; a core built at stale cols/rows is never swapped in |
| `evicted_baseline` / marks taken from the wrong core on the split | Medium | High | Reconcile uses the worker-built core's drained values (eviction starts at 0), identical to synchronous `reset_frame_for_replay` |
| Live-output queue growth while pending | Low | Low | Bounded by parse duration (≤ ~230 ms at 2 MiB); no explicit cap, but the queue is per-pending-switch and cleared on swap/supersede |
| Background-tab pending switch never polled | Low | Medium | Poll runs per owning tab each pump (not active-only); background swaps apply, with selection/scroll deferred to activation |

## Open Questions
- [x] NFR4 (no memory regression / 1 core per tab) is verified by design review rather than an automated test — accepted at verify-plan (no per-pane resident cores / no LRU; transient in-flight worker core only).
- [x] Threshold default — resolved at verify-plan to **64 KiB (≈ ~7 ms, well under one 60 fps frame)** so the sub-threshold synchronous block stays imperceptible; may still be re-tuned at implement if measurement on the target machine differs.

## Success Metrics
- [ ] FR1–FR7 implemented and tested.
- [ ] `mux-scroll-isolation` FR2/FR3 + marks/folds/selection invariants preserved.
- [ ] Default `cargo test` + CLI-only `cargo check` green; no new flaky async tests.
