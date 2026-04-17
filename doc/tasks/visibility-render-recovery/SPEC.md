# Feature: Visibility & Focus-Based Recovery

## Overview

When the desktop is locked, suspended, or the eMterm window becomes hidden, two distinct failures have been observed after restoration:

1. **rAF suspension** — WebKitGTK stops delivering `requestAnimationFrame` callbacks (Page Visibility API throttling). After the page becomes visible again, rAF is not automatically re-scheduled because the existing code has already switched to degraded (setTimeout) mode.
2. **WASM memory corruption** — After long idle / system suspend, WASM calls fail with `RuntimeError: Out of bounds memory access`. The existing WASM recovery in `pty-handler.ts` only triggers via `processPendingData`, so if no PTY data arrives after unlock, the terminal stays broken indefinitely. Render, resize, cursor-blink, and `tab:activated` paths swallow the error without triggering recovery.

This feature adds:
- A `visibilitychange` listener to re-kick the rAF-based rendering pipeline.
- A window focus listener (via Tauri `onFocusChanged`) to perform a WASM health check and trigger recovery on focus restoration.
- A shared recovery entry point usable from both listeners and existing error sites.

## Objectives

- Automatically recover from rAF suspension when the page becomes visible again
- Automatically recover from WASM memory corruption when the window regains focus
- Eliminate permanent freeze after desktop lock/unlock, suspend/resume, or long idle
- Consolidate WASM recovery logic so all error paths can trigger recovery

## User Stories

### US1: Recovery after desktop unlock (rAF)
As a user, I want eMterm to resume rendering after I unlock my desktop, so that I don't have to restart the application.

**Acceptance Criteria:**
- [ ] After desktop lock/unlock cycle, terminal rendering resumes automatically
- [ ] No user interaction required to trigger recovery
- [ ] Canvas content is fully re-rendered (no stale/blank frame)

### US2: Recovery after long idle / suspend (WASM)
As a user, I want eMterm to recover from WASM crashes triggered by system suspend, so that mux tabs and the main terminal become usable again after unlock without restart.

**Acceptance Criteria:**
- [ ] After system suspend/resume cycle, WASM-backed rendering works again
- [ ] Recovery fires even when no PTY data arrives post-unlock
- [ ] Mux tab switching, resize, and cursor blink stop producing repeated `Out of bounds memory access` logs

## Technical Requirements

### Functional Requirements

- **FR1: Visibilitychange Listener** — Register a `visibilitychange` event listener on `document` within the PTY handler setup.
- **FR2: rAF Recovery on Visible** — When `document.visibilityState` transitions to `"visible"`:
  1. If `rafDegraded` is `true`, set it to `false`
  2. If there are pending chunks or leftover data, call `scheduleProcessing()` to re-enter the rAF path
  3. Call `forceRender()` to repaint the canvas
  4. Log the recovery event at `console.warn` level
- **FR3: Cleanup** — The `visibilitychange` listener must be removed when the PTY handler is destroyed.
- **FR4: Focus Listener** — Register a Tauri window focus listener (`getCurrentWindow().onFocusChanged` or equivalent `tauri://focus` event). On transition to focused:
  1. Perform a WASM health probe on the active core (a cheap call guarded by try/catch)
  2. If `WebAssembly.RuntimeError` is caught, invoke the shared recovery function (FR5)
  3. Log the health check outcome at `console.warn` level when recovery is triggered
- **FR5: Shared Recovery Entry Point** — Extract the existing WASM recovery logic in `pty-handler.ts` (`processPendingData` catch block, lines around 440–518) into a reusable function. Call sites:
  - Existing `processPendingData` catch path (unchanged behavior)
  - New focus health check (FR4)
  - `canvas-renderer.ts` cursor blink `RuntimeError` branch (currently logs only)
  - `canvas-renderer.ts` render error branch (currently logs only)
  - `resize-handler.ts` resize error branch (currently logs only)
  - `tab:activated` event handler error branch (currently logs only)
