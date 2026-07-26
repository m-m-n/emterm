//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::ipc::connection::handle_connection;
use super::ipc::handlers::{handle_destroy_pane, reevaluate_agent_waiters};
use super::ipc::protocol::{
    AgentStatusUpdateMsg, ClientType, HelloMsg, MAX_FRAME_LENGTH, MessageType, MuxMessage,
    NotifyMsg, PREVIOUS_PROTOCOL_VERSION, PROTOCOL_VERSION, RenameWindowMsg, WelcomeMsg,
    parse_rejected_server_version,
};
use super::session::manager::SessionManager;
use super::session::pane::{
    AgentStatusReportSender, NotificationSender, PaneExitSender, PaneId, SharedPaneExitSender,
    TitleChangeSender,
};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

/// Daemon-level title channel capacity.
const TITLE_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level notification channel capacity (OSC 9 desktop notifications
/// detected on Detached panes).
const NOTIFICATION_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level agent-status report channel capacity (SPEC FR3): raw OSC
/// payload strings forwarded from every pane's reader thread, regardless of
/// attach state.
const AGENT_STATUS_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level pane-exit channel capacity. Reader threads enqueue a bare
/// `PaneId` here on PTY EOF; the reap task drains it. EOF is one-shot per
/// pane so the channel never sustains high throughput.
const PANE_EXIT_CHANNEL_CAPACITY: usize = 64;

#[cfg(unix)]
use tokio::net::UnixListener;

/// Get the socket path for the mux daemon.
///
/// Linux: `$XDG_RUNTIME_DIR/emterm/mux-default.sock`
///   fallback: `~/.local/run/emterm/mux-default.sock`
/// Windows: `%LOCALAPPDATA%\emterm\mux-default.sock`
pub fn socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(runtime_dir)
                .join("emterm")
                .join("mux-default.sock")
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
                .join(".local")
                .join("run")
                .join("emterm")
                .join("mux-default.sock")
        } else {
            PathBuf::from("/tmp")
                .join("emterm")
                .join("mux-default.sock")
        }
    }
    #[cfg(windows)]
    {
        let local_app_data =
            std::env::var("LOCALAPPDATA").unwrap_or_else(|_| "C:\\Users\\Default".to_string());
        PathBuf::from(local_app_data)
            .join("emterm")
            .join("mux-default.sock")
    }
}

/// Get the Named Pipe name for the mux daemon (Windows).
///
/// Includes the current username to isolate pipes per user,
/// preventing cross-user access on shared machines.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    format!(r"\\.\pipe\emterm-mux-{}", user)
}

/// Open a mux log file for appending, refusing symlinks on Unix.
///
/// All three mux log files (`mux-daemon.log`, `mux-bridge.log`, `mux-client.log`)
/// live in the same user-writable runtime directory. A pre-placed symlink on
/// that path would otherwise redirect appended log lines into an arbitrary
/// user-writable file. `O_NOFOLLOW` + `mode(0o600)` closes that at open time
/// on Unix. Windows falls back to the default open, which has a residual
/// same-user reparse-point TOCTOU; the directory is per-user (`%LOCALAPPDATA%`)
/// so the threat surface is limited to a locally compromised user account.
pub fn open_mux_log_append(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
        opts.mode(0o600);
    }
    opts.open(path)
}

/// Check if a daemon is already running by attempting to connect to the socket.
#[cfg(unix)]
pub fn is_daemon_running(path: &std::path::Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Check if a daemon is already running by attempting to open the Named Pipe.
#[cfg(windows)]
pub fn is_daemon_running(_path: &std::path::Path) -> bool {
    use std::fs::OpenOptions;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name())
        .is_ok()
}

/// Remove stale socket file if daemon is not running.
pub fn cleanup_stale_socket(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() && !is_daemon_running(path) {
        log::info!("Removing stale socket: {:?}", path);
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Ensure the mux daemon is running, spawning it if necessary.
///
/// If the socket file does not exist, spawns the daemon as a background
/// process and waits for it to become ready with exponential backoff.
/// Returns the socket path on success.
pub fn ensure_daemon_running() -> Result<PathBuf, String> {
    let sock_path = socket_path();

    // Clean up stale socket (daemon died but socket file remains)
    cleanup_stale_socket(&sock_path)
        .map_err(|e| format!("Failed to clean up stale socket: {}", e))?;

    let mut daemon_running = if cfg!(unix) {
        sock_path.exists()
    } else {
        is_daemon_running(&sock_path)
    };

    if daemon_running {
        // Strategy B (task0010 rework): a presence check alone cannot tell
        // an old-protocol daemon from a compatible one — every mux client
        // would fail against a long-lived v1 daemon after an eMterm
        // upgrade. Probe the real protocol version and, on the adjacent
        // older version, shut the legacy daemon down automatically so a
        // compatible one can start in its place.
        match recover_from_legacy_daemon(&sock_path)? {
            LegacyRecovery::Compatible => {}
            LegacyRecovery::Recovered => daemon_running = false,
        }
    }

    if !daemon_running {
        spawn_daemon(&sock_path)?;
    }

    Ok(sock_path)
}

/// Create the socket's parent directory with restricted permissions, spawn
/// the daemon as a detached background process, and wait for it to become
/// ready with exponential backoff.
///
/// Extracted out of [`ensure_daemon_running`] (task0001) so the `emterm mux
/// attach` path can respawn a daemon after a legacy-daemon recovery
/// shutdown, without duplicating the spawn/readiness logic.
///
/// Precondition: no compatible daemon currently owns `sock_path` (the
/// caller is responsible for stale-socket cleanup, the presence check, and
/// the recovery probe, as [`ensure_daemon_running`] does). Postcondition: a
/// daemon answers on `sock_path`, or this returns an error string identical
/// to the pre-extraction failure messages ("Failed to spawn daemon: …",
/// "Failed to start mux daemon").
pub(in crate::mux) fn spawn_daemon(sock_path: &Path) -> Result<(), String> {
    // Ensure parent directory exists with restricted permissions
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create socket directory: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let exe = crate::self_exec::self_exe_path()
        .map_err(|e| format!("Failed to get executable path: {}", e))?;

    let log_path = sock_path.with_file_name("mux-daemon.log");
    let log_file = open_mux_log_append(&log_path).or_else(|_| {
        let fallback = if cfg!(windows) {
            std::env::temp_dir().join("emterm-mux-daemon.log")
        } else {
            std::path::PathBuf::from("/tmp/emterm-mux-daemon.log")
        };
        open_mux_log_append(&fallback)
    });

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["mux", "--daemon"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    // Fall back to discarding daemon stderr if no log file could be opened
    // (symlink refusal, permission denied, disk full). Prior code panicked
    // via `.unwrap()` on the fallback, which aborted daemon startup.
    match log_file {
        Ok(f) => {
            cmd.stderr(std::process::Stdio::from(f));
        }
        Err(e) => {
            eprintln!(
                "Daemon log unavailable ({}): {} (daemon stderr discarded)",
                log_path.display(),
                e
            );
            cmd.stderr(std::process::Stdio::null());
        }
    }

    // Detach daemon into its own session so it survives parent terminal exit
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe per POSIX
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const DETACHED_PROCESS: u32 = 0x00000008;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    cmd.spawn().map_err(|e| {
        crate::self_exec::note_spawn_failure();
        format!("Failed to spawn daemon: {}", e)
    })?;

    // Wait for daemon to start with exponential backoff
    let mut started = false;
    for i in 0..50 {
        if is_daemon_running(sock_path) {
            started = true;
            break;
        }
        let delay = std::cmp::min(10 * (1 << i.min(4)), 100);
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    if !started {
        return Err("Failed to start mux daemon".to_string());
    }

    Ok(())
}

// ============================================================================
// task0010 rework: safe PROTOCOL_VERSION upgrade path (strategy B)
//
// A version bump alone left a running v1 daemon stranded after an eMterm
// upgrade: `ensure_daemon_running` only checked socket presence, and even
// `mux kill` couldn't recover it (the old server rejects a v2 Hello before
// ever reading Shutdown). The helpers below open a short handshake to
// probe the real protocol version and, on the adjacent older version
// (`PREVIOUS_PROTOCOL_VERSION`), send a version-tolerant Shutdown so the
// legacy daemon exits and a compatible one can take its place.
// ============================================================================

/// Outcome of [`recover_from_legacy_daemon`]'s handshake probe.
///
/// `pub(in crate::mux)` (task0001): the `emterm mux attach` path
/// (`mux::cli::execute_attach`) needs to branch on this outcome the same
/// way [`ensure_daemon_running`] does, without exposing it outside the mux
/// module.
#[derive(Debug)]
pub(in crate::mux) enum LegacyRecovery {
    /// The running daemon already accepted a [`PROTOCOL_VERSION`] Hello —
    /// nothing to recover.
    Compatible,
    /// A daemon speaking [`PREVIOUS_PROTOCOL_VERSION`] was found and asked
    /// to exit; the caller should now spawn a fresh daemon.
    Recovered,
}

/// Connect to the daemon's control channel with read/write timeouts, ready
/// for a [`handshake_with_version`] call. Unix: the `AF_UNIX` socket at
/// `sock_path`. Windows: the daemon's Named Pipe (`sock_path` is unused
/// there, matching [`is_daemon_running`]'s existing `_path` convention).
#[cfg(unix)]
fn connect_daemon(sock_path: &Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(sock_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

#[cfg(windows)]
fn connect_daemon(_sock_path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name())
}

/// Send a `Hello` advertising `protocol_version` and read back the decoded
/// `Welcome`. Generic over the platform stream type returned by
/// [`connect_daemon`] (`UnixStream` / Windows `File`), both `Read + Write`.
/// Never panics on a malformed reply — any framing/decode problem surfaces
/// as an `io::Error` so callers can produce a short user-facing message
/// instead of an opaque bincode error (AC-3).
fn handshake_with_version<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    protocol_version: u32,
) -> std::io::Result<WelcomeMsg> {
    let hello = HelloMsg {
        client_type: ClientType::Cli,
        protocol_version,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "daemon frame exceeds the maximum frame length",
        ));
    }
    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;
    let welcome_msg = MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid daemon frame")
    })?;
    welcome_msg.decode_payload::<WelcomeMsg>().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid Welcome payload from daemon",
        )
    })
}

