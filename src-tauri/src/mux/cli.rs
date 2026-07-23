//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` -- Start/attach to default session (long-running bridge)
//! - `emterm mux --daemon` -- Run as daemon process (internal)
//! - `emterm mux attach [session]` -- Attach to existing session (long-running bridge)
//! - `emterm mux ls` -- List sessions
//! - `emterm mux kill [session]` -- Kill a session
//! - `emterm mux new [name]` -- Create a new session
//!
//! `run` is the single entry point invoked from `main.rs`; it owns all
//! subcommand-argument parsing for the mux CLI so the binary entry point
//! stays a one-liner.

use serde::{Deserialize, Serialize};

use super::bridge::run_bridge;
use super::daemon;
use super::ipc::protocol::*;
// `tmux_import` writes to GUI-only `settings_store`, so it is reachable only
// in the GUI build. The mux-only deb (`emterm-mux`) is intended for
// headless SSH hosts where `settings.json` is hand-managed; auto-importing
// `~/.tmux.conf` into it is meaningless there.
#[cfg(feature = "gui")]
use super::tmux_import::import_tmux_conf_if_needed;

/// Dispatch `emterm mux …` subcommands. `args` is the slice that follows
/// the literal `mux` token (so `args[0]` is the next positional, e.g.
/// `attach` / `--daemon` / `ls`). Returns the desired process exit code.
///
/// All argument-parsing logic for the mux CLI lives here, mirroring the
/// `emterm::cli::run` pattern used for `markdown` / `json` / `yaml` /
/// `image`. Adding a new mux subcommand means editing this file only;
/// `main.rs` just forwards.
pub fn run(args: &[String]) -> i32 {
    // `--daemon` is recognised both as a bare flag and as a "subcommand"
    // alongside the positional ones, matching the legacy clap surface.
    let mut daemon_mode = false;
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if a == "--daemon" {
            daemon_mode = true;
        } else {
            positional.push(a.as_str());
        }
    }

    if daemon_mode {
        return match execute_daemon() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Daemon error: {e}");
                1
            }
        };
    }

    let sub = positional.first().copied();
    let rest: Vec<&str> = positional.iter().copied().skip(1).collect();

    let result: Result<(), Box<dyn std::error::Error>> = match sub {
        Some("ls") => execute_ls(),
        Some("kill") => execute_kill(rest.first().copied()),
        Some("attach") => execute_attach(rest.first().copied()),
        Some("script") => execute_script(),
        Some("clear-logs") => execute_clear_logs(),
        Some("new-window") => {
            let mut name: Option<&str> = None;
            let mut command: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-n" | "--name" => name = iter.next(),
                    "-c" | "--command" => command = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `new-window`: {other}");
                        return 2;
                    }
                }
            }
            execute_new_window(name, command)
        }
        Some("switch-window") => {
            let Some(arg) = rest.first().copied() else {
                eprintln!("Error: `switch-window` requires a 0-based window index");
                return 2;
            };
            let idx: u32 = match arg.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("Error: window index must be a non-negative integer (got {arg:?})");
                    return 2;
                }
            };
            execute_switch_window(idx)
        }
        Some("send-keys") => {
            let mut target: Option<u32> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-t" | "--target" => {
                        let Some(v) = iter.next() else {
                            eprintln!("Error: `--target` requires a window index");
                            return 2;
                        };
                        match v.parse() {
                            Ok(n) => target = Some(n),
                            Err(_) => {
                                eprintln!(
                                    "Error: target must be a non-negative integer (got {v:?})"
                                );
                                return 2;
                            }
                        }
                    }
                    other => {
                        eprintln!("Error: unknown argument to `send-keys`: {other}");
                        return 2;
                    }
                }
            }
            execute_send_keys(target)
        }
        Some("read") => {
            let mut pane: Option<&str> = None;
            let mut lines: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--lines" => lines = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `read`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `read` requires --pane <id|current>");
                return 2;
            };
            return execute_mux_read(pane, lines);
        }
        Some("send") => {
            let mut pane: Option<&str> = None;
            let mut text: Option<&str> = None;
            let mut use_stdin = false;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--text" => text = iter.next(),
                    "--stdin" => use_stdin = true,
                    other => {
                        eprintln!("Error: unknown argument to `send`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `send` requires --pane <id|current>");
                return 2;
            };
            if text.is_some() == use_stdin {
                eprintln!("Error: `send` requires exactly one of --text <s> or --stdin");
                return 2;
            }
            return execute_mux_send(pane, text, use_stdin);
        }
        Some("wait") => {
            let mut pane: Option<&str> = None;
            let mut state: Option<&str> = None;
            let mut timeout: Option<&str> = None;
            let mut after: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--state" => state = iter.next(),
                    "--timeout" => timeout = iter.next(),
                    "--after" => after = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `wait`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `wait` requires --pane <id|current>");
                return 2;
            };
            let Some(state) = state else {
                eprintln!("Error: `wait` requires --state <set> (comma-separated)");
                return 2;
            };
            return execute_mux_wait(pane, state, timeout, after);
        }
        Some(other) => {
            eprintln!("Error: unknown `mux` subcommand: {other}");
            eprintln!(
                "Available: ls / kill [session] / attach [session] / new-window / script / \
                 switch-window <index> / send-keys / read / send / wait / clear-logs / \
                 (no subcommand = start session)"
            );
            return 2;
        }
        None => execute_mux(),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

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

    // Auto-import tmux.conf on first mux startup (GUI builds only, because
    // the importer writes to GUI-only `settings_store`). Mux-only builds run
    // on headless hosts where `settings.json` is hand-managed.
    #[cfg(feature = "gui")]
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

