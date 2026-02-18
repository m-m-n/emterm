# Implementation Plan: Key Input Performance Optimization

## Overview

Optimize the PTY write path to achieve key repeat throughput comparable to other modern terminal emulators (WezTerm, Alacritty). The primary bottleneck is the per-keystroke lock contention chain in the Rust backend (3 sequential lock acquisitions per write).

## Objectives

- Eliminate multi-level lock contention on the PTY write path
- Reduce Tauri command handler overhead for pty_write
- Maintain single-key latency (no degradation)
- Preserve IME compatibility and all existing functionality

## Prerequisites

### Development Environment
- Rust 1.85+, Bun, Tauri CLI
- Existing Docker E2E test infrastructure

### Dependencies
- `tokio` (already present) - for unbounded MPSC channel with blocking receiver support
- No new external dependencies required

## Architecture Overview

### Technology Stack
- **Backend**: Rust + Tauri v2 (PTY write path)
- **Frontend**: TypeScript (PtyClient, KeyboardHandler)
- **Key Libraries**: tokio (MPSC channels), portable-pty (PTY I/O)

### Design Approach

Replace the current lock-heavy synchronous write path with a channel-based architecture where each PTY session has a dedicated writer thread consuming from an MPSC channel.

### Component Interaction

**Current write path (3 lock acquisitions per keystroke):**

```
invoke("pty_write")
  → [Lock 1] PtyManager.sessions RwLock (read)
  → [Lock 2] Arc<Mutex<PtySession>> (async lock)
  → [Lock 3] PtySession.writer StdMutex (sync lock)
  → write_all() + flush()
  → return response (unused by fire-and-forget)
```

**Proposed write path (1 lightweight read + lock-free send):**

```
pty_write command (synchronous, non-async)
  → [Lock 1] WriterRegistry RwLock (read) - session-id → sender lookup
  → channel send (lock-free operation)
  → return immediately

Writer Thread (per session, dedicated):
  → blocking receive from channel
  → write_all() + flush() on PTY writer (exclusive ownership, no locks)
```

## Implementation Phases

### Phase 1: Backend Write Channel Architecture

**Goal**: Replace the lock-heavy PTY write path with a channel-based architecture that eliminates per-keystroke lock contention.

**Files to Create**:
- `src-tauri/src/pty/writer.rs` - Writer thread and channel registry

**Files to Modify**:
- `src-tauri/src/pty/mod.rs` - Add writer module, re-export types
- `src-tauri/src/pty/session.rs` - Extract writer handle at creation for channel ownership
- `src-tauri/src/pty/manager.rs` - Integrate writer registry, manage writer lifecycle
- `src-tauri/src/lib.rs` - Convert pty_write to synchronous command using writer registry

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| WriterRegistry | Map session IDs to write channel senders | PtyManager initialized | Lookup returns sender for valid session ID |
| WriterThread | Consume from channel, write to PTY, handle errors | Session spawned, writer handle extracted | Data written to PTY in order, thread exits on channel close |
| PtySession (modified) | Expose writer handle extraction for channel ownership | Session created with PTY pair | Writer handle transferred to WriterThread exclusively |

**Processing Flow** (diagram-convertible):

1. Session spawn
   - Create PTY session (existing flow)
   - Extract writer handle from session
   - Create MPSC channel (unbounded sender + receiver)
   - Spawn dedicated writer thread with receiver + writer handle
   - Register sender in WriterRegistry keyed by session ID
2. PTY write (per keystroke)
   - Lookup sender in WriterRegistry by session ID
     - Found → send data through channel → return success
     - Not found → return session-not-found error
3. Writer thread loop
   - Blocking receive from channel
     - Data received → write_all + flush to PTY writer
     - Channel closed (sender dropped) → exit thread
     - Write error → log error, continue (PTY may be closing)
4. Session cleanup
   - Remove sender from WriterRegistry (drops sender)
   - Writer thread detects closed channel, exits naturally
   - Existing session cleanup flow continues (kill, event emission)

**Implementation Steps** (high-level):

1. **Create WriterRegistry** - Define a synchronized map (session ID → channel sender) with methods for register, lookup, and remove
2. **Create WriterThread** - Define the writer thread function that owns the PTY writer handle exclusively and consumes from the channel receiver
3. **Modify PtySession** - Add method to extract/take the writer handle so it can be transferred to the writer thread (writer is no longer accessed through PtySession.write())
4. **Integrate into PtyManager** - Add WriterRegistry as a field, manage lifecycle (register on create, remove on session close)
5. **Convert pty_write command** - Change from async command with 3-lock chain to synchronous command with single registry lookup + channel send

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: WriterRegistry lookup for valid/invalid session IDs
- Unit: Writer thread graceful shutdown on channel close
- Integration: pty_write delivers data correctly through channel to PTY
- Integration: Session lifecycle (create → write → close) works end-to-end

