# Feature: visibility-raf-heartbeat

## Overview

Augment `VisibilityController` with a self-driven `requestAnimationFrame` heartbeat so the frontend can detect WebKit background-throttled states even when `document.visibilitychange` does not fire (workspace switch, occluded window, screen lock). When rAF stalls for >= 5 seconds, the controller treats the session as `effective_visible=false` and reuses the existing `setVisibility(false)` notification path; rAF resumption immediately notifies `setVisibility(true)`.

## Objectives

- Eliminate freeze cases where `chunkRecv` stops while `setInterval` keeps running because rAF is throttled but `visibilitychange` is not delivered.
- Reuse the existing detached / snapshot mechanism implemented in `visibility-aware-pty-streaming` (FR1〜FR16) without changing backend code or mux protocol.
- Provide observable diagnostics (`reason=raf-stall`) so operations can distinguish rAF-driven hidden transitions from document-level ones.

## User Stories

### US1: Workspace switching while AI session runs

As a Linux desktop user running Claude Code in emterm, I want the terminal to suspend backend streaming when I switch workspaces, so that returning hours later does not freeze the UI.

**Acceptance Criteria:**
- [ ] After 5 seconds of rAF stall the backend receives `setVisibility(false)`.
- [ ] On rAF resumption the backend receives `setVisibility(true)` immediately and the snapshot is replayed by the existing path.
- [ ] No `backpressure stalled` warning appears in the backend log for the duration of the hidden state.

### US2: Screen lock recovery

As a desktop user who locks the screen, I want emterm to resume cleanly with a fresh frame state on unlock.

**Acceptance Criteria:**
- [ ] During the locked period the backend stops streaming raw bytes and `in_flight` does not grow above the high-water mark.
- [ ] On unlock the visible path delivers the snapshot in a single message (existing FR8/FR9 behavior).

### US3: Laptop suspend / resume

As a notebook user who suspends and resumes the system, I want the controller to avoid declaring rAF dead just because the system was paused.

**Acceptance Criteria:**
- [ ] No false `setVisibility(false)` notification fires immediately after a system resume.
- [ ] The `lastRafPerfMs` baseline is reset cleanly so the next legitimate stall is still detected.

## Technical Requirements

### Functional Requirements

- **FR1 (rAF self-loop):** `VisibilityController` schedules a `requestAnimationFrame` callback whenever `effective_visible` is true. Each callback updates `lastRafPerfMs = nowFn()` and re-schedules itself. The loop terminates when `effective_visible` becomes false.
- **FR2 (rAF dead detection):** A periodic health-check (the existing `HEALTH_CHECK_MS = 10000` interval) computes `now - lastRafPerfMs`. If the result exceeds `RAF_DEAD_THRESHOLD_MS = 5000`, `rafAlive` flips to false and `evaluate()` is invoked.
- **FR3 (currentEffective integration):** `currentEffective()` returns `getDocumentVisible() && focused && rafAlive`.
- **FR4 (rAF resume → immediate visible):** When the rAF callback fires after `rafAlive` was false, the controller flips `rafAlive` to true and calls `evaluate()` synchronously. No debounce is applied.
- **FR5 (suspend gap skip):** The health-check tracks `lastHealthTickPerfMs`. If the gap between consecutive ticks exceeds `HEALTH_CHECK_MS * 3 = 30000`, the dead-detection branch is skipped for that tick and `lastRafPerfMs` is reset to the current `now`.
- **FR6 (DIAG-IDLE reason field):** Hidden notifications append `| reason={document|focus|raf-stall}` to the existing `[DIAG-IDLE] visibility→hidden at <ISO>` log line. Multiple causes are joined with `+` (e.g., `reason=document+focus`).
- **FR7 (loop termination on hidden):** When `notify(false)` runs, the controller cancels any scheduled rAF (via `cancelAnimationFrame`) and the next callback (if already in flight) does not re-schedule itself. No CPU work happens while hidden.
- **FR8 (DI for rAF / now):** `VisibilityControllerDeps` adds optional `requestAnimationFrameFn`, `cancelAnimationFrameFn`, and `nowFn` fields. The constructor stores **lazy wrapper lambdas** as defaults (e.g., `(cb) => globalThis.requestAnimationFrame(cb)`) so that test-time monkey-patching of the global property is observed by the controller. Lazy wrappers are also used for `cancelAnimationFrame` and `now` (`performance.now()` with `Date.now()` fallback resolved per call).
- **FR9 (start grace period):** When `lastRafPerfMs === null` (initial state, before any rAF callback), the dead-detection branch is skipped. The first scheduled rAF establishes the baseline.
- **FR10 (compat with FR1〜FR16 of visibility-aware-pty-streaming):** No backend / IPC code is touched. The new logic only changes whether `setVisibility(true|false)` is dispatched; the dispatch APIs themselves (`PtyClient.setVisibility`, `MuxClient.sendSetVisibility`) are unchanged.

