# Implementation Plan: microtask-driven-pty-flow

## Overview

Replace the `requestAnimationFrame`-based scheduler that drives WASM PTY-data parsing in `src/terminal-app/pty-handler.ts` with a microtask-based scheduler (primary: `MessageChannel`, fallbacks: `queueMicrotask`, `setTimeout(0)`). The change is local to the data-consumption path; Canvas rendering, ack coalescing, the WASM time budget, and all visibility safety nets remain untouched.

## Objectives

- Decouple the WASM parse / `pty_ack` flow from `requestAnimationFrame`, so it keeps running while the WebView is hidden, occluded, or on a background workspace.
- Keep typing latency and sustained throughput within ±10% of the rAF-baseline.
- Preserve the existing `scheduleToken`, `pendingAckBytes` / `ACK_FLUSH_*`, and frame-budget mechanics verbatim.
- Make zero changes to backend, mux daemon, mux IPC protocol, and WASM core.

## Prerequisites

### Development Environment

- bun (used as TypeScript test / typecheck runner)
- Docker + docker-compose (used for `bun test` / E2E runs per project policy)
- Existing `./scripts/run-e2e-docker.sh` toolchain

### Dependencies

External:
- `MessageChannel` Web API (standard, present in WebKitGTK and WebView2)
- `queueMicrotask` Web API (ES2018, defensive fallback)

Internal (already present, unchanged):
- `src/terminal-app/pty-handler.ts` — primary site of the change
- `src/pty/client.ts` — `ackBytes` (untouched)
- `src/terminal/canvas-renderer.ts` — rAF render loop (untouched)
- `src-tauri/src/pty/backpressure.rs` — `SessionBackpressure::ack` / `wait_for_drain` (untouched)
- Existing safety nets: `visibility-raf-heartbeat`, `visibility-aware-pty-streaming`, `visibility-render-recovery` (untouched)

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (frontend), Rust (untouched)
- **Framework**: Tauri (untouched)
- **Key Web APIs**:
  - `MessageChannel` — primary microtask delivery primitive
  - `queueMicrotask` — first fallback
  - `setTimeout(fn, 0)` — last-resort fallback
  - `requestAnimationFrame` — kept only for Canvas rendering and the rAF heartbeat probe (i.e., not on the data path)

### Design Approach

Single-file modification with strict separation of two concerns:

1. **Data path (microtask-driven)**: `pendingChunks → processPendingData → WASM process_pty_data → pendingAckBytes → pty_ack`. Driven exclusively by the microtask scheduler.
2. **Render path (rAF-driven)**: `currentRenderer.renderImmediate / forceRender / paintIfDirty`. Continues to be invoked from inside `processPendingData` (synchronous within the microtask) — visual output still aligns with vsync because the renderer itself is rAF-paced internally for cursor blink and other periodic redraws, and the microtask-scheduled `renderImmediate` simply marks the canvas dirty in time for the next frame.

The scheduler is selected once per handler instance during `setupPtyHandlers`. The selected primitive determines the trigger label passed to `processPendingData` (`"microtask"` vs `"timer"`).

### Component Interaction

Unchanged at the module-boundary level:

- `PtyClient.onData` -> `pendingChunks.push` -> `scheduleProcessing()`
- `injectData` -> `pendingChunks.push` -> `scheduleProcessing()`
- `notifyTabActivated` -> `scheduleProcessing()` if pending work exists
- `processPendingData` -> `ptyClient.ackBytes`

The only internal change is what `scheduleProcessing` does between its early-return guard and its callback: it now invokes a stored `schedule()` closure instead of `requestAnimationFrame`.

## Implementation Phases

### Phase 1: Microtask scheduler primitive

**Goal**: Replace the `requestAnimationFrame` call site in `scheduleProcessing` with a `MessageChannel`-backed scheduler created once at handler setup, with `queueMicrotask` and `setTimeout(0)` defensive fallbacks.

**Files to Create**: none.

