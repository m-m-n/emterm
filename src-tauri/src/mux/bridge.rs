//! Bridge process: stdin/stdout APC ↔ daemon socket forwarding.
//!
//! The bridge translates between APC escape sequences on stdin/stdout
//! and MuxMessage frames on the Unix domain socket.

use super::ipc::protocol::*;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[cfg(unix)]
use tokio::net::UnixStream;

/// Global storage for original termios, so we can restore it before process::exit().
#[cfg(unix)]
static ORIGINAL_TERMIOS: std::sync::OnceLock<libc::termios> = std::sync::OnceLock::new();

/// Restore stdin from the global original termios (safe to call from any context).
#[cfg(unix)]
fn restore_stdin_global() {
    if let Some(orig) = ORIGINAL_TERMIOS.get() {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, orig);
        }
        log::info!("stdin restored from global termios");
    }
}

/// Transport format for mux messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Transport {
    /// APC format (default, works on Linux).
    Apc = 0,
    /// OSC 9999 format (fallback for Windows ConPTY output direction).
    Osc = 1,
    /// Plaintext format (Windows ConPTY input direction: `EMUX;<base64>\r`;
    /// parser also accepts LF / CRLF / LFCR).
    Plaintext = 2,
}

/// Sentinel value indicating transport has not been detected yet.
const TRANSPORT_UNDETECTED: u8 = 0xFF;

/// Run the long-running bridge process.
///
/// Connects to the daemon via Unix socket, performs handshake, then
/// translates between APC on stdin/stdout and MuxMessage on the socket.
#[cfg(unix)]
pub(super) fn run_bridge(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async { bridge_main_loop(sock_path).await })
}

/// Run the long-running bridge process on Windows via Named Pipe.
#[cfg(windows)]
pub(super) fn run_bridge(_sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async { bridge_main_loop_windows().await })
}

/// Run the long-running bridge process (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub(super) fn run_bridge(_sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
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
        // Store in global so process::exit() path can restore it
        let _ = ORIGINAL_TERMIOS.set(orig);
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

/// RAII guard that restores terminal settings on drop.
#[cfg(unix)]
struct RawModeGuard(Option<libc::termios>);

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Some(ref orig) = self.0 {
            restore_stdin(orig);
        }
    }
}

/// Global storage for original console mode (Windows).
#[cfg(windows)]
static ORIGINAL_CONSOLE_MODE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

/// Restore stdin from the global console mode (Windows).
#[cfg(windows)]
fn restore_stdin_windows_global() {
    if let Some(&mode) = ORIGINAL_CONSOLE_MODE.get() {
        restore_stdin_windows(mode);
        log::info!("stdin restored from global console mode");
    }
}

/// Set stdin to raw mode on Windows (enable VT input processing).
/// Returns the original console mode for restoration on exit.
#[cfg(windows)]
fn set_stdin_raw_windows() -> Option<u32> {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE as _ {
            log::warn!("GetStdHandle failed, stdin may not be a console");
            return None;
        }
        let mut original_mode: u32 = 0;
        if GetConsoleMode(handle, &mut original_mode) == 0 {
            log::warn!("GetConsoleMode failed, stdin may not be a console");
            return None;
        }
        // Store in global so process::exit() path can restore it
        let _ = ORIGINAL_CONSOLE_MODE.set(original_mode);
        if SetConsoleMode(handle, ENABLE_VIRTUAL_TERMINAL_INPUT) == 0 {
            log::warn!("SetConsoleMode failed");
            return None;
        }
        log::info!("stdin set to raw mode (VT input)");
        Some(original_mode)
    }
}

/// Restore original console mode on Windows.
#[cfg(windows)]
fn restore_stdin_windows(original_mode: u32) {
    use windows_sys::Win32::System::Console::*;
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        SetConsoleMode(handle, original_mode);
    }
    log::info!("stdin restored to original mode");
}

/// RAII guard that restores console mode on drop (Windows).
#[cfg(windows)]
struct RawModeGuardWindows(Option<u32>);

#[cfg(windows)]
impl Drop for RawModeGuardWindows {
    fn drop(&mut self) {
        if let Some(mode) = self.0 {
            restore_stdin_windows(mode);
        }
    }
}

/// Async bridge main loop (Unix): connect via Unix socket, then forward.
///
/// After the daemon sends the upgrade announcement (`MessageType::Upgrading`)
/// and the connection then drops, this loop reconnects instead of exiting
/// (IMPLEMENTATION.md D2, task0006 AC-2/AC-4/AC-5/AC-6). Without the
/// announcement, a dropped connection ends the bridge exactly as before
/// (AC-3), and a fresh connection that later drops without a NEW
/// announcement also ends the bridge as before (AC-7) — see
/// `forward_loop`'s per-call `announced` flag.
///
/// The stdin handle (and its parser) are created ONCE here, before the
/// reconnect loop, and lent to every `forward_loop` call by mutable
/// reference (task0010 AC-1/AC-2/AC-3): a reconnect never spins up a
/// second `tokio::io::stdin()`, so a read left in flight when a connection
/// drops is not abandoned — it is still being awaited by the SAME handle
/// once the next connection's `forward_loop` call resumes reading from it,
/// and any bytes it yields are delivered to the daemon after the reconnect
/// instead of being discarded with a throwaway handle.
#[cfg(unix)]
async fn bridge_main_loop(sock_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Connecting to daemon at {:?}", sock_path);
    let stream = UnixStream::connect(sock_path)
        .await
        .map_err(|e| format!("Failed to connect to daemon at {:?}: {}", sock_path, e))?;
    log::info!("Socket connected");

    let _raw_guard = RawModeGuard(set_stdin_raw());

    let (mut sock_reader, mut sock_writer) = tokio::io::split(stream);
    let welcome_msg = perform_handshake(&mut sock_reader, &mut sock_writer).await?;
    write_welcome_to_stdout(&welcome_msg)?;

    let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

    // Owned here, for the lifetime of the whole bridge run, so a reconnect
    // never creates a second stdin handle or a second parser (task0010
    // AC-1).
    let mut stdin = tokio::io::stdin();
    let mut stdin_parser = StdinApcParser::new();

    loop {
        let ended = forward_loop(
            &mut sock_reader,
            &mut sock_writer,
            &transport,
            &last_attach,
            &mut stdin,
            &mut stdin_parser,
        )
        .await;
        match ended {
            ConnectionEnded::Normal => break,
            ConnectionEnded::Announced => {
                match reconnect_and_reattach(sock_path, &last_attach).await {
                    Some(new_stream) => {
                        let (r, w) = tokio::io::split(new_stream);
                        sock_reader = r;
                        sock_writer = w;
                        continue;
                    }
                    None => {
                        log::warn!(
                            "mux bridge: reconnect window exhausted after upgrade \
                             announcement, exiting"
                        );
                        break;
                    }
                }
            }
        }
    }

    finish_bridge_exit(&transport)
}

