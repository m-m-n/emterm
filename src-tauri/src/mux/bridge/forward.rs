//! Forwarding core: the bounded stdout writer pump, the
//! stdin/daemon/stdout forward loop, bridge exit, and the
//! post-upgrade reconnect-with-backoff path.

use super::*;

// ---- stdout writer: the only place forwarded frames reach stdout ----
//
// `daemon_to_stdout` (below) must never perform the stdout syscall itself
// (task0002 invariant 1): a stalled GUI-side PTY reader would then block
// the tokio runtime thread, stalling `stdin_to_daemon` too. Instead it
// admits each `Forward` frame to a bounded channel consumed by a pump
// running on tokio's dedicated blocking-thread pool (never a runtime
// worker thread), and only that pump touches stdout.

/// Bounds the stdout writer's admission channel (FR4): how many decoded
/// daemon frames may be queued awaiting a stalled sink before
/// `daemon_to_stdout`'s admission suspends. Keeps memory growth capped and
/// gives the socket-drain direction a bounded amount of slack before its
/// own backpressure (not reading more from the socket) kicks in.
pub(in crate::mux::bridge) const STDOUT_WRITER_CAPACITY: usize = 64;

/// How long `forward_loop` waits for the stdout writer to drain its queue
/// after the connection ends, before giving up and returning anyway
/// (invariant 6: bounded quiesce, not an unconditional wait). Matches the
/// project's named 5-second timeout convention (IMPLEMENTATION.md
/// Convention 5, mirroring `connection.rs`'s `HANDSHAKE_TIMEOUT`).
const STDOUT_WRITER_QUIESCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Where the stdout writer pump sends encoded bytes. Production plugs in
/// real process stdout (locked once, inside the pump's own thread, since
/// `StdoutLock` cannot be constructed elsewhere and then moved in); tests
/// inject a controllable sink that can block until released, fail on
/// demand, and record write order (Test Notes: testability seam).
pub(in crate::mux::bridge) trait StdoutSink {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
}

/// Production `StdoutSink`: the process's real stdout. Locks stdout PER
/// WRITE rather than holding a `StdoutLock` for the pump's lifetime, so an
/// abandoned pump (after `STDOUT_WRITER_QUIESCE_TIMEOUT` gives up on it)
/// holds the global stdout lock for at most one write, not forever. That
/// keeps `finish_bridge_exit`'s own `stdout.lock()` and a later
/// `ProcessStdout::new()` from a reconnect's `forward_loop` from
/// deadlocking against a pump thread that is blocked writing and was never
/// aborted.
struct ProcessStdout;

impl ProcessStdout {
    fn new() -> Self {
        Self
    }
}

impl StdoutSink for ProcessStdout {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        std::io::Write::write_all(&mut std::io::stdout().lock(), buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut std::io::stdout().lock())
    }
}

/// Runs on a dedicated blocking-capable execution context (never the tokio
/// runtime thread — invariant 1): the ONLY place forwarded frames are
/// written to `sink`. Consumes `rx` in strict FIFO admission order
/// (invariant 3, FR3) via `blocking_recv` (the tokio API meant for use
/// inside `spawn_blocking`). Resolves each frame's transport encoding
/// against `transport` AT WRITE TIME (invariant 4), the same per-frame
/// resolution point as today's inline write. Ends when the channel closes
/// (all senders dropped: normal end-of-connection quiesce) or a sink write
/// fails (invariant 5): the loop stops and `rx` is dropped when this
/// function returns, so a pending or future `send` on the paired `Sender`
/// observes the channel is gone and the async side can react.
fn stdout_writer_pump<S: StdoutSink>(
    mut rx: tokio::sync::mpsc::Receiver<MuxMessage>,
    transport: &Arc<AtomicU8>,
    mut sink: S,
) {
    while let Some(msg) = rx.blocking_recv() {
        let t = transport.load(Ordering::Relaxed);
        let write_result = if t == TRANSPORT_UNDETECTED {
            // Transport not yet detected: send both so at least one arrives.
            let osc = msg.to_osc();
            let apc = msg.to_apc();
            sink.write_all(osc.as_bytes())
                .and_then(|_| sink.write_all(apc.as_bytes()))
        } else {
            // Plaintext input means Windows ConPTY: use OSC for output.
            let encoded = if t == Transport::Osc as u8 || t == Transport::Plaintext as u8 {
                msg.to_osc()
            } else {
                msg.to_apc()
            };
            sink.write_all(encoded.as_bytes())
        };
        if write_result.is_err() {
            log::info!("stdout write error, stopping stdout writer");
            break;
        }
        let _ = sink.flush();
    }
}

