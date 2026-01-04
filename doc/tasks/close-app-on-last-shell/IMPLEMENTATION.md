# Implementation Plan: Auto-close Application on Last Shell Exit

## Overview
Fix the race condition preventing `pty_exit` events from reaching the frontend, which causes the application window to remain open after the last shell session terminates. The fix involves correcting the event filtering logic in `PtyClient.onExit()` to handle events that arrive before `sessionId` is set, and adding comprehensive debug logging for troubleshooting.

## Objectives
- Ensure `pty_exit` events reliably reach the frontend in all scenarios
- Eliminate race condition where events arrive before `sessionId` is set
- Add debug logging at critical stages for observability
- Achieve < 500ms window close latency after shell termination
- Maintain compatibility with future multi-tab functionality

## Prerequisites

### Development Environment
- Bun package manager (for frontend)
- Rust toolchain (for Tauri backend)
- Node.js/npm (for Tauri CLI)

### Dependencies
All dependencies are already present in the project:
- `@tauri-apps/api` v2.0.0 (event system)
- `@tauri-apps/cli` v2.9.6 (build tooling)
- Tauri backend with PTY manager

### Knowledge Requirements
- Understanding of Tauri's event system (emit/listen)
- Familiarity with async race conditions in TypeScript
- Knowledge of PTY lifecycle in the application
- Understanding of event buffering patterns

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend)
- **Framework**: Tauri 2.x
- **Event System**: Tauri's built-in event emitter/listener
- **Key Libraries**:
  - `@tauri-apps/api/core` - `invoke()` for commands
  - `@tauri-apps/api/event` - `listen()` for events
  - `@tauri-apps/api/webviewWindow` - window management

### Design Approach
**Root Cause**: The `onExit()` method filters events using `this.sessionId`, but this value is `null` until `spawn()` returns. If the shell exits very quickly (within milliseconds), the `pty_exit` event may arrive before `sessionId` is set, causing it to be silently ignored.

**Solution**: Modify the event filter logic to allow processing events even when `sessionId` is `null`. Since there's only one PTY session per window in the current implementation, we can safely process all `pty_exit` events. This approach also prepares for future multi-tab support where proper session tracking will be essential.

**Alternative Considered**: Buffer-based approach (store events until `sessionId` is set, then flush). Rejected because it's more complex and the simpler filter change is sufficient for the current single-session model.

### Component Interaction
```
Frontend (main.ts)
    ↓
    Creates PtyClient
    ↓
    Registers onExit callback (with window.close logic)
    ↓
    Registers onTerminalActions callback
    ↓
    Calls spawn()
    ↓
Backend (Rust)
    ↓
    Spawns shell, starts reader thread
    ↓
    (Shell may exit immediately)
    ↓
    Detects exit, emits pty_exit event
    ↓
Frontend (PtyClient)
    ↓
    onExit filter checks sessionId
    ↓
    [BUG] If sessionId=null → event ignored
    ↓
    [FIX] If sessionId=null OR matches → process event
    ↓
    Callback invoked → window closes
```

## Implementation Phases

### Phase 1: Fix Event Filtering Logic

**Goal**: Ensure `pty_exit` events are processed even when `sessionId` has not been set yet

**Files to Modify**:
- `src/pty/client.ts`:
  - Modify `onExit()` method to accept events when `sessionId` is `null`
  - Add console logging for received events

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PtyClient.onExit() | Register exit event listener | None | Listener registered, ready to receive events |
| Event filter logic | Determine if event should be processed | Event received | Event passed to callback OR ignored |
| sessionId check | Validate event belongs to current session | Event received | Boolean result (process or ignore) |

**Processing Flow**:
```
1. pty_exit event arrives at frontend
   ├─ sessionId is null (spawn not returned yet)
   │  └─ [OLD] Ignored (BUG)
   │  └─ [NEW] Process event (FIX)
   │
   ├─ sessionId matches event.payload.session_id
   │  └─ Process event
   │
   └─ sessionId does NOT match
      └─ Ignore event (different session)

2. Process event
   └─ Log event details
   └─ Invoke callback with (code, remaining_sessions)
   └─ Set sessionId to null (mark as closed)
```

**Implementation Steps**:

1. **Modify onExit() event filter condition**
   - Change from: `if (this.sessionId !== null && event.payload.session_id === this.sessionId)`
   - Change to: `if (this.sessionId === null || event.payload.session_id === this.sessionId)`
   - Rationale: Allow processing when sessionId is not yet set (race condition scenario)
   - **IMPORTANT**: Add documentation comment explaining multi-tab limitation:
     ```typescript
     // NOTE: This implementation assumes single-session model (one PTY per window).
     // The condition `sessionId === null` handles the race where shell exits before
     // spawn() returns, but this will NOT work correctly in multi-tab scenarios.
     // FUTURE: When implementing multi-tab support, replace this with event buffering
     // to avoid processing events from unrelated sessions. See SPEC.md NFR4.
     if (this.sessionId === null || event.payload.session_id === this.sessionId) {
       // ...
     }
     ```

