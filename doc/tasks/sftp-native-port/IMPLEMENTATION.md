# Implementation Plan: SFTP Upload — native-poc Port

## Overview
Port the WebView SFTP upload feature into native-poc by reusing the
Tauri-independent source core and replacing the Tauri IPC/UI layers with an
in-process service, an in-process progress channel, and egui overlays.

## Objectives
- Reach SFTP upload parity with the WebView build inside native-poc.
- Keep the ported core Tauri-free and behavior-preserving.
- Integrate with native-poc's existing per-tab SSH, OSC 7 CWD, settings, winit,
  and egui-overlay seams.

## Prerequisites

### Development Environment
- Rust toolchain (workspace-pinned), `rustfmt`, `cargo clippy`.
- An `sftp` binary (OpenSSH) on the host for manual/runtime verification only.

### Dependencies
- Source core to port from: `src-tauri/src/sftp/*`, `src-tauri/src/commands/sftp.rs`.
- Existing native-poc components that must exist (verified): per-tab SSH profile
  support, OSC 7 CWD tracking, settings loader with `sftp_max_concurrent_uploads`,
  egui overlay rendering, the in-process PTY-event channel pattern.

## Architecture Overview

### Technology Stack
- **Language**: Rust (`emterm-native-poc` binary).
- **Framework**: winit + egui (native window/event/render).
- **Key components**: in-process progress channel (same pattern as the existing
  PTY-event channel), background worker threads, `Mutex`/`Condvar` concurrency
  pool, external `sftp` subprocess.

### Design Approach
Bottom-up. Port pure logic first (no integration risk), build the orchestration
service on top, then wire the native-only seams (per-tab connection, file drop,
UI) in dependency order. The source core is moved with behavior unchanged; only
the Tauri-specific outer layer is redesigned for native.

### Component Interaction
File drop (winit) → aggregated batch → SFTP UI state → on confirm, the service
validates and spawns worker threads governed by the concurrency pool; the
process manager runs the `sftp` subprocess; progress flows back over the
progress channel, pumped each frame to update toasts.

## Implementation Phases

### Phase A: Core logic port

**Goal**: The source core lives in `native-poc/src/sftp/` as Tauri-free modules
with their unit tests passing.

**Files to Create**:
- `native-poc/src/sftp/mod.rs` - upload status + progress payload types; module re-exports.
- `native-poc/src/sftp/args.rs` - sftp argument construction.
- `native-poc/src/sftp/check.rs` - remote-listing parse + duplicate detection.
- `native-poc/src/sftp/pool.rs` - concurrency pool.
- `native-poc/src/sftp/progress.rs` - progress/error line parsing.
- `native-poc/src/sftp/process.rs` - sftp subprocess manager (port of source `upload.rs`).

**Files to Modify**:
- `native-poc/src/main.rs` - register the new `sftp` module in the module list.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| build_sftp_args | Build the sftp argv from connection fields | hostname/port/username known | argv with bracketed IPv6, uppercase port flag, batch-stdin flag, tilde-expanded identity file |
| find_duplicates | Return remote names that already exist | remote listing text + candidate names | subset of candidate names present remotely |
| ConcurrentUploadPool | Cap simultaneous uploads | max-concurrent set | acquire blocks past the cap; release wakes one waiter |
| parse_error_line | Detect a transfer error message | a subprocess output line | Some(message) when the line indicates failure |
| SftpProcessManager | Run/track/cancel sftp subprocesses keyed by session id | a session id | process tracked; cancel kills it; all killed on drop |

**Processing Flow**:
1. Move each source module file into `native-poc/src/sftp/`.
2. Redirect the tilde-expansion dependency to the native-poc equivalent (`crate::profiles::expand_tilde`).
3. Register the module so the crate compiles.

**Implementation Steps** (max 7):
1. **Port types** - upload status enum + progress payload struct into `mod.rs`.
2. **Port args** - argument construction with the native tilde-expansion source.
3. **Port check/progress** - parsing helpers, with their source tests.
4. **Port pool** - concurrency pool with its source tests.
5. **Port process manager** - subprocess spawn/ls/cancel/drop, with its source tests.
6. **Register module** - add to the crate module list; resolve warnings.

