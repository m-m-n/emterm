# Feature: SFTP Upload — native-poc Port

## Overview

Port the WebView build's SFTP file-upload feature (drag & drop → dialog →
subprocess management → progress toasts) to the `native-poc` crate
(egui/winit native terminal). The source core (`src-tauri/src/sftp/*`) is
Tauri-independent pure Rust and is reused almost verbatim; the Tauri-specific
IPC command layer, event emission, and WebView UI are replaced by an in-process
service, a `crossbeam_channel` progress bridge, and egui overlays.

Design source of truth: `tmp/sftp-native-port-design.md` (Phase A–F).
Progress tracker: `tmp/sftp-native-port-progress.md`.

## Objectives

- Reach SFTP upload parity with the WebView build inside native-poc.
- Reuse the Tauri-independent source core (args/check/pool/progress/process)
  without behavioral change.
- Integrate with native-poc's existing seams: per-tab SSH connection, OSC 7
  CWD, settings loader, winit window events, egui overlay rendering.

## User Stories

### US1: Upload by drag & drop on an SSH tab
As a user with an SSH-connected tab, I want to drop local files onto the
terminal, so that they are uploaded to the remote host via SFTP.

**Acceptance Criteria:**
- [ ] Dropping one or more files on an SSH tab opens an upload dialog listing
      the files and a destination path (pre-filled from OSC 7 CWD).
- [ ] Confirming uploads each file; progress is shown as per-file toasts.
- [ ] Directories are uploaded recursively (`put -r`).

### US2: Drop on a non-SSH tab pastes paths
As a user on a local (non-SSH) tab, I want dropped file paths pasted into the
terminal, so that I can use them as command arguments.

**Acceptance Criteria:**
- [ ] Dropping files on a non-SSH tab writes the formatted paths to the PTY
      (paths containing spaces are double-quoted, joined by spaces).
- [ ] No upload dialog appears.

### US3: Avoid accidental overwrite
As a user, I want to be warned when dropped files already exist remotely, so
that I do not overwrite them unintentionally.

**Acceptance Criteria:**
- [ ] Before upload, the remote directory is listed and duplicate file names
      are detected.
- [ ] When duplicates exist, an overwrite-confirmation dialog lists them; the
      default focus is Cancel.

### US4: Monitor and cancel uploads
As a user, I want to see upload progress and cancel an in-flight upload.

**Acceptance Criteria:**
- [ ] Each upload shows a toast with file name and status
      (preparing/uploading/completed/failed/cancelled).
- [ ] Terminal states auto-dismiss after a short delay.
- [ ] A Cancel control on an active toast aborts the upload (kills the sftp
      subprocess) and the toast shows "cancelled".

### US5: Don't lose uploads on tab close
As a user, I want a confirmation when closing a tab that has active uploads.

**Acceptance Criteria:**
- [ ] Closing a tab with active uploads shows a confirmation dialog.
- [ ] Confirming cancels the in-flight uploads and closes the tab.

## Technical Requirements

### Functional Requirements

- **FR1 — Core logic port:** Port `args` / `check` / `pool` / `progress` /
  `process` (source `upload.rs`) and the `SftpUploadStatus` /
  `SftpUploadProgress` types into `native-poc/src/sftp/`, Tauri-free, with their
  unit tests passing. `expand_tilde` is reused from `crate::profiles`.
- **FR2 — Orchestration service:** A Tauri-independent `SftpService` owns the
  process manager and the concurrency pool, generates session IDs (monotonic
  `AtomicU64` counter — no wall-clock), validates inputs, spawns upload worker
  threads, reports progress over a progress channel, delivers the off-thread
  duplicate-check outcome over a result channel, and records each session's
  originating tab (session→tab map) so a tab-scoped close guard is possible.
- **FR3 — Per-tab SSH connection:** `resolve_spawn` additionally returns the
  resolved `ssh_connection_name`; `Tab` stores `ssh_connection_name:
  Option<String>`; a helper resolves it to a `SshConnection` via
  `settings.ssh_connections`. A tab is "SSH" iff `ssh_connection_name.is_some()`.
- **FR4 — File drop & dispatch:** winit `HoveredFile` / `HoveredFileCancelled` /
  `DroppedFile` are handled; per-file `DroppedFile` events are aggregated into a
  single drop batch. SSH tab → upload flow; non-SSH tab → paste paths to PTY.
  `is_directory` is determined accurately via `std::fs::metadata`.