2. **Add console logging (development build only)**
   - Log when event is received: `code`, `remaining_sessions`
   - Purpose: Provide visibility into event delivery for troubleshooting
   - Format: `[PtyClient] pty_exit received: code={code}, remaining={remaining}`
   - **IMPORTANT**: Wrap all console.log calls with `import.meta.env.DEV` check to avoid performance overhead in production
   - Example: `if (import.meta.env.DEV) { console.log('[PtyClient] pty_exit received: ...'); }`
   - Rationale: Console logging has negligible overhead in dev mode but should be avoided in production builds

3. **Implement duplicate event prevention**
   - Add `private exitHandled = false;` field to PtyClient class
   - Check `exitHandled` flag at start of event handler (early return if true)
   - Set `exitHandled = true` before invoking callback
   - Call `unlisten()` after callback to deregister the listener
   - Set `this.sessionId = null` after unlisten
   - Rationale: Prevents duplicate processing if same event arrives multiple times
   - Prevents memory leaks by cleaning up event listeners
   - Example implementation:
     ```typescript
     private exitHandled = false;

     async onExit(callback: PtyExitCallback): Promise<void> {
       const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
         // Prevent duplicate processing
         if (this.exitHandled) {
           if (import.meta.env.DEV) {
             console.log('[PtyClient] pty_exit already handled, ignoring duplicate event');
           }
           return;
         }

         // [Filter condition from step 1]

         if (this.sessionId === null || event.payload.session_id === this.sessionId) {
           if (import.meta.env.DEV) {
             console.log(`[PtyClient] pty_exit received: code=${event.payload.code}, remaining=${event.payload.remaining_sessions}`);
           }

           this.exitHandled = true;  // Mark as handled
           callback(event.payload.code, event.payload.remaining_sessions);
           unlisten();  // Cleanup listener
           this.sessionId = null;
         }
       });
       this.unlisteners.push(unlisten);
     }
     ```

**Dependencies**:
- Requires: None (independent change)
- Blocks: Phase 2 (logging builds on this)

**Testing Approach**:

*Unit Tests*:
- Test event processing when `sessionId` is `null` (pre-spawn scenario)
- Test event processing when `sessionId` matches
- Test event ignoring when `sessionId` does not match
- Test duplicate event prevention (emit same event twice, verify callback called only once)
- Test that `unlisten()` is called after event processing
- Mock Tauri's `listen()` to emit test events

*Integration Tests*:
- Spawn session and immediately send exit command
- Verify event is received and callback invoked
- Use real Tauri invoke/listen (no mocking)

*Manual Testing*:
- [ ] Start eMterm, press Ctrl+D immediately, verify window closes
- [ ] Start eMterm, type `exit`, verify window closes
- [ ] Check browser console for `[PtyClient] pty_exit received` log
- [ ] Verify no duplicate event processing

**Acceptance Criteria**:
- [ ] `onExit()` callback is invoked even when `sessionId` is `null`
- [ ] Console log shows `pty_exit received` with correct code and count
- [ ] Window closes when `remaining_sessions === 0`
- [ ] No duplicate event processing occurs
- [ ] Code review confirms logic is correct

**Estimated Effort**: 小 (1-2 hours)

**Risks and Mitigation**:
- **Risk**: Processing events from unrelated sessions (in future multi-tab scenario)
  - **Mitigation**: Current single-session model makes this safe; future multi-tab will require revisiting this logic
- **Risk**: Race condition still occurs in other edge cases
  - **Mitigation**: Comprehensive logging will reveal any remaining issues

---

### Phase 2: Add Debug Logging

**Goal**: Provide comprehensive visibility into event flow for troubleshooting and verification

**Files to Modify**:
- `src/main.ts`:
  - Add logging in `setupNewTerminalHandlers()` onExit callback
  - Log remaining sessions count and window close actions
- `src-tauri/src/lib.rs`:
  - Verify existing backend logs are present (no changes needed)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Frontend onExit callback | Log exit event and window close actions | Event received | Logs written to console |
| Backend PTY reader | Log session exit and event emission | Shell exits | Logs written to stderr |
| Log format | Standardize log messages with prefixes | None | Consistent, greppable logs |

