//! PTY spawning and reader loop for mux panes.

use std::io::Read;
use std::sync::Arc;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::ring_buffer::DetachRingBuffer;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    MuxPane, PaneId, PaneOutputTarget, PtyOutputChunk, SharedOutputTarget, SharedShadowParser,
};

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

    let shell = crate::pty::detect_default_shell();
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
pub(super) fn register_pane_and_start_reader(
    mgr: &mut SessionManager,
    session_id: u32,
    window_id: u32,
    cols: u16,
    rows: u16,
    spawned: SpawnedPty,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
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
    window.add_pane(pane);

    let reader = spawned.reader;
    std::thread::spawn(move || {
        pty_reader_loop(pane_id, reader, output_target, shadow_parser);
    });

    Some(pane_id)
}

/// Read PTY output in a blocking loop and forward to the output target.
/// Runs in a dedicated std::thread since PTY reads are blocking I/O.
///
/// When the connected channel fails (GUI disconnected), the reader automatically
/// switches to buffering mode using a ring buffer. The reader thread stays alive
/// so the PTY process output is never lost.
fn pty_reader_loop(
    pane_id: u32,
    mut reader: Box<dyn Read + Send>,
    output_target: SharedOutputTarget,
    shadow_parser: SharedShadowParser,
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
                // Feed shadow parser for screen state tracking (for restoration on reattach)
                shadow_parser.lock().unwrap().process(data);

                // Lock briefly to try non-blocking send or clone the sender.
                // IMPORTANT: release lock before blocking_send to avoid deadlock
                // with session_manager lock held by collect_reattach_data.
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
                                    // Channel closed — switch to detached
                                    let mut ring = DetachRingBuffer::new(
                                        crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                                    );
                                    ring.write(data);
                                    *target = PaneOutputTarget::Detached(ring);
                                    Some(Err(()))
                                }
                            }
                        }
                        PaneOutputTarget::Detached(ring) => {
                            ring.write(data);
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
