//! Daemon endpoint discovery and the client-side connect path: socket /
//! pipe naming, liveness probing, stale-socket cleanup, daemon spawning,
//! and the version handshake.

use super::*;
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

/// Create the socket's parent directory with restricted permissions, spawn
/// the daemon as a detached background process, and wait for it to become
/// ready with exponential backoff.
///
/// Extracted out of the ensure-daemon-running bootstrap (task0001, now in
/// `control_client.rs`) so the `emterm mux attach` path can respawn a
/// daemon after a legacy-daemon recovery shutdown, without duplicating the
/// spawn/readiness logic.
///
/// Precondition: no compatible daemon currently owns `sock_path` (the
/// caller is responsible for stale-socket cleanup, the presence check, and
/// the recovery probe, as `control_client.rs`'s ensure-daemon-running
/// bootstrap does). Postcondition: a
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

/// Connect to the daemon's control channel with read/write timeouts, ready
/// for a [`handshake_with_version`] call. Unix: the `AF_UNIX` socket at
/// `sock_path`. Windows: the daemon's Named Pipe (`sock_path` is unused
/// there, matching [`is_daemon_running`]'s existing `_path` convention).
///
/// `pub(in crate::mux)` (task0005): widened so `mux::cli::execute_upgrade`
/// can drive the same connect/handshake sequence for the standalone
/// `upgrade` subcommand, without duplicating it.
#[cfg(unix)]
pub(in crate::mux) fn connect_daemon(
    sock_path: &Path,
) -> std::io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(sock_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

#[cfg(windows)]
pub(in crate::mux) fn connect_daemon(_sock_path: &Path) -> std::io::Result<std::fs::File> {
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
///
/// `pub(in crate::mux)` (task0005): widened alongside [`connect_daemon`] for
/// `mux::cli::execute_upgrade`.
pub(in crate::mux) fn handshake_with_version<S: std::io::Read + std::io::Write>(
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