**Processing Flow**:
```
1. Backend detects shell exit
   └─ Log: "PTY reader: session {id} exited with code {code}, {n} sessions remaining"
   └─ Log: "PTY reader: emitting pty_exit event for session {id}"
   └─ Emit pty_exit event
   └─ If emit fails: Log error

2. Frontend PtyClient receives event
   └─ Log: "[PtyClient] pty_exit received: code={code}, remaining={remaining}"
   └─ Invoke callback

3. Frontend main.ts callback executes
   └─ Log: "[Main] onExit callback: code={code}, remainingSessions={remaining}"
   └─ If remaining === 0:
      └─ Log: "[Main] Last session exited, closing window..."
      └─ Attempt window close
      └─ If success: Log: "[Main] Window closed successfully"
      └─ If error: Log error
   └─ Else:
      └─ Log: "[Main] {n} session(s) remaining, keeping window open"
```

**Implementation Steps**:

1. **Add frontend main.ts logging (development build only)**
   - In `setupNewTerminalHandlers()`, inside `onExit()` callback
   - Log at entry: callback invoked with parameters
   - Log before window close: intention to close
   - Log after window close: success or error
   - Log if keeping window open: remaining session count
   - **IMPORTANT**: Wrap all console.log calls with `import.meta.env.DEV` check
   - Example:
     ```typescript
     await ptyClient.onExit(async (code, remainingSessions) => {
       if (import.meta.env.DEV) {
         console.log(`[Main] onExit callback: code=${code}, remainingSessions=${remainingSessions}`);
       }

       if (remainingSessions === 0) {
         if (import.meta.env.DEV) {
           console.log('[Main] Last session exited, closing window...');
         }
         try {
           const appWindow = getCurrentWebviewWindow();
           await appWindow.close();
           if (import.meta.env.DEV) {
             console.log('[Main] Window closed successfully');
           }
         } catch (error) {
           if (import.meta.env.DEV) {
             console.error('[Main] Failed to close window:', error);
           }
         }
       } else {
         if (import.meta.env.DEV) {
           console.log(`[Main] ${remainingSessions} session(s) remaining, keeping window open`);
         }
       }
     });
     ```

2. **Verify backend logging**
   - Check `src-tauri/src/lib.rs` for existing logs
   - According to spec, these should already exist:
     - Session exit detection
     - Event emission
     - Emission errors
   - No code changes needed, just verification

3. **Standardize log format**
   - Use prefixes: `[Main]`, `[PtyClient]`, `PTY reader:`
   - Include relevant data: session ID, exit code, remaining count
   - Use consistent terminology
   - Note: Frontend logs are wrapped in `import.meta.env.DEV` checks (development only)
   - Backend logs remain unconditional (stderr output for production debugging)

**Dependencies**:
- Requires: Phase 1 (builds on modified onExit logic)
- Blocks: Phase 3 (logs are used for testing verification)

**Testing Approach**:

*Manual Testing*:
- [ ] Start eMterm, check console and stderr for initialization logs
- [ ] Press Ctrl+D, verify log sequence:
  1. Backend: "session exited with code 0"
  2. Backend: "emitting pty_exit event"
  3. Frontend: "[PtyClient] pty_exit received: code=0, remaining=0"
  4. Frontend: "[Main] onExit callback: code=0, remainingSessions=0"
  5. Frontend: "[Main] Last session exited, closing window..."
  6. Frontend: "[Main] Window closed successfully"
- [ ] Type `exit`, verify same log sequence
- [ ] Simulate error (modify code to throw in window.close), verify error is logged

**Acceptance Criteria**:
- [ ] All critical stages have corresponding log messages
- [ ] Log messages include relevant context (codes, counts, session IDs)
- [ ] Logs are written to appropriate streams (console for frontend, stderr for backend)
- [ ] Log format is consistent and greppable
- [ ] Errors are logged with full details

**Estimated Effort**: 小 (1 hour)

**Risks and Mitigation**:
- **Risk**: Excessive logging impacts performance
  - **Mitigation**: Frontend logs are wrapped in `import.meta.env.DEV` checks, disabled in production builds
  - **Mitigation**: Backend logs remain for production debugging (stderr output)
- **Risk**: Sensitive data leaked in logs
  - **Mitigation**: Only log session IDs, exit codes, counts; no user data

---

### Phase 3: Testing and Validation

**Goal**: Verify the fix works across all scenarios and meets performance requirements

**Files to Create**:
- `src/pty/client.test.ts` (if not exists) - Unit tests for PtyClient
- Test cases for event filtering edge cases

**Files to Modify**:
- Existing test files to add new test cases

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Unit tests | Verify event filter logic in isolation | Phase 1 complete | Tests pass |
| Integration tests | Verify full spawn-to-exit flow | Phase 1 complete | Tests pass |
| E2E tests | Verify user-facing behavior | Phase 1, 2 complete | Tests pass |
| Performance tests | Measure window close latency | Phase 1, 2 complete | < 500ms confirmed |

