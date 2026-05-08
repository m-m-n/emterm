# Feature: microtask-driven-pty-flow

## Overview

Replace the `requestAnimationFrame`-based scheduler that drives WASM PTY data parsing in `pty-handler.ts` with a sub-rAF scheduler built on `MessageChannel.postMessage` (primary) with a `setTimeout(0)` fallback. Both primitives are task schedulers, so they keep running while the page is hidden / occluded AND yield between drains so rendering and input can interleave. `queueMicrotask` is deliberately NOT in the fallback chain because microtask chaining cannot yield to the task queue and would starve rendering under sustained bursts. WebKitGTK throttles or stops `requestAnimationFrame` callbacks when the page is hidden, occluded, or on a non-focused workspace; this stalls WASM parsing and therefore stalls the existing `pty_ack` flow control, which in turn lets `SessionBackpressure.in_flight` grow until the reader thread blocks in `wait_for_drain`.

xterm.js solves the same problem by using `MessageChannel.postMessage` to yield to the next event loop turn — microtasks keep running across visibility states. This feature adopts the same approach for eMterm's WASM parsing path. Canvas rendering itself remains driven by `requestAnimationFrame`, so visual updates are still vsync-aligned.

The existing safety nets (`visibility-raf-heartbeat`, `visibility-aware-pty-streaming`, `visibility-render-recovery`) are kept as-is. They serve different purposes (backend bandwidth suppression, WASM crash recovery) and continue to coexist with the new scheduler.

## Objectives

- Eliminate the structural dependency on `requestAnimationFrame` for PTY data consumption.
- Keep PTY data flowing (parse + ack) when the page is hidden, occluded, or on a background workspace.
- Preserve normal-case throughput and typing latency within ±10% of current.
- Keep all existing safety nets functional and untouched.
- Avoid backend, mux daemon, and mux IPC protocol changes.

## User Stories

### US1: Long-running AI session across workspace switches

As a user running Claude Code in eMterm, I want the terminal to keep consuming PTY output even when I switch workspaces for hours, so that I do not return to a frozen UI.

**Acceptance Criteria:**
- [ ] After switching to a different workspace for >= 30 minutes with active PTY output, the foreground returns without freeze.
- [ ] `SessionBackpressure.in_flight` does not exceed `HIGH_WATER_BYTES` (8 MiB) at any point during the hidden period (with visibility-aware streaming engaged or disengaged).
- [ ] `console.warn` does not log `backpressure stalled` for that session.

### US2: Build runs while window is minimized

As a developer, I want background builds running in eMterm to keep emitting output through to completion even when the window is minimized for a long time.

**Acceptance Criteria:**
- [ ] No frontend backlog accumulates after restoring the window.
- [ ] No PTY data is dropped.

### US3: No regression in typing latency

As a user typing into eMterm continuously, I want the new scheduler to not introduce perceptible input lag.

**Acceptance Criteria:**
- [ ] Round-trip latency from key press to first paint is within ±10% of the rAF-based baseline (measured manually or via existing benchmark).
- [ ] Sustained throughput (`yes | head -c 100M`) is within ±10% of baseline.

## Technical Requirements

### Functional Requirements

