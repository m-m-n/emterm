//! Mux daemon process entry point.
//!
//! Listens on a Unix domain socket, accepts client connections,
//! and manages PTY sessions. Auto-exits when all sessions end.

use std::path::PathBuf;

use tokio::net::UnixListener;

use super::ipc::connection::handle_connection;

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
pub fn is_daemon_running(path: &std::path::Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

/// Remove stale socket file if daemon is not running.
pub fn cleanup_stale_socket(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() && !is_daemon_running(path) {
        log::info!("Removing stale socket: {:?}", path);
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Run the mux daemon.
///
/// This is the main entry point for `emterm mux --daemon`.
/// It blocks until all sessions end or SIGTERM is received.
pub async fn run_daemon() -> anyhow::Result<()> {
    let sock_path = socket_path();

    // Ensure parent directory exists
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Clean up stale socket
    cleanup_stale_socket(&sock_path)?;

    // Bind listener
    let listener = UnixListener::bind(&sock_path)?;
    log::info!("Mux daemon listening on {:?}", sock_path);

    // Handle SIGTERM for graceful shutdown
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    loop {
        #[cfg(unix)]
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        tokio::spawn(handle_connection(stream));
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
        }

        #[cfg(windows)]
        {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    tokio::spawn(handle_connection(stream));
                }
                Err(e) => {
                    log::error!("Accept error: {}", e);
                }
            }
        }
    }

    // Cleanup socket file
    let _ = std::fs::remove_file(&sock_path);
    log::info!("Daemon shutdown complete");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