**Processing Flow**:
```
Testing workflow:
1. Run unit tests
   └─ Mock Tauri event system
   └─ Test event filtering logic
   └─ Verify callback invocation

2. Run integration tests
   └─ Use real Tauri backend
   └─ Spawn and exit shell
   └─ Verify session cleanup

3. Run E2E tests
   └─ Launch application
   └─ Simulate user actions (Ctrl+D, exit, crash)
   └─ Measure window close timing

4. Manual testing
   └─ Follow test checklist
   └─ Verify across scenarios
   └─ Check logs for correctness
```

**Implementation Steps**:

1. **Create unit tests for PtyClient.onExit()**
   - Test: Event arrives before `sessionId` is set
     - Setup: Create PtyClient, register onExit, emit event (without calling spawn)
     - Assert: Callback invoked with correct parameters
   - Test: Event arrives after `sessionId` is set and matches
     - Setup: Set `sessionId`, emit matching event
     - Assert: Callback invoked
   - Test: Event with non-matching session ID is ignored
     - Setup: Set `sessionId` to "abc", emit event with session_id "xyz"
     - Assert: Callback NOT invoked

2. **Add integration test for immediate exit**
   - Test: Spawn shell that exits immediately (e.g., `bash -c 'exit'`)
   - Verify: `onExit` callback is invoked
   - Verify: `remaining_sessions` count is 0
   - Challenge: May require Rust-side test or mocking

3. **Create E2E tests using e2e-testing skill**
   - Test: Ctrl+D closes window within 500ms
   - Test: `exit` command closes window
   - Test: Shell crash closes window
   - Use `chrome-devtools` MCP tools to automate browser interactions
   - Measure timing from shell exit to window close

4. **Manual testing checklist**
   - Follow test scenarios from SPEC.md (listed in VERIFICATION.md)
   - Verify logs at each step
   - Test on Linux (primary platform)
   - Document any issues found

5. **Performance validation**
   - Measure window close latency (target: < 500ms, 95th percentile)
   - Method: Instrument code with timestamps, calculate delta
   - Record results in test report

**Dependencies**:
- Requires: Phase 1 (fix must be in place)
- Requires: Phase 2 (logs needed for verification)
- Blocks: None (final phase)

**Testing Approach**:

*Unit Tests*:
- Run: `bun test`
- Verify all PtyClient tests pass
- Aim for 90%+ coverage of modified code paths

*Integration Tests*:
- Run: `cargo test --manifest-path src-tauri/Cargo.toml`
- Verify PTY lifecycle tests pass
- May require new Rust tests for immediate exit scenario

*E2E Tests*:
- Run: `/e2e-testing` skill or manual WebDriver tests
- Verify window close behavior end-to-end
- Test on real application build

*Manual Testing*:
- [ ] Start app, press Ctrl+D → window closes < 500ms
- [ ] Start app, type `exit` → window closes < 500ms
- [ ] Start app, type `kill -9 $$` → window closes < 500ms
- [ ] Start app, click × button → window closes immediately
- [ ] Verify console logs show expected sequence
- [ ] Verify no error messages in console or stderr

**Acceptance Criteria**:
- [ ] All unit tests pass (`bun test`)
- [ ] All integration tests pass (`cargo test`)
- [ ] E2E tests pass (Ctrl+D, exit, crash scenarios)
- [ ] Window close latency < 500ms in 95% of manual tests
- [ ] Debug logs confirm correct event flow
- [ ] No regressions in existing functionality
- [ ] Code coverage meets project standards

**Estimated Effort**: 中 (3-4 hours)

**Risks and Mitigation**:
- **Risk**: E2E tests are flaky due to timing issues
  - **Mitigation**: Use generous timeouts, retry logic, focus on manual testing if automation is unreliable
- **Risk**: Cannot achieve < 500ms latency on slow systems
  - **Mitigation**: Measure on reference hardware, document minimum requirements
- **Risk**: Tests require mocking complex Tauri APIs
  - **Mitigation**: Focus on manual and integration tests if unit test mocking is too complex

---

## Complete File Structure

