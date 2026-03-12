# Implementation Plan: Download Streaming

## Overview

Refactor file download to use streaming I/O on both CLI (sender) and frontend/backend (receiver), eliminating the need to hold the entire file in memory. This removes the 500MB limit and enables arbitrary-size file downloads with constant memory usage.

## Objectives

- Constant O(chunk_size) memory on CLI sender and frontend receiver
- Backend file handle registry with streaming write, timeout, and cleanup
- Save dialog shown at download start (on `begin`), not at end
- No protocol changes (OSC sequence format unchanged)
- Remove the deprecated `write_download_file` command

## Prerequisites

### Development Environment
- Rust toolchain (src-tauri)
- Bun (frontend)
- Docker for testing

### Dependencies
- No new external dependencies required
- Existing: `uuid`, `base64`, `tauri-plugin-dialog`, `tokio`

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri commands, file I/O, session registry)
- **Frontend**: TypeScript (session manager, IPC calls)
- **CLI**: Rust (streaming file read + OSC output)

### Design Approach

The change splits the monolithic read-encode-output pipeline into a chunked streaming pipeline on the CLI side, and replaces the accumulate-join-send pattern on the frontend with immediate per-chunk IPC to the backend.

**CLI side**: Read file in 8MiB chunks, base64-encode each, output as individual OSC sequences, flush, repeat. stdin remains buffered (size unknown upfront).

**Frontend side**: On `begin`, invoke backend to show save dialog and open file. On each `chunk`, invoke backend to decode+write. On `end`, invoke backend to close file. No data accumulation in memory.

**Backend side**: New session registry holds open file handles keyed by session ID. Three new Tauri commands replace the single `write_download_file`.

### Component Interaction

```
CLI:
  File/stdin -> [8MiB buffer] -> base64 encode -> OSC sequence -> stdout (per chunk)

Frontend (DownloadSessionManager):
  begin  -> start_download_file IPC -> backend opens file, returns handle ID
  chunk  -> append_download_chunk IPC -> backend decodes + writes
  end    -> finish_download_file IPC -> backend closes file

Backend (DownloadRegistry):
  Manages HashMap<String, OpenFileHandle> with timeout cleanup
```

## Implementation Phases

### Phase 1: Backend Download Registry and Streaming Commands

**Goal**: Create the backend file handle registry and three new Tauri commands that enable streaming file writes.

**Files to Create**:
- `src-tauri/src/download_registry.rs` - File handle registry with session management, timeout, and cleanup

**Files to Modify**:
- `src-tauri/src/tauri_commands.rs` - Add `start_download_file`, `append_download_chunk`, `finish_download_file`, `cancel_download_file` commands; remove `write_download_file`
- `src-tauri/src/app.rs` - Register new commands in invoke_handler, add DownloadRegistry to managed state
- `src-tauri/src/lib.rs` - Add `pub mod download_registry` (under `#[cfg(feature = "gui")]`)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DownloadRegistry | Manage open file handles with insert/get/remove/cleanup | Valid session ID (UUID) | File handle stored/retrieved/removed |
| start_download_file | Show save dialog, open file, register handle | Filename string | Returns {id, path} or null (cancelled) |
| append_download_chunk | Decode base64 chunk, write to file | Valid handle ID, valid base64 | Bytes written to file |
| finish_download_file | Flush and close file handle, remove from registry | Valid handle ID | File closed, registry entry removed |
| cancel_download_file | Close handle, delete partial file, remove from registry | Valid handle ID | Partial file deleted, registry entry cleaned |

**Processing Flow**:
1. `start_download_file` invoked with filename
   - Session count >= 10 -> return error
   - Show native save dialog with filename as default
   - User cancels -> return null
   - User confirms -> open file for writing, generate handle ID, store in registry with timestamp -> return {id, path}
2. `append_download_chunk` invoked with handle ID and base64 data
   - Handle not found -> return error
   - Decode base64 -> write bytes to file -> update last-activity timestamp
   - I/O error -> close handle, delete partial file, remove from registry, return error
3. `finish_download_file` invoked with handle ID
   - Flush file -> close handle -> remove from registry
4. `cancel_download_file` invoked with handle ID
   - Close handle -> delete partial file -> remove from registry
5. Background cleanup: periodically check for handles idle > 120s, close and delete