- **FR1 (Microtask scheduler primary):** `pty-handler.ts` introduces a single, long-lived `MessageChannel` instance. `port2.onmessage` invokes `processPendingData("microtask")`. `scheduleProcessing()` calls `port1.postMessage(0)` exactly once per pending burst (deduplicated via the `processScheduled` flag). The channel is created during `setupPtyHandler` and disposed in the cleanup path (no per-handle allocation).
- **FR2 (Fallback):** When `MessageChannel` is unavailable in the current runtime (defensive — present in WebKitGTK / WebView2 in supported builds), fall back directly to `setTimeout(() => runScheduledCallback("timer", token), 0)`. `queueMicrotask` is intentionally NOT a fallback step: microtask chaining (leftover → scheduleProcessing → queueMicrotask) cannot yield to the task queue, so a sustained burst would starve rendering and input. Both supported primitives (MessageChannel `postMessage`, `setTimeout(0)`) are task schedulers, so they yield between drains. Selection is performed once at handler setup and stored in a const-bound `schedule` function reference.
- **FR3 (Remove rAF from data path):** `requestAnimationFrame` and `cancelAnimationFrame` are removed from the data-consumption path. Specifically: the body of `scheduleProcessing` no longer calls `requestAnimationFrame`. The fields `rafScheduled` and `rafHandle` are renamed to `processScheduled` and `pendingHandle` respectively. `pendingHandle` is a `ReturnType<typeof setTimeout> | null` value populated only on the `setTimeout(0)` fallback path; it is `null` on the `MessageChannel` path. The runtime type matches the existing `ackFlushTimer` declaration in the same file so cross-runtime (browser / bun) typing stays consistent.
- **FR4 (Token semantics preserved):** The `scheduleToken` mechanism that invalidates stale callbacks is preserved. Each `scheduleProcessing` call captures the current `scheduleToken` value AT QUEUE TIME and passes it to the scheduler primitive (as the `postMessage` payload, or as a closure-captured argument on the timer path). When the scheduled callback fires, it compares the captured token against `scheduleToken` and bails if they differ — including after a synchronous `processPendingData` (which bumps `scheduleToken`) or a `destroy()` (which also bumps it).
- **FR5 (Trigger label set):** The `trigger` argument of `processPendingData` accepts the values `"microtask"`, `"timer"`, and `"manual"`. The value `"raf"` is removed from the type alias and call sites. `"manual"` continues to label all direct-invoke paths (`processNow`, the health-check force-drain, etc.) as today; no new label is introduced for those call sites.
- **FR6 (Re-schedule on leftover):** When `processPendingData` ends with non-empty `leftoverData`, it calls `scheduleProcessing()` to continue draining on the next microtask tick. This replaces the existing `if (leftoverData && !rafScheduled) scheduleProcessing();` block. The conditional check is updated to use `processScheduled`.
- **FR7 (Ack coalescing unchanged in shape):** The existing ack coalescing (`pendingAckBytes` / `ACK_FLUSH_BYTES` / `ACK_FLUSH_INTERVAL_MS` / `ackFlushTimer`) is kept verbatim. No new types are introduced. The `setTimeout(..., ACK_FLUSH_INTERVAL_MS)` flush timer continues to function — Even if WebKit throttles `setTimeout` to ~1 second under hidden state, the size-based flush (`ACK_FLUSH_BYTES`) ensures backpressure does not accumulate.
- **FR8 (Time budget preserved):** The existing per-call WASM time budget (whatever `WASM_TIME_BUDGET_MS` or equivalent is currently used to slice large `process_pty_data` calls) is preserved. When the budget is exceeded mid-frame, leftover data is parked into `leftoverData` and a re-schedule occurs (FR6). The behavior is identical to current; only the scheduling primitive changes.
- **FR9 (Canvas rendering still rAF-driven):** Canvas rendering invocations (`forceRender`, `paintIfDirty`, cursor blink etc.) remain on the existing `requestAnimationFrame` path. The microtask handler updates WASM grid state and dirty flags; the renderer reads them on the next vsync. No structural change to the renderer is required.
- **FR10 (Coexistence with visibility safety nets):** `visibility-raf-heartbeat`, `visibility-aware-pty-streaming`, and `visibility-render-recovery` continue to operate without modification:
  - `visibility-raf-heartbeat` still detects rAF stalls and dispatches `setVisibility(false)`. Its semantic role narrows from "the data path is dead" to "the render path is dead", which still correctly triggers backend bandwidth suppression.
  - `visibility-aware-pty-streaming` still pauses backend forwarding while hidden via `pty_set_visibility(false)`.
  - `visibility-render-recovery` still performs WASM health probes on focus restoration.
  - The existing in-`pty-handler.ts` health-check (`setTimeout(healthCheck, HEALTH_CHECK_INTERVAL_MS)` loop) and its synchronous force-drain branch (triggered when `processScheduled` has been stuck for > 3 s) are kept verbatim for defense-in-depth. Under the microtask scheduler this branch is not expected to fire — microtasks keep running while hidden — but it remains a useful belt-and-braces guard against future regressions where a synchronous body holds the main thread long enough for `processScheduled` to appear stuck. The flag name in the warn line changes from `rafScheduled` to `processScheduled` (NFR6).