**Files to Modify**:
- `src/terminal-app/pty-handler.ts`
  - Introduce a private scheduler factory that selects the best available primitive.
  - Allocate one scheduler instance per `setupPtyHandlers` call.
  - Replace `rafScheduled` / `rafHandle` with `processScheduled` / `pendingHandle`.
  - Update `ProcessTrigger` union: remove `"raf"`, add `"microtask"` and `"timer"`.
  - Replace the `requestAnimationFrame(...)` call in `scheduleProcessing` with `schedule()`.
  - Remove the now-redundant `cancelAnimationFrame` call site at the top of `processPendingData`.
  - Update the leftover-data re-schedule guard to reference `processScheduled`.
  - Update diagnostic log lines that print `rafScheduled=...` to use the renamed flag (preserves the `[DIAG-*]` / health-check log shape per NFR4 / FR6).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Scheduler factory | Choose `MessageChannel` / `queueMicrotask` / `setTimeout` once per handler; return a closure that delivers the chosen microtask primitive | Called exactly once during `setupPtyHandlers`. The factory receives two callbacks (one for the `MessageChannel` / `queueMicrotask` paths labelled `"microtask"`, one for the `setTimeout` path labelled `"timer"`) | Returns a `schedule()` closure that, when called, causes exactly one delivery of the chosen callback on the next event-loop turn. Also returns disposal callbacks (port close + active timer-handle access) used by `destroy()` |
| `processScheduled` flag | Deduplicate concurrent `scheduleProcessing` calls | True only while a microtask delivery is queued but not yet running | Reset to `false` at the start of each `processPendingData` invocation, before the user-visible body runs, so that re-entrant `scheduleProcessing` (e.g. from leftover-data continuation) can enqueue the next tick |
| `pendingHandle` field | Retain a cancellable `setTimeout` id when the timer fallback is active | `null` for `MessageChannel` / `queueMicrotask` paths; a positive integer when a timer is pending | Cleared back to `null` once the timer fires or `destroy()` cancels it |
| `scheduleToken` | Invalidate stale microtasks after `flushPendingData()` or `processPendingData` re-entry | Compared against the `myToken` captured by the scheduler closure on each delivery | Microtask whose `myToken !== scheduleToken` exits without invoking `processPendingData` |

**Processing Flow** (diagram-convertible):

1. Caller invokes `scheduleProcessing()`.
   - If `processScheduled` is true -> return immediately.
   - Else -> set `processScheduled = true`, capture `myToken = ++scheduleToken`, record `lastScheduleTime`, then call `schedule(myToken)`.
2. Microtask is delivered via the chosen primitive.
   - If `myToken !== scheduleToken` -> exit (stale schedule, do nothing).
   - Else -> reset `processScheduled` to `false`, clear `pendingHandle` if present, then invoke `processPendingData(<label>)` where `<label>` is `"microtask"` or `"timer"` depending on the active primitive.
3. `processPendingData` proceeds exactly as today (frame budget, hard timeout, ack coalescing).
4. End of `processPendingData`:
   - If `leftoverData` is non-null -> call `scheduleProcessing()` (deduplication is handled by `processScheduled`).

**Implementation Steps** (high level, 5 max):

1. **Define `ProcessTrigger` union** — replace the existing `"raf" | "manual"` alias with `"microtask" | "timer" | "manual"` per SPEC §FR5. `"manual"` continues to label all direct-invoke paths (`processNow`, the health-check force-drain).
2. **Add the scheduler factory** — encapsulate the primitive selection and trigger-label mapping inside `setupPtyHandlers`. The factory accepts `{ onMicrotask, onTimer }` callbacks (one per trigger label) and returns `{ schedule, dispose }`. The two callback shapes share the same body modulo the trigger string. The factory's `dispose()` hook closes both `MessagePort` instances on the `MessageChannel` path and is a no-op on the `queueMicrotask` / `setTimeout` paths.
3. **Rewrite `scheduleProcessing` body** — remove the `requestAnimationFrame` call and the `rafHandle` assignment; call `scheduler.schedule()` instead, with `pendingToken` updated immediately before. Rename `rafScheduled -> processScheduled`, `rafHandle -> pendingHandle`. The type of `pendingHandle` is `ReturnType<typeof setTimeout> | null` (matching `ackFlushTimer`) so the `setTimeout` fallback id can be cleared without type assertions. Delete the `cancelAnimationFrame` block at the top of `processPendingData` since the rAF handle no longer exists; the token check inside the scheduler callback now performs the equivalent invalidation. The synchronous `scheduleToken++` at the top of `processPendingData` is preserved (it invalidates any in-flight microtasks for the upcoming run).
4. **Update leftover-data re-schedule guard** — change `if (leftoverData && !rafScheduled)` to `if (leftoverData)` (the deduplication is already enforced by `scheduleProcessing` itself).
5. **Update health-check / event-loop probe log lines** — replace `rafScheduled=...` with `processScheduled=...` so the diagnostic format stays consistent and the renamed identifier is referenced everywhere.

