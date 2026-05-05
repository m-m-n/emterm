//! PTY spawning and reader loop for mux panes.

use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::ring_buffer::DetachRingBuffer;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    MuxPane, PaneId, PaneOutputTarget, PtyOutputChunk, SharedOutputTarget, SharedShadowParser,
    SharedTitleSender, TitleChangeSender,
};
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::RawPassthroughBuffer;

/// Shared per-pane raw passthrough buffer (image / Markdown OSC bytes
/// captured while detached or hidden). Drained into the resume snapshot.
type SharedRawPassthrough = Arc<StdMutex<RawPassthroughBuffer>>;

/// Shared per-pane stateful passthrough scanner. Lives outside the buffer
/// so partial sequences spanning chunk boundaries are recovered.
type SharedPassthroughScanner = Arc<StdMutex<PassthroughScanner>>;

/// Detect the default shell for the current platform.
fn detect_default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }

    #[cfg(windows)]
    {
        "powershell.exe".to_string()
    }
}

/// Result of spawning a PTY with shell process.
pub(super) struct SpawnedPty {
    pub(super) master: Box<dyn MasterPty + Send>,
    pub(super) writer: Box<dyn std::io::Write + Send>,
    pub(super) reader: Box<dyn std::io::Read + Send>,
}

/// Spawn a PTY with a shell process at the given size.
pub(super) fn spawn_pty(cols: u16, rows: u16) -> Result<SpawnedPty, String> {
    let pty_system = portable_pty::native_pty_system();
    let pty_size = portable_pty::PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(pty_size)
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let shell = detect_default_shell();
    let mut cmd = portable_pty::CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "emterm");
    cmd.env("EMTERM_MUX", "1");
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");

    #[cfg(unix)]
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(&home);
    }
    #[cfg(windows)]
    if let Ok(home) = std::env::var("USERPROFILE") {
        cmd.cwd(&home);
    }

    pair.slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to take PTY writer: {}", e))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

    Ok(SpawnedPty {
        master: pair.master,
        writer,
        reader,
    })
}

/// Register a new pane in the session manager and start its reader thread.
///
/// Returns the new pane_id and its output target (for the reader thread).
#[allow(clippy::too_many_arguments)]
pub(super) fn register_pane_and_start_reader(
    mgr: &mut SessionManager,
    session_id: u32,
    window_id: u32,
    cols: u16,
    rows: u16,
    spawned: SpawnedPty,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    title_tx: &TitleChangeSender,
) -> Option<PaneId> {
    // Verify session/window exist before allocating pane ID
    {
        let session = mgr.get_session(session_id)?;
        session.windows.get(&window_id)?;
    }

    let pane_id = mgr.alloc_pane_id();
    let session = mgr.get_session_mut(session_id)?;
    let window = session.windows.get_mut(&window_id)?;

    let output_target: SharedOutputTarget = Arc::new(std::sync::Mutex::new(
        PaneOutputTarget::Connected(pane_output_tx.clone()),
    ));
    let pane = MuxPane::new(
        pane_id,
        cols,
        rows,
        output_target.clone(),
        spawned.writer,
        spawned.master,
    );
    let shadow_parser = pane.shadow_parser.clone();
    let pane_cwd = pane.cwd.clone();
    let pane_title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    // Store initial title_tx in the swappable sender (reattach will swap in a new one)
    *title_sender.lock().unwrap() = Some(title_tx.clone());
    window.add_pane(pane);

    let reader = spawned.reader;
    std::thread::spawn(move || {
        pty_reader_loop(
            pane_id,
            reader,
            output_target,
            shadow_parser,
            pane_cwd,
            pane_title,
            title_sender,
            raw_passthrough,
            passthrough_scanner,
        );
    });

    Some(pane_id)
}

