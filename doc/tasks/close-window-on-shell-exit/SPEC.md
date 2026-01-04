# Feature: Tab-Aware Shell Exit and Window Close

## Overview

This feature implements a tab-aware architecture for handling shell termination and window closure in eMterm. While the current implementation immediately closes the window when a shell exits, this design prepares the codebase for future multi-tab support by introducing tab counting and intelligent close behavior.

**Current Behavior:** Single PTY session per window. Shell exit → Window close.

**Target Behavior (Future-Ready):**
- Single tab (current): Shell exit → Window close
- Multiple tabs (future): Shell exit → Close that tab only, window remains if other tabs exist

## Objectives

- Utilize existing PtyManager's session registry (HashMap) as the single source of truth for tab count
- Refactor shell exit handling to be tab-aware
- Implement graceful shutdown sequence for manual tab close
- Maintain current user experience (no UI changes yet)
- Establish foundation for future multi-tab UI implementation
- Achieve comprehensive test coverage (80%+)

## User Stories

### US1: Shell Natural Exit Closes Tab
As a terminal user, when I exit the shell (via `exit` command or Ctrl+D), the tab should close automatically.

**Acceptance Criteria:**
- [ ] Shell exit is detected within 500ms
- [ ] If it's the last (only) tab, the window closes
- [ ] If multiple tabs exist (future), only that tab closes
- [ ] Exit code is captured and logged

### US2: Last Tab Close Triggers Window Close
As a system, when the last active tab is closed, the entire window should close automatically.

**Acceptance Criteria:**
- [ ] Tab count is accurately maintained
- [ ] When count reaches 0, window close is triggered
- [ ] No orphaned processes remain after window close
- [ ] All resources are properly cleaned up

### US3: Manual Tab Close Uses Graceful Shutdown
As a user, when I manually close a tab (future UI), the shell should be terminated gracefully without data loss.

**Acceptance Criteria:**
- [ ] System sends `exit\n` command first
- [ ] Waits up to 5 seconds for natural exit
- [ ] If timeout, sends Ctrl+D (EOF)
- [ ] Waits additional 2 seconds
- [ ] As last resort, force kills the process
- [ ] Tab close completes within 10 seconds total

### US4: Multiple Tabs Are Isolated (Future)
As a user with multiple tabs open, when one shell exits, other tabs should continue working normally.

**Acceptance Criteria:**
- [ ] Tab close affects only the target session
- [ ] Other sessions remain active
- [ ] Window stays open
- [ ] Tab count decrements correctly

## Technical Requirements

### Functional Requirements

- **FR1:** System shall maintain an accurate count of active PTY sessions (tabs)
- **FR2:** System shall detect shell process termination within 500ms
- **FR3:** System shall close the window only when the last tab is closed
- **FR4:** System shall implement graceful shutdown with 3-stage escalation (exit → EOF → kill)
- **FR5:** System shall emit events for tab lifecycle (created, closed, count changed)
- **FR6:** System shall preserve all existing pty_exit event behavior (backward compatibility)

### Non-Functional Requirements

- **NFR1 - Performance:**
  - Tab count update: < 10ms
  - Shell exit detection: < 500ms
  - Manual close initiation: < 100ms
  - Memory overhead: < 10KB additional

- **NFR2 - Reliability:**
  - Tab count synchronization must be thread-safe
  - No race conditions in concurrent close operations
  - Auto-recovery from count inconsistencies

- **NFR3 - Maintainability:**
  - Tab management logic isolated in dedicated module
  - Clear separation between tab logic and PTY session logic
  - Comprehensive documentation and comments

- **NFR4 - Testability:**
  - Unit test coverage: 80%+
  - E2E test coverage for all user stories
  - Benchmarks for performance requirements

## Implementation Approach

### Architecture

**Simplified Architecture (Backend as Single Source of Truth):**

The existing `PtyManager` already manages sessions in a `HashMap<SessionId, PtySession>`.
The `session_count()` method (already implemented) returns `sessions.len()`, which is always accurate.

**Key Design Decision:** No separate TabManager module needed. Use existing PtyManager.