/// Spawns the stdout writer pump on tokio's dedicated blocking-thread pool
/// and returns the admission channel's sender plus a join handle the
/// caller quiesces against once the connection ends. `make_sink` is called
/// ON the blocking thread (not here), so a non-`Send` sink like
/// `ProcessStdout` never has to cross the thread boundary itself — only
/// the zero/small-capture constructor does.
pub(in crate::mux::bridge) fn spawn_stdout_writer<S, F>(
    transport: Arc<AtomicU8>,
    make_sink: F,
) -> (
    tokio::sync::mpsc::Sender<MuxMessage>,
    tokio::task::JoinHandle<()>,
)
where
    S: StdoutSink,
    F: FnOnce() -> S + Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel::<MuxMessage>(STDOUT_WRITER_CAPACITY);
    let handle = tokio::task::spawn_blocking(move || {
        let sink = make_sink();
        stdout_writer_pump(rx, &transport, sink);
    });
    (tx, handle)
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
///
/// Thin wrapper over [`forward_loop_inner`] that plugs in real process
/// stdout. Kept as a separate, signature-stable function (rather than
/// folding the sink parameter in here) so every existing call site —
/// including `bridge_main_loop_windows`, which task0002 must leave
/// textually untouched (NFR3) — keeps compiling unchanged; tests call
/// `forward_loop_inner` directly with an injected sink instead.
pub(in crate::mux::bridge) async fn forward_loop<R, W, I>(
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
    forward_loop_inner(
        sock_reader,
        sock_writer,
        transport,
        last_attach,
        stdin,
        parser,
        ProcessStdout::new,
    )
    .await
}

/// Why one `forward_loop_inner` call's `tokio::select!` resolved — used to
/// decide, right after, whether the stdout writer still needs quiescing
/// (invariant 6/7) or has already ended on its own.
enum ForwardEnd {
    StdinEnded,
    StdoutEnded,
    WriterEnded,
}

