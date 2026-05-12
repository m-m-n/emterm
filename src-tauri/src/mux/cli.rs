//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` -- Start/attach to default session (long-running bridge)
//! - `emterm mux --daemon` -- Run as daemon process (internal)
//! - `emterm mux attach [session]` -- Attach to existing session (long-running bridge)
//! - `emterm mux ls` -- List sessions
//! - `emterm mux kill [session]` -- Kill a session
//! - `emterm mux new [name]` -- Create a new session

use super::bridge::run_bridge;
use super::daemon;
use super::ipc::protocol::*;
use super::tmux_import::import_tmux_conf_if_needed;

/// Check for nesting (EMTERM_MUX=1).
fn check_nesting() -> Result<(), String> {
    if std::env::var("EMTERM_MUX").is_ok() {
        Err("Cannot nest mux sessions (EMTERM_MUX is set)".to_string())
    } else {
        Ok(())
    }
}

/// Initialize env_logger with a component label prefix (e.g. "[DAEMON]", "[BRIDGE]").
fn init_mux_logger(component: &'static str) {
    use std::io::Write;

    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(move |buf, record| {
            writeln!(
                buf,
                "{} {}{} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f%:z"),
                record.level(),
                component,
                record.args()
            )
        })
        .init();
}

/// Execute the `emterm mux --daemon` command (runs the daemon).
pub fn execute_daemon() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger for daemon process (Tauri's logger is not available here).
    // Daemon stderr is redirected to mux-daemon.log by the spawning process.
    init_mux_logger("[DAEMON]");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon())?;
    Ok(())
}

/// Initialize bridge logger, writing to mux-bridge.log (same directory as daemon log).
///
/// Opens the log in **append mode** so concurrent bridge processes (e.g. two
/// `emterm mux attach` invocations) do not clobber each other's log contents.
/// Hardening (O_NOFOLLOW, 0o600) lives in `daemon::open_mux_log_append` and
/// is shared with the daemon and client log paths.
fn init_bridge_logger() {
    let log_dir = daemon::socket_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("mux-bridge.log");

    match daemon::open_mux_log_append(&log_path) {
        Ok(log_file) => {
            use std::io::Write;

            env_logger::Builder::from_default_env()
                .filter_level(log::LevelFilter::Info)
                .target(env_logger::Target::Pipe(Box::new(log_file)))
                .format(move |buf, record| {
                    writeln!(
                        buf,
                        "{} {}[BRIDGE] {}",
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f%:z"),
                        record.level(),
                        record.args()
                    )
                })
                .init();
        }
        Err(e) => {
            // Visible diagnostic: Windows sharing-mode rejections and Unix
            // `O_NOFOLLOW` symlink refusals both land here. Without this
            // message the bridge would run silently without a log file.
            eprintln!("Bridge logger unavailable ({}): {}", log_path.display(), e);
        }
    }
}

/// Execute the `emterm mux script` command (start daemon without attaching).
///
/// Starts the daemon if not running, then exits immediately.
/// Designed for shell scripts that initialize mux sessions before attaching.
/// Stdout emits exactly one line: the daemon socket path.
#[cfg(unix)]
pub fn execute_script() -> Result<(), Box<dyn std::error::Error>> {
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let sock_path =
        daemon::ensure_daemon_running().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    println!("{}", sock_path.display());
    Ok(())
}

