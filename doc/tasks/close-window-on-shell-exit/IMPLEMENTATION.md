# Implementation Plan: Tab-Aware Shell Exit and Window Close

## Overview

This implementation prepares eMterm for future multi-tab support by utilizing the existing `PtyManager` session registry as the single source of truth for tab counting.

**Key Design Decision:** No separate TabManager module needed. The existing `PtyManager` already manages sessions in a `HashMap<SessionId, PtySession>` with a `session_count()` method that returns `sessions.len()`.

## Objectives

- Expose existing `session_count()` as a Tauri command
- Update frontend to query backend session count on shell exit
- Close window only when session count reaches 0
- Implement graceful shutdown sequence for manual tab close
- Emit tab lifecycle events (FR5: tab_created, tab_closed, tab_count_changed)
- Maintain backward compatibility with existing PTY behavior
- Achieve 80%+ test coverage for new code

## Prerequisites

### Development Environment

- **Rust**: 1.70+ (existing project requirement)
- **Bun**: Latest stable version (existing project requirement)
- **Tauri CLI**: v2.x (existing project requirement)

All dependencies are already installed in the existing eMterm project.

### Existing Infrastructure

The following already exists and will be reused:

```rust
// src-tauri/src/pty/manager.rs (lines 18-21)
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<PtySession>>>>>,
}

// src-tauri/src/pty/manager.rs (lines 93-97)
pub async fn session_count(&self) -> usize {
    let sessions = self.sessions.read().await;
    sessions.len()
}
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│          Frontend (TypeScript)                  │
│  ┌──────────────────────────────────────────┐  │
│  │   Shell Exit Handler (main.ts)           │  │
│  │   - Query backend: invoke('session_count')│  │
│  │   - Close window if count == 0           │  │
│  └──────────────────────────────────────────┘  │
│                    ↕ IPC                        │
├─────────────────────────────────────────────────┤
│          Backend (Rust/Tauri)                   │
│  ┌──────────────────────────────────────────┐  │
│  │   PtyManager (Existing)                  │  │
│  │   - sessions: HashMap<SessionId, ...>    │  │
│  │   - session_count() → sessions.len()     │  │
│  └──────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────┐  │
│  │   GracefulShutdown (New)                 │  │
│  │   - 3-stage shutdown sequence            │  │
│  └──────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

## Implementation Phases

### Phase 1: Backend Session Count Command & Tab Lifecycle Events (FR5)

**Goal**: Expose existing `session_count()` as a Tauri command and emit tab lifecycle events.

**Effort**: 2-4 hours

**Files to Modify:**
- `src-tauri/src/lib.rs`: Add `session_count` command, emit tab events
- `src-tauri/src/pty/manager.rs`: Emit events on session create/remove

**Implementation:**

```rust
// Add to src-tauri/src/lib.rs

#[tauri::command]
async fn session_count(
    state: State<'_, Arc<PtyManager>>,
) -> Result<usize, String> {
    Ok(state.session_count().await)
}

// Register in .invoke_handler():
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    session_count,
])
```

**Tab Lifecycle Events (FR5):**

| Event | Payload | When Emitted |
|-------|---------|--------------|
| `tab_created` | `{ session_id: string }` | After `create_session()` |
| `tab_closed` | `{ session_id: string, exit_code: i32 }` | After session removal |
| `tab_count_changed` | `{ count: usize }` | After any count change |

```rust
// In PtyManager.create_session() - emit after insertion
app_handle.emit("tab_created", json!({ "session_id": id }))?;
app_handle.emit("tab_count_changed", json!({ "count": sessions.len() }))?;

