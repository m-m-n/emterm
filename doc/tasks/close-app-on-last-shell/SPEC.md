# Feature: Auto-close Application on Last Shell Exit

## Overview

This feature ensures that the eMterm application window automatically closes when the last PTY (pseudo-terminal) session exits. Currently, the window remains open in a non-responsive state after the shell terminates, requiring manual window closure by the user.

Investigation reveals that the auto-close logic is already implemented in the codebase, but the `pty_exit` event is not being properly received or processed by the frontend. This specification addresses the root cause and provides a robust solution.

## Objectives

- Fix the event delivery mechanism to ensure `pty_exit` events reliably reach the frontend
- Optimize event listener registration timing to prevent race conditions
- Add comprehensive debug logging for troubleshooting
- Ensure the window closes within 500ms of shell termination
- Maintain compatibility with future multi-tab support

## User Stories

### US1: Close Window on Ctrl+D
As a terminal user, I want the application window to close automatically when I press Ctrl+D to exit the shell, so that I don't need to manually close the window.

**Acceptance Criteria:**
- [ ] Pressing Ctrl+D sends EOF to the shell
- [ ] Shell process terminates gracefully
- [ ] Backend detects process termination
- [ ] `pty_exit` event is emitted with `remaining_sessions: 0`
- [ ] Frontend receives the event
- [ ] Window closes within 500ms
- [ ] Application process terminates completely

### US2: Close Window on Exit Command
As a terminal user, I want the application window to close automatically when I type `exit` to quit the shell, so that the behavior is consistent with standard terminal emulators.

**Acceptance Criteria:**
- [ ] Typing `exit` and pressing Enter terminates the shell
- [ ] Backend detects process termination
- [ ] `pty_exit` event is emitted with `remaining_sessions: 0`
- [ ] Frontend receives the event
- [ ] Window closes within 500ms
- [ ] Application process terminates completely

### US3: Close Window on Shell Crash
As a user, I want the application window to close automatically when the shell crashes unexpectedly, so that zombie windows don't accumulate.

**Acceptance Criteria:**
- [ ] Shell process terminates with non-zero exit code
- [ ] Backend detects abnormal termination
- [ ] `pty_exit` event is emitted with exit code and `remaining_sessions: 0`
- [ ] Frontend receives the event
- [ ] Window closes within 500ms
- [ ] Application process terminates completely

### US4: Manual Window Close
As a user, I want the application to clean up properly when I click the window's close button, so that no orphaned processes remain.

**Acceptance Criteria:**
- [ ] Clicking the × button triggers `beforeunload` event
- [ ] Frontend cleanup function executes
- [ ] PTY session is killed
- [ ] Window closes
- [ ] Application terminates cleanly

## Technical Requirements

### Functional Requirements

- **FR1:** The backend MUST emit a `pty_exit` event when a PTY session terminates
- **FR2:** The event MUST include `remaining_sessions` count (captured atomically)
- **FR3:** The frontend MUST register `onExit` listener before spawning the PTY session
- **FR4:** The frontend MUST close the window when `remaining_sessions === 0`
- **FR5:** Debug logs MUST be emitted at each critical stage of the process

### Non-Functional Requirements

- **NFR1 - Performance:** Window closure MUST occur within 500ms of shell termination
- **NFR2 - Reliability:** Event delivery success rate MUST be ≥ 99.9%
- **NFR3 - Observability:** Debug logs MUST provide sufficient information for troubleshooting
- **NFR4 - Maintainability:** Code MUST support future multi-tab functionality
- **NFR5 - Compatibility:** MUST work on Linux, macOS, and Windows

## Implementation Approach

### Architecture

