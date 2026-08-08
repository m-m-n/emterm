//! Snapshot / restore of the live mux session tree to and from the
//! versioned handoff document, and the handoff file's lifetime (mux daemon
//! hot-upgrade, task0003; IMPLEMENTATION.md Shared Components "Upgrade
//! snapshot / restore").
//!
//! **snapshot** walks the live [`SessionManager`] tree, clears
//! `FD_CLOEXEC` on every live pane's master descriptor and on the listen
//! descriptor the caller supplies, captures each pane's scrollback, and
//! writes the resulting [`HandoffDocument`] to a freshly created,
//! owner-only file next to the daemon's listen socket.
//!
//! **restore** rebuilds a [`SessionManager`] from a decoded document: the
//! identifier counters and incarnation token are restored verbatim
//! (IMPLEMENTATION.md D5), each live pane's descriptor is re-adopted through
//! the inherited master adapter (task0002) and its writer / reader thread
//! re-established with the SAME wiring a freshly spawned pane gets, and its
//! terminal-state view is rebuilt by replaying restored scrollback
//! (IMPLEMENTATION.md D8) rather than expecting serialised parser state. A
//! pane whose descriptor cannot be adopted is restored as exited instead of
//! dropping the whole session (AC-7).
//!
//! task0004 (agent-exit-after-icon, SPEC FR6) extends the same per-pane
//! snapshot/restore pair to also carry each pane's inferred-clear latch
//! state (task0001 [`crate::agent_status_exit_latch::AgentStatusExitLatch`],
//! wired per-pane by task0003) across the upgrade boundary, transferred
//! verbatim via its raw state components — never reset and never
//! re-derived — so a pane mid-"command_ended" before an upgrade is still
//! mid-"command_ended" immediately after it.
//!
//! mux-hot-upgrade-alt-screen task0002 (SPEC FR3-FR8) extends the same
//! per-pane snapshot/restore pair to also carry each pane's
//! alternate-screen state (flag + formatted-contents dump, handoff schema
//! version 3) across the upgrade boundary via
//! [`crate::mux::session::pane::MuxPane::capture_alt_state`] /
//! [`crate::mux::session::pane::MuxPane::from_restored`], so a
//! formerly-alt-screen pane's shadow parser reports the alternate screen
//! again immediately after restore.
//!
//! task0006 (review rework, finding `2e6f18b4dc0a7593`) confirmed that
//! `snapshot`'s read of the tree is atomic (taken under the
//! `SessionManager` lock) but is NOT a cut of the live event stream: pane
//! reader threads and the daemon's agent-status task keep running after
//! `snapshot` returns and can still apply a live `Set`/`Clear`/inferred
//! clear to a pane's `agent_status` / `agent_status_exit_latch` before
//! `exec` replaces the process image, leaving the already-written document
//! stale. [`refresh_live_agent_state`] + [`rewrite_handoff_file`] narrow
//! that window: called by `prepare_upgrade` as late as possible (after its
//! multi-second wait for client acknowledgement — the dominant portion of
//! the window), they re-capture and rewrite just the affected fields. See
//! [`refresh_live_agent_state`]'s doc comment for exactly what this does
//! and does not close.
//!
//! Everything in this module is Unix-only (gated at the `mod upgrade;`
//! declaration in `mux::mod`).

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::MasterPty;

use mux_ipc::handoff::{
    HANDOFF_SCHEMA_VERSION, HandoffDecodeError, HandoffDocument, HandoffPane, HandoffSession,
    HandoffWindow, decode_handoff_document, encode_handoff_document,
};

use crate::agent_status_exit_latch::AgentStatusExitLatch;
use crate::mux::daemon::{from_wire_state, to_wire_state};
use crate::mux::inherited_pty::InheritedMasterPty;
use crate::mux::scrollback_buffer::{
    DEFAULT_SCROLLBACK_CAPACITY, ScrollbackRingBuffer, ScrollbackSnapshot,
};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatus, AgentStatusReportSender, DetachReason, MuxPane, NotificationSender,
    PaneOutputTarget, SharedOutputTarget, SharedPaneExitSender, TitleChangeSender,
};
use crate::mux::session::session::MuxSession;
use crate::mux::session::window::MuxWindow;

/// Handoff file name, placed alongside the daemon's listen socket (same
/// directory, which is already created owner-only — see `daemon::socket_path`
/// / `daemon::spawn_daemon`).
const HANDOFF_FILE_NAME: &str = "mux-handoff.bin";