- **FR5 — Remote path resolution:** The destination is derived from the active
  tab's OSC 7 CWD (`tab.cb_state.lock().cwd`), parsing `file://host/path` (URL
  decoded) to a path; empty CWD yields an empty path (sftp default / home).
- **FR6 — Duplicate check & overwrite dialog:** Before upload, `spawn_ls` +
  `find_duplicates` detect existing remote files. The check runs off the UI
  thread and its outcome returns over the result channel; if duplicates exist,
  an overwrite dialog is shown (Cancel default-focused) before proceeding.
- **FR7 — Concurrency limit:** `ConcurrentUploadPool` caps simultaneous uploads
  at `settings.sftp_max_concurrent_uploads` (default 4); excess uploads queue.
- **FR8 — Progress toasts:** A toast per session shows file name + status; the
  progress channel is pumped each frame; terminal states auto-dismiss after a
  short delay (driven by egui frame time, not wall-clock).
- **FR9 — Cancel:** A toast Cancel control calls `service.cancel(session_id)`,
  killing the subprocess and releasing the pool slot.
- **FR10 — Tab-close guard:** Closing a tab with active uploads shows a
  confirmation dialog; confirming cancels that tab's in-flight uploads. The
  tab's sessions are identified via the service's session→tab association.
- **FR11 — Settings reflection:** The pool's max-concurrent is initialized from
  settings and updated on settings reload (`reload_settings_from_disk`).
- **FR12 — i18n:** All UI strings (overlay, dialogs, toasts) use native-poc's
  i18n mechanism (`src/i18n.rs`) with en/ja translations from the start.

### Non-Functional Requirements