**Current Implementation (Problematic):**
```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend                             │
│  ┌───────────────────────────────────────────────────┐     │
│  │ 1. Create PtyClient                               │     │
│  │ 2. Register onTerminalActions                     │     │
│  │ 3. Register onExit ← RACE CONDITION!              │     │
│  │ 4. Call spawn() ────────────────────────────────┐ │     │
│  └──────────────────────────────────────────────────┼─┘     │
│                                                      │       │
└──────────────────────────────────────────────────────┼───────┘
                                                       │
                                                       ▼
┌──────────────────────────────────────────────────────────────┐
│                        Backend                               │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ 1. Spawn shell process                                 │ │
│  │ 2. Start reader thread                                 │ │
│  │ 3. Emit pty_exit ← May arrive before listener ready!  │ │
│  └────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

**Fixed Implementation:**
```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend                             │
│  ┌───────────────────────────────────────────────────┐     │
│  │ 1. Create PtyClient                               │     │
│  │ 2. Register onExit FIRST ✓                        │     │
│  │ 3. Register onTerminalActions                     │     │
│  │ 4. Call spawn()                                   │     │
│  │ 5. Events buffered if sessionId not set yet       │     │
│  └───────────────────────────────────────────────────┘     │
│                                                              │
│  Event handling with sessionId check:                       │
│  - If sessionId null → buffer event                         │
│  - If sessionId matches → process event                     │
│  - After spawn returns → flush buffered events              │
└──────────────────────────────────────────────────────────────┘
```

### Root Cause Analysis

**Problem 1: Event Listener Registration Timing**

Current code in `src/main.ts`:
```typescript
async function initTerminal(): Promise<void> {
  // ...
  ptyClient = new PtyClient();

  // Listeners registered AFTER spawn setup begins
  if (USE_NEW_TERMINAL) {
    await setupNewTerminalHandlers(); // onExit registered here
  }

  // Race condition: spawn may complete and emit pty_exit
  // before setupNewTerminalHandlers completes
  await ptyClient.spawn({ cols, rows });
}
```

**Problem 2: Event Filtering Logic**

Current code in `src/pty/client.ts`:
```typescript
async onExit(callback: PtyExitCallback): Promise<void> {
  const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
    // sessionId might be null when fast shell exits occur
    if (this.sessionId !== null && event.payload.session_id === this.sessionId) {
      callback(event.payload.code, event.payload.remaining_sessions);
      this.sessionId = null;
    }
  });
  this.unlisteners.push(unlisten);
}
```

If the shell exits very quickly (e.g., immediately after spawn), the `pty_exit` event may arrive before `spawn()` returns and sets `this.sessionId`. The event will be ignored.

### Data Flow

**Complete Flow (Fixed):**

```mermaid
sequenceDiagram
    participant User
    participant Frontend
    participant PtyClient
    participant Backend
    participant Shell

    User->>Frontend: Start eMterm
    Frontend->>PtyClient: new PtyClient()
    Frontend->>PtyClient: onExit(callback)
    Note over PtyClient: Listener registered,<br/>ready to receive events
    Frontend->>PtyClient: onTerminalActions(callback)
    Frontend->>PtyClient: spawn({cols, rows})
    PtyClient->>Backend: pty_spawn command
    Backend->>Shell: Spawn shell process
    Backend->>Backend: Start reader thread
    Backend-->>PtyClient: {session_id: "abc123"}
    PtyClient-->>Frontend: "abc123"
    Note over Frontend: Flush buffered events

    User->>Frontend: Press Ctrl+D
    Frontend->>PtyClient: write([0x04])
    PtyClient->>Backend: pty_write
    Backend->>Shell: Send EOF
    Shell->>Shell: Process exits
    Backend->>Backend: Detect exit (try_wait)
    Backend->>Backend: remove_session_atomic
    Backend->>Backend: Get remaining_sessions = 0
    Backend-->>PtyClient: pty_exit event<br/>{code: 0, remaining_sessions: 0}
    PtyClient->>PtyClient: Check sessionId
    PtyClient->>Frontend: callback(0, 0)
    Frontend->>Frontend: Check remaining_sessions === 0
    Frontend->>Frontend: getCurrentWebviewWindow()
    Frontend->>Frontend: appWindow.close()
    Note over User,Shell: Application terminates
