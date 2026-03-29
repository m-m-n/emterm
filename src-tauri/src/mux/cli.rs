//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` -- Start/attach to default session (long-running bridge)
//! - `emterm mux --daemon` -- Run as daemon process (internal)
//! - `emterm mux attach [session]` -- Attach to existing session (long-running bridge)
//! - `emterm mux ls` -- List sessions
//! - `emterm mux kill [session]` -- Kill a session
//! - `emterm mux new [name]` -- Create a new session

use super::daemon;
use super::ipc::protocol::*;

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
                buf.timestamp_millis(),
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
fn init_bridge_logger() {
    let log_dir = daemon::socket_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("mux-bridge.log");

    if let Ok(log_file) = std::fs::File::create(&log_path) {
        use std::io::Write;

        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Info)
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .format(move |buf, record| {
                writeln!(
                    buf,
                    "{} {}[BRIDGE] {}",
                    buf.timestamp_millis(),
                    record.level(),
                    record.args()
                )
            })
            .init();
    }
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

    let sock_path = daemon::socket_path();

    if !sock_path.exists() {
        eprintln!("No mux sessions to attach to (daemon not running)");
        eprintln!("Use 'emterm mux' to start a new session.");
        return Ok(());
    }

    // Run the long-running bridge process
    run_bridge(&sock_path)?;

    Ok(())
}

/// Run the long-running bridge process.
///
/// Connects to the daemon via Unix socket, performs handshake, then
/// translates between APC on stdin/stdout and MuxMessage on the socket.
#[cfg(unix)]
fn run_bridge(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async { bridge_main_loop(sock_path).await })
}

#[cfg(not(unix))]
fn run_bridge(_sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux bridge is not supported on this platform");
    std::process::exit(1);
}

/// Set stdin to raw mode (non-canonical, no echo) so APC bytes arrive immediately.
/// Returns the original termios for restoration on exit.
#[cfg(unix)]
fn set_stdin_raw() -> Option<libc::termios> {
    use std::mem::MaybeUninit;
    unsafe {
        let mut orig = MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(libc::STDIN_FILENO, orig.as_mut_ptr()) != 0 {
            log::warn!("tcgetattr failed, stdin may not be a tty");
            return None;
        }
        let orig = orig.assume_init();
        let mut raw = orig;
        libc::cfmakeraw(&mut raw);
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            log::warn!("tcsetattr failed");
            return None;
        }
        log::info!("stdin set to raw mode");
        Some(orig)
    }
}

/// Restore original termios settings.
#[cfg(unix)]
fn restore_stdin(orig: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, orig);
    }
    log::info!("stdin restored to original mode");
}

