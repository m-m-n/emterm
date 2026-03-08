# Implementation Plan: SFTP File Upload via Drag & Drop

## Overview

Add SFTP file upload capability to eMterm by allowing users to drag and drop files onto SSH-connected terminal tabs. The feature uses the external `sftp` command as a subprocess, reusing existing SSH connection settings. Non-SSH tabs receive the dropped file's path as terminal input.

## Objectives

- Enable file upload to remote hosts via drag & drop onto SSH tabs
- Support parallel uploads with configurable concurrency
- Support recursive directory uploads
- Provide visual feedback during drag, confirmation dialog, and progress display
- Paste file paths into non-SSH tabs on drop

## Prerequisites

### Development Environment

- Rust toolchain (existing)
- Bun (existing)
- Docker for testing (existing)

### Dependencies

- External `sftp` command (openssh-client) at runtime
- No new Rust crate dependencies
- Existing SSH connection settings (`SshConnection` struct)
- Existing OSC 7 working directory tracking (`TerminalState.workingDirectory`)
- Existing profile system (`Profile.ssh_connection_name`)

## Architecture Overview

### Technology Stack

- **Backend**: Rust (Tauri) - SFTP subprocess management, progress parsing, upload pool
- **Frontend**: TypeScript - Drag & drop handling, dialogs, progress UI, upload coordination
- **IPC**: Tauri commands (frontend -> backend) and Tauri events (backend -> frontend)

### Design Approach

- **Subprocess delegation**: All file transfer is delegated to the external `sftp` command. No SSH/SFTP protocol implementation in eMterm.
- **Reuse existing infrastructure**: SSH connection settings, OSC 7 CWD, profile SSH detection are all reused without modification.
- **Frontend-driven coordination**: The frontend manages the upload workflow (dialog flow, queue management), while the backend handles subprocess lifecycle and progress reporting.
- **Event-driven progress**: Backend emits progress events during transfer; frontend renders them non-blockingly.

### Component Interaction

```
Frontend                          Backend
--------                          -------
FileDropHandler
  |-- detects drag/drop
  |-- checks ssh_connection_name
  |-- shows overlay
  |
UploadDialog
  |-- displays file list
  |-- gets destination path
  |
UploadManager
  |-- calls sftp_check_duplicates --> sftp module (spawns sftp, runs ls)
  |                               <-- returns conflicting file names
  |
OverwriteConfirmDialog
  |-- user approves
  |
UploadManager
  |-- calls sftp_upload (per file) --> sftp::pool (manages concurrent slots)
  |                                    sftp::upload (spawns sftp subprocess)
  |                                    sftp::progress (parses stdout/stderr)
  |                               <-- sftp-upload-progress events
  |
UploadProgressBar
  |-- renders progress toast
```

## Implementation Phases

### Phase 1: Backend SFTP Infrastructure

**Goal**: Implement SFTP subprocess management, argument construction, progress parsing, concurrent upload pool, and Tauri command/event interfaces. All backend functionality is testable via unit tests.

**Files to Create**:
- `src-tauri/src/sftp/mod.rs` - Module declarations and shared types (upload status enum, progress payload struct)
- `src-tauri/src/sftp/args.rs` - SFTP command argument construction from SshConnection settings
- `src-tauri/src/sftp/upload.rs` - SFTP subprocess lifecycle management (spawn, stdin commands, kill)
- `src-tauri/src/sftp/progress.rs` - stdout/stderr line parsing for transfer progress extraction
- `src-tauri/src/sftp/pool.rs` - Concurrent upload pool with configurable slot limit and queue
- `src-tauri/src/sftp/check.rs` - Duplicate file checking via sftp `ls` output parsing
- `src-tauri/src/commands/sftp.rs` - Tauri commands: sftp_check_duplicates, sftp_upload, sftp_cancel_upload

**Files to Modify**:
- `src-tauri/src/lib.rs` - Add `sftp` module declaration
- `src-tauri/src/commands/mod.rs` - Add `sftp` command module
- `src-tauri/src/commands/config/settings.rs` - Add `sftp_max_concurrent_uploads` field to AppSettings
- `src-tauri/src/app.rs` - Register new Tauri commands and manage sftp pool state

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| args | Build sftp CLI arguments from SshConnection | Valid SshConnection with hostname | Array of arguments ready for subprocess spawn |
| upload | Manage single sftp subprocess lifecycle | Valid args, local file path, remote path | Subprocess spawned, stdin commands written, completion/error reported |
| progress | Parse sftp stdout/stderr lines | Raw output line from sftp process | Extracted progress info (bytes, percentage) or None if unparseable |
| pool | Manage concurrent upload slots | Max concurrent limit set | Files queued and dispatched to free slots, events emitted per file |
| check | Check remote file existence | Valid SSH connection, remote directory, file name list | List of names that already exist |