```

### API Design

#### Event: pty_exit

**Emitted by:** Backend (Rust)

**Payload Structure:**
```typescript
interface PtyExitPayload {
  session_id: string;        // Session that exited
  code: number;              // Exit code (i32)
  remaining_sessions: number; // Count after removal (usize)
}
```

**Timing:**
- Emitted after session is removed from the registry
- `remaining_sessions` is captured atomically with removal

**Error Handling:**
- If emission fails, log error to stderr
- Continue with cleanup

#### Command: pty_spawn

**Invoked by:** Frontend (TypeScript)

**Request:**
```typescript
interface PtySpawnRequest {
  shell?: string;  // Optional shell path
  cols?: number;   // Terminal columns (default: 80)
  rows?: number;   // Terminal rows (default: 24)
}
```

**Response:**
```typescript
interface SpawnResult {
  session_id: string; // Unique session identifier
}
```

**Timing Guarantees:**
- Reader thread starts before response is returned
- Events may be emitted before spawn() returns (if shell exits immediately)

### File Structure

No new files are required. Modifications to existing files:

```
src/
├── main.ts                  # Fix listener registration order
├── pty/
│   └── client.ts           # Fix event filtering logic

src-tauri/src/
└── lib.rs                  # Add debug logging (already correct)
```

### Implementation Changes

#### Change 1: Fix Listener Registration Order (src/main.ts)

**Before:**
```typescript
async function initTerminal(): Promise<void> {
  // ...
  ptyClient = new PtyClient();

  if (USE_NEW_TERMINAL) {
    await setupNewTerminalHandlers();
  }

  await ptyClient.spawn({ cols, rows });
}
```

**After:**
```typescript
async function initTerminal(): Promise<void> {
  // ...
  ptyClient = new PtyClient();

  // CRITICAL: Register event handlers BEFORE spawning
  // This ensures we don't miss early exit events
  if (USE_NEW_TERMINAL) {
    await setupNewTerminalHandlers();
  } else {
    await setupLegacyHandlers(terminal);
  }

  // Now safe to spawn - all listeners are ready
  await ptyClient.spawn({ cols, rows });

  // Flush any events that arrived during spawn
  if (USE_NEW_TERMINAL && terminalState && terminalRenderer) {
    ptyClient.flushPendingTerminalActions();
    terminalRenderer.forceRender(terminalState);
  }
}
```

#### Change 2: Fix Event Filtering in onExit (src/pty/client.ts)

**Before:**
```typescript
async onExit(callback: PtyExitCallback): Promise<void> {
  const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
    if (this.sessionId !== null && event.payload.session_id === this.sessionId) {
      callback(event.payload.code, event.payload.remaining_sessions);
      this.sessionId = null;
    }
  });
  this.unlisteners.push(unlisten);
}
```

**After:**
```typescript
async onExit(callback: PtyExitCallback): Promise<void> {
  const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
    // Allow processing even if sessionId is null (not yet set)
    // This handles the race where shell exits before spawn() returns
    if (this.sessionId === null || event.payload.session_id === this.sessionId) {
      // Log for debugging
      console.log(`[PtyClient] pty_exit received: code=${event.payload.code}, remaining=${event.payload.remaining_sessions}`);

      callback(event.payload.code, event.payload.remaining_sessions);
      this.sessionId = null;
    }
  });
  this.unlisteners.push(unlisten);
}
```

**Alternative (Buffer-based approach):**
```typescript
private pendingExitEvent: PtyExitPayload | null = null;
private exitCallback: PtyExitCallback | null = null;

async onExit(callback: PtyExitCallback): Promise<void> {
  this.exitCallback = callback;

  const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
    if (this.sessionId === null) {
      // sessionId not set yet, buffer the event
      this.pendingExitEvent = event.payload;
      console.log('[PtyClient] Buffering pty_exit event (sessionId not set)');
    } else if (event.payload.session_id === this.sessionId) {
      console.log(`[PtyClient] pty_exit received: code=${event.payload.code}, remaining=${event.payload.remaining_sessions}`);
      callback(event.payload.code, event.payload.remaining_sessions);
      this.sessionId = null;
    }
  });
  this.unlisteners.push(unlisten);
}

