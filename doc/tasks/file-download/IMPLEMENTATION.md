# Implementation Plan: File Download (emterm download)

## Overview

Add an `emterm download` CLI command that reads a file, base64-encodes it into OSC 777 escape sequences, and outputs them to stdout. eMterm detects these sequences via the WASM parser, reassembles the file, shows a save dialog, and writes the file locally.

## Objectives

- Enable file download from remote servers via terminal escape sequences
- Follow the existing stateless CLI pattern (same architecture as `emterm markdown`)
- Provide progress feedback during transfer

## Prerequisites

### Development Environment
- Rust toolchain, wasm-pack, Bun, Tauri CLI (existing)

### Dependencies
- `uuid` crate (existing) for session ID generation
- `clap` crate (existing) for CLI argument parsing
- `tauri-plugin-dialog` for native save file dialog (new dependency)

## Architecture Overview

### Technology Stack
- **Backend CLI**: Rust (clap subcommand, base64 encoding, OSC generation)
- **Parser**: Rust/WASM (OSC 777 download handler, callback dispatch)
- **Frontend**: Vanilla TypeScript (chunk accumulation, progress display, save dialog)
- **IPC**: Tauri commands (file write after dialog confirmation)

### Design Approach

Reuse the proven three-phase OSC 777 protocol (begin/chunk/end) from the markdown feature. The download command mirrors the markdown command's encoding pipeline. On the frontend, a new `DownloadSessionManager` (modeled after `MarkdownSessionManager`) accumulates chunks and triggers the save flow.

### Component Interaction

```
CLI (Rust)
  │  Read file → base64 encode → chunk → OSC 777 begin/chunk/end → stdout
  │  (tmux? → DCS passthrough wrap)
  ▼
PTY binary channel
  ▼
WASM Parser (osc_handler.rs)
  │  Detect "download" verb in OSC 777 emterm extension
  │  Fire osc_callback(100, "emterm;download;begin|chunk|end;...")
  ▼
TypeScript OSC dispatch (osc_handlers.ts)
  │  Route "download" verb to DownloadSessionManager
  ▼
DownloadSessionManager
  │  Accumulate base64 chunks by UUID
  │  Track progress (received bytes / total size)
  │  On "end": decode base64 → binary data
  ▼
Save flow
  │  Show native save dialog (Tauri dialog plugin)
  │  User confirms → write file via Tauri fs API
  │  User cancels → discard data
  ▼
Progress UI
  │  Toast-style indicator (reuse SFTP progress pattern)
  │  Show filename, percentage, completion notification
```

## Implementation Phases

### Phase 1: CLI Command & OSC Generation

**Goal**: `emterm download <file>` reads a file and outputs valid OSC 777 download sequences to stdout. `emterm download --name <name>` reads from stdin.

**Files to Create**:
- `src-tauri/src/commands/download.rs` - CLI download command entry point and execution logic

**Files to Modify**:
- `src-tauri/src/commands/mod.rs` - Register download module
- `src-tauri/src/main.rs` - Add `download` subcommand to clap definition
- `src-tauri/src/encoding/osc.rs` - Add OSC generation function for download sequences

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| download subcommand (clap) | Parse `<FILE>` arg and `--name` option | Valid CLI invocation | Arguments available for command execution |
| execute_download_command | Orchestrate file read, encoding, output | File exists and is readable (or stdin available) | OSC sequences written to stdout, exit code 0 |
| generate_download_osc | Produce begin/chunk/end OSC 777 sequences | Raw file bytes and metadata | Valid OSC sequence string with chunked base64 |
| filename sanitizer | Strip path components, reject traversal | Raw filename from argument or --name | Basename-only string, no path separators or `..` |

**Processing Flow**:
1. Parse CLI arguments
   - File argument provided → validate file exists, is a file, is readable
   - No file argument → check stdin is not TTY → require `--name` flag → read stdin to buffer
   - Error conditions → print message to stderr, exit with appropriate code
2. Extract or derive filename
   - File argument → basename of path
   - Stdin → value of `--name` flag
   - Sanitize: strip path separators and `..` components
3. Read file content (or stdin buffer)
4. Generate OSC 777 download sequences (reuse base64 chunking from markdown)
5. Apply tmux DCS passthrough wrapping if needed (reuse `passthrough_if_needed`)
6. Write sequences to stdout