/// Async bridge main loop: handshake, then bidirectional APC/socket forwarding.
#[cfg(unix)]
async fn bridge_main_loop(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    // Set stdin to raw mode so APC escape sequences arrive byte-by-byte
    let orig_termios = set_stdin_raw();

    // Connect to daemon
    log::info!("Connecting to daemon at {:?}", sock_path);
    let stream = UnixStream::connect(sock_path)
        .await
        .map_err(|e| format!("Failed to connect to daemon at {:?}: {}", sock_path, e))?;
    log::info!("Socket connected");

    let (mut sock_reader, mut sock_writer) = tokio::io::split(stream);

    // Perform handshake
    log::info!("Sending Hello (protocol v{})", PROTOCOL_VERSION);
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let hello_msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = hello_msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    sock_writer.write_all(&len).await?;
    sock_writer.write_all(&body).await?;
    sock_writer.flush().await?;

    // Read Welcome with 5-second timeout (handles non-eMterm terminals naturally)
    log::info!("Waiting for Welcome (5s timeout)");
    let welcome_msg = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut len_buf = [0u8; 4];
        sock_reader
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("read Welcome length: {}", e))?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len > MAX_FRAME_LENGTH {
            return Err("Frame too large during handshake".to_string());
        }
        let mut frame_buf = vec![0u8; frame_len];
        sock_reader
            .read_exact(&mut frame_buf)
            .await
            .map_err(|e| format!("read Welcome body: {}", e))?;
        MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| "Invalid Welcome frame".to_string())
    })
    .await
    .map_err(|_| "Daemon did not respond within 5 seconds")?
    .map_err(|e: String| -> Box<dyn std::error::Error> { e.into() })?;
    if welcome_msg.msg_type != MessageType::Welcome {
        log::error!("Expected Welcome, got {:?}", welcome_msg.msg_type);
        return Err(format!("Expected Welcome, got {:?}", welcome_msg.msg_type).into());
    }
    log::info!("Handshake complete, received Welcome");

    // Write Welcome as APC to stdout so GUI receives it
    let welcome_apc = welcome_msg.to_apc();
    {
        use std::io::Write;
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        stdout.write_all(welcome_apc.as_bytes())?;
        stdout.flush()?;
    }

    // Bidirectional forwarding: stdin -> daemon, daemon -> stdout
    log::info!("Starting bidirectional forwarding");
    let stdin_to_daemon = async {
        let mut stdin = tokio::io::stdin();
        let mut parser = StdinApcParser::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stdin.read(&mut buf).await {
                Ok(0) => {
                    log::info!("stdin EOF, stopping stdin→daemon");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    log::warn!("stdin read error: {}, stopping stdin→daemon", e);
                    break;
                }
            };

            let actions = parser.feed(&buf[..n]);
            for action in actions {
                match action {
                    StdinAction::MuxMessage(msg) => {
                        log::info!("stdin→daemon: forwarding {:?} pane={}", msg.msg_type, msg.pane_id);
                        let body = msg.to_frame_body();
                        let len = (body.len() as u32).to_be_bytes();
                        if sock_writer.write_all(&len).await.is_err() {
                            log::warn!("stdin→daemon: socket write failed (len)");
                            return;
                        }
                        if sock_writer.write_all(&body).await.is_err() {
                            log::warn!("stdin→daemon: socket write failed (body)");
                            return;
                        }
                        let _ = sock_writer.flush().await;
                    }
                    StdinAction::Passthrough(_) => {
                        // Non-APC data from stdin is dropped in bridge mode
                    }
                }
            }
        }
    };

    let daemon_to_stdout = async {
        let mut len_buf = [0u8; 4];
        loop {
            if let Err(e) = sock_reader.read_exact(&mut len_buf).await {
                log::info!("Daemon socket read error: {}, stopping daemon→stdout", e);
                break;
            }
            let frame_len = u32::from_be_bytes(len_buf) as usize;
            if frame_len > MAX_FRAME_LENGTH || frame_len == 0 {
                log::warn!("Invalid frame length {}, stopping daemon→stdout", frame_len);
                break;
            }
            let mut frame_buf = vec![0u8; frame_len];
            if let Err(e) = sock_reader.read_exact(&mut frame_buf).await {
                log::warn!("Daemon socket read error (body): {}, stopping daemon→stdout", e);
                break;
            }

            if let Some(msg) = MuxMessage::from_frame_body(&frame_buf) {
                let apc = msg.to_apc();
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut stdout = stdout.lock();
                if stdout.write_all(apc.as_bytes()).is_err() {
                    log::info!("stdout write error, stopping daemon→stdout");
                    break;
                }
                let _ = stdout.flush();
            } else {
                log::warn!("Invalid frame body ({} bytes), skipping", frame_len);
            }
        }
    };

    // Run both directions concurrently; exit when either ends
    tokio::select! {
        _ = stdin_to_daemon => {
            log::info!("stdin→daemon ended, shutting down bridge");
        }
        _ = daemon_to_stdout => {
            log::info!("daemon→stdout ended, shutting down bridge");
        }
    }

    // Ensure all stdout data (including final Detached APC) is flushed
    // before exiting, so the GUI receives it.
    log::info!("Flushing stdout before exit");
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    // Restore terminal settings
    if let Some(ref orig) = orig_termios {
        restore_stdin(orig);
    }

    // Brief delay so the GUI's PTY reader can consume the flushed data
    // before the PTY slave fd is closed by process exit.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Exit immediately so the host shell returns to foreground promptly.
    // Without this, tokio runtime shutdown waits for the blocked stdin
    // reader task, delaying shell prompt redraw by seconds.
    log::info!("Bridge exiting via process::exit");
    std::process::exit(0);
}