/// Absolute path of the handoff state file for the daemon whose listen
/// socket lives at `socket_path` (Design step 5 / "Handoff file lifetime").
pub fn handoff_file_path(socket_path: &Path) -> PathBuf {
    socket_path.with_file_name(HANDOFF_FILE_NAME)
}

/// Failure reported by [`snapshot`]: which stage failed and why (Design:
/// "Failure at any stage aborts and reports which stage failed"). Snapshot
/// failure leaves the session tree untouched — any descriptor flags already
/// cleared before the failing stage are harmless if the upgrade is
/// abandoned (the process simply keeps running), so no rollback is
/// performed or required.
#[derive(Debug)]
pub enum SnapshotError {
    /// Clearing `FD_CLOEXEC` on the listen descriptor supplied by the
    /// caller failed.
    ListenDescriptor { fd: RawFd, source: std::io::Error },
    /// Clearing `FD_CLOEXEC` on a live pane's master descriptor failed.
    PaneDescriptor {
        pane_id: u32,
        fd: RawFd,
        source: std::io::Error,
    },
    /// Writing the serialised document to the handoff file failed. The
    /// partially written file, if any, has already been removed (see
    /// [`write_handoff_file`]).
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ListenDescriptor { fd, source } => write!(
                f,
                "failed to clear FD_CLOEXEC on listen descriptor {fd}: {source}"
            ),
            Self::PaneDescriptor {
                pane_id,
                fd,
                source,
            } => write!(
                f,
                "failed to clear FD_CLOEXEC on pane {pane_id}'s master descriptor {fd}: {source}"
            ),
            Self::WriteFile { path, source } => {
                write!(f, "failed to write handoff file {path:?}: {source}")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Failure reported by [`read_and_remove_handoff_file`]. The file is removed
/// regardless of which of these is returned (see that function's doc).
#[derive(Debug)]
pub enum HandoffReadError {
    /// The file could not be read (missing, permission denied, ...).
    Io(std::io::Error),
    /// The bytes could not become a supported [`HandoffDocument`] (see
    /// [`HandoffDecodeError`]).
    Decode(HandoffDecodeError),
}

impl std::fmt::Display for HandoffReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "failed to read handoff file: {e}"),
            Self::Decode(e) => write!(f, "failed to decode handoff document: {e}"),
        }
    }
}

impl std::error::Error for HandoffReadError {}

/// Clear `FD_CLOEXEC` on `fd` (Design steps 2/3). `fd` must be a descriptor
/// the caller owns or otherwise controls for the duration of this call (a
/// live pane's master, or the listen descriptor supplied by the caller).
fn clear_cloexec(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: `fd` is a live descriptor the caller is responsible for
    // keeping open for the duration of this call; `F_GETFD`/`F_SETFD` only
    // inspect/modify its close-on-exec bit and touch no memory we control.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Set `FD_CLOEXEC` on `fd` — the restore-side symmetric counterpart of
/// [`clear_cloexec`] (Design "Descriptor lifetime": snapshot clears the flag
/// so a descriptor survives `execve`; restore must set it back once adopted
/// so it never reaches a LATER pane's child process). `fd` must be a
/// descriptor the caller owns or otherwise controls for the duration of this
/// call.
fn set_cloexec(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: see `clear_cloexec` above.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Confirm that `fd` refers to a live, listening `AF_UNIX` socket, WITHOUT
/// taking ownership of it (Design "Adoption validation" — as defensive as
/// adopting a pane master already is via [`InheritedMasterPty::new`]'s
/// `isatty` check). Checked, in order: `fstat` succeeds and reports a socket
/// (`S_IFSOCK`); `SO_ACCEPTCONN` is set (the socket is in the listening
/// state, not merely any open socket); `getsockname` reports the `AF_UNIX`
/// family. Any failure means "do not adopt this descriptor" — the caller
/// falls back to a fresh bind (AC-6).
fn validate_listen_fd(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: `st` is a zero-initialized POD `fstat` fills in; `fd` is a
    // caller-supplied descriptor number, valid or not (`fstat`'s return
    // code reports the invalid case).
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if st.st_mode & libc::S_IFMT != libc::S_IFSOCK {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("fd {fd} is not a socket"),
        ));
    }

    let mut accept_conn: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `accept_conn`/`len` are valid, correctly-sized out-parameters;
    // `fd` was just confirmed to be an open socket above.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_ACCEPTCONN,
            &mut accept_conn as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if accept_conn == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("fd {fd} is a socket but is not in the listening state"),
        ));
    }

    let mut addr: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut addr_len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
    // SAFETY: `addr`/`addr_len` are valid, correctly-sized out-parameters;
    // `fd` was just confirmed to be an open, listening socket above.
    let rc = unsafe {
        libc::getsockname(
            fd,
            &mut addr as *mut _ as *mut libc::sockaddr,
            &mut addr_len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if addr.ss_family as libc::c_int != libc::AF_UNIX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("fd {fd} is a listening socket but not AF_UNIX"),
        ));
    }

    Ok(())
}