**Dependencies**: none (this is the first and only structural change). Blocks: Phase 2.

**Testing Approach**:
- Unit (`src/terminal-app/pty-handler.test.ts`): TS-MT-1, TS-MT-2, TS-MT-3, TS-MT-5, TS-MT-6, TS-MT-7, TS-MT-10, TS-MT-11.
- E2E: smoke run of existing specs to verify no regression after the rename / rewrite.

**Acceptance Criteria**:
- [ ] `scheduleProcessing` no longer calls `requestAnimationFrame` or `cancelAnimationFrame`.
- [ ] A single `MessageChannel` instance is allocated per handler.
- [ ] `processPendingData("microtask")` is the dominant trigger label observed in logs during normal streaming.
- [ ] `bun run typecheck` passes.

**Estimated Effort**: small.

---

### Phase 2: Cleanup, token semantics, and edge cases

**Goal**: Ensure `destroy()` cleanly tears down the scheduler primitive (close `MessageChannel` ports, cancel any pending `setTimeout`), verify that `scheduleToken` invalidates stale microtasks correctly across `flushPendingData`, and confirm that re-entrant `scheduleProcessing` from `leftoverData` works without unbounded recursion or starvation.

**Files to Create**: none.

**Files to Modify**:
- `src/terminal-app/pty-handler.ts` — extend the `destroy` closure with scheduler teardown.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `destroy()` cleanup block | Close `MessageChannel` ports, cancel pending `setTimeout`, clear `processScheduled`, retain existing pendingAckBytes flush + focus listener unregister | `destroy()` is called at most once per handler | After return: no scheduled microtask can re-enter `processPendingData`; both message ports are closed; any active `pendingHandle` is cleared |
| `scheduleToken` invariant | Maintain monotonic increment-on-direct-process semantics | `processPendingData` increments `scheduleToken` at the very top (already implemented today) | Any microtask whose captured `myToken` is older than the current `scheduleToken` is a no-op. Holds across `flushPendingData()` because that path also drives `processPendingData` indirectly (via leftover clear) and the existing increment fires |

**Processing Flow** (cleanup):

1. `destroy()` invoked.
2. Existing teardown (focus listener, ackFlushTimer, pendingAckBytes flush) runs as today.
3. Scheduler teardown:
   - Invoke the factory-returned `dispose()` hook (try/catch — closing already-closed ports is a benign no-op; the `queueMicrotask` / `setTimeout` factory branches return a no-op `dispose()`).
   - If `pendingHandle` is non-null -> `clearTimeout(pendingHandle)` and reset to `null`.
   - Set `processScheduled = false` (defensive).
4. Subsequent `injectData` / `notifyTabActivated` calls would be erroneous (handler is destroyed); they are not protected today and remain so.

**Implementation Steps** (3 max):

1. **Extend `destroy`** — call the factory-returned `dispose()` hook (which closes the `MessageChannel` ports when active) and `clearTimeout(pendingHandle)` next to the existing `cancelAnimationFrame(rafHandle)` block (which is removed in Phase 1).
2. **Verify `flushPendingData` semantics** — confirm that `flushPendingData` does not need a `scheduleToken` bump itself: it only clears `pendingChunks` / `leftoverData` and does not call `processPendingData`, so any in-flight microtask that subsequently runs will see empty queues and be a benign no-op even without a token bump (current behavior is unchanged).
3. **Re-entrant safety** — confirm that the `processScheduled = false` reset happens before `processPendingData` is invoked inside the scheduler closure, so a leftover-driven `scheduleProcessing()` call inside `processPendingData` can enqueue the next tick.

**Dependencies**: requires Phase 1.

**Testing Approach**:
- Unit: TS-MT-4 (token invalidation), TS-MT-9 (destroy closes ports + clears timer).
- Edge cases: re-entrant injectData during a microtask, flushPendingData while a microtask is in flight.