/// Actions produced by the stdin APC parser.
#[derive(Debug)]
pub enum StdinAction {
    /// A decoded mux APC message to forward to the daemon.
    MuxMessage(MuxMessage),
    /// Passthrough data (non-APC bytes).
    Passthrough(Vec<u8>),
}

/// Maximum APC payload size (matches MAX_FRAME_LENGTH after Base64 expansion).
/// Base64 expands by ~4/3, so 22MB covers the 16MB frame limit.
const MAX_APC_PAYLOAD: usize = 22 * 1024 * 1024;

/// State machine that separates APC sequences from passthrough data on stdin.
///
/// Handles partial reads across buffer boundaries.
pub struct StdinApcParser {
    state: ParserState,
    apc_buf: Vec<u8>,
    passthrough_buf: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// Normal text / passthrough mode.
    Ground,
    /// Seen ESC, waiting for _ (APC start) or \ (APC end inside accumulation).
    EscSeen,
    /// Inside APC body accumulation.
    InApc,
    /// Inside APC body, seen ESC (could be ST = ESC \).
    InApcEsc,
}

impl StdinApcParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
            apc_buf: Vec::new(),
            passthrough_buf: Vec::new(),
        }
    }

    /// Feed bytes into the parser and return resulting actions.
    pub fn feed(&mut self, data: &[u8]) -> Vec<StdinAction> {
        let mut actions = Vec::new();

        for &byte in data {
            match self.state {
                ParserState::Ground => {
                    if byte == 0x1B {
                        self.state = ParserState::EscSeen;
                    } else {
                        self.passthrough_buf.push(byte);
                    }
                }
                ParserState::EscSeen => {
                    if byte == b'_' {
                        // APC start: flush passthrough first
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.state = ParserState::InApc;
                        self.apc_buf.clear();
                    } else {
                        // Not APC start: treat ESC + byte as passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(byte);
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApc => {
                    if byte == 0x1B {
                        self.state = ParserState::InApcEsc;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        // APC payload too large: discard and reset to ground
                        eprintln!(
                            "Bridge: APC payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApcEsc => {
                    if byte == b'\\' {
                        // APC ST found: decode the APC payload
                        let payload = String::from_utf8_lossy(&self.apc_buf).to_string();
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;

                        if payload.starts_with(APC_PREFIX) {
                            match MuxMessage::from_apc(&payload) {
                                Ok(msg) => actions.push(StdinAction::MuxMessage(msg)),
                                Err(e) => {
                                    eprintln!("Bridge: APC decode error: {}", e);
                                }
                            }
                        } else {
                            // Non-mux APC: forward as passthrough (ESC_ + body + ESC\)
                            let mut pdata = Vec::with_capacity(2 + payload.len() + 2);
                            pdata.extend_from_slice(b"\x1b_");
                            pdata.extend_from_slice(payload.as_bytes());
                            pdata.extend_from_slice(b"\x1b\\");
                            actions.push(StdinAction::Passthrough(pdata));
                        }
                    } else {
                        // ESC inside APC but not followed by \: keep accumulating
                        self.apc_buf.push(0x1B);
                        self.apc_buf.push(byte);
                        self.state = ParserState::InApc;
                    }
                }
            }
        }

        // Flush remaining passthrough
        if !self.passthrough_buf.is_empty() && self.state == ParserState::Ground {
            actions.push(StdinAction::Passthrough(std::mem::take(
                &mut self.passthrough_buf,
            )));
        }

        actions
    }
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

/// Execute the `emterm mux new-window` command (Windows stub).
#[cfg(not(unix))]
pub fn execute_new_window(
    _name: Option<&str>,
    _command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
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

/// Execute the `emterm mux ls` command.
#[cfg(not(unix))]
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

/// Execute the `emterm mux kill` command.
///
/// Removes the daemon socket file to prevent new connections.
/// A proper daemon shutdown with PID tracking can be added later.
#[cfg(unix)]
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    if _session.is_some() {
        eprintln!(
            "Killing specific sessions is not yet supported. Use 'emterm mux kill' to kill the daemon."
        );
        return Ok(());
    }

    let _ = std::fs::remove_file(&sock_path);
    println!("Mux daemon socket removed. Active sessions will continue until shells exit.");
    println!("To force stop, use: pkill -f 'emterm mux --daemon'");

    Ok(())
}

/// Execute the `emterm mux kill` command.
#[cfg(not(unix))]
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    Ok(())
}

/// Import tmux.conf settings on first mux startup.
///
/// Reads the eMterm settings file directly (without AppHandle),
/// checks `tmux_conf_imported` flag, and applies tmux.conf conversions.
fn import_tmux_conf_if_needed() {
    let settings_path = match settings_file_path() {
        Some(p) => p,
        None => return,
    };

    let mut settings = if settings_path.exists() {
        match std::fs::read_to_string(&settings_path) {
            Ok(contents) => serde_json::from_str::<crate::commands::config::AppSettings>(&contents)
                .unwrap_or_default(),
            Err(_) => crate::commands::config::AppSettings::default(),
        }
    } else {
        crate::commands::config::AppSettings::default()
    };

    // Skip if already imported
    if settings.mux.tmux_conf_imported {
        return;
    }

    // Mark as imported (even if no tmux.conf exists, don't retry)
    settings.mux.tmux_conf_imported = true;

    // Try auto-import
    if let Some(result) = super::tmux_conf::converter::auto_import_tmux_conf() {
        for (key, value) in &result.settings {
            match key.as_str() {
                "prefix" => settings.mux.prefix = value.clone(),
                "base_index" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.mux.base_index = v;
                    }
                }
                "mouse" => {
                    settings.mux.mouse = value == "true";
                }
                "status_position" => {
                    settings.mux.status_position = value.clone();
                }
                k if k.starts_with("keybind.") => {
                    let bind_key = k.strip_prefix("keybind.").unwrap().to_string();
                    settings.mux.keybinds.insert(bind_key, value.clone());
                }
                _ => {}
            }
        }

        for warning in &result.warnings {
            log::warn!("tmux.conf import: {}", warning);
        }

        if !result.settings.is_empty() {
            log::info!(
                "tmux.conf: imported {} settings ({} warnings)",
                result.settings.len(),
                result.warnings.len()
            );
        }
    }

    // Save settings
    if let Some(parent) = settings_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("Failed to create config directory: {}", e);
            return;
        }
    }
    match serde_json::to_string_pretty(&settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&settings_path, json) {
                log::warn!("Failed to save settings after tmux.conf import: {}", e);
            }
        }
        Err(e) => {
            log::warn!("Failed to serialize settings: {}", e);
        }
    }
}