**Dependencies**: Requires `crate::profiles::expand_tilde`. Blocks Phase B.

**Testing Approach**:
- Unit: ported source tests for args/check/progress/pool/process (pure logic).
- Manual: none.

**Acceptance Criteria**:
- [ ] Ported modules compile under the crate.
- [ ] Ported unit tests pass.
- [ ] `grep tauri native-poc/src/sftp/` is empty.

**Estimated Effort**: medium

---

### Phase B: Orchestration service

**Goal**: A Tauri-independent service owns the process manager and pool,
validates inputs, generates session ids without wall-clock, runs uploads on
worker threads, reports progress over an in-process channel, returns the
off-thread duplicate-check result over a result channel, and tracks each
session's originating tab so a tab-scoped close guard is possible.

**Files to Create**:
- `native-poc/src/sftp/service.rs` - upload orchestration, input validation, sftp-binary detection.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SftpService | Own manager + pool + progress sender + result sender; track session→tab; orchestrate uploads | constructed with a max-concurrent, a progress sender, and a result sender | uploads run off-thread, progress/results emitted over channels, sessions cancellable, each session associated with its originating tab |
| next_session_id | Produce a unique session id | none | monotonically increasing id (no wall-clock) |
| check_duplicates | List remote dir off-thread and detect duplicates | valid connection + remote dir | duplicate-check outcome (names or error) delivered over the result channel; UI thread never blocks |
| start_upload | Validate then spawn an upload worker; record session→tab | valid connection + paths + originating tab id | preparing emitted immediately; uploading/terminal emitted from the worker; session associated with the tab |
| cancel | Abort an in-flight upload | a session id | subprocess killed, pool slot released, session→tab entry removed |
| active_for_tab | Report whether a given tab has active uploads, and cancel them | a tab id | the tab's sessions identified via the session→tab map |
| validation helpers | Reject unsafe connection/remote/local inputs | raw inputs | Ok or a descriptive error |

**Processing Flow**:
1. On construct: detect the sftp binary once; create manager, pool, store the
   progress sender and the result sender.
2. check_duplicates: spawn a short worker that runs the remote listing and sends
   the outcome over the result channel (UI thread is never blocked).
3. start_upload: validate connection/remote/local → on error return immediately;
   record the session→tab association.
4. Emit preparing → spawn a worker thread.
5. Worker: acquire pool slot → emit uploading → run subprocess → release slot →
   emit completed, or failed/cancelled (cancelled when the error text indicates
   cancellation); on terminal state, remove the session→tab entry.

**Implementation Steps** (max 7):
1. **Binary detection** - port the platform sftp-binary detection.
2. **Validation** - port connection/remote/local validation helpers.
3. **Connection adapter** - map the settings SSH-connection type to the argv inputs.
4. **Service struct** - own manager/pool/progress sender/result sender; session-id counter; session→tab map.
5. **start_upload/cancel** - worker-thread orchestration mirroring the source command flow; record/remove session→tab; `active_for_tab` for the close guard.
6. **Off-thread duplicate check** - run the remote listing on a short worker; deliver the outcome over the result channel.
7. **Size + progress payloads** - local-size computation; emit the staged progress payloads.

**Dependencies**: Requires Phase A and the settings SSH-connection type. Blocks Phase E.

**Testing Approach**:
- Unit: validation rejection cases; session-id monotonicity; empty-connection rejection.
- Integration: none that spawns a real subprocess (out of scope).

**Acceptance Criteria**:
- [ ] Service constructs and exposes start/cancel/duplicate-check/has-active/set-max.
- [ ] Validation + session-id unit tests pass.

**Estimated Effort**: medium

---

### Phase C: Per-tab SSH connection

**Goal**: A tab knows which SSH connection it was spawned with, so SFTP can
build its connection inputs.