/// Send a bare `Shutdown` control message. `Shutdown`'s wire shape (message
/// type only, empty payload) has never changed, which is what lets a v2
/// client ask an adjacent-version daemon to exit once the Hello handshake
/// has admitted the connection.
fn send_shutdown<S: std::io::Write>(stream: &mut S) -> std::io::Result<()> {
    let msg = MuxMessage {
        msg_type: MessageType::Shutdown,
        pane_id: 0,
        payload: Vec::new(),
    };
    let body = msg.to_frame_body();
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()
}

/// Poll until the daemon at `sock_path` is no longer reachable (bounded to
/// ~5s), then remove any leftover socket/marker file. Used after sending a
/// `Shutdown` to a legacy daemon so the caller can safely spawn a
/// replacement without racing the exiting process for the socket.
fn wait_for_daemon_exit(sock_path: &Path) -> Result<(), String> {
    for _ in 0..50 {
        if !is_daemon_running(sock_path) {
            let _ = std::fs::remove_file(sock_path);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(
        "The legacy mux daemon did not exit after a shutdown request within 5 \
         seconds. Stop it manually (e.g. `pkill -f 'emterm mux --daemon'`) and \
         retry."
            .to_string(),
    )
}

/// Probe the daemon already occupying `sock_path` and recover automatically
/// when it is running the adjacent older protocol version (AC-1, task0010
/// rework — see IMPLEMENTATION.md "Old GUI × new daemon pairing").
///
/// Performs a real handshake first; only on a version mismatch does it
/// retry with [`PREVIOUS_PROTOCOL_VERSION`] (which the legacy daemon
/// accepts) and send a `Shutdown` there.
///
/// Returns `Ok(LegacyRecovery::Compatible)` when the running daemon already
/// speaks [`PROTOCOL_VERSION`] (nothing to do), `Ok(LegacyRecovery::Recovered)`
/// once a legacy daemon has been asked to exit and has released the socket,
/// or `Err` with a short, human-readable message (never a bincode/decode
/// error, per AC-3) when recovery could not complete.
///
/// `pub(in crate::mux)` (task0001): widened so `mux::cli::execute_attach`
/// can run the same probe before deciding whether to respawn.
pub(in crate::mux) fn recover_from_legacy_daemon(
    sock_path: &Path,
) -> Result<LegacyRecovery, String> {
    let mut probe = connect_daemon(sock_path)
        .map_err(|e| format!("Could not connect to the existing mux daemon: {e}"))?;
    match handshake_with_version(&mut probe, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => Ok(LegacyRecovery::Compatible),
        Ok(WelcomeMsg::Rejected { reason }) => {
            drop(probe); // the daemon already closed its side after rejecting
            let reported = parse_rejected_server_version(&reason)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            log::warn!(
                "mux daemon at {:?} reports protocol version {} (this build is {}); \
                 attempting automatic recovery",
                sock_path,
                reported,
                PROTOCOL_VERSION
            );

            let mut legacy = connect_daemon(sock_path).map_err(|e| {
                format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but it became unreachable while recovering: {e}"
                )
            })?;
            match handshake_with_version(&mut legacy, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    send_shutdown(&mut legacy).map_err(|e| {
                        format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but failed to send its shutdown request: {e}"
                        )
                    })?;
                    drop(legacy);
                    wait_for_daemon_exit(sock_path)?;
                    log::info!(
                        "Recovered mux socket from a protocol version {} daemon; a \
                         compatible daemon can now start",
                        reported
                    );
                    Ok(LegacyRecovery::Recovered)
                }
                Ok(WelcomeMsg::Rejected {
                    reason: retry_reason,
                }) => Err(format!(
                    "The running mux daemon (protocol version {reported}) could not \
                     be recovered automatically: {retry_reason}. Stop it manually \
                     (e.g. `pkill -f 'emterm mux --daemon'`) and retry."
                )),
                Err(e) => Err(format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but failed to negotiate a compatible shutdown: {e}"
                )),
            }
        }
        Err(e) => Err(format!(
            "Failed to communicate with the existing mux daemon: {e}"
        )),
    }
}

/// Result of [`shutdown_daemon_any_version`], used by `emterm mux kill`
/// (AC-2/AC-3, task0010 rework).
#[derive(Debug)]
pub enum ShutdownOutcome {
    /// A Shutdown request was accepted by the daemon. Carries a short
    /// user-facing status line (e.g. noting when a legacy protocol version
    /// was detected and handled automatically).
    ShutDown(String),
    /// The daemon was unreachable outright (process already gone); the
    /// stale socket/marker file was removed. Mirrors the pre-task0010
    /// `execute_kill` fallback behavior.
    StaleSocketRemoved(String),
}

/// Shut down whatever daemon is occupying `sock_path`, regardless of
/// protocol version (AC-2). Tries [`PROTOCOL_VERSION`] first; on a version
/// mismatch it retries with [`PREVIOUS_PROTOCOL_VERSION`] so an adjacent
/// legacy daemon accepts the connection and can be asked to exit. Every
/// failure path returns a short explanatory message — never an opaque
/// bincode/decode error (AC-3).
pub fn shutdown_daemon_any_version(sock_path: &Path) -> Result<ShutdownOutcome, String> {
    let mut stream = match connect_daemon(sock_path) {
        Ok(s) => s,
        Err(_) => {
            let _ = std::fs::remove_file(sock_path);
            return Ok(ShutdownOutcome::StaleSocketRemoved(
                "Mux daemon not reachable (stale socket removed)".to_string(),
            ));
        }
    };

    match handshake_with_version(&mut stream, PROTOCOL_VERSION) {
        Ok(WelcomeMsg::Accepted { .. }) => {
            send_shutdown(&mut stream)
                .map_err(|e| format!("Failed to send shutdown request: {e}"))?;
            Ok(ShutdownOutcome::ShutDown(
                "Mux daemon shutting down".to_string(),
            ))
        }
        Ok(WelcomeMsg::Rejected { reason }) => {
            drop(stream);
            let reported = parse_rejected_server_version(&reason)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let mut legacy = connect_daemon(sock_path).map_err(|e| {
                format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but it became unreachable while retrying: {e}"
                )
            })?;
            match handshake_with_version(&mut legacy, PREVIOUS_PROTOCOL_VERSION) {
                Ok(WelcomeMsg::Accepted { .. }) => {
                    send_shutdown(&mut legacy).map_err(|e| {
                        format!(
                            "Detected an incompatible mux daemon (protocol version \
                             {reported}) but failed to send its shutdown request: {e}"
                        )
                    })?;
                    Ok(ShutdownOutcome::ShutDown(format!(
                        "Detected a mux daemon on an older protocol version ({reported}); \
                         sent a compatible shutdown request. Run `emterm mux` to start \
                         the current version."
                    )))
                }
                Ok(WelcomeMsg::Rejected {
                    reason: retry_reason,
                }) => Err(format!(
                    "The running mux daemon (protocol version {reported}) could not be \
                     shut down automatically: {retry_reason}. Stop it manually (e.g. \
                     `pkill -f 'emterm mux --daemon'`) and retry."
                )),
                Err(e) => Err(format!(
                    "Detected an incompatible mux daemon (protocol version {reported}) \
                     but failed to negotiate a compatible shutdown: {e}"
                )),
            }
        }
        Err(e) => Err(format!("Failed to communicate with the mux daemon: {e}")),
    }
}