```
emterm/
├── src/
│   ├── main.ts                          # [MODIFY] Add onExit callback logging
│   ├── index.html                       # No changes
│   ├── styles.css                       # No changes
│   ├── pty/
│   │   ├── client.ts                    # [MODIFY] Fix onExit event filter
│   │   ├── client.test.ts               # [CREATE IF NEEDED] Unit tests for PtyClient
│   │   ├── index.ts                     # No changes
│   │   ├── keyboard.ts                  # No changes
│   │   ├── measure.ts                   # No changes
│   │   └── resize.ts                    # No changes
│   ├── terminal/
│   │   └── ...                          # No changes
│   └── types/
│       ├── pty.ts                       # No changes (defines PtyExitPayload)
│       └── terminal.ts                  # No changes
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs                       # [VERIFY ONLY] Check existing logs
│   │   └── pty/
│   │       ├── manager.rs               # No changes
│   │       └── graceful_shutdown.rs     # No changes (already correct)
│   └── Cargo.toml                       # No changes
├── doc/
│   └── tasks/
│       └── close-app-on-last-shell/
│           ├── SPEC.md                  # Reference document
│           ├── IMPLEMENTATION.md        # This document
│           └── VERIFICATION.md          # Companion verification document
├── package.json                         # No changes
└── README.md                            # No changes

[MODIFY] = File requires code changes
[VERIFY ONLY] = Check existing code, no changes needed
[CREATE IF NEEDED] = Create only if not already present
```

**File Descriptions**:
- `src/pty/client.ts` - PtyClient class managing PTY communication; contains the buggy `onExit()` filter
- `src/main.ts` - Application entry point; sets up event handlers and spawns PTY
- `src-tauri/src/lib.rs` - Tauri command handlers; emits `pty_exit` events
- `src/types/pty.ts` - TypeScript type definitions for PTY events and payloads
- `doc/tasks/close-app-on-last-shell/SPEC.md` - Feature specification (source of truth)
- `doc/tasks/close-app-on-last-shell/VERIFICATION.md` - Verification checklist and test cases

## Testing Strategy

### Unit Testing

**Approach**:
- Use Bun's built-in test runner (`bun test`)
- Mock Tauri APIs (`invoke`, `listen`) using test doubles
- Focus on PtyClient event filtering logic in isolation
- Table-driven tests for multiple scenarios

**Test Coverage Goals**:
- PtyClient.onExit() logic: 100% (critical path)
- Event filtering conditions: All branches covered
- Overall PtyClient class: 80%+

**Key Test Areas**:

1. **PtyClient.onExit() Event Filtering** (`src/pty/client.test.ts`)
   - Event arrives when `sessionId` is `null` → callback invoked
   - Event arrives when `sessionId` matches event session → callback invoked
   - Event arrives when `sessionId` does NOT match → callback NOT invoked
   - After callback, `sessionId` is set to `null`
   - Callback receives correct parameters (code, remaining_sessions)

2. **Edge Cases**:
   - Multiple events with same session ID → only first is processed
   - Events arriving in rapid succession
   - Event with invalid payload structure → handled gracefully (or throws)

3. **Logging**:
   - Verify console.log is called with expected format
   - Verify log includes code and remaining_sessions values

**Example Test Structure**:
```typescript
// Pseudocode - not actual implementation
describe('PtyClient.onExit()', () => {
  test('processes event when sessionId is null', async () => {
    // Given: PtyClient with null sessionId
    const client = new PtyClient();
    let callbackInvoked = false;
    await client.onExit((code, remaining) => {
      callbackInvoked = true;
    });

    // When: pty_exit event is emitted
    emitMockEvent('pty_exit', { session_id: 'test', code: 0, remaining_sessions: 0 });

    // Then: callback should be invoked
    expect(callbackInvoked).toBe(true);
  });

  test('processes event when sessionId matches', async () => {
    // Setup and assertions...
  });

  test('ignores event when sessionId does not match', async () => {
    // Setup and assertions...
  });
});
```

### Integration Testing

**Scenarios**:
1. Full spawn → exit flow with logging
   - Start PTY, send `exit\n`, verify callback invoked
   - Check logs for complete sequence
2. Immediate exit scenario (shell exits before spawn returns)
   - Use shell command like `bash -c 'exit'`
   - Verify event is still processed
3. Window close during shell execution
   - Simulate `beforeunload` event
   - Verify cleanup executes

**Approach**:
- May require Rust integration tests (`cargo test`)
- Use real Tauri backend (no mocking)
- Test PTY manager and event emission
- Verify end-to-end flow without UI

**Rust Integration Test Example**:
```rust
// Pseudocode - shows testing approach
#[tokio::test]
async fn test_immediate_shell_exit() {
    // Given: PTY manager
    let manager = PtyManager::new();

    // When: Spawn shell that exits immediately
    let result = manager.create_session_atomic(Some("bash -c 'exit'"), 80, 24).await;

    // Then: Session should be removed after exit
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(manager.session_count().await, 0);
}
```

### E2E Tests

**Framework**: Use `e2e-testing` skill with `chrome-devtools` MCP tools, or WebDriver/Playwright