#[cfg(windows)]
pub fn execute_script() -> Result<(), Box<dyn std::error::Error>> {
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let sock_path =
        daemon::ensure_daemon_running().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    println!("{}", sock_path.display());
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
pub fn execute_script() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Execute the `emterm mux` command (start/attach as long-running bridge).
pub fn execute_mux() -> Result<(), Box<dyn std::error::Error>> {
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    init_bridge_logger();

    log::info!("Starting mux bridge (pid={})", std::process::id());

    let sock_path =
        daemon::ensure_daemon_running().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    log::info!("Daemon ready at {:?}", sock_path);

    // Auto-import tmux.conf on first mux startup
    import_tmux_conf_if_needed();

    // Run the long-running bridge process
    run_bridge(&sock_path)?;

    log::info!("Bridge exiting");
    Ok(())
}

/// Execute the `emterm mux attach` command (long-running bridge).
///
/// Attaches to an existing session. If no daemon is running, prints an error.
pub fn execute_attach(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    init_bridge_logger();

    log::info!(
        "Starting mux bridge via attach (pid={})",
        std::process::id()
    );

    let sock_path = daemon::socket_path();

    if !sock_path.exists() {
        return Err("No mux sessions to attach to (daemon not running)\nUse 'emterm mux' to start a new session.".into());
    }

    // Run the long-running bridge process
    run_bridge(&sock_path)?;

    log::info!("Bridge exiting");
    Ok(())
}

/// Connect to the daemon, perform handshake, and return session list.
/// Uses blocking I/O since CLI commands run in a synchronous context.
#[cfg(unix)]
fn cli_handshake()
-> Result<(std::os::unix::net::UnixStream, Vec<SessionInfo>), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        return Err("No mux daemon running".into());
    }

    let mut stream = std::os::unix::net::UnixStream::connect(&sock_path)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    // Send Hello
    let hello = HelloMsg {
        client_type: ClientType::Cli,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    // Read Welcome
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let welcome_msg = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    let welcome: WelcomeMsg = welcome_msg
        .decode_payload()
        .ok_or("Invalid Welcome payload")?;

    match welcome {
        WelcomeMsg::Accepted { sessions, .. } => Ok((stream, sessions)),
        WelcomeMsg::Rejected { reason } => Err(format!("Connection rejected: {}", reason).into()),
    }
}

/// Connect to the daemon via Named Pipe, perform handshake, and return session list.
/// Uses blocking I/O since CLI commands run in a synchronous context.
#[cfg(windows)]
fn cli_handshake() -> Result<(std::fs::File, Vec<SessionInfo>), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let pipe_name = daemon::pipe_name();
    if !daemon::is_daemon_running(&daemon::socket_path()) {
        return Err("No mux daemon running".into());
    }

    let mut stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)?;

    // Send Hello
    let hello = HelloMsg {
        client_type: ClientType::Cli,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    // Read Welcome
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let welcome_msg = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    let welcome: WelcomeMsg = welcome_msg
        .decode_payload()
        .ok_or("Invalid Welcome payload")?;

    match welcome {
        WelcomeMsg::Accepted { sessions, .. } => Ok((stream, sessions)),
        WelcomeMsg::Rejected { reason } => Err(format!("Connection rejected: {}", reason).into()),
    }
}

/// Execute the `emterm mux new-window` command.
///
/// Connects to the daemon, performs handshake, sends CreateWindow with
/// optional name and command, and waits for PaneCreated response.
#[cfg(unix)]
pub fn execute_new_window(
    name: Option<&str>,
    command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let (mut stream, _sessions) = cli_handshake()?;

    // Build CreateWindowPayload
    let payload = CreateWindowPayload {
        name: name.map(|s| s.to_string()),
        command: command.map(|s| s.to_string()),
    };

    // Send CreateWindow message (session_id in pane_id field = 0, daemon uses active session)
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    // Read response
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let resp = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    match resp.msg_type {
        MessageType::PaneCreated => {
            // Success - window created
            Ok(())
        }
        MessageType::Error => {
            let err: ErrorMsg = resp.decode_payload().unwrap_or(ErrorMsg {
                message: "Unknown error".to_string(),
            });
            Err(format!("Failed to create window: {}", err.message).into())
        }
        _ => Err(format!("Unexpected response: {:?}", resp.msg_type).into()),
    }
}

/// Execute the `emterm mux new-window` command (Windows).
#[cfg(windows)]
pub fn execute_new_window(
    name: Option<&str>,
    command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let (mut stream, _sessions) = cli_handshake()?;

    let payload = CreateWindowPayload {
        name: name.map(|s| s.to_string()),
        command: command.map(|s| s.to_string()),
    };

    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let resp = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    match resp.msg_type {
        MessageType::PaneCreated => Ok(()),
        MessageType::Error => {
            let err: ErrorMsg = resp.decode_payload().unwrap_or(ErrorMsg {
                message: "Unknown error".to_string(),
            });
            Err(format!("Failed to create window: {}", err.message).into())
        }
        _ => Err(format!("Unexpected response: {:?}", resp.msg_type).into()),
    }
}

/// Execute the `emterm mux new-window` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_new_window(
    _name: Option<&str>,
    _command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Execute the `emterm mux switch-window` command.
