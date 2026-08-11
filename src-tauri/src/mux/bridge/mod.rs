//! Bridge process: stdin/stdout APC ↔ daemon socket forwarding.
//!
//! The bridge translates between APC escape sequences on stdin/stdout
//! and MuxMessage frames on the Unix domain socket.

use mux_ipc::protocol::{
    ClientType, HelloMsg, MAX_FRAME_LENGTH, MessageType, MuxMessage, PROTOCOL_VERSION,
};

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU8;

#[cfg(unix)]
use tokio::net::UnixStream;

mod stdin_parser;
use stdin_parser::*;

mod term_mode;
use term_mode::*;

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

mod forward;
use forward::*;

#[cfg(test)]
mod tests;