- **FR6: Recovery Idempotency** — Recovery must be guarded by `wasmRecoveryInProgress` and `wasmUnrecoverable` flags so concurrent triggers (focus + PTY data + render) do not start parallel recoveries.
- **FR7: Focus Listener Cleanup** — The focus listener must be removed when the PTY handler is destroyed.

### Non-Functional Requirements

- **NFR1 — Latency:** Recovery must complete within a single rAF frame after visibility restoration (< 50ms perceived delay) for the rAF path.
- **NFR2 — Compatibility:** Must not interfere with existing degraded mode logic or the rAF watchdog mechanism.
- **NFR3 — No regression:** Normal rAF-based rendering when the page is continuously visible must remain unchanged.
- **NFR4 — Focus event robustness:** The focus-based recovery must tolerate compositor differences on Linux where `onFocusChanged` may fire late or not at all; the existing `processPendingData` recovery path remains as a safety net.

## Implementation Approach

### Data Flow — rAF path (existing)

```
Desktop unlock
  → visibilitychange event fires (visibilityState = "visible")
  → listener checks rafDegraded flag
  → if degraded: reset flag, scheduleProcessing(), forceRender()
  → rAF pipeline resumes normally
```

### Data Flow — Focus-based WASM recovery (new)

```
Window regains focus (Tauri onFocusChanged(true))
  → health probe: try cheap WASM call on active core
  → catch WebAssembly.RuntimeError
  → invoke sharedRecovery()
      → sharedRecovery first tries recreateWasmCore()
      → on failure, reinitWasm() asynchronously
      → on success, forceRender() and startCursorBlink()
```

### Target Files

- `src/terminal-app/pty-handler.ts` — visibilitychange listener, focus listener, shared recovery extraction
- `src/terminal/canvas-renderer.ts` — route cursor-blink and render `RuntimeError` into shared recovery
- `src/terminal-app/resize-handler.ts` — route resize `RuntimeError` into shared recovery
- `src/terminal-app/index.ts` — route `tab:activated` `RuntimeError` into shared recovery (and `recheckSize` resize error)

### Dependencies

**Internal Dependencies:**
- `pty-handler.ts`: `rafDegraded`, `scheduleProcessing()`, `forceRender()`, `pendingChunks`, `leftoverData`, `wasmRecoveryAttempts`, `wasmRecoveryInProgress`, `wasmUnrecoverable`, `recreateWasmCore()`, `reinitWasm()`
- `canvas-renderer.ts`: `forceRender()`, `startCursorBlink()`, `stopCursorBlink()`
- Tauri API: `getCurrentWindow().onFocusChanged` (or `listen('tauri://focus'|'tauri://blur')`)

## Test Scenarios

### Unit Tests
- [ ] Simulating visibilitychange to "visible" while rafDegraded=true triggers rAF recovery
- [ ] Simulating visibilitychange to "visible" while rafDegraded=false is a no-op
- [ ] Visibilitychange listener is removed on handler cleanup
- [ ] Simulated focus event triggers health probe
- [ ] Health probe throwing `WebAssembly.RuntimeError` invokes shared recovery
- [ ] Health probe succeeding leaves state unchanged
- [ ] Shared recovery called concurrently from two sites only runs once (idempotency)
- [ ] Focus listener is removed on handler cleanup

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (34 specs)
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Multiple rapid visibility toggles (hidden→visible→hidden→visible) do not cause duplicate listeners or state corruption
- [ ] Multiple rapid focus toggles do not cause duplicate recoveries
- [ ] Recovery works correctly when there are no pending chunks (idle terminal)
- [ ] Recovery works correctly when there are pending chunks accumulated during hidden period
- [ ] Focus event firing while `wasmRecoveryInProgress=true` is a no-op
- [ ] Focus event firing while `wasmUnrecoverable=true` is a no-op

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] Desktop lock/unlock cycle no longer causes permanent UI freeze (rAF path)
- [ ] System suspend/resume no longer leaves WASM in a broken state (focus path)
- [ ] Recovery is logged for diagnostics
- [ ] Existing degraded mode and watchdog logic remain functional
- [ ] Existing `processPendingData` WASM recovery path remains functional
- [ ] No regressions in existing E2E tests