```
┌─────────────────────────────────────────────────┐
│          Frontend (TypeScript)                  │
│  ┌──────────────────────────────────────────┐  │
│  │   Shell Exit Handler                     │  │
│  │   - Query backend session_count          │  │
│  │   - Close window if count == 0           │  │
│  └──────────────────────────────────────────┘  │
│                    ↕ IPC Events/Commands        │
├─────────────────────────────────────────────────┤
│          Backend (Rust/Tauri)                   │
│  ┌──────────────────────────────────────────┐  │
│  │   PtyManager (Existing)                  │  │
│  │   - sessions: HashMap<SessionId, ...>    │  │
│  │   - session_count() → sessions.len()     │  │
│  │   - Single source of truth               │  │
│  └──────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────┐  │
│  │   GracefulShutdown (New Module)          │  │
│  │   - Implement 3-stage shutdown           │  │
│  │   - Timeout management                   │  │
│  └──────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

**Component Diagram:**
```
┌──────────────┐      creates      ┌──────────────┐
│   Window     │ ───────────────→  │  TabManager  │
└──────────────┘                    └──────────────┘
                                           │
                                           │ manages
                                           ↓
                                    ┌──────────────┐
                                    │  PtySession  │
                                    │  (1 to N)    │
                                    └──────────────┘
                                           │
                                           │ spawns
                                           ↓
                                    ┌──────────────┐
                                    │ ShellProcess │
                                    └──────────────┘
```

### Data Flow

#### Shell Exit Flow
```
Shell Process Exits
    ↓
PTY Reader Thread detects (via try_wait polling)
    ↓
Backend: PtyManager.remove_session() called
    ↓
Emit "pty_exit" event (existing)
    ↓
Frontend PtyClient.onExit() handler
    ↓
Query backend: session_count command
    ↓
If count == 0: Close window
If count > 0: Do nothing (other tabs remain)
```

#### Manual Tab Close Flow
```
User initiates close (future: click X button)
    ↓
Frontend calls TabManager.closeTab(sessionId)
    ↓
Backend GracefulShutdown.shutdown(sessionId)
    ↓
Stage 1: Send "exit\n" → Wait 5s
    ↓ (timeout)
Stage 2: Send Ctrl+D (0x04) → Wait 2s
    ↓ (timeout)
Stage 3: Kill process (SIGTERM → SIGKILL)
    ↓
Emit "pty_exit" event
    ↓
Follow normal exit flow
```

### API Design

#### New Tauri Commands (Backend)

##### session_count
**Request:**
```
Method: Tauri Command
Name: session_count
Arguments: None
```

**Response:**
```rust
Result<usize, String>
// Success: 2
// Error: "Failed to get session count"
```

**Implementation (uses existing PtyManager.session_count()):**
```rust
#[tauri::command]
async fn session_count(
    state: State<'_, Arc<PtyManager>>,
) -> Result<usize, String> {
    Ok(state.session_count().await)
}
```

##### tab_close_graceful
**Request:**
```
Method: Tauri Command
Name: tab_close_graceful
Arguments:
  - session_id: String
  - timeout_ms: Option<u64> (default: 5000)
```

**Response:**
```rust
Result<(), String>
// Success: ()
// Error: "Session not found" | "Close operation failed"
```

**Implementation:**
```rust
#[tauri::command]
async fn tab_close_graceful(
    state: State<'_, PtyManager>,
    session_id: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    // Implementation in GracefulShutdown module
}
```

#### Frontend API Changes (TypeScript)

##### Shell Exit Handler (in main.ts or client.ts)
```typescript
/**
 * Handle shell exit event - query backend for session count
 * and close window if no sessions remain
 */
async function handleShellExit(sessionId: string, exitCode: number): Promise<void> {
  // Query backend for accurate session count
  const count = await invoke<number>('session_count');

  if (count === 0) {
    // Last session closed - close window
    await getCurrentWebviewWindow().close();
  }
  // If count > 0, other sessions exist (future multi-tab)
}

/**
 * Close a tab gracefully (for future manual tab close UI)
 */