/// Run the mux daemon.
///
/// This is the main entry point for `emterm mux --daemon`.
/// It blocks until all sessions end or SIGTERM is received.
#[cfg(unix)]
pub async fn run_daemon() -> anyhow::Result<()> {
    let sock_path = socket_path();

    // Ensure parent directory exists with restricted permissions
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    // Clean up stale socket
    cleanup_stale_socket(&sock_path)?;

    // Bind listener and restrict socket permissions to owner only
    let listener = UnixListener::bind(&sock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o700))?;
    }
    log::info!("Mux daemon listening on {:?}", sock_path);

    let session_manager = Arc::new(Mutex::new(SessionManager::new()));

    // Daemon-level title channel: lives as long as the daemon so every pane
    // (GUI-created or CLI-created) can propagate OSC title changes to the
    // session manager even when no GUI client is attached.
    let (title_tx, title_rx): (TitleChangeSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(TITLE_CHANNEL_CAPACITY);
    tokio::spawn(run_title_update_task(session_manager.clone(), title_rx));

    // Daemon-level notification channel: pane reader threads forward OSC 9
    // desktop notifications detected on Detached output here; the task
    // broadcasts them to connected GUI clients via notify_tx (FR2, NFR3).
    let (notification_tx, notification_rx): (NotificationSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(NOTIFICATION_CHANNEL_CAPACITY);
    tokio::spawn(run_notification_task(
        session_manager.clone(),
        notification_rx,
    ));

    // Daemon-level agent-status channel (SPEC FR3): pane reader threads
    // forward raw agent-status OSC payload strings here REGARDLESS of
    // attach state (unlike notifications, which only scan while Detached) —
    // the daemon owns per-pane agent-status state unconditionally.
    let (agent_status_tx, agent_status_rx): (
        AgentStatusReportSender,
        mpsc::Receiver<(u32, String)>,
    ) = mpsc::channel(AGENT_STATUS_CHANNEL_CAPACITY);
    tokio::spawn(run_agent_status_task(
        session_manager.clone(),
        agent_status_rx,
    ));

    // Shutdown signal: sent by handle_destroy_pane/handle_destroy_window when all sessions empty
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Daemon-level pane-exit channel: pane reader threads enqueue their pane_id
    // here on PTY EOF (regardless of attach state); the reap task reaps each via
    // handle_destroy_pane, making "PTY death -> reap" the single authority (FR1,
    // FR2, FR7). The SharedPaneExitSender is fixed at pane creation and never
    // swapped on detach, so a detached pane can still notify on EOF (M1).
    let (pane_exit_tx, pane_exit_rx): (PaneExitSender, mpsc::Receiver<PaneId>) =
        mpsc::channel(PANE_EXIT_CHANNEL_CAPACITY);
    tokio::spawn(run_pane_exit_task(
        session_manager.clone(),
        shutdown_tx.clone(),
        pane_exit_rx,
    ));
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(Some(pane_exit_tx)));

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    #[cfg(unix)]
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    loop {
        #[cfg(unix)]
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        tokio::spawn(handle_connection(stream, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), agent_status_tx.clone(), pane_exit_sender.clone()));
                    }
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                    }
                }
            }
            _ = sigterm.recv() => {
                log::info!("SIGTERM received, shutting down");
                break;
            }
            _ = sigint.recv() => {
                log::info!("SIGINT received, shutting down");
                break;
            }
            _ = sighup.recv() => {
                log::info!("SIGHUP received, ignoring (daemon continues)");
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("All sessions empty, auto-shutting down");
                    break;
                }
            }
        }
    }

    // Graceful shutdown: close all PTYs so shell processes terminate
    graceful_shutdown(&session_manager).await;

    // Cleanup socket file
    let _ = std::fs::remove_file(&sock_path);
    log::info!("Daemon shutdown complete");

    Ok(())
}

/// Run the mux daemon on Windows using Named Pipes.
///
/// Listens on `\\.\pipe\emterm-mux-default`, accepts client connections,
/// and manages PTY sessions. Auto-exits when all sessions end or Ctrl+C.
#[cfg(windows)]
pub async fn run_daemon() -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name_str = pipe_name();

    // Write marker file so socket_path().exists() works for other checks
    let sock_path = socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&sock_path, pipe_name_str.as_bytes())?;

    log::info!("Mux daemon listening on {}", pipe_name_str);

    let session_manager = Arc::new(Mutex::new(SessionManager::new()));

    // Daemon-level title channel: lives as long as the daemon so every pane
    // (GUI-created or CLI-created) can propagate OSC title changes to the
    // session manager even when no GUI client is attached.
    let (title_tx, title_rx): (TitleChangeSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(TITLE_CHANNEL_CAPACITY);
    tokio::spawn(run_title_update_task(session_manager.clone(), title_rx));

    // Daemon-level notification channel: pane reader threads forward OSC 9
    // desktop notifications detected on Detached output here; the task
    // broadcasts them to connected GUI clients via notify_tx (FR2, NFR3).
    let (notification_tx, notification_rx): (NotificationSender, mpsc::Receiver<(u32, String)>) =
        mpsc::channel(NOTIFICATION_CHANNEL_CAPACITY);
    tokio::spawn(run_notification_task(
        session_manager.clone(),
        notification_rx,
    ));

    // Daemon-level agent-status channel (SPEC FR3, same wiring as the Unix
    // run loop): pane reader threads forward raw agent-status OSC payload
    // strings here regardless of attach state.
    let (agent_status_tx, agent_status_rx): (
        AgentStatusReportSender,
        mpsc::Receiver<(u32, String)>,
    ) = mpsc::channel(AGENT_STATUS_CHANNEL_CAPACITY);
    tokio::spawn(run_agent_status_task(
        session_manager.clone(),
        agent_status_rx,
    ));

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Daemon-level pane-exit channel (same wiring as the Unix run loop, FR7):
    // reader threads enqueue their pane_id on PTY EOF; the reap task reaps each
    // via handle_destroy_pane regardless of attach state (FR1, FR2). The
    // SharedPaneExitSender is fixed at pane creation and never swapped (M1).
    let (pane_exit_tx, pane_exit_rx): (PaneExitSender, mpsc::Receiver<PaneId>) =
        mpsc::channel(PANE_EXIT_CHANNEL_CAPACITY);
    tokio::spawn(run_pane_exit_task(
        session_manager.clone(),
        shutdown_tx.clone(),
        pane_exit_rx,
    ));
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(Some(pane_exit_tx)));

    // First iteration claims exclusive pipe ownership to prevent hijacking
    let mut is_first_instance = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(is_first_instance)
            .reject_remote_clients(true)
            .create(&pipe_name_str)?;
        is_first_instance = false;

        tokio::select! {
            result = server.connect() => {
                match result {
                    Ok(()) => {
                        tokio::spawn(handle_connection(server, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), agent_status_tx.clone(), pane_exit_sender.clone()));
                    }
                    Err(e) => {
                        log::error!("Pipe accept error: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("Ctrl+C received, shutting down");
                break;
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    log::info!("All sessions empty, auto-shutting down");
                    break;
                }
            }
        }
    }

    graceful_shutdown(&session_manager).await;

    // Cleanup marker file
    let _ = std::fs::remove_file(&sock_path);
    log::info!("Daemon shutdown complete");

    Ok(())
}

/// Run the mux daemon (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub async fn run_daemon() -> anyhow::Result<()> {
    anyhow::bail!("Mux daemon is not supported on this platform.");
}