**Acceptance Criteria**:
- [ ] pty_write command handler acquires at most 1 read lock per call
- [ ] Write data reaches PTY correctly and in order
- [ ] Session cleanup properly shuts down writer thread
- [ ] Existing Rust tests pass

**Estimated Effort**: medium

---

### Phase 2: Frontend Serialization Optimization

**Goal**: Reduce the per-keystroke serialization overhead in the frontend IPC path by eliminating the unnecessary `Array.from()` conversion.

**Files to Modify**:
- `src/pty/client.ts` - Optimize PtyClient.write() serialization

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| PtyClient.write() | Send key input data to backend with minimal overhead | Session active, data is Uint8Array or string | Data sent via IPC without unnecessary conversion |

**Processing Flow** (diagram-convertible):

1. PtyClient.write(data) called
   - If string → encode to bytes
   - Send bytes directly to backend (avoid intermediate Array conversion)
   - Fire-and-forget (no await, existing pattern preserved)

**Implementation Steps** (high-level):

1. **Evaluate Tauri's Uint8Array serialization** - Determine if Tauri v2's invoke can accept Uint8Array directly without manual Array.from() conversion, or if a more efficient serialization exists
2. **Optimize write() method** - Remove `Array.from(bytes)` conversion if Tauri supports direct Uint8Array, or replace with minimal-overhead alternative
3. **Verify IME write path** - Ensure the IME handler's text-to-PTY write path also benefits from optimization

**Dependencies**: Phase 1 (backend must handle the data format)

**Testing Approach**:
- Unit: PtyClient.write() sends correct byte sequences
- Integration: Key input end-to-end through optimized path
- Manual: IME input (Japanese) works correctly

**Acceptance Criteria**:
- [ ] No `Array.from()` conversion in the hot path (or justified if unavoidable)
- [ ] Existing TypeScript tests pass
- [ ] Type check passes

**Estimated Effort**: small

---

### Phase 3: Verification and Performance Validation

**Goal**: Validate that key repeat performance meets the target (comparable to WezTerm/Alacritty) and no regressions are introduced.

**Files to Create**: None

**Files to Modify**: None (testing/verification only)

**Implementation Steps** (high-level):

1. **Run existing test suites** - Rust tests, TypeScript tests, type check
2. **Manual performance comparison** - Compare key repeat speed with WezTerm/Alacritty using identical OS key repeat settings
3. **IME compatibility verification** - Test Japanese input with IME in both EditContext and textarea fallback modes
4. **Multi-tab verification** - Test key input across tab switches, concurrent sessions
5. **Edge case testing** - Rapid key switching, session close during input, paste operations

**Dependencies**: Phase 1, Phase 2

**Testing Approach**:
- Manual: Side-by-side key repeat comparison with WezTerm
- Manual: IME input testing (composition, conversion, commit)
- Manual: Multi-tab input isolation
- Automated: Full test suite (Rust + TypeScript)

**Acceptance Criteria**:
- [ ] Key repeat speed is perceptually equivalent to WezTerm/Alacritty
- [ ] Single-key latency is not degraded
- [ ] IME input works correctly
- [ ] All keybindings and shortcuts work
- [ ] All existing tests pass

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/pty/
├── mod.rs               # Add writer module and re-exports
├── writer.rs            # NEW: WriterRegistry + WriterThread
├── session.rs           # Modified: writer handle extraction
├── manager.rs           # Modified: WriterRegistry integration
├── shell.rs             # No change
└── graceful_shutdown.rs # No change (may need minor update for writer cleanup)

src-tauri/src/
└── lib.rs               # Modified: pty_write command simplified

src/pty/
├── client.ts            # Modified: serialization optimization
└── keyboard.ts          # No change
```

## Testing Strategy

- **Unit**: WriterRegistry operations, writer thread lifecycle, session writer extraction (80%+ coverage)
- **Integration**: Full write path from command handler through channel to PTY
- **Manual**: Key repeat performance comparison, IME compatibility, multi-tab input

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tokio | 1.x (existing) | MPSC channel with blocking receive support |

No new external dependencies required.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Writer thread does not exit cleanly on session close | Low | Medium | Channel close propagation ensures natural shutdown; add timeout fallback |
| Data ordering issues with channel-based writes | Low | High | MPSC channel guarantees FIFO ordering; single sender per session |
| Tauri sync command has unexpected limitations | Low | Medium | Fallback to async command with minimal lock path |
| Array.from() removal breaks Tauri serialization | Medium | Low | Test with Tauri's actual serialization behavior; keep Array.from() if required |

## Open Questions

- None. All requirements are resolved.

## Success Metrics

- [ ] Key repeat throughput comparable to WezTerm/Alacritty (manual comparison)
- [ ] Single-key latency not degraded
- [ ] All existing tests pass (Rust + TypeScript)
- [ ] IME compatibility maintained
- [ ] No new external dependencies added