/// Validate `fd` ([`validate_listen_fd`]) and, only on success, take
/// ownership of it as a std [`std::os::unix::net::UnixListener`] with
/// `FD_CLOEXEC` restored (Design "Adoption validation" / "Descriptor
/// lifetime"). On failure, `fd` is left completely untouched — no ownership
/// is taken, so the caller can safely fall back to a fresh bind without
/// risking a wild close of an unrelated descriptor (AC-6).
pub fn adopt_listener(fd: RawFd) -> std::io::Result<std::os::unix::net::UnixListener> {
    validate_listen_fd(fd)?;
    // SAFETY: validated immediately above to be a live, listening `AF_UNIX`
    // socket; ownership is taken exactly once here.
    let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(fd) };
    // Ownership WAS taken (`from_raw_fd` above); `listener`'s own Drop closes
    // `fd` exactly once if `?` returns `Err` here — no separate manual close
    // needed.
    set_cloexec(fd)?;
    Ok(listener)
}

/// Open the handoff file fresh: owner-only permission, refusing to follow an
/// existing path (symlink or otherwise) — `O_NOFOLLOW` + `create_new` +
/// `mode(0o600)`, mirroring `daemon::open_mux_log_append`'s hardening
/// convention for files in the same directory.
fn create_handoff_file(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    opts.mode(0o600);
    opts.custom_flags(libc::O_NOFOLLOW);
    opts.open(path)
}