**Acceptance Criteria**:
- [ ] `destroy()` closes both `MessagePort` objects and clears any pending `setTimeout`.
- [ ] After `flushPendingData()`, an already-queued microtask is a no-op (verified via the existing `scheduleToken` mechanism).
- [ ] No memory leak across handler create/destroy cycles (manually verified via repeat E2E).

**Estimated Effort**: small.

---

### Phase 3: E2E verification and regression coverage

**Goal**: Provide an automated E2E that proves the data path keeps draining when `requestAnimationFrame` is stalled, and confirm that all existing E2E specs still pass.

**Files to Create**:
- `e2e-tests/specs/microtask-data-flow.e2e.js` — new spec implementing E2E-MT-2 (the central NFR1 verification).

**Files to Modify**: none (existing specs are not modified per FR10 / NFR5).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `microtask-data-flow.e2e.js` | Stall `globalThis.requestAnimationFrame` while the document remains visible, drive a busy PTY producer, assert that `pty_ack` continues to fire (verified by sampling `pty_get_send_stats(sessionId)` and confirming `sent_count` / `sent_bytes` keep increasing across samples — which is only possible if the reader is not blocked in `wait_for_drain`) and that no `backpressure stalled` warning is emitted | Test environment is the standard Docker E2E harness | Test exits 0 when `sent_bytes` keeps growing while rAF is stalled; fails when it plateaus (would indicate reader blocked) |

**Processing Flow** (test):

1. Wait for terminal to be ready and obtain `sessionId`.
2. Install rAF stall stub (record-but-don't-invoke), modeled on the existing `installRafStall` helper in `visibility-raf-heartbeat.e2e.js`.
3. Drive a sustained PTY producer (e.g. a shell loop emitting many lines).
4. Sample `pty_get_send_stats(sessionId)` at ~500ms intervals across a 5–8s window.
   - Pre-check: at least one sample shows `sent_bytes > 0` so we know data is reaching the backend reader.
   - Assertion: across the observation window, `sent_bytes` keeps increasing (≥ 2× the value of the first sample by the end). `pty_get_send_stats` returns `(sent_count, sent_bytes)` and does not expose `in_flight` directly, but if acks stopped flowing the reader would block in `wait_for_drain` once `in_flight` hit `HIGH_WATER_BYTES` (8 MiB) and `sent_bytes` would plateau. Continued monotonic growth therefore proves the data path is consuming and acking under rAF stall.
5. Restore real rAF.
6. Verify no `backpressure stalled` warning was logged during the rAF-stall window.
7. Ctrl+C the producer to leave the terminal clean.

**Implementation Steps** (4 max):

1. **Author the new spec** — derive structure from `visibility-raf-heartbeat.e2e.js` (which already handles rAF stubbing); reuse `getSessionId`, `getSendStats`, and `installRafStall` patterns.
2. **Tune sample windows** — choose an `in_flight` poll interval (≈ 500ms) and total observation window (≈ 5–8s) consistent with other E2E specs.
3. **Run the full E2E suite** — `./scripts/run-e2e-docker.sh test` to confirm no regression in the existing 30+ specs (NFR5 / FR10).
4. **Manual freeze reproduction** — run the original 30-minute-hidden manual scenario (US1 / US2) and confirm no freeze on resume.

**Dependencies**: requires Phase 1 + Phase 2.

**Testing Approach**:
- E2E: E2E-MT-1 (existing visibility-raf-heartbeat passes), E2E-MT-2 (new spec), E2E-MT-3 (throughput within ±10% — visibility-throughput-bench), E2E-MT-4 (all other specs pass).
- Manual: Perf-1 / Perf-2, the US1 / US2 long-duration freeze reproduction.

**Acceptance Criteria**:
- [ ] `microtask-data-flow.e2e.js` passes.
- [ ] All existing E2E specs in `e2e-tests/specs/` pass without modification.
- [ ] Throughput benchmark within ±10% of pre-change baseline.
- [ ] Manual long-hidden reproduction confirms no freeze.

**Estimated Effort**: medium (E2E specs are heavier to author and run than unit tests).

---

## Complete File Structure

```
emterm/
├── src/
│   └── terminal-app/
│       ├── pty-handler.ts          # MODIFIED — scheduler swap, rename, trigger update
│       └── pty-handler.test.ts     # MODIFIED — add TS-MT-1〜TS-MT-11
├── e2e-tests/
│   └── specs/
│       └── microtask-data-flow.e2e.js   # NEW — NFR1 verification
└── doc/
    └── tasks/
        └── microtask-driven-pty-flow/
            ├── 要件定義書.md         # existing (sdd.1 output)
            ├── SPEC.md               # existing (sdd.1 output)
            ├── IMPLEMENTATION.md     # this file (sdd.2 output)
            ├── VERIFICATION.md       # sdd.2 output
            ├── tasks.yaml            # sdd.2 output
            └── sdd.yaml              # updated with tasks/tests refs
```

No backend file is touched. No file is added under `src/` or `wasm/` outside the test file modifications above.

## Testing Strategy

- **Unit (`bun test`)**:
  - `pty-handler.test.ts` gains TS-MT-1〜TS-MT-11 covering scheduler primitive selection, deduplication, token invalidation, destroy cleanup, and the `requestAnimationFrame` call-site removal (TS-MT-7 / TS-MT-11).
  - Existing pty-handler tests (recovery, focus listener) must continue to pass — they cover orthogonal concerns and are not impacted by the scheduler swap.
  - Coverage target: maintain existing coverage for `pty-handler.ts` (no regression). Critical paths (scheduler selection, destroy cleanup) at 90%+.
- **E2E (`./scripts/run-e2e-docker.sh test`)**:
  - New: `microtask-data-flow.e2e.js`.
  - Existing: `visibility-raf-heartbeat.e2e.js`, `freeze-regression.e2e.js`, `visibility-throughput-bench.e2e.js`, `visibility-aware-streaming.e2e.js`, `visibility-resume-block.e2e.js`, and all other specs.
- **Manual**:
  - 30-minute workspace-switch / minimize reproduction (US1 / US2).
  - Throughput / latency micro-benchmarks (Perf-1 / Perf-2).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none — feature is implemented entirely with platform Web APIs) | — | — |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Microtasks deliver too eagerly and starve other event-loop tasks (input events, timers) | low | medium | Keep the existing `FRAME_BUDGET_MS` (12ms) and `HARD_TIMEOUT_MS` (200ms). When the budget is exceeded mid-frame, `leftoverData` parking + re-schedule yields the event loop. |
