# Feature: Visibility-based Render Recovery

## Overview

When the desktop is locked or the eMterm window becomes hidden, WebKitGTK stops delivering `requestAnimationFrame` callbacks (Page Visibility API throttling). After the page becomes visible again, rAF is not automatically re-scheduled because the existing code has already switched to degraded (setTimeout) mode. This results in a permanently frozen UI despite the process remaining alive.

This feature adds a `visibilitychange` event listener to detect visibility restoration and actively re-kick the rAF-based rendering pipeline.

## Objectives

- Automatically recover from rAF suspension when the page becomes visible again
- Eliminate the permanent UI freeze after desktop unlock / window restore
- Maintain existing degraded mode as a safety net for other rAF delivery failures

## User Stories

### US1: Recovery after desktop unlock
As a user, I want eMterm to resume rendering after I unlock my desktop, so that I don't have to restart the application.

**Acceptance Criteria:**
- [ ] After desktop lock/unlock cycle, terminal rendering resumes automatically
- [ ] No user interaction required to trigger recovery
- [ ] Canvas content is fully re-rendered (no stale/blank frame)

## Technical Requirements

### Functional Requirements

- **FR1: Visibilitychange Listener** - Register a `visibilitychange` event listener on `document` within the PTY handler setup (where `rafDegraded` state is managed).
- **FR2: rAF Recovery on Visible** - When `document.visibilityState` transitions to `"visible"`:
  1. If `rafDegraded` is `true`, set it to `false`
  2. If there are pending chunks or leftover data, call `scheduleProcessing()` to re-enter the rAF path
  3. Call `forceRender()` to repaint the canvas (WebKitGTK may have discarded canvas contents)
  4. Log the recovery event at `console.warn` level for diagnostics
- **FR3: Cleanup** - The `visibilitychange` listener must be removed when the PTY handler is destroyed (to prevent leaks on tab close).

### Non-Functional Requirements

- **NFR1 - Latency:** Recovery must complete within a single rAF frame after visibility restoration (< 50ms perceived delay)
- **NFR2 - Compatibility:** Must not interfere with existing degraded mode logic or the rAF watchdog mechanism
- **NFR3 - No regression:** Normal rAF-based rendering when the page is continuously visible must remain unchanged

## Implementation Approach

### Data Flow

```
Desktop unlock
  → visibilitychange event fires (visibilityState = "visible")
  → listener checks rafDegraded flag
  → if degraded: reset flag, scheduleProcessing(), forceRender()
  → rAF pipeline resumes normally
```

### Target File

- `src/terminal-app/pty-handler.ts` - Add visibilitychange listener alongside existing rAF/degraded mode logic

### Dependencies

**Internal Dependencies:**
- `pty-handler.ts`: `rafDegraded`, `scheduleProcessing()`, `forceRender()`, `pendingChunks`, `leftoverData` (all already exist)
- `canvas-renderer.ts`: `forceRender()` method (already exists)

## Test Scenarios

### Unit Tests
- [ ] Simulating visibilitychange to "visible" while rafDegraded=true triggers recovery
- [ ] Simulating visibilitychange to "visible" while rafDegraded=false is a no-op
- [ ] Listener is removed on handler cleanup

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (34 specs)
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Multiple rapid visibility toggles (hidden→visible→hidden→visible) do not cause duplicate listeners or state corruption
- [ ] Recovery works correctly when there are no pending chunks (idle terminal)
- [ ] Recovery works correctly when there are pending chunks accumulated during hidden period

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] Desktop lock/unlock cycle no longer causes permanent UI freeze
- [ ] Recovery is logged for diagnostics
- [ ] Existing degraded mode and watchdog logic remain functional
- [ ] No regressions in existing E2E tests