/// Read PTY output in a blocking loop and forward to the output target.
/// Runs in a dedicated std::thread since PTY reads are blocking I/O.
///
/// When the connected channel fails (GUI disconnected), the reader automatically
/// switches to buffering mode using a ring buffer. The reader thread stays alive
/// so the PTY process output is never lost.
#[allow(clippy::too_many_arguments)]
fn pty_reader_loop(
    pane_id: u32,
    mut reader: Box<dyn Read + Send>,
    output_target: SharedOutputTarget,
    shadow_parser: SharedShadowParser,
    pane_cwd: Arc<std::sync::Mutex<Option<String>>>,
    last_title: Arc<std::sync::Mutex<Option<String>>>,
    title_sender: SharedTitleSender,
    raw_passthrough: SharedRawPassthrough,
    passthrough_scanner: SharedPassthroughScanner,
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let target_state = {
                    let t = output_target.lock().unwrap();
                    match &*t {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached(_) => "Detached",
                    }
                };
                log::info!(
                    "PTY reader EOF for pane {} (output_target={})",
                    pane_id,
                    target_state
                );
                // Signal exit to connected client if any
                let target = output_target.lock().unwrap();
                if let PaneOutputTarget::Connected(ref tx) = *target {
                    let _ = tx.blocking_send(PtyOutputChunk {
                        pane_id,
                        data: Vec::new(),
                    });
                }
                break;
            }
            Ok(n) => {
                let data = &buf[..n];
                // Feed shadow parser and detect OSC title in a single lock scope
                let title_changed = {
                    let mut parser = shadow_parser.lock().unwrap();
                    parser.process(data);
                    let new_title = parser.screen().title();
                    if new_title.is_empty() {
                        None
                    } else {
                        let mut current = last_title.lock().unwrap();
                        if Some(new_title) != current.as_deref() {
                            let owned = new_title.to_string();
                            *current = Some(owned.clone());
                            Some(owned)
                        } else {
                            None
                        }
                    }
                };
                if let Some(new_title) = title_changed {
                    if let Some(tx) = title_sender.lock().unwrap().as_ref() {
                        let _ = tx.try_send((pane_id, new_title));
                    }
                }

                // Detect OSC 7 (cwd reporting) and cache the path
                if let Some(cwd) = crate::mux::ipc::statusbar::detect_osc7_cwd(data) {
                    *pane_cwd.lock().unwrap() = Some(cwd);
                }

                // Lock briefly to try non-blocking send or clone the sender.
                // IMPORTANT: release lock before blocking_send to avoid deadlock
                // with session_manager lock held by collect_reattach_data.
                //
                // The Detached arms also feed `passthrough_scanner` so that
                // image / Markdown OSC byte runs survive a hidden / network
                // detach window and can be replayed via the resume snapshot.
                let send_result = {
                    let mut target = output_target.lock().unwrap();
                    match &mut *target {
                        PaneOutputTarget::Connected(tx) => {
                            // Single allocation: data owned by PtyOutputChunk
                            let chunk = PtyOutputChunk {
                                pane_id,
                                data: data.to_vec(),
                            };
                            match tx.try_send(chunk) {
                                Ok(()) => None, // sent successfully
                                Err(mpsc::error::TrySendError::Full(chunk)) => {
                                    // Channel full — need blocking send outside lock
                                    Some(Ok((tx.clone(), chunk)))
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    // Channel closed — switch to detached and
                                    // capture passthrough bytes from this chunk.
                                    let mut ring = DetachRingBuffer::new(
                                        crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                                    );
                                    ring.write(data);
                                    capture_passthrough(
                                        pane_id,
                                        data,
                                        &raw_passthrough,
                                        &passthrough_scanner,
                                    );
                                    *target = PaneOutputTarget::Detached(ring);
                                    Some(Err(()))
                                }
                            }
                        }
                        PaneOutputTarget::Detached(ring) => {
                            ring.write(data);
                            capture_passthrough(
                                pane_id,
                                data,
                                &raw_passthrough,
                                &passthrough_scanner,
                            );
                            None
                        }
                    }
                }; // output_target lock released here

                // Handle backpressure outside the lock to avoid deadlock
                if let Some(Ok((tx, chunk))) = send_result {
                    log::debug!("Pane {} backpressure: channel full, blocking", pane_id);
                    if tx.blocking_send(chunk).is_err() {
                        log::info!("Pane {} switching to detached buffering mode", pane_id);
                        let mut target = output_target.lock().unwrap();
                        let mut ring =
                            DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
                        ring.write(data);
                        capture_passthrough(pane_id, data, &raw_passthrough, &passthrough_scanner);
                        *target = PaneOutputTarget::Detached(ring);
                    }
                }
            }
            Err(e) => {
                log::info!(
                    "PTY reader error for pane {}: {} (kind={:?})",
                    pane_id,
                    e,
                    e.kind()
                );
                break;
            }
        }
    }
}

/// Run `data` through the per-pane passthrough scanner and append any
/// completed image / Markdown OSC sequences to the per-pane raw buffer.
///
/// Called from the Detached arms of `pty_reader_loop`. Logs a single warn
/// when the buffer drops the oldest captured bytes due to capacity overflow.
fn capture_passthrough(
    pane_id: PaneId,
    data: &[u8],
    raw_passthrough: &SharedRawPassthrough,
    passthrough_scanner: &SharedPassthroughScanner,
) {
    let extracted = passthrough_scanner.lock().unwrap().process(data);
    if extracted.is_empty() {
        return;
    }
    let dropped = raw_passthrough.lock().unwrap().append(&extracted);
    if dropped {
        log::warn!(
            "[WARN][BACKEND] mux pane {} raw_passthrough capacity exceeded; oldest captured bytes dropped",
            pane_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::visibility::HIDDEN_PASSTHROUGH_CAPACITY_MUX;

    fn shared_buffer() -> (SharedRawPassthrough, SharedPassthroughScanner) {
        (
            Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            Arc::new(StdMutex::new(PassthroughScanner::new())),
        )
    }

    /// TS-19: passthrough sequences feed raw_passthrough while detached.
    #[test]
    fn capture_passthrough_appends_completed_kitty_apc() {
        let (buf, scanner) = shared_buffer();
        capture_passthrough(7, b"\x1b_Gi=1;ZZ\x1b\\", &buf, &scanner);
        let stored = buf.lock().unwrap().read_all();
        assert!(
            stored
                .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
                .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
            "captured bytes must contain the original Kitty APC sequence"
        );
    }

    /// TS-19: a sequence split across two chunks is still recovered because
    /// the scanner is stateful and shared.
    #[test]
    fn capture_passthrough_handles_chunk_boundary() {
        let (buf, scanner) = shared_buffer();
        capture_passthrough(7, b"\x1b_Gi=1;Z", &buf, &scanner);
        // Mid-sequence: nothing complete yet.
        assert_eq!(buf.lock().unwrap().len(), 0);
        capture_passthrough(7, b"Z\x1b\\", &buf, &scanner);
        let stored = buf.lock().unwrap().read_all();
        assert!(
            stored
                .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
                .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
            "chunk-split sequence must be reassembled"
        );
    }

    /// Plain output that contains no image / Markdown OSC must not touch
    /// the raw buffer.
    #[test]
    fn capture_passthrough_ignores_plain_text() {
        let (buf, scanner) = shared_buffer();
        capture_passthrough(7, b"hello world\n", &buf, &scanner);
        assert_eq!(buf.lock().unwrap().len(), 0);
    }
}