**Test Cases**:
1. **Ctrl+D closes window**
   - Launch eMterm
   - Wait for terminal ready
   - Send Ctrl+D key event
   - Verify window closes within 500ms
   - Verify application process terminates

2. **Exit command closes window**
   - Launch eMterm
   - Type `exit` and press Enter
   - Verify window closes within 500ms

3. **Shell crash closes window**
   - Launch eMterm
   - Type `kill -9 $$` (kill shell)
   - Verify window closes within 500ms

4. **Manual close works**
   - Launch eMterm
   - Click × button
   - Verify window closes immediately
   - Verify PTY cleanup occurs

**E2E Test Execution**:
- Run: `/e2e-testing` skill or custom test script
- Requires: Development build running (`bun tauri:dev`)
- Environment: Linux (primary), macOS/Windows (if available)

### Manual Testing Checklist

Based on SPEC.md test scenarios:

**Basic Functionality**:
- [ ] Start eMterm, press Ctrl+D → window closes
- [ ] Start eMterm, type `exit` → window closes
- [ ] Start eMterm, type `kill -9 $$` → window closes (crash scenario)
- [ ] Start eMterm, click × button → window closes immediately

**Timing and Performance**:
- [ ] Measure window close latency with stopwatch
  - Ctrl+D: ___ms (target: < 500ms)
  - exit: ___ms (target: < 500ms)
  - crash: ___ms (target: < 500ms)

**Logging Verification**:
- [ ] Open browser console (Ctrl+Shift+I in Tauri dev mode)
- [ ] Press Ctrl+D, verify logs appear:
  - `[PtyClient] pty_exit received: code=0, remaining=0`
  - `[Main] onExit callback: code=0, remainingSessions=0`
  - `[Main] Last session exited, closing window...`
  - `[Main] Window closed successfully`
- [ ] Check terminal stderr for backend logs:
  - `PTY reader: session {id} exited with code 0, 0 sessions remaining`
  - `PTY reader: emitting pty_exit event for session {id}`

**Edge Cases**:
- [ ] Spawn shell with immediate exit: `bash -c 'exit'`
  - Verify window closes (tests race condition fix)
- [ ] Rapid spawn/exit cycles (spawn, exit, spawn, exit)
  - Verify each session is cleaned up properly
- [ ] Window close during shell execution
  - Type long-running command (e.g., `sleep 60`)
  - Click × button while running
  - Verify shell is killed and window closes

**Error Handling**:
- [ ] Simulate window.close() error (requires code modification for testing)
  - Modify `appWindow.close()` to throw
  - Verify error is logged: `[Main] Failed to close window: ...`
  - Verify window remains open (expected fallback behavior)

### Performance Tests

**Metric 1: Window Close Latency**
- Measurement: Time from shell exit to window close
- Target: < 500ms (95th percentile)
- Method:
  - Instrument code with `performance.now()` timestamps
  - Record timestamp when `pty_exit` event is emitted (backend)
  - Record timestamp when `appWindow.close()` is called (frontend)
  - Calculate delta
- Tools: Manual timing with logs, or automated with test harness

**Metric 2: Event Delivery Latency**
- Measurement: Time from backend emit to frontend callback
- Target: < 100ms
- Method: Timestamps in backend (emit) and frontend (callback entry)
- Purpose: Verify Tauri event system overhead is negligible

**Performance Test Execution**:
- Run 10 trials of each scenario (Ctrl+D, exit, crash)
- Record latencies
- Calculate mean, median, 95th percentile
- Document results in test report

## Dependencies

### External Dependencies

All dependencies are already present in `package.json` and `Cargo.toml`:

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| @tauri-apps/api | ^2.0.0 | Event system, window API | `bun install` |
| @tauri-apps/cli | ^2.9.6 | Build tooling | `bun install` |
| Bun | latest | Package manager, test runner | System package |
| Rust | 1.70+ | Tauri backend | System package |

No new dependencies are required for this fix.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Fix Event Filtering (no dependencies)
2. Phase 2: Add Debug Logging (depends on Phase 1)
3. Phase 3: Testing (depends on Phase 1 and 2)

**Component Dependencies**:
- `src/main.ts` depends on `src/pty/client.ts` (PtyClient class)
- `src/pty/client.ts` depends on Tauri API (`invoke`, `listen`)
- `src/pty/client.ts` depends on `src/types/pty.ts` (type definitions)
- Frontend depends on backend emitting `pty_exit` events correctly (already implemented)

**No circular dependencies exist.**

## Risk Assessment

### Technical Risks