/// Async bridge main loop (Windows): connect via Named Pipe, then forward.
///
/// Reconnect-after-upgrade-announcement is Unix-only (every upgrade-related
/// code path is Unix-only, since the hot-upgrade daemon itself is Unix-only);
/// a dropped connection here always ends the bridge, matching today's
/// behaviour regardless of what `forward_loop` reports.
#[cfg(windows)]
async fn bridge_main_loop_windows() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = super::daemon::pipe_name();
    log::info!("Connecting to daemon at {}", pipe_name);
    let stream = ClientOptions::new().open(&pipe_name)?;
    log::info!("Pipe connected");

    let _raw_guard = RawModeGuardWindows(set_stdin_raw_windows());

    let (mut sock_reader, mut sock_writer) = tokio::io::split(stream);
    let welcome_msg = perform_handshake(&mut sock_reader, &mut sock_writer).await?;
    write_welcome_to_stdout(&welcome_msg)?;

    let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));
    let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

    // Windows never reconnects (see the doc comment above), but the stdin
    // handle and parser are still created here rather than inside
    // `forward_loop`, keeping the single call site consistent with Unix.
    let mut stdin = tokio::io::stdin();
    let mut stdin_parser = StdinApcParser::new();

    let _ended = forward_loop(
        &mut sock_reader,
        &mut sock_writer,
        &transport,
        &last_attach,
        &mut stdin,
        &mut stdin_parser,
    )
    .await;

    finish_bridge_exit(&transport)
}

/// Send Hello and wait for Welcome (5s timeout), returning the decoded
/// Welcome frame. Shared by the initial connection and every Unix
/// reconnect attempt (IMPLEMENTATION.md Shared Components: "retry
/// connect-and-handshake").
async fn perform_handshake<R, W>(
    sock_reader: &mut R,
    sock_writer: &mut W,
) -> Result<MuxMessage, Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
    Ok(welcome_msg)
}

/// Write the Welcome frame to stdout as both OSC and APC so the GUI
/// receives it regardless of which transport the terminal supports. OSC is
/// sent FIRST because Windows ConPTY may corrupt stream state when
/// encountering APC (ESC _), potentially consuming subsequent data.
fn write_welcome_to_stdout(welcome_msg: &MuxMessage) -> std::io::Result<()> {
    use std::io::Write;
    let welcome_osc = welcome_msg.to_osc();
    let welcome_apc = welcome_msg.to_apc();
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(welcome_osc.as_bytes())?;
    stdout.write_all(welcome_apc.as_bytes())?;
    stdout.flush()
}

/// What the bridge does with one decoded daemon→client frame.
#[derive(Debug)]
enum DaemonFrameEffect {
    /// Forward this message to the terminal output path (today's
    /// behaviour, unchanged).
    Forward(MuxMessage),
    /// `MessageType::Upgrading` arrived: the announcement carries no
    /// payload and is never forwarded to the terminal output path (AC-1);
    /// its arrival only arms the reconnect loop.
    Announced,
    /// The frame body did not decode to a known message type.
    Ignored,
}

/// Classify one daemon frame body. Pure and side-effect free so the AC-1
/// guarantee (the announcement never reaches the terminal output path) is
/// directly testable without a live socket or stdout.
fn decide_daemon_frame_effect(frame_buf: &[u8]) -> DaemonFrameEffect {
    match MuxMessage::from_frame_body(frame_buf) {
        None => DaemonFrameEffect::Ignored,
        Some(msg) if msg.msg_type == MessageType::Upgrading => DaemonFrameEffect::Announced,
        Some(msg) => DaemonFrameEffect::Forward(msg),
    }
}

/// Returns the raw frame body to remember for a later reconnect's
/// re-attach (AC-4) when `msg` is an `Attach` request; `None` otherwise.
fn capture_if_attach(msg: &MuxMessage, body: &[u8]) -> Option<Vec<u8>> {
    if msg.msg_type == MessageType::Attach {
        Some(body.to_vec())
    } else {
        None
    }
}

/// Why one daemon connection's forwarding loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionEnded {
    /// The connection dropped without ever seeing `MessageType::Upgrading`
    /// on it: exit exactly as today (AC-3).
    Normal,
    /// The connection dropped after `MessageType::Upgrading` arrived on
    /// it: the caller (Unix only) should attempt to reconnect (AC-2).
    Announced,
}

fn conclude_connection(announced: bool) -> ConnectionEnded {
    if announced {
        ConnectionEnded::Announced
    } else {
        ConnectionEnded::Normal
    }
}

