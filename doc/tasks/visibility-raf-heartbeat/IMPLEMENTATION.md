# Implementation Plan: visibility-raf-heartbeat

## Overview

Add a self-driven `requestAnimationFrame` heartbeat to the existing `VisibilityController` so the frontend can detect WebKit-throttled hidden states even when `document.visibilitychange` does not fire. The mechanism reuses the existing `setVisibility(true|false)` dispatch path, so no backend code is touched.

## Objectives

- Detect rAF stall (>= 5 s) within 10 s and notify the backend as hidden
- Notify the backend immediately on rAF resumption
- Skip false positives caused by system suspend (>= 30 s tick gap)
- Preserve `visibility-aware-pty-streaming` FR1〜FR16 behavior

## Prerequisites

### Development Environment

- Node.js / Bun (existing toolchain)
- Docker (for E2E via `./scripts/run-e2e-docker.sh`)
- Existing `visibility-aware-pty-streaming` feature merged

### Dependencies

- Existing `VisibilityController` and `PtyClient.setVisibility` / `MuxClient.sendSetVisibility`
- Existing E2E harness (`scripts/run-e2e-docker.sh`)
- No new npm or Cargo dependency

## Architecture Overview

### Technology Stack

- **Language**: TypeScript (frontend only)
- **Framework**: Tauri (existing); WebKitGTK runtime
- **Test runner**: Bun test (unit), tauri-driver + WebdriverIO (E2E)

### Design Approach

The controller gains a third effective-visibility input besides `document.visibilityState` and Tauri webview focus: a self-loop rAF heartbeat. The heartbeat publishes `lastRafPerfMs` per frame, the existing 10 s health-check evaluates staleness, and a hidden notification is dispatched via the existing path. Hidden state cancels the rAF loop, eliminating per-frame work while occluded.

### Component Interaction

```
[document.visibilitychange] ─┐
[onFocusChanged]            ─┼─▶ VisibilityController.evaluate()
[rAF callback (NEW)]        ─┘            │
                                          ▼
                          notify(true|false) ──▶ PtyClient.setVisibility
                                              ──▶ MuxClient.sendSetVisibility
                                              ──▶ scheduleRaf / cancelRaf
```

`scheduleRaf` runs only while the last notified state is `true`. The health-check interval (existing `HEALTH_CHECK_MS = 10_000`) gains rAF dead detection alongside its current `resendCurrent()` work.

## Implementation Phases

### Phase 1: Heartbeat fields, DI, and constants

**Goal**: Lay the foundation. Add new state fields, constants, dependency injection slots without changing observable behavior.

**Files to Modify**:
- `src/pty/visibility-controller.ts` — add heartbeat state, DI fields, helper accessors

**Files to Create**: none

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `RAF_DEAD_THRESHOLD_MS` | Module constant for stall threshold (5_000) | — | Exported for tests |
| `SUSPEND_GAP_MS` | Module constant for suspend-gap detection (30_000) | — | Exported for tests |
| `VisibilityControllerDeps` | Optional DI fields for rAF / now functions | — | Defaults bind to globals when absent |
| `VisibilityController` private state | Holds `rafAlive`, `lastRafPerfMs`, `lastHealthTickPerfMs`, `rafHandle` | — | Initialized in constructor |
| `nowFn` accessor | Returns monotonic ms | — | Returns `performance.now()` if available, else `Date.now()` |

**Processing Flow** (constructor):
1. Bind existing dependencies (timers, focus subscribe, etc.) — unchanged
2. Bind `requestAnimationFrameFn`, `cancelAnimationFrameFn`, `nowFn` from deps or globals
3. Initialize `rafAlive = true`, all timestamp fields `= null`, `rafHandle = null`

**Implementation Steps**:
1. **Export new constants** — Add `RAF_DEAD_THRESHOLD_MS` and `SUSPEND_GAP_MS` as module-level exports
2. **Extend deps interface** — Add three optional fields documented as test-injectable
3. **Add private state fields** — `rafAlive`, `lastRafPerfMs`, `lastHealthTickPerfMs`, `rafHandle`
4. **Use lazy default wrappers in constructor** — Unlike the existing `setTimeoutFn` which binds at construction, the rAF / cancelAF / now defaults wrap a per-call `globalThis.<api>` lookup so post-construction monkey-patches (used by E2E) are observed
5. **Add `nowFn` resolution** — Prefer `performance.now()` then fall back to `Date.now()`, resolved per call so the default also follows the lazy pattern