### Non-Functional Requirements

- **NFR1 - Performance:** Inside the rAF callback, only `nowFn()` is read and a single boolean compared. No allocation, no I/O, no log emission per frame. While hidden, the rAF loop is fully stopped.
- **NFR2 - Resource:** Adds at most ~5 numeric/boolean fields to `VisibilityController`. No long-lived buffers.
- **NFR3 - Reliability:** The controller initializes safely in environments without rAF (DI fallback). Idempotent `start()` / `stop()` semantics from the existing controller are preserved.
- **NFR4 - Observability:** Hidden transitions caused by rAF stall are clearly attributed via `reason=raf-stall` in the `[DIAG-IDLE]` log; existing `[DIAG-PTY-HEALTH]` snapshot is unchanged.
- **NFR5 - Compatibility:** `visibility-aware-pty-streaming` FR1〜FR16 must continue to pass their existing tests. The `[DIAG-IDLE]` log retains its prefix and `at <ISO>` field; `reason` is appended at the end.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  VisibilityController                   │
│                                                         │
│  Inputs (3 sources):                                    │
│    1. document.visibilitychange  → getDocumentVisible() │
│    2. tauri webview onFocusChanged → focused            │
│    3. rAF heartbeat (NEW)         → rafAlive            │
│                                                         │
│  effective_visible = #1 && #2 && #3                     │
│                                                         │
│  Hide path: 1000 ms debounce (existing)                 │
│  Show path: immediate (existing)                        │
│                                                         │
│  Dispatch (unchanged):                                  │
│    PtyClient.setVisibility(boolean)                     │
│    MuxClient.sendSetVisibility(boolean)                 │
└─────────────────────────────────────────────────────────┘
```

### Component Diagram

```
                    [requestAnimationFrame self-loop]
                                  │
                                  ▼
                       VisibilityController
                       ┌───────────────────┐
   visibilitychange ──▶│ document signal    │
   onFocusChanged   ──▶│ focus signal       │── evaluate ──▶ notify ──▶ dispatch
   rAF callback     ──▶│ rafAlive signal    │
                       │ (NEW: heartbeat)   │
                       └───────────────────┘
                                  │
                                  ▼
                       [setInterval health-check]
                              ─ rAF dead detection
                              ─ suspend gap detection
```

### Data Flow

```
visible state                  rAF callback                health-check tick
     │                              │                              │
     ▼                              ▼                              ▼
schedule rAF ─────────▶  update lastRafPerfMs ─────▶ compare with now
     │                          │                              │
     │                          ▼                              ▼
     │                  if rafAlive flipped          if gap > 5000ms
     │                  call evaluate()              flip rafAlive=false
     │                                               call evaluate()
     ▼