/// Write `bytes` to `file`, removing `path` if the write itself fails —
/// "the partially written file is removed" (Design). Split out from
/// [`write_handoff_file`] so the cleanup-on-failure behavior is unit
/// testable by forcing a write failure on an already-created file, without
/// needing genuine disk-full-style fault injection.
fn write_bytes_or_remove(
    mut file: std::fs::File,
    path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    if let Err(e) = file.write_all(bytes) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

/// Serialise `document` and write it to `path`, created fresh with
/// owner-only permission (Design step 5). Fails rather than write anywhere
/// world-readable, and fails rather than follow or reuse an existing path.
fn write_handoff_file(document: &HandoffDocument, path: &Path) -> std::io::Result<()> {
    let bytes = encode_handoff_document(document);
    let file = create_handoff_file(path)?;
    write_bytes_or_remove(file, path, &bytes)
}

/// Read, version-checked-decode, and unconditionally remove the handoff
/// file at `path` ("Handoff file lifetime": removal happens after both a
/// successful and a failed read — the file must never survive the
/// operation that created it).
pub fn read_and_remove_handoff_file(path: &Path) -> Result<HandoffDocument, HandoffReadError> {
    let result = std::fs::read(path)
        .map_err(HandoffReadError::Io)
        .and_then(|bytes| decode_handoff_document(&bytes).map_err(HandoffReadError::Decode));
    let _ = std::fs::remove_file(path);
    result
}

/// Best-effort removal of the handoff file, for an aborted snapshot that a
/// caller detects at a point outside this module (defensive; the write path
/// already removes a partially written file on its own failure).
pub fn remove_handoff_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// task0006 (review rework, finding `2e6f18b4dc0a7593`): re-read each still
/// live pane's CURRENT `agent_status` and inferred-clear latch state from
/// `mgr` and patch those fields, in place, into the matching pane entries of
/// `document`.
///
/// mux-hot-upgrade-alt-screen task0002 (SPEC FR7) extends this same re-read
/// to the pane's alt-screen flag + dump (via
/// [`crate::mux::session::pane::MuxPane::capture_alt_state`], the same
/// helper `snapshot_pane` uses): a pane that switched buffers between the
/// original `snapshot` and this refresh pass gets its document entry
/// updated in both directions — main->alt gains flag true + a fresh dump,
/// alt->main returns to flag false + empty dump.
///
/// **Why this exists**: `snapshot` takes the `SessionManager` lock only for
/// the duration of the tree walk, then releases it. Between that release and
/// the caller's eventual `exec` (`prepare_upgrade`'s bounded wait for client
/// acknowledgement of the `Upgrading` broadcast, plus the daemon runtime's
/// own shutdown grace period — both driven by std::thread reader threads
/// that are NOT part of the async runtime and therefore keep consuming live
/// PTY bytes right up to `exec`), the daemon's agent-status task keeps
/// applying live OSC 777 reports and OSC 133 marks to each pane's
/// `agent_status` / `agent_status_exit_latch` — the SAME fields `snapshot`
/// already captured. A live `D`→`A` transition (or an explicit `Set`/
/// `Clear`) landing in that window changes the pane's true state WITHOUT
/// updating the already-written handoff document: exactly the "torn
/// snapshot" the finding describes (e.g. an inferred clear fires in this
/// process, disarming the latch and clearing the icon, while the document
/// still records the pre-clear armed/command-ended state — the successor
/// then restores a latch waiting for an `A` that was already consumed and
/// will never arrive again).
///
/// **What this does NOT close**: this only re-reads state that the daemon's
/// agent-status task already applied by the time it runs; it does not stop
/// pane reader threads from reading further PTY bytes, so a mark landing in
/// the (much smaller, sub-second) window between this call and the actual
/// `exec` is still unrepresented. Closing that residual window fully would
/// require pausing each pane's reader thread at a defined byte boundary
/// (the finding's `suggestion`), which touches the reader-thread wiring in
/// `mux::ipc::pty_spawn` — out of this file's scope. Calling this function
/// as late as possible (immediately before `prepare_upgrade` returns, i.e.
/// after the client-acknowledgement wait — by far the dominant, multi-second
/// portion of the window — has already elapsed) is what makes the residual
/// gap small rather than eliminating it.
///
/// Panes recorded exited in `document` (`master_fd: None`), panes no longer
/// found in `mgr`, and panes that have since exited in `mgr` are left
/// untouched — refreshing exited-pane state is a separate, pre-existing
/// concern (the descriptor/exited-flag mismatch that can also arise if a
/// pane exits during this same window) that this function does not attempt
/// to fix.
pub fn refresh_live_agent_state(document: &mut HandoffDocument, mgr: &SessionManager) {
    for session_doc in &mut document.sessions {
        for window_doc in &mut session_doc.windows {
            for pane_doc in &mut window_doc.panes {
                if pane_doc.master_fd.is_none() {
                    // Recorded exited (or already had no descriptor) --
                    // nothing live to refresh from.
                    continue;
                }
                let Some((sid, wid)) = mgr.find_pane(pane_doc.id) else {
                    continue;
                };
                let Some(pane) = mgr
                    .get_session(sid)
                    .and_then(|s| s.windows.get(&wid))
                    .and_then(|w| w.panes.get(&pane_doc.id))
                else {
                    continue;
                };
                if pane.exited {
                    continue;
                }

                let (agent_state, agent_name, agent_revision) = {
                    let status = pane.agent_status.lock().unwrap();
                    (
                        status.state.map(to_wire_state),
                        status.name.clone(),
                        status.revision,
                    )
                };
                let (latch_armed, latch_command_ended, latch_generation) =
                    pane.agent_status_exit_latch.lock().unwrap().state_parts();
                // mux-hot-upgrade-alt-screen task0002 (SPEC FR7): re-capture
                // the alt-screen flag + dump through the SAME helper
                // `snapshot_pane` uses, so a pane that switched buffers
                // between the original snapshot and this refresh pass gets
                // its document entry updated in both directions (main->alt
                // and alt->main).
                let (alt_screen, alt_screen_dump) = pane.capture_alt_state();

                pane_doc.agent_state = agent_state;
                pane_doc.agent_name = agent_name;
                pane_doc.agent_revision = agent_revision;
                pane_doc.latch_armed = latch_armed;
                pane_doc.latch_command_ended = latch_command_ended;
                pane_doc.latch_generation = latch_generation;
                pane_doc.alt_screen = alt_screen;
                pane_doc.alt_screen_dump = alt_screen_dump;
            }
        }
    }
}

/// Re-serialise `document` and atomically replace the ALREADY-WRITTEN
/// handoff file next to `socket_path` (task0006: the companion write for
/// [`refresh_live_agent_state`]). Unlike [`write_handoff_file`], the target
/// path is expected to already exist from an earlier `snapshot` call in the
/// same upgrade attempt.
///
/// Unlike an in-place truncate-then-write, this never destroys the
/// already-written (stale but decodable) document before the replacement is
/// known-good: the new content is written in full to a same-directory temp
/// path (same `create_new` + owner-only + `O_NOFOLLOW` hardening as
/// [`create_handoff_file`], via [`write_bytes_or_remove`] for the
/// partial-write-cleans-up-after-itself invariant), and only a successful
/// write is `rename(2)`d over the real path. If the write to the temp path
/// fails, the temp path is removed and the pre-existing handoff file at
/// `path` is left completely untouched, so a caller that only logs and
/// continues on `Err` (as `daemon.rs` does) hands the successor the
/// still-valid prior document instead of a torn one.
pub fn rewrite_handoff_file(document: &HandoffDocument, socket_path: &Path) -> std::io::Result<()> {
    let bytes = encode_handoff_document(document);
    let path = handoff_file_path(socket_path);
    let mut tmp_path = path.clone();
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    tmp_name.push(".tmp");
    tmp_path.set_file_name(tmp_name);

    // A leftover temp file from a previous crashed run would make
    // `create_handoff_file`'s `create_new` fail spuriously -- clear it
    // first, best-effort.
    let _ = std::fs::remove_file(&tmp_path);

    let file = create_handoff_file(&tmp_path)?;
    if let Err(e) = write_bytes_or_remove(file, &tmp_path, &bytes) {
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

/// Snapshot the live session tree into a [`HandoffDocument`] and write it to
/// the handoff file next to `socket_path` (Design steps 1-5).
///
/// Precondition: the caller holds the session-manager lock (the `&`
/// reference here is the caller's proof of that).
///
/// Postcondition on success: every non-exited pane's master descriptor and
/// `listen_fd` have `FD_CLOEXEC` cleared, every pane's scrollback was
/// captured under its own lock, and the document was written to the handoff
/// file. On failure, the session tree is untouched and no partially written
/// file survives (see [`SnapshotError`]).
pub fn snapshot(
    mgr: &SessionManager,
    listen_fd: RawFd,
    socket_path: &Path,
) -> Result<HandoffDocument, SnapshotError> {
    clear_cloexec(listen_fd).map_err(|source| SnapshotError::ListenDescriptor {
        fd: listen_fd,
        source,
    })?;

    // Stable order (Design step 1): `SessionManager` stores sessions in a
    // `HashMap`, whose iteration order is not stable across runs — sort by
    // id so the document's tree order is deterministic.
    let mut sessions: Vec<&MuxSession> = mgr.sessions_iter().collect();
    sessions.sort_by_key(|s| s.id);

    let mut sessions_doc = Vec::with_capacity(sessions.len());
    for session in sessions {
        let mut windows_doc = Vec::with_capacity(session.window_order.len());
        for &wid in &session.window_order {
            let Some(window) = session.windows.get(&wid) else {
                continue;
            };
            let mut panes_doc = Vec::with_capacity(window.panes.len());
            for pane in window.panes.values() {
                panes_doc.push(snapshot_pane(pane)?);
            }
            windows_doc.push(HandoffWindow {
                id: window.id,
                name: window.name.clone(),
                active_pane_id: window.active_pane_id,
                next_pane_id: window.next_pane_id_counter(),
                panes: panes_doc,
            });
        }
        sessions_doc.push(HandoffSession {
            id: session.id,
            name: session.name.clone(),
            window_order: session.window_order.clone(),
            active_window_id: session.active_window_id,
            next_window_id: session.next_window_id_counter(),
            windows: windows_doc,
        });
    }

    let document = HandoffDocument {
        schema_version: HANDOFF_SCHEMA_VERSION,
        incarnation: mgr.incarnation().to_string(),
        listen_fd,
        next_session_id: mgr.next_session_id_counter(),
        next_pane_id: mgr.next_pane_id_counter(),
        sessions: sessions_doc,
    };

    let path = handoff_file_path(socket_path);
    write_handoff_file(&document, &path)
        .map_err(|source| SnapshotError::WriteFile { path, source })?;

    Ok(document)
}

/// Build one pane's document entry (Design steps 2/4): a non-exited pane
/// contributes its master descriptor (`FD_CLOEXEC` cleared) and child pid;
/// an exited pane contributes neither. Scrollback is captured while the
/// pane's own scrollback lock is held (via [`ScrollbackRingBuffer::capture`]).
///
/// mux-hot-upgrade-alt-screen task0002 (SPEC FR5/FR8): the alt-screen flag +
/// dump are captured via [`MuxPane::capture_alt_state`] for non-exited panes
/// only — an exited pane has no live alternate-screen semantics to carry,
/// and its restore path (`from_restored_exited`) builds a fresh parser
/// anyway, so it contributes flag false + empty dump.
fn snapshot_pane(pane: &MuxPane) -> Result<HandoffPane, SnapshotError> {
    let (master_fd, child_pid) = if pane.exited {
        (None, None)
    } else if let Some(fd) = pane.master_raw_fd() {
        clear_cloexec(fd).map_err(|source| SnapshotError::PaneDescriptor {
            pane_id: pane.id,
            fd,
            source,
        })?;
        (Some(fd), pane.child_pid())
    } else {
        // Should not happen for a real production pane (every non-exited
        // pane owns a master until `mark_exited`); defensively degrade to
        // "no descriptor" rather than aborting the whole snapshot.
        log::warn!(
            "upgrade snapshot: pane {} is not exited but has no master descriptor; \
             recording it with no descriptor to adopt",
            pane.id
        );
        (None, None)
    };

    let (agent_state, agent_name, agent_revision) = {
        let status = pane.agent_status.lock().unwrap();
        (
            status.state.map(to_wire_state),
            status.name.clone(),
            status.revision,
        )
    };
    // task0004 (SPEC FR6): capture this pane's inferred-clear latch state
    // (task0001 `AgentStatusExitLatch`, wired per-pane by task0003)
    // verbatim, via the exact state components a caller outside the
    // latch's own module can read (`state_parts`) — never re-derived or
    // reset, so a pane mid-"command_ended" before the upgrade is still
    // mid-"command_ended" in the document.
    let (latch_armed, latch_command_ended, latch_generation) =
        pane.agent_status_exit_latch.lock().unwrap().state_parts();
    let scrollback = pane.scrollback.lock().unwrap().capture().data;
    // mux-hot-upgrade-alt-screen task0002 (SPEC FR5/FR8): a pane recorded
    // exited has no live alternate-screen semantics to carry (its restore
    // path builds a fresh shadow parser regardless) — only a non-exited
    // pane's CURRENT shadow-parser state is captured.
    let (alt_screen, alt_screen_dump) = if pane.exited {
        (false, Vec::new())
    } else {
        pane.capture_alt_state()
    };

    Ok(HandoffPane {
        id: pane.id,
        cols: pane.cols,
        rows: pane.rows,
        cwd: pane.cwd.lock().unwrap().clone(),
        title: pane.title.lock().unwrap().clone(),
        agent_state,
        agent_name,
        agent_revision,
        exited: pane.exited,
        child_pid,
        master_fd,
        scrollback,
        latch_armed,
        latch_command_ended,
        latch_generation,
        alt_screen,
        alt_screen_dump,
    })
}

/// Rebuild a [`SessionManager`] from a decoded, version-supported
/// [`HandoffDocument`] (Design steps 1-6).
///
/// Precondition: `document` was already decoded successfully (its schema
/// version is one this build supports).
///
/// Postcondition: the returned manager's identifier counters and
/// incarnation token equal the document's; sessions, windows and panes are
/// rebuilt in the document's recorded order, preserving window ordering and
/// active-entry selections; every pane recorded live has its descriptor
/// re-adopted (with writer and reader thread re-established) unless
/// adoption fails, in which case it is rebuilt as exited instead (AC-7,
/// logged); every pane recorded exited is rebuilt as exited without
/// adopting a descriptor (AC-6).
///
/// `title_tx` / `notification_tx` / `agent_status_tx` / `pane_exit_sender`
/// are the daemon's own lifetime channels (the same ones a freshly spawned
/// pane's reader thread is wired to) — the caller (task0004) creates them
/// once at daemon startup and passes them through here.
pub fn restore(
    document: &HandoffDocument,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> SessionManager {
    let mut mgr = SessionManager::for_restore(
        document.next_session_id,
        document.next_pane_id,
        document.incarnation.clone(),
    );

    for session_doc in &document.sessions {
        let mut windows: BTreeMap<u32, MuxWindow> = BTreeMap::new();
        for window_doc in &session_doc.windows {
            let mut panes: BTreeMap<u32, MuxPane> = BTreeMap::new();
            for pane_doc in &window_doc.panes {
                let pane = restore_pane(
                    pane_doc,
                    title_tx,
                    notification_tx,
                    agent_status_tx,
                    pane_exit_sender,
                );
                panes.insert(pane_doc.id, pane);
            }
            let window = MuxWindow::from_restored(
                window_doc.id,
                window_doc.name.clone(),
                panes,
                window_doc.active_pane_id,
                window_doc.next_pane_id,
            );
            windows.insert(window_doc.id, window);
        }
        let session = MuxSession::from_restored(
            session_doc.id,
            session_doc.name.clone(),
            windows,
            session_doc.window_order.clone(),
            session_doc.active_window_id,
            session_doc.next_window_id,
        );
        mgr.insert_session(session);
    }

    mgr
}

/// A freshly restored pane's initial output target: `Detached` with a
/// system origin (`owner: None`) — no client is attached immediately after
/// a hot-upgrade (IMPLEMENTATION.md D2: client connections are not
/// inherited), matching the existing "pane spawned before any client
/// attached" convention (`PaneOutputTarget::Detached` doc).
fn new_detached_output_target() -> SharedOutputTarget {
    Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }))
}

/// Build one pane from its document entry (AC-6/AC-7): live iff the
/// document recorded it live AND its descriptor adopts successfully;
/// exited otherwise (with the reason logged at `warn`, surviving release
/// filtering).
fn restore_pane(
    doc: &HandoffPane,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> MuxPane {
    let agent_status = AgentStatus {
        state: doc.agent_state.map(from_wire_state),
        name: doc.agent_name.clone(),
        revision: doc.agent_revision,
    };
    let scrollback = ScrollbackRingBuffer::load_snapshot(&ScrollbackSnapshot {
        capacity: DEFAULT_SCROLLBACK_CAPACITY,
        data: doc.scrollback.clone(),
    });
    // task0004 (SPEC FR6): reconstruct the pane's inferred-clear latch
    // from the document's raw state components, EXACTLY as recorded —
    // never reset to a fresh/disarmed latch and never re-derived from
    // agent_status — so a pane armed (or mid-"command_ended") before the
    // upgrade restores into that same state, for every restore outcome
    // below (live-adopted or exited).
    let latch = AgentStatusExitLatch::from_state_parts(
        doc.latch_armed,
        doc.latch_command_ended,
        doc.latch_generation,
    );

    if doc.exited {
        return build_exited_pane(doc, scrollback, agent_status, latch);
    }

    let Some(fd) = doc.master_fd else {
        log::warn!(
            "upgrade restore: pane {} recorded live but carries no master descriptor; \
             restoring as exited",
            doc.id
        );
        return build_exited_pane(doc, scrollback, agent_status, latch);
    };

    let master: Box<dyn MasterPty + Send> = match adopt_master(fd) {
        Ok(m) => m,
        Err(e) => {
            // AC-11 / medium performance finding: no explicit close needed
            // here — `adopt_master`'s own contract guarantees `fd` is
            // already closed on every `Err` path, so closing it again here
            // would double-close.
            log::warn!(
                "upgrade restore: pane {} master descriptor {} could not be adopted \
                 ({}); restoring as exited",
                doc.id,
                fd,
                e
            );
            return build_exited_pane(doc, scrollback, agent_status, latch);
        }
    };

    let writer = match master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            log::warn!(
                "upgrade restore: pane {} adopted master {} but take_writer failed \
                 ({}); restoring as exited",
                doc.id,
                fd,
                e
            );
            return build_exited_pane(doc, scrollback, agent_status, latch);
        }
    };

    let reader = match master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "upgrade restore: pane {} adopted master {} but try_clone_reader \
                 failed ({}); restoring as exited",
                doc.id,
                fd,
                e
            );
            return build_exited_pane(doc, scrollback, agent_status, latch);
        }
    };

    let pane = MuxPane::from_restored(
        doc.id,
        doc.cols,
        doc.rows,
        new_detached_output_target(),
        writer,
        master,
        scrollback,
        doc.cwd.clone(),
        doc.title.clone(),
        agent_status,
        doc.child_pid,
        // mux-hot-upgrade-alt-screen task0002 (SPEC FR6): pass the
        // document's alt-screen flag + dump through verbatim — `from_restored`
        // replays them onto the shadow parser after the scrollback replay.
        doc.alt_screen,
        doc.alt_screen_dump.clone(),
    );
    // task0004: install the restored latch state BEFORE the reader thread
    // starts observing this pane's live PTY stream, so the earliest live
    // OSC 133 mark after restore is evaluated against the CORRECT
    // pre-upgrade state, never a fresh `AgentStatusExitLatch::new()`.
    *pane.agent_status_exit_latch.lock().unwrap() = latch;
    spawn_restored_reader_thread(
        &pane,
        reader,
        title_tx.clone(),
        notification_tx.clone(),
        agent_status_tx.clone(),
        pane_exit_sender.clone(),
    );
    pane
}