**Implementation Steps**:
1. **Create DownloadRegistry** - Session registry holding file handles, paths, and timestamps with insert/get/remove/cleanup-expired operations. Max 10 concurrent sessions, 120s idle timeout.
2. **Implement start_download_file command** - Save dialog via tauri-plugin-dialog, file creation, registry insertion. Return result type with id and path.
3. **Implement append_download_chunk command** - Registry lookup, base64 decode, file write. On error: cleanup partial file.
4. **Implement finish_download_file command** - Flush, close, remove from registry.
5. **Implement cancel_download_file command** - Close handle, delete file at stored path, remove from registry.
6. **Remove write_download_file** - Delete the old command from tauri_commands.rs and app.rs invoke_handler registration.
7. **Register state and commands** - Add DownloadRegistry as Tauri managed state, register all four new commands in app.rs.

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: Registry insert/get/remove/timeout-cleanup, max sessions enforcement
- Integration: Commands with mock file system (if feasible), or end-to-end in Phase 3

**Acceptance Criteria**:
- [ ] DownloadRegistry correctly manages concurrent sessions with timeout
- [ ] start_download_file shows dialog and returns handle or null
- [ ] append_download_chunk decodes and writes to file
- [ ] finish_download_file flushes and closes cleanly
- [ ] cancel_download_file deletes partial file
- [ ] write_download_file removed from codebase
- [ ] All Rust tests pass

**Estimated Effort**: medium

---

### Phase 2: CLI Streaming Read and OSC Generation

**Goal**: Modify CLI download command to read files in 8MiB chunks and output OSC sequences incrementally, achieving constant memory usage.

**Files to Modify**:
- `src-tauri/src/commands/download.rs` - Replace `read_to_end` with chunked read loop; change chunk size from 128KB to 8MiB raw
- `src-tauri/src/encoding/osc.rs` - Add individual OSC begin/chunk/end generation functions (existing `generate_download_osc` generates all at once)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| generate_download_osc_begin | Generate begin OSC sequence string | Session ID, filename, file size | Valid OSC begin sequence |
| generate_download_osc_chunk | Generate single chunk OSC sequence string | Session ID, seq number, base64 data | Valid OSC chunk sequence |
| generate_download_osc_end | Generate end OSC sequence string | Session ID | Valid OSC end sequence |
| execute_download_command (modified) | Stream file in 8MiB chunks | Valid file path | OSC sequences written to stdout with constant memory |
| execute_download_from_stdin (unchanged) | Buffer stdin fully, then output | stdin data | OSC sequences (buffered, size known after read) |

**Processing Flow**:
1. Open file, read metadata for size
2. Generate session UUID
3. Output begin OSC sequence (with known file size) -> flush
4. Loop:
   - Read up to 8MiB into fixed buffer
   - If 0 bytes read -> break
   - Base64 encode the buffer
   - Output chunk OSC sequence with seq number -> flush
   - Increment seq
5. Output end OSC sequence -> flush
6. For stdin: continue buffering fully (size unknown upfront), then output all sequences

**Implementation Steps**:
1. **Add individual OSC generators** - Three functions in osc.rs for begin/chunk/end that return a single OSC sequence string each
2. **Change DOWNLOAD_CHUNK_SIZE** - From 128KB (base64) to 8MiB (raw bytes). This is now a raw read size, not a base64 chunk size.
3. **Refactor execute_download_command** - Replace read_to_end with a read loop using fixed buffer. Output each OSC sequence immediately and flush.
4. **Keep execute_download_from_stdin unchanged** - stdin must be fully buffered (spec note). May refactor to use new individual generators for consistency.
5. **Keep generate_download_osc for backward compatibility** - Or remove if stdin path is refactored to use individual generators.

**Dependencies**: None (independent of Phase 1, but both needed for Phase 3)

**Testing Approach**:
- Unit: Individual OSC begin/chunk/end generators produce correct format
- Unit: Streaming read for small file produces begin + chunk(s) + end
- Unit: Streaming read for multi-chunk file has correct chunk count and seq numbers
- Unit: Empty file produces begin + end with no chunks
- Integration: CLI output for known file content matches expected OSC format

**Acceptance Criteria**:
- [ ] Individual OSC generators produce correct format matching existing protocol
- [ ] File read uses fixed-size buffer (8MiB), not read_to_end
- [ ] Each chunk is flushed to stdout immediately
- [ ] stdin mode continues to work (buffered)
- [ ] Empty file handled correctly (begin + end, no chunks)
- [ ] All Rust tests pass

**Estimated Effort**: small

---

### Phase 3: Frontend Streaming Session

**Goal**: Modify frontend DownloadSessionManager to invoke streaming backend commands instead of accumulating chunks in memory. Remove the 500MB size limit.

**Files to Modify**:
- `src/download/session.ts` - Replace chunk accumulation with per-chunk IPC; invoke streaming commands on begin/chunk/end; remove MAX_DOWNLOAD_SIZE
- `src/download/session.test.ts` - Update all tests for new streaming flow with mocked IPC

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DownloadSessionManager (modified) | Orchestrate streaming download via IPC | OSC begin/chunk/end events | Backend file written incrementally |
| DownloadSession (modified) | Track handle ID and received bytes (no chunk storage) | Valid session from begin | Progress tracked, no data accumulated |

