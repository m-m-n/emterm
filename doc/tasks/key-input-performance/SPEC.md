# Feature: Key Input Performance Optimization

## Overview

Optimize the key input pipeline in eMterm to achieve key repeat throughput comparable to other modern terminal emulators (WezTerm, Alacritty). The current implementation suffers from per-keystroke IPC overhead, JSON serialization costs, and multiple lock acquisitions in the Rust backend.

## Objectives

- Achieve key repeat speed comparable to WezTerm/Alacritty
- Reduce per-keystroke IPC overhead
- Eliminate unnecessary serialization in the input path
- Reduce lock contention in the Rust backend
- Maintain single-key latency (no degradation)
- Maintain full IME compatibility

## User Stories

### US1: Fast Key Repeat
As a terminal user, I want key repeat to be as fast as other terminal emulators, so that holding down a key produces characters at the expected rate.

**Acceptance Criteria:**
- [ ] Key repeat throughput matches WezTerm/Alacritty
- [ ] No perceivable difference in key repeat speed compared to native terminals

### US2: Responsive Single Key Press
As a terminal user, I want single key presses to remain responsive after optimization, so that the optimization does not degrade interactive typing experience.

**Acceptance Criteria:**
- [ ] Single key latency is not increased
- [ ] No additional delay on first keypress

### US3: IME Compatibility
As a Japanese-speaking user, I want IME input to work correctly after optimization, so that I can continue typing in Japanese without issues.

**Acceptance Criteria:**
- [ ] EditContext API mode works correctly
- [ ] Textarea fallback mode works correctly
- [ ] Composition, conversion, and commit work as before

## Technical Requirements

### Functional Requirements
- **FR1:** Replace JSON-based invoke IPC with a lower-overhead mechanism for PTY writes
- **FR2:** Eliminate Uint8Array-to-number[] conversion (`Array.from(bytes)`) in the write path
- **FR3:** Reduce lock acquisition overhead in the Rust pty_write handler
- **FR4:** Maintain fire-and-forget semantics for key input (no await blocking)
- **FR5:** Preserve IME input path (EditContext and textarea fallback) without modification

### Non-Functional Requirements
- **NFR1 - Throughput:** Key repeat throughput comparable to WezTerm/Alacritty
- **NFR2 - Latency:** Single-key input latency must not increase
- **NFR3 - CPU:** Minimize additional CPU usage during key repeat
- **NFR4 - Compatibility:** All existing keybindings and shortcuts must continue to work
- **NFR5 - Stability:** No regressions in existing test suite

## Implementation Approach

### Architecture

**Current Input Pipeline:**
```
keydown event
  → keyEventToBytes() [keyboard.ts]
  → invoke("pty_write", { sessionId, data: Array.from(bytes) }) [client.ts]
  → JSON serialize → Tauri IPC → JSON deserialize
  → pty_write command handler [lib.rs]
    → PtyManager.get_session() [async RwLock]
    → PtySession lock [async Mutex]
    → writer.write_all() + flush() [sync StdMutex]
  → Response back to frontend (ignored by fire-and-forget)
```

**Optimized Input Pipeline:**
```
keydown event
  → keyEventToBytes() [keyboard.ts]
  → emit("pty-write", { sessionId, data: Uint8Array }) [client.ts]
  → Binary/minimal-overhead IPC
  → pty_write event handler [lib.rs]
    → Direct writer access (reduced locking)
    → writer.write_all() + flush()
  → No response needed (true fire-and-forget)
```

### Key Optimization Strategies

#### Strategy 1: Event-based IPC instead of Command invoke

Replace `invoke("pty_write")` with Tauri event emission or a dedicated write channel.

**Rationale:**
- `invoke` is request-response: even though we don't await, the runtime still processes the response
- Events are true fire-and-forget with lower overhead
- Tauri's event system supports binary payloads

**Candidate approaches (to be evaluated in implementation planning):**
- Tauri `emit` with binary payload
- Tauri `Channel` for frontend-to-backend streaming
- Raw WebView message passing

#### Strategy 2: Binary Serialization

Eliminate `Array.from(bytes)` conversion and JSON serialization.