/// Build an already-exited pane for the restore path (AC-6/AC-7): shared by
/// every "cannot / should not adopt a descriptor" branch in
/// [`restore_pane`]. `latch` is installed verbatim (task0004) — an exited
/// pane still carries whatever inferred-clear latch state it had at the
/// moment of the upgrade, even though it can no longer receive live marks.
fn build_exited_pane(
    doc: &HandoffPane,
    scrollback: ScrollbackRingBuffer,
    agent_status: AgentStatus,
    latch: AgentStatusExitLatch,
) -> MuxPane {
    let pane = MuxPane::from_restored_exited(
        doc.id,
        doc.cols,
        doc.rows,
        new_detached_output_target(),
        scrollback,
        doc.cwd.clone(),
        doc.title.clone(),
        agent_status,
    );
    *pane.agent_status_exit_latch.lock().unwrap() = latch;
    pane
}

/// Adopt `fd` as a `MasterPty` through task0002's inherited master adapter,
/// restoring `FD_CLOEXEC` on success (Design "Descriptor lifetime": snapshot
/// cleared it so `fd` would survive `execve`; a pane spawned AFTER this
/// hot-upgrade must never inherit it, AC-4).
///
/// Contract (AC-11 / medium performance finding: a fd that fails adoption
/// must never leak): on any `Err`, `fd` is guaranteed already closed —
/// callers must not close it again.
/// - [`InheritedMasterPty::new`] failing means it never took ownership (its
///   own documented contract), so this function closes `fd` itself before
///   returning.
/// - `set_cloexec` failing AFTER a successful adopt means `fd` IS owned (by
///   the local `master`); returning `Err` here lets `master` drop at the end
///   of this function's scope, which closes `fd` exactly once through
///   [`InheritedMasterPty`]'s own `Drop` impl — no second explicit close.
fn adopt_master(fd: RawFd) -> anyhow::Result<Box<dyn MasterPty + Send>> {
    let master = match InheritedMasterPty::new(fd) {
        Ok(m) => m,
        Err(e) => {
            // SAFETY: `fd` is still owned by the caller at this point
            // (construction failed without taking ownership); nothing else
            // references it.
            unsafe {
                libc::close(fd);
            }
            return Err(e);
        }
    };
    set_cloexec(master.raw_fd())?;
    Ok(Box::new(master))
}

