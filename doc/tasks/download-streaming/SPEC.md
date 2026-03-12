# Feature: Download Streaming

## Overview

Refactor the file download feature to use streaming I/O, eliminating the requirement to hold the entire file in memory on both the sending (CLI) and receiving (frontend) sides. This removes the current 500MB limit and enables downloading files of arbitrary size with constant memory usage.

## Current Architecture (Problem)

```
CLI (sender):
  read_to_end()        → entire file in memory
  base64_encode(all)   → +4/3× in memory
  chunk(encoded_str)   → output to stdout

Frontend (receiver):
  accumulate chunks    → all base64 in Map
  join + atob()        → +decoded copy in memory
  invoke IPC           → base64 string sent again to backend
  backend decode+write → final file
```

A 2GB file requires ~4.7GB RAM on each side. Frontend hard-caps at 500MB.

## New Architecture (Streaming)

```
CLI (sender):
  loop:
    read N bytes       → fixed buffer (8MiB raw → ~10.7MiB base64)
    base64_encode(buf) → encode in-place
    output OSC chunk   → flush to stdout
  Memory: O(chunk_size) constant

Frontend (receiver):
  on begin  → invoke start_download_file (opens save dialog, returns handle)
  on chunk  → invoke append_download_chunk (decode + append to file)
  on end    → invoke finish_download_file (close handle)
  Memory: O(chunk_size) constant

Backend (Tauri):
  start_download_file   → save dialog → open file → return handle ID
  append_download_chunk → decode base64 → write to file
  finish_download_file  → close file handle
```

## Functional Requirements

### FR1: CLI Streaming Read & Encode

Modify `execute_download_command()` and `execute_download_from_stdin()` to read the file in fixed-size chunks (8MiB raw ≈ 10.7MiB base64), encode each chunk individually, and output the OSC sequence immediately before reading the next chunk. Peak memory usage must not exceed a small constant regardless of file size.

Chunk size rationale: 8MiB raw produces ~10.7MiB base64 + OSC header (~80 bytes), well within the WASM parser's `MAX_OSC_LEN = 16MiB` limit with comfortable margin.

For file path input, file size is known from metadata. For stdin input, size is unknown; set `size=0` in the begin sequence and determine total size after all data is read.

**Note on stdin**: stdin cannot be streamed because the total size is unknown upfront and must be sent in the begin sequence. Continue to buffer stdin fully before output. This is acceptable because piped data is typically small.

### FR2: Backend Streaming File Write

Add three new Tauri commands to replace `write_download_file`:

- `start_download_file(filename: String) → Result<StartDownloadResult, String>`
  - Show native save dialog with `filename` as default
  - If user confirms, open file for writing, store handle in a session registry
  - Return `{ id: String, path: String }` on confirm, or `null` on cancel

- `append_download_chunk(id: String, data_base64: String) → Result<(), String>`
  - Look up file handle by `id`
  - Decode base64, write bytes to file
  - Update last-activity timestamp

- `finish_download_file(id: String) → Result<(), String>`
  - Flush and close the file handle
  - Remove from session registry

### FR3: Frontend Streaming Session

Modify `DownloadSessionManager` to:

- On `begin`: invoke `start_download_file` to show save dialog and get a handle ID. If the user cancels, discard the session immediately.
- On `chunk`: invoke `append_download_chunk` with the handle ID and base64 data. Do not accumulate chunks in memory.
- On `end`: invoke `finish_download_file` to close the handle.

Remove the `chunks: Map<number, string>` accumulation. Track only `receivedBytes` for progress calculation.

### FR4: Remove 500MB Limit

Remove the `MAX_DOWNLOAD_SIZE` constant from the frontend. File size is no longer bounded by memory. The backend file write is the only storage constraint.

### FR5: Backend Session Registry & Cleanup

The backend must maintain a registry of active download file handles (HashMap keyed by session ID). Implement:

- Maximum concurrent sessions: 10
- Idle timeout: 120 seconds (longer than frontend's 60s to avoid premature close)
- Cleanup on app exit (drop all handles)

### FR6: Error Recovery

- If `append_download_chunk` fails (disk full, I/O error), close and delete the partial file. Notify frontend.
- If frontend session times out or is discarded, invoke `finish_download_file` or a new `cancel_download_file` command to clean up the backend handle and delete the partial file.
- If the app crashes, OS handles are released automatically. Partial files remain on disk (acceptable).

### FR7: Deprecate write_download_file

Remove the old `write_download_file` Tauri command after migration. It is no longer needed.

## Non-Functional Requirements

- **NFR1 - Memory**: CLI and frontend memory usage must be O(chunk_size), not O(file_size).
- **NFR2 - Throughput**: No regression in transfer speed for files under 100MB.
- **NFR3 - Compatibility**: OSC sequence format is unchanged. Existing begin/chunk/end structure is preserved. `version=1.0` remains valid.
- **NFR4 - UX**: Save dialog appears at the start of download (on begin), not at the end. Progress display continues to work.

## OSC Sequence Format (Unchanged)

```
Begin: ESC ] 777 ; emterm ; download ; begin ; id={uuid} ; name={filename} ; size={bytes} ; version=1.0 ESC \
Chunk: ESC ] 777 ; emterm ; download ; chunk ; id={uuid} ; seq={N} ; data={base64} ESC \
End:   ESC ] 777 ; emterm ; download ; end ; id={uuid} ESC \
```

No protocol changes required.

## Data Flow (New)

```
CLI:
  file.read(96KB) → base64(128KB) → OSC chunk → stdout → flush
  (repeat until EOF)

eMterm frontend:
  begin  → invoke start_download_file → save dialog → handle ID
  chunk  → invoke append_download_chunk(handle, base64) → backend decodes + writes
  end    → invoke finish_download_file(handle) → close

Backend:
  start  → dialog → File::create → registry.insert(id, file)
  append → registry.get(id) → base64_decode → file.write_all
  finish → registry.remove(id) → file.flush → drop
```

## Implementation Scope

### Files to Modify

| File | Change |
|------|--------|
| `src-tauri/src/commands/download.rs` | Streaming file read + chunk-by-chunk output |
| `src-tauri/src/encoding/osc.rs` | Add `generate_download_osc_begin/chunk/end` individual functions |
| `src-tauri/src/tauri_commands.rs` | Replace `write_download_file` with 3 streaming commands |
| `src/download/session.ts` | Remove chunk accumulation, call streaming IPC |
| `src/download/session.test.ts` | Update tests for new flow |

### Files to Add

| File | Purpose |
|------|---------|
| `src-tauri/src/download_registry.rs` | Backend file handle registry with timeout cleanup |

### Files Unchanged

| File | Reason |
|------|--------|
| `wasm/src/parser.rs` | OSC format unchanged, parser already works |
| `src/download/progress.ts` | Progress display API unchanged |
| `src/download/index.ts` | Module exports unchanged |
| `src/terminal-app/index.ts` | Integration point unchanged |

## Security Considerations

- Save dialog is shown at the beginning of the download (before any data is written). User consent is required.
- Backend file handle registry prevents leaked handles via timeout.
- Filename sanitization is unchanged.
- Backend session IDs are UUID v4, unpredictable.
- `cancel_download_file` deletes partial files to avoid leaving untrusted incomplete data on disk.

## Test Scenarios

### Unit Tests (Rust)
- Streaming read produces correct OSC sequence for small file
- Streaming read produces correct OSC sequence for file requiring multiple chunks
- Empty file produces begin + end with no chunks
- Individual OSC begin/chunk/end generators produce correct format
- Download registry: insert, get, remove, timeout cleanup
- Download registry: max sessions limit

### Unit Tests (TypeScript)
- Session lifecycle: begin → chunk → end with mocked IPC
- User cancel on save dialog discards session
- Out-of-order chunk detection still works
- Session timeout triggers cancel_download_file
- Progress calculation without chunk accumulation
- Multiple concurrent sessions

### Integration Tests (Rust)
- CLI streaming output for small file matches expected OSC format
- CLI streaming output for large file (>128KB) has correct chunk count
- stdin mode still works (buffered)
- File not found / permission denied errors unchanged

## Error Handling

| Scenario | Handling |
|----------|----------|
| User cancels save dialog | `start_download_file` returns null, frontend discards session |
| Disk full during append | Backend returns error, frontend shows error, partial file deleted |
| Invalid base64 in chunk | Backend returns decode error, frontend discards session, partial file deleted |
| Frontend session timeout | Frontend invokes `cancel_download_file`, backend deletes partial file |
| Backend handle timeout (120s) | Backend closes handle, deletes partial file |
| Out-of-order chunks | Frontend discards session, invokes `cancel_download_file` |
| App crash | OS releases handles; partial files remain (acceptable) |