- **FR11 (Cleanup):** When the PTY handler is destroyed (`handle.destroy()` or equivalent), a `disposed` flag is set to `true` and `scheduleToken` is bumped immediately so any in-flight scheduled callback that fires after teardown observes the disposed state and bails before calling `processPendingData`. The factory-returned `dispose()` hook is then invoked: on the `MessageChannel` path it detaches `port2.onmessage` first and then closes both `MessagePort` instances; on the `setTimeout` path it is a no-op (the timer handle is cleared explicitly afterwards). The `processScheduled` flag is reset, any pending `setTimeout` fallback handle in `pendingHandle` is cleared via `clearTimeout`, and `pendingAckBytes` is flushed if non-zero (existing behavior). The `MessagePort` instances are encapsulated inside the factory and never leak out — `dispose()` is the only way to close them.

### Non-Functional Requirements

- **NFR1 (Hidden-state operation):** The data path keeps running while the page is hidden, occluded, or on a background workspace. Verified by E2E with virtual display occlusion or by injecting a synthetic rAF stall while the document remains visible (test-only monkey-patch).
- **NFR2 (Throughput):** Sustained PTY throughput (≥ 100 MB streaming) is within ±10% of the baseline measured before the change.
- **NFR3 (Latency):** Typing latency (key press → first paint) is within ±10% of baseline.
- **NFR4 (Compatibility):** Runs on Linux (WebKitGTK) and Windows (WebView2) without behavioral difference. `MessageChannel` is available on both.
- **NFR5 (Compatibility with safety nets):** All existing tests for `visibility-raf-heartbeat`, `visibility-aware-pty-streaming`, and `visibility-render-recovery` continue to pass without modification.
- **NFR6 (Observability):** The `[DIAG-*]` log lines emitted from `processPendingData` continue to be emitted with the same prefix; only the `trigger` field value changes. No new log levels or formats are introduced.

## Implementation Approach

### Architecture

```
Before (current):

  PTY chunk (Tauri Channel)
        │
        ▼
  injectData / pendingChunks.push
        │
        ▼
  scheduleProcessing()
        │
        ▼  requestAnimationFrame  ← STOPPED by WebKit on hidden/occluded
        │
        ▼
  processPendingData("raf")
        │
        ├─▶ WASM process_pty_data (consumes chunks)
        ├─▶ pendingAckBytes += consumed
        └─▶ pty_ack (size or timer driven)


After (this feature):

  PTY chunk (Tauri Channel)
        │
        ▼
  injectData / pendingChunks.push
        │
        ▼
  scheduleProcessing()
        │
        ▼  MessageChannel.port1.postMessage(token)   ← KEEPS RUNNING when hidden
        │   (setTimeout 0 fallback)
        │
        ▼
  processPendingData("microtask")
        │
        ├─▶ WASM process_pty_data (consumes chunks)
        ├─▶ pendingAckBytes += consumed
        ├─▶ pty_ack (size or timer driven)
        └─▶ if leftoverData: scheduleProcessing()  (yields to next microtask)

  Renderer (separate)
        │
        ▼  requestAnimationFrame  ← still vsync-aligned
        │
        ▼
  CanvasRenderer.paintIfDirty (reads WASM grid dirty flags)
```

### Component Diagram

```
┌─────────────────────────────────────────────────────────┐
│ pty-handler.ts (after this change)                      │
│                                                         │
│  scheduler = (() => {                                   │
│    const ch = new MessageChannel();                     │
│    ch.port2.onmessage = () => process("microtask");     │
│    return () => ch.port1.postMessage(0);                │
│  })()    // primary                                     │
│                                                         │
│  // setTimeout(0) fallback if MessageChannel            │
│  // is unavailable (defensive)                          │
│                                                         │
│  scheduleProcessing()                                   │
│    └ if (processScheduled) return                       │
│      processScheduled = true                            │
│      scheduler()                                        │
│                                                         │
│  processPendingData(trigger)                            │
│    └ processScheduled = false                           │
│      ... (consume chunks, ack, etc.)                    │
│      if (leftoverData) scheduleProcessing()             │
└─────────────────────────────────────────────────────────┘
                      │
                      ▼ unchanged
┌─────────────────────────────────────────────────────────┐
│ src/pty/client.ts                                       │
│   ackBytes(n) → invoke("pty_ack", { ... })              │
└─────────────────────────────────────────────────────────┘
                      │
                      ▼ unchanged
┌─────────────────────────────────────────────────────────┐
│ src-tauri/src/pty/backpressure.rs                       │
│   SessionBackpressure::ack(n)                           │
│   wait_for_drain() wakes when in_flight <= LOW_WATER    │
└─────────────────────────────────────────────────────────┘
```