**Dependencies**: Requires nothing. Blocks Phase 2.

**Testing Approach**:
- Unit: TS-37 (constructs without injected rAF)
- Integration: not applicable
- E2E: not applicable
- Manual: not applicable

**Acceptance Criteria**:
- [ ] Existing tests still pass with no behavior change
- [ ] `RAF_DEAD_THRESHOLD_MS` and `SUSPEND_GAP_MS` are imported by tests in Phase 2

**Estimated Effort**: small

---

### Phase 2: rAF self-loop and dead detection

**Goal**: Make the controller actually drive a heartbeat and act on staleness.

**Files to Modify**:
- `src/pty/visibility-controller.ts` — `scheduleRaf`, `cancelRaf`, `currentEffective`, `notify`, health-tick extension

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `scheduleRaf()` | Schedules next rAF when last notified state is visible | Controller started, `effective_visible=true` | `rafHandle` holds the request id |
| `cancelRaf()` | Cancels pending rAF request | — | `rafHandle` reset to null |
| `currentEffective()` | AND of document, focus, rAF alive | — | Returns boolean; rAF alive participates |
| Health tick extension | Detect suspend gap; detect rAF stall | Existing 10 s interval | Updates rAF alive flag; calls evaluate when state changes |
| `notify(visible)` | Adds rAF loop start/stop on transition | — | Loop exists iff visible |

**Processing Flow** (rAF callback):
1. Record `now = nowFn()` and store in `lastRafPerfMs`
2. If `rafAlive == false`: flip to true, call `evaluate()`
3. If still effective_visible: re-schedule via `requestAnimationFrameFn`
4. Else: terminate loop (do not re-schedule)

**Processing Flow** (health tick):
1. Resend current state (existing behavior, idempotent)
2. Compute `now = nowFn()`
3. Compare with previous tick:
   - First tick (`lastHealthTickPerfMs == null`) → record now, return
   - Gap > `SUSPEND_GAP_MS` → reset `lastRafPerfMs = now`, record tick, return
4. Record this tick
5. Skip dead detection if `lastNotified != true` (only meaningful while visible)
6. Skip dead detection if `lastRafPerfMs == null` (start grace period)
7. If `now - lastRafPerfMs > RAF_DEAD_THRESHOLD_MS`:
   - If `rafAlive` was true: flip to false, call `evaluate()`

**Processing Flow** (notify):
1. Existing dedup / log / dispatch path unchanged
2. After dispatch, if visible → call `scheduleRaf()`
3. If hidden → call `cancelRaf()`

**Implementation Steps**:
1. **Extend `currentEffective`** — Include `rafAlive` in the AND
2. **Add `scheduleRaf` / `cancelRaf`** — Request/cancel a single in-flight callback at a time
3. **Wire `notify` to start/stop the loop** — Visible transition kicks off, hidden cancels
4. **Extend the health-check interval body** — Suspend gap detection then dead detection
5. **Initial schedule on `start()`** — After existing `evaluate()` call, schedule rAF when initial state is visible

**Dependencies**: Requires Phase 1. Blocks Phase 3 logging changes.

**Testing Approach**:
- Unit: TS-29 (stall triggers hidden), TS-30 (rafAlive false beats document=visible), TS-31 (resume immediate visible), TS-32 (grace period), TS-33 (suspend gap skip), TS-34 (cancel on hidden), TS-38 (stop/start cycle), TS-39 (mux dispatch on rAF dead)
- Integration: not applicable
- E2E: covered in Phase 4
- Manual: not applicable

**Acceptance Criteria**:
- [ ] Mocking rAF to never fire causes hidden notification within 6 s of becoming visible
- [ ] Resuming rAF after stall causes immediate visible notification
- [ ] No rAF callbacks are scheduled while in hidden state

**Estimated Effort**: medium

---

### Phase 3: DIAG-IDLE reason logging