// ============================================================================
// Agent-facing API: `emterm mux read|send|wait` (task0004)
//
// Exit codes per IMPLEMENTATION.md "Conventions": 0 success; 2 usage/invalid
// input; 3 wait timeout (dedicated); 4 unknown pane / pane gone; 5
// not_mux_pane; 1 all other errors (connection failure etc.).
// ============================================================================

/// Default `--lines` for `emterm mux read` when omitted.
const READ_DEFAULT_LINES: u32 = 100;
/// Default `--timeout` (seconds) for `emterm mux wait` when omitted.
const WAIT_DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Margin added to the client-side read timeout above the daemon's own
/// `timeout_ms` for `wait`, so the client never gives up before the daemon
/// replies with its own (bounded) timeout error.
const WAIT_CLIENT_TIMEOUT_MARGIN_MS: u64 = 5000;

/// Map an `AgentApiErrorKind` to the CLI exit-code convention
/// (IMPLEMENTATION.md "Conventions"). Pure — testable without a live daemon
/// (AC-7).
fn agent_api_error_exit_code(kind: AgentApiErrorKind) -> i32 {
    match kind {
        AgentApiErrorKind::InvalidInput => 2,
        AgentApiErrorKind::Timeout => 3,
        AgentApiErrorKind::UnknownPane | AgentApiErrorKind::PaneGone => 4,
        AgentApiErrorKind::NotMuxPane => 5,
    }
}

/// Resolve a `--pane` argument: `"current"` resolves from `EMTERM_PANE_ID`
/// (a missing variable is a usage error per FR13); any other value passes
/// through verbatim as the public pane ID. Pure aside from the env read —
/// testable via `temp_env` (AC-8).
fn resolve_pane_arg(pane_arg: &str) -> Result<String, String> {
    if pane_arg == "current" {
        std::env::var("EMTERM_PANE_ID")
            .map_err(|_| "EMTERM_PANE_ID is not set (required for --pane current)".to_string())
    } else {
        Ok(pane_arg.to_string())
    }
}

fn parse_agent_state_str(s: &str) -> Option<AgentState> {
    match s {
        "idle" => Some(AgentState::Idle),
        "working" => Some(AgentState::Working),
        "blocked" => Some(AgentState::Blocked),
        "done" => Some(AgentState::Done),
        _ => None,
    }
}