**Implementation Steps**:
1. **Add generate_download_osc function** - Similar to existing generate_markdown_osc but with download verb and additional size parameter in begin sequence
2. **Add filename sanitization** - Extract basename, strip traversal components
3. **Create download command module** - Read file/stdin, validate inputs, call generation function, handle tmux wrapping, write stdout
4. **Register in main.rs** - Add clap subcommand definition with FILE arg and --name option
5. **Add unit tests** - OSC generation, chunking, filename sanitization, error conditions

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: OSC sequence format validation, chunk splitting for various sizes, filename sanitization, empty file handling
- Integration: Full command execution with temp files, error cases (not found, directory, permission denied)

**Acceptance Criteria**:
- [ ] `emterm download <file>` outputs valid OSC 777 download sequences
- [ ] `cat file | emterm download --name out.txt` reads stdin and outputs sequences
- [ ] Missing `--name` with stdin produces exit code 2
- [ ] File errors produce exit code 1 with descriptive message
- [ ] Filename sanitization strips path components and traversal
- [ ] tmux passthrough wrapping works when `$TMUX` is set
- [ ] Empty file produces begin + end with size=0, no chunks

**Estimated Effort**: medium

---

### Phase 2: WASM Parser Extension

**Goal**: The WASM parser recognizes OSC 777 download sequences and dispatches them to the TypeScript layer via the existing callback mechanism.

**Files to Modify**:
- `wasm/src/osc_handler.rs` - Add download verb routing in emterm extension handler

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OSC 777 emterm extension router | Route "download" verb alongside existing "markdown" verb | OSC 777 with "emterm" prefix parsed | Callback fired with action type 100 and download data string |

**Processing Flow**:
1. WASM parser detects OSC 777 sequence
2. osc_handler routes to emterm extension handler (action type 100)
3. Existing callback mechanism fires with full data string including "download" verb
4. No new action type needed - reuse type 100 (EmtermExtension), let TypeScript route by verb