/// Bidirectional forwarding for one daemon connection: stdin -> daemon,
/// daemon -> stdout. Ends when either direction's stream fails, and
/// reports whether the upgrade announcement (`MessageType::Upgrading`)
/// arrived on THIS connection first (`ConnectionEnded::Announced`) or not
/// (`::Normal`) — see `conclude_connection`. A fresh `announced` flag is
/// created on every call, so a reconnected connection that sees no NEW
/// announcement concludes `Normal` even though an earlier connection
/// concluded `Announced` (AC-7).
///
/// `stdin` and `parser` are owned by the CALLER and lent by mutable
/// reference (task0010 AC-1): every call across a bridge's reconnects
/// reads from the same stdin handle and feeds the same parser, instead of
/// each call creating its own. Reading from `tokio::io::stdin()` is not
/// cancellation-safe — a read left in flight when this function returns
/// (because the daemon side ended first) keeps running in the background
/// and would silently discard whatever it eventually reads if the handle
/// were dropped; by keeping the same handle alive across calls, that read
/// is instead picked back up the next time the caller invokes
/// `forward_loop`, so bytes typed while a connection is dropping still
/// reach the daemon after the reconnect (AC-2) and no second handle is
/// ever created to strand a read against (AC-3).
///
/// Does not perform the synthetic-Detached-then-exit sequence; callers
/// decide that (Unix: only after giving up on reconnecting; Windows:
/// always) via `finish_bridge_exit`.
async fn forward_loop<R, W, I>(
    sock_reader: &mut R,
    sock_writer: &mut W,
    transport: &Arc<AtomicU8>,
    last_attach: &Arc<Mutex<Option<Vec<u8>>>>,
    stdin: &mut I,
    parser: &mut StdinApcParser,
) -> ConnectionEnded
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
    I: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    log::info!("Starting bidirectional forwarding");

    // Start with undetected transport. While undetected, send both OSC and APC.
    // Once the GUI sends its first message, lock to the detected transport.
    let announced = Arc::new(AtomicBool::new(false));
    let announced_for_stdout = Arc::clone(&announced);
    let transport_for_stdin = Arc::clone(transport);
    let transport_for_stdout = Arc::clone(transport);
    let last_attach_for_stdin = Arc::clone(last_attach);

    let stdin_to_daemon = async {
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
                    StdinAction::MuxMessage(msg, t) => {
                        // Set transport on first detected message
                        let _ = transport_for_stdin.compare_exchange(
                            TRANSPORT_UNDETECTED,
                            t as u8,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        );
                        log::info!(
                            "stdin→daemon: forwarding {:?} pane={} transport={:?}",
                            msg.msg_type,
                            msg.pane_id,
                            t
                        );
                        let body = msg.to_frame_body();
                        if let Some(captured) = capture_if_attach(&msg, &body) {
                            *last_attach_for_stdin
                                .lock()
                                .expect("last_attach mutex poisoned") = Some(captured);
                        }
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
                        // Non-mux data from stdin is dropped in bridge mode
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
                log::warn!(
                    "Daemon socket read error (body): {}, stopping daemon→stdout",
                    e
                );
                break;
            }

            match decide_daemon_frame_effect(&frame_buf) {
                DaemonFrameEffect::Ignored => {
                    log::warn!("Invalid frame body ({} bytes), skipping", frame_len);
                }
                DaemonFrameEffect::Announced => {
                    log::info!("mux bridge: received upgrade announcement, arming reconnect loop");
                    announced_for_stdout.store(true, Ordering::Release);
                }
                DaemonFrameEffect::Forward(msg) => {
                    log::info!(
                        "daemon→stdout: {:?} pane={} ({} bytes)",
                        msg.msg_type,
                        msg.pane_id,
                        msg.payload.len()
                    );
                    // OSC-probe (temporary): flag when a PtyOutput carries an
                    // `emterm` viewer-launch OSC 777, so we can compare against
                    // the daemon-side probe (pty_spawn.rs) and the GUI-side probe
                    // (tabs.rs) to locate where the sequence is lost. Only
                    // metadata is logged (never the payload bytes) so this probe
                    // cannot leak user file content into persisted release logs.
                    if msg.msg_type == MessageType::PtyOutput {
                        const OSC_PROBE_NEEDLE: &[u8] = b"\x1b]777;emterm;";
                        if let Some(off) = msg
                            .payload
                            .windows(OSC_PROBE_NEEDLE.len())
                            .position(|w| w == OSC_PROBE_NEEDLE)
                        {
                            log::warn!(
                                "[osc-probe bridge] pane={} payload_len={} osc_off={}",
                                msg.pane_id,
                                msg.payload.len(),
                                off,
                            );
                        }
                    }
                    let t = transport_for_stdout.load(Ordering::Relaxed);
                    use std::io::Write;
                    let stdout = std::io::stdout();
                    let mut stdout = stdout.lock();
                    if t == TRANSPORT_UNDETECTED {
                        // Transport not yet detected: send both so at least one arrives.
                        let osc = msg.to_osc();
                        let apc = msg.to_apc();
                        if stdout.write_all(osc.as_bytes()).is_err()
                            || stdout.write_all(apc.as_bytes()).is_err()
                        {
                            log::info!("stdout write error, stopping daemon→stdout");
                            break;
                        }
                    } else {
                        // Plaintext input means Windows ConPTY: use OSC for output
                        let encoded =
                            if t == Transport::Osc as u8 || t == Transport::Plaintext as u8 {
                                msg.to_osc()
                            } else {
                                msg.to_apc()
                            };
                        if stdout.write_all(encoded.as_bytes()).is_err() {
                            log::info!("stdout write error, stopping daemon→stdout");
                            break;
                        }
                    }
                    let _ = stdout.flush();
                }
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

    conclude_connection(announced.load(Ordering::Acquire))
}

/// Write the synthetic Detached message, flush, restore the terminal, and
/// exit the process — today's unconditional end-of-bridge behaviour. Unix
/// calls this only once reconnecting is not attempted or has been
/// exhausted; Windows calls it unconditionally (reconnect-after-upgrade is
/// Unix-only).
fn finish_bridge_exit(transport: &Arc<AtomicU8>) -> ! {
    // Write synthetic Detached message so the GUI exits mux mode.
    // When the daemon dies, no explicit Detached is sent — the bridge
    // must synthesise one before exiting.
    {
        use std::io::Write;
        let detached = MuxMessage::control(MessageType::Detached, 0, &());
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        let t = transport.load(Ordering::Relaxed);
        if t == TRANSPORT_UNDETECTED {
            let _ = lock.write_all(detached.to_osc().as_bytes());
            let _ = lock.write_all(detached.to_apc().as_bytes());
        } else if t == Transport::Osc as u8 || t == Transport::Plaintext as u8 {
            let _ = lock.write_all(detached.to_osc().as_bytes());
        } else {
            let _ = lock.write_all(detached.to_apc().as_bytes());
        }
        log::info!("Wrote synthetic Detached message to stdout");
    }

    // Ensure all stdout data (including final Detached APC) is flushed
    // before exiting, so the GUI receives it.
    log::info!("Flushing stdout before exit");
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }

    // Brief delay so the GUI's PTY reader can consume the flushed data
    // before the PTY slave fd is closed by process exit.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Restore stdin before exit — process::exit() skips Drop, so
    // the RawModeGuard won't run. Without this, the host terminal
    // stays in raw mode (no OPOST → LF without CR, echo disabled).
    #[cfg(unix)]
    restore_stdin_global();
    #[cfg(windows)]
    restore_stdin_windows_global();

    // Exit immediately so the host shell returns to foreground promptly.
    // Without this, tokio runtime shutdown waits for the blocked stdin
    // reader task, delaying shell prompt redraw by seconds.
    log::info!("Bridge exiting via process::exit");
    std::process::exit(0);
}

/// Bounds how long the bridge spends trying to reconnect after an upgrade
/// announcement before giving up (AC-5). Kept short: the daemon's listen
/// socket stays open across the replacement (IMPLEMENTATION.md D4), so a
/// reconnect that will succeed at all typically succeeds on the very first
/// attempt; these bounds only cover the retry corner cases.
#[cfg(unix)]
const RECONNECT_MAX_ATTEMPTS: u32 = 5;
#[cfg(unix)]
const RECONNECT_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_millis(50);
#[cfg(unix)]
const RECONNECT_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_millis(800);

/// One connect-and-handshake attempt against `sock_path`. Failure (connect
/// or handshake) is reported as `Err` for the caller to retry — never
/// propagated as a fatal bridge error, unlike the initial connection.
#[cfg(unix)]
async fn try_reconnect_once(sock_path: &std::path::Path) -> Result<UnixStream, String> {
    let stream = UnixStream::connect(sock_path)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let (mut reader, mut writer) = tokio::io::split(stream);
    perform_handshake(&mut reader, &mut writer)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    Ok(reader.unsplit(writer))
}

/// Retries `try_reconnect_once` with backoff, bounded by
/// `RECONNECT_MAX_ATTEMPTS` (AC-5). Sleeps between attempts rather than
/// spinning (AC-6). Returns `None` once the window is exhausted.
#[cfg(unix)]
async fn reconnect_with_backoff(sock_path: &std::path::Path) -> Option<UnixStream> {
    let mut backoff = RECONNECT_INITIAL_BACKOFF;
    for attempt in 1..=RECONNECT_MAX_ATTEMPTS {
        match try_reconnect_once(sock_path).await {
            Ok(stream) => {
                log::info!(
                    "mux bridge: reconnected to daemon after upgrade announcement \
                     (attempt {}/{})",
                    attempt,
                    RECONNECT_MAX_ATTEMPTS
                );
                return Some(stream);
            }
            Err(e) => {
                log::warn!(
                    "mux bridge: reconnect attempt {}/{} failed: {}",
                    attempt,
                    RECONNECT_MAX_ATTEMPTS,
                    e
                );
                if attempt == RECONNECT_MAX_ATTEMPTS {
                    break;
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_MAX_BACKOFF);
            }
        }
    }
    None
}