/// Parse a comma-separated `--state` value (e.g. `"done,blocked"`) into the
/// wire `Vec<AgentState>`. Pure — testable without a live daemon.
fn parse_agent_states(s: &str) -> Result<Vec<AgentState>, String> {
    let mut states = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("invalid --state value: {s:?}"));
        }
        match parse_agent_state_str(part) {
            Some(state) => states.push(state),
            None => {
                return Err(format!(
                    "unknown state {part:?} (expected idle|working|blocked|done)"
                ));
            }
        }
    }
    if states.is_empty() {
        return Err("`--state` requires at least one state".to_string());
    }
    Ok(states)
}

/// Send a bincode-encoded control request and read back one framed
/// response. Generic over the platform-specific stream type returned by
/// `cli_handshake()` (`UnixStream` / Windows `File`), both `Read + Write`.
fn send_agent_request<S, T>(
    stream: &mut S,
    msg_type: MessageType,
    payload: &T,
) -> Result<MuxMessage, Box<dyn std::error::Error>>
where
    S: std::io::Read + std::io::Write,
    T: Serialize,
{
    let msg = MuxMessage::control(msg_type, 0, payload);
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
    MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| "Invalid frame".into())
}

/// Decode `resp` as either the expected success payload `T` (`on_success`,
/// exit 0) or a shared `AgentApiErrorMsg` (mapped via
/// [`agent_api_error_exit_code`]). Any other response shape is an exit-1
/// protocol error.
fn handle_agent_response<T: for<'a> Deserialize<'a>>(
    resp: MuxMessage,
    expected: MessageType,
    on_success: impl FnOnce(T),
) -> i32 {
    if resp.msg_type == expected {
        match resp.decode_payload::<T>() {
            Some(v) => {
                on_success(v);
                0
            }
            None => {
                eprintln!("Error: malformed response payload");
                1
            }
        }
    } else if resp.msg_type == MessageType::AgentApiError {
        match resp.decode_payload::<AgentApiErrorMsg>() {
            Some(err) => {
                eprintln!("Error: {}", err.message);
                agent_api_error_exit_code(err.kind)
            }
            None => {
                eprintln!("Error: malformed error response");
                1
            }
        }
    } else {
        eprintln!("Error: unexpected response type {:?}", resp.msg_type);
        1
    }
}

/// Execute `emterm mux read --pane <id|current> [--lines N]` (FR10).
pub fn execute_mux_read(pane_arg: &str, lines_arg: Option<&str>) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let lines: u32 = match lines_arg {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("Error: --lines must be a non-negative integer (got {s:?})");
                return 2;
            }
        },
        None => READ_DEFAULT_LINES,
    };

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let req = ReadPaneMsg {
        public_pane_id,
        lines,
    };
    let resp = match send_agent_request(&mut stream, MessageType::ReadPane, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<ReadPaneResultMsg>(resp, MessageType::ReadPaneResult, |r| {
        println!("{}", r.text);
    })
}

/// Execute `emterm mux send --pane <id|current> (--text <s> | --stdin)`
/// (FR11). Argument mutual-exclusivity is validated by the caller (`run`).
pub fn execute_mux_send(pane_arg: &str, text_arg: Option<&str>, use_stdin: bool) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };

    let bytes: Vec<u8> = if use_stdin {
        use std::io::Read;
        let mut buf = Vec::new();
        if let Err(e) = std::io::stdin().read_to_end(&mut buf) {
            eprintln!("Error: failed to read stdin: {e}");
            return 1;
        }
        buf
    } else {
        // `run` guarantees `text_arg.is_some()` when `!use_stdin`.
        text_arg.unwrap_or_default().as_bytes().to_vec()
    };

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let req = SendTextMsg {
        public_pane_id,
        bytes,
    };
    let resp = match send_agent_request(&mut stream, MessageType::SendText, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<SendTextResultMsg>(resp, MessageType::SendTextResult, |r| {
        println!("revision_watermark={}", r.revision_watermark);
    })
}