async function closeTabGracefully(sessionId: string): Promise<void> {
  await invoke('tab_close_graceful', { sessionId });
  // Backend will emit pty_exit event, triggering handleShellExit
}
```

**Note:** No client-side TabManager class needed. Backend is the single source of truth.

### Database Schema

Not applicable - all state is in-memory during application runtime.

### Dependencies

**Internal Dependencies:**
- `PtyManager`: Tab counting hooks into session creation/removal
- `PtySession`: Exit detection mechanism is reused
- Tauri IPC: Event emission and command handling

**External Dependencies:**
- `portable-pty` (existing): Process management and exit detection
- `tokio` (existing): Async runtime for timeout handling
- `tauri` (existing): IPC framework

**New Rust Crates (if needed):**
- None - use existing dependencies

**New npm Packages (if needed):**
- None - use existing dependencies

### File Structure

**Backend (Rust):**
```
src-tauri/src/
├── pty/
│   ├── mod.rs                    # Module exports (updated)
│   ├── manager.rs                # PtyManager (existing - already has session_count())
│   ├── session.rs                # PtySession (unchanged)
│   ├── shell.rs                  # Shell detection (unchanged)
│   └── graceful_shutdown.rs      # NEW: Graceful shutdown implementation
├── lib.rs                        # Updated: register session_count and tab_close_graceful commands
└── main.rs                       # Unchanged
```

**Frontend (TypeScript):**
```
src/
├── pty/
│   ├── client.ts                 # PtyClient (enhanced: shell exit handler)
│   └── index.ts                  # Unchanged
├── main.ts                       # Updated: query backend session count on exit
└── types/
    └── pty.ts                    # Unchanged (no new types needed)
```

**Note:** No new TabManager modules needed - existing PtyManager handles tab counting via HashMap.

## Test Scenarios

### Unit Tests

#### Rust Unit Tests

**Note:** Existing `manager.rs` already has tests for `session_count()`. See lines 104-187.

**Test File:** `src-tauri/src/pty/graceful_shutdown.rs`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exit_command_sent_first() {
        // Mock PtySession that captures writes
        // Verify "exit\n" is sent first
    }

    #[tokio::test]
    async fn test_timeout_triggers_eof() {
        // Create session with shell that ignores exit
        // Verify EOF is sent after timeout
    }

    #[tokio::test]
    async fn test_force_kill_as_last_resort() {
        // Create session with shell that hangs
        // Verify force kill happens after all timeouts
    }

    #[tokio::test]
    async fn test_immediate_exit_on_success() {
        // Shell exits on first "exit\n"
        // Verify no subsequent signals are sent
    }
}
```

#### TypeScript Unit Tests

**Note:** No client-side TabManager class needed. Testing focuses on:
1. `handleShellExit()` function behavior (mock `invoke` calls)
2. Integration with existing PtyClient.onExit() handler

```typescript
// src/pty/client.test.ts (additions)
import { describe, test, expect, mock } from 'bun:test';

describe('Shell Exit Handler', () => {
  test('should close window when session count is 0', async () => {
    // Mock invoke to return 0 sessions
    // Verify window.close() is called
  });

  test('should not close window when sessions remain', async () => {
    // Mock invoke to return 1+ sessions
    // Verify window.close() is NOT called
  });
});
```

### Integration Tests

**Note:** Existing `manager.rs` already has integration tests for session lifecycle.
See `test_create_session`, `test_remove_session`, `test_multiple_sessions` (lines 104-187).

Additional tests for graceful shutdown:

**Test File:** `src-tauri/src/pty/graceful_shutdown.rs` (integration tests)
```rust
#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_graceful_shutdown_normal_shell() {
        // Create session with /bin/sh
        // Call graceful_shutdown()
        // Verify session exits in Stage 1 (exit command)
        // Verify session_count() is 0
    }

    #[tokio::test]
    async fn test_graceful_shutdown_hanging_process() {
        // Create session
        // Run `sleep 999` in shell
        // Call graceful_shutdown()
        // Verify Stage 3 (force kill) is reached
        // Verify session_count() is 0
    }
}
```

### E2E Tests

**Test File:** `test/e2e/tab-close.test.ts`
```typescript
import { test, expect } from '@playwright/test';
import { spawn } from 'child_process';

test.describe('Tab Close Behavior', () => {
  test('window closes when shell exits (single tab)', async ({ page }) => {
    // 1. Launch eMterm
    const emterm = spawn('bun', ['tauri', 'dev']);

    // 2. Wait for window to appear
    await page.waitForSelector('#terminal');

    // 3. Send 'exit\n' to shell
    await page.keyboard.type('exit');
    await page.keyboard.press('Enter');

    // 4. Verify window closes within 1 second
    await expect(page).toHaveTitle('', { timeout: 1000 });
  });

  test('graceful shutdown on manual close', async ({ page }) => {
    // 1. Launch eMterm
    // 2. Trigger manual close via future API
    // 3. Monitor that 'exit\n' was sent (via logs or instrumentation)
    // 4. Verify window closed
  });

  test('force kill after timeout', async ({ page }) => {
    // 1. Launch eMterm
    // 2. Run 'sleep 999999' in shell
    // 3. Trigger manual close
    // 4. Verify process killed after 7 seconds (5s + 2s)
    // 5. Verify window closed
  });

  test('tab count accuracy across lifecycle', async ({ page }) => {
    // Future: When multi-tab UI exists
    // 1. Open 3 tabs
    // 2. Close 1 tab
    // 3. Verify tab count is 2
    // 4. Close all tabs
    // 5. Verify window closed
  });
});
```

