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
    let result = std::fs::read(path).map_err(HandoffReadError::Io).and_then(|bytes| {
        decode_handoff_document(&bytes).map_err(HandoffReadError::Decode)
    });
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
                    (status.state.map(to_wire_state), status.name.clone(), status.revision)
                };
                let (latch_armed, latch_command_ended, latch_generation) =
                    pane.agent_status_exit_latch.lock().unwrap().state_parts();

                pane_doc.agent_state = agent_state;
                pane_doc.agent_name = agent_name;
                pane_doc.agent_revision = agent_revision;
                pane_doc.latch_armed = latch_armed;
                pane_doc.latch_command_ended = latch_command_ended;
                pane_doc.latch_generation = latch_generation;
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
    write_handoff_file(&document, &path).map_err(|source| SnapshotError::WriteFile {
        path,
        source,
    })?;

    Ok(document)
}

/// Build one pane's document entry (Design steps 2/4): a non-exited pane
/// contributes its master descriptor (`FD_CLOEXEC` cleared) and child pid;
/// an exited pane contributes neither. Scrollback is captured while the
/// pane's own scrollback lock is held (via [`ScrollbackRingBuffer::capture`]).
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
        (status.state.map(to_wire_state), status.name.clone(), status.revision)
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
mod tests {
    use super::*;
    use crate::agent_status::{AgentState, AgentStatusEvent};
    use crate::mux::session::pane::PaneOutputTarget;
    use crate::prompts::PromptMarkKind;
    use std::os::unix::io::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use tokio::sync::mpsc;

    fn test_output_target() -> SharedOutputTarget {
        let (tx, _rx) = mpsc::channel(1);
        StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
    }

    fn test_restore_channels() -> (
        TitleChangeSender,
        NotificationSender,
        AgentStatusReportSender,
        SharedPaneExitSender,
    ) {
        let (title_tx, _title_rx) = mpsc::channel(16);
        let (notification_tx, _notification_rx) = mpsc::channel(16);
        let (agent_status_tx, _agent_status_rx) = mpsc::channel(16);
        let (pane_exit_tx, _pane_exit_rx) = mpsc::channel(16);
        let pane_exit_sender: SharedPaneExitSender = StdArc::new(StdMutex::new(Some(pane_exit_tx)));
        (title_tx, notification_tx, agent_status_tx, pane_exit_sender)
    }