**Files to Modify**:
- `native-poc/src/profiles.rs` - spawn-overrides carry the resolved connection name; the SSH branch sets it.
- `native-poc/src/tabs.rs` - the tab stores the connection name; spawn fills it; the three tab constructions pass it through.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| spawn-overrides connection name | Carry the resolved SSH connection name out of resolution | a profile + settings | Some(name) for the SSH branch, None otherwise |
| Tab connection name | Persist the connection name for the tab's lifetime | tab spawned | the tab reports whether it is an SSH tab and which connection |
| tab_ssh_connection lookup | Resolve a tab to its connection record | a tab + settings | the matching connection record, or none |

**Processing Flow**:
1. Resolution sets the connection name on the SSH branch (the WSL branch keeps none).
2. Spawn stores the name on the tab.
3. A helper looks the name up in the settings connection list on demand.

**Implementation Steps** (max 7):
1. **Extend spawn-overrides** - add the connection-name carrier; populate it in the SSH branch.
2. **Add tab field** - store the connection name; thread it through spawn and the three constructions.
3. **Lookup helper** - resolve a tab + settings to a connection record; expose an "is SSH tab" predicate.

**Dependencies**: Independent of A/B; Blocks Phase D and E.

**Testing Approach**:
- Unit: resolution sets the name on the SSH branch and leaves none on the WSL branch.

**Acceptance Criteria**:
- [ ] An SSH-profile tab reports its connection name; a non-SSH tab reports none.
- [ ] Resolution tests pass; the crate compiles with all constructions filled.

**Estimated Effort**: small

---

### Phase D: File drop, aggregation, remote path

**Goal**: Dropped files are aggregated into one batch and dispatched — upload
on SSH tabs, paste on non-SSH tabs — with an accurate directory flag and a
remote destination derived from OSC 7 CWD.

**Files to Create**:
- `native-poc/src/sftp/remote_path.rs` - OSC 7 URI → remote directory; local-path paste formatting.