/// Does the actual work for [`forward_loop`]; generic over the stdout
/// sink so tests can inject a controllable one (Test Notes: testability
/// seam) while production always supplies [`ProcessStdout`]. `make_sink`
/// is invoked on the writer's own blocking-thread context, never here
/// (see [`spawn_stdout_writer`]).
#[allow(clippy::too_many_arguments)]
pub(in crate::mux::bridge) async fn forward_loop_inner<R, W, I, S, F>(
    sock_reader: &mut R,
    sock_writer: &mut W,
    transport: &Arc<AtomicU8>,
    last_attach: &Arc<Mutex<Option<Vec<u8>>>>,
    stdin: &mut I,
    parser: &mut StdinApcParser,
    make_sink: F,
) -> ConnectionEnded
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
    I: tokio::io::AsyncRead + Unpin,
    S: StdoutSink,
    F: FnOnce() -> S + Send + 'static,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    log::info!("Starting bidirectional forwarding");

    // Start with undetected transport. While undetected, send both OSC and APC.
    // Once the GUI sends its first message, lock to the detected transport.
    let announced = Arc::new(AtomicBool::new(false));
    let announced_for_stdout = Arc::clone(&announced);
    let transport_for_stdin = Arc::clone(transport);
    let last_attach_for_stdin = Arc::clone(last_attach);

    // The only place forwarded frames reach stdout (invariant 1): a
    // dedicated blocking-capable pump, fed through a bounded admission
    // channel. `daemon_to_stdout` only ever awaits room in this channel —
    // it never performs the stdout syscall itself.
    let (stdout_tx, mut writer_handle) = spawn_stdout_writer(Arc::clone(transport), make_sink);

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
                    // Admit to the writer's bounded channel via an
                    // asynchronous, yielding wait (invariant 1/2): this
                    // suspends the daemon→stdout future — never a thread —
                    // once the channel fills, which is exactly the
                    // backpressure signal that stops reading more frames
                    // from the socket until the writer catches up.
                    // Encoding is resolved by the writer AT WRITE TIME
                    // (invariant 4), not here.
                    if stdout_tx.send(msg).await.is_err() {
                        log::info!("stdout write error, stopping daemon→stdout");
                        break;
                    }
                }
            }
        }
    };

    // Run both directions concurrently, plus a third arm watching the
    // stdout writer itself: if the writer ends on its own (a sink write
    // error — invariant 5) while `daemon_to_stdout` is parked on a socket
    // read rather than on channel admission, this arm is what makes that
    // failure observable and ends the whole loop instead of leaving the
    // bridge reading a socket forever on a dead output.
    let which = tokio::select! {
        _ = stdin_to_daemon => {
            log::info!("stdin→daemon ended, shutting down bridge");
            ForwardEnd::StdinEnded
        }
        _ = daemon_to_stdout => {
            log::info!("daemon→stdout ended, shutting down bridge");
            ForwardEnd::StdoutEnded
        }
        join_result = &mut writer_handle => {
            match join_result {
                Ok(()) => log::info!("stdout writer ended, shutting down bridge"),
                Err(e) => log::warn!("stdout writer task panicked: {}", e),
            }
            ForwardEnd::WriterEnded
        }
    };

    // Quiesce the writer before returning (invariant 6/7): stop admitting
    // (drop the sender) and give the writer a bounded chance to drain
    // whatever it already has queued, so no queued frame can interleave
    // with or follow whatever the caller writes to stdout next (the
    // synthetic Detached in `finish_bridge_exit`, or a fresh writer for
    // the next `forward_loop` call after a reconnect). If the writer
    // already ended on its own (`ForwardEnd::WriterEnded`), it has nothing
    // left to drain and re-awaiting the handle would just be a no-op wait.
    drop(stdout_tx);
    if !matches!(which, ForwardEnd::WriterEnded) {
        match tokio::time::timeout(STDOUT_WRITER_QUIESCE_TIMEOUT, writer_handle).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("stdout writer task ended abnormally: {}", e),
            Err(_) => log::warn!(
                "stdout writer did not quiesce within {:?}; abandoning it",
                STDOUT_WRITER_QUIESCE_TIMEOUT
            ),
        }
    }

    conclude_connection(announced.load(Ordering::Acquire))
}

/// Write the synthetic Detached message, flush, restore the terminal, and
/// exit the process — today's unconditional end-of-bridge behaviour. Unix
/// calls this only once reconnecting is not attempted or has been
/// exhausted; Windows calls it unconditionally (reconnect-after-upgrade is
/// Unix-only).
pub(in crate::mux::bridge) fn finish_bridge_exit(transport: &Arc<AtomicU8>) -> ! {
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
pub(in crate::mux::bridge) const RECONNECT_INITIAL_BACKOFF: std::time::Duration =
    std::time::Duration::from_millis(50);
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
pub(in crate::mux::bridge) async fn reconnect_with_backoff(
    sock_path: &std::path::Path,
) -> Option<UnixStream> {
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
pub(in crate::mux::bridge) async fn reconnect_and_reattach(
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