// In session removal (pty_exit handler) - emit after removal
app_handle.emit("tab_closed", json!({ "session_id": id, "exit_code": code }))?;
app_handle.emit("tab_count_changed", json!({ "count": sessions.len() }))?;
```

**Thread Safety (NFR2):**
- Events are emitted **inside** the RwLock write guard
- This ensures count is accurate at emission time
- No race condition between count change and event

**Testing:**
- Verify command returns 0 when no sessions
- Verify command returns correct count after creating sessions
- Verify `tab_created` event emitted on session creation
- Verify `tab_closed` event emitted on session removal
- Verify `tab_count_changed` reflects accurate count
- Existing `manager.rs` tests already cover `session_count()` behavior

**Acceptance Criteria:**
- [ ] `session_count` command registered
- [ ] Returns correct count
- [ ] `tab_created` event emitted
- [ ] `tab_closed` event emitted
- [ ] `tab_count_changed` event emitted
- [ ] Events emitted inside lock (thread-safe)
- [ ] Existing tests pass

---

### Phase 2: Graceful Shutdown Mechanism

**Goal**: Implement 3-stage graceful shutdown sequence for manual tab close.

**Effort**: 2-3 days

**Files to Create:**
- `src-tauri/src/pty/graceful_shutdown.rs`

**Files to Modify:**
- `src-tauri/src/pty/mod.rs`: Add `pub mod graceful_shutdown;`
- `src-tauri/src/lib.rs`: Add `tab_close_graceful` command

**Shutdown Sequence:**

```
Stage 1: Send "exit\n" → Wait 5 seconds
    ↓ (timeout)
Stage 2: Send EOF (0x04) → Wait 2 seconds
    ↓ (timeout)
Stage 3: Force kill (SIGTERM → SIGKILL)
```

**Key Components:**

| Function | Responsibility |
|----------|----------------|
| `shutdown()` | Execute full 3-stage sequence |
| `send_exit_command()` | Write `b"exit\n"` to PTY |
| `send_eof()` | Write `b"\x04"` (Ctrl+D) to PTY |
| `force_kill()` | Kill process with SIGTERM/SIGKILL |
| `wait_for_exit()` | Poll process status with timeout |

**Implementation Outline:**

```rust
// src-tauri/src/pty/graceful_shutdown.rs

use std::time::Duration;
use tokio::time::timeout;