**Processing Flow** (diagram-convertible):
1. Frontend calls `sftp_check_duplicates` with connection name, remote dir, file names
   - Backend resolves SshConnection from settings
   - Backend spawns sftp subprocess, sends `ls` command to stdin
   - Backend parses output, returns list of existing file names
2. Frontend calls `sftp_upload` with session_id, connection name, local/remote paths, is_directory flag
   - Backend enqueues upload request to pool
   - Pool assigns to available slot or queues
   - When slot available: spawn sftp subprocess, write `put` (or `put -r`) command
   - Parse progress from stdout/stderr, emit `sftp-upload-progress` events
   - On completion/error: free slot, dequeue next, emit final status event
3. Frontend calls `sftp_cancel_upload` with session_id
   - Backend kills the sftp subprocess for that session
   - Emits cancelled status event

**Implementation Steps**:
1. **Settings extension** - Add `sftp_max_concurrent_uploads` (u16, default 4) to AppSettings with serde defaults and null handling
2. **Argument builder** - Implement sftp argument construction: port (-P), identity file (-i), ssh_options (-o), batch mode (-b -), user@host
3. **Subprocess management** - Implement sftp process spawn, stdin command writing (put, put -r, ls), process termination
4. **Progress parser** - Parse sftp stdout/stderr output to extract transfer progress information
5. **Upload pool** - Implement concurrent slot management with configurable limit and FIFO queue
6. **Tauri commands and events** - Wire up sftp_check_duplicates, sftp_upload, sftp_cancel_upload commands and sftp-upload-progress event emission

**Dependencies**: None (foundational phase)

**Testing Approach**:
- Unit: Argument construction with various SshConnection configurations (full fields, minimal, custom port, ssh_options)
- Unit: Progress line parsing (valid progress lines, error lines, unparseable lines)
- Unit: Pool slot management (enqueue, dequeue, respect max limit, slot freed triggers next)
- Unit: Duplicate check output parsing (file names from sftp ls output)
- Integration: Settings round-trip with new field

**Acceptance Criteria**:
- [ ] sftp arguments correctly constructed for all SshConnection field combinations
- [ ] Upload pool respects configured concurrent limit
- [ ] Progress events emitted during file transfer
- [ ] Upload can be cancelled mid-transfer
- [ ] Settings migration: existing settings.json loads correctly with new field defaulting to 4

**Estimated Effort**: large

---

### Phase 2: Frontend Drag & Drop and Dialogs

**Goal**: Implement file drop detection, drag overlay, SSH tab routing, upload confirmation dialog, overwrite confirmation dialog, and non-SSH tab file path paste. User can complete the full drag-drop-confirm workflow.

**Files to Create**:
- `src/sftp/file-drop-handler.ts` - Drag & drop event handling via Tauri `onDragDropEvent` API (enter/leave/drop), overlay display, SSH tab routing
- `src/sftp/upload-dialog.ts` - Upload confirmation modal: file list display, destination path input, confirm/cancel
- `src/sftp/overwrite-dialog.ts` - Duplicate file confirmation modal: conflicting file list, bulk approve/cancel

**Files to Modify**:
- `src/terminal-app/index.ts` - Initialize FileDropHandler, wire up to terminal area
- `src/styles.css` - Add styles for drag overlay and dialog components (following UI-DESIGN-GUIDELINES.yaml)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FileDropHandler | Detect file drag/drop via Tauri `onDragDropEvent`, show overlay, route by tab type | Attached via `getCurrentWebviewWindow().onDragDropEvent()` | Drop triggers dialog (SSH) or path paste (non-SSH) |
| UploadDialog | Collect upload confirmation from user | File list and default destination path provided | User confirms with destination path, or cancels |
| OverwriteConfirmDialog | Collect overwrite approval from user | List of conflicting file names provided | User approves bulk overwrite, or cancels |

**Processing Flow** (diagram-convertible):
1. User drags files over terminal area
   - Tauri DragDropEvent "enter" -> show overlay with upload message
   - Tauri DragDropEvent "leave" -> hide overlay
2. User drops files (Tauri DragDropEvent "drop")
   - hide overlay
   - Extract absolute file paths from `event.payload.paths`
   - Check active tab's profile ssh_connection_name
     - Non-empty (SSH): open UploadDialog
     - Empty (non-SSH): write file paths to PTY via pty_write
3. UploadDialog flow
   - Display file/directory list
   - Pre-fill destination path from TerminalState.workingDirectory (OSC 7) or empty for sftp default (home)
   - User confirms -> return (files, destination) to caller
   - User cancels -> abort