### Edge Cases

- [ ] **Edge case 1:** User kills shell process externally (via `kill` command) - System should detect via try_wait and close tab normally
- [ ] **Edge case 2:** Shell crashes or segfaults - Exit code is non-zero, but tab close proceeds normally
- [ ] **Edge case 3:** Network interruption during SSH session in shell - PTY should detect broken pipe and close
- [ ] **Edge case 4:** Rapid succession of tab opens and closes - HashMap naturally handles this (no race conditions possible with RwLock)
- [ ] **Edge case 5:** Window close during graceful shutdown in progress - Cleanup completes before window destruction

**Note:** Edge case 6 (negative tab count) is no longer possible since we use `HashMap.len()` which is always >= 0.

### Performance Tests

**Note:** Since we use existing `PtyManager.session_count()` which just calls `HashMap.len()`,
performance is guaranteed to be O(1). No separate benchmarks needed for tab counting.

**Benchmark File:** `src-tauri/benches/graceful_shutdown.rs` (if needed)
```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_graceful_shutdown(c: &mut Criterion) {
    // Benchmark graceful shutdown stages
    // Note: This involves actual process creation/killing, so results vary
}

criterion_group!(benches, benchmark_graceful_shutdown);
criterion_main!(benches);
```

**Performance Acceptance Criteria:**
- Session count query (`session_count()`): O(1), effectively instantaneous
- Shell exit detection: < 500ms from process termination
- Manual close initiation: < 100ms to send first signal

## Security Considerations

### Input Validation
- **Session ID validation:** All session IDs must match pattern `[a-zA-Z0-9-_]+` (validated in PtyManager)
- **Command injection prevention:** The `exit\n` and Ctrl+D sequences are hardcoded byte arrays, not user input
- **Timeout bounds:** Timeout values are clamped to reasonable ranges (100ms - 60s) to prevent DoS

### Authentication & Authorization
Not applicable - desktop application without multi-user scenarios.

### Data Protection
- **Process isolation:** Each PTY session runs in isolated process
- **No sensitive data in tab count:** Tab count is non-sensitive metadata

### XSS Prevention
Not applicable - no web content rendering in this feature.

### SQL Injection Prevention
Not applicable - no database queries.

### CSRF Protection
Not applicable - no web endpoints.

### Additional Security Measures
- **Resource cleanup:** Ensure zombie processes don't accumulate on failed graceful shutdown
- **Log sanitization:** Session IDs in logs are sanitized (no PII)
- **Process kill permissions:** Verify current user has permission to kill spawned shells

## Error Handling

### Error Codes

| Code | Description | HTTP Status | User Message |
|------|-------------|-------------|--------------|
| TAB_002 | Session not found on close | N/A (Internal) | "Tab already closed" |
| TAB_003 | Graceful shutdown timeout | N/A (Internal) | "Tab close in progress" |
| TAB_004 | Force kill failed | N/A (Internal) | "Failed to close tab" |
| TAB_005 | Window close failed | N/A (Internal) | "Failed to close window" |

**Note:** TAB_001 (negative tab count) removed - impossible with HashMap-based counting.

### Error Flow

```
Error Occurs
    ↓
Log error with context (session_id, error_code)
    ↓
Determine error severity
    ↓
Non-critical (e.g., already closed): Log and continue
    ↓
Return appropriate error response to caller
```

### Error Recovery Strategies

**Graceful Shutdown Timeout (TAB_003):**
- Stage 1 timeout → Proceed to Stage 2 (EOF)
- Stage 2 timeout → Proceed to Stage 3 (force kill)
- Force kill timeout → Log error and remove session anyway

## Performance Optimization