///
/// Connects to the daemon and sends SwitchWindow for the given window index.
#[cfg(unix)]
pub fn execute_switch_window(target: u32) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let (mut stream, sessions) = cli_handshake()?;
    let session = sessions.first().ok_or("No active session")?;

    if target as usize >= session.windows.len() {
        return Err(format!(
            "Window index {} out of range (0..{})",
            target,
            session.windows.len()
        )
        .into());
    }

    let window_id = session.windows[target as usize].id;
    let msg = MuxMessage {
        msg_type: MessageType::SwitchWindow,
        pane_id: window_id,
        payload: vec![],
    };
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux switch-window` command (Windows).
#[cfg(windows)]
pub fn execute_switch_window(target: u32) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let (mut stream, sessions) = cli_handshake()?;
    let session = sessions.first().ok_or("No active session")?;

    if target as usize >= session.windows.len() {
        return Err(format!(
            "Window index {} out of range (0..{})",
            target,
            session.windows.len()
        )
        .into());
    }

    let window_id = session.windows[target as usize].id;
    let msg = MuxMessage {
        msg_type: MessageType::SwitchWindow,
        pane_id: window_id,
        payload: vec![],
    };
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux switch-window` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_switch_window(_target: u32) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Resolve the target pane ID from sessions and optional window index.
///
/// Returns the active pane ID of the resolved window.
fn resolve_target_pane(
    sessions: &[SessionInfo],
    target: Option<u32>,
) -> Result<u32, Box<dyn std::error::Error>> {
    let session = sessions.first().ok_or("No active session")?;

    let window_index = match target {
        Some(idx) => {
            if idx as usize >= session.windows.len() {
                return Err(format!(
                    "Window index {} out of range (0..{})",
                    idx,
                    session.windows.len()
                )
                .into());
            }
            idx as usize
        }
        None => {
            if session.windows.is_empty() {
                return Err("No windows in session".into());
            }
            let idx = session.active_window_index as usize;
            if idx >= session.windows.len() {
                return Err(format!(
                    "Active window index {} out of range (0..{})",
                    idx,
                    session.windows.len()
                )
                .into());
            }
            idx
        }
    };

    let window = &session.windows[window_index];
    let pane_id = window.active_pane_id;

    if pane_id == 0 {
        return Err(format!("No active pane in window {}", window_index).into());
    }

    Ok(pane_id)
}

/// Execute the `emterm mux send-keys` command.
///
/// Reads stdin, connects to daemon, resolves target pane from window index,
/// and sends PtyInput message.
#[cfg(unix)]
pub fn execute_send_keys(target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    // Read stdin with size limit (MAX_FRAME_LENGTH = 16MB)
    let mut data = Vec::new();
    let bytes_read = std::io::stdin()
        .take(MAX_FRAME_LENGTH as u64 + 1)
        .read_to_end(&mut data)?;
    if bytes_read > MAX_FRAME_LENGTH {
        return Err(format!(
            "stdin data exceeds maximum size ({}MB)",
            MAX_FRAME_LENGTH / 1024 / 1024
        )
        .into());
    }

    // Empty stdin: exit 0 without connecting
    if data.is_empty() {
        return Ok(());
    }

    let (mut stream, sessions) = cli_handshake()?;

    let pane_id = resolve_target_pane(&sessions, target)?;

    // Send PtyInput
    let msg = MuxMessage::pty_input(pane_id, data);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux send-keys` command (Windows).
#[cfg(windows)]
pub fn execute_send_keys(target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let mut data = Vec::new();
    let bytes_read = std::io::stdin()
        .take(MAX_FRAME_LENGTH as u64 + 1)
        .read_to_end(&mut data)?;
    if bytes_read > MAX_FRAME_LENGTH {
        return Err(format!(
            "stdin data exceeds maximum size ({}MB)",
            MAX_FRAME_LENGTH / 1024 / 1024
        )
        .into());
    }

    if data.is_empty() {
        return Ok(());
    }

    let (mut stream, sessions) = cli_handshake()?;

    let pane_id = resolve_target_pane(&sessions, target)?;

    let msg = MuxMessage::pty_input(pane_id, data);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux send-keys` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_send_keys(_target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Execute the `emterm mux ls` command.
#[cfg(unix)]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    let (_stream, sessions) = cli_handshake()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    for session in &sessions {
        println!(
            "{}: {} ({} windows, {} panes)",
            session.id, session.name, session.window_count, session.pane_count
        );
    }

    Ok(())
}

/// Execute the `emterm mux ls` command (Windows).
#[cfg(windows)]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    let (_stream, sessions) = cli_handshake()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    for session in &sessions {
        println!(
            "{}: {} ({} windows, {} panes)",
            session.id, session.name, session.window_count, session.pane_count
        );
    }

    Ok(())
}