### Data Flow

**Hidden / occluded state, visibility-aware streaming engaged (normal):**

```
  PTY produces data → backend reader sees session.is_visible() == false
    → data is consumed by shadow parser only, channel.send is skipped
    → in_flight does not grow
    → frontend has nothing to process; microtask scheduler is idle
  On visibility restore → shadow snapshot is sent as one chunk
    → injectData → scheduleProcessing() → microtask drains snapshot
```

**Hidden / occluded state, visibility-aware streaming NOT engaged (degraded — e.g. notification missed, mux delayed):**

```
  PTY produces data → backend reader still calls channel.send
    → in_flight grows
    → frontend receives chunk via Tauri Channel → injectData
    → scheduleProcessing() → MessageChannel postMessage  (keeps running)
    → processPendingData("microtask") consumes chunk
    → pty_ack flushes (size or timer trigger)
    → in_flight stays under control
```

This is the failure case that motivated the feature. With the microtask scheduler, even if a visibility notification is dropped, data still drains.

**Visible state, normal:**

```
  Same as above, just continuously. No difference from rAF behavior except
  that processing yields slightly earlier (per microtask vs per frame).
  Renderer still paints at vsync via existing rAF path.
```

### State Machine

The `processScheduled` flag is a simple two-state guard:

```
        scheduleProcessing()
              │ if !processScheduled
              ▼
     ┌──────────────────┐
     │ processScheduled │
     │     = true       │
     │ scheduler()      │
     └────────┬─────────┘
              │ scheduler delivers microtask
              ▼
   processPendingData(trigger)
              │ entry: processScheduled = false
              ▼
   ... consume work ...
              │
              ├─ if more work: scheduleProcessing() (re-enters above)
              └─ else: idle
```

### File Structure

```
src/terminal-app/
  pty-handler.ts            # Modified: replace rAF with MessageChannel scheduler,
                            #   rename rafScheduled → processScheduled,
                            #   rename rafHandle → pendingHandle,
                            #   remove "raf" from trigger union,
                            #   add "microtask" / "timer" to trigger union.

src/terminal-app/handlers/  # If trigger label type lives here, update accordingly.

doc/tasks/microtask-driven-pty-flow/
  要件定義書.md
  SPEC.md                   # this file
  IMPLEMENTATION.md         # by sdd.2
  VERIFICATION.md           # by sdd.2
  VERIFICATION_RESULT.md    # by sdd.6
  sdd.yaml
```

No new files are created in `src/`. No backend file is touched.

### Public API Changes

#### `pty-handler.ts` internal types

The trigger string union (currently includes `"raf"`) is updated:

```typescript
// Before
type ProcessTrigger = "raf" | "manual";

// After
type ProcessTrigger = "microtask" | "timer" | "manual";
```

`"raf"` is replaced by `"microtask"` (primary) plus `"timer"` (added for the rare `setTimeout` fallback). The `"manual"` label is preserved as-is for all direct-invoke paths.

#### Internal scheduler factory

The factory selects the best available primitive once per handler and returns
both a `schedule(token)` closure (used by `scheduleProcessing`) and a
`dispose()` hook (used by `destroy()`). The token is captured at queue time
and passed to the chosen primitive (as the `postMessage` payload, or as a
closure-captured argument on the timer path) so the scheduled callback can
compare it against `scheduleToken` when it eventually fires.