1. **Race Condition Still Occurs in Edge Cases**
   - **Risk**: The fix may not cover all timing scenarios
   - **Likelihood**: Low (logic change is comprehensive)
   - **Impact**: High (window remains open, poor UX)
   - **Mitigation**:
     - Comprehensive testing across scenarios
     - Debug logging will reveal any missed cases
     - Fallback: Users can manually close window (current behavior)

2. **Performance Degradation from Logging**
   - **Risk**: Excessive console logging impacts performance
   - **Likelihood**: None (eliminated by design)
   - **Impact**: None in production
   - **Mitigation**:
     - Frontend logs are wrapped in `import.meta.env.DEV` checks
     - Production builds have zero logging overhead (logs are tree-shaken out)
     - Backend logs remain for production debugging (stderr, minimal overhead)

3. **Breaks Multi-Tab Future Feature**
   - **Risk**: Accepting events when `sessionId` is `null` may cause issues in multi-tab scenario
   - **Likelihood**: Medium (future feature)
   - **Impact**: Medium (requires refactoring when multi-tab is implemented)
   - **Mitigation**:
     - Document this limitation in code comments
     - SPEC.md explicitly acknowledges this tradeoff
     - Multi-tab implementation will require revisiting this logic

### Implementation Risks

1. **Insufficient Testing Coverage**
   - **Risk**: Edge cases not covered by tests
   - **Likelihood**: Medium
   - **Impact**: Medium (bugs in production)
   - **Mitigation**:
     - Follow comprehensive test checklist
     - Manual testing supplements automated tests
     - Staged rollout (test on dev machine first)

