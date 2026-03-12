# Feature: File Download (emterm download)

## Overview

Add an `emterm download` CLI command that reads a file on the remote server, encodes it as base64, and outputs OSC 777 escape sequences to stdout. eMterm detects these sequences, reassembles the file, and presents a save dialog for the user to store the file locally.

## Objectives

- Enable file download from remote servers via terminal escape sequences
- Follow the existing stateless CLI pattern (same as `emterm markdown` / `emterm image`)
- Support stdin pipe input for flexible usage
- Provide progress feedback in the eMterm UI during transfer

## User Stories

### US1: Download a Remote File
As a developer connected to a remote server via SSH, I want to run `emterm download <file>` to transfer a file to my local machine, so that I don't need to use separate tools like scp.

**Acceptance Criteria:**
- [ ] `emterm download <file>` reads the file and outputs OSC sequences
- [ ] eMterm receives the sequences and shows a save dialog
- [ ] The saved file is byte-identical to the original
- [ ] Works over SSH connections

### US2: Download via Pipe
As a developer, I want to pipe command output to `emterm download --name output.txt` to save arbitrary data as a local file.

**Acceptance Criteria:**
- [ ] `cat file | emterm download --name output.txt` works
- [ ] The `--name` flag is required when reading from stdin

### US3: Progress Feedback
As a user downloading a large file, I want to see transfer progress in eMterm, so that I know the download is proceeding.

**Acceptance Criteria:**
- [ ] eMterm displays filename and progress percentage during transfer
- [ ] A completion notification is shown when download finishes

## Technical Requirements

### Functional Requirements
- **FR1: CLI Command** - Add `emterm download <file>` subcommand using clap. Read file, base64-encode, generate OSC 777 sequences, output to stdout.
- **FR2: Stdin Input** - When no file argument is given and stdin is not a TTY, read from stdin. Require `--name <filename>` flag for the save filename.
- **FR3: OSC Sequence Generation** - Generate `ESC ] 777 ; emterm ; download ; {begin|chunk|end} ; ... ESC \` sequences with UUID session ID, filename, file size, and chunked base64 data.
- **FR4: WASM Parser Extension** - Extend the WASM OSC parser to recognize `download` type sequences and invoke a TypeScript callback with session metadata and chunk data.
- **FR5: File Save Dialog** - On receiving a complete download sequence, show a Tauri save file dialog with the remote filename as default. Write the decoded file on user confirmation.
- **FR6: Progress Display** - Show a progress indicator in eMterm during chunk reception, calculated from received bytes vs total size from the begin sequence.
- **FR7: tmux Passthrough** - Wrap output in DCS passthrough when running inside tmux (reuse existing `passthrough_if_needed` logic).
- **FR8: Cancel/Discard** - If the user cancels the save dialog, discard the received data without saving.

### Non-Functional Requirements
- **NFR1 - Performance:** Chunk size of 128KB (base64-encoded) to balance throughput and terminal buffer limits. No file size limit.
- **NFR2 - Security:** Always show save dialog before writing files (user consent). Sanitize filenames: strip path separators and `..` components, use basename only.
- **NFR3 - Compatibility:** Support Linux and Windows. Support tmux DCS passthrough. Work over SSH.
- **NFR4 - Reliability:** Validate OSC sequence integrity (matching UUIDs, sequential chunk numbers). Discard incomplete transfers silently.

## Implementation Approach

### Architecture

```
Remote Server                         eMterm (Local)
┌──────────────┐                     ┌─────────────────────────────┐
│ emterm CLI   │                     │ WASM Parser                 │
│  download    │ ── OSC 777 ──────→  │  (OSC download handler)     │
│  (Rust)      │   via SSH/PTY       │         │                   │
└──────────────┘                     │         ▼                   │
                                     │ TypeScript Callback         │
                                     │  - Accumulate chunks        │
                                     │  - Track progress           │
                                     │  - Show save dialog         │
                                     │  - Write file via Tauri API │
                                     └─────────────────────────────┘
```

### OSC Sequence Format

```
Begin:
  ESC ] 777 ; emterm ; download ; begin ; id={uuid} ; name={filename} ; size={bytes} ; version=1.0 ESC \