/// Writes one pre-framed message body (length prefix + body) directly to
/// the socket, bypassing the stdin parser. Used to resend the last `Attach`
/// frame right after a reconnect's handshake completes.
#[cfg(unix)]
async fn resend_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    body: &[u8],
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let len = (body.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Reconnects with backoff and, on success, resends the last-known `Attach`
/// frame so the daemon re-attaches the bridge to the same session it was
/// attached to before the drop, letting the daemon's existing reattach
/// machinery repaint the panes (AC-4). Returns the new connection, or
/// `None` once the reconnect window is exhausted (AC-5).
#[cfg(unix)]
async fn reconnect_and_reattach(
    sock_path: &std::path::Path,
    last_attach: &Arc<Mutex<Option<Vec<u8>>>>,
) -> Option<UnixStream> {
    let stream = reconnect_with_backoff(sock_path).await?;
    let attach_body = last_attach
        .lock()
        .expect("last_attach mutex poisoned")
        .clone();
    let Some(body) = attach_body else {
        return Some(stream);
    };
    let (reader, mut writer) = tokio::io::split(stream);
    if let Err(e) = resend_frame(&mut writer, &body).await {
        log::warn!("mux bridge: failed to resend Attach after reconnect: {}", e);
    }
    Some(reader.unsplit(writer))
}

/// Actions produced by the stdin mux parser.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum StdinAction {
    /// A decoded mux message to forward to the daemon, with transport info.
    MuxMessage(MuxMessage, Transport),
    /// Passthrough data (non-mux bytes).
    Passthrough(Vec<u8>),
}

/// Maximum APC payload size (matches MAX_FRAME_LENGTH after Base64 expansion).
/// Base64 expands by ~4/3, so 22MB covers the 16MB frame limit.
const MAX_APC_PAYLOAD: usize = 22 * 1024 * 1024;

// `PLAINTEXT_PREFIX` (and `APC_PREFIX` / `MUX_OSC_PARAM`) come from the
// `mux_ipc::protocol` SSOT via the `use super::ipc::protocol::*` glob above, so
// all three mux transport markers have a single owner.

/// State machine that separates APC/OSC/plaintext mux sequences from passthrough data on stdin.
///
/// Handles partial reads across buffer boundaries.
/// Recognizes APC (ESC _), OSC 9999 (ESC ]), and plaintext (`EMUX;<base64>\r`,
/// also accepting LF / CRLF / LFCR) mux sequences.
///
/// This is the bridge subprocess's INPUT-direction scanner and is intentionally
/// separate from `term_core::MuxApcExtractor` (the GUI's OUTPUT-direction outer
/// parse): only this side handles the Plaintext `EMUX;` transport and forwards
/// non-mux bytes as passthrough. Both lean on the same `mux_ipc::protocol`
/// markers and `MuxMessage::from_apc` decode, so the wire format has a single
/// SSOT even though the two byte-level state machines are not shared.
pub(super) struct StdinApcParser {
    state: ParserState,
    apc_buf: Vec<u8>,
    passthrough_buf: Vec<u8>,
    /// Accumulator for OSC numeric parameter.
    osc_param_accum: u16,
    /// Number of digits accumulated for the OSC parameter (to reconstruct on rejection).
    osc_param_digits: Vec<u8>,
    /// Whether the current sequence entered via the OSC path (ESC ] 9999 ;).
    is_osc: bool,
    /// Number of bytes matched in the EMUX; prefix so far.
    plaintext_prefix_matched: usize,
    /// True iff a Plaintext message just completed and the next byte, if
    /// it is the partner half of a CRLF / LFCR terminator pair, must be
    /// dropped silently (not passthrough'd). Reset by any non-EOL byte.
    swallow_partner_eol: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// Normal text / passthrough mode.
    Ground,
    /// Seen ESC, waiting for _ (APC start), ] (OSC start), or \ (APC end inside accumulation).
    EscSeen,
    /// Inside OSC parameter accumulation (digits after ESC ]).
    InOscParam,
    /// Inside APC/OSC body accumulation.
    InApc,
    /// Inside APC/OSC body, seen ESC (could be ST = ESC \).
    InApcEsc,
    /// Matching EMUX; prefix (plaintext_prefix_matched tracks position).
    InPlaintextPrefix,
    /// Inside plaintext body accumulation (after EMUX; prefix matched).
    InPlaintext,
}

