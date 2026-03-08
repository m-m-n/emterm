# Feature: SFTP File Upload via Drag & Drop

## Overview

Add SFTP file upload capability to eMterm by allowing users to drag and drop files onto SSH-connected terminal tabs. The feature uses the external `sftp` command as a subprocess, reusing existing SSH connection settings for authentication. Non-SSH tabs receive the dropped file's path as terminal input instead.

## Objectives

- Enable file upload to remote hosts via drag & drop onto SSH tabs
- Support parallel uploads with configurable concurrency
- Support recursive directory uploads
- Provide visual feedback during drag, upload confirmation dialog, and progress display
- Paste file paths into non-SSH tabs on drop

## User Stories

### US1: Upload Files to Remote Host
As a developer, I want to drag files from my file manager onto an SSH terminal tab, so that I can upload them to the remote host without typing sftp/scp commands.

**Acceptance Criteria:**
- [ ] Dragging files over an SSH tab shows a drop overlay
- [ ] Dropping files opens a confirmation dialog with file list and destination path input
- [ ] Default destination path is the remote CWD (from OSC 7) or home directory as fallback
- [ ] Files are uploaded via sftp subprocess using the tab's SSH connection settings
- [ ] Upload progress is shown in the top-right corner of the terminal

### US2: Upload Directories Recursively
As a developer, I want to drag a directory onto an SSH tab, so that the entire directory structure is uploaded recursively.

**Acceptance Criteria:**
- [ ] Directories are uploaded using sftp `put -r`
- [ ] Directory structure is preserved on the remote host

### US3: Handle File Name Conflicts
As a developer, I want to be warned when uploading files that already exist on the remote host, so that I don't accidentally overwrite important files.

**Acceptance Criteria:**
- [ ] Before upload, remote files are checked for name conflicts via sftp `ls`
- [ ] If conflicts exist, a confirmation dialog shows the list of conflicting files
- [ ] User approves or cancels the overwrite in bulk
- [ ] After approval, upload proceeds automatically without further interaction

### US4: Drop Files on Non-SSH Tabs
As a user, I want to drop files onto a local terminal tab, so that the file path is inserted into the terminal input.

**Acceptance Criteria:**
- [ ] Dropping files on a non-SSH tab pastes the absolute file path(s) into the terminal
- [ ] No upload dialog is shown

### US5: Manage In-Progress Uploads on Tab Close
As a user, I want to be warned when closing a tab with active uploads, so that I don't accidentally cancel transfers.

**Acceptance Criteria:**
- [ ] Closing a tab with active uploads shows a confirmation dialog
- [ ] User can choose to cancel uploads (and close) or keep the tab open

## Technical Requirements

### Functional Requirements
- **FR1: File Drop Detection** - Detect file drag & drop events on the terminal area using Tauri's `onDragDropEvent` API (`@tauri-apps/api/webviewWindow`). The API provides absolute file paths directly via `DragDropEvent.payload.paths`. Note: Tauri's built-in drag-drop handler (`dragDropEnabled: true` by default) intercepts OS-level file drops before they reach the WebView, so HTML5 Drag and Drop API cannot be used.
- **FR2: Drag Overlay** - Display a full-terminal overlay during file drag with a message indicating the drop action. The overlay appears on Tauri's `enter` event and disappears on `leave` or `drop` event.
- **FR3: SSH Tab Detection** - Determine if the active tab is an SSH session by checking the profile's `ssh_connection_name` field. Non-empty value means SSH tab.
- **FR4: Upload Confirmation Dialog** - Show a modal dialog on drop (SSH tab) with: file/directory list, destination path input field (pre-filled with default), confirm and cancel buttons.
- **FR5: Remote CWD Detection** - Obtain the remote shell's current working directory from OSC 7 escape sequence. Fall back to the sftp default directory (home directory) if OSC 7 is unavailable.
- **FR6: Duplicate File Check** - Before upload, connect via sftp and run `ls` on the destination directory to check for name conflicts. If conflicts are found, display a bulk confirmation dialog listing all conflicting files.
- **FR7: SFTP Upload Execution** - Execute `sftp` as a subprocess with arguments constructed from the SSH connection settings. Upload files using sftp `put` command (or `put -r` for directories).
- **FR8: SFTP Argument Construction** - Build sftp command arguments from SshConnection settings: `sftp [-P port] [-i identity_file] [-o Key=Value ...] [user@]hostname`. The `ssh_options` array from the SSH connection is passed directly to sftp.
- **FR9: Parallel Upload Control** - Limit concurrent uploads to `sftp_max_concurrent_uploads` (default: 4). Use a pool/queue pattern: when a slot frees up, the next queued file begins uploading.
- **FR10: Progress Display** - Show upload progress in the top-right corner of the terminal as a toast/bar. The display must not block terminal interaction.
- **FR11: Non-SSH Tab File Path Paste** - When files are dropped on a non-SSH tab, insert the absolute file path(s) into the terminal input.
- **FR12: Tab Close Upload Guard** - When a tab with active uploads is being closed, show a confirmation dialog offering to cancel uploads or keep the tab open.
- **FR13: Recursive Directory Upload** - Support uploading directories recursively using sftp `put -r`.