```typescript
type Scheduler = {
  schedule: (token: number) => void;
  dispose: () => void;
};

const runScheduledCallback = (
  trigger: "microtask" | "timer",
  capturedToken: number,
) => {
  pendingToken = capturedToken;
  processScheduled = false;
  pendingHandle = null;
  if (disposed) return;
  if (capturedToken !== scheduleToken) return;
  processPendingData(trigger);
};

function createMicrotaskScheduler(): Scheduler {
  if (typeof MessageChannel !== "undefined") {
    const ch = new MessageChannel();
    ch.port2.onmessage = (e) => {
      const token = typeof e.data === "number" ? e.data : 0;
      runScheduledCallback("microtask", token);
    };
    return {
      schedule: (token) => {
        try { ch.port1.postMessage(token); } catch { /* ignore */ }
      },
      dispose: () => {
        // Detach onmessage first so any task in flight before close()
        // becomes a no-op when dispatched.
        try { ch.port2.onmessage = null; } catch { /* ignore */ }
        try { ch.port1.close(); } catch { /* ignore */ }
        try { ch.port2.close(); } catch { /* ignore */ }
      },
    };
  }
  return {
    schedule: (token) => {
      pendingHandle = setTimeout(() => runScheduledCallback("timer", token), 0);
    },
    dispose: () => { /* pendingHandle is cleared by destroy() directly */ },
  };
}
```

`queueMicrotask` is intentionally absent from the chain. A microtask fired from inside `processPendingData` ends up calling `scheduleProcessing` again when `leftoverData` remains, which would chain another microtask before the task queue ever yields — starving rendering and input. Both supported primitives (`MessageChannel.postMessage` and `setTimeout(0)`) are task schedulers, so they yield naturally between drains.

Token capture is deterministic: `scheduleProcessing` reads `++scheduleToken` once at queue time and passes that value into the scheduler primitive. The scheduled callback gets the captured value back when it fires (via the `MessageEvent.data` payload on the channel path, or via closure capture on the timer path). A direct synchronous `processPendingData` (which bumps `scheduleToken`) or a `destroy()` (which also bumps it) therefore makes any in-flight callback's captured token stale, and `runScheduledCallback` bails before invoking `processPendingData`.

#### Internal state rename

```diff
- let rafScheduled = false;
- let rafHandle: number | null = null;
+ let processScheduled = false;
+ let pendingHandle: ReturnType<typeof setTimeout> | null = null;   // populated only on setTimeout fallback
```

`pendingHandle` is populated only on the `setTimeout(0)` fallback path (so that `destroy()` can call `clearTimeout`). The `MessageChannel` path leaves it `null` (no cancellable handle). `runScheduledCallback` resets `pendingHandle` to `null` regardless of the path it took — that is a no-op on the channel path and the intended cleanup on the timer path. The type matches the existing `ackFlushTimer` declaration so the same `clearTimeout` call works without type assertions.

### Algorithms

#### `scheduleProcessing` (new body)

```typescript
const scheduleProcessing = () => {
  if (processScheduled) return;
  processScheduled = true;
  lastScheduleTime = performance.now();
  pendingToken = ++scheduleToken; // closure-shared state read by the callback
  scheduler.schedule();
};
```

The `onMicrotask` / `onTimer` callbacks passed to the factory share the same body shape (only the trigger label differs):

```typescript
const onMicrotask = () => {
  const myToken = pendingToken;
  processScheduled = false;     // reset BEFORE invoking process
  pendingHandle = null;
  if (myToken !== scheduleToken) return;
  processPendingData("microtask");
};

const onTimer = () => { /* same as above with "timer" label */ };
```

Resetting `processScheduled` before `processPendingData` allows a re-entrant `scheduleProcessing()` (from leftoverData) to schedule the next microtask. The token check ensures stale microtasks (after `flushPendingData()` increments `scheduleToken` indirectly via a sync `processPendingData()`) are no-ops.