**Goal**: Make the new mechanism observable in production logs without breaking existing log-format consumers.

**Files to Modify**:
- `src/pty/visibility-controller.ts` — extend `logTransition` for hidden case, add `hiddenReason()` helper

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `hiddenReason()` | Compose human-readable cause string | Called from `logTransition(false)` | Returns string with one or more of `document`, `focus`, `raf-stall`, joined by `+` |
| `logTransition(false)` extension | Append reason to existing log line | — | Output includes `\| reason=<reason>` suffix; visible-side log unchanged |

**Processing Flow** (hiddenReason):
1. Start with empty list
2. If `getDocumentVisible()` is false → push `document`
3. If `focused` is false → push `focus`
4. If `rafAlive` is false → push `raf-stall`
5. Return `unknown` if list empty (defensive); otherwise join with `+`

**Implementation Steps**:
1. **Add `hiddenReason()` private helper**
2. **Extend `logTransition(false)`** — Append `| reason=...` to the existing format
3. **Leave visible-side log untouched** — Maintains `hiddenForMs=N` field

**Dependencies**: Requires Phase 2 (`rafAlive` accessible).

**Testing Approach**:
- Unit: TS-35 (reason=raf-stall when only rAF), TS-36 (reason=document+focus when both DOM signals)
- E2E: indirectly via E2E-1 logging

**Acceptance Criteria**:
- [ ] Existing E2E specs that grep `[DIAG-IDLE] visibility→hidden` still match
- [ ] New `reason=...` field is present and parseable

**Estimated Effort**: small

---

### Phase 4: Tests

**Goal**: Cover all FR / NFR via unit and E2E tests.

**Files to Modify**:
- `src/pty/visibility-controller.test.ts` — add cases TS-29〜TS-38

**Files to Create**:
- `e2e-tests/specs/visibility-raf-heartbeat.e2e.js` — rAF monkey-patch verification

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Unit fixture | Provide fake timers, fake rAF, fake nowFn | Bun test isolation | Each test uses fresh controller; deps fully mocked |
| E2E spec | Drive Tauri webview, monkey-patch rAF, observe backend stats | tauri-driver running | Backend `pty_get_send_stats` shows no growth during stall |

**Processing Flow** (E2E spec):
1. Launch app, wait for terminal idle
2. Start a busy PTY producer in the focused pane (e.g., `seq 1 999999` typed by helper) so that backend has data to forward — required so the post-stall byte-stability assertion is meaningful
3. Snapshot `pty_get_send_stats` (`sent_count_pre`, `sent_bytes_pre`)
4. Inject script that overrides `globalThis.requestAnimationFrame` with a stub that records calls but never invokes the callback (lazy default in the controller observes the stub)
5. Wait ~6 s
6. Confirm `pty_set_visibility(false)` was dispatched (log scan for `[DIAG-IDLE] visibility→hidden ... reason=raf-stall`, or backend stats showing the hidden short-circuit)
7. Assert `pty_get_send_stats.sent_bytes` did not exceed `sent_bytes_pre` materially during the stall (allow only the small pre-stall in-flight residue)
8. Restore real rAF (re-attach the original reference)
9. Assert `pty_set_visibility(true)` is dispatched within 100 ms (log scan for `[DIAG-IDLE] visibility→visible`)
10. Assert subsequent terminal output renders (final-state grid contains a value produced after the resume)

**Implementation Steps**:
1. **Add unit fixture utilities** — Helpers to drive fake rAF and to run the health-tick on demand
2. **Author TS-29〜TS-39** — Each focuses on a single FR / NFR. TS-39 covers the mux dispatch path on rAF dead, mirroring the existing dispatch test that uses a mock MuxClient
3. **Create E2E spec** — Use existing helpers from `e2e-tests/helpers/`
4. **Verify regression** — Run existing visibility-related E2E specs unchanged

**Dependencies**: Requires Phases 1–3.

**Testing Approach**:
- Unit: 10 new cases (TS-29〜TS-38)
- E2E: 1 new spec (3 scenarios)
- Manual: covered in Phase 5

