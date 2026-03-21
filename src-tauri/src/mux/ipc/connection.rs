//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use portable_pty::MasterPty;
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::ring_buffer::DetachRingBuffer;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    MuxPane, PaneId, PaneOutputTarget, PtyOutputChunk, SharedOutputTarget,
};

/// Handshake timeout: client must send Hello within this duration.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle a new client connection through handshake and message loop.
pub async fn handle_connection(stream: UnixStream, session_manager: Arc<Mutex<SessionManager>>) {
    let mut framed = Framed::new(stream, MuxCodec::new());

    // Wait for Hello with timeout to prevent idle connection DoS
    let hello_result = tokio::time::timeout(HANDSHAKE_TIMEOUT, framed.next()).await;
    let hello = match hello_result {
        Ok(Some(Ok(msg))) if msg.msg_type == MessageType::Hello => {
            match msg.decode_payload::<HelloMsg>() {
                Some(h) => h,
                None => {
                    log::warn!("Invalid Hello payload");
                    return;
                }
            }
        }
        Ok(_) => {
            log::warn!("Expected Hello message, disconnecting");
            return;
        }
        Err(_) => {
            log::warn!("Handshake timeout, disconnecting");
            return;
        }
    };

    // Validate protocol version
    if hello.protocol_version != PROTOCOL_VERSION {
        let reject = WelcomeMsg::Rejected {
            reason: format!(
                "Protocol version mismatch: client={}, server={}",
                hello.protocol_version, PROTOCOL_VERSION
            ),
        };
        let msg = MuxMessage::control(MessageType::Welcome, 0, &reject);
        let _ = framed.send(msg).await;
        return;
    }

    // Send Welcome with session list, auto-creating default session if none exist
    let welcome = {
        let mut mgr = session_manager.lock().await;
        if mgr.is_empty() {
            mgr.create_session("default".to_string());
        }
        WelcomeMsg::Accepted {
            server_version: PROTOCOL_VERSION,
            sessions: mgr.session_list(),
        }
    };
    let msg = MuxMessage::control(MessageType::Welcome, 0, &welcome);
    if framed.send(msg).await.is_err() {
        return;
    }

    log::info!(
        "Client connected: {:?}, protocol v{}",
        hello.client_type,
        hello.protocol_version
    );

    // Shared channel: all pane reader threads send output here,
    // and the select! loop forwards it to the client.
    let (pane_output_tx, mut pane_output_rx) = mpsc::channel::<PtyOutputChunk>(1024);

    // Reattach existing panes: replay buffered output and reconnect output targets
    {
        let reattach_data = {
            let mgr = session_manager.lock().await;
            let mut data: Vec<(PaneId, Vec<u8>)> = Vec::new();
            for session in mgr.sessions_iter() {
                for window in session.windows.values() {
                    for pane in window.panes.values() {
                        if pane.exited {
                            continue;
                        }
                        let mut target = pane.output_target.lock().unwrap();
                        let buffered = if let PaneOutputTarget::Detached(ref mut ring) = *target {
                            let buf = ring.read_all();
                            ring.clear();
                            buf
                        } else {
                            Vec::new()
                        };
                        // Switch to connected mode with the new channel
                        *target = PaneOutputTarget::Connected(pane_output_tx.clone());
                        data.push((pane.id, buffered));
                    }
                }
            }
            data
        };
        // Send reattach messages outside the session manager lock
        for (pane_id, buffered) in &reattach_data {
            let resp = MuxMessage::control(MessageType::PaneCreated, *pane_id, pane_id);
            if framed.send(resp).await.is_err() {
                return;
            }
            if !buffered.is_empty() {
                let msg = MuxMessage::pty_output(*pane_id, buffered.clone());
                if framed.send(msg).await.is_err() {
                    return;
                }
            }
        }
        if !reattach_data.is_empty() {
            log::info!("Reattached {} existing pane(s)", reattach_data.len());
        }
    }

    // Message + output loop using select! to handle both directions concurrently
    loop {
        tokio::select! {
            msg = framed.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Err(should_break) = route_message(
                            msg,
                            &session_manager,
                            &mut framed,
                            &pane_output_tx,
                        ).await {
                            if should_break {
                                break;
                            }
                        }
                    }
                    Some(Err(e)) => {
                        log::warn!("Connection error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            chunk = pane_output_rx.recv() => {
                if let Some(chunk) = chunk {
                    if chunk.data.is_empty() {
                        // Empty chunk = PTY exited signal
                        log::info!("PTY exited for pane {}", chunk.pane_id);
                        let exit_msg = PtyExitedMsg { exit_code: Some(0) };
                        let msg = MuxMessage::control(MessageType::PtyExited, chunk.pane_id, &exit_msg);
                        if framed.send(msg).await.is_err() {
                            break;
                        }
                    } else {
                        let msg = MuxMessage::pty_output(chunk.pane_id, chunk.data);
                        if framed.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    log::info!("Client disconnected");
}

/// Route a single message to the appropriate handler.
///
/// Returns `Err(true)` when the connection should be closed,
/// `Err(false)` on a non-fatal send error, and `Ok(())` otherwise.
async fn route_message(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<UnixStream, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Result<(), bool> {
    match msg.msg_type {
        MessageType::CreateWindow => {
            handle_create_window(session_manager, framed, pane_output_tx).await?;
        }
        MessageType::SplitPane => {
            handle_split_pane(msg, session_manager, framed, pane_output_tx).await?;
        }
        MessageType::Detach => {
            log::info!("Client requested detach");
            let resp = MuxMessage::control(MessageType::Detached, 0, &());
            let _ = framed.send(resp).await;
            return Err(true);
        }
        MessageType::DestroyPane => {
            handle_destroy_pane(msg.pane_id, session_manager).await;
        }
        MessageType::SwitchWindow => {
            log::info!("SwitchWindow requested");
            // TODO: track active window per client
        }
        MessageType::RenameWindow => {
            log::info!("RenameWindow requested");
            // TODO: decode payload for new name
        }
        MessageType::DestroyWindow => {
            log::info!("DestroyWindow requested");
            // TODO: destroy window and its panes
        }
        MessageType::Resize => {
            handle_resize(msg, session_manager).await;
        }
        MessageType::PtyInput => {
            let pane_id = msg.pane_id;
            let mgr = session_manager.lock().await;
            if let Some((session_id, window_id)) = mgr.find_pane(pane_id) {
                if let Some(session) = mgr.get_session(session_id) {
                    if let Some(window) = session.windows.get(&window_id) {
                        if let Some(pane) = window.panes.get(&pane_id) {
                            if let Err(e) = pane.write_input(&msg.payload) {
                                log::warn!("Failed to write to pane {}: {}", pane_id, e);
                            }
                        }
                    }
                }
            }
        }
        _ => {
            log::debug!(
                "Unhandled {:?} for pane {} ({} bytes)",
                msg.msg_type,
                msg.pane_id,
                msg.payload.len()
            );
        }
    }
    Ok(())
}

/// Result of spawning a PTY with shell process.
struct SpawnedPty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn std::io::Write + Send>,
    reader: Box<dyn std::io::Read + Send>,
}

/// Spawn a PTY with a shell process at the given size.
fn spawn_pty(cols: u16, rows: u16) -> Result<SpawnedPty, String> {
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
fn register_pane_and_start_reader(
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
    window.add_pane(pane);

    let reader = spawned.reader;
    std::thread::spawn(move || {
        pty_reader_loop(pane_id, reader, output_target);
    });

    Some(pane_id)
}

/// Spawn a PTY, create a pane, and start a reader thread for output streaming.
async fn handle_create_window(
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<UnixStream, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Result<(), bool> {
    let spawned = match spawn_pty(80, 24) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{}", e);
            return Ok(());
        }
    };

    let mut mgr = session_manager.lock().await;
    let window_id = match mgr.create_window(1, "shell".to_string()) {
        Some(id) => id,
        None => {
            log::error!("Failed to create window in session 1");
            return Ok(());
        }
    };

    let pane_id = match register_pane_and_start_reader(
        &mut mgr,
        1,
        window_id,
        80,
        24,
        spawned,
        pane_output_tx,
    ) {
        Some(id) => id,
        None => {
            log::error!("Failed to register pane in window {}", window_id);
            return Ok(());
        }
    };

    drop(mgr);

    log::info!(
        "Created window {} with pane {} (PTY spawned)",
        window_id,
        pane_id
    );

    let resp = MuxMessage::control(MessageType::PaneCreated, pane_id, &pane_id);
    if framed.send(resp).await.is_err() {
        return Err(true);
    }

    Ok(())
}

/// Split an existing pane by spawning a new PTY in the same window.
async fn handle_split_pane(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<UnixStream, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Result<(), bool> {
    let _split_msg: SplitPaneMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid SplitPane payload");
            return Ok(());
        }
    };

    let source_pane_id = msg.pane_id;
    log::info!("SplitPane requested for pane {}", source_pane_id);

    // Find which session/window contains the source pane
    let (session_id, window_id, cols, rows) = {
        let mgr = session_manager.lock().await;
        match mgr.find_pane(source_pane_id) {
            Some((sid, wid)) => {
                let session = mgr.get_session(sid).unwrap();
                let window = session.windows.get(&wid).unwrap();
                let pane = window.panes.get(&source_pane_id).unwrap();
                (sid, wid, pane.cols, pane.rows)
            }
            None => {
                log::warn!("SplitPane: pane {} not found", source_pane_id);
                return Ok(());
            }
        }
    };

    let spawned = match spawn_pty(cols, rows) {
        Ok(s) => s,
        Err(e) => {
            log::error!("SplitPane: {}", e);
            return Ok(());
        }
    };

    let mut mgr = session_manager.lock().await;
    let new_pane_id = match register_pane_and_start_reader(
        &mut mgr,
        session_id,
        window_id,
        cols,
        rows,
        spawned,
        pane_output_tx,
    ) {
        Some(id) => id,
        None => {
            log::error!("SplitPane: failed to register pane in window {}", window_id);
            return Ok(());
        }
    };

    drop(mgr);

    log::info!(
        "Split pane {}: created new pane {} in window {}",
        source_pane_id,
        new_pane_id,
        window_id
    );

    let resp = MuxMessage::control(MessageType::PaneCreated, new_pane_id, &new_pane_id);
    if framed.send(resp).await.is_err() {
        return Err(true);
    }

    Ok(())
}

/// Destroy a pane, removing it from its window. Cleans up empty windows and sessions.
async fn handle_destroy_pane(pane_id: PaneId, session_manager: &Arc<Mutex<SessionManager>>) {
    log::info!("DestroyPane requested for pane {}", pane_id);

    let mut mgr = session_manager.lock().await;
    let (session_id, window_id) = match mgr.find_pane(pane_id) {
        Some(ids) => ids,
        None => {
            log::warn!("DestroyPane: pane {} not found", pane_id);
            return;
        }
    };

    // Remove pane from window (drops writer/master, closing PTY)
    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            if let Some(mut pane) = window.remove_pane(pane_id) {
                pane.mark_exited();
                log::info!("Destroyed pane {}", pane_id);
            }

            if window.is_empty() {
                session.remove_window(window_id);
                log::info!(
                    "Removed empty window {} from session {}",
                    window_id,
                    session_id
                );

                if session.is_empty() {
                    mgr.remove_session(session_id);
                    log::info!("Removed empty session {}", session_id);

                    if mgr.is_empty() {
                        log::info!("All sessions empty, daemon may exit");
                    }
                }
            }
        }
    }
}

/// Resize a pane's PTY to the requested dimensions.
async fn handle_resize(msg: MuxMessage, session_manager: &Arc<Mutex<SessionManager>>) {
    let resize_msg: ResizeMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid Resize payload");
            return;
        }
    };

    let pane_id = msg.pane_id;
    let mut mgr = session_manager.lock().await;
    let (session_id, window_id) = match mgr.find_pane(pane_id) {
        Some(ids) => ids,
        None => {
            log::warn!("Resize: pane {} not found", pane_id);
            return;
        }
    };

    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            if let Some(pane) = window.panes.get_mut(&pane_id) {
                if let Err(e) = pane.resize(resize_msg.cols, resize_msg.rows) {
                    log::warn!("Resize pane {}: {}", pane_id, e);
                } else {
                    log::info!(
                        "Resized pane {} to {}x{}",
                        pane_id,
                        resize_msg.cols,
                        resize_msg.rows
                    );
                }
            }
        }
    }
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
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                log::info!("PTY reader EOF for pane {}", pane_id);
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
                let data = buf[..n].to_vec();
                let mut target = output_target.lock().unwrap();
                match &mut *target {
                    PaneOutputTarget::Connected(tx) => {
                        let chunk = PtyOutputChunk {
                            pane_id,
                            data: data.clone(),
                        };
                        if tx.blocking_send(chunk).is_err() {
                            // Channel closed — GUI disconnected, switch to buffering
                            log::info!("Pane {} switching to detached buffering mode", pane_id);
                            let mut ring = DetachRingBuffer::new(
                                crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                            );
                            ring.write(&data);
                            *target = PaneOutputTarget::Detached(ring);
                        }
                    }
                    PaneOutputTarget::Detached(ring) => {
                        ring.write(&data);
                    }
                }
            }
            Err(e) => {
                log::info!("PTY reader error for pane {}: {}", pane_id, e);
                break;
            }
        }
    }
}