keep looping...
```

### State Machine

```
                   ┌────────────────────────────────────┐
                   │            STARTED                 │
                   │  (start() called, listeners up)    │
                   └─────────────┬──────────────────────┘
                                 │
              ┌──────────────────┴───────────────────┐
              ▼                                      ▼
   ┌───────────────────┐                   ┌───────────────────┐
   │ effective_visible │                   │ effective_hidden  │
   │  rAF loop ON      │                   │  rAF loop OFF     │
   └───────────────────┘                   └───────────────────┘
        │     ▲                                  │      ▲
        │     │                                  │      │
        │     │ rAF callback after dead          │      │
        │     │ (rafAlive flips true)            │      │
        │     │ OR document/focus comes back     │      │
        │     │                                  │      │
        │     └──────────────────────────────────┘      │
        │                                               │
        │ rAF stall > 5s   (rafAlive=false)             │
        │ OR document.hidden=true                       │
        │ OR window blur (debounced 1s)                 │
        └───────────────────────────────────────────────▶
```

### File Structure

```
src/pty/
  visibility-controller.ts       # Modified: add rAF heartbeat fields & methods
  visibility-controller.test.ts  # Modified: add TS-29〜TS-33

e2e-tests/specs/
  visibility-raf-heartbeat.e2e.js  # NEW: rAF monkey-patch verification

doc/tasks/visibility-raf-heartbeat/
  REQUIREMENTS.md
  SPEC.md
  IMPLEMENTATION.md       # by sdd.2
  VERIFICATION.md         # by sdd.2
  VERIFICATION_RESULT.md  # by sdd.6
  freeze-repro-rafstall.md  # NEW: manual repro procedure
  sdd.yaml
```

### Public API Changes

#### `VisibilityControllerDeps` (additions)

```typescript
export interface VisibilityControllerDeps {
  // ...existing fields...

  /** Test injectable. Defaults to globalThis.requestAnimationFrame. */
  requestAnimationFrameFn?: typeof requestAnimationFrame;
  /** Test injectable. Defaults to globalThis.cancelAnimationFrame. */
  cancelAnimationFrameFn?: typeof cancelAnimationFrame;
  /** Test injectable. Returns monotonic ms; defaults to performance.now() (Date.now() fallback). */
  nowFn?: () => number;
}
```

#### `VisibilityController` (added private state)

```typescript
private rafAlive: boolean = true;
private lastRafPerfMs: number | null = null;
private lastHealthTickPerfMs: number | null = null;
private rafHandle: number | null = null;
```

#### Constants

```typescript
/** Threshold above which the rAF heartbeat is considered dead. */
export const RAF_DEAD_THRESHOLD_MS = 5_000;

/** Tick gap above which suspend is suspected and dead detection is skipped. */
export const SUSPEND_GAP_MS = 30_000;
```

### Algorithms

#### rAF self-loop

```typescript
private scheduleRaf(): void {
  if (!this.lastNotifiedTrue || this.destroyed) return;
  this.rafHandle = this.requestAnimationFrameFn((_ts) => {
    this.rafHandle = null;
    const now = this.nowFn();
    this.lastRafPerfMs = now;
    if (!this.rafAlive) {
      this.rafAlive = true;
      this.evaluate();
    }
    if (this.lastNotifiedTrue && !this.destroyed) {
      this.scheduleRaf();
    }
  });
}