**Implementation Steps**:
1. **File drop handler** - Register Tauri `onDragDropEvent` on the current webview window, handle file path extraction from `event.payload.paths`, manage overlay visibility. Note: `attach()` is async as `onDragDropEvent()` returns a Promise.
2. **SSH tab detection** - Read active tab's profile ssh_connection_name to determine routing
3. **Non-SSH path paste** - On drop to non-SSH tab, write space-separated absolute paths to PTY
4. **Upload dialog** - Create modal dialog with file list, destination path input (pre-filled from OSC 7), confirm/cancel buttons
5. **Overwrite dialog** - Create modal dialog showing conflicting file names with bulk approve/cancel
6. **Styling** - Add CSS for overlay (full-terminal, semi-transparent) and dialogs following UI-DESIGN-GUIDELINES.yaml tokens

**Dependencies**: Requires Phase 1 (settings field for concurrency, but dialogs can be built independently of backend commands)

**Testing Approach**:
- Unit: SSH tab detection logic (ssh_connection_name empty vs non-empty)
- Unit: File path paste formatting (single file, multiple files)
- E2E (Docker): Drag overlay appears on file drag over terminal
- E2E (Docker): Upload dialog opens on file drop to SSH tab
- Manual: Visual appearance of overlay and dialogs

**Acceptance Criteria**:
- [ ] Drag overlay appears on dragenter, disappears on dragleave/drop
- [ ] SSH tab drop opens upload dialog with file list and destination input
- [ ] Non-SSH tab drop pastes absolute file paths into terminal
- [ ] Default destination path populated from OSC 7 when available
- [ ] Overwrite dialog shows conflicting file names with bulk approve/cancel

**Estimated Effort**: medium

---

### Phase 3: Progress Display and Upload Coordination

**Goal**: Implement the upload progress UI, end-to-end upload manager coordinating frontend and backend, tab close guard for active uploads, and settings UI for concurrency configuration. Full feature is functional.

**Files to Create**:
- `src/sftp/upload-progress.ts` - Progress bar/toast UI in top-right corner, per-file status display
- `src/sftp/upload-manager.ts` - Upload orchestrator: connects dialogs to backend commands, manages upload lifecycle

**Files to Modify**:
- `src/terminal-app/index.ts` - Wire UploadManager into terminal app lifecycle, register tab close guard
- `src/tab-bar/tab-bar-ui.ts` - Add tab close guard check for active uploads
- `src/settings/types.ts` - Add `sftp_max_concurrent_uploads` to AppSettings interface
- `src/settings/settings-sections.ts` - Add sftp concurrency setting to SSH section
- `src/i18n/locales/en.json` - Add English strings for sftp UI
- `src/i18n/locales/ja.json` - Add Japanese strings for sftp UI
- `src-tauri/locales/en.json` - Add English strings for backend sftp messages
- `src-tauri/locales/ja.json` - Add Japanese strings for backend sftp messages
- `src/styles.css` - Add styles for progress bar

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UploadManager | Orchestrate full upload workflow | FileDropHandler triggers with files and tab info | Files uploaded via backend, progress shown, completion notified |
| UploadProgressBar | Display non-blocking progress toast | Upload in progress, receiving progress events | Visual progress in top-right corner, does not block terminal |
| Tab close guard | Warn user on close with active uploads | Upload manager tracks active uploads per tab | Confirmation dialog shown, user chooses cancel or keep |

**Processing Flow** (diagram-convertible):
1. UploadManager receives confirmed upload (files, destination, connection name)
   - Call sftp_check_duplicates with file names
     - Conflicts found: show OverwriteConfirmDialog
       - Approved: proceed to upload
       - Rejected: abort
     - No conflicts: proceed to upload
   - For each file: call sftp_upload with unique session_id
   - Listen for sftp-upload-progress events
   - Update UploadProgressBar per event
   - On all complete: show completion notification
2. Tab close intercepted
   - Check UploadManager for active uploads on this tab
     - Active uploads exist: show confirmation dialog
       - Cancel uploads: call sftp_cancel_upload for each, allow tab close
       - Keep tab: dismiss dialog
     - No active uploads: allow tab close normally

**Implementation Steps**:
1. **Upload manager** - Implement orchestrator connecting dialog results to backend sftp commands, tracking active uploads per tab
2. **Progress bar UI** - Implement top-right toast/bar showing per-file progress, overall status, following UI-DESIGN-GUIDELINES.yaml
3. **Event listener** - Subscribe to sftp-upload-progress Tauri events, dispatch to progress bar
4. **Tab close guard** - Intercept tab close to check for active uploads, show confirmation dialog
5. **Settings UI** - Add sftp_max_concurrent_uploads number input to SSH settings section
6. **i18n strings** - Add all user-facing strings for sftp feature in both English and Japanese