For the `setTimeout` fallback, `pendingHandle` must be assigned the timer id at schedule time so `destroy()` can `clearTimeout` it. This assignment is performed inside `scheduleProcessing` (or inside the factory's `schedule()` body, with a write-back to a shared variable). What matters is that `pendingHandle` is non-null exactly during a pending timer-fallback delivery and `null` otherwise.

#### `processPendingData` end-of-frame change

```diff
- if (leftoverData && !rafScheduled) {
-   scheduleProcessing();
- }
+ if (leftoverData) {
+   scheduleProcessing();
+ }
```

The `!rafScheduled` guard becomes redundant because `scheduleProcessing` itself short-circuits when `processScheduled` is true.

#### Cleanup

`destroy()` first sets `disposed = true` and bumps `scheduleToken` so any
in-flight scheduled callback that fires after teardown observes the disposed
state (or finds its captured token stale) and bails before calling
`processPendingData`. It then invokes the factory-returned `dispose()` hook,
which detaches `port2.onmessage` and closes both `MessagePort` objects on
the `MessageChannel` path (no-op on the `setTimeout` fallback path), and
finally clears any pending timer handle held in `pendingHandle`:

```typescript
const destroy = () => {
  // ...existing cleanup (focus listener, ackFlushTimer, pendingAckBytes flush)...
  try { scheduler.dispose(); } catch { /* ignore */ }
  if (pendingHandle !== null) {
    clearTimeout(pendingHandle);
    pendingHandle = null;
  }
  processScheduled = false; // defensive
};
```

The `MessagePort` instances themselves are encapsulated inside the factory's
closure and not exposed as variables outside it; `dispose()` is the single
entry point that closes them. This keeps the scheduler primitive an
implementation detail of the factory.

### Dependencies

**Internal Dependencies:**
- `src/terminal-app/pty-handler.ts`: Primary site of the change.
- `src/pty/client.ts`: `ackBytes` is invoked from the new path identically to before; no API change.
- `src/terminal/canvas-renderer.ts`: Continues to be driven by its own `requestAnimationFrame` loop. No change.

**External Dependencies:**
- `MessageChannel` Web API: Available in WebKitGTK and WebView2. Standard ES feature.
- No new npm or Cargo dependencies.

## Test Scenarios

### Unit Tests (TypeScript, `bun test`)

Test file: `src/terminal-app/pty-handler.test.ts` (existing). Add new cases.

- [ ] **TS-MT-1 (FR1):** Calling `scheduleProcessing()` once results in exactly one `port1.postMessage(0)` call (verified via `MessageChannel` mock injection or test-only override of `globalThis.MessageChannel`).
- [ ] **TS-MT-2 (FR1):** Two consecutive `scheduleProcessing()` calls before the microtask fires produce only one `postMessage` (deduplication via `processScheduled`).
- [ ] **TS-MT-3 (FR2):** When `MessageChannel` is undefined (mocked), the scheduler falls through directly to `setTimeout(0)` and `trigger` is `"timer"`. `queueMicrotask` is not consulted.
- [ ] **TS-MT-4 (FR4):** A scheduled callback whose captured token no longer matches `scheduleToken` (e.g. after a synchronous `processPendingData()` call bumps the token at the top of the function) does not re-enter `processPendingData`. The token is captured at queue time and travels with the scheduled delivery — via the `MessageEvent.data` payload on the `MessageChannel` path, or via closure capture on the timer path.
- [ ] **TS-MT-5 (FR5):** `processPendingData` is called with `trigger="microtask"` on the primary path. There is no remaining call site that passes `"raf"`.
- [ ] **TS-MT-6 (FR6):** When `process_pty_data` returns `consumed < input.length` (leftoverData path) the handler schedules another microtask before returning.
- [ ] **TS-MT-7 (FR7):** `pty-handler.ts` itself does not call `globalThis.requestAnimationFrame` on the data path. Verified statically (source scan: zero `requestAnimationFrame(` references in `pty-handler.ts` outside of comments) and dynamically by spying on `globalThis.requestAnimationFrame` while injecting a chunk through a stub renderer that does not call rAF — the spy must record zero calls. (The real `CanvasRenderer` may call rAF for vsync-aligned painting, which is unchanged and out of scope for this assertion.)
- [ ] **TS-MT-8 (FR9):** `forceRender` (or its current equivalent) still uses `requestAnimationFrame` — the renderer call site is unchanged.
- [ ] **TS-MT-9 (FR11):** `destroy()` closes both `MessagePort` instances (after detaching `port2.onmessage`) and clears any pending timer handle. A `MessageChannel` callback that fires after `destroy()` is a no-op (verified via late-flush of a deferred fake channel).
- [ ] **TS-MT-10 (FR3):** `pendingHandle` is `null` on the `MessageChannel` path and a non-null `ReturnType<typeof setTimeout>` value on the `setTimeout` fallback.
- [ ] **TS-MT-11 (FR7):** `rafScheduled` and `rafHandle` identifiers are removed from the source (verified by a `bun test` source scan or existing typecheck).

### Integration Tests

(eMterm uses unit + E2E split; no separate integration suite.)

### E2E Tests

**Existing E2E run:** `./scripts/run-e2e-docker.sh test`

- [ ] **E2E-MT-1 (NFR1):** Existing `visibility-raf-heartbeat.e2e.js` continues to pass — the rAF heartbeat still fires `setVisibility(false)` on monkey-patched rAF stall, because the visibility controller is independent from the data scheduler.
- [ ] **E2E-MT-2 (NFR1):** New spec `microtask-data-flow.e2e.js`:
  - Monkey-patch `globalThis.requestAnimationFrame` to a no-op (still records calls but never invokes callbacks).
  - Inject a busy PTY producer (e.g. write `"abcdef".repeat(10000)` directly via the `injectData` test hook, or run `seq 1 100000` in a real PTY).
  - Wait 2 seconds.
  - Assert that the WASM grid state has changed (e.g. cursor row advanced) OR that `pty_ack` was invoked at least once during the rAF stall (verified via `pty_get_send_stats.in_flight` decrease).
  - Assert no `backpressure stalled` warn line in the backend log for the duration.
- [ ] **E2E-MT-3 (NFR3):** Existing `visibility-throughput-bench.e2e.js` (or equivalent throughput spec) shows total processed bytes within ±10% of baseline.
- [ ] **E2E-MT-4 (NFR5):** All other existing E2E specs in `e2e-tests/specs/` pass without modification.

### Edge Cases

- [ ] Edge: `injectData` is called during a microtask (re-entrant). The new chunk goes into `pendingChunks` and is consumed either in the current microtask (if not yet drained) or the next scheduled one. Verified by TS-MT-6 with mid-flight injection.
- [ ] Edge: `flushPendingData()` runs while a microtask is in flight. The token check (FR4) makes the microtask a no-op; pendingChunks is cleared without parsing.
- [ ] Edge: `tab:activated` while no chunks are pending. `notifyTabActivated` invokes `scheduleProcessing()` only if there are pending chunks or leftover data (existing behavior, unchanged).
- [ ] Edge: System under high CPU load — microtasks queue but each one obeys the time budget. Behavior degrades gracefully (longer per-tick latency) without losing data.
- [ ] Edge: `MessageChannel.close` after destroy — any in-flight `postMessage` is dropped silently; the token-check path also bails because `processScheduled` is reset.

### Performance Tests

Manual benchmarking expected (the project does not have an automated micro-benchmark suite):

- [ ] **Perf-1 (NFR2):** Run `yes | head -c 100M` in eMterm before and after the change; record total wall time. Difference within ±10%.
- [ ] **Perf-2 (NFR3):** Type-and-paint round trip (manual or via E2E) within ±10% of baseline.

### Manual Reproduction (Existing Freeze Symptom)

Reproduce the original freeze and confirm fix:

- [ ] Launch eMterm and run `while true; do date; sleep 0.05; done` in a tab.
- [ ] Switch to a different workspace and lock the desktop for >= 30 minutes.
- [ ] Return to eMterm. UI must respond immediately. The terminal must show output up to the most recent timestamp (delivered via visibility-aware-pty-streaming snapshot if engaged, or live-streamed via the microtask scheduler if not).
- [ ] Repeat with mux mode, multiple panes.

## Security Considerations

Not applicable. The change is internal to the renderer process. `MessageChannel` is a standard browser primitive; no cross-origin or network surface is touched.

## Error Handling

| Error Condition | Handling |
|------------------|----------|
| `MessageChannel` constructor throws (highly unexpected) | Caught at handler setup. Falls through to the `setTimeout(0)` factory branch. Logged once at `console.warn`. |
| `port1.postMessage` throws after `close()` | Wrapped in try/catch; ignored (handler is being torn down anyway). |
| `processPendingData` throws | Existing catch in `processPendingData` handles errors via `tryRecoverFromWasmCrash`. The scheduled callback itself does not propagate an unhandled rejection because the body is synchronous. |
| `setTimeout` fallback fails to fire (e.g. heavily throttled tab) | Hidden state is the only realistic case; visibility-aware-pty-streaming already pauses the backend in this case so no data accumulates frontside. The `MessageChannel` path is not throttled by WebKit, so this is theoretical for the primary path. |

## Performance Optimization

- The `MessageChannel` scheduler is allocated **once per PTY handler** and reused for the handler's lifetime.
- `scheduler()` performs a single `port1.postMessage(0)` per scheduled tick — payload is a literal `0` to avoid clonable-data overhead.
- The `processScheduled` early-return pattern from the rAF path is preserved, ensuring at most one in-flight microtask per handler.
- The existing per-call WASM time budget (FR8) prevents pathological long microtasks from blocking input events. Leftover work is parked into `leftoverData` and resumed on the next microtask, the same way the rAF path handled it.
- No additional allocations per scheduled tick beyond what `port1.postMessage(0)` does internally (negligible).

## Success Criteria

- [ ] FR1〜FR11 implemented and covered by unit tests (TS-MT-1〜TS-MT-11).
- [ ] NFR1〜NFR6 satisfied (NFR1 verified by E2E-MT-2, NFR2/NFR3 by Perf-1/2, NFR5 by E2E-MT-4).
- [ ] All existing E2E specs (`./scripts/run-e2e-docker.sh test`) pass without modification.
- [ ] `bun test` passes.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` passes (no backend change, but sanity).
- [ ] `bun run typecheck` passes.
- [ ] Manual reproduction (US1 / US2) confirms no freeze across workspace switch / minimize for >= 30 min.

## Open Questions

None. Technical decisions made during requirements gathering:

1. **MessageChannel as primary, `setTimeout(0)` as the sole fallback.** Reason: both are task schedulers that yield between drains. `queueMicrotask` was excluded because microtask chaining (leftover → scheduleProcessing → queueMicrotask) cannot yield to the task queue and would starve rendering under sustained bursts. MessageChannel is observable in DevTools as a discrete task tier (helps profiling) and is uniformly available in WebKitGTK / WebView2.
2. **No new backend ack event.** Reason: backend already provides `pty_ack` and `wait_for_drain` with full backpressure semantics. The original "ack-based flow control" item from the strategy memo turned out to already exist; what was missing was that the **frontend stopped sending acks** because rAF stopped. Microtask scheduling alone resolves the root cause.
3. **Renderer stays on rAF.** Reason: visual updates should remain vsync-aligned, and the renderer being paused while hidden is correct (no work to be done, the WASM grid is updated in the background).
4. **Existing safety nets retained.** Reason: each addresses a different failure mode (rAF heartbeat = render stalled; visibility-aware streaming = backend bandwidth saving; visibility-render-recovery = WASM corruption after suspend). They remain orthogonally useful.

## Implementation Phases

### Phase 1: Scheduler primitive

**Goals:** Introduce `createMicrotaskScheduler` factory and integrate into `scheduleProcessing`.

**Deliverables:**
- `pty-handler.ts` modifications: add factory, replace rAF call site, rename `rafScheduled` → `processScheduled`, rename `rafHandle` → `pendingHandle`, update `trigger` type.
- Unit tests TS-MT-1 〜 TS-MT-7, TS-MT-9, TS-MT-10, TS-MT-11.

### Phase 2: Cleanup and edge cases

**Goals:** Ensure `destroy()` cleanly tears down the channel and any timer fallback. Verify token semantics across `flushPendingData()`.

**Deliverables:**
- `destroy` path updates.
- Unit test TS-MT-4, TS-MT-8.

### Phase 3: E2E and verification

**Goals:** Confirm hidden-state operation and no regression.

**Deliverables:**
- New `e2e-tests/specs/microtask-data-flow.e2e.js`.
- Run `./scripts/run-e2e-docker.sh test` and ensure all specs pass.
- Manual reproduction of the original freeze symptom (US1).

## References

- Strategy memo: `tmp/xterm-vs-emterm-flow-control.md`
- Existing features:
  - `doc/tasks/visibility-raf-heartbeat/SPEC.md`
  - `doc/tasks/visibility-aware-pty-streaming/SPEC.md`
  - `doc/tasks/visibility-render-recovery/SPEC.md`
- xterm.js reference: `xterm.js/src/common/input/WriteBuffer.ts` (`MessageChannel`-based microtask scheduling)
- Existing source:
  - `src/terminal-app/pty-handler.ts` (`scheduleProcessing`, `processPendingData`, ack coalescing)
  - `src/pty/client.ts` (`ackBytes` → `pty_ack`)
  - `src-tauri/src/pty/backpressure.rs` (`SessionBackpressure::{ack, wait_for_drain, hidden_wake, force_wake}`)