/// Execute the `emterm mux ls` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

/// Execute the `emterm mux kill` command.
///
/// Sends a Shutdown message to the daemon via IPC, triggering graceful shutdown
/// of all PTY sessions. Falls back to socket file removal if the daemon is unreachable.
#[cfg(unix)]
pub fn execute_kill(session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    if session.is_some() {
        eprintln!(
            "Killing specific sessions is not yet supported. Use 'emterm mux kill' to kill the daemon."
        );
        return Ok(());
    }

    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    // Try to connect and send Shutdown message
    match std::os::unix::net::UnixStream::connect(&sock_path) {
        Ok(mut stream) => {
            stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;

            // Send Hello
            let hello = HelloMsg {
                client_type: ClientType::Cli,
                protocol_version: PROTOCOL_VERSION,
            };
            let hello_msg = MuxMessage::control(MessageType::Hello, 0, &hello);
            let body = hello_msg.to_frame_body();
            let len = (body.len() as u32).to_be_bytes();
            stream.write_all(&len)?;
            stream.write_all(&body)?;

            // Send Shutdown (fire-and-forget)
            let shutdown_msg = MuxMessage {
                msg_type: MessageType::Shutdown,
                pane_id: 0,
                payload: vec![],
            };
            let body = shutdown_msg.to_frame_body();
            let len = (body.len() as u32).to_be_bytes();
            stream.write_all(&len)?;
            stream.write_all(&body)?;
            stream.flush()?;

            println!("Mux daemon shutting down");
        }
        Err(_) => {
            // Daemon unreachable — clean up stale socket
            let _ = std::fs::remove_file(&sock_path);
            println!("Mux daemon not reachable (stale socket removed)");
        }
    }

    Ok(())
}

/// Execute the `emterm mux kill` command (Windows).
///
/// Sends a Shutdown message to the daemon via Named Pipe, triggering graceful shutdown
/// of all PTY sessions. Falls back to marker file removal if the daemon is unreachable.
#[cfg(windows)]
pub fn execute_kill(session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    if session.is_some() {
        eprintln!(
            "Killing specific sessions is not yet supported. Use 'emterm mux kill' to kill the daemon."
        );
        return Ok(());
    }

    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    let pipe_name = daemon::pipe_name();

    // Try to connect and send Shutdown message
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_name)
    {
        Ok(mut stream) => {
            // Send Hello
            let hello = HelloMsg {
                client_type: ClientType::Cli,
                protocol_version: PROTOCOL_VERSION,
            };
            let hello_msg = MuxMessage::control(MessageType::Hello, 0, &hello);
            let body = hello_msg.to_frame_body();
            let len = (body.len() as u32).to_be_bytes();
            stream.write_all(&len)?;
            stream.write_all(&body)?;

            // Send Shutdown (fire-and-forget)
            let shutdown_msg = MuxMessage {
                msg_type: MessageType::Shutdown,
                pane_id: 0,
                payload: vec![],
            };
            let body = shutdown_msg.to_frame_body();
            let len = (body.len() as u32).to_be_bytes();
            stream.write_all(&len)?;
            stream.write_all(&body)?;
            stream.flush()?;

            println!("Mux daemon shutting down");
        }
        Err(_) => {
            // Daemon unreachable — clean up stale marker file
            let _ = std::fs::remove_file(&sock_path);
            println!("Mux daemon not reachable (stale marker removed)");
        }
    }

    Ok(())
}

/// Execute the `emterm mux kill` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

/// Format a byte count as a human-readable size.
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

/// Open an existing regular file for truncation, refusing symlinks.
///
/// Uses `symlink_metadata` to reject symlinked targets before opening, and on
/// Unix additionally sets `O_NOFOLLOW` to close the TOCTOU window between the
/// stat and the open. `create(false)` prevents re-creating a file that was
/// deleted between the check and the open (which would otherwise leave behind
/// a zero-byte file with default umask permissions).
fn truncate_regular_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let meta = std::fs::symlink_metadata(path)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to truncate symlink",
        ));
    }
    if !ft.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).truncate(true).create(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    opts.open(path)
}