### Performance Goals
- Response time: < 100ms for session operations (99th percentile)
- Throughput: Support 100+ tab open/close cycles per second (future)
- Resource usage: Minimal overhead (uses existing PtyManager HashMap)

### Optimization Strategies

**1. Use Existing HashMap (Already Optimal):**
```rust
// PtyManager already uses RwLock<HashMap> for session management
// session_count() is O(1) - just returns HashMap.len()
pub async fn session_count(&self) -> usize {
    let sessions = self.sessions.read().await;
    sessions.len()
}
```

**2. Non-blocking Graceful Shutdown:**
- Run shutdown sequence in spawned tokio task (don't block Tauri command thread)
- Use async/await for timeout handling (tokio::time::timeout)

### Caching Strategy
Not applicable - session count is O(1) from HashMap.

### Database Query Optimization
Not applicable - no database.

## Success Criteria

- [ ] All functional requirements (FR1-FR6) are implemented and verified
- [ ] All test scenarios pass (unit, integration, E2E)
- [ ] Performance meets specified goals (< 500ms exit detection, < 10ms count operations)
- [ ] Security requirements are satisfied (no command injection, proper cleanup)
- [ ] Code coverage ≥ 80% for new modules
- [ ] Code review completed with no major issues
- [ ] Documentation is complete (inline comments, this spec)
- [ ] Existing functionality remains unchanged (backward compatibility verified)
- [ ] No memory leaks detected (valgrind or similar tool)
- [ ] Graceful shutdown succeeds in ≥ 95% of cases in E2E tests

## Open Questions

- [ ] **Q1:** Should we expose tab count to the frontend UI (for future status bar)?
  - *Decision needed:* Add `tab_get_count` Tauri command now, or wait for UI implementation?

- [ ] **Q2:** Should graceful shutdown timeout be user-configurable?
  - *Proposal:* Keep hardcoded for now, make configurable in settings later.

- [ ] **Q3:** What should happen if graceful shutdown takes > 10 seconds on a slow system?
  - *Proposal:* Use absolute timeout (10s max) regardless of stage progression.

- [ ] **Q4:** Should we emit a `tab_close_warning` event when force kill is needed?
  - *Proposal:* Yes, for future UI to show "Force closing tab..." notification.

## Implementation Phases

### Phase 1: Backend Session Count Command (Small)
**Goals:** Expose existing `session_count()` as Tauri command.

**Deliverables:**
- `session_count` Tauri command in `lib.rs`
- Unit test for command

**Success Metrics:**
- Command returns correct count
- Existing tests still pass

**Effort:** 1-2 hours

### Phase 2: Graceful Shutdown (Medium)
**Goals:** Implement 3-stage graceful shutdown mechanism.

**Deliverables:**
- `src-tauri/src/pty/graceful_shutdown.rs` module
- `tab_close_graceful` Tauri command
- Integration tests for shutdown sequence
- Timeout handling and force kill fallback

**Success Metrics:**
- Graceful shutdown success rate ≥ 95% in tests
- All stages execute in specified timeouts

**Effort:** 2-3 days

### Phase 3: Frontend Integration (Small)
**Goals:** Update frontend to query backend session count on shell exit.

**Deliverables:**
- Updated `src/main.ts` to query `session_count` on exit
- Close window only when count == 0
- Unit tests for exit handler

**Success Metrics:**
- Window closes correctly on last session exit
- All tests pass

**Effort:** 1 day

### Phase 4: E2E Testing (Medium)
**Goals:** Comprehensive end-to-end testing.

**Deliverables:**
- E2E test suite covering all user stories
- Edge case coverage
- Documentation of test results

**Success Metrics:**
- All E2E tests pass
- Code coverage ≥ 80%

**Effort:** 2-3 days

## References

- **Project Documentation:** `/home/sakura/cache/worktrees/emterm/fix-close-window-on-shell-exit/CLAUDE.md`
- **Current PTY Implementation:** `src-tauri/src/pty/`
- **Current Frontend:** `src/main.ts`, `src/pty/client.ts`
- **Requirements Document:** `doc/tasks/close-window-on-shell-exit/要件定義書.md`
- **portable-pty API:** https://docs.rs/portable-pty/latest/portable_pty/
- **Tauri Events:** https://v2.tauri.app/develop/calling-frontend/
- **Rust Atomic Types:** https://doc.rust-lang.org/std/sync/atomic/