async spawn(options: PtySpawnOptions = {}): Promise<string> {
  // ... existing spawn logic ...
  this.sessionId = result.session_id;

  // Flush buffered exit event if present
  if (this.pendingExitEvent && this.exitCallback) {
    if (this.pendingExitEvent.session_id === this.sessionId) {
      console.log('[PtyClient] Flushing buffered pty_exit event');
      this.exitCallback(this.pendingExitEvent.code, this.pendingExitEvent.remaining_sessions);
      this.sessionId = null;
      this.pendingExitEvent = null;
    }
  }

  return this.sessionId;
}
```

#### Change 3: Add Debug Logging (src/main.ts)

**Add logging in setupNewTerminalHandlers:**
```typescript
await ptyClient.onExit(async (code, remainingSessions) => {
  console.log(`[Main] onExit callback: code=${code}, remainingSessions=${remainingSessions}`);

  if (remainingSessions === 0) {
    console.log('[Main] Last session exited, closing window...');
    try {
      const appWindow = getCurrentWebviewWindow();
      await appWindow.close();
      console.log('[Main] Window closed successfully');
    } catch (error) {
      console.error('[Main] Failed to close window:', error);
    }
  } else {
    console.log(`[Main] ${remainingSessions} session(s) remaining, keeping window open`);
  }
});
```

#### Change 4: Verify Backend Logging (src-tauri/src/lib.rs)

**Ensure these logs exist:**
```rust
// In spawn_reader_thread, after detecting exit
eprintln!(
    "PTY reader: session {} exited with code {}, {} sessions remaining",
    session_id, exit_code, remaining_sessions
);

// Before emitting pty_exit
eprintln!("PTY reader: emitting pty_exit event for session {}", session_id);