- **NFR1 — Security:** Reuse the source validation:
  `validate_connection` (reject shell metacharacters in hostname),
  `validate_remote_path` (reject null bytes and `; | & \` $ ( ) " \ \n \r`),
  `validate_local_path` (reject unsafe chars; allow `\` on Windows; require
  existence). sftp arguments are passed as an argv array, never string-
  concatenated. SSH key authentication only; no password storage.
  Argv flag smuggling is blocked: a hostname/username starting with `-` is
  rejected, and `build_sftp_args` inserts a `--` end-of-options marker before
  the positional `[user@]host` element (defense in depth). Paste injection is
  blocked: paths dropped onto a non-SSH tab are POSIX single-quote escaped, and
  any path containing a control character (newline/CR/NUL) is dropped rather
  than written to the PTY.
- **NFR2 — Architecture:** Source core stays Tauri-free
  (`grep tauri native-poc/src/sftp/` must be empty). Progress is delivered via
  `crossbeam_channel`, mirroring the existing PTY-event pump pattern.
- **NFR3 — Responsiveness:** Uploads run on background threads; the UI thread
  is never blocked on a transfer. The duplicate-check (`spawn_ls`) must not
  block the UI thread (run off-thread if invoked from the UI thread).
- **NFR4 — No wall-clock:** No `Instant::now()` / `Date.now()`-equivalent direct
  calls (native-poc policy). Session IDs use an `AtomicU64` counter; toast fade
  timing uses egui frame time / a frame counter.
- **NFR5 — Cross-platform:** sftp binary detection works on Unix and Windows
  (port source `detect_sftp_binary`).

## Implementation Approach

### Architecture

```
winit WindowEvent (HoveredFile/DroppedFile)        [window_host.rs]
        │  aggregate per-file drops into one batch
        ▼
SFTP UI state (drop overlay / upload dialog /       [sftp/ui.rs, App field]
 overwrite dialog / toasts)  ── egui draw ──────────[render/mod.rs]
        │  confirm
        ▼
SftpService  ── validate ── check_duplicates (off-thread) ─┐  [sftp/service.rs]
        │                                                   │
        │ start_upload (thread, records session→tab)        │ spawn_ls
        ▼                                                   ▼
ConcurrentUploadPool ── SftpProcessManager (sftp subprocess)
        │                                       [sftp/pool.rs, sftp/process.rs]
        │ progress / completion          duplicate-check result
        ▼                                       ▼
progress channel                          result channel
        └──────────────┬───────────────────────┘
                       │  both pumped each frame (App::pump_all / about_to_wait)
                       ▼
        toast update / overwrite dialog
```

### Data Flow

```
Drop → aggregate → (SSH?) → upload dialog → check_duplicates
     → (dup?) → overwrite dialog → start_upload(thread)
     → pool slot → sftp put/put -r → progress channel → toast
Drop → (non-SSH) → format_paths_for_paste → PTY write
```

### Module Structure

```
native-poc/src/sftp/
  mod.rs          # SftpUploadStatus, SftpUploadProgress + re-exports
  args.rs         # build_sftp_args (port; expand_tilde from crate::profiles)
  check.rs        # parse_ls_output, find_duplicates (port)
  pool.rs         # ConcurrentUploadPool (Mutex+Condvar, port)
  progress.rs     # ProgressInfo, parse_progress_line, parse_error_line (port)
  process.rs      # SftpProcessManager (port of upload.rs; Tauri-free)
  service.rs      # NEW: SftpService orchestration + validation + detect binary
  remote_path.rs  # NEW: OSC 7 file:// URI → remote dir; format_paths_for_paste
  ui.rs           # NEW: egui dialog/toast state + helpers
```
`native-poc/src/main.rs` mod list gains `mod sftp;`.

### Wiring Seams (verified, file:line)

| Purpose | Location |
| --- | --- |
| SSH connection types | `app_settings::{SshConnection,SshOption}` (crates/app_settings/src/settings.rs:260-284) |
| settings profiles/connections/max | `native-poc/src/settings.rs:643-659` (`sftp_max_concurrent_uploads` :658, default 4) |
| expand_tilde | `native-poc/src/profiles.rs:76` (`pub fn`) |
| resolve_spawn (SSH branch) | `native-poc/src/profiles.rs:149` (SSH branch :166) |
| Tab struct | `native-poc/src/tabs.rs:33`; `spawn_shell` :173; constructions :1179/:1199/:1469 |
| event pump | `App::pump_all` (app.rs:1554); `about_to_wait` (window_host.rs:2519-2562) |
| PTY channel precedent | `tabs.rs:59 events: Receiver<PtyEvent>`; `pty/mod.rs:376 crossbeam bounded` |
| winit drop (to add) | `window_host.rs` WindowEvent match (:1942-2512; unused `dropped_files` :1615) |
| OSC 7 CWD | `callbacks.rs:194-196 cwd`; `status_bar/providers/cwd.rs:108 basename` |
| modal precedent | `ui/profile_selector.rs` + `render/mod.rs:320` |
| floating overlay precedent | `ui/search_bar.rs`; preedit `render/mod.rs:542` |
| settings reload | `window_host.rs:2533-2535` |

### Dependencies

**Internal:** `crate::profiles::expand_tilde`, `crate::settings::Settings`,
`app_settings::{SshConnection,SshOption}`, `crate::callbacks` (OSC 7 CWD),
`crate::i18n`, existing egui render/overlay infrastructure, existing PTY write
path.

**External:** `crossbeam_channel` (already used), `std::process` /
`std::thread` / `std::sync` (Mutex/Condvar). No new heavy dependencies; the
sftp transfer uses the external `sftp` binary (OpenSSH), same as the source.

## Test Scenarios

### Unit Tests
- [ ] `build_sftp_args`: IPv6 bracketing, `-P` port, `-b -` batch, identity-file
      tilde expansion (ported source tests).
- [ ] `parse_ls_output` / `find_duplicates`: prompt-line skip, basename
      extraction, spaces in names.
- [ ] `parse_progress_line` / `parse_error_line`: percent/bytes extraction,
      error-line detection.
- [ ] `ConcurrentUploadPool`: acquire blocks past max, release wakes one,
      `set_max_concurrent` changes effective limit, `has_active_uploads`.
- [ ] validation helpers: reject shell metacharacters / null bytes / unsafe
      chars; local path existence.
- [ ] `extract_remote_path`: `file://host/path` decode, non-ASCII, plain path,
      empty input.
- [ ] `format_paths_for_paste`: quote-on-space, space join.
- [ ] drop aggregation: multiple `PathBuf` events fold into one batch.
- [ ] `SftpService`: session-id monotonicity; empty-connection rejection.
- [ ] toast state machine: status transition + fade-deadline + dialog-confirm
      branch (duplicates → overwrite dialog vs direct upload).
- [ ] `resolve_spawn`: SSH branch sets `ssh_connection_name`; WSL branch leaves
      `None`.

### Integration Tests
- [ ] settings reload updates pool max-concurrent (`set_max_concurrent`
      behavior change observed).

> Tests that spawn a real `sftp` subprocess are out of scope (cannot run in CI).
> Coverage targets pure logic (args/check/progress/remote_path/validate/
> aggregation/UI-state).

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (WebView, tauri-driver/WebdriverIO via
`scripts/run-e2e-docker.sh`). native-poc is a separate binary not covered by the
existing WebView E2E harness.
**Run command**: `./scripts/run-e2e-docker.sh` (WebView only)
- [ ] Existing WebView E2E tests pass without regression (no source `sftp/`
      changes are required by this feature).
- [ ] Manual native-poc verification (built `native-poc/target-host/release`):
      SSH tab drop → dialog → upload → toast completion; non-SSH tab drop →
      paths pasted.

### Edge Cases
- [ ] Drop with empty OSC 7 CWD → destination empty (sftp home default).
- [ ] Duplicate names with spaces detected correctly.
- [ ] Cancel during preparing vs uploading both release the pool slot.
- [ ] Directory drop uploads recursively; size computed via recursive walk.
- [ ] sftp binary missing → upload fails with a clear message in the toast.
- [ ] Closing a tab mid-upload → confirmation; confirm cancels then closes.

## Security Considerations

- **Input Validation:** Hostname (shell metacharacters), remote path (null +
  dangerous chars), local path (unsafe chars, existence) — ported from source
  `commands/sftp.rs`.
- **Command Injection Prevention:** sftp arguments passed as argv array; put
  commands sent over stdin with quoted paths; remote/local path validation
  blocks quote/newline escapes.
- **Authentication:** SSH key authentication only (identity_file); no password
  storage.

## Error Handling

| Condition | Handling |
| --- | --- |
| sftp binary not found | `start_upload` returns Err → toast Failed with message |
| invalid connection / path | synchronous validation Err → no thread spawned; toast Failed |
| remote ls fails (check) | `check_duplicates` Err → surfaced before upload |
| transfer error | `parse_error_line` detects; status Failed with message |
| cancellation | `e.contains("cancelled")` → status Cancelled |

## Implementation Phases

### Phase A: Core logic port
**Deliverables:** `mod/args/check/pool/progress/process` in
`native-poc/src/sftp/`; ported unit tests pass; `mod sftp;` added.

### Phase B: Orchestration service
**Deliverables:** `service.rs` (`SftpService`), validation + binary detection,
session-id counter, progress channel; service unit tests.

### Phase C: Per-tab SSH connection
**Deliverables:** `SpawnOverrides.ssh_connection_name`, `resolve_spawn` update,
`Tab.ssh_connection_name`, lookup helper; resolve_spawn tests.

### Phase D: File drop + aggregation + remote path
**Deliverables:** winit Hovered/Dropped handling, drop aggregation,
SSH/non-SSH dispatch, `remote_path.rs`; aggregation + path tests.

### Phase E: egui UI
**Deliverables:** drop overlay, upload dialog, overwrite dialog, progress
toasts; progress-channel pump; toast state-machine tests.

### Phase F: Settings + tab-close guard + polish
**Deliverables:** max-concurrent init + reload reflection, tab-close
confirmation dialog, i18n strings, clippy/cleanup; settings-reflection test.

## Success Criteria

- [ ] All functional requirements implemented and unit-tested (pure-logic).
- [ ] `grep tauri native-poc/src/sftp/` is empty.
- [ ] `cargo test`/`cargo check` pass for `emterm-native-poc`.
- [ ] Existing WebView E2E tests show no regression.
- [ ] Manual native-poc verification of US1 and US2 passes.

## References

- Design: `tmp/sftp-native-port-design.md`
- Progress: `tmp/sftp-native-port-progress.md`
- Source core: `src-tauri/src/sftp/*`, `src-tauri/src/commands/sftp.rs`
- Source UI: `src/sftp/*`, `src/terminal-app/sftp-setup.ts`
- Source SDD: `doc/tasks/sftp-upload/`