**Processing Flow**:
1. On `begin`:
   - Invoke `start_download_file` with filename
   - If backend returns null (user cancelled) -> discard session, show cancelled
   - If backend returns {id, path} -> store handle ID in session
2. On `chunk`:
   - Validate sequential ordering (existing logic, but track by counter not Map size)
   - Invoke `append_download_chunk` with handle ID and base64 data
   - If error -> discard session, invoke `cancel_download_file`
   - Update receivedBytes for progress (do not store chunk data)
3. On `end`:
   - Invoke `finish_download_file` with handle ID
   - Show completed progress
4. On timeout/discard:
   - Invoke `cancel_download_file` to clean up backend handle

**Implementation Steps**:
1. **Modify DownloadSession interface** - Remove `chunks: Map<number, string>`, add `handleId: string | null` and `nextSeq: number` for ordering validation
2. **Make handleBegin async** - Invoke `start_download_file`, handle cancel/confirm, store handle ID
3. **Make handleChunk async** - Invoke `append_download_chunk` instead of storing in Map. Validate seq with counter.
4. **Simplify handleEnd** - Invoke `finish_download_file` instead of joining chunks and calling write_download_file
5. **Update discardSession** - Invoke `cancel_download_file` if handle ID exists
6. **Remove MAX_DOWNLOAD_SIZE** - Remove constant and all size-limit checks (both in begin and chunk handlers)
7. **Update tests** - Mock the four new IPC commands; verify streaming lifecycle; verify cancel on dialog dismiss; verify error recovery

**Dependencies**: Phase 1 (backend commands must exist)

**Testing Approach**:
- Unit: Session lifecycle begin -> chunk -> end with mocked IPC (verify correct commands invoked)
- Unit: User cancel on save dialog discards session immediately
- Unit: Out-of-order chunk detection still works (counter-based)
- Unit: Session timeout triggers cancel_download_file
- Unit: Progress calculation works without chunk accumulation
- Unit: Multiple concurrent sessions
- Unit: append_download_chunk error triggers cancel and discard

**Acceptance Criteria**:
- [ ] No chunk data stored in frontend memory (Map removed)
- [ ] Save dialog shown on begin (not end)
- [ ] Each chunk immediately forwarded to backend via IPC
- [ ] MAX_DOWNLOAD_SIZE removed
- [ ] Session timeout invokes cancel_download_file
- [ ] Out-of-order detection works via seq counter
- [ ] All TypeScript tests pass
- [ ] Typecheck passes

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
  download_registry.rs   (NEW)  - Backend file handle registry
  tauri_commands.rs       (MOD)  - New streaming commands, old command removed
  app.rs                  (MOD)  - Command registration, managed state
  lib.rs                  (MOD)  - Module declaration
  commands/download.rs    (MOD)  - Chunked file read
  encoding/osc.rs         (MOD)  - Individual OSC generators

src/download/
  session.ts              (MOD)  - Streaming session manager
  session.test.ts         (MOD)  - Updated tests
  progress.ts             (UNCHANGED)
  index.ts                (UNCHANGED)
```

## Testing Strategy

- **Unit (Rust)**: Registry operations, OSC generators, streaming read logic. Target 90%+ on new code.
- **Unit (TypeScript)**: Session lifecycle, IPC mock verification, error paths. Target 90%+ on new code.
- **Integration (Rust)**: CLI output format validation for various file sizes.
- **E2E (Docker)**: Full download flow with small file (CLI -> terminal -> save).
- **Manual**: Large file download (>500MB) to verify memory stays constant; save dialog UX.

## Dependencies

No new external packages required. All functionality uses existing dependencies.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Save dialog blocking during download start | Low | Medium | Dialog is shown once at begin; async await prevents deadlock |
| File handle leak on unexpected disconnect | Medium | Medium | 120s timeout cleanup + app exit cleanup |
| IPC overhead per chunk | Low | Low | 8MiB chunks mean few IPC calls even for large files |
| Breaking change if old CLI used with new frontend | Low | High | OSC protocol is unchanged; only IPC layer changes |

## Open Questions

- None identified. All requirements are clearly specified.

## Success Metrics

- [ ] Files of any size can be downloaded (tested with >500MB)
- [ ] Memory usage stays constant regardless of file size
- [ ] No throughput regression for files under 100MB
- [ ] Save dialog appears at start of download
- [ ] Progress bar works correctly
- [ ] Partial files cleaned up on error/cancel