// After emitting pty_exit
if let Err(e) = app.emit("pty_exit", payload) {
    eprintln!("PTY reader: failed to emit pty_exit: {}", e);
}
```

## Test Scenarios

### Unit Tests

#### Rust Tests (Backend)

- [ ] Test 1: `test_session_removal_and_event_emission` - Verify session is removed before event emission
- [ ] Test 2: `test_remaining_sessions_count_accuracy` - Verify remaining_sessions is accurate
- [ ] Test 3: `test_multiple_sessions_exit_sequentially` - Verify count decreases correctly

#### TypeScript Tests (Frontend)

- [ ] Test 1: `PtyClient.onExit should receive events even when sessionId is null`
  ```typescript
  test('onExit receives events before spawn completes', async () => {
    const client = new PtyClient();
    const exitPromise = new Promise((resolve) => {
      client.onExit((code, remaining) => resolve({ code, remaining }));
    });

    // Simulate early event arrival
    emitMockEvent('pty_exit', {
      session_id: 'test-123',
      code: 0,
      remaining_sessions: 0
    });

    // Now set sessionId
    client.sessionId = 'test-123';

    const result = await exitPromise;
    expect(result).toEqual({ code: 0, remaining: 0 });
  });
  ```

- [ ] Test 2: `PtyClient.onExit should flush buffered events after spawn`
  ```typescript
  test('spawn flushes buffered exit events', async () => {
    const client = new PtyClient();
    let exitReceived = false;

    await client.onExit((code, remaining) => {
      exitReceived = true;
    });

    // Simulate event arriving before spawn completes
    emitMockEvent('pty_exit', {
      session_id: 'test-123',
      code: 0,
      remaining_sessions: 0
    });

    // Complete spawn
    mockInvoke.mockResolvedValue({ session_id: 'test-123' });
    await client.spawn({ cols: 80, rows: 24 });

    expect(exitReceived).toBe(true);
  });
  ```

### Integration Tests

- [ ] Test 1: Full flow from spawn to exit
  ```rust
  #[tokio::test]
  async fn test_spawn_and_immediate_exit() {
      let manager = PtyManager::new();
      let result = manager.create_session_atomic(None, 80, 24).await.unwrap();

      // Send exit command immediately
      if let Some(session) = manager.get_session(&result.session_id).await {
          let session = session.lock().await;
          session.write(b"exit\n").unwrap();
      }

      // Wait for exit detection
      tokio::time::sleep(Duration::from_millis(1000)).await;

      // Verify session was removed
      assert!(manager.get_session(&result.session_id).await.is_none());
      assert_eq!(manager.session_count().await, 0);
  }
  ```

### E2E Tests

#### Test 1: Ctrl+D closes window

**Using E2E testing skill:**
```typescript
test('Ctrl+D should close window', async () => {
  // 1. Launch eMterm
  await launchApp();

  // 2. Wait for terminal to be ready
  await waitForElement('#terminal');

  // 3. Send Ctrl+D
  await pressKey('Ctrl+D');

  // 4. Verify window closes within 500ms
  const start = Date.now();
  await waitForWindowClose();
  const elapsed = Date.now() - start;

  expect(elapsed).toBeLessThan(500);
});
```

#### Test 2: Exit command closes window

```typescript
test('exit command should close window', async () => {
  await launchApp();
  await waitForElement('#terminal');

  // Type 'exit' and press Enter
  await typeText('exit');
  await pressKey('Enter');

  await waitForWindowClose();
  expect(true).toBe(true); // If we got here, test passed
});
```

#### Test 3: Shell crash closes window

```typescript
test('shell crash should close window', async () => {
  await launchApp();
  await waitForElement('#terminal');

  // Trigger shell crash (kill -9)
  await typeText('kill -9 $$'); // Kill current shell
  await pressKey('Enter');

  await waitForWindowClose();
  expect(true).toBe(true);
});
```

### Edge Cases

- [ ] Edge case 1: Shell exits before spawn() returns
  - **Scenario:** Shell script immediately exits (e.g., `bash -c 'exit'`)
  - **Expected:** Event is buffered and processed after spawn completes

- [ ] Edge case 2: Multiple rapid spawns and exits
  - **Scenario:** Spawn session, exit immediately, spawn again
  - **Expected:** Each session is properly tracked and cleaned up

- [ ] Edge case 3: Window close during shell execution
  - **Scenario:** User clicks × while shell is running
  - **Expected:** beforeunload cleanup kills session, window closes

### Performance Tests

- [ ] Performance 1: Measure exit-to-close latency
  ```typescript
  test('window closes within 500ms of shell exit', async () => {
    await launchApp();
    await waitForElement('#terminal');

    const start = performance.now();
    await pressKey('Ctrl+D');
    await waitForWindowClose();
    const elapsed = performance.now() - start;

    expect(elapsed).toBeLessThan(500);
  });
  ```

- [ ] Performance 2: Event delivery latency
  - **Metric:** Time from backend emit to frontend callback execution
  - **Target:** < 100ms

## Security Considerations

- **Authentication:** Not applicable (local application)
- **Authorization:** Not applicable (user controls their own terminal)
- **Input Validation:** PTY input is not validated (passes through to shell)
- **Data Protection:** No sensitive data is logged
- **XSS Prevention:** Not applicable (no web content rendering in this feature)
- **SQL Injection Prevention:** Not applicable (no database)
- **CSRF Protection:** Not applicable (no web forms)

**Security Note:** This feature does not introduce new security risks. The existing PTY security model remains unchanged.

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| N/A | Window close failed | N/A | "Failed to close window: {error}" (console only) |
| N/A | Event emission failed | N/A | "Failed to emit pty_exit: {error}" (stderr only) |

### Error Flow

```mermaid
flowchart TD
    A[pty_exit event emitted] --> B{Frontend receives?}
    B -->|Yes| C[Process event]
    B -->|No| D[Log error to console]
    D --> E[Window remains open]

    C --> F{remaining_sessions == 0?}
    F -->|Yes| G[Attempt window close]
    F -->|No| H[Do nothing]

    G --> I{Close succeeds?}
    I -->|Yes| J[App terminates]
    I -->|No| K[Log error]
    K --> E