**Implementation Steps**:
1. **Verify existing routing** - Confirm OSC 777 emterm extension handler already passes through arbitrary verbs (download data will flow through as-is if handler doesn't filter)
2. **Add download verb handling if needed** - Only if the existing handler filters to known verbs
3. **Add WASM-level tests** - Verify download sequences pass through callback correctly

**Dependencies**: None (parallel with Phase 1)

**Testing Approach**:
- Unit: Parse OSC 777 download begin/chunk/end sequences, verify callback invocation with correct data

**Acceptance Criteria**:
- [ ] OSC 777 download begin/chunk/end sequences trigger callback
- [ ] Data string preserves all parameters (id, name, size, seq, data)
- [ ] Existing markdown sequences continue to work (no regression)

**Estimated Effort**: small

---

### Phase 3: Frontend Download Handler & Save Dialog

**Goal**: eMterm accumulates download chunks, decodes the file, shows a native save dialog, writes the file, and displays progress throughout.

**Files to Create**:
- `src/download/session.ts` - DownloadSessionManager: chunk accumulation, session tracking, progress calculation
- `src/download/progress.ts` - Download progress UI (toast-style, modeled after SFTP progress pattern)
- `src/download/index.ts` - Module barrel export

**Files to Modify**:
- `src/terminal/handlers/osc_handlers.ts` - Route "download" verb to DownloadSessionManager
- `src/terminal-app/index.ts` - Initialize DownloadSessionManager, wire into OSC dispatch
- `src/index.html` or app initialization - Ensure Tauri dialog plugin is available
- `src-tauri/Cargo.toml` - Add `tauri-plugin-dialog` dependency
- `src-tauri/src/app.rs` - Register dialog plugin
- `package.json` - Add `@tauri-apps/plugin-dialog` npm dependency

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DownloadSessionManager | Manage active download sessions, accumulate chunks, validate sequence integrity | Initialized and wired to OSC dispatch | Complete file data available after end sequence |
| DownloadSession | Track single download: UUID, filename, expected size, chunks, received bytes | Created on "begin" sequence | Holds accumulated base64 data and metadata |
| DownloadProgressDisplay | Show toast with filename, percentage bar, completion/cancel state | Active download session exists | Visual feedback visible to user |
| Save flow | Decode base64, show native save dialog, write file | All chunks received, end sequence processed | File written to user-chosen path or discarded on cancel |

**Processing Flow**:
1. "begin" sequence received
   - Create new DownloadSession with UUID, filename, expected size
   - Show progress toast with filename and 0%
   - Condition: duplicate UUID → discard (already active session)
2. "chunk" sequence received
   - Validate UUID matches active session
   - Validate seq is sequential
   - Accumulate base64 data
   - Update progress: (received encoded bytes / expected encoded size) percentage
   - Condition: unknown UUID → discard silently
   - Condition: out-of-order seq → discard entire session
3. "end" sequence received
   - Validate UUID matches active session
   - Concatenate all base64 chunks → decode to binary
   - Show native save dialog with filename as default
   - User confirms → write binary data to chosen path
   - User cancels → discard data
   - Update progress to completed or cancelled
   - Clean up session
4. Timeout handling
   - If no chunks received for extended period → discard session silently

**Implementation Steps**:
1. **Add Tauri dialog plugin** - Add dependency to Cargo.toml and package.json, register plugin
2. **Create DownloadSessionManager** - Session lifecycle, chunk accumulation, integrity validation
3. **Create DownloadProgressDisplay** - Toast UI with filename, percentage, auto-dismiss
4. **Wire into OSC dispatch** - Route "download" verb from handleEmtermExtension to session manager
5. **Implement save flow** - Base64 decode, native save dialog, file write via Tauri API
6. **Initialize in TerminalApp** - Create manager instance, connect to OSC handler chain

**Dependencies**: Phase 2 (WASM passes download sequences through)

**Testing Approach**:
- Unit: Session accumulation logic, base64 decode, filename handling, progress calculation, integrity validation (UUID mismatch, out-of-order seq)
- E2E (Docker): Regression - existing E2E tests still pass
- Manual: Full download flow over SSH, save dialog interaction, cancel behavior, progress display, tmux passthrough

**Acceptance Criteria**:
- [ ] Download begin creates session and shows progress
- [ ] Chunks accumulate correctly with progress updates
- [ ] End triggers base64 decode and save dialog
- [ ] Saved file is byte-identical to original
- [ ] Cancel discards data without writing
- [ ] Mismatched UUID or out-of-order seq discards session
- [ ] Progress shows filename and percentage

**Estimated Effort**: large

---

## Complete File Structure

```
src-tauri/
├── Cargo.toml                          # Add tauri-plugin-dialog dependency
├── src/
│   ├── main.rs                         # Add download subcommand
│   ├── app.rs                          # Register dialog plugin
│   ├── commands/
│   │   ├── mod.rs                      # Register download module
│   │   └── download.rs                 # NEW: CLI download command
│   └── encoding/
│       └── osc.rs                      # Add generate_download_osc()

wasm/src/
├── osc_handler.rs                      # Extend emterm extension routing (if needed)

src/
├── download/
│   ├── index.ts                        # NEW: Module barrel export
│   ├── session.ts                      # NEW: DownloadSessionManager
│   └── progress.ts                     # NEW: Download progress UI
├── terminal/
│   └── handlers/
│       └── osc_handlers.ts             # Route download verb
├── terminal-app/
│   └── index.ts                        # Initialize DownloadSessionManager

package.json                            # Add @tauri-apps/plugin-dialog
```

## Testing Strategy

- **Unit (Rust CLI)**: OSC generation, chunking, sanitization - target 90%+ for new code
- **Unit (WASM)**: Download sequence parsing and callback dispatch
- **Unit (TypeScript)**: Session management, chunk accumulation, progress calculation, integrity validation
- **Integration (Rust)**: Full command execution with real files
- **E2E (Docker)**: Regression testing only (existing tests pass)
- **Manual**: Full download flow, save dialog, cancel, progress, SSH, tmux

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `tauri-plugin-dialog` | latest compatible | Native save file dialog |
| `@tauri-apps/plugin-dialog` | latest compatible | TypeScript bindings for dialog plugin |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Large file memory pressure (frontend) | Medium | Medium | Stream base64 chunks to temp storage if file exceeds threshold; initial implementation keeps in memory |
| Base64 decode performance for very large files | Low | Medium | Decode in chunks rather than concatenating all base64 first |
| OSC sequence truncation by terminal multiplexers | Low | High | Chunk size of 128KB is well within common limits; tmux passthrough tested |
| Dialog plugin compatibility (Linux + Windows) | Low | Medium | tauri-plugin-dialog is official and well-tested on both platforms |

## Open Questions

None.

## Success Metrics

- [ ] All functional requirements (FR1-FR8) implemented and tested
- [ ] Downloaded files are byte-identical to originals
- [ ] Works over SSH with tmux passthrough
- [ ] Save dialog always shown before writing (security)
- [ ] Progress displayed during transfer
- [ ] No regression in existing features