/// Resolve the eMterm settings file path without AppHandle.
///
/// Uses XDG_CONFIG_HOME or ~/.config as base, matching Tauri's
/// `app_config_dir()` behavior on Linux.
fn settings_file_path() -> Option<std::path::PathBuf> {
    let config_base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(
        config_base
            .join("net.laser5.app.emterm")
            .join("settings.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_nesting_not_set() {
        // In test environment, EMTERM_MUX should not be set
        unsafe { std::env::remove_var("EMTERM_MUX") };
        assert!(check_nesting().is_ok());
    }

    #[test]
    fn test_check_nesting_set() {
        unsafe { std::env::set_var("EMTERM_MUX", "1") };
        assert!(check_nesting().is_err());
        unsafe { std::env::remove_var("EMTERM_MUX") };
    }

    #[test]
    fn test_settings_file_path_with_home() {
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("HOME", "/tmp/test_home") };
        let path = settings_file_path().unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/test_home/.config/net.laser5.app.emterm/settings.json")
        );
    }

    #[test]
    fn test_settings_file_path_with_xdg() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_config") };
        let path = settings_file_path().unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/xdg_config/net.laser5.app.emterm/settings.json")
        );
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    // ---- StdinApcParser tests ----

    #[test]
    fn test_stdin_parser_passthrough_only() {
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(b"hello world");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => assert_eq!(data, b"hello world"),
            _ => panic!("Expected Passthrough"),
        }
    }

    #[test]
    fn test_stdin_parser_apc_mux_message() {
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let apc = msg.to_apc();
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(apc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_passthrough_then_apc() {
        let msg = MuxMessage::pty_input(1, vec![0x41]);
        let apc = msg.to_apc();
        let mut input = b"before".to_vec();
        input.extend_from_slice(apc.as_bytes());
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        assert_eq!(actions.len(), 2);
        match &actions[0] {
            StdinAction::Passthrough(data) => assert_eq!(data, b"before"),
            _ => panic!("Expected Passthrough"),
        }
        match &actions[1] {
            StdinAction::MuxMessage(decoded) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_split_across_boundaries() {
        let msg = MuxMessage::pty_input(5, vec![0xFF]);
        let apc = msg.to_apc();
        let bytes = apc.as_bytes();
        let mid = bytes.len() / 2;

        let mut parser = StdinApcParser::new();

        // Feed first half
        let actions1 = parser.feed(&bytes[..mid]);
        // Should not produce MuxMessage yet (APC not complete)
        for a in &actions1 {
            assert!(
                !matches!(a, StdinAction::MuxMessage(_)),
                "Should not decode incomplete APC"
            );
        }

        // Feed second half
        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_)));
        assert!(has_msg, "Should decode APC after second half");
    }

    #[test]
    fn test_stdin_parser_non_mux_apc() {
        // A non-mux APC sequence (e.g., Kitty graphics) should be passed through
        let input = b"\x1b_Gf=32;data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => {
                assert_eq!(data, input);
            }
            _ => panic!("Expected Passthrough for non-mux APC"),
        }
    }

    #[test]
    fn test_stdin_parser_esc_not_apc() {
        // ESC followed by something other than _ should be passthrough
        let input = b"\x1b[31m";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::Passthrough(data) => {
                assert_eq!(data, &b"\x1b[31m".to_vec());
            }
            _ => panic!("Expected Passthrough"),
        }
    }

    #[test]
    fn test_stdin_parser_multiple_apc_in_one_feed() {
        let msg1 = MuxMessage::pty_input(1, vec![0x01]);
        let msg2 = MuxMessage::pty_input(2, vec![0x02]);
        let mut input = msg1.to_apc().into_bytes();
        input.extend_from_slice(msg2.to_apc().as_bytes());

        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].pane_id, 1);
        assert_eq!(msgs[1].pane_id, 2);
    }

    #[test]
    fn test_stdin_parser_empty_input() {
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(b"");
        assert!(actions.is_empty());
    }

    #[test]
    fn test_stdin_parser_esc_inside_apc_not_st() {
        // ESC inside APC body but not followed by \ should continue accumulation
        // Use a crafted payload that has ESC followed by a non-\ byte
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b_emterm-mux;");
        input.push(0x1B); // ESC
        input.push(b'X'); // Not \, so should be added to APC buf
        // Now send the real ST
        // But since the APC content is now "emterm-mux;\x1bX", it won't decode correctly
        // That's fine - we test the parser state machine, not decode success
        input.extend_from_slice(b"\x1b\\");
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        // Should get 1 action (either MuxMessage error printed to stderr, or decoded)
        // The APC body is "emterm-mux;\x1bX" which is invalid base64
        assert_eq!(actions.len(), 0); // decode error is printed, no action produced
    }
}