**Files to Modify**:
- `native-poc/src/window_host.rs` - handle the winit hovered/dropped/hover-cancelled events; aggregate per-file drops; dispatch on the next loop turn.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| drop aggregation | Fold per-file drop events into one batch | one or more per-file drop events in a loop turn | a single batch of paths for one drop gesture |
| dispatch | Route a batch by tab kind | an aggregated batch + active tab | SSH tab opens the upload flow; non-SSH writes formatted paths to the PTY |
| directory flag | Determine whether a dropped path is a directory | a dropped path | accurate is-directory flag |
| extract_remote_path | Derive a remote directory from OSC 7 CWD | a CWD string (possibly a file:// URI) | a decoded path, or empty when unknown |
| format_paths_for_paste | Format local paths for terminal paste | dropped paths | space-joined, with space-containing paths quoted |

**Processing Flow**:
1. Hovered file → show the drop overlay (message depends on SSH/non-SSH).
2. Hover-cancelled → hide the overlay.
3. Dropped file(s) → accumulate paths; request a redraw.
4. Next loop turn → finalize the batch once and dispatch.
   - SSH tab → derive the remote path from the active tab's OSC 7 CWD; open the upload dialog.
   - Non-SSH tab → format and write paths to the PTY.

**Implementation Steps** (max 7):
1. **Window events** - add hovered/dropped/hover-cancelled handling.
2. **Aggregation** - accumulate per-file drops into a batch finalized on the next turn.
3. **remote_path helpers** - URI decode + paste formatting, with tests.
4. **Dispatch** - branch by tab kind; compute the directory flag per path.
5. **Non-SSH paste** - write formatted paths through the existing PTY-write path.

**Dependencies**: Requires Phase C (tab kind, connection) and OSC 7 CWD. Blocks Phase E (SSH branch opens the dialog).

**Testing Approach**:
- Unit: aggregation folds multiple paths into one batch; remote-path URI decode
  (incl. non-ASCII, plain path, empty); paste formatting (quote-on-space).
- Manual: actual drag-and-drop gesture on a window.

**Acceptance Criteria**:
- [ ] Multiple per-file drops become one batch.
- [ ] Non-SSH drop pastes formatted paths; SSH drop opens the upload flow.
- [ ] remote-path + paste-format unit tests pass.

**Estimated Effort**: medium

---

### Phase E: egui UI (overlay, dialogs, toasts)

**Goal**: Drop overlay, upload dialog, overwrite dialog, and progress toasts
are rendered and driven by the progress channel.

**Files to Create**:
- `native-poc/src/sftp/ui.rs` - SFTP UI state and helpers (dialog/toast state machine).

**Files to Modify**:
- `native-poc/src/app.rs` - hold the SFTP UI state and the progress receiver; pump progress each frame.
- `native-poc/src/render/mod.rs` - draw the overlay/dialogs/toasts alongside the existing overlays.
- `native-poc/src/i18n.rs` - register UI strings (en/ja).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| SFTP UI state | Hold overlay/dialog/toast state | app constructed | dialogs and toasts queryable for rendering |
| upload dialog | Confirm files + destination | an aggregated SSH batch | on confirm, trigger the duplicate check then upload (or overwrite dialog) |
| overwrite dialog | Confirm overwrite of existing remote files | duplicates detected | on confirm, start uploads; on cancel, abort |
| toast list | Show per-session progress | progress events arriving | toast reflects latest status; terminal states schedule auto-dismiss |
| progress pump | Apply progress events to toasts each frame | a progress receiver | toasts updated; finished toasts marked for dismissal by frame time |
| result pump | Apply duplicate-check results each frame | a result receiver | a result opens the overwrite dialog (duplicates) or proceeds to upload (none) |

**Processing Flow**:
1. Drop overlay shown while hovering (Phase D supplies the state).
2. SSH batch → upload dialog (destination pre-filled from remote path).
3. Confirm → request an off-thread duplicate check (does not block the UI).
4. Each frame → drain the result channel → a result with duplicates opens the
   overwrite dialog, a result with none starts the uploads.
5. Each frame → drain progress events → update toasts → auto-dismiss terminal
   toasts after a frame-time delay.
6. Toast cancel → cancel the session.

**Implementation Steps** (max 7):
1. **UI state** - overlay/dialog/toast state container and transitions.
2. **App wiring** - hold state + progress receiver + result receiver; drain both each frame.
3. **Overlay + dialogs render** - draw via the existing overlay/modal patterns; keyboard (Esc/Enter) + initial focus.
4. **Toast render** - top-right stack; status text; cancel control.
5. **Confirm logic** - dialog confirm → request off-thread duplicate check; the result-channel pump branches to overwrite or upload.
6. **i18n strings** - register en/ja keys for all SFTP UI text.

**Dependencies**: Requires Phase B (service) and Phase D (SSH batch + remote path). Blocks Phase F polish.

**Testing Approach**:
- Unit: toast state transitions + auto-dismiss decision; dialog-confirm branch
  (duplicates vs direct upload).
- Manual: dialog/toast appearance, keyboard handling, multi-upload stacking.

**Acceptance Criteria**:
- [ ] SSH drop shows the upload dialog; confirm uploads; toasts reflect status.
- [ ] Duplicates trigger the overwrite dialog.
- [ ] Toast cancel aborts an upload.
- [ ] UI-state unit tests pass.

**Estimated Effort**: large

---

### Phase F: Settings, tab-close guard, polish

**Goal**: Concurrency reflects settings, closing a tab with active uploads is
guarded, and the feature is cleaned up.

**Files to Modify**:
- `native-poc/src/app.rs` - construct the service from settings; guard tab close on active uploads.
- `native-poc/src/window_host.rs` - apply max-concurrent on settings reload.
- `native-poc/src/i18n.rs` - close-guard strings.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| settings reflection | Initialize + update the concurrency cap | settings load / reload | the pool's cap matches settings |
| tab-close guard | Confirm before closing a tab with active uploads | a close request on a tab with active uploads | a confirmation dialog; confirm cancels the tab's uploads then closes |

**Processing Flow**:
1. On startup, construct the service with the settings concurrency value.
2. On settings reload, update the pool's cap.
3. On tab close, query `active_for_tab` (using the session→tab map) for the
   closing tab; if it has active uploads, show a confirmation; on confirm,
   cancel that tab's sessions, then close.

**Implementation Steps** (max 7):
1. **Settings init** - construct the service from the settings value.
2. **Reload reflection** - update the cap on settings reload.
3. **Close guard** - detect the closing tab's active uploads via the session→tab map; confirm; cancel that tab's sessions, then close.
4. **i18n + cleanup** - close-guard strings; remove dead-code allowances; clippy pass.

**Dependencies**: Requires Phase B and E.

**Testing Approach**:
- Unit/Integration: changing the cap changes acquire behavior.
- Manual: reload changes effective concurrency; close-with-uploads shows the guard.

**Acceptance Criteria**:
- [ ] Concurrency follows settings at startup and after reload.
- [ ] Closing a tab with active uploads is guarded and cancels on confirm.
- [ ] Cap-change unit test passes; clippy clean.

**Estimated Effort**: medium

---

## Complete File Structure

```
native-poc/src/
├── main.rs                 # (modify) register `mod sftp`
├── profiles.rs             # (modify) spawn-overrides carry SSH connection name
├── tabs.rs                 # (modify) Tab stores connection name
├── app.rs                  # (modify) SFTP UI state + progress pump + service + close guard
├── window_host.rs          # (modify) winit file-drop events + settings-reload cap
├── render/mod.rs           # (modify) draw SFTP overlay/dialogs/toasts
├── i18n.rs                 # (modify) SFTP UI strings (en/ja)
└── sftp/                   # (new module)
    ├── mod.rs              # status + progress types
    ├── args.rs             # sftp argv construction
    ├── check.rs            # remote-listing parse + duplicate detection
    ├── pool.rs             # concurrency pool
    ├── progress.rs         # progress/error line parsing
    ├── process.rs          # sftp subprocess manager
    ├── service.rs          # orchestration + validation + binary detection
    ├── remote_path.rs      # OSC 7 URI → remote dir; paste formatting
    └── ui.rs               # egui dialog/toast state
```

## Testing Strategy
- Unit: pure logic (args/check/progress/pool/validation/remote_path/aggregation/
  UI-state/resolution) — high coverage on the ported and new logic.
- Integration: settings cap-change effect on the pool.
- E2E: existing WebView E2E remains green (no source changes); native-poc is not
  in that harness, so native-poc behavior is verified manually.
- Manual: drag-and-drop gesture, dialog/toast UX, cancel, close guard.

## Dependencies
| Component | Source | Purpose |
|-----------|--------|---------|
| sftp core | `src-tauri/src/sftp/*` | ported pure logic |
| SSH connection type | `app_settings` crate | connection fields |
| expand_tilde | `native-poc/src/profiles.rs` | identity-file expansion |
| OSC 7 CWD | `native-poc/src/callbacks.rs` | remote destination |
| in-process channel | existing PTY-event pattern | progress delivery |
| external sftp binary | OpenSSH | the actual transfer |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| winit gives no drop-complete signal | High | Medium | aggregate per-file drops, finalize on the next loop turn |
| duplicate check blocks the UI thread | Medium | Medium | run the remote listing off the UI thread |
| toast timing needs wall-clock | Medium | Low | drive auto-dismiss from egui frame time / a frame counter |
| concurrent sessions confuse cancel | Low | Medium | key everything by session id (ported model) |

## Open Questions
- [ ] None blocking. Tab-close behavior and UI-string i18n were resolved during
      spec creation (confirmation dialog; i18n keys from the start).

## Success Metrics
- [ ] All FR/NFR implemented and unit-tested where pure logic.
- [ ] `grep tauri native-poc/src/sftp/` empty; crate builds and tests pass.
- [ ] Existing WebView E2E unaffected; manual US1/US2 verified.