/// Apply a title change to the SessionManager with diff detection.
///
/// Returns `true` when `window.name` was updated and a broadcast was sent;
/// `false` when the pane was not found or the title was unchanged.
async fn apply_title_change(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    new_title: String,
) -> bool {
    let mut mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        log::warn!("apply_title_change: pane {} not found", pane_id);
        return false;
    };
    let unchanged = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .map(|w| w.name == new_title)
        .unwrap_or(false);
    if unchanged {
        return false;
    }
    log::info!(
        "Title change: pane {} -> window {} -> '{}'",
        pane_id,
        wid,
        new_title
    );
    mgr.rename_window(sid, wid, new_title.clone());
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    let rename_payload = RenameWindowMsg { name: new_title };
    let msg = MuxMessage::control(MessageType::RenameWindow, wid, &rename_payload);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!("apply_title_change: no active subscribers: {}", e);
    }
    true
}

/// Run the daemon-level title update task.
///
/// Exits when all senders are dropped (daemon shutdown).
async fn run_title_update_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut title_rx: mpsc::Receiver<(u32, String)>,
) {
    log::info!("Title update task started");
    while let Some((pane_id, new_title)) = title_rx.recv().await {
        apply_title_change(&session_manager, pane_id, new_title).await;
    }
    log::info!("Title update task exiting");
}

/// Broadcast a Detached-pane OSC 9 notification to connected GUI clients.
///
/// The notification is sent via the SessionManager `notify_tx` broadcast; the
/// per-connection select! loop forwards it to its GUI client. If no GUI client
/// is currently subscribed the broadcast simply has no receivers (the
/// notification is fire-and-forget; FR5 keeps it out of any replay buffer, so
/// nothing replays it later). The GUI fires the OS notification (NFR3).
async fn relay_notification(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    message: String,
) {
    let notify_tx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().clone()
    };
    let payload = NotifyMsg { message };
    let msg = MuxMessage::control(MessageType::Notify, pane_id, &payload);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!("relay_notification: no active subscribers: {}", e);
    }
}

/// Run the daemon-level notification relay task.
///
/// Consumes `(pane_id, message)` from Detached pane reader threads and
/// broadcasts each as a `Notify` control message to GUI clients. Exits when
/// all senders are dropped (daemon shutdown).
async fn run_notification_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut notification_rx: mpsc::Receiver<(u32, String)>,
) {
    log::info!("Notification relay task started");
    while let Some((pane_id, message)) = notification_rx.recv().await {
        relay_notification(&session_manager, pane_id, message).await;
    }
    log::info!("Notification relay task exiting");
}

/// Map the core (build-agnostic) `AgentState` to the `mux_ipc` wire mirror
/// enum. Two distinct types by design: `mux_ipc` must not depend on the
/// binary crate (task0002 IMPLEMENTATION.md), so this conversion (and its
/// inverse, [`from_wire_state`]) is the only place the two ever meet.
/// Widened beyond this module (`pub(in crate::mux)`) so the agent API
/// handlers (task0004, `mux::ipc::handlers`) share the same conversion
/// rather than re-deriving it.
pub(in crate::mux) fn to_wire_state(
    state: crate::agent_status::AgentState,
) -> crate::mux::ipc::protocol::AgentState {
    use crate::agent_status::AgentState as Core;
    use crate::mux::ipc::protocol::AgentState as Wire;
    match state {
        Core::Idle => Wire::Idle,
        Core::Working => Wire::Working,
        Core::Blocked => Wire::Blocked,
        Core::Done => Wire::Done,
    }
}

/// Inverse of [`to_wire_state`]: map the `mux_ipc` wire `AgentState` to the
/// core (build-agnostic) enum. Used by the agent API's `WaitAgentState`
/// handler to match a request's wire `states` set against pane state held
/// in the core type.
pub(in crate::mux) fn from_wire_state(
    state: crate::mux::ipc::protocol::AgentState,
) -> crate::agent_status::AgentState {
    use crate::agent_status::AgentState as Core;
    use crate::mux::ipc::protocol::AgentState as Wire;
    match state {
        Wire::Idle => Core::Idle,
        Wire::Working => Core::Working,
        Wire::Blocked => Core::Blocked,
        Wire::Done => Core::Done,
    }
}

/// Apply one raw agent-status OSC report to its pane and broadcast the
/// result (SPEC FR3 / FR5, task0003 AC-1/AC-2/AC-4).
///
/// Validates `raw_payload` via [`crate::agent_status::parse`]; a rejected
/// (`None`) parse leaves ALL state untouched and broadcasts nothing (AC-2).
/// An accepted event is applied to the pane (revision increments) and
/// exactly one `AgentStatusUpdate` (`replay_derived: false`) is broadcast
/// with the pane's current public ID (AC-4).
async fn apply_agent_status_report(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: u32,
    raw_payload: String,
) {
    let Some(event) = crate::agent_status::parse(&raw_payload) else {
        // Rejected sequence: no state change, no broadcast (AC-2).
        return;
    };

    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        log::warn!("apply_agent_status_report: pane {} not found", pane_id);
        return;
    };
    let Some(pane) = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id))
    else {
        return;
    };

    let revision = pane.apply_agent_status_event(event);
    // task0004 "Wait implementation": every accepted report (set, clear,
    // same-state re-report) re-evaluates this pane's registered
    // `WaitAgentState` waiters (level-triggered, no polling).
    reevaluate_agent_waiters(pane);
    let (state, name) = {
        let status = pane.agent_status.lock().unwrap();
        (status.state, status.name.clone())
    };
    let public_pane_id = mgr.public_pane_id(pane_id);
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);

    let payload = AgentStatusUpdateMsg {
        pane_id,
        public_pane_id,
        state: state.map(to_wire_state),
        name,
        revision,
        replay_derived: false,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &payload);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!("apply_agent_status_report: no active subscribers: {}", e);
    }
}

/// Run the daemon-level agent-status task.
///
/// Consumes `(pane_id, raw_payload)` from every pane's reader thread
/// (regardless of attach state, SPEC FR3) and applies + broadcasts each via
/// [`apply_agent_status_report`]. Exits when all senders are dropped
/// (daemon shutdown).
async fn run_agent_status_task(
    session_manager: Arc<Mutex<SessionManager>>,
    mut agent_status_rx: mpsc::Receiver<(u32, String)>,
) {
    log::info!("Agent-status task started");
    while let Some((pane_id, raw_payload)) = agent_status_rx.recv().await {
        apply_agent_status_report(&session_manager, pane_id, raw_payload).await;
    }
    log::info!("Agent-status task exiting");
}

/// Broadcast one `AgentStatusUpdate` (`replay_derived: true`) per pane in
/// `session_id` whose GUI-visible state may need replacement after a
/// snapshot (SPEC FR4/FR5, task0003 AC-5, task0013 AC-1/AC-2/AC-3). Called
/// after a client receives a snapshot (attach / window switch) so state —
/// stripped from the replayed bytes — is resynced out-of-band.
///
/// Emits for every pane with `revision > 0`, i.e. every pane that has ever
/// had an accepted report, REGARDLESS of whether its current `state` is
/// `Some` or `None`: revision starts at 0 and only increments on an
/// accepted report (set, clear, or same-state re-report — see
/// `AgentStatus`), so `revision > 0` is exactly "this pane's GUI-visible
/// state may be stale" without an extra flag. This covers a pane that was
/// cleared while the GUI was detached (state is `None` here, but the GUI
/// may still show a stale badge) — the message carries `state: None` in
/// that case so the GUI clears it. Panes that have never reported
/// (`revision == 0`) produce no message, since the GUI has no stale state
/// to clear for them.
pub(in crate::mux) async fn sync_agent_status_after_snapshot(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
) {
    let mgr = session_manager.lock().await;
    let Some(session) = mgr.get_session(session_id) else {
        return;
    };
    let mut updates = Vec::new();
    for (_wid, pane) in session.panes_iter() {
        let status = pane.agent_status.lock().unwrap();
        if status.revision == 0 {
            continue;
        }
        updates.push(AgentStatusUpdateMsg {
            pane_id: pane.id,
            public_pane_id: mgr.public_pane_id(pane.id),
            state: status.state.map(to_wire_state),
            name: status.name.clone(),
            revision: status.revision,
            replay_derived: true,
        });
    }
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    for update in updates {
        let pane_id = update.pane_id;
        let msg = MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &update);
        if let Err(e) = notify_tx.send(msg) {
            log::debug!(
                "sync_agent_status_after_snapshot: no active subscribers: {}",
                e
            );
        }
    }
}

