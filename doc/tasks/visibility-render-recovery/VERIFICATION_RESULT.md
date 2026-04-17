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

---

# Verification Result: Focus-Based WASM Recovery (scope extension)

Verified against SPEC.md revision that added FR4-FR7 and NFR4 (Visibility & Focus-Based Recovery).

## Functional Requirements

### FR4: Focus Listener - PASS
- Async IIFE registers `onFocusChanged` via `getCurrentWebviewWindow()` at `src/terminal-app/pty-handler.ts:609-636`
- Guard on focused payload (`if (!focused) return;`) at line 613
- Idempotency guard `if (wasmRecoveryInProgress || wasmUnrecoverable) return;` at line 614
- Read-only WASM probe: `core.cols()` at line 621 (zero-side-effect TerminalCore accessor)
- On thrown error, logs `[WARN][FRONTEND] focus health probe failed — invoking WASM recovery` and calls `tryRecoverFromWasmCrash(error)` at lines 622-625
- Registration failure path logs `[WARN][FRONTEND] failed to register focus listener:` at line 634

### FR5: Shared Recovery Entry Point - PASS
- `tryRecoverFromWasmCrash(error: unknown): boolean` defined at `src/terminal-app/pty-handler.ts:258-352`
- Classification of WASM errors (RuntimeError / recursive-use / WASM-not-initialized) at lines 263-267
- Attempt counting + 60s window + stopCursorBlink + recreateWasmCore → reinitWasm fallback preserved (lines 274-345)
- Exposed on `PtyHandlerHandle` at line 185; assigned at line 661; noopHandle stub at line 193
- Call sites wired:
  - `processPendingData` catch at line 565
  - Focus health probe at line 624
  - Canvas renderer render/renderImmediate error paths via `wasmRecoveryCallback` (see FR5 call sites below)
  - Canvas renderer cursor-blink catch
  - Resize handler catch at `src/terminal-app/resize-handler.ts:82`
  - `TerminalApp.recheckSize` catch in `src/terminal-app/index.ts`
  - `tab:activated` handler catch at `src/main.ts:421`
- Renderer integration: `CanvasRenderer.setWasmRecoveryCallback` at `src/terminal/canvas-renderer.ts:467`; wired from `TerminalApp` at `src/terminal-app/index.ts:619-620`

### FR6: Recovery Idempotency - PASS
- Flags `wasmRecoveryInProgress` (line 243) and `wasmUnrecoverable` (line 244) gate entry at line 271 and 614 (focus listener)
- `processPendingData` also gates on both flags at line 359
- Async `reinitWasm()` branch sets `wasmRecoveryInProgress = true` on entry (line 322), clears in `finally` (line 338)
- Unit test `pty-handler.test.ts` covers concurrent calls during in-flight recovery (second call is no-op and returns true)
- Unit test covers the 3-attempt exhaustion path marking `wasmUnrecoverable=true` and subsequent calls being no-ops

### FR7: Focus Listener Cleanup - PASS
- `focusListenerDisposed` flag at line 608 guards late-resolving registration
- `destroy` sets `focusListenerDisposed = true` and calls `unlistenFocus()` at lines 664-668
- Race: if `destroy()` runs before the async registration resolves, the completion handler at line 627 calls `unlisten()` immediately instead of storing it
- Unit test `pty-handler.test.ts` verifies destroy invokes unlisten and that late-resolving registration is also cleaned up

## Non-Functional Requirements

### NFR4 - Focus Event Robustness - PASS (by design)
- `processPendingData` catch block retains the shared recovery call at line 565, so PTY data arrival also triggers recovery as a safety net on compositors where `onFocusChanged` is unreliable
- Registration failure is caught and logged (line 633-635) — the app continues to function, relying on the PTY-data safety net

## Coverage of Error Sites Listed in SPEC FR5

| Site | File:Line | Routed |
|------|-----------|--------|
| processPendingData catch | `pty-handler.ts:565` | ✓ |
| Render failed (rAF) | `canvas-renderer.ts` via `wasmRecoveryCallback` | ✓ |
| Render failed (sync) | `canvas-renderer.ts` via `wasmRecoveryCallback` | ✓ |
| cursor blink skipped | `canvas-renderer.ts` (cursor-blink catch) | ✓ |
| Failed to resize terminal | `resize-handler.ts:82` | ✓ |
| Failed to resize in recheckSize | `index.ts` recheckSize catch | ✓ |
| tab:activated handler | `main.ts:421` | ✓ |

## Unit Test Results

- `src/terminal-app/pty-handler.test.ts` — 11 tests covering classification, idempotency, unrecoverable gate, non-throwing contract, focus listener registration / invocation / blur-no-op / destroy-unlisten, noop handle behavior
- `src/terminal/canvas-renderer-recovery.test.ts` — 4 tests covering callback invocation from `renderImmediate`, cursor-blink path, and null clearing
- Full suite: **2285 pass, 0 fail, 17 todo** (reported by implement phase)
- TypeScript typecheck: clean

## Manual Verification Required

The following acceptance criteria require real hardware and cannot be fully automated — user must verify:

- [ ] US2 AC1: After system suspend/resume cycle, WASM-backed rendering works again without restart
- [ ] US2 AC2: Recovery fires even when no PTY data arrives post-unlock (verify by unlocking without touching any terminal and confirming no cursor-blink-skipped flood in `~/.local/share/net.laser5.app.emterm/logs/emterm.log`)
- [ ] US2 AC3: Mux tab switching, resize, and cursor blink stop producing repeated `Out of bounds memory access` logs
- [ ] Expected log markers on successful recovery:
  - `[WARN][FRONTEND] focus health probe failed — invoking WASM recovery`
  - `[WARN][FRONTEND] WASM crash detected — attempting recovery (N/3)`
  - `[WARN][FRONTEND] WASM module reinitialized — terminal recovered` (if reinitWasm path ran)

## Scope Extension Summary

| Category | Result |
|----------|--------|
| FR4-FR7 | All PASS (code review + unit tests) |
| NFR4 | PASS (by design) |
| FR5 error-site routing (7 sites) | All PASS |
| Unit tests (focus + renderer) | 15 new tests, all pass |
| Full suite regression | 2285 pass, 0 fail |
| Manual (lock/unlock/suspend) | Pending user verification |