### Non-Functional Requirements
- **NFR1 - Performance:** No file size limit imposed by eMterm; file transfer performance is delegated to sftp. Parallel upload count is configurable.
- **NFR2 - Security:** Authentication reuses SSH connection settings (key-based only). Passwords are never stored. Command arguments are passed as arrays to prevent injection. File paths are validated before use.
- **NFR3 - Usability:** Drag overlay provides clear visual feedback. Progress display does not obstruct terminal usage. Default destination path minimizes user input.
- **NFR4 - Platform Compatibility:** Support Linux and Windows where openssh sftp command is available.
- **NFR5 - Logging:** Log upload start, completion, and errors via the backend logging system.

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────────┐
│ Frontend (TypeScript)                               │
│  ├─ FileDropHandler (drag/drop events, overlay)     │
│  ├─ UploadDialog (file list, destination input)     │
│  ├─ OverwriteConfirmDialog (conflict resolution)    │
│  ├─ UploadProgressBar (top-right toast/bar)         │
│  └─ Settings UI (sftp_max_concurrent_uploads)       │
├─────────────────────────────────────────────────────┤
│ Tauri Commands (IPC)                                │
│  ├─ sftp_check_duplicates                           │
│  ├─ sftp_upload                                     │
│  └─ sftp_cancel_upload                              │
├─────────────────────────────────────────────────────┤
│ Tauri Events (Backend → Frontend)                   │
│  └─ sftp-upload-progress                            │
├─────────────────────────────────────────────────────┤
│ Backend (Rust)                                      │
│  ├─ sftp::upload    (sftp subprocess management)    │
│  ├─ sftp::progress  (stdout/stderr parsing)         │
│  └─ sftp::pool      (concurrent upload pool)        │
├─────────────────────────────────────────────────────┤
│ Existing Infrastructure                             │
│  ├─ SshConnection   (reused for sftp args)          │
│  ├─ Settings        (extended with upload config)   │
│  └─ OSC 7 Parser    (reused for remote CWD)         │
└─────────────────────────────────────────────────────┘
```

### Data Flow

**File Drop → Upload Flow:**
```
User drags file → Tauri DragDropEvent "enter" → Show overlay
User drops file → Tauri DragDropEvent "drop" → Hide overlay
  → Check ssh_connection_name on active tab's profile
    → Non-empty (SSH): Open upload dialog
      → User confirms → sftp_check_duplicates (Rust)
        → No conflicts: sftp_upload (Rust) → Progress events → UI
        → Conflicts: Show overwrite dialog → User approves → sftp_upload
    → Empty (Local): Paste file path into terminal
```

**SFTP Subprocess Flow:**
```
sftp_upload command (Rust)
  → Build sftp args from SshConnection
  → Spawn sftp subprocess with stdin pipe
  → Write "put <local_path> <remote_path>" to stdin
  → Parse stdout/stderr for progress
  → Emit sftp-upload-progress events to frontend
  → On completion: emit success/failure event
```

**Parallel Upload Pool:**
```
Upload queue: [file1, file2, file3, ..., fileN]
Active slots: [slot1, slot2, slot3, slot4] (max = sftp_max_concurrent_uploads)

On slot completion:
  → Dequeue next file
  → Start upload in freed slot
  → Update progress UI