/// Single-pane counterpart of [`sync_agent_status_after_snapshot`] (SPEC
/// FR4/FR5, task0003 AC-5, task0013 AC-1/AC-2/AC-3): broadcasts one
/// `AgentStatusUpdate` (`replay_derived: true`) for `pane_id` if it has
/// `revision > 0` (ever had an accepted report — see the doc comment on
/// `sync_agent_status_after_snapshot` for why this covers cleared state
/// too). Used after an on-demand per-pane snapshot (`RequestPaneSnapshot`,
/// the same-session window-switch path) rather than a full session attach.
pub(in crate::mux) async fn sync_agent_status_after_pane_snapshot(
    session_manager: &Arc<Mutex<SessionManager>>,
    pane_id: PaneId,
) {
    let mgr = session_manager.lock().await;
    let Some((sid, wid)) = mgr.find_pane(pane_id) else {
        return;
    };
    let Some(pane) = mgr
        .get_session(sid)
        .and_then(|s| s.windows.get(&wid))
        .and_then(|w| w.panes.get(&pane_id))
    else {
        return;
    };
    let status = pane.agent_status.lock().unwrap();
    if status.revision == 0 {
        return;
    }
    let update = AgentStatusUpdateMsg {
        pane_id,
        public_pane_id: mgr.public_pane_id(pane_id),
        state: status.state.map(to_wire_state),
        name: status.name.clone(),
        revision: status.revision,
        replay_derived: true,
    };
    drop(status);
    let notify_tx = mgr.notify_tx().clone();
    drop(mgr);
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, pane_id, &update);
    if let Err(e) = notify_tx.send(msg) {
        log::debug!(
            "sync_agent_status_after_pane_snapshot: no active subscribers: {}",
            e
        );
    }
}

/// Run the daemon-level pane-exit reap task.
///
/// Consumes a bare `PaneId` from each per-pane reader thread that observed
/// PTY EOF (sent regardless of attach state, FR1) and reaps the pane via
/// `handle_destroy_pane`, making "PTY death -> reap" the single authority
/// independent of attach state (FR2). Because reap is keyed on `pane_id` and
/// ignores the pane's `output_target`, this covers the detached path and the
/// connection-reset race (FR6) uniformly, and is a safe no-op when the pane
/// was already reaped via the Connected empty-chunk path (FR4). When the
/// reaped pane is the last one, `handle_destroy_pane` fires
/// `shutdown_tx.send(true)` (FR5). Exits when all senders are dropped (daemon
/// shutdown).
async fn run_pane_exit_task(
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    mut pane_exit_rx: mpsc::Receiver<PaneId>,
) {
    log::info!("Pane-exit reap task started");
    while let Some(pane_id) = pane_exit_rx.recv().await {
        handle_destroy_pane(pane_id, &session_manager, &shutdown_tx).await;
    }
    log::info!("Pane-exit reap task exiting");
}