2. **Tauri Event System Behavior Changes**
   - **Risk**: Tauri 2.x event system has undocumented edge cases
   - **Likelihood**: Low
   - **Impact**: High (fix doesn't work)
   - **Mitigation**:
     - Rely on documented Tauri APIs
     - Debug logging provides visibility
     - Community support available for Tauri issues

3. **Cross-Platform Differences**
   - **Risk**: Fix works on Linux but not macOS/Windows
   - **Likelihood**: Low (event system is cross-platform)
   - **Impact**: Medium (incomplete fix)
   - **Mitigation**:
     - Primary development on Linux (available)
     - Manual testing on macOS/Windows if available
     - Document platform-specific issues if found

## Performance Considerations

### Window Close Latency

**Target**: < 500ms from shell exit to window close (95th percentile)

**Factors Affecting Latency**:
1. Shell exit detection (backend) - typically < 10ms
2. Event emission (Tauri backend) - typically < 5ms
3. Event delivery (Tauri IPC) - typically < 50ms
4. Frontend callback execution - typically < 5ms
5. Window close operation - typically < 100ms
6. **Total expected**: ~170ms (well under 500ms target)

**Optimization**:
- No optimization needed; current architecture is efficient
- Logging adds negligible overhead (< 1ms)

### Event Processing Overhead

**Current State**:
- Each `pty_exit` event triggers: filter check, callback invocation, logging
- Overhead per event: < 1ms CPU time
- Frequency: Once per session exit (very low)

**No optimization needed** - overhead is negligible.

### Memory Considerations

- No event buffering in Phase 1 fix (alternative approach rejected)
- Logging strings are short-lived (garbage collected immediately)
- No memory leaks introduced
- Memory impact: negligible

## Security Considerations

**This feature does not introduce new security risks.**

The fix only modifies event filtering logic and adds logging. No user input is processed, no external data is fetched, and no security boundaries are crossed.

**Relevant Security Aspects**:
1. **Input Validation**: Not applicable (no user input in this feature)
2. **Authentication/Authorization**: Not applicable (local application)
3. **Data Protection**: Logs do not contain sensitive data (only session IDs, exit codes, counts)
4. **XSS Prevention**: Not applicable (no web content rendering)
5. **SQL Injection**: Not applicable (no database)
6. **CSRF**: Not applicable (no web forms)

**Logging Security**:
- Session IDs are UUIDs (not sensitive)
- Exit codes are integers (not sensitive)
- No user data, file paths, or command contents are logged

## Open Questions

### From Specification

1. **Should we add a user preference to disable auto-close?**
   - **Current Answer**: No (spec decision: fixed behavior)
   - **Rationale**: Simplifies UX, matches standard terminal behavior
   - **Action**: None required

2. **How should we handle multiple windows in the future?**
   - **Current Answer**: Deferred to multi-window feature
   - **Impact on This Fix**: Current fix assumes single window; multi-window will require refactoring
   - **Action**: Document limitation in code comments

3. **Should we add a timeout for window close operation?**
   - **Current Answer**: To be determined during implementation
   - **Proposal**: No timeout initially; if `appWindow.close()` hangs, we'll add 5s timeout
   - **Action**: Test during Phase 3, add timeout if needed

### Implementation-Specific Questions

1. **Should we keep `sessionId = null` assignment after callback?**
   - **Answer**: Yes
   - **Rationale**: Prevents duplicate processing if same event arrives twice (unlikely but defensive)

2. **Should we log to stderr instead of console.log?**
   - **Answer**: Use console.log for frontend (visible in dev tools), stderr for backend (already done)
   - **Rationale**: Frontend logs are for developers debugging; stderr is for production logs

3. **Should we add metrics/telemetry for window close latency?**
   - **Answer**: Out of scope for this fix
   - **Rationale**: Can be added later as separate observability feature

## Future Enhancements

Items deferred to later features or releases:

### Multi-Tab Support
- When multiple PTY sessions exist in tabs, only close window when ALL tabs are closed
- Requires: Tab management UI, session-to-tab mapping
- Impact on this fix: Will need to revert `sessionId === null` condition, implement proper session tracking

### User Preference for Auto-Close
- Settings UI to toggle auto-close behavior
- Per-session or global preference
- Requires: Settings storage, UI for preference

### Advanced Logging
- Structured logging (JSON format)
- Log levels (debug, info, warn, error)
- Log filtering and searching
- Telemetry for performance monitoring

### Graceful Shutdown Improvements
- Show "Closing..." message during shutdown
- Timeout for window close (if it hangs)
- Cancel shutdown option (confirmation dialog)

**Not in Current Spec**: These are hypothetical enhancements, not planned features.

## Success Metrics

### Functional Completeness
- [ ] Phase 1 implementation complete (event filter fixed)
- [ ] Phase 2 implementation complete (logging added)
- [ ] Phase 3 implementation complete (tests pass)
- [ ] All test scenarios pass (unit, integration, E2E)
- [ ] Manual testing checklist completed
- [ ] No regressions in existing functionality

### Quality Metrics
- [ ] Code review completed and approved
- [ ] Unit test coverage ≥ 80% for modified code
- [ ] All tests pass (`bun test`, `cargo test`)
- [ ] No TypeScript type errors (`bun run typecheck`)
- [ ] Code follows project conventions (formatting, style)

### Performance Metrics
- [ ] Window close latency < 500ms in 95% of tests
- [ ] Event delivery latency < 100ms
- [ ] No performance degradation in general terminal use

### User Experience
- [ ] Window closes automatically on Ctrl+D (verified manually)
- [ ] Window closes automatically on `exit` command (verified manually)
- [ ] Window closes automatically on shell crash (verified manually)
- [ ] Debug logs provide clear visibility into process
- [ ] No error messages in console during normal operation

### Documentation
- [ ] Code comments explain fix rationale
- [ ] SPEC.md success criteria checked off
- [ ] IMPLEMENTATION.md marked as complete
- [ ] VERIFICATION.md updated with test results

## References

- **Specification**: `doc/tasks/close-app-on-last-shell/SPEC.md`
- **Requirements Document**: `doc/tasks/close-app-on-last-shell/要件定義書.md`
- **Verification Checklist**: `doc/tasks/close-app-on-last-shell/VERIFICATION.md`
- **Tauri Events API**: https://tauri.app/v2/reference/javascript/api/core/#emitter
- **Tauri WebviewWindow API**: https://tauri.app/v2/reference/javascript/api/webviewwindow/
- **Bun Test Runner**: https://bun.sh/docs/test/

**Related Code**:
- `src-tauri/src/lib.rs` - PTY command handlers and event emission
- `src-tauri/src/pty/manager.rs` - Session management
- `src-tauri/src/pty/graceful_shutdown.rs` - Graceful shutdown logic (already correct)
- `src/main.ts` - Application initialization and event handling
- `src/pty/client.ts` - PTY client interface (contains bug)
- `src/types/pty.ts` - Type definitions for PTY events

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - Review this document for correctness and completeness
   - Confirm approach addresses the root cause
   - Approve to proceed with implementation

2. **Begin Implementation**
   - Start with Phase 1 (fix event filtering)
   - Follow implementation steps in order
   - Commit changes incrementally

3. **Testing**
   - Run unit tests after Phase 1
   - Add debug logging in Phase 2
   - Execute full test suite in Phase 3

4. **Verification**
   - Follow VERIFICATION.md checklist
   - Document test results
   - Confirm all success criteria met

5. **Documentation**
   - Update SPEC.md with implementation status
   - Mark VERIFICATION.md items as complete
   - Add code comments explaining fix

6. **Deployment**
   - Merge to main branch
   - Build release if applicable
   - Monitor for issues

**Ready to implement?** Use `/sdd.3-verify-plan` to verify plan consistency, then `/sdd.4-implement` to begin implementation.