const STAGE1_TIMEOUT: Duration = Duration::from_secs(5);
const STAGE2_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn shutdown(
    pty_manager: &PtyManager,
    session_id: &str,
) -> Result<(), String> {
    let session = pty_manager.get_session(session_id).await
        .ok_or("Session not found")?;

    // Stage 1: Send exit command
    {
        let mut s = session.lock().await;
        s.write(b"exit\n").map_err(|e| e.to_string())?;
    }

    if wait_for_exit(&session, STAGE1_TIMEOUT).await {
        return Ok(());
    }

    // Stage 2: Send EOF
    {
        let mut s = session.lock().await;
        s.write(b"\x04").map_err(|e| e.to_string())?;
    }

    if wait_for_exit(&session, STAGE2_TIMEOUT).await {
        return Ok(());
    }

    // Stage 3: Force kill
    {
        let mut s = session.lock().await;
        s.kill().map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn wait_for_exit(
    session: &Arc<Mutex<PtySession>>,
    timeout_duration: Duration,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout_duration {
        let s = session.lock().await;
        if s.try_wait().is_some() {
            return true;
        }
        drop(s);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}
```

**Tauri Command:**

```rust
#[tauri::command]
async fn tab_close_graceful(
    state: State<'_, Arc<PtyManager>>,
    session_id: String,
) -> Result<(), String> {
    graceful_shutdown::shutdown(&state, &session_id).await
}
```

**Testing:**
- Test Stage 1 success (normal shell exits on `exit`)
- Test Stage 2 escalation (shell ignores exit, responds to EOF)
- Test Stage 3 escalation (hanging process requires kill)
- Test session removal after shutdown

**Acceptance Criteria:**
- [ ] 3-stage shutdown implemented
- [ ] Timeouts configurable
- [ ] Process always terminated (no orphans)
- [ ] Unit tests pass
- [ ] Integration tests pass

---

### Phase 3: Frontend Integration

**Goal**: Update frontend to query backend session count on shell exit.

**Effort**: 1 day

**Files to Modify:**
- `src/main.ts`: Update shell exit handler

**Current Implementation (main.ts):**
```typescript
// Current: Closes window immediately on shell exit
ptyClient.onExit = (exitCode) => {
  getCurrentWebviewWindow().close();
};
```

**New Implementation:**
```typescript
// New: Query session count before closing
ptyClient.onExit = async (exitCode) => {
  const count = await invoke<number>('session_count');
  if (count === 0) {
    getCurrentWebviewWindow().close();
  }
  // If count > 0, other sessions exist (future multi-tab)
};
```

**Testing:**
- Mock `invoke` to return 0 → verify window closes
- Mock `invoke` to return 1 → verify window does NOT close
- Integration test with real backend

**Acceptance Criteria:**
- [ ] Frontend queries backend on exit
- [ ] Window closes only when count == 0
- [ ] Unit tests pass

---

### Phase 4: E2E Testing

**Goal**: Comprehensive end-to-end testing.

**Effort**: 2-3 days

**Test Scenarios:**

| Scenario | Expected Behavior |
|----------|-------------------|
| Shell exits naturally | Window closes (single tab) |
| Manual close on normal shell | Exit in Stage 1 |
| Manual close on hanging process | Kill in Stage 3 |
| External kill (via `kill` cmd) | Window closes |
| Rapid open/close cycles | No memory leaks |

**Manual Testing Checklist:**
- [ ] Start eMterm, type `exit` → window closes
- [ ] Start eMterm, press Ctrl+D → window closes
- [ ] Run `sleep 999`, trigger close → process killed
- [ ] Verify no zombie processes remain

**Acceptance Criteria:**
- [ ] All E2E tests pass
- [ ] No memory leaks
- [ ] Code coverage ≥ 80%

---

## Complete File Structure

```
src-tauri/src/
├── pty/
│   ├── mod.rs                    # Updated: add graceful_shutdown
│   ├── manager.rs                # Unchanged (already has session_count)
│   ├── session.rs                # Unchanged
│   ├── shell.rs                  # Unchanged
│   └── graceful_shutdown.rs      # NEW: 3-stage shutdown
├── lib.rs                        # Updated: add session_count, tab_close_graceful
└── main.rs                       # Unchanged

src/
├── main.ts                       # Updated: query session_count on exit
└── pty/
    └── client.ts                 # Unchanged
```

## Resolved Questions

### Q5: Frontend/Backend Sync
**Decision:** Not needed.

Backend is the single source of truth. Frontend queries backend on shell exit events.
Tauri IPC is reliable (process-internal communication), so events won't be lost.

### Q6: Negative Tab Count Recovery
**Decision:** Not applicable.

Using `HashMap.len()` which is always >= 0. Negative counts are impossible.

### Q7: Graceful Shutdown Progress Events
**Decision:** Defer to future.

Current implementation logs progress. UI progress events can be added when multi-tab UI is implemented.

### Q8: Active Tab Tracking
**Decision:** Defer to multi-tab UI implementation.

Not needed for current scope (single tab).

## Success Criteria

- [ ] `session_count` command works correctly
- [ ] `tab_close_graceful` command works correctly
- [ ] Tab lifecycle events emitted (FR5: tab_created, tab_closed, tab_count_changed)
- [ ] Events emitted inside RwLock guard (NFR2: thread-safe)
- [ ] Window closes only when last session exits
- [ ] Graceful shutdown completes within 10 seconds
- [ ] No orphaned processes
- [ ] All tests pass
- [ ] Code coverage ≥ 80%
- [ ] Existing functionality unchanged

## References

- **Specification**: `doc/tasks/close-window-on-shell-exit/SPEC.md`
- **Requirements**: `doc/tasks/close-window-on-shell-exit/要件定義書.md`
- **Existing PtyManager**: `src-tauri/src/pty/manager.rs`
- **Existing Tests**: `src-tauri/src/pty/manager.rs` (lines 100-187)