**Dependencies**: Requires Phase 1 (backend commands) and Phase 2 (dialogs and drop handler)

**Testing Approach**:
- Unit: Upload lifecycle state transitions (queued, uploading, completed, failed, cancelled)
- Integration: Upload session lifecycle (start, progress events, complete)
- E2E (Docker): sftp_max_concurrent_uploads setting visible in SSH section
- E2E (Docker): Existing E2E tests pass without regression
- Manual: Progress bar visual appearance and non-blocking behavior
- Manual: Tab close guard dialog interaction

**Acceptance Criteria**:
- [ ] Upload progress displayed in top-right corner without blocking terminal
- [ ] Parallel uploads respect sftp_max_concurrent_uploads setting
- [ ] Tab close with active uploads shows confirmation dialog
- [ ] sftp_max_concurrent_uploads configurable in SSH settings section
- [ ] All i18n strings present in both en and ja
- [ ] Existing E2E tests pass without regression

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
  sftp/
    mod.rs               - Module declarations, shared types (status enum, progress payload)
    args.rs              - SFTP argument construction from SshConnection
    upload.rs            - SFTP subprocess lifecycle (spawn, command, kill)
    progress.rs          - stdout/stderr parsing for progress extraction
    pool.rs              - Concurrent upload pool with slot management
    check.rs             - Duplicate file checking via sftp ls
  commands/
    sftp.rs              - Tauri commands (sftp_check_duplicates, sftp_upload, sftp_cancel_upload)
    mod.rs               - (modify) Add sftp module
  commands/config/
    settings.rs          - (modify) Add sftp_max_concurrent_uploads
  lib.rs                 - (modify) Add sftp module declaration
  app.rs                 - (modify) Register commands, manage pool state

src/
  sftp/
    file-drop-handler.ts - Drag & drop events, overlay, SSH tab routing
    upload-dialog.ts     - Upload confirmation modal (file list, destination)
    overwrite-dialog.ts  - Duplicate file confirmation modal
    upload-progress.ts   - Progress bar UI (top-right toast)
    upload-manager.ts    - Upload orchestrator (dialog -> backend coordination)
  terminal-app/
    index.ts             - (modify) Initialize sftp components
  tab-bar/
    tab-bar-ui.ts        - (modify) Tab close guard for active uploads
  settings/
    types.ts             - (modify) Add sftp_max_concurrent_uploads
    settings-sections.ts - (modify) Add sftp setting to SSH section
  i18n/locales/
    en.json              - (modify) Add sftp UI strings
    ja.json              - (modify) Add sftp UI strings
  styles.css             - (modify) Add overlay, dialog, progress bar styles

src-tauri/locales/
  en.json                - (modify) Add sftp backend strings
  ja.json                - (modify) Add sftp backend strings
```

## Testing Strategy

- **Unit tests**: Core backend logic (argument construction, progress parsing, pool management, duplicate checking) - target 90%+ coverage. Frontend SSH detection and path paste logic.
- **Integration tests**: Settings round-trip, upload session lifecycle.
- **E2E (Docker)**: Overlay appearance, dialog opening, settings UI visibility, regression of existing tests.
- **Manual**: Visual appearance of overlay/dialogs/progress, non-blocking behavior, tab close guard UX.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| openssh (sftp) | System | Runtime dependency for file transfer |

No new Rust crate or npm dependencies required. All functionality built on existing standard library and Tauri APIs.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| sftp progress output format varies across versions/platforms | Medium | Medium | Parse conservatively; treat unparseable output as "in progress" without percentage |
| OSC 7 not emitted by remote shell | Medium | Low | Fall back to sftp default directory (home); document requirement |
| sftp command not available on user's system | Low | High | Check sftp availability on first upload attempt; show clear error message |
| Large directory upload saturates subprocess management | Low | Medium | Pool pattern limits concurrency; each directory is one sftp subprocess |
| Tauri drag & drop event limitations on different platforms | Low | Medium | Use Tauri's `onDragDropEvent` API which provides consistent behavior and absolute file paths across platforms |

## Open Questions

- None. All questions were resolved during the specification phase.

## Success Metrics

- [ ] All functional requirements (FR1-FR13) implemented and tested
- [ ] All unit test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] Parallel uploads work correctly with configurable concurrency
- [ ] Upload progress displayed without blocking terminal usage
- [ ] Linux and Windows platforms supported
- [ ] Settings migration: existing settings.json loads with new field defaulting to 4