**Rationale:**
- Current path: `Uint8Array → Array.from() → number[] → JSON string → parse → Vec<u8>`
- Optimal path: `Uint8Array → binary transfer → Vec<u8>` (zero-copy or minimal-copy)
- The project already uses binary IPC for WASM ANSI parser output (precedent exists)

#### Strategy 3: Lock Contention Reduction

Reduce the number and cost of lock acquisitions per write.

**Current lock chain (per keystroke):**
1. `PtyManager.sessions` - async RwLock (read)
2. `PtySession` - async Mutex (lock)
3. `PtySession.writer` - sync StdMutex (lock)

**Optimization approaches (to be evaluated):**
- Cache session handle to skip manager lookup
- Use dedicated write channel per session (bypasses session-level lock)
- Combine writer access pattern to reduce lock scope

### Data Flow

```
User Key Press
  → [Frontend] keydown event handler
  → [Frontend] keyEventToBytes() - encode to terminal bytes
  → [Frontend] Low-overhead IPC send (binary, fire-and-forget)
  → [Backend] Event/channel handler
  → [Backend] Direct PTY write (minimal locking)
  → [PTY] write_all + flush
  → [PTY] Shell/application receives input
```

### Dependencies

**Internal Dependencies:**
- `src/terminal-app/handlers/keyboard.ts`: Key event handling
- `src/pty/client.ts`: PtyClient.write() method
- `src/pty/keyboard.ts`: Key-to-bytes conversion (unchanged)
- `src-tauri/src/lib.rs`: pty_write command handler
- `src-tauri/src/pty/session.rs`: PtySession.write()
- `src-tauri/src/pty/manager.rs`: PtyManager session lookup

**External Dependencies:**
- `tauri` crate: IPC mechanism (events, channels, commands)
- `portable-pty` crate: PTY write interface (unchanged)

### File Structure

```
src/
├── pty/
│   ├── client.ts              # PtyClient.write() - IPC mechanism change
│   └── keyboard.ts            # keyEventToBytes() - no change expected
├── terminal-app/
│   └── handlers/
│       ├── keyboard.ts        # KeyboardHandler - may need minor updates
│       └── ime.ts             # IME handler - no change (compatibility)
src-tauri/
├── src/
│   ├── lib.rs                 # pty_write handler - new event/channel handler
│   └── pty/
│       ├── session.rs         # PtySession.write() - possible optimization
│       └── manager.rs         # PtyManager - possible optimization
```

## Test Scenarios

### Unit Tests
- [ ] Key-to-bytes conversion produces correct output (existing tests, no change)
- [ ] New IPC mechanism correctly delivers bytes to PTY
- [ ] Write operations complete without errors under rapid input

### Integration Tests
- [ ] Key repeat at maximum OS rate is handled without dropped keystrokes
- [ ] Multi-tab input isolation works with new IPC mechanism
- [ ] Session lifecycle (create/destroy) works with new write path

### Edge Cases
- [ ] Very rapid key repeat (OS maximum rate) does not cause buffer overflow
- [ ] Switching tabs during key repeat transitions cleanly
- [ ] PTY close during write does not cause crash or hang
- [ ] Multiple simultaneous key holds (e.g., arrow + modifier) work correctly

### Compatibility Tests
- [ ] IME composition and commit work correctly (EditContext mode)
- [ ] IME composition and commit work correctly (textarea fallback mode)
- [ ] All keybindings (copy, paste, search, tab management) work
- [ ] Control characters (Ctrl+C, Ctrl+D, etc.) work
- [ ] Application cursor keys mode (DECCKM) works
- [ ] Clipboard paste (large text chunking) still works

## Performance Optimization

### Performance Goals
- Key repeat throughput: Comparable to WezTerm/Alacritty (no perceivable difference)
- Single-key latency: No increase from current baseline
- CPU usage: Minimal increase during key repeat

### Measurement
- Manual comparison with WezTerm/Alacritty using same OS key repeat settings
- Profiling IPC round-trip time before and after optimization

## Success Criteria

- [ ] Key repeat speed is perceptually equivalent to other modern terminals
- [ ] Single-key latency is not degraded
- [ ] IME (Japanese input) works correctly
- [ ] All existing keybindings and shortcuts work
- [ ] All existing tests pass
- [ ] No regressions in terminal functionality

## Open Questions

> **Note**: All requirements are resolved. No TBD items.