| `MessageChannel` constructor or `port1.postMessage` throws in some unusual runtime | very low | low | Factory wraps in try/catch and falls through to `queueMicrotask` then `setTimeout`. Logged once at `console.warn`. |
| `setTimeout(0)` fallback throttled while hidden | low | low | This path only activates when both `MessageChannel` and `queueMicrotask` are unavailable, which is not expected on supported builds. Even when active, visibility-aware streaming pauses the backend while hidden so no frontside backlog accumulates. |
| Rename causes diff churn that masks logic regressions during code review | medium | low | Rename is a deliberate single commit (or co-located with the body changes); reviewers can cross-check the diff against the old `rafScheduled` / `rafHandle` references via `git grep`. |
| The new microtask path interacts badly with `scheduleToken` after `flushPendingData()` | low | medium | Existing `scheduleToken` invariant (incremented at top of `processPendingData`) is preserved; TS-MT-4 covers it. |
| E2E rAF-stall stub interferes with the renderer | low | low | The stub records calls and never invokes them, identical to the existing `visibility-raf-heartbeat.e2e.js` pattern; unit-level renderer tests are unaffected. |

## Open Questions

- [ ] None at the time of plan creation. SPEC §14 confirms all decisions are settled.

## Success Metrics

- [ ] Functional completeness: FR1〜FR11 each have at least one corresponding unit or E2E test (mapped in VERIFICATION.md).
- [ ] Quality: `bun test`, `bun run typecheck`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `./scripts/run-e2e-docker.sh test` all pass.
- [ ] Performance: throughput regression test within ±10% of baseline (NFR2); typing latency within ±10% (NFR3).
- [ ] User-visible: manual workspace-switch / minimize-for-30min scenarios complete without freeze (US1 / US2).
- [ ] Observability: `[DIAG-*]` log shape unchanged except for trigger label; no new log levels introduced (NFR6 / NFR4).