impl StdinApcParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
            apc_buf: Vec::new(),
            passthrough_buf: Vec::new(),
            osc_param_accum: 0,
            osc_param_digits: Vec::new(),
            is_osc: false,
            plaintext_prefix_matched: 0,
            swallow_partner_eol: false,
        }
    }

    /// Complete a plaintext mux sequence (`EMUX;<base64>\r`; also accepts
    /// LF / CRLF / LFCR — see the `InPlaintext` arm in [`Self::feed`]).
    fn complete_plaintext_sequence(apc_buf: &mut Vec<u8>, actions: &mut Vec<StdinAction>) {
        let payload_str = String::from_utf8_lossy(apc_buf).to_string();
        apc_buf.clear();

        // The buf contains the base64 data (after EMUX; prefix, before the
        // CR/LF terminator). Wrap it with APC_PREFIX so from_apc can decode it.
        let with_prefix = format!("{}{}", APC_PREFIX, payload_str);
        match MuxMessage::from_apc(&with_prefix) {
            Ok(msg) => actions.push(StdinAction::MuxMessage(msg, Transport::Plaintext)),
            Err(e) => {
                eprintln!("Bridge: plaintext mux decode error: {}", e);
            }
        }
    }

    /// Complete a mux sequence: decode the accumulated APC buffer and produce an action.
    fn complete_mux_sequence(apc_buf: &mut Vec<u8>, is_osc: bool, actions: &mut Vec<StdinAction>) {
        let payload = String::from_utf8_lossy(apc_buf).to_string();
        apc_buf.clear();

        let transport = if is_osc {
            Transport::Osc
        } else {
            Transport::Apc
        };

        if payload.starts_with(APC_PREFIX) {
            match MuxMessage::from_apc(&payload) {
                Ok(msg) => actions.push(StdinAction::MuxMessage(msg, transport)),
                Err(e) => {
                    eprintln!("Bridge: mux decode error: {}", e);
                }
            }
        } else if !is_osc {
            // Non-mux APC: forward as passthrough (ESC_ + body + ESC\)
            let mut pdata = Vec::with_capacity(2 + payload.len() + 2);
            pdata.extend_from_slice(b"\x1b_");
            pdata.extend_from_slice(payload.as_bytes());
            pdata.extend_from_slice(b"\x1b\\");
            actions.push(StdinAction::Passthrough(pdata));
        }
        // Non-mux OSC with param 9999 shouldn't happen (we only enter InApc for 9999),
        // but if the body doesn't have APC_PREFIX, just discard it.
    }

    /// Feed bytes into the parser and return resulting actions.
    pub fn feed(&mut self, data: &[u8]) -> Vec<StdinAction> {
        let mut actions = Vec::new();

        for &byte in data {
            match self.state {
                ParserState::Ground => {
                    // Drop the partner half of a CRLF / LFCR terminator that
                    // just closed a Plaintext message, so a trailing CR or LF
                    // doesn't leak out as passthrough. Any non-EOL byte
                    // immediately disarms the swallow.
                    if self.swallow_partner_eol {
                        self.swallow_partner_eol = false;
                        if byte == b'\r' || byte == b'\n' {
                            continue;
                        }
                    }
                    if byte == 0x1B {
                        self.state = ParserState::EscSeen;
                    } else if byte == PLAINTEXT_PREFIX[0] {
                        // Potential start of EMUX; prefix
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.plaintext_prefix_matched = 1;
                        self.state = ParserState::InPlaintextPrefix;
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
                        self.is_osc = false;
                    } else if byte == b']' {
                        // OSC start: flush passthrough and begin parameter accumulation
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.osc_param_accum = 0;
                        self.osc_param_digits.clear();
                        self.state = ParserState::InOscParam;
                    } else {
                        // Not APC/OSC start: treat ESC + byte as passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(byte);
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InOscParam => {
                    if byte.is_ascii_digit() {
                        self.osc_param_digits.push(byte);
                        self.osc_param_accum =
                            self.osc_param_accum.saturating_mul(10) + (byte - b'0') as u16;
                    } else if byte == b';' {
                        if self.osc_param_accum == MUX_OSC_PARAM {
                            // Matched OSC 9999; -> reuse APC accumulation for body
                            self.state = ParserState::InApc;
                            self.apc_buf.clear();
                            self.is_osc = true;
                        } else {
                            // Wrong OSC param: push ESC ] <digits> ; as passthrough
                            self.passthrough_buf.push(0x1B);
                            self.passthrough_buf.push(b']');
                            self.passthrough_buf
                                .extend_from_slice(&self.osc_param_digits);
                            self.passthrough_buf.push(b';');
                            self.osc_param_digits.clear();
                            self.state = ParserState::Ground;
                        }
                    } else {
                        // Non-digit, non-semicolon: not a valid OSC param, passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(b']');
                        self.passthrough_buf
                            .extend_from_slice(&self.osc_param_digits);
                        self.passthrough_buf.push(byte);
                        self.osc_param_digits.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApc => {
                    if byte == 0x1B {
                        self.state = ParserState::InApcEsc;
                    } else if self.is_osc && byte == 0x07 {
                        // BEL as alternative OSC terminator
                        Self::complete_mux_sequence(&mut self.apc_buf, self.is_osc, &mut actions);
                        self.state = ParserState::Ground;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        // APC/OSC payload too large: discard and reset to ground
                        eprintln!(
                            "Bridge: APC/OSC payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApcEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) found: decode the payload
                        Self::complete_mux_sequence(&mut self.apc_buf, self.is_osc, &mut actions);
                        self.state = ParserState::Ground;
                    } else {
                        // ESC inside APC but not followed by \: keep accumulating
                        self.apc_buf.push(0x1B);
                        self.apc_buf.push(byte);
                        self.state = ParserState::InApc;
                    }
                }
                ParserState::InPlaintextPrefix => {
                    let expected = PLAINTEXT_PREFIX[self.plaintext_prefix_matched];
                    if byte == expected {
                        self.plaintext_prefix_matched += 1;
                        if self.plaintext_prefix_matched == PLAINTEXT_PREFIX.len() {
                            // Full prefix matched: start accumulating body
                            self.apc_buf.clear();
                            self.state = ParserState::InPlaintext;
                        }
                    } else {
                        // Prefix mismatch: push matched bytes + current as passthrough
                        self.passthrough_buf
                            .extend_from_slice(&PLAINTEXT_PREFIX[..self.plaintext_prefix_matched]);
                        self.passthrough_buf.push(byte);
                        self.plaintext_prefix_matched = 0;
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InPlaintext => {
                    if byte == b'\r' || byte == b'\n' {
                        // EITHER CR or LF terminates the plaintext message.
                        // The Windows host writes the envelope with a CR
                        // terminator because portable-pty 0.8 opens ConPTY
                        // with `PSEUDOCONSOLE_WIN32_INPUT_MODE` and raw LF is
                        // not delivered as a real key event on that channel
                        // (CR rides through as VK_RETURN). Intermediate
                        // layers can still substitute or duplicate the
                        // terminator: this branch accepts whichever arrives
                        // first; the Ground state then swallows the partner
                        // half via `swallow_partner_eol` so a trailing
                        // CRLF/LFCR doesn't surface as passthrough.
                        // base64 STANDARD's alphabet contains neither CR nor
                        // LF, so a CR/LF inside the body is always a
                        // terminator (the surrounding handshake is
                        // CR/LF-free).
                        Self::complete_plaintext_sequence(&mut self.apc_buf, &mut actions);
                        self.swallow_partner_eol = true;
                        self.state = ParserState::Ground;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        eprintln!(
                            "Bridge: plaintext payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
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

#[cfg(test)]
mod tests {
    use super::*;

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
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Apc);
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
            StdinAction::MuxMessage(decoded, _) => {
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
                !matches!(a, StdinAction::MuxMessage(_, _)),
                "Should not decode incomplete APC"
            );
        }

        // Feed second half
        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
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
        // ESC followed by something other than _ or ] should be passthrough
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
                StdinAction::MuxMessage(m, _) => Some(m),
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
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b_emterm-mux;");
        input.push(0x1B); // ESC
        input.push(b'X'); // Not \, so should be added to APC buf
        input.extend_from_slice(b"\x1b\\");
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        // The APC body is "emterm-mux;\x1bX" which is invalid base64
        assert_eq!(actions.len(), 0); // decode error is printed, no action produced
    }

    // ---- OSC mux message tests ----

    #[test]
    fn test_stdin_parser_osc_mux_message() {
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let osc = msg.to_osc();
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(osc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Osc);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_osc_with_bel_terminator() {
        // OSC 9999 terminated with BEL (0x07) instead of ESC \
        let msg = MuxMessage::pty_input(3, vec![0xAA]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let osc = format!("\x1b]9999;emterm-mux;{}\x07", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(osc.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 3);
                assert_eq!(decoded.payload, vec![0xAA]);
                assert_eq!(*transport, Transport::Osc);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_osc_wrong_param_passthrough() {
        // OSC with param != 9999 should be passed through
        let input = b"\x1b]1234;some-data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        // The ESC ] 1 2 3 4 ; is pushed as passthrough, then "some-data" and ESC \ are ground
        assert!(!actions.is_empty());
        // All actions should be Passthrough
        for a in &actions {
            assert!(
                matches!(a, StdinAction::Passthrough(_)),
                "Expected Passthrough for non-9999 OSC, got {:?}",
                a
            );
        }
    }

    #[test]
    fn test_stdin_parser_mixed_apc_and_osc() {
        let msg1 = MuxMessage::pty_input(1, vec![0x01]);
        let msg2 = MuxMessage::pty_input(2, vec![0x02]);
        let msg3 = MuxMessage::pty_input(3, vec![0x03]);
        let mut input = msg1.to_apc().into_bytes();
        input.extend_from_slice(msg2.to_osc().as_bytes());
        input.extend_from_slice(msg3.to_apc().as_bytes());

        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m, t) => Some((m, *t)),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].0.pane_id, 1);
        assert_eq!(msgs[0].1, Transport::Apc);
        assert_eq!(msgs[1].0.pane_id, 2);
        assert_eq!(msgs[1].1, Transport::Osc);
        assert_eq!(msgs[2].0.pane_id, 3);
        assert_eq!(msgs[2].1, Transport::Apc);
    }

    #[test]
    fn test_stdin_parser_osc_split_across_boundaries() {
        let msg = MuxMessage::pty_input(5, vec![0xFF]);
        let osc = msg.to_osc();
        let bytes = osc.as_bytes();
        let mid = bytes.len() / 2;

        let mut parser = StdinApcParser::new();

        // Feed first half
        let actions1 = parser.feed(&bytes[..mid]);
        for a in &actions1 {
            assert!(
                !matches!(a, StdinAction::MuxMessage(_, _)),
                "Should not decode incomplete OSC"
            );
        }

        // Feed second half
        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
        assert!(has_msg, "Should decode OSC after second half");
    }

    #[test]
    fn test_stdin_parser_osc_non_digit_in_param() {
        // ESC ] followed by non-digit should be passthrough
        let input = b"\x1b]abc;data\x1b\\";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        // 'a' is non-digit, so ESC ] a is pushed as passthrough, rest continues as ground
        assert!(!actions.is_empty());
        for a in &actions {
            assert!(matches!(a, StdinAction::Passthrough(_)));
        }
    }

    // ---- Plaintext mux message tests ----

    #[test]
    fn test_stdin_parser_plaintext_mux_message() {
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("EMUX;{}\n", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input.as_bytes());
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Plaintext);
            }
            _ => panic!("Expected MuxMessage"),
        }
    }

    #[test]
    fn test_stdin_parser_plaintext_with_passthrough() {
        let msg = MuxMessage::pty_input(1, vec![0x41]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("beforeEMUX;{}\nafter", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input.as_bytes());
        // "before" as passthrough, then MuxMessage, then "after" as passthrough
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m, _) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].pane_id, 1);
    }

    #[test]
    fn test_stdin_parser_plaintext_split_across_boundaries() {
        let msg = MuxMessage::pty_input(5, vec![0xFF]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("EMUX;{}\n", encoded);
        let bytes = input.as_bytes();
        let mid = bytes.len() / 2;

        let mut parser = StdinApcParser::new();

        let actions1 = parser.feed(&bytes[..mid]);
        for a in &actions1 {
            assert!(
                !matches!(a, StdinAction::MuxMessage(_, _)),
                "Should not decode incomplete plaintext"
            );
        }

        let actions2 = parser.feed(&bytes[mid..]);
        let has_msg = actions2
            .iter()
            .any(|a| matches!(a, StdinAction::MuxMessage(_, _)));
        assert!(has_msg, "Should decode plaintext after second half");
    }

    #[test]
    fn test_stdin_parser_plaintext_prefix_mismatch() {
        // "EMUY;" is not a valid prefix, should be passthrough
        let input = b"EMUY;data\n";
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input);
        assert!(!actions.is_empty());
        for a in &actions {
            assert!(matches!(a, StdinAction::Passthrough(_)));
        }
    }

    #[test]
    fn test_stdin_parser_plaintext_cr_terminator() {
        // The Windows host writes the envelope with a CR terminator because
        // portable-pty 0.8 opens ConPTY with `PSEUDOCONSOLE_WIN32_INPUT_MODE`,
        // and raw LF is not delivered as a real key event on that channel —
        // only CR (VK_RETURN) survives. This pins that CR alone closes the
        // message; if it doesn't, the bridge stalls in `InPlaintext` forever
        // with the prefix matched but no terminator ever arriving (the exact
        // regression observed against a hyper-v mux daemon).
        let msg = MuxMessage::pty_input(1, vec![0x41, 0x42]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("EMUX;{}\r", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input.as_bytes());
        assert_eq!(actions.len(), 1, "CR alone must produce exactly one action");
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 1);
                assert_eq!(decoded.payload, vec![0x41, 0x42]);
                assert_eq!(*transport, Transport::Plaintext);
            }
            other => panic!("Expected MuxMessage, got {other:?}"),
        }
    }

    #[test]
    fn test_stdin_parser_plaintext_crlf_terminator() {
        // Intermediate layers (ConPTY, ssh, the host shell) can append LF
        // after the encoder's CR terminator, or translate the standalone CR
        // to a CRLF pair on the way to the bridge. The parser must complete
        // on the first half and silently consume the partner half so the
        // trailing LF doesn't leak out as passthrough.
        let msg = MuxMessage::pty_input(3, vec![0xAB, 0xCD, 0xEF]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("EMUX;{}\r\n", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input.as_bytes());
        assert_eq!(actions.len(), 1, "CRLF must produce exactly one action");
        match &actions[0] {
            StdinAction::MuxMessage(decoded, transport) => {
                assert_eq!(decoded.msg_type, MessageType::PtyInput);
                assert_eq!(decoded.pane_id, 3);
                assert_eq!(decoded.payload, vec![0xAB, 0xCD, 0xEF]);
                assert_eq!(*transport, Transport::Plaintext);
            }
            other => panic!("Expected MuxMessage, got {other:?}"),
        }
    }

    #[test]
    fn test_stdin_parser_plaintext_lfcr_terminator() {
        // The reverse order LF→CR must also collapse to a single message,
        // mirroring the CRLF tolerance above. base64 STANDARD has neither
        // CR nor LF in its alphabet, so a trailing CR after the LF is
        // unambiguously the partner half of the terminator.
        let msg = MuxMessage::pty_input(2, vec![0x10]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let input = format!("EMUX;{}\n\r", encoded);
        let mut parser = StdinApcParser::new();
        let actions = parser.feed(input.as_bytes());
        assert_eq!(actions.len(), 1, "LFCR must produce exactly one action");
        match &actions[0] {
            StdinAction::MuxMessage(decoded, _) => {
                assert_eq!(decoded.pane_id, 2);
                assert_eq!(decoded.payload, vec![0x10]);
            }
            other => panic!("Expected MuxMessage, got {other:?}"),
        }
    }

    #[test]
    fn test_stdin_parser_plaintext_terminator_split_across_feeds() {
        // The terminator pair (CRLF) is allowed to land on opposite sides of
        // a feed boundary: the CR completes the message at the end of the
        // first feed, and the LF arriving as the first byte of the next feed
        // must be silently swallowed via `swallow_partner_eol` rather than
        // leaking out as a stray passthrough byte.
        let msg = MuxMessage::pty_input(9, vec![0xFF]);
        let body = msg.to_frame_body();
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        let first = format!("EMUX;{}\r", encoded);

        let mut parser = StdinApcParser::new();
        let actions1 = parser.feed(first.as_bytes());
        let msgs1: Vec<_> = actions1
            .iter()
            .filter(|a| matches!(a, StdinAction::MuxMessage(_, _)))
            .collect();
        assert_eq!(msgs1.len(), 1, "CR alone closes the message in feed 1");

        // Feed 2 starts with the partner LF: must be swallowed silently.
        let actions2 = parser.feed(b"\nrest");
        let passthroughs: Vec<&Vec<u8>> = actions2
            .iter()
            .filter_map(|a| match a {
                StdinAction::Passthrough(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            passthroughs.len(),
            1,
            "feed 2 produces exactly one passthrough (just 'rest')"
        );
        assert_eq!(passthroughs[0], b"rest");
    }

    #[test]
    fn test_stdin_parser_mixed_plaintext_and_apc() {
        let msg1 = MuxMessage::pty_input(1, vec![0x01]);
        let msg2 = MuxMessage::pty_input(2, vec![0x02]);
        let body2 = msg2.to_frame_body();
        use base64::Engine as _;
        let encoded2 = base64::engine::general_purpose::STANDARD.encode(&body2);
        let mut input = msg1.to_apc().into_bytes();
        input.extend_from_slice(format!("EMUX;{}\n", encoded2).as_bytes());

        let mut parser = StdinApcParser::new();
        let actions = parser.feed(&input);
        let msgs: Vec<_> = actions
            .iter()
            .filter_map(|a| match a {
                StdinAction::MuxMessage(m, t) => Some((m, *t)),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0.pane_id, 1);
        assert_eq!(msgs[0].1, Transport::Apc);
        assert_eq!(msgs[1].0.pane_id, 2);
        assert_eq!(msgs[1].1, Transport::Plaintext);
    }

    // ---- task0006: client reconnect after an upgrade announcement ----

    /// AC-1: the announcement decodes to `Announced`, never `Forward` — so
    /// it can never reach the terminal output path.
    #[test]
    fn decide_daemon_frame_effect_recognizes_upgrading_without_forwarding() {
        let announcement = MuxMessage::control(MessageType::Upgrading, 0, &());
        let body = announcement.to_frame_body();

        match decide_daemon_frame_effect(&body) {
            DaemonFrameEffect::Announced => {}
            other => panic!("expected Announced, got {other:?}"),
        }
    }

    /// Sanity check alongside AC-1: an ordinary message still decodes to
    /// `Forward` with its fields intact, so recognizing the announcement
    /// didn't regress the existing forwarding path.
    #[test]
    fn decide_daemon_frame_effect_forwards_ordinary_messages() {
        let msg = MuxMessage::pty_output(3, vec![0x41, 0x42]);
        let body = msg.to_frame_body();

        match decide_daemon_frame_effect(&body) {
            DaemonFrameEffect::Forward(forwarded) => {
                assert_eq!(forwarded.msg_type, MessageType::PtyOutput);
                assert_eq!(forwarded.pane_id, 3);
                assert_eq!(forwarded.payload, vec![0x41, 0x42]);
            }
            other => panic!("expected Forward, got {other:?}"),
        }
    }

    #[test]
    fn decide_daemon_frame_effect_ignores_undecodable_frames() {
        // Too short to contain even the 5-byte header.
        let body = vec![0x01, 0x02];
        match decide_daemon_frame_effect(&body) {
            DaemonFrameEffect::Ignored => {}
            other => panic!("expected Ignored, got {other:?}"),
        }
    }

    /// Half of AC-4: only an `Attach` request is captured for a later
    /// resend; every other message type is not.
    #[test]
    fn capture_if_attach_captures_attach_and_only_attach() {
        let attach = MuxMessage::control(MessageType::Attach, 0, &AttachMsg { session_id: 5 });
        let attach_body = attach.to_frame_body();
        assert_eq!(
            capture_if_attach(&attach, &attach_body),
            Some(attach_body.clone())
        );

        let other = MuxMessage::pty_input(1, vec![0x41]);
        let other_body = other.to_frame_body();
        assert_eq!(capture_if_attach(&other, &other_body), None);
    }

    /// AC-3 / AC-7: the connection-ended decision is exactly the observed
    /// announcement flag — never sticky across calls (`forward_loop`
    /// creates a fresh flag every invocation, so a reconnected connection
    /// that never sees a NEW announcement concludes `Normal`).
    #[test]
    fn conclude_connection_maps_the_announced_flag() {
        assert_eq!(conclude_connection(false), ConnectionEnded::Normal);
        assert_eq!(conclude_connection(true), ConnectionEnded::Announced);
    }

    // ---- reconnect: stand-in daemon in a plain thread, mirroring the
    // pattern in daemon.rs's tests (spawn_fake_legacy_daemon) ----

    #[cfg(unix)]
    fn read_frame_blocking<S: std::io::Read>(stream: &mut S) -> MuxMessage {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).expect("read frame length");
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut frame_buf = vec![0u8; frame_len];
        stream.read_exact(&mut frame_buf).expect("read frame body");
        MuxMessage::from_frame_body(&frame_buf).expect("valid frame")
    }

    #[cfg(unix)]
    fn write_frame_blocking<S: std::io::Write>(stream: &mut S, msg: &MuxMessage) {
        let body = msg.to_frame_body();
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .expect("write frame length");
        stream.write_all(&body).expect("write frame body");
        stream.flush().expect("flush");
    }

    #[cfg(unix)]
    fn accept_and_handshake_blocking(
        listener: &std::os::unix::net::UnixListener,
    ) -> std::os::unix::net::UnixStream {
        let (mut stream, _) = listener.accept().expect("accept reconnect attempt");
        let hello_frame = read_frame_blocking(&mut stream);
        assert_eq!(hello_frame.msg_type, MessageType::Hello);
        let welcome = WelcomeMsg::Accepted {
            server_version: PROTOCOL_VERSION,
            sessions: Vec::new(),
        };
        write_frame_blocking(
            &mut stream,
            &MuxMessage::control(MessageType::Welcome, 0, &welcome),
        );
        stream
    }

    /// AC-2 / AC-4: `reconnect_and_reattach` completes a second handshake
    /// against the stand-in daemon and then resends the previously
    /// captured `Attach` frame verbatim.
    #[cfg(unix)]
    #[tokio::test]
    async fn reconnect_and_reattach_resends_the_last_attach_after_reconnecting() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("reconnect.sock");

        let attach = MuxMessage::control(MessageType::Attach, 0, &AttachMsg { session_id: 7 });
        let attach_body = attach.to_frame_body();
        let last_attach: Arc<Mutex<Option<Vec<u8>>>> =
            Arc::new(Mutex::new(Some(attach_body.clone())));

        let bind_path = sock_path.clone();
        let daemon = std::thread::spawn(move || {
            let listener = UnixListener::bind(&bind_path).expect("bind stand-in daemon socket");
            let mut stream = accept_and_handshake_blocking(&listener);
            // The very next frame after the reconnect handshake must be
            // the resent Attach request (AC-4), not anything else.
            read_frame_blocking(&mut stream)
        });

        let result = reconnect_and_reattach(&sock_path, &last_attach).await;
        assert!(
            result.is_some(),
            "reconnect_and_reattach should succeed against a live stand-in daemon"
        );

        let resent = daemon.join().expect("stand-in daemon thread panicked");
        assert_eq!(resent.msg_type, MessageType::Attach);
        let decoded: AttachMsg = resent.decode_payload().expect("Attach payload");
        assert_eq!(decoded.session_id, 7);
    }

    /// AC-5: with nothing ever listening on `sock_path`, every attempt
    /// fails and the bounded retry loop gives up rather than retrying
    /// forever.
    #[cfg(unix)]
    #[tokio::test]
    async fn reconnect_with_backoff_gives_up_after_the_window_is_exhausted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("nothing-here.sock");

        let started = std::time::Instant::now();
        let result = reconnect_with_backoff(&sock_path).await;
        let elapsed = started.elapsed();

        assert!(
            result.is_none(),
            "reconnect must give up, not retry forever"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "the retry window must be bounded, took {:?}",
            elapsed
        );
    }

    /// AC-6: the stand-in daemon only starts listening after the first
    /// backoff delay would have elapsed, so the first attempt observably
    /// fails (nothing bound yet) before a later attempt succeeds — proving
    /// a real delay separates attempts rather than a tight spin.
    #[cfg(unix)]
    #[tokio::test]
    async fn reconnect_with_backoff_waits_between_failed_attempts() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().expect("tempdir");
        let sock_path = dir.path().join("delayed.sock");
        let bind_path = sock_path.clone();

        let daemon = std::thread::spawn(move || {
            std::thread::sleep(RECONNECT_INITIAL_BACKOFF + std::time::Duration::from_millis(20));
            let listener = UnixListener::bind(&bind_path).expect("bind delayed stand-in daemon");
            let _stream = accept_and_handshake_blocking(&listener);
        });

        let started = std::time::Instant::now();
        let result = reconnect_with_backoff(&sock_path).await;
        let elapsed = started.elapsed();

        daemon.join().expect("stand-in daemon thread panicked");

        assert!(
            result.is_some(),
            "reconnect should succeed once the daemon starts listening"
        );
        assert!(
            elapsed >= RECONNECT_INITIAL_BACKOFF,
            "attempts must be separated by a backoff delay, took only {:?}",
            elapsed
        );
    }

    // ---- task0010: the stdin handle (and its parser) persist across
    // `forward_loop` calls instead of being recreated per reconnect ----

    /// AC-1 / AC-2 / AC-3: `forward_loop` takes the stdin handle and its
    /// parser as caller-owned `&mut` parameters instead of constructing its
    /// own, so the SAME instances can be threaded through every connection
    /// of a bridge run (exactly how `bridge_main_loop` uses them across a
    /// reconnect). This test drives two `forward_loop` calls with the same
    /// handle/parser and proves the direct consequence: a byte that
    /// arrives on stdin only AFTER the first connection has already ended
    /// is still delivered to the daemon once the second connection's call
    /// starts reading (AC-2). Before this fix, each call constructed its
    /// own `tokio::io::stdin()`; that handle would already have been
    /// dropped by the time this byte arrived, discarding it silently while
    /// leaving the abandoned blocking read (AC-3) occupying a pool thread
    /// until the user's next keystroke. Because only one handle is ever
    /// constructed for the whole run (AC-1), no such second handle exists
    /// to strand a read against.
    #[cfg(unix)]
    #[tokio::test]
    async fn forward_loop_persistent_stdin_handle_delivers_bytes_queued_across_reconnect() {
        use tokio::io::AsyncReadExt as _;
        use tokio::io::AsyncWriteExt as _;

        // One stdin handle + one parser for the whole simulated bridge
        // run — created ONCE, exactly as `bridge_main_loop` does, and
        // passed into BOTH `forward_loop` calls below.
        let (mut stdin, mut stdin_probe) = tokio::io::duplex(4096);
        let mut parser = StdinApcParser::new();
        let transport = Arc::new(AtomicU8::new(TRANSPORT_UNDETECTED));
        let last_attach: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

        // Connection #1's "daemon" side is closed immediately, before
        // stdin has produced anything, so `daemon_to_stdout` ends the call
        // right away — mirroring the daemon dropping the connection while
        // a keystroke is still in flight on stdin.
        let (conn1, conn1_daemon_side) = tokio::io::duplex(64);
        drop(conn1_daemon_side);
        let (mut r1, mut w1) = tokio::io::split(conn1);

        let ended1 = forward_loop(
            &mut r1,
            &mut w1,
            &transport,
            &last_attach,
            &mut stdin,
            &mut parser,
        )
        .await;
        assert_eq!(ended1, ConnectionEnded::Normal);

        // The keystroke arrives only now, strictly after connection #1 has
        // already ended — the window in which a freshly-recreated handle
        // would already have been dropped.
        let msg = MuxMessage::pty_input(1, vec![0x41]);
        let apc = msg.to_apc();
        stdin_probe
            .write_all(apc.as_bytes())
            .await
            .expect("write pending keystroke");
        stdin_probe.flush().await.expect("flush pending keystroke");

        // Connection #2 stands in for the reconnect: a fresh "daemon"
        // duplex, but `stdin` and `parser` are the SAME instances again —
        // never recreated.
        let (conn2, mut conn2_daemon_side) = tokio::io::duplex(4096);
        let (mut r2, mut w2) = tokio::io::split(conn2);

        let daemon_task = tokio::spawn(async move {
            let mut len_buf = [0u8; 4];
            conn2_daemon_side
                .read_exact(&mut len_buf)
                .await
                .expect("read forwarded frame length");
            let frame_len = u32::from_be_bytes(len_buf) as usize;
            let mut frame_buf = vec![0u8; frame_len];
            conn2_daemon_side
                .read_exact(&mut frame_buf)
                .await
                .expect("read forwarded frame body");
            MuxMessage::from_frame_body(&frame_buf).expect("valid forwarded frame")
            // `conn2_daemon_side` drops here, ending the duplex so `r2`
            // observes EOF and the `forward_loop` call below returns.
        });

        let ended2 = forward_loop(
            &mut r2,
            &mut w2,
            &transport,
            &last_attach,
            &mut stdin,
            &mut parser,
        )
        .await;
        assert_eq!(ended2, ConnectionEnded::Normal);

        let forwarded = daemon_task.await.expect("daemon task panicked");
        assert_eq!(forwarded.msg_type, MessageType::PtyInput);
        assert_eq!(forwarded.pane_id, 1);
        assert_eq!(forwarded.payload, vec![0x41]);
    }
}