/// Execute the `emterm mux clear-logs` command.
///
/// Truncates `mux-daemon.log`, `mux-bridge.log`, and `mux-client.log` in-place.
/// Truncation (not removal) keeps open file descriptors held by the running
/// daemon/bridge valid — new log lines continue writing from offset 0.
///
/// Returns `Err` if any targeted file could not be truncated, so callers
/// (including the CLI dispatcher in `main.rs`) propagate a non-zero exit
/// status. Partial-failure details are still printed to stderr per file.
pub fn execute_clear_logs() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    let log_dir = sock_path
        .parent()
        .ok_or("Failed to resolve mux log directory")?;

    let files = ["mux-daemon.log", "mux-bridge.log", "mux-client.log"];
    let mut cleared = 0u32;
    let mut total_bytes = 0u64;
    let mut failed = 0u32;

    for name in &files {
        let path = log_dir.join(name);
        // Use symlink_metadata so a dangling/victim symlink is treated as
        // "refuse to touch" rather than being silently skipped via exists().
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("Failed to stat {}: {}", path.display(), e);
                failed += 1;
                continue;
            }
        };
        let size = meta.len();
        match truncate_regular_file(&path) {
            Ok(_) => {
                println!("Cleared: {} ({})", path.display(), format_size(size));
                cleared += 1;
                total_bytes += size;
            }
            Err(e) => {
                eprintln!("Failed to clear {}: {}", path.display(), e);
                failed += 1;
            }
        }
    }

    if cleared == 0 && failed == 0 {
        println!("No mux log files to clear in {}", log_dir.display());
    } else {
        println!(
            "Cleared {} file(s), freed {}",
            cleared,
            format_size(total_bytes)
        );
    }

    if failed > 0 {
        Err(format!("Failed to clear {} file(s)", failed).into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_nesting_not_set() {
        temp_env::with_var_unset("EMTERM_MUX", || {
            assert!(check_nesting().is_ok());
        });
    }

    #[test]
    fn test_check_nesting_set() {
        temp_env::with_var("EMTERM_MUX", Some("1"), || {
            assert!(check_nesting().is_err());
        });
    }

    // ---- send-keys target resolution tests ----

    use crate::mux::ipc::protocol::WindowInfo;

    fn make_test_sessions(windows: Vec<WindowInfo>, active_window_index: u32) -> Vec<SessionInfo> {
        vec![SessionInfo {
            id: 1,
            name: "test".to_string(),
            window_count: windows.len() as u32,
            pane_count: windows.len() as u32,
            active_window_index,
            windows,
        }]
    }

    #[test]
    fn test_resolve_target_pane_active_window() {
        let sessions = make_test_sessions(
            vec![
                WindowInfo {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: 10,
                },
                WindowInfo {
                    id: 2,
                    name: "editor".to_string(),
                    active_pane_id: 20,
                },
            ],
            1, // active window index = 1
        );
        let pane_id = resolve_target_pane(&sessions, None).unwrap();
        assert_eq!(pane_id, 20); // active window's pane
    }

    #[test]
    fn test_resolve_target_pane_explicit_index() {
        let sessions = make_test_sessions(
            vec![
                WindowInfo {
                    id: 1,
                    name: "shell".to_string(),
                    active_pane_id: 10,
                },
                WindowInfo {
                    id: 2,
                    name: "editor".to_string(),
                    active_pane_id: 20,
                },
            ],
            0,
        );
        let pane_id = resolve_target_pane(&sessions, Some(0)).unwrap();
        assert_eq!(pane_id, 10);
        let pane_id = resolve_target_pane(&sessions, Some(1)).unwrap();
        assert_eq!(pane_id, 20);
    }

    #[test]
    fn test_resolve_target_pane_out_of_range() {
        let sessions = make_test_sessions(
            vec![WindowInfo {
                id: 1,
                name: "shell".to_string(),
                active_pane_id: 10,
            }],
            0,
        );
        let err = resolve_target_pane(&sessions, Some(5)).unwrap_err();
        assert!(err.to_string().contains("out of range"));
    }

    #[test]
    fn test_resolve_target_pane_no_sessions() {
        let err = resolve_target_pane(&[], None).unwrap_err();
        assert!(err.to_string().contains("No active session"));
    }

    #[test]
    fn test_resolve_target_pane_no_active_pane() {
        let sessions = make_test_sessions(
            vec![WindowInfo {
                id: 1,
                name: "empty".to_string(),
                active_pane_id: 0,
            }],
            0,
        );
        let err = resolve_target_pane(&sessions, None).unwrap_err();
        assert!(err.to_string().contains("No active pane"));
    }
}