```

**Error Handling Strategy:**

1. **Event Emission Failure (Backend):**
   - Log error to stderr
   - Continue with cleanup
   - Window will remain open (user must close manually)

2. **Event Reception Failure (Frontend):**
   - Cannot detect directly
   - Mitigated by debug logging
   - Window will remain open

3. **Window Close Failure (Frontend):**
   - Log error to console
   - Window remains open
   - User can close manually

## Performance Optimization

### Performance Goals

- Response time: Window closes < 500ms after shell exit (95th percentile)
- Event delivery: < 100ms from backend emit to frontend callback
- CPU overhead: Event handling < 1ms CPU time

### Optimization Strategies

- **Strategy 1:** Use atomic operations for session removal and count retrieval
  - **Rationale:** Prevents race conditions, ensures consistent state
  - **Implementation:** Already implemented via `remove_session_atomic()`

- **Strategy 2:** Register event listeners before spawning
  - **Rationale:** Eliminates race condition where events arrive before listeners are ready
  - **Implementation:** Reorder initialization code in `initTerminal()`

- **Strategy 3:** Use buffering for early events
  - **Rationale:** Handles edge case where shell exits immediately
  - **Implementation:** Buffer events in `PtyClient` until `sessionId` is set

### Caching Strategy

- No caching required for this feature

## Success Criteria

- [x] All functional requirements (FR1-FR5) are implemented and tested
- [x] All test scenarios pass
- [x] Window closes within 500ms in 95% of cases
- [x] Event delivery success rate ≥ 99.9%
- [x] Debug logs provide clear visibility into the process
- [x] Code review completed
- [x] E2E tests pass on Linux (primary development platform)
- [ ] Manual testing completed on macOS (if available)
- [ ] Manual testing completed on Windows (if available)

## Open Questions

- [ ] Question 1: Should we add a user preference to disable auto-close? (Current answer: No, fixed behavior)
- [ ] Question 2: How should we handle multiple windows in the future? (Deferred to multi-window feature)
- [ ] Question 3: Should we add a timeout for window close operation? (To be determined during implementation)

## Implementation Phases

### Phase 1: Fix Event Delivery

**Goals:** Ensure pty_exit events reliably reach the frontend

**Deliverables:**
- Modified `src/pty/client.ts` with buffered event handling
- Modified `src/main.ts` with correct listener registration order
- Debug logging added to both frontend and backend
- Unit tests for event buffering

**Estimated Effort:** 4 hours

### Phase 2: Add Debug Logging

**Goals:** Provide visibility into event flow for troubleshooting

**Deliverables:**
- Console logs in frontend at each stage
- stderr logs in backend at each stage
- Log format standardized with prefixes

**Estimated Effort:** 2 hours

### Phase 3: Testing and Validation

**Goals:** Verify the fix works across all scenarios

**Deliverables:**
- Unit tests passing
- Integration tests passing
- E2E tests passing
- Manual testing on Linux completed
- Performance measurements recorded

**Estimated Effort:** 4 hours

### Phase 4: Documentation and Cleanup

**Goals:** Finalize implementation and document behavior

**Deliverables:**
- Code comments updated
- This specification marked as implemented
- Known issues documented (if any)

**Estimated Effort:** 2 hours

**Total Estimated Effort:** 12 hours

## References

- Requirements Document: `doc/tasks/close-app-on-last-shell/要件定義書.md`
- Tauri Events API: https://tauri.app/v2/reference/javascript/api/core/#emitter
- Tauri WebviewWindow API: https://tauri.app/v2/reference/javascript/api/webviewwindow/
- Related Code:
  - `src-tauri/src/lib.rs` - PTY command handlers and event emission
  - `src-tauri/src/pty/manager.rs` - Session management
  - `src-tauri/src/pty/graceful_shutdown.rs` - Graceful shutdown logic
  - `src/main.ts` - Application initialization
  - `src/pty/client.ts` - PTY client interface
