//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

use std::path::PathBuf;

#[cfg(unix)]
use std::sync::Arc;

#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(unix)]
use tokio::sync::Mutex;

#[cfg(unix)]
use super::ipc::connection::handle_connection;
#[cfg(unix)]
use super::session::manager::SessionManager;

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

/// Check if a daemon is already running by attempting to connect to the socket.
#[cfg(unix)]
pub fn is_daemon_running(path: &std::path::Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(windows)]
pub fn is_daemon_running(path: &std::path::Path) -> bool {
    // On Windows, attempt to connect to the AF_UNIX socket
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        std::time::Duration::from_millis(100),
    )
    .is_err(); // placeholder — Windows AF_UNIX requires separate handling
    path.exists() // fallback: assume running if socket file exists
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

    if !sock_path.exists() {
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

        let exe =
            std::env::current_exe().map_err(|e| format!("Failed to get executable path: {}", e))?;

        let log_path = sock_path.with_file_name("mux-daemon.log");
        let log_file = std::fs::File::create(&log_path)
            .unwrap_or_else(|_| std::fs::File::create("/tmp/emterm-mux-daemon.log").unwrap());

        let mut cmd = std::process::Command::new(&exe);
        cmd.args(["mux", "--daemon"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(log_file));

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

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn daemon: {}", e))?;

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

    // Shutdown signal: sent by handle_destroy_pane/handle_destroy_window when all sessions empty
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

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
                        tokio::spawn(handle_connection(stream, session_manager.clone(), shutdown_tx.clone()));
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

        #[cfg(windows)]
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        tokio::spawn(handle_connection(stream, session_manager.clone(), shutdown_tx.clone()));
                    }
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("Ctrl+C received, shutting down");
                break;
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

/// Run the mux daemon.
///
/// Not yet supported on Windows. Returns an error with a clear message.
#[cfg(not(unix))]
pub async fn run_daemon() -> anyhow::Result<()> {
    anyhow::bail!("Mux daemon is not yet supported on this platform. Linux is required.");
}

/// Close all PTYs in all sessions for graceful daemon shutdown.
#[cfg(unix)]
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
}