/// Re-establish a restored live pane's reader thread using the SAME wiring
/// (`pty_reader_loop`) a freshly spawned pane's reader thread gets — not a
/// reimplementation — so a restored pane's scrollback filtering,
/// agent-status forwarding, and title/cwd detection stay byte-for-byte
/// identical to a freshly spawned pane's.
fn spawn_restored_reader_thread(
    pane: &MuxPane,
    reader: Box<dyn Read + Send>,
    title_tx: TitleChangeSender,
    notification_tx: NotificationSender,
    agent_status_tx: AgentStatusReportSender,
    pane_exit_sender: SharedPaneExitSender,
) {
    let pane_id = pane.id;
    let output_target = pane.output_target.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let pane_cwd = pane.cwd.clone();
    let pane_title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let pane_dims = pane.dims.clone();

    // Populate the swappable/daemon-lifetime senders BEFORE the reader
    // thread starts, mirroring `register_pane_and_start_reader`'s wiring
    // for a freshly spawned pane.
    *title_sender.lock().unwrap() = Some(title_tx);
    *notification_sender.lock().unwrap() = Some(notification_tx);
    *agent_status_report_sender.lock().unwrap() = Some(agent_status_tx);

    std::thread::spawn(move || {
        crate::mux::ipc::pty_spawn::pty_reader_loop(
            pane_id,
            reader,
            output_target,
            shadow_parser,
            pane_cwd,
            pane_title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            pane_dims,
            pane_exit_sender,
        );
    });
}

#[cfg(test)]
mod tests;