/// Close all PTYs in all sessions for graceful daemon shutdown.
async fn graceful_shutdown(session_manager: &Arc<Mutex<SessionManager>>) {
    let mut mgr = session_manager.lock().await;
    let mut pane_count = 0u32;
    let session_ids: Vec<u32> = mgr.sessions_iter().map(|s| s.id).collect();
    for session_id in session_ids {
        if let Some(session) = mgr.get_session_mut(session_id) {
            for window in session.windows.values_mut() {
                for pane in window.panes.values_mut() {
                    if !pane.exited {
                        pane.mark_exited();
                        pane_count += 1;
                    }
                }
            }
        }
    }
    log::info!("Graceful shutdown: closed {} PTY(s)", pane_count);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use std::path::PathBuf;

    #[test]
    fn test_socket_path_not_empty() {
        let path = socket_path();
        assert!(!path.as_os_str().is_empty());
        assert!(path.to_str().unwrap().contains("emterm"));
        assert!(path.to_str().unwrap().contains("mux-default.sock"));
    }

    #[test]
    fn test_socket_path_contains_directory() {
        let path = socket_path();
        assert!(path.parent().is_some());
    }

    #[test]
    fn test_cleanup_stale_nonexistent() {
        let path = PathBuf::from("/tmp/emterm-test-nonexistent.sock");
        assert!(cleanup_stale_socket(&path).is_ok());
    }

    // ---- task0010 rework: legacy-daemon recovery (strategy B) ----
    //
    // A fake v1 daemon is a bare `UnixListener` thread rather than a real
    // spawned process, per the task's Test Notes ("Simulate a v1 server ...
    // by manually crafting the handshake bytes"). It speaks the exact wire
    // shapes (`HelloMsg`/`WelcomeMsg`/`Shutdown`) the real daemon does, with
    // a hardcoded `server_version` and no session/PTY machinery.

    // task0005 rework: derived from `PREVIOUS_PROTOCOL_VERSION` rather than
    // hardcoded to `1`. `recover_from_legacy_daemon`'s retry handshake uses
    // `PREVIOUS_PROTOCOL_VERSION` (exactly one version behind whatever
    // `PROTOCOL_VERSION` currently is) — a fixed literal here silently
    // stopped matching that retry the moment `PROTOCOL_VERSION` moved past
    // 2, at which point the fake daemon's `else` branch below rejects the
    // retry, `recover_from_legacy_daemon` gives up (returns `Err`), and the
    // fake daemon's `accept()` loop is left waiting forever for a THIRD
    // connection that will never arrive — a `server.join()` in a test below
    // then hangs indefinitely. Tying this constant to
    // `PREVIOUS_PROTOCOL_VERSION` keeps the fixture "one version behind
    // current" through any future bump.
    #[cfg(unix)]
    const FAKE_LEGACY_VERSION: u32 = PREVIOUS_PROTOCOL_VERSION;

    #[cfg(unix)]
    fn read_frame<S: std::io::Read>(stream: &mut S) -> MuxMessage {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).expect("read frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        stream.read_exact(&mut frame_buf).expect("read frame body");
        MuxMessage::from_frame_body(&frame_buf).expect("valid frame")
    }

    #[cfg(unix)]
    fn write_welcome<S: std::io::Write>(stream: &mut S, welcome: &WelcomeMsg) {
        let msg = MuxMessage::control(MessageType::Welcome, 0, welcome);
        let body = msg.to_frame_body();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .expect("write frame length");
        stream.write_all(&body).expect("write frame body");
        stream.flush().expect("flush");
    }

    /// Spawn a thread that behaves like a single-instance legacy (v1) mux
    /// daemon on `sock_path`: rejects any Hello whose `protocol_version`
    /// isn't [`FAKE_LEGACY_VERSION`] with the exact reason text the real
    /// daemon produces, accepts a matching Hello, waits for `Shutdown`, then
    /// removes the socket file and exits — mirroring the real daemon's
    /// Shutdown -> `graceful_shutdown` -> `remove_file` sequence.
    #[cfg(unix)]
    fn spawn_fake_legacy_daemon(sock_path: std::path::PathBuf) -> std::thread::JoinHandle<()> {
        use std::os::unix::net::UnixListener;

        let listener = UnixListener::bind(&sock_path).expect("bind fake daemon socket");
        std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let hello_frame = read_frame(&mut stream);
                assert_eq!(hello_frame.msg_type, MessageType::Hello);
                let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");

                if hello.protocol_version != FAKE_LEGACY_VERSION {
                    let reject = WelcomeMsg::Rejected {
                        reason: format!(
                            "Protocol version mismatch: client={}, server={}",
                            hello.protocol_version, FAKE_LEGACY_VERSION
                        ),
                    };
                    write_welcome(&mut stream, &reject);
                    // Connection closes here (stream dropped) — the real
                    // daemon's handshake path returns immediately after
                    // sending Rejected too.
                    continue;
                }

                let accept = WelcomeMsg::Accepted {
                    server_version: FAKE_LEGACY_VERSION,
                    sessions: Vec::<crate::mux::ipc::protocol::SessionInfo>::new(),
                };
                write_welcome(&mut stream, &accept);

                let shutdown_frame = read_frame(&mut stream);
                assert_eq!(shutdown_frame.msg_type, MessageType::Shutdown);

                // Simulate process exit: release the socket like the real
                // daemon's shutdown path does.
                let _ = std::fs::remove_file(&sock_path);
                break;
            }
        })
    }

    /// AC-1: a v2 client recovers from encountering a running v1 daemon —
    /// `recover_from_legacy_daemon` detects the mismatch, sends a
    /// version-tolerant Shutdown, waits for the legacy daemon to release
    /// the socket, and reports `Recovered`.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_shuts_down_v1_and_reports_recovered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("legacy.sock");
        let server = spawn_fake_legacy_daemon(sock_path.clone());

        let result = recover_from_legacy_daemon(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Recovered) => {}
            other => panic!("expected Ok(Recovered), got {other:?}"),
        }
        assert!(
            !sock_path.exists(),
            "legacy daemon's socket file must be removed after recovery"
        );
    }

    /// AC-4: a compatible (current-version) daemon is left untouched —
    /// `recover_from_legacy_daemon` performs exactly one Hello/Welcome
    /// round trip and reports `Compatible` without sending Shutdown.
    #[cfg(unix)]
    #[test]
    fn recover_from_legacy_daemon_is_noop_against_a_compatible_daemon() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("compatible.sock");
        let listener = UnixListener::bind(&sock_path).expect("bind fake v2 daemon socket");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let hello_frame = read_frame(&mut stream);
            let hello: HelloMsg = hello_frame.decode_payload().expect("Hello payload");
            assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
            let accept = WelcomeMsg::Accepted {
                server_version: PROTOCOL_VERSION,
                sessions: Vec::<crate::mux::ipc::protocol::SessionInfo>::new(),
            };
            write_welcome(&mut stream, &accept);
        });

        let result = recover_from_legacy_daemon(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(LegacyRecovery::Compatible) => {}
            other => panic!("expected Ok(Compatible), got {other:?}"),
        }
        assert!(sock_path.exists(), "a compatible daemon is left untouched");
    }

    /// AC-2: `emterm mux kill`'s underlying helper succeeds against a v1
    /// daemon. AC-3: the resulting message is plain, human-readable text —
    /// never an opaque bincode/decode error.
    #[cfg(unix)]
    #[test]
    fn shutdown_daemon_any_version_succeeds_against_v1_daemon() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("legacy.sock");
        let server = spawn_fake_legacy_daemon(sock_path.clone());

        let result = shutdown_daemon_any_version(&sock_path);
        server.join().expect("fake daemon thread panicked");

        match result {
            Ok(ShutdownOutcome::ShutDown(msg)) => {
                assert!(
                    msg.is_ascii(),
                    "expected a plain-text status message, got {msg:?}"
                );
                assert!(
                    msg.to_lowercase().contains("protocol version"),
                    "expected the message to explain the protocol mismatch, got {msg:?}"
                );
            }
            other => panic!("expected Ok(ShutDown(_)), got {other:?}"),
        }
    }

    /// `shutdown_daemon_any_version` falls back to stale-file cleanup when
    /// the daemon is unreachable outright (process already gone), mirroring
    /// the pre-task0010 `execute_kill` behavior.
    #[cfg(unix)]
    #[test]
    fn shutdown_daemon_any_version_removes_stale_socket_when_unreachable() {
        let dir = tempfile::tempdir().expect("tempdir");
        // No listener bound at this path: connect() fails immediately.
        let sock_path = dir.path().join("nothing-here.sock");

        let result = shutdown_daemon_any_version(&sock_path);
        match result {
            Ok(ShutdownOutcome::StaleSocketRemoved(msg)) => {
                assert!(msg.contains("not reachable"));
            }
            other => panic!("expected Ok(StaleSocketRemoved(_)), got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_marks_all_panes_exited() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::sync::mpsc;

        fn make_test_pane(id: u32) -> MuxPane {
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            MuxPane::new_test(id, 80, 24, target)
        }

        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        // Set up two sessions with panes
        {
            let mut m = mgr.lock().await;
            let s1 = m.create_session("s1".to_string());
            let w1 = m.create_window(s1, "w1".to_string()).unwrap();
            let session = m.get_session_mut(s1).unwrap();
            session
                .windows
                .get_mut(&w1)
                .unwrap()
                .add_pane(make_test_pane(10));
            session
                .windows
                .get_mut(&w1)
                .unwrap()
                .add_pane(make_test_pane(11));

            let s2 = m.create_session("s2".to_string());
            let w2 = m.create_window(s2, "w2".to_string()).unwrap();
            let session2 = m.get_session_mut(s2).unwrap();
            session2
                .windows
                .get_mut(&w2)
                .unwrap()
                .add_pane(make_test_pane(20));
        }

        graceful_shutdown(&mgr).await;

        // Verify all panes are marked exited
        let m = mgr.lock().await;
        for session in m.sessions_iter() {
            for window in session.windows.values() {
                for pane in window.panes.values() {
                    assert!(pane.exited, "pane {} should be exited", pane.id);
                }
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_skips_already_exited() {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        use tokio::sync::mpsc;

        fn make_test_pane(id: u32) -> MuxPane {
            let (tx, _rx) = mpsc::channel(1);
            let target: SharedOutputTarget =
                StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
            MuxPane::new_test(id, 80, 24, target)
        }

        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        {
            let mut m = mgr.lock().await;
            let s1 = m.create_session("s1".to_string());
            let w1 = m.create_window(s1, "w1".to_string()).unwrap();
            let session = m.get_session_mut(s1).unwrap();
            let window = session.windows.get_mut(&w1).unwrap();
            window.add_pane(make_test_pane(10));
            window.add_pane(make_test_pane(11));
            // Mark one pane as already exited
            window.panes.get_mut(&10).unwrap().mark_exited();
        }

        // Should not panic; should handle already-exited panes gracefully
        graceful_shutdown(&mgr).await;

        let m = mgr.lock().await;
        let session = m.sessions_iter().next().unwrap();
        let window = session.windows.values().next().unwrap();
        assert!(window.panes.get(&10).unwrap().exited);
        assert!(window.panes.get(&11).unwrap().exited);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_graceful_shutdown_empty_manager() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        // Should not panic on empty manager
        graceful_shutdown(&mgr).await;
    }

    /// Helpers for title-update tests. Returns the pane plus the `Sender`
    /// installed into its `output_target`, so tests can pass the matching
    /// `Sender` to identity-scoped `detach_session_panes`.
    fn make_title_test_pane(
        id: u32,
    ) -> (
        crate::mux::session::pane::MuxPane,
        mpsc::Sender<crate::mux::session::pane::PtyOutputChunk>,
    ) {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget =
            StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
        (MuxPane::new_test(id, 80, 24, target), tx)
    }

    async fn setup_single_pane_manager() -> (
        Arc<Mutex<SessionManager>>,
        u32,
        u32,
        u32,
        mpsc::Sender<crate::mux::session::pane::PtyOutputChunk>,
    ) {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        let pane_id = 42;
        let (pane, pane_tx) = make_title_test_pane(pane_id);
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        drop(m);
        (mgr, sid, wid, pane_id, pane_tx)
    }

    #[tokio::test]
    async fn test_apply_title_change_updates_window_and_broadcasts() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let changed = apply_title_change(&mgr, pane_id, "hello".to_string()).await;
        assert!(changed, "first title change should return true");

        let m = mgr.lock().await;
        let name = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .name
            .clone();
        assert_eq!(name, "hello");
        drop(m);

        let msg = notify_rx.recv().await.unwrap();
        assert_eq!(msg.msg_type, MessageType::RenameWindow);
        assert_eq!(msg.pane_id, wid);
        let payload: RenameWindowMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.name, "hello");
    }

    #[tokio::test]
    async fn test_apply_title_change_same_title_skips_broadcast() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let _ = apply_title_change(&mgr, pane_id, "hello".to_string()).await;

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        let changed = apply_title_change(&mgr, pane_id, "hello".to_string()).await;
        assert!(!changed, "same title should return false");

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            timeout.is_err(),
            "no broadcast should be sent for unchanged title"
        );
    }

    #[tokio::test]
    async fn test_apply_title_change_unknown_pane_no_change() {
        let (mgr, sid, wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let changed = apply_title_change(&mgr, 9999, "bogus".to_string()).await;
        assert!(!changed);

        let m = mgr.lock().await;
        assert_eq!(
            m.get_session(sid).unwrap().windows.get(&wid).unwrap().name,
            "shell"
        );
        drop(m);

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "no broadcast for unknown pane");
    }

    #[tokio::test]
    async fn test_title_update_task_applies_messages_from_channel() {
        let (mgr, sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let (tx, rx) = mpsc::channel::<(u32, String)>(8);
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        let mgr_clone = mgr.clone();
        let task = tokio::spawn(run_title_update_task(mgr_clone, rx));

        tx.send((pane_id, "first".to_string())).await.unwrap();
        tx.send((pane_id, "first".to_string())).await.unwrap();
        tx.send((pane_id, "second".to_string())).await.unwrap();

        // Expect two broadcasts: "first" and "second"
        let msg1 = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let p1: RenameWindowMsg = msg1.decode_payload().unwrap();
        assert_eq!(p1.name, "first");
        let msg2 = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let p2: RenameWindowMsg = msg2.decode_payload().unwrap();
        assert_eq!(p2.name, "second");

        // Drop sender so task exits.
        drop(tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;

        let m = mgr.lock().await;
        assert_eq!(
            m.get_session(sid).unwrap().windows.get(&wid).unwrap().name,
            "second"
        );
    }

    /// TS-10: after a detach (output_target switched to Detached, title_sender
    /// preserved), a title change still propagates to window.name through the
    /// daemon-level title task. The subsequent Welcome snapshot observes the
    /// updated name.
    #[tokio::test]
    async fn test_detached_pane_title_change_updates_window_name() {
        use crate::mux::ipc::reattach::detach_session_panes;

        let (mgr, sid, wid, pane_id, pane_tx) = setup_single_pane_manager().await;
        let (tx, rx) = mpsc::channel::<(u32, String)>(8);

        // Attach the daemon-level tx to the pane (simulating CLI-created pane).
        {
            let m = mgr.lock().await;
            let session = m.get_session(sid).unwrap();
            let pane = session
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            *pane.title_sender.lock().unwrap() = Some(tx.clone());
        }

        // Simulate GUI disconnect: pass the pane's matching tx so the
        // identity-scoped detach_session_panes actually flips output_target
        // to Detached. The assertion below verifies title_sender is preserved
        // through this state transition.
        detach_session_panes(&mgr, sid, &pane_tx).await;
        {
            let m = mgr.lock().await;
            let session = m.get_session(sid).unwrap();
            let pane = session
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            assert!(
                pane.title_sender.lock().unwrap().is_some(),
                "detach must preserve title_sender"
            );
        }

        // Launch the daemon-level title task and send a title through the
        // pane-side sender to simulate an OSC update while detached.
        let task = tokio::spawn(run_title_update_task(mgr.clone(), rx));
        tx.send((pane_id, "detached-title".to_string()))
            .await
            .unwrap();
        drop(tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;

        // The next Welcome would observe this new name via session_list().
        let m = mgr.lock().await;
        let list = m.session_list();
        let window = list[0].windows.iter().find(|w| w.id == wid).unwrap();
        assert_eq!(window.name, "detached-title");
    }

    /// Build a pane whose `output_target` is `Detached(NetworkDetach)` with a
    /// system origin (`owner = None`), matching the state a pane is left in by
    /// `detach_session_panes` during the connection-reset race (FR6).
    #[cfg(unix)]
    fn make_detached_test_pane(id: u32) -> crate::mux::session::pane::MuxPane {
        use crate::mux::session::pane::{
            DetachReason, MuxPane, PaneOutputTarget, SharedOutputTarget,
        };
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let target: SharedOutputTarget = StdArc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        MuxPane::new_test(id, 80, 24, target)
    }

    /// Build a Connected test pane (the default attached state).
    #[cfg(unix)]
    fn make_connected_test_pane(id: u32) -> crate::mux::session::pane::MuxPane {
        use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
        use std::sync::{Arc as StdArc, Mutex as StdMutex};
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget =
            StdArc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        MuxPane::new_test(id, 80, 24, target)
    }

    /// TS-1: detached last-pane reap drives shutdown. One session / window /
    /// pane fed to the reap task; the pane is removed, the session is gone,
    /// the manager is empty, and the watch channel observes `true`.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_task_last_pane_reap_fires_shutdown() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 42u32;
        let (sid, wid) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_detached_test_pane(pane_id));
            (sid, wid)
        };

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let (exit_tx, exit_rx) = mpsc::channel::<u32>(PANE_EXIT_CHANNEL_CAPACITY);
        let task = tokio::spawn(run_pane_exit_task(
            mgr.clone(),
            shutdown_tx.clone(),
            exit_rx,
        ));

        exit_tx.send(pane_id).await.unwrap();
        // Wait for the reap task to fire the shutdown signal.
        shutdown_rx.changed().await.unwrap();
        assert!(*shutdown_rx.borrow(), "shutdown signal must be true");

        let m = mgr.lock().await;
        assert!(m.is_empty(), "manager must be empty after last pane reaped");
        assert!(m.get_session(sid).is_none(), "session must be removed");
        assert!(
            m.get_session(sid)
                .and_then(|s| s.windows.get(&wid))
                .is_none(),
            "window must be removed"
        );
        drop(m);

        drop(exit_tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;
    }

    /// TS-2: detached non-last pane reap. Two panes in distinct windows; reap
    /// one and assert only it is removed and the shutdown signal does NOT fire.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_task_non_last_pane_reap_keeps_daemon_alive() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (sid, wid1, wid2) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid1 = m.create_window(sid, "w1".to_string()).unwrap();
            let wid2 = m.create_window(sid, "w2".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid1)
                .unwrap()
                .add_pane(make_detached_test_pane(1));
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid2)
                .unwrap()
                .add_pane(make_detached_test_pane(2));
            (sid, wid1, wid2)
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        handle_destroy_pane(1, &mgr, &shutdown_tx).await;

        // Pane 1 removed, its (now-empty) window removed; pane 2 / its window
        // intact; the session survives; shutdown NOT fired.
        let m = mgr.lock().await;
        assert!(!m.is_empty(), "session must survive a non-last reap");
        let session = m.get_session(sid).expect("session must remain");
        assert!(
            session.windows.get(&wid1).is_none(),
            "emptied window must be pruned"
        );
        let window2 = session.windows.get(&wid2).expect("window 2 must remain");
        assert!(window2.panes.contains_key(&2), "pane 2 must remain");
        drop(m);

        assert!(
            !*shutdown_rx.borrow(),
            "shutdown signal must not fire while a pane remains"
        );
    }

    /// TS-3: connection-reset race (FR6). A pane switched to
    /// `Detached(NetworkDetach)` (as `detach_session_panes` does) must still be
    /// reaped regardless of its `output_target`.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_reap_removes_network_detached_pane() {
        use crate::mux::session::pane::PaneOutputTarget;

        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 7u32;
        let sid = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_detached_test_pane(pane_id));
            // Confirm the precondition: the pane is Detached, not Connected.
            let pane = m
                .get_session(sid)
                .unwrap()
                .windows
                .get(&wid)
                .unwrap()
                .panes
                .get(&pane_id)
                .unwrap();
            assert!(matches!(
                *pane.output_target.lock().unwrap(),
                PaneOutputTarget::Detached { .. }
            ));
            sid
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;

        let m = mgr.lock().await;
        assert!(
            m.get_session(sid).is_none(),
            "the detached pane's session must be reaped despite Detached target"
        );
        assert!(
            m.is_empty(),
            "manager must be empty after reaping last pane"
        );
        drop(m);
        // Last pane gone -> shutdown fires.
        assert!(
            *shutdown_rx.borrow(),
            "shutdown must fire on last pane reap"
        );
    }

    /// TS-4: idempotent reap (FR4). Reaping the same pane id twice — and also
    /// a pane that was already torn down via the Connected empty-chunk path —
    /// is a safe no-op: no panic, and the shutdown signal is not re-fired.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_pane_exit_reap_is_idempotent() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let pane_id = 5u32;
        {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(make_connected_test_pane(pane_id));
        }

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        // First reap removes the pane and fires shutdown (last pane).
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;
        assert!(shutdown_rx.has_changed().unwrap());
        let _ = shutdown_rx.changed().await;
        assert!(*shutdown_rx.borrow());

        // Second reap of the same (already-removed) pane is a safe no-op:
        // no panic, and the watch channel observes no further change.
        handle_destroy_pane(pane_id, &mgr, &shutdown_tx).await;
        assert!(
            !shutdown_rx.has_changed().unwrap(),
            "double reap must not re-fire the shutdown signal"
        );

        let m = mgr.lock().await;
        assert!(m.is_empty());
    }

    /// TS-11: notify_tx subscription taken before Welcome construction must
    /// capture any RenameWindow emitted between snapshot build and message-
    /// loop entry. Emulate the race: subscribe first, then broadcast, then
    /// verify the subscriber receives the event (i.e. no gap).
    #[tokio::test]
    async fn test_subscribe_before_welcome_catches_rename() {
        let (mgr, _sid, wid, pane_id, _pane_tx) = setup_single_pane_manager().await;

        // Phase A: subscribe BEFORE any broadcast (simulates the reordered
        // sequence in handle_connection).
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        // Phase B: build Welcome snapshot (mimics the Welcome frame build).
        let _snapshot_len = { mgr.lock().await.session_list().len() };

        // Phase C: broadcast arrives between snapshot and loop-entry.
        apply_title_change(&mgr, pane_id, "raced-title".to_string()).await;

        // Phase D: subscriber must see it (the race is closed).
        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("notify_rx should receive RenameWindow")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::RenameWindow);
        assert_eq!(msg.pane_id, wid);
        let payload: RenameWindowMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.name, "raced-title");
    }

    // ── apply_agent_status_report / sync_agent_status_after_snapshot ─────
    // (SPEC FR3/FR4/FR5, task0003 AC-1/AC-2/AC-4/AC-5)

    /// AC-4: an accepted report updates the pane and broadcasts exactly one
    /// `AgentStatusUpdate` with `replay_derived = false` and the pane's
    /// current public ID.
    #[tokio::test]
    async fn test_apply_agent_status_report_accepted_broadcasts_update() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=working;name=claude".to_string(),
        )
        .await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive AgentStatusUpdate")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        assert_eq!(msg.pane_id, pane_id);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        let expected_public_id = { mgr.lock().await.public_pane_id(pane_id) };
        assert_eq!(payload.public_pane_id, expected_public_id);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Working)
        );
        assert_eq!(payload.name.as_deref(), Some("claude"));
        assert_eq!(payload.revision, 1);
        assert!(!payload.replay_derived);

        // No further message pending (exactly one broadcast).
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(none.is_err(), "exactly one AgentStatusUpdate expected");
    }

    /// AC-2: a rejected sequence leaves state and revision untouched and
    /// broadcasts nothing.
    #[tokio::test]
    async fn test_apply_agent_status_report_rejected_no_broadcast_no_mutation() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=bogus".to_string(),
        )
        .await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "rejected report must not broadcast");

        let m = mgr.lock().await;
        let pane = m
            .get_session(m.find_pane(pane_id).unwrap().0)
            .and_then(|s| s.windows.values().next())
            .and_then(|w| w.panes.get(&pane_id))
            .unwrap();
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 0);
    }

    /// AC-2: same-state re-report is accepted (revision increments) and
    /// broadcasts again.
    #[tokio::test]
    async fn test_apply_agent_status_report_same_state_re_report_broadcasts_again() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=idle".to_string(),
        )
        .await;
        apply_agent_status_report(
            &mgr,
            pane_id,
            "emterm;agent-status;v=1;state=idle".to_string(),
        )
        .await;

        let msg1 = notify_rx.recv().await.unwrap();
        let p1: AgentStatusUpdateMsg = msg1.decode_payload().unwrap();
        let msg2 = notify_rx.recv().await.unwrap();
        let p2: AgentStatusUpdateMsg = msg2.decode_payload().unwrap();
        assert_eq!(p1.revision, 1);
        assert_eq!(p2.revision, 2);
    }

    /// AC-4: an unknown pane_id is a no-op (no broadcast, no panic).
    #[tokio::test]
    async fn test_apply_agent_status_report_unknown_pane_no_broadcast() {
        let (mgr, _sid, _wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        apply_agent_status_report(
            &mgr,
            9999,
            "emterm;agent-status;v=1;state=working".to_string(),
        )
        .await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "unknown pane must not broadcast");
    }

    /// AC-5: after a snapshot, each stateful pane produces one
    /// `AgentStatusUpdate` with `replay_derived = true`; a stateless pane
    /// produces none.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_only_stateful_panes() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (sid, stateful_id, stateless_id) = {
            let mut m = mgr.lock().await;
            let sid = m.create_session("default".to_string());
            let wid = m.create_window(sid, "shell".to_string()).unwrap();
            let (stateful_pane, _tx1) = make_title_test_pane(1);
            let (stateless_pane, _tx2) = make_title_test_pane(2);
            stateful_pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Blocked,
                name: Some("agent".to_string()),
            });
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(stateful_pane);
            m.get_session_mut(sid)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(stateless_pane);
            (sid, 1u32, 2u32)
        };

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_snapshot(&mgr, sid).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive one AgentStatusUpdate")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, stateful_id);
        assert!(payload.replay_derived);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Blocked)
        );

        // Nothing further: the stateless pane produces no message.
        let none =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            none.is_err(),
            "stateless pane {} must not produce a message",
            stateless_id
        );
    }

    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_unknown_session_no_panic() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        // Should not panic on an unknown session id.
        sync_agent_status_after_snapshot(&mgr, 9999).await;
    }

    /// task0013 AC-1 (rework, review round 1 `replay_clear_lost`): a pane
    /// that transitioned blocked -> cleared (revision now 2, state now
    /// None) while the GUI was detached must still produce a
    /// replay-derived `AgentStatusUpdate` with `state: None` on reattach,
    /// so the stale badge/summary from before the clear is replaced.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_cleared_pane_emits_state_none() {
        let (mgr, sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Blocked,
                name: Some("agent".to_string()),
            });
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Clear);
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_snapshot(&mgr, sid).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive a replay-derived AgentStatusUpdate for the cleared pane")
            .unwrap();
        assert_eq!(msg.msg_type, MessageType::AgentStatusUpdate);
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(payload.state, None, "cleared pane must sync as state=None");
        assert_eq!(payload.name, None);
        assert_eq!(payload.revision, 2);
    }

    /// task0013 AC-2: a pane that has never reported any state (revision
    /// still 0) must not produce a sync message on reattach — no
    /// unnecessary state=None update for a pane that never had state.
    #[tokio::test]
    async fn test_sync_agent_status_after_snapshot_never_reported_pane_no_broadcast() {
        let (mgr, sid, _wid, _pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        sync_agent_status_after_snapshot(&mgr, sid).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(
            timeout.is_err(),
            "a pane that never reported must not produce a sync message"
        );
    }

    /// AC-5 (per-pane / window-switch counterpart): a stateful pane
    /// produces one `AgentStatusUpdate` with `replay_derived = true`.
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_stateful_pane_broadcasts() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Done,
                name: None,
            });
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive AgentStatusUpdate")
            .unwrap();
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(
            payload.state,
            Some(crate::mux::ipc::protocol::AgentState::Done)
        );
    }

    /// AC-5: a stateless (never-reported, revision == 0) pane produces no
    /// message (task0013 AC-2).
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_stateless_pane_no_broadcast() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };

        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let timeout =
            tokio::time::timeout(std::time::Duration::from_millis(50), notify_rx.recv()).await;
        assert!(timeout.is_err(), "stateless pane must not broadcast");
    }

    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_unknown_pane_no_panic() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        sync_agent_status_after_pane_snapshot(&mgr, 9999).await;
    }

    /// task0013 AC-1 (per-pane / window-switch counterpart): a pane that
    /// transitioned blocked -> cleared while the GUI was detached must
    /// still produce a replay-derived `AgentStatusUpdate` with
    /// `state: None` on the per-pane snapshot sync path.
    #[tokio::test]
    async fn test_sync_agent_status_after_pane_snapshot_cleared_pane_emits_state_none() {
        let (mgr, _sid, _wid, pane_id, _pane_tx) = setup_single_pane_manager().await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(m.find_pane(pane_id).unwrap().0)
                .and_then(|s| s.windows.values().next())
                .and_then(|w| w.panes.get(&pane_id))
                .unwrap();
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Set {
                state: crate::agent_status::AgentState::Working,
                name: Some("agent".to_string()),
            });
            pane.apply_agent_status_event(crate::agent_status::AgentStatusEvent::Clear);
        }

        let mut notify_rx = { mgr.lock().await.notify_tx().subscribe() };
        sync_agent_status_after_pane_snapshot(&mgr, pane_id).await;

        let msg = tokio::time::timeout(std::time::Duration::from_secs(1), notify_rx.recv())
            .await
            .expect("must receive a replay-derived AgentStatusUpdate for the cleared pane")
            .unwrap();
        let payload: AgentStatusUpdateMsg = msg.decode_payload().unwrap();
        assert_eq!(payload.pane_id, pane_id);
        assert!(payload.replay_derived);
        assert_eq!(payload.state, None, "cleared pane must sync as state=None");
        assert_eq!(payload.revision, 2);
    }
}
