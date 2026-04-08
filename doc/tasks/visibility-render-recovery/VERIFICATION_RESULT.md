# Verification Result: Visibility-based Render Recovery

## Functional Requirements

### FR1: Visibilitychange Listener - PASS
- `document.addEventListener("visibilitychange", onVisibilityChange)` registered in `pty-handler.ts:666`
- Listener is within the PTY handler setup function, co-located with `rafDegraded` state

### FR2: rAF Recovery on Visible - PASS
- Guards: `document.visibilityState !== "visible"` and `!rafDegraded` (lines 642-643)
- Step 1: `rafDegraded = false; rafScheduled = false` (lines 651-652)
- Step 2: `scheduleProcessing()` called when `pendingChunks.length > 0 || leftoverData !== null` (lines 662-664)
- Step 3: `forceRender()` called unconditionally when state/renderer available (lines 655-659)
- Step 4: `console.warn` with diagnostic info (lines 645-649)

### FR3: Cleanup - PASS
- `PtyHandlerHandle.destroy()` method added to interface (line 172)
- Implementation calls `document.removeEventListener("visibilitychange", onVisibilityChange)` (line 692)
- noop handle includes no-op `destroy` (line 178)

## Non-Functional Requirements

### NFR1 - Latency - PASS (by design)
- Recovery logic is synchronous: flag reset + forceRender + scheduleProcessing
- No async operations or timeouts in the recovery path

### NFR2 - Compatibility - PASS
- Existing degraded mode switch logic (`fromWatchdog` path at line 261) unchanged
- Existing `startRafRecoveryCheck()` remains functional
- `scheduleProcessing()` correctly routes to rAF path when `rafDegraded=false`

### NFR3 - No regression - PASS
- When `rafDegraded=false`, `onVisibilityChange` returns immediately (line 643)
- TypeScript typecheck: PASS
- Unit tests: 2146 pass, 35 fail (all pre-existing SettingsPanel failures, unrelated)

## Edge Cases

### Multiple rapid visibility toggles - PASS (by design)
- Single listener registered (not per-toggle)
- Idempotent: if already `rafDegraded=false`, returns early

### Recovery with no pending chunks (idle terminal) - PASS
- `forceRender()` called regardless of pending data (repaints canvas)
- `scheduleProcessing()` skipped when no pending data (correct: nothing to process)

### Recovery with pending chunks - PASS
- `scheduleProcessing()` called, which now routes to `requestAnimationFrame` (since `rafDegraded=false`)

## Manual Verification Required

- [ ] Desktop lock/unlock cycle test (requires physical desktop environment)

## Summary

| Category | Result |
|----------|--------|
| FR1-FR3 | All PASS |
| NFR1-NFR3 | All PASS |
| TypeScript typecheck | PASS |
| Unit tests | No new failures |
| Edge cases | All PASS (by design) |