```

### Tauri Commands

#### Command: sftp_check_duplicates

Checks for existing files at the remote destination using sftp `ls`.

**Signature:** `fn sftp_check_duplicates(ssh_connection_name: String, remote_dir: String, file_names: Vec<String>) -> Result<Vec<String>, String>`

**Parameters:**
- `ssh_connection_name`: Name of the SSH connection to use
- `remote_dir`: Remote directory path to check
- `file_names`: List of file/directory names to check

**Returns:** List of file names that already exist on the remote host.

**Implementation:** Spawns sftp subprocess, runs `ls` on the remote directory, compares output with provided file names.

#### Command: sftp_upload

Uploads a file or directory to the remote host via sftp.

**Signature:** `fn sftp_upload(session_id: String, ssh_connection_name: String, local_path: String, remote_path: String, is_directory: bool) -> Result<(), String>`

**Parameters:**
- `session_id`: Unique ID for tracking this upload session
- `ssh_connection_name`: Name of the SSH connection to use
- `local_path`: Local file/directory path
- `remote_path`: Remote destination path
- `is_directory`: Whether to use recursive upload (`put -r`)

**Returns:** Success or error.

**Side effects:** Emits `sftp-upload-progress` events during transfer.

#### Command: sftp_cancel_upload

Cancels an in-progress upload by killing the sftp subprocess.

**Signature:** `fn sftp_cancel_upload(session_id: String) -> Result<(), String>`

**Parameters:**
- `session_id`: The upload session to cancel

### Tauri Events

#### Event: sftp-upload-progress

Emitted from backend to frontend during file transfer.

**Payload:**
```rust
pub struct SftpUploadProgress {
    pub session_id: String,
    pub file_name: String,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub status: SftpUploadStatus, // "uploading" | "completed" | "failed" | "cancelled"
    pub error_message: Option<String>,
}
```

### SFTP Command Argument Construction

Build sftp command arguments from SshConnection:

```
args = []
if port != 22:            args.extend(["-P", port.to_string()])
if identity_file != "":   args.extend(["-i", expanded_identity_file])
for opt in ssh_options:    args.extend(["-o", format!("{}={}", opt.key, opt.value)])
args.push("-b")           // batch mode flag
args.push("-")            // read commands from stdin
if username != "":        args.push(format!("{}@{}", username, hostname))
else:                     args.push(hostname)
```

Note: sftp uses `-P` (uppercase) for port, unlike ssh which uses `-p` (lowercase).

### Settings Schema Changes

#### Rust: AppSettings additions

```rust
// AppSettings additions
pub struct AppSettings {
    // ... existing fields ...
    #[serde(default = "default_sftp_max_concurrent_uploads")]
    pub sftp_max_concurrent_uploads: u16,
}

fn default_sftp_max_concurrent_uploads() -> u16 {
    4
}
```

#### TypeScript: AppSettings additions

```typescript
export interface AppSettings {
  // ... existing fields ...
  sftp_max_concurrent_uploads: number;
}
```

### Dependencies

**Internal Dependencies:**
- SSH connection settings (`SshConnection` struct): Reused for sftp argument construction
- Profile system (`ssh_connection_name`): Used to detect SSH tabs
- OSC 7 parser: Reused for remote CWD detection
- Settings system: Extended with `sftp_max_concurrent_uploads`
- Settings UI: Extended with new field in SSH section

**External Dependencies:**
- External `sftp` command (openssh-client): Required at runtime
- No new Rust crate dependencies anticipated

### File Structure

```
src-tauri/src/
├── commands/
│   ├── sftp.rs              # Tauri commands: sftp_check_duplicates, sftp_upload, sftp_cancel_upload
│   └── mod.rs               # Add sftp module
├── sftp/
│   ├── mod.rs               # Module declarations
│   ├── upload.rs            # SFTP upload subprocess management
│   ├── progress.rs          # stdout/stderr parsing for progress
│   └── pool.rs              # Concurrent upload pool management
├── commands/config/
│   └── settings.rs          # Add sftp_max_concurrent_uploads