/// Execute `emterm mux wait --pane <id|current> --state <set> [--timeout
/// <sec>] [--after <revision>]` (FR12).
pub fn execute_mux_wait(
    pane_arg: &str,
    states_arg: &str,
    timeout_arg: Option<&str>,
    after_arg: Option<&str>,
) -> i32 {
    let public_pane_id = match resolve_pane_arg(pane_arg) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let states = match parse_agent_states(states_arg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return 2;
        }
    };
    let timeout_secs: u64 = match timeout_arg {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!(
                    "Error: --timeout must be a non-negative integer number of seconds (got {s:?})"
                );
                return 2;
            }
        },
        None => WAIT_DEFAULT_TIMEOUT_SECS,
    };
    let after_revision: Option<u64> = match after_arg {
        Some(s) => match s.parse() {
            Ok(n) => Some(n),
            Err(_) => {
                eprintln!("Error: --after must be a non-negative integer revision (got {s:?})");
                return 2;
            }
        },
        None => None,
    };
    let timeout_ms = timeout_secs.saturating_mul(1000);

    let (mut stream, _sessions) = match cli_handshake() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    // Extend the client-side read timeout beyond the daemon's own wait
    // timeout so we don't give up before it replies (Unix only: the
    // Windows Named Pipe `File` handle has no read-timeout API, and blocks
    // indefinitely by default — which is already bounded by the daemon's
    // own `timeout_ms`).
    #[cfg(unix)]
    {
        if let Err(e) = stream.set_read_timeout(Some(std::time::Duration::from_millis(
            timeout_ms + WAIT_CLIENT_TIMEOUT_MARGIN_MS,
        ))) {
            eprintln!("Error: {e}");
            return 1;
        }
    }

    let req = WaitAgentStateMsg {
        public_pane_id,
        states,
        timeout_ms,
        after_revision,
    };
    let resp = match send_agent_request(&mut stream, MessageType::WaitAgentState, &req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    handle_agent_response::<WaitAgentStateResultMsg>(resp, MessageType::WaitAgentStateResult, |r| {
        println!("state={:?} revision={}", r.state, r.revision);
    })
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

    // ---- Agent API (read/send/wait): exit-code mapping and --pane current
    // resolution, per Test Notes "no live daemon needed" (AC-7, AC-8) ----

    #[test]
    fn agent_api_error_exit_code_matches_convention() {
        assert_eq!(
            agent_api_error_exit_code(AgentApiErrorKind::InvalidInput),
            2
        );
        assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::Timeout), 3);
        assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::UnknownPane), 4);
        assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::PaneGone), 4);
        assert_eq!(agent_api_error_exit_code(AgentApiErrorKind::NotMuxPane), 5);
    }

    #[test]
    fn resolve_pane_arg_passes_through_explicit_id() {
        assert_eq!(
            resolve_pane_arg("abc123-7").unwrap(),
            "abc123-7".to_string()
        );
    }

    #[test]
    fn resolve_pane_arg_current_resolves_from_env() {
        temp_env::with_var("EMTERM_PANE_ID", Some("deadbeef-3"), || {
            assert_eq!(resolve_pane_arg("current").unwrap(), "deadbeef-3");
        });
    }

    #[test]
    fn resolve_pane_arg_current_missing_env_is_usage_error() {
        temp_env::with_var_unset("EMTERM_PANE_ID", || {
            let err = resolve_pane_arg("current").unwrap_err();
            assert!(err.contains("EMTERM_PANE_ID"));
        });
    }

    #[test]
    fn parse_agent_states_single_and_multiple() {
        assert_eq!(parse_agent_states("done").unwrap(), vec![AgentState::Done]);
        assert_eq!(
            parse_agent_states("done,blocked").unwrap(),
            vec![AgentState::Done, AgentState::Blocked]
        );
        assert_eq!(
            parse_agent_states(" idle , working ").unwrap(),
            vec![AgentState::Idle, AgentState::Working]
        );
    }

    #[test]
    fn parse_agent_states_rejects_unknown_state() {
        assert!(parse_agent_states("bogus").is_err());
        assert!(parse_agent_states("done,bogus").is_err());
    }

    #[test]
    fn parse_agent_states_rejects_empty() {
        assert!(parse_agent_states("").is_err());
        assert!(parse_agent_states(",").is_err());
    }
}