**Acceptance Criteria**:
- [ ] `bun test` passes including new cases
- [ ] `./scripts/run-e2e-docker.sh test visibility-raf-heartbeat.e2e.js` passes
- [ ] Existing visibility-related E2E specs continue to pass

**Estimated Effort**: medium

---

### Phase 5: Manual verification & documentation

**Goal**: Provide an actionable manual repro recipe for ops / future regressions.

**Files to Create**:
- `doc/tasks/visibility-raf-heartbeat/freeze-repro-rafstall.md` — manual procedure for workspace switching, screen lock, suspend recovery cases

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Manual repro doc | Step-by-step reproduction of rAF-stall hidden detection | Test user with Linux desktop | Doc covers workspace move, occluded window, screen lock, laptop suspend |

**Implementation Steps**:
1. **Compose repro recipes** — Each scenario states pre-conditions, steps, expected emterm.log evidence, pass criteria
2. **Cross-link from REQUIREMENTS section 11.1** — Mark as referenced
3. **Document log evidence pattern** — `grep "reason=raf-stall" emterm.log` and `grep "backpressure stalled" emterm.log` (latter must be empty)

**Dependencies**: Requires Phases 1–4.

**Testing Approach**:
- Manual only

**Acceptance Criteria**:
- [ ] Doc lists at least four scenarios (workspace move, occluded window, screen lock, suspend)
- [ ] Each scenario has clear pass criteria

**Estimated Effort**: small

---

## Complete File Structure

```
emterm/
├── doc/tasks/visibility-raf-heartbeat/
│   ├── REQUIREMENTS.md                     [exists]
│   ├── SPEC.md                             [exists]
│   ├── IMPLEMENTATION.md                   [this file]
│   ├── VERIFICATION.md                     [created by sdd.2]
│   ├── tasks.yaml                          [created by sdd.2]
│   ├── sdd.yaml                            [exists]
│   └── freeze-repro-rafstall.md            [created in Phase 5]
├── src/pty/
│   ├── visibility-controller.ts            [modified Phases 1–3]
│   └── visibility-controller.test.ts       [modified Phase 4]
└── e2e-tests/specs/
    └── visibility-raf-heartbeat.e2e.js     [created Phase 4]
```

## Testing Strategy

- **Unit (Bun test)**: Cover FR1〜FR9 via TS-29〜TS-38 in `visibility-controller.test.ts`. Use fake timers and injected rAF/now functions. Target near-100% coverage on the new code path
- **E2E (Docker + tauri-driver)**: Validate live behavior in `visibility-raf-heartbeat.e2e.js` by monkey-patching rAF in the running webview and observing backend `pty_get_send_stats`
- **Regression**: Run `visibility-aware-streaming.e2e.js`, `freeze-regression.e2e.js`, `visibility-throughput-bench.e2e.js`, `visibility-resume-block.e2e.js` unchanged
- **Manual**: Workspace move / screen lock / suspend recovery via `freeze-repro-rafstall.md`

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (no new dependencies) | — | The change uses only existing browser/WebKit APIs and existing project utilities |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 5 s threshold causes false-positive on slow rendering | Medium | Medium | Threshold exported and tunable; observed via `reason=raf-stall` log; can lengthen if needed |
| Suspend gap > 30 s but rAF actually dead | Low | Low | Mitigated by next health-tick (10 s later) catching genuine stall |
| WebKitGTK rAF semantics differ from spec | Medium | Medium | Manual repro procedure exercises real desktop scenarios; E2E uses monkey-patch which is platform-agnostic |
| Existing visibility-aware-streaming regression | Low | High | NFR5 mandates running unchanged E2E specs; tests fail loudly if regressed |

## Open Questions

(none — all clarification points were resolved during requirements gathering)

## Success Metrics

- [ ] All FR1〜FR10 implemented and verified by unit tests
- [ ] All NFR1〜NFR5 satisfied (qualitative review at sdd.6-verify)
- [ ] Existing visibility-aware-streaming regression suite passes unchanged
- [ ] `reason=raf-stall` appears in `[DIAG-IDLE]` log when rAF stalls, never otherwise
- [ ] Manual repro per `freeze-repro-rafstall.md` shows no freeze and no `backpressure stalled` log entries