src/
├── sftp/
│   ├── file-drop-handler.ts  # Drag & drop event handling, overlay
│   ├── upload-dialog.ts      # Upload confirmation modal dialog
│   ├── overwrite-dialog.ts   # Duplicate file confirmation dialog
│   ├── upload-progress.ts    # Progress bar UI (top-right)
│   └── upload-manager.ts     # Upload queue/pool coordination
├── settings/
│   ├── types.ts              # Add sftp_max_concurrent_uploads to AppSettings
│   └── settings-sections.ts  # Add sftp setting to SSH section
```

## Test Scenarios

### Unit Tests
- [ ] SFTP argument construction with all fields (port, identity_file, ssh_options, username)
- [ ] SFTP argument construction with minimal fields (hostname only)
- [ ] SFTP argument construction with custom port uses `-P` (uppercase)
- [ ] SFTP argument construction with ssh_options passes `-o Key=Value`
- [ ] Upload pool respects max concurrent limit
- [ ] Upload pool queues files when all slots are occupied
- [ ] Upload pool starts next file when a slot is freed
- [ ] Progress parsing extracts bytes transferred from sftp output
- [ ] SSH tab detection: ssh_connection_name non-empty returns true
- [ ] SSH tab detection: ssh_connection_name empty returns false
- [ ] Duplicate check parsing extracts file names from sftp ls output
- [ ] File path paste for non-SSH tab (single file)
- [ ] File path paste for non-SSH tab (multiple files)

### Integration Tests
- [ ] Settings round-trip: save and load sftp_max_concurrent_uploads
- [ ] SFTP argument construction from saved SshConnection
- [ ] Upload session lifecycle: start, progress, complete

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with WebdriverIO + tauri-driver
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] Drag overlay appears when dragging files over SSH tab
- [ ] Upload dialog opens on file drop (SSH tab)
- [ ] sftp_max_concurrent_uploads setting is visible in SSH settings section

### Edge Cases
- [ ] Drop with no files (empty file list) - ignore
- [ ] OSC 7 not available - fall back to home directory
- [ ] sftp command not found - show error message
- [ ] Remote directory does not exist - sftp error reported to user
- [ ] Upload cancelled mid-transfer - sftp subprocess killed
- [ ] Tab closed during upload - confirmation dialog shown
- [ ] All files are duplicates - overwrite dialog shows all files
- [ ] No files are duplicates - upload proceeds without overwrite dialog
- [ ] Mixed files and directories in single drop - handle each appropriately
- [ ] Very long file path - no truncation in dialog
- [ ] Network disconnection during upload - sftp error reported to user

## Security Considerations

- **Authentication:** Reuses SSH connection key-based authentication. No new credential storage.
- **Command Injection Prevention:** sftp arguments are passed as array elements to the subprocess, never concatenated into a shell string.
- **Input Validation:** File paths and remote paths are validated before use.
- **Data Protection:** No file contents are stored locally beyond the original files. No passwords are stored.

## Error Handling

### Error Scenarios

| Scenario | Handling | User Message |
|----------|----------|--------------|
| sftp command not found | Check on upload attempt | "sftp command not found. Ensure openssh is installed." |
| SSH connection settings not found | Lookup failure | "SSH connection '{name}' not found" |
| sftp connection refused | sftp subprocess error | "Failed to connect: {sftp error output}" |
| Remote directory does not exist | sftp error | "Remote directory not found: {path}" |
| Upload failed (permission denied) | sftp error | "Upload failed: {sftp error output}" |
| Upload cancelled by user | Kill subprocess | "Upload cancelled" |
| Network disconnection | sftp subprocess exits | "Connection lost during upload: {file_name}" |
| Identity file not found | Pre-flight check | "Identity file not found: {path}" |

## Success Criteria

- [ ] All functional requirements (FR1-FR13) are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] Parallel uploads work correctly with configurable concurrency
- [ ] Upload progress is displayed in the top-right corner without blocking terminal usage
- [ ] Linux and Windows platforms supported
- [ ] Settings migration: existing settings.json loads correctly with new field defaulting

## Open Questions

> **Note**: No unresolved requirements. All questions were clarified during the specification phase.

## Implementation Phases

### Phase 1: Backend SFTP Infrastructure
**Goals:** SFTP subprocess management, argument construction, progress parsing
**Deliverables:**
- sftp module (upload, progress, pool)
- Tauri commands (sftp_check_duplicates, sftp_upload, sftp_cancel_upload)
- Settings schema addition (sftp_max_concurrent_uploads)
- Unit tests for all backend modules

### Phase 2: Frontend Drag & Drop and Dialogs
**Goals:** File drop handling, overlay, upload dialog, overwrite dialog
**Deliverables:**
- FileDropHandler (drag/drop events, overlay display)
- Upload confirmation dialog (file list, destination path input)
- Overwrite confirmation dialog (bulk conflict resolution)
- Non-SSH tab file path paste

### Phase 3: Progress Display and Upload Coordination
**Goals:** Progress UI, parallel upload management, tab close guard
**Deliverables:**
- Upload progress bar (top-right toast/bar)
- Upload manager (queue/pool coordination with frontend)
- Tab close upload guard
- Settings UI for sftp_max_concurrent_uploads

## References

- SSH connection feature: `doc/tasks/ssh-connection/SPEC.md`
- SSH connection settings: `src-tauri/src/commands/config/settings.rs` (SshConnection struct)
- Profile system: `src/settings/types.ts` (Profile interface)
- Tab drag handler (existing): `src/tab-bar/drag-handler.ts`
- OSC 7 parsing: WASM ANSI parser