    fn has_cloexec(fd: RawFd) -> bool {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0, "fcntl(F_GETFD) failed");
        flags & libc::FD_CLOEXEC != 0
    }

    /// `(st_dev, st_ino)` for `fd`, or `None` if `fd` is not currently open.
    /// Used (rather than a plain "is this fd number valid" check) so a test
    /// asserting a descriptor was closed stays correct even when `cargo
    /// test`'s parallel threads — sharing one process-wide fd table — hand
    /// that freed NUMBER to an unrelated descriptor from a concurrently
    /// running test before the assertion runs (mirrors
    /// `inherited_pty.rs::tests::stat_rdev`'s identical rationale, adapted to
    /// regular files via device+inode identity instead of a device's rdev).
    fn stat_dev_ino(fd: RawFd) -> Option<(u64, u64)> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::fstat(fd, &mut st) };
        if rc == 0 {
            Some((st.st_dev, st.st_ino))
        } else {
            None
        }
    }

    fn minimal_document() -> HandoffDocument {
        HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "deadbeef".to_string(),
            listen_fd: 3,
            next_session_id: 1,
            next_pane_id: 1,
            sessions: Vec::new(),
        }
    }

    // ── AC-1 / AC-3: tree / ordering / active selections / counters /
    // incarnation round-trip ──────────────────────────────────────────────

    #[test]
    fn snapshot_then_restore_round_trips_tree_ordering_active_selections_counters_and_incarnation()
     {
        let mut mgr = SessionManager::new();
        let sid1 = mgr.create_session("first".to_string());
        let wid1a = mgr.create_window(sid1, "a".to_string()).unwrap();
        let wid1b = mgr.create_window(sid1, "b".to_string()).unwrap();
        let pid1 = mgr.alloc_pane_id();
        let pid2 = mgr.alloc_pane_id();
        {
            let session = mgr.get_session_mut(sid1).unwrap();
            session
                .windows
                .get_mut(&wid1a)
                .unwrap()
                .add_pane(MuxPane::new_test(pid1, 80, 24, test_output_target()));
            session
                .windows
                .get_mut(&wid1b)
                .unwrap()
                .add_pane(MuxPane::new_test(pid2, 100, 30, test_output_target()));
            // Reorder so wid1b becomes first — proves window_order (not
            // creation order) is what round-trips.
            assert!(session.move_window(wid1b, 0));
        }
        let sid2 = mgr.create_session("second".to_string());
        let wid2 = mgr.create_window(sid2, "c".to_string()).unwrap();
        let pid3 = mgr.alloc_pane_id();
        mgr.get_session_mut(sid2)
            .unwrap()
            .windows
            .get_mut(&wid2)
            .unwrap()
            .add_pane(MuxPane::new_test(pid3, 80, 24, test_output_target()));

        let incarnation = mgr.incarnation().to_string();
        let next_session_id = mgr.next_session_id_counter();
        let next_pane_id = mgr.next_pane_id_counter();

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();

        let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");

        assert_eq!(document.incarnation, incarnation);
        assert_eq!(document.next_session_id, next_session_id);
        assert_eq!(document.next_pane_id, next_pane_id);
        assert_eq!(document.sessions.len(), 2);
        assert_eq!(document.sessions[0].id, sid1);
        assert_eq!(
            document.sessions[0].window_order,
            vec![wid1b, wid1a],
            "reordered window_order must be preserved"
        );
        assert_eq!(
            document.sessions[0].active_window_id,
            mgr.get_session(sid1).unwrap().active_window_id
        );
        assert_eq!(document.sessions[1].id, sid2);

        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_restore_channels();
        let mut restored = restore(
            &document,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );

        assert_eq!(restored.incarnation(), incarnation);
        assert_eq!(restored.next_session_id_counter(), next_session_id);
        assert_eq!(restored.next_pane_id_counter(), next_pane_id);
        assert_eq!(
            restored.alloc_pane_id(),
            next_pane_id,
            "next allocated pane id must continue the original sequence"
        );

        let restored_s1 = restored.get_session(sid1).expect("session 1 must restore");
        assert_eq!(restored_s1.window_order, vec![wid1b, wid1a]);
        assert_eq!(
            restored_s1.windows.get(&wid1b).unwrap().name,
            "b",
            "window identity must be preserved despite the reorder"
        );
        assert!(restored.get_session(sid2).is_some(), "session 2 must restore");
    }

    // ── AC-2: descriptor flags cleared, verified by querying them ─────────

    #[test]
    fn snapshot_clears_cloexec_on_live_pane_master_and_listen_descriptor() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let master_fd = pair.master.as_raw_fd().unwrap();
        assert!(
            has_cloexec(master_fd),
            "portable_pty opens the master with FD_CLOEXEC set (precondition)"
        );
        let writer = pair.master.take_writer().unwrap();
        let mut mgr = SessionManager::new();
        let sid = mgr.create_session("s".to_string());
        let wid = mgr.create_window(sid, "w".to_string()).unwrap();
        let pid = mgr.alloc_pane_id();
        let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();
        assert!(
            has_cloexec(listen_fd),
            "freshly opened files default to FD_CLOEXEC (precondition)"
        );

        snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");

        let live_master_fd = mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap()
            .master_raw_fd()
            .unwrap();
        assert_eq!(live_master_fd, master_fd);
        assert!(
            !has_cloexec(live_master_fd),
            "snapshot must clear FD_CLOEXEC on the live pane's master descriptor"
        );
        assert!(
            !has_cloexec(listen_fd),
            "snapshot must clear FD_CLOEXEC on the listen descriptor"
        );
    }

    // ── AC-4 / AC-5: scrollback byte-for-byte + restored pane stays
    // writable through its (re-adopted) master ────────────────────────────

    #[test]
    fn snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let mut mgr = SessionManager::new();
        let sid = mgr.create_session("s".to_string());
        let wid = mgr.create_window(sid, "w".to_string()).unwrap();
        let pid = mgr.alloc_pane_id();
        let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
        pane.scrollback
            .lock()
            .unwrap()
            .write(b"hello from before the upgrade");
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();

        let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
        let pane_doc = &document.sessions[0].windows[0].panes[0];
        assert_eq!(pane_doc.scrollback, b"hello from before the upgrade");
        let original_fd = pane_doc.master_fd.expect("live pane must record a master fd");

        // Simulate the fd surviving a process replacement: duplicate it so
        // the restore path (which will close its adopted copy on drop)
        // never races the ORIGINAL pane's own master (still alive in `mgr`,
        // which a real hot-upgrade would have replaced entirely) — same
        // technique `inherited_pty`'s own tests use to avoid a double-close.
        let dup_fd = unsafe { libc::dup(original_fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        document.sessions[0].windows[0].panes[0].master_fd = Some(dup_fd);

        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_restore_channels();
        let restored = restore(
            &document,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );

        let restored_session = restored.get_session(sid).unwrap();
        let restored_window = restored_session.windows.get(&wid).unwrap();
        let restored_pane = restored_window.panes.get(&pid).unwrap();
        assert!(
            !restored_pane.exited,
            "a live pane's descriptor must adopt successfully in this test"
        );
        assert_eq!(
            restored_pane.scrollback.lock().unwrap().read_all(),
            b"hello from before the upgrade"
        );
        // AC-5: the restored pane is still writable through its adopted
        // master (the dedicated pane-level test
        // `from_restored_pane_can_write_and_read_through_its_adopted_master`
        // additionally proves the read side deterministically, without a
        // background reader thread in the way).
        restored_pane
            .write_input(b"after-restore\n")
            .expect("restored pane must remain writable through its adopted master");
    }

    // ── AC-6 / AC-7: exited-pane restore + unadoptable-descriptor restore,
    // rest of the tree still restores ──────────────────────────────────────

    #[test]
    fn restore_handles_exited_and_unadoptable_panes_while_the_rest_of_the_tree_still_restores() {
        let bogus_fd: i32 = 999_999; // never a live fd in the test process
        let doc = HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "abc123".to_string(),
            listen_fd: 3,
            next_session_id: 2,
            next_pane_id: 3,
            sessions: vec![HandoffSession {
                id: 1,
                name: "s".to_string(),
                window_order: vec![1],
                active_window_id: Some(1),
                next_window_id: 2,
                windows: vec![HandoffWindow {
                    id: 1,
                    name: "w".to_string(),
                    active_pane_id: Some(1),
                    next_pane_id: 3,
                    panes: vec![
                        // AC-7: recorded live, but the descriptor cannot be
                        // adopted.
                        HandoffPane {
                            id: 1,
                            cols: 80,
                            rows: 24,
                            cwd: None,
                            title: None,
                            agent_state: None,
                            agent_name: None,
                            agent_revision: 0,
                            exited: false,
                            child_pid: Some(1234),
                            master_fd: Some(bogus_fd),
                            scrollback: b"leftover".to_vec(),
                            // task0004: a mid-flight latch, to prove it
                            // still carries over even when the descriptor
                            // fails to adopt and the pane restores as
                            // exited (AC-7's outcome).
                            latch_armed: true,
                            latch_command_ended: true,
                            latch_generation: 5,
                        },
                        // AC-6: recorded exited — adopts no descriptor.
                        HandoffPane {
                            id: 2,
                            cols: 80,
                            rows: 24,
                            cwd: Some("/tmp".to_string()),
                            title: None,
                            agent_state: None,
                            agent_name: None,
                            agent_revision: 0,
                            exited: true,
                            child_pid: None,
                            master_fd: None,
                            scrollback: Vec::new(),
                            latch_armed: false,
                            latch_command_ended: false,
                            latch_generation: 0,
                        },
                    ],
                }],
            }],
        };

        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_restore_channels();
        let mgr = restore(
            &doc,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        );

        // The rest of the tree still restores despite pane 1's unadoptable
        // descriptor.
        let session = mgr.get_session(1).expect("session must still restore");
        let window = session.windows.get(&1).expect("window must still restore");

        let pane1 = window.panes.get(&1).unwrap();
        assert!(
            pane1.exited,
            "AC-7: an unadoptable descriptor must restore the pane as exited"
        );
        assert_eq!(
            pane1.scrollback.lock().unwrap().read_all(),
            b"leftover",
            "non-descriptor attributes still restore even when adoption fails"
        );
        // AC-7 "the reason is logged": verified by inspection of the
        // `log::warn!` call in `restore_pane`'s adopt-failure arm (matching
        // this project's established convention for asserting on log output
        // — see `child_reaper.rs`'s equivalent tests).
        // task0004: the inferred-clear latch still carries over verbatim
        // even on the "restore as exited" path taken when a live pane's
        // descriptor fails to adopt.
        assert_eq!(
            *pane1.agent_status_exit_latch.lock().unwrap(),
            AgentStatusExitLatch::from_state_parts(true, true, 5),
            "task0004: a mid-flight latch must survive restore even when the pane's \
             descriptor could not be adopted"
        );

        let pane2 = window.panes.get(&2).unwrap();
        assert!(pane2.exited, "AC-6: a pane recorded exited restores as exited");
        assert_eq!(*pane2.cwd.lock().unwrap(), Some("/tmp".to_string()));
        assert_eq!(
            *pane2.agent_status_exit_latch.lock().unwrap(),
            AgentStatusExitLatch::new(),
            "task0004: a disarmed latch restores disarmed"
        );
    }

    // ── AC-4: restore sets close-on-exec back on every adopted descriptor,
    // and a pane spawned AFTERWARDS does not inherit it ────────────────────

    #[test]
    fn restore_sets_cloexec_back_on_the_adopted_pane_master() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let master_fd = pair.master.as_raw_fd().unwrap();
        // Simulate what snapshot does pre-exec: clear CLOEXEC on a
        // (duplicated, to avoid a double-close race with `pair.master`'s own
        // Drop) descriptor recording this pane as live.
        let dup_fd = unsafe { libc::dup(master_fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec");
        assert!(
            !has_cloexec(dup_fd),
            "sanity: CLOEXEC must be cleared before adoption, mirroring a real handoff"
        );

        let master = adopt_master(dup_fd).expect("a live descriptor must adopt");

        assert!(
            has_cloexec(master.as_raw_fd().unwrap()),
            "AC-4: restore must set CLOEXEC back on the adopted descriptor"
        );
    }

    /// AC-4 (continued): a pane child spawned AFTER a restored pane's master
    /// is adopted must not inherit that master's descriptor — proven with a
    /// REAL spawned child inspecting its own fd table, per the Test Notes
    /// ("AC-4 needs a real spawned child to inspect what it inherited").
    #[test]
    fn a_pane_child_spawned_after_restore_does_not_inherit_the_adopted_master() {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let master_fd = pair.master.as_raw_fd().unwrap();
        let dup_fd = unsafe { libc::dup(master_fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec");

        let master = adopt_master(dup_fd).expect("a live descriptor must adopt");
        let adopted_fd = master.as_raw_fd().unwrap();

        // A fresh child process, spawned in THIS process after adoption —
        // standing in for a brand-new pane's shell spawned after the
        // hot-upgrade. `std::process::Command`'s fork+exec inherits every
        // open descriptor that lacks FD_CLOEXEC; if the adopted master were
        // still missing that flag, `/proc/self/fd/<adopted_fd>` would exist
        // in the CHILD too.
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "if [ -e /proc/self/fd/{adopted_fd} ]; then echo INHERITED; else echo NOT_INHERITED; fi"
            ))
            .output()
            .expect("spawn a real child process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("NOT_INHERITED"),
            "AC-4: a pane child spawned after restore must not inherit the adopted master \
             (child fd table output: {stdout:?})"
        );

        drop(master);
    }

    // ── AC-11 / medium performance finding: a descriptor that fails
    // adoption is closed, not leaked ────────────────────────────────────────

    #[test]
    fn adopt_master_closes_the_descriptor_when_construction_fails() {
        // An ordinary (non-terminal) file: `InheritedMasterPty::new`'s own
        // `isatty` check fails, so `adopt_master` must close it itself
        // (Contract: "on any Err, fd is guaranteed already closed").
        let file = tempfile::tempfile().expect("must create a temp file");
        let fd = file.as_raw_fd();
        // Duplicate so we can assert on the ORIGINAL descriptor number
        // independently of `file`'s own (later) Drop.
        let dup_fd = unsafe { libc::dup(fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        // Identity of the file `dup_fd` currently refers to, captured before
        // adoption closes it. Compared by (dev, ino) rather than fd validity
        // alone (matching `inherited_pty.rs`'s established convention): if
        // `cargo test`'s parallel threads — sharing one process-wide fd
        // table — hand this freed NUMBER to an unrelated descriptor from a
        // concurrently running test before the assertion below runs, that
        // descriptor can only ever refer to a DIFFERENT file, which still
        // proves `dup_fd` was closed.
        let original_identity = stat_dev_ino(dup_fd).expect("sanity: fd must be open before adoption");

        let result = adopt_master(dup_fd);
        assert!(result.is_err(), "a non-terminal fd must fail adoption");

        match stat_dev_ino(dup_fd) {
            None => {}
            Some(identity) => assert_ne!(
                identity,
                original_identity,
                "AC-11: a descriptor that fails adoption must be closed, not leaked"
            ),
        }
    }

    // ── AC-6: listener adoption validates before taking ownership ─────────

    #[test]
    fn adopt_listener_succeeds_over_a_real_listening_unix_socket_and_sets_cloexec() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("adopt-listener.sock");
        let std_listener =
            std::os::unix::net::UnixListener::bind(&sock_path).expect("bind real listener");
        let fd = std_listener.as_raw_fd();
        // Duplicate so `adopt_listener` can take ownership of ITS OWN
        // descriptor independently of `std_listener`'s own Drop (which would
        // otherwise double-close the same fd number once this test ends).
        let dup_fd = unsafe { libc::dup(fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        clear_cloexec(dup_fd).expect("simulate snapshot's clear_cloexec on the listen descriptor");

        let adopted = adopt_listener(dup_fd).expect("a real listening AF_UNIX socket must adopt");

        assert_eq!(adopted.as_raw_fd(), dup_fd);
        assert!(
            has_cloexec(dup_fd),
            "AC-4: adopt_listener must set CLOEXEC back on the adopted listener"
        );
    }

    #[test]
    fn adopt_listener_rejects_a_non_socket_descriptor_without_taking_ownership() {
        let file = tempfile::tempfile().expect("must create a temp file");
        let fd = file.as_raw_fd();

        let result = adopt_listener(fd);

        assert!(result.is_err(), "a regular file must not adopt as a listener");
        // AC-6: the descriptor is still open and unchanged afterward — no
        // ownership was taken, so `file`'s own (still-live) handle proves
        // this by continuing to work normally.
        assert!(
            std::fs::metadata(format!("/proc/self/fd/{fd}")).is_ok(),
            "AC-6: a failed adoption must leave the descriptor open and unchanged"
        );
        drop(file);
    }

    #[test]
    fn adopt_listener_rejects_a_connected_non_listening_socket_without_taking_ownership() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("not-listening.sock");
        let listener =
            std::os::unix::net::UnixListener::bind(&sock_path).expect("bind real listener");
        let client =
            std::os::unix::net::UnixStream::connect(&sock_path).expect("connect a client socket");
        let fd = client.as_raw_fd();

        let result = adopt_listener(fd);

        assert!(
            result.is_err(),
            "AC-6: a connected (non-listening) socket must not adopt as a listener"
        );
        assert!(
            std::fs::metadata(format!("/proc/self/fd/{fd}")).is_ok(),
            "AC-6: a failed adoption must leave the descriptor open and unchanged"
        );
        drop(client);
        drop(listener);
    }

    // ── AC-8: handoff file 0600, removed in all three outcomes ────────────

    #[test]
    fn handoff_file_is_created_0600_and_removed_after_a_successful_read() {
        let mgr = SessionManager::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();

        let document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
        let path = handoff_file_path(&socket_path);
        assert!(path.exists(), "handoff file must be created");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "handoff file must be owner-only (0600)");

        let read_back = read_and_remove_handoff_file(&path).expect("read must succeed");
        assert_eq!(read_back, document);
        assert!(!path.exists(), "handoff file must be removed after a successful read");
    }

    #[test]
    fn handoff_file_is_removed_after_a_failed_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mux-handoff.bin");
        std::fs::write(&path, b"not a valid handoff document").unwrap();
        assert!(path.exists());

        let result = read_and_remove_handoff_file(&path);
        assert!(result.is_err(), "malformed bytes must fail to decode");
        assert!(
            !path.exists(),
            "handoff file must be removed even after a failed read"
        );
    }

    /// Forces `write_all` to fail deterministically via `/dev/full` (a
    /// standard Linux device that always reports `ENOSPC` on write),
    /// instead of manipulating a live `std::fs::File`'s raw descriptor
    /// directly — closing an fd a `File` still believes it owns trips
    /// std's own IO-safety double-close abort, so that technique is
    /// unusable here. `path` still refers to a REAL regular file (created
    /// via `create_handoff_file`, matching production's real target), so
    /// the removal this test asserts on is genuine.
    #[test]
    fn write_bytes_or_remove_cleans_up_the_file_when_the_write_itself_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mux-handoff.bin");
        // Create the real target file (as `write_handoff_file` would), then
        // release the handle — only the on-disk file matters for the
        // removal assertion below.
        drop(create_handoff_file(&path).expect("create must succeed"));
        assert!(path.exists());

        let full_device = OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("/dev/full must be openable for writing on Linux");

        let result = write_bytes_or_remove(full_device, &path, b"some bytes");
        assert!(
            result.is_err(),
            "every write to /dev/full must fail with ENOSPC"
        );
        assert!(
            !path.exists(),
            "a partially written handoff file must be removed on write failure \
             (aborted-snapshot outcome)"
        );
    }

    #[test]
    fn write_handoff_file_refuses_to_overwrite_an_existing_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mux-handoff.bin");
        std::fs::write(&path, b"leftover").unwrap();

        let result = write_handoff_file(&minimal_document(), &path);
        assert!(
            result.is_err(),
            "must fail rather than follow/reuse an existing path"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"leftover",
            "the pre-existing file must be left untouched"
        );
    }

    // ── task0004 AC-1/AC-2/AC-3: the daemon-side inferred-clear latch
    // (task0001 AgentStatusExitLatch, wired per-pane by task0003) survives a
    // snapshot/restore round trip in each of its three reachable states,
    // preserved verbatim rather than reset or re-derived ────────────────────

    /// Build a live pane on a real PTY, registered in a fresh single-session/
    /// single-window `SessionManager`, mirroring this file's own established
    /// live-pane round-trip fixture (see
    /// `snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable`).
    /// Returns the manager plus the ids needed to look the pane back up.
    fn single_live_pane_manager() -> (SessionManager, u32, u32, u32) {
        let pty_system = portable_pty::native_pty_system();
        let size = portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let mut mgr = SessionManager::new();
        let sid = mgr.create_session("s".to_string());
        let wid = mgr.create_window(sid, "w".to_string()).unwrap();
        let pid = mgr.alloc_pane_id();
        let pane = MuxPane::new(pid, 80, 24, test_output_target(), writer, pair.master, None);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        (mgr, sid, wid, pid)
    }

    /// Snapshot `mgr` and restore it back, simulating the fd surviving a
    /// process replacement by duplicating the recorded master fd before
    /// restoring (same technique
    /// `snapshot_then_restore_round_trips_scrollback_and_the_restored_pane_is_writable`
    /// uses, so the ORIGINAL pane still alive in `mgr` is never double-closed).
    fn snapshot_then_restore_live_pane(mgr: &SessionManager) -> SessionManager {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();

        let mut document = snapshot(mgr, listen_fd, &socket_path).expect("snapshot must succeed");
        let pane_doc = &mut document.sessions[0].windows[0].panes[0];
        let original_fd = pane_doc.master_fd.expect("live pane must record a master fd");
        let dup_fd = unsafe { libc::dup(original_fd) };
        assert!(dup_fd >= 0, "dup(2) failed");
        pane_doc.master_fd = Some(dup_fd);

        let (title_tx, notification_tx, agent_status_tx, pane_exit_sender) =
            test_restore_channels();
        restore(
            &document,
            &title_tx,
            &notification_tx,
            &agent_status_tx,
            &pane_exit_sender,
        )
    }

    /// AC-1: a pane whose latch is armed (`Set` observed, no `D`/`A` yet) at
    /// the moment of a hot-upgrade remains armed with the SAME generation
    /// immediately after the upgrade completes.
    #[test]
    fn armed_latch_with_no_d_or_a_yet_survives_restore_with_the_same_generation() {
        let (mut mgr, sid, wid, pid) = single_live_pane_manager();
        {
            let pane = mgr
                .get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .panes
                .get_mut(&pid)
                .unwrap();
            pane.apply_agent_status_event(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            });
        }
        let expected_latch = *mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap()
            .agent_status_exit_latch
            .lock()
            .unwrap();
        assert_eq!(
            expected_latch,
            AgentStatusExitLatch::from_state_parts(true, false, 1),
            "sanity: a bare Set arms the latch with generation 1"
        );

        let restored = snapshot_then_restore_live_pane(&mgr);

        let restored_pane = restored
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        assert!(
            !restored_pane.exited,
            "AC-1 setup: the pane's descriptor must adopt successfully in this test"
        );
        assert_eq!(
            *restored_pane.agent_status_exit_latch.lock().unwrap(),
            expected_latch,
            "AC-1: an armed latch (Set observed, no D/A yet) must survive a hot-upgrade \
             with the same generation"
        );
    }

    /// AC-2: a pane whose latch has recorded `command_ended` (`Set` -> live
    /// `D`, no `A` yet) at the moment of a hot-upgrade still fires exactly
    /// one inferred clear when its matching `A` arrives after the upgrade.
    #[test]
    fn command_ended_latch_fires_exactly_once_on_the_first_live_a_after_restore() {
        let (mut mgr, sid, wid, pid) = single_live_pane_manager();
        {
            let pane = mgr
                .get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .panes
                .get_mut(&pid)
                .unwrap();
            pane.apply_agent_status_event(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            });
            let fired = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
            assert_eq!(fired, None, "a lone D must not fire a clear");
        }

        let restored = snapshot_then_restore_live_pane(&mgr);

        let restored_pane = restored
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        assert!(
            !restored_pane.exited,
            "AC-2 setup: the pane's descriptor must adopt successfully in this test"
        );
        assert_eq!(
            *restored_pane.agent_status_exit_latch.lock().unwrap(),
            AgentStatusExitLatch::from_state_parts(true, true, 1),
            "AC-2 setup: the restored latch must still record command_ended"
        );

        let fired = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert!(
            fired.is_some(),
            "AC-2: the pending D->A transition must fire an inferred clear after restore"
        );
        assert_eq!(restored_pane.agent_status.lock().unwrap().state, None);

        let fired_again = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(
            fired_again, None,
            "AC-2: a second live A must not re-fire an already-fired latch"
        );
    }

    /// AC-3: a pane whose latch is disarmed (no `Set` since the last
    /// `Clear`) at the moment of a hot-upgrade remains disarmed after the
    /// upgrade — no spurious clear and no false arm introduced by the
    /// transfer itself.
    #[test]
    fn disarmed_latch_stays_disarmed_after_restore_with_no_spurious_clear() {
        let (mut mgr, sid, wid, pid) = single_live_pane_manager();
        {
            let pane = mgr
                .get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .panes
                .get_mut(&pid)
                .unwrap();
            pane.apply_agent_status_event(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: None,
            });
            pane.apply_agent_status_event(AgentStatusEvent::Clear);
        }
        let expected_latch = *mgr
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap()
            .agent_status_exit_latch
            .lock()
            .unwrap();
        let (armed, command_ended, _generation) = expected_latch.state_parts();
        assert!(
            !armed && !command_ended,
            "sanity: Set then Clear leaves the latch disarmed"
        );

        let restored = snapshot_then_restore_live_pane(&mgr);

        let restored_pane = restored
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap();
        assert!(
            !restored_pane.exited,
            "AC-3 setup: the pane's descriptor must adopt successfully in this test"
        );
        assert_eq!(
            *restored_pane.agent_status_exit_latch.lock().unwrap(),
            expected_latch,
            "AC-3: a disarmed latch must be transferred verbatim, not reset or re-derived"
        );

        let revision_before = restored_pane.agent_status.lock().unwrap().revision;
        let d_result = restored_pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
        let a_result = restored_pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(d_result, None, "AC-3: no false arm — a live D must not fire while disarmed");
        assert_eq!(
            a_result, None,
            "AC-3: no spurious clear — a live A must not fire while disarmed"
        );
        assert_eq!(
            restored_pane.agent_status.lock().unwrap().revision,
            revision_before,
            "AC-3: neither D nor A may change agent_status while the latch is disarmed"
        );
    }

    // ── task0006 (review rework, finding 2e6f18b4dc0a7593): refresh closes
    // the torn-snapshot window for live agent-status/latch state ──────────

    /// Reproduces the finding's exact example: a live D->A transition
    /// completing AFTER `snapshot` already wrote the document (simulating
    /// the daemon's agent-status task consuming a live mark during
    /// `prepare_upgrade`'s post-snapshot client-acknowledgement wait) must
    /// be reflected by `refresh_live_agent_state` -- otherwise the
    /// successor would restore a latch waiting for an `A` that was already
    /// consumed in THIS process and will never arrive again.
    #[test]
    fn refresh_live_agent_state_pulls_in_a_clear_that_completed_after_snapshot() {
        let (mut mgr, sid, wid, pid) = single_live_pane_manager();
        {
            let pane = mgr
                .get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .panes
                .get_mut(&pid)
                .unwrap();
            pane.apply_agent_status_event(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            });
            let fired = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
            assert_eq!(fired, None, "a lone D must not fire a clear yet");
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();

        let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
        {
            let pane_doc = &document.sessions[0].windows[0].panes[0];
            assert!(
                pane_doc.latch_armed && pane_doc.latch_command_ended,
                "the original snapshot must record the pre-clear armed/command_ended state"
            );
            assert_eq!(
                pane_doc.agent_state,
                Some(to_wire_state(AgentState::Working)),
                "the original snapshot must record the pre-clear agent_state"
            );
        }

        // Simulate the daemon's agent-status task consuming a live A DURING
        // prepare_upgrade's post-snapshot acknowledgement wait -- exactly
        // the window the finding describes -- WITHOUT re-snapshotting.
        let fired = {
            let pane = mgr
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pid)
                .unwrap();
            pane.record_live_osc133_mark(PromptMarkKind::PromptStart)
        };
        assert!(
            fired.is_some(),
            "the pending D->A transition must fire an inferred clear in THIS process"
        );

        refresh_live_agent_state(&mut document, &mgr);

        let refreshed_pane_doc = &document.sessions[0].windows[0].panes[0];
        assert!(
            !refreshed_pane_doc.latch_armed && !refreshed_pane_doc.latch_command_ended,
            "task0006: refresh must pull in the disarm that happened after the original snapshot"
        );
        assert_eq!(
            refreshed_pane_doc.agent_state, None,
            "task0006: refresh must pull in the inferred clear's agent_state update"
        );
    }

    /// A pane that exits (in `mgr`) after the original snapshot must be left
    /// exactly as originally recorded -- refreshing exited-pane state is a
    /// separate, pre-existing concern this function does not attempt to fix
    /// (see its doc comment).
    #[test]
    fn refresh_live_agent_state_leaves_a_pane_that_since_exited_untouched() {
        let (mut mgr, sid, wid, pid) = single_live_pane_manager();
        {
            let pane = mgr
                .get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .panes
                .get_mut(&pid)
                .unwrap();
            pane.apply_agent_status_event(AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            });
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let listen_file = tempfile::NamedTempFile::new_in(dir.path()).expect("listen fd stand-in");
        let listen_fd = listen_file.as_raw_fd();
        let mut document = snapshot(&mgr, listen_fd, &socket_path).expect("snapshot must succeed");
        let recorded = document.sessions[0].windows[0].panes[0].clone();

        // The pane exits in `mgr`, AND its agent_status is explicitly
        // cleared, AFTER the snapshot above -- proving refresh really SKIPS
        // an exited pane rather than happening to leave the same value.
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .panes
            .get_mut(&pid)
            .unwrap()
            .mark_exited();
        mgr.get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&pid)
            .unwrap()
            .apply_agent_status_event(AgentStatusEvent::Clear);

        refresh_live_agent_state(&mut document, &mgr);

        assert_eq!(
            document.sessions[0].windows[0].panes[0], recorded,
            "task0006: a pane that has since exited must be left exactly as originally recorded"
        );
    }

    /// A document pane whose id no longer resolves in `mgr` (e.g. destroyed
    /// between snapshot and refresh) must be left untouched, not panic.
    #[test]
    fn refresh_live_agent_state_leaves_a_pane_no_longer_present_in_the_manager_untouched() {
        let (mgr, _sid, _wid, _pid) = single_live_pane_manager();
        let mut document = HandoffDocument {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incarnation: "deadbeef".to_string(),
            listen_fd: 3,
            next_session_id: 1,
            next_pane_id: 1,
            sessions: vec![HandoffSession {
                id: 1,
                name: "s".to_string(),
                window_order: vec![1],
                active_window_id: Some(1),
                next_window_id: 2,
                windows: vec![HandoffWindow {
                    id: 1,
                    name: "w".to_string(),
                    active_pane_id: Some(1),
                    next_pane_id: 2,
                    panes: vec![HandoffPane {
                        id: 999, // no such pane in `mgr`
                        cols: 80,
                        rows: 24,
                        cwd: None,
                        title: None,
                        agent_state: Some(to_wire_state(AgentState::Working)),
                        agent_name: Some("claude".to_string()),
                        agent_revision: 3,
                        exited: false,
                        child_pid: Some(1234),
                        master_fd: Some(42),
                        scrollback: Vec::new(),
                        latch_armed: true,
                        latch_command_ended: true,
                        latch_generation: 7,
                    }],
                }],
            }],
        };
        let recorded = document.sessions[0].windows[0].panes[0].clone();

        refresh_live_agent_state(&mut document, &mgr);

        assert_eq!(
            document.sessions[0].windows[0].panes[0], recorded,
            "task0006: a pane no longer present in mgr must be left untouched, not panic"
        );
    }

    /// [`rewrite_handoff_file`] must replace the SAME handoff file
    /// [`write_handoff_file`] already created -- unlike `write_handoff_file`'s
    /// `create_new`, a second call must not fail just because the file
    /// already exists, and the visible result at `path` must be the new
    /// content (regardless of the temp-file-then-rename mechanics used to
    /// get there).
    #[test]
    fn rewrite_handoff_file_replaces_an_already_written_handoff_file_at_the_same_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let path = handoff_file_path(&socket_path);

        let mut document = minimal_document();
        write_handoff_file(&document, &path).expect("initial write must succeed");

        document.next_session_id = 42;
        rewrite_handoff_file(&document, &socket_path)
            .expect("rewrite must succeed over an already-existing handoff file");

        let bytes = std::fs::read(&path).expect("file must still exist at the same path");
        let decoded = decode_handoff_document(&bytes).expect("must decode as a valid document");
        assert_eq!(
            decoded.next_session_id, 42,
            "rewrite must persist the updated content, not the original write"
        );
    }

    /// The core regression this fix closes (finding `b58e0d47f3c2916a`): if
    /// `rewrite_handoff_file` cannot produce the new content, the
    /// PREVIOUSLY-WRITTEN handoff document at `path` must survive intact and
    /// decodable -- never truncated/torn -- since the caller (`daemon.rs`)
    /// only logs a warning on `Err` and continues toward `exec`, handing
    /// whatever is on disk to the successor process.
    ///
    /// Forces the failure by pre-occupying the same-directory temp path
    /// (`<handoff file>.tmp`) with a directory, so the internal
    /// `create_handoff_file(tmp_path)` call fails before any byte of the new
    /// content is written anywhere near the real path -- exercising the
    /// same "the real path must be untouched by a failed rewrite" property
    /// a mid-write failure (e.g. ENOSPC) would.
    #[test]
    fn rewrite_handoff_file_leaves_the_previous_document_intact_when_the_write_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("mux-default.sock");
        let path = handoff_file_path(&socket_path);

        let document = minimal_document();
        write_handoff_file(&document, &path).expect("initial write must succeed");
        let original_bytes = std::fs::read(&path).expect("initial file must exist");

        let mut tmp_path = path.clone();
        let mut tmp_name = path.file_name().unwrap().to_os_string();
        tmp_name.push(".tmp");
        tmp_path.set_file_name(tmp_name);
        std::fs::create_dir(&tmp_path).expect("occupy the temp path with a directory");

        let mut broken_document = document.clone();
        broken_document.next_session_id = 42;
        let result = rewrite_handoff_file(&broken_document, &socket_path);
        assert!(
            result.is_err(),
            "rewrite must fail when it cannot create its temp file"
        );

        let bytes_after = std::fs::read(&path).expect("original handoff file must still exist");
        assert_eq!(
            bytes_after, original_bytes,
            "a failed rewrite must leave the previously written handoff document byte-for-byte \
             intact, not truncated or partially overwritten"
        );
        let decoded =
            decode_handoff_document(&bytes_after).expect("surviving document must still decode");
        assert_eq!(
            decoded.next_session_id, document.next_session_id,
            "surviving document must be the ORIGINAL content, not the failed rewrite's"
        );
    }
}
