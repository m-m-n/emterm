//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

use std::path::PathBuf;
use std::sync::OnceLock;

use super::ipc::connection::handle_connection;
use super::ipc::handlers::handle_destroy_pane;
use super::ipc::protocol::{MessageType, MuxMessage, NotifyMsg, RenameWindowMsg};
use super::session::manager::SessionManager;
use super::session::pane::{
    NotificationSender, PaneExitSender, PaneId, SharedPaneExitSender, TitleChangeSender,
};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;
use tokio::sync::mpsc;

/// Process-global daemon incarnation token (IMPLEMENTATION.md "Public pane
/// ID format"): a lowercase-hex token minted once per daemon process,
/// combined with the wire `pane_id` (u32) to form the public, opaque,
/// non-reusable-across-restarts pane ID. Lazily generated on first use so
/// every code path (production daemon, unit tests) shares one instance for
/// the lifetime of the process.
///
/// This is task0004-provisional infrastructure: IMPLEMENTATION.md assigns
/// incarnation-token GENERATION to task0003 (`session/manager.rs`, at
/// daemon start) and compose/parse helpers to task0002 (`mux_ipc`). A
/// process-global static here satisfies "generated once per daemon start"
/// without task0004 needing to touch either of those files ahead of their
/// owners landing; reconciliation is expected via parent-side adoption.
static DAEMON_INCARNATION: OnceLock<String> = OnceLock::new();

/// Return this daemon process's incarnation token, generating it on first
/// call. Dependency-free (no external RNG crate): mixes the current time,
/// the process ID, and a stack-address salt into a 64-bit value rendered as
/// lowercase hex.
pub fn daemon_incarnation() -> &'static str {
    DAEMON_INCARNATION.get_or_init(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        // A stack-local address as a lightweight, dependency-free salt —
        // not cryptographic, just enough entropy to avoid same-nanosecond
        // collisions across processes started in a tight loop (e.g. tests).
        let local = 0u8;
        let salt = std::ptr::addr_of!(local) as u128;
        let mixed = nanos ^ (pid << 32) ^ salt;
        format!("{:016x}", mixed as u64)
    })
}

/// Build the public (API-facing, opaque) pane ID for `pane_id` in this
/// daemon's incarnation. See [`daemon_incarnation`] and
/// `mux_ipc::protocol::compose_public_pane_id`.
pub fn public_pane_id(pane_id: PaneId) -> String {
    super::ipc::protocol::compose_public_pane_id(daemon_incarnation(), pane_id)
}

/// Parse a public pane ID, returning the internal [`PaneId`] only when the
/// embedded incarnation token matches this daemon's CURRENT incarnation.
/// A syntactically valid but stale (previous-daemon-run) incarnation, or
/// any malformed input, returns `None` — callers map that uniformly to
/// `unknown_pane` per IMPLEMENTATION.md's shared error contract.
pub fn resolve_public_pane_id(public_id: &str) -> Option<PaneId> {
    let (incarnation, pane_id) = super::ipc::protocol::parse_public_pane_id(public_id)?;
    if incarnation != daemon_incarnation() {
        return None;
    }
    Some(pane_id)
}

/// Daemon-level title channel capacity.
const TITLE_CHANNEL_CAPACITY: usize = 64;

/// Daemon-level notification channel capacity (OSC 9 desktop notifications
/// detected on Detached panes).
const NOTIFICATION_CHANNEL_CAPACITY: usize = 64;

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

    let daemon_running = if cfg!(unix) {
        sock_path.exists()
    } else {
        is_daemon_running(&sock_path)
    };
    if !daemon_running {
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
            if is_daemon_running(&sock_path) {
                started = true;
                break;
            }
            let delay = std::cmp::min(10 * (1 << i.min(4)), 100);
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        if !started {
            return Err("Failed to start mux daemon".to_string());
        }
    }

    Ok(sock_path)
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
                        tokio::spawn(handle_connection(stream, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), pane_exit_sender.clone()));
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
                        tokio::spawn(handle_connection(server, session_manager.clone(), shutdown_tx.clone(), title_tx.clone(), notification_tx.clone(), pane_exit_sender.clone()));
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

    // ---- daemon incarnation / public pane ID (task0004, IMPLEMENTATION.md
    // "Public pane ID format") ----

    /// AC-6/AC-7 support: the incarnation token is stable across calls
    /// within one process (the `OnceLock` is generated once), so all public
    /// IDs minted in this test binary's run share it.
    #[test]
    fn daemon_incarnation_is_stable_across_calls() {
        let a = daemon_incarnation();
        let b = daemon_incarnation();
        assert_eq!(a, b);
        assert!(!a.is_empty());
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "incarnation must be lowercase hex, got {a:?}"
        );
    }

    #[test]
    fn public_pane_id_round_trips_through_resolve() {
        let id = public_pane_id(42);
        assert!(id.starts_with(daemon_incarnation()));
        assert_eq!(resolve_public_pane_id(&id), Some(42));
    }

    #[test]
    fn resolve_public_pane_id_rejects_stale_incarnation() {
        // A syntactically valid public pane ID minted under a DIFFERENT
        // (stale) incarnation must resolve to None, not to the numeric
        // pane_id — this is what makes a pane ID non-reusable across
        // daemon restarts (a request "unknown_pane"s instead of silently
        // targeting a same-numbered pane from a fresh daemon run).
        let stale = super::super::ipc::protocol::compose_public_pane_id("deadbeef00000000", 7);
        assert_ne!(stale.split('-').next().unwrap(), daemon_incarnation());
        assert_eq!(resolve_public_pane_id(&stale), None);
    }

    #[test]
    fn resolve_public_pane_id_rejects_malformed_input() {
        assert_eq!(resolve_public_pane_id(""), None);
        assert_eq!(resolve_public_pane_id("no-separator-here-notanumber"), None);
        assert_eq!(resolve_public_pane_id("not_mux_pane_at_all"), None);
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
}