Chunk:
  ESC ] 777 ; emterm ; download ; chunk ; id={uuid} ; seq={N} ; data={base64} ESC \

End:
  ESC ] 777 ; emterm ; download ; end ; id={uuid} ESC \
```

- `id`: UUID v4 session identifier (matches begin/chunk/end)
- `name`: Original filename (basename only, sanitized)
- `size`: Total file size in bytes (before base64 encoding)
- `seq`: 0-indexed chunk sequence number
- `data`: Base64-encoded file data chunk (up to 128KB encoded)

### Data Flow

```
CLI: Read file → base64 encode → chunk (128KB) → generate OSC sequences → stdout
     (tmux? → DCS wrap)

eMterm: PTY data → WASM parser → detect OSC 777 download → callback to TS
        → accumulate base64 chunks → decode → show save dialog → write file
```

### CLI Design

```
emterm download <FILE>
emterm download --name <NAME>   (reads from stdin)

Arguments:
  FILE    Path to the file to download

Options:
  --name <NAME>    Filename for the saved file (required when reading from stdin)
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/encoding/osc.rs`: Add `generate_download_osc()` function
- `src-tauri/src/encoding/base64.rs`: Reuse existing `encode_base64()` and `chunk_data()`
- `src-tauri/src/commands/tmux.rs`: Reuse `passthrough_if_needed()`
- `wasm/src/parser/osc.rs`: Extend OSC handler for download type
- `src/terminal/wasm/`: Add download callback handling

**External Dependencies:**
- `uuid` (existing): For session ID generation
- `clap` (existing): For CLI argument parsing
- Tauri dialog API: For save file dialog

### File Structure

```
src-tauri/src/
├── commands/
│   ├── download.rs         # CLI download command implementation
│   └── mod.rs              # Add download module
├── encoding/
│   └── osc.rs              # Add generate_download_osc()
├── main.rs                 # Add download subcommand

wasm/src/
├── parser/
│   └── osc.rs              # Extend OSC parser for download type
├── callbacks.rs            # Add download callback

src/
├── terminal/
│   └── download.ts         # Download handler (accumulate, save, progress)
```

## Test Scenarios

### Unit Tests
- [ ] `generate_download_osc()` produces correct begin/chunk/end sequences
- [ ] Chunk splitting works correctly for various file sizes
- [ ] Filename sanitization strips path components and `..`
- [ ] Empty file produces begin + end with no chunks
- [ ] stdin detection works (TTY vs pipe)

### Integration Tests
- [ ] `execute_download_command()` reads a file and outputs valid OSC sequences
- [ ] File not found returns appropriate error code
- [ ] Directory argument returns error
- [ ] Permission denied returns error

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/*.e2e.js`
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] Empty file (0 bytes): Should produce begin + end with size=0, no chunks
- [ ] File with special characters in name: Sanitized properly
- [ ] Very large file: Chunks are generated correctly, no memory issues (streaming)
- [ ] Filename with path traversal (`../../etc/passwd`): Basename only used

## Security Considerations

- **User Consent:** Save dialog is always shown; user must explicitly choose save location
- **Filename Sanitization:** Strip directory components, reject or sanitize `..`, use basename only
- **Input Validation:** Validate OSC sequence format, discard malformed sequences
- **No Auto-Save:** Never write files without user interaction

## Error Handling

### Error Codes

| Error | Condition | Exit Code | Message |
|-------|-----------|-----------|---------|
| FileNotFound | File does not exist | 2 | File not found: {path} |
| NotAFile | Path is a directory | 2 | Not a file: {path} |
| PermissionDenied | No read permission | 2 | Permission denied: {path} |
| ReadError | I/O error during read | 2 | Failed to read file: {error} |
| NameRequired | stdin mode without --name | 2 | --name is required when reading from stdin |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Downloaded files are byte-identical to originals
- [ ] Works over SSH with tmux passthrough
- [ ] Save dialog is always shown (security)
- [ ] Progress is displayed during transfer
- [ ] Code review is completed

## Open Questions

None.