private cancelRaf(): void {
  if (this.rafHandle !== null) {
    this.cancelAnimationFrameFn(this.rafHandle);
    this.rafHandle = null;
  }
}
```

(`lastNotifiedTrue` is the existing `lastNotified === true` view.)

#### Health-check tick (extended)

```typescript
private healthTick(): void {
  this.resendCurrent(); // existing behavior

  const now = this.nowFn();
  if (this.lastHealthTickPerfMs !== null) {
    const gap = now - this.lastHealthTickPerfMs;
    if (gap > SUSPEND_GAP_MS) {
      this.lastHealthTickPerfMs = now;
      this.lastRafPerfMs = now; // reset baseline; suspend gap suspected
      return;
    }
  }
  this.lastHealthTickPerfMs = now;

  if (this.lastNotified !== true) return;       // only meaningful while visible
  if (this.lastRafPerfMs === null) return;       // grace period

  const sinceRaf = now - this.lastRafPerfMs;
  if (sinceRaf > RAF_DEAD_THRESHOLD_MS) {
    if (this.rafAlive) {
      this.rafAlive = false;
      this.evaluate();
    }
  }
}
```

#### `notify` integration

```typescript
private notify(visible: boolean): void {
  if (this.lastNotified === visible) return;
  this.lastNotified = visible;
  this.logTransition(visible);
  this.dispatch(visible);
  if (visible) {
    this.scheduleRaf();
  } else {
    this.cancelRaf();
  }
}
```

#### Reason composition for `[DIAG-IDLE]`

```typescript
private hiddenReason(): string {
  const causes: string[] = [];
  if (!this.deps.getDocumentVisible()) causes.push("document");
  if (!this.focused) causes.push("focus");
  if (!this.rafAlive) causes.push("raf-stall");
  return causes.length === 0 ? "unknown" : causes.join("+");
}
```

The `logTransition(false)` branch becomes:

```typescript
console.warn(
  `[WARN][FRONTEND] [DIAG-IDLE] visibility→hidden at ${nowIso} | reason=${this.hiddenReason()}`,
);
```

### Dependencies

**Internal Dependencies:**
- `visibility-aware-pty-streaming` feature: This feature relies on its existing `setVisibility(true|false)` IPC paths and snapshot replay (FR8/FR9). No interface changes required.

**External Dependencies:**
- WebKitGTK rAF / cancelAnimationFrame APIs (available in all supported Tauri targets).
- No new npm or Cargo dependencies.

## Test Scenarios

### Unit Tests (`src/pty/visibility-controller.test.ts`)

- [ ] **TS-29 (FR1, FR2):** rAF stall (no rAF callback for >= 5s) triggers `setVisibility(false)`.
- [ ] **TS-30 (FR3):** `document.hidden=false` and `focused=true` but rAF dead → `currentEffective()` returns false.
- [ ] **TS-31 (FR4):** rAF callback after dead state immediately re-flags `rafAlive=true` and dispatches `setVisibility(true)` (no debounce).
- [ ] **TS-32 (FR9):** When `lastRafPerfMs === null`, the health-check tick must not fire `setVisibility(false)`.
- [ ] **TS-33 (FR5):** Tick gap > 30 s skips dead detection and resets `lastRafPerfMs`; the immediately following stale value does not trigger hidden.
- [ ] **TS-34 (FR7):** After `notify(false)`, `cancelAnimationFrame` is called and no further `requestAnimationFrame` is scheduled until `notify(true)`.
- [ ] **TS-35 (FR6):** `[DIAG-IDLE]` hidden log includes `reason=raf-stall` when only the rAF signal is responsible.
- [ ] **TS-36 (FR6):** `reason=document+focus` when both DOM signals are responsible at the same time.
- [ ] **TS-37 (NFR3):** Constructing the controller without injecting `requestAnimationFrameFn` succeeds in environments where the global is missing (mock).
- [ ] **TS-38 (FR10 idempotency):** Stop/start cycle correctly cancels and restarts the rAF loop.
- [ ] **TS-39 (FR10 mux dispatch):** When `getMuxClient()` returns a connected mux client and rAF dead is detected, `MuxClient.sendSetVisibility(false)` is invoked (and on resume, `sendSetVisibility(true)`).

### Integration / E2E Tests

**Existing E2E tests:** `visibility-aware-streaming.e2e.js`, `freeze-regression.e2e.js`, `visibility-throughput-bench.e2e.js`, `visibility-resume-block.e2e.js`. All must continue to pass without modification.

**Run command:** `./scripts/run-e2e-docker.sh test`

#### `e2e-tests/specs/visibility-raf-heartbeat.e2e.js` (NEW)

- [ ] **E2E-1:** Monkey-patch `window.requestAnimationFrame` to record schedules but never invoke callbacks. After 6 s, verify backend received `pty_set_visibility(false)` (via diagnostic invoke).
- [ ] **E2E-2:** After E2E-1, restore the real rAF and verify backend received `pty_set_visibility(true)` within 100 ms and the rendered grid catches up after replay.
- [ ] **E2E-3:** Backend `pty_get_send_stats` shows `sent_bytes` did not grow during the stall. Test setup must run a busy PTY producer (e.g., `seq 1 999999` or `yes`) before the stall so that `sent_bytes` would otherwise grow if the hidden path were not engaged.

### Edge Cases

- [ ] Edge: rAF dead detected in mux mode → `MuxClient.sendSetVisibility(false)` is called (and not just `PtyClient.setVisibility`). Verified by **TS-39** (mux client mock asserts `sendSetVisibility(false)` is dispatched).
- [ ] Edge: `document.hidden=true` and rAF dead arrive at the same tick → `notify(false)` is called only once.
- [ ] Edge: Multiple `start()` calls do not duplicate the rAF loop.
- [ ] Edge: `stop()` while rAF is scheduled correctly cancels the handle.

### Performance Tests

Implicit only. The rAF self-loop adds one `performance.now()` call per frame while visible. No explicit micro-benchmark required.

## Security Considerations

Not applicable. The change is internal to the renderer process and does not handle user input, network, or privileged data.

## Error Handling

| Error condition | Handling |
|------------------|----------|
| `requestAnimationFrame` throws | Caught and logged via `console.warn`; controller falls back to relying on `document.visibilitychange` and `onFocusChanged` (degraded mode but no crash) |
| `cancelAnimationFrame` throws | Swallowed (best-effort cleanup, same as existing `focusUnsubscribe()` pattern) |
| `nowFn()` returns non-monotonic value (test fake) | Treated literally; tests are responsible for monotonic fakes |

## Performance Optimization

- rAF callback body is intentionally minimal (one `nowFn()` call + boolean check).
- Hidden state cancels the rAF loop entirely → no per-frame cost while hidden.
- Suspend gap detection prevents unnecessary `setVisibility(false)` flapping after system wake.

## Success Criteria

- [ ] All FR1〜FR10 implemented and covered by unit tests.
- [ ] All NFR1〜NFR5 satisfied (qualitative review).
- [ ] `visibility-raf-heartbeat.e2e.js` passes in the Docker E2E environment.
- [ ] All existing `visibility-aware-streaming.e2e.js` / `freeze-regression.e2e.js` / `visibility-throughput-bench.e2e.js` / `visibility-resume-block.e2e.js` continue to pass.
- [ ] `bun test` and `cargo test` both pass.
- [ ] `bun run typecheck` passes.
- [ ] Manual repro per `freeze-repro-rafstall.md` shows no freeze.

## Open Questions

None at the time of writing. All clarification points were resolved during requirements gathering.

## Implementation Phases

### Phase 1: Controller heartbeat additions
**Goals:** Add rAF self-loop, dead detection, suspend gap detection, reason logging.
**Deliverables:**
- `src/pty/visibility-controller.ts` modifications (FR1〜FR9)
- New constants `RAF_DEAD_THRESHOLD_MS`, `SUSPEND_GAP_MS`

### Phase 2: Tests
**Goals:** Validate behavior in unit and E2E.
**Deliverables:**
- `src/pty/visibility-controller.test.ts` cases TS-29〜TS-38
- `e2e-tests/specs/visibility-raf-heartbeat.e2e.js`

### Phase 3: Verification & Documentation
**Goals:** Verify regression-free integration with `visibility-aware-pty-streaming`.
**Deliverables:**
- All existing E2E specs continue to pass
- `freeze-repro-rafstall.md` describing manual reproduction (workspace switch / screen lock)

## References

- Strategy memo: `tmp/freeze-rafheartbeat-direction.md`
- Existing feature: `doc/tasks/visibility-aware-pty-streaming/SPEC.md`
- Existing controller: `src/pty/visibility-controller.ts`
- Freeze evidence log: `~/.local/share/net.laser5.app.emterm/logs/emterm.log` (2026-05-05 22:42 〜 2026-05-06 12:17)
