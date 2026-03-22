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
pub async fn handle_connection(
    stream: UnixStream,
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) {
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

    // CLI clients only need the session list from Welcome.
    // Skip reattach and message loop to avoid stealing panes from GUI.
    if hello.client_type == ClientType::Cli {
        log::info!("CLI client served, disconnecting");
        return;
    }

    // Determine active session: use first session (auto-created "default")
    let mut active_session_id: u32 = {
        let mgr = session_manager.lock().await;
        mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1)
    };

    // Shared channel: all pane reader threads send output here,
    // and the select! loop forwards it to the client.
    let (pane_output_tx, mut pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    // NOTE: Reattach data is NOT sent here. The client must send an Attach
    // message after its output stream is ready. This eliminates the timing
    // dependency where reattach data could arrive before the client is listening.

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
                            &mut active_session_id,
                            &shutdown_tx,
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

    // Switch all panes in the active session to detached buffering mode.
    // This prevents pty_reader_loop from racing with the next connection's
    // collect_reattach_data when the output_target is still Connected(dead_tx).
    detach_session_panes(&session_manager, active_session_id).await;

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
    active_session_id: &mut u32,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) -> Result<(), bool> {
    match msg.msg_type {
        MessageType::CreateWindow => {
            handle_create_window(session_manager, framed, pane_output_tx, *active_session_id)
                .await?;
        }
        MessageType::Attach => {
            handle_attach(
                msg,
                session_manager,
                framed,
                pane_output_tx,
                active_session_id,
            )
            .await?;
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
            handle_destroy_pane(msg.pane_id, session_manager, shutdown_tx).await;
        }
        MessageType::SwitchWindow => {
            log::info!("SwitchWindow requested: window {}", msg.pane_id);
        }
        MessageType::RenameWindow => {
            handle_rename_window(msg, session_manager).await;
        }
        MessageType::DestroyWindow => {
            handle_destroy_window(msg.pane_id, session_manager, shutdown_tx).await;
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
    active_session_id: u32,
) -> Result<(), bool> {
    let spawned = match spawn_pty(80, 24) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{}", e);
            return Ok(());
        }
    };

    let mut mgr = session_manager.lock().await;
    let window_id = match mgr.create_window(active_session_id, "shell".to_string()) {
        Some(id) => id,
        None => {
            log::error!("Failed to create window in session {}", active_session_id);
            return Ok(());
        }
    };

    let pane_id = match register_pane_and_start_reader(
        &mut mgr,
        active_session_id,
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
/// Signals daemon shutdown when all sessions become empty.
async fn handle_destroy_pane(
    pane_id: PaneId,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) {
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
                        log::info!("All sessions empty, daemon shutting down");
                        let _ = shutdown_tx.send(true);
                    }
                }
            }
        }
    }
}

/// Rename a window, decoding the new name from the message payload.
async fn handle_rename_window(msg: MuxMessage, session_manager: &Arc<Mutex<SessionManager>>) {
    let rename_msg: RenameWindowMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid RenameWindow payload");
            return;
        }
    };
    let window_id = msg.pane_id;
    log::info!(
        "RenameWindow: window {} -> '{}'",
        window_id,
        rename_msg.name
    );

    let mut mgr = session_manager.lock().await;
    let session_id = mgr.find_window_session(window_id);
    match session_id {
        Some(sid) => {
            mgr.rename_window(sid, window_id, rename_msg.name);
        }
        None => {
            log::warn!("RenameWindow: window {} not found", window_id);
        }
    }
}

/// Destroy a window and all its panes, cleaning up empty sessions.
/// Signals daemon shutdown when all sessions become empty.
async fn handle_destroy_window(
    window_id: u32,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) {
    log::info!("DestroyWindow requested for window {}", window_id);

    let mut mgr = session_manager.lock().await;

    let session_id = match mgr.find_window_session(window_id) {
        Some(id) => id,
        None => {
            log::warn!("DestroyWindow: window {} not found", window_id);
            return;
        }
    };

    // Mark all panes in the window as exited before removal
    if let Some(session) = mgr.get_session_mut(session_id) {
        if let Some(window) = session.windows.get_mut(&window_id) {
            for pane in window.panes.values_mut() {
                pane.mark_exited();
            }
        }
    }

    if let Some(session_empty) = mgr.remove_window(session_id, window_id) {
        log::info!("Removed window {} from session {}", window_id, session_id);
        if session_empty {
            mgr.remove_session(session_id);
            log::info!("Removed empty session {}", session_id);
            if mgr.is_empty() {
                log::info!("All sessions empty, daemon shutting down");
                let _ = shutdown_tx.send(true);
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

/// Collect reattach data for panes in the given session.
///
/// Drains buffered output from detached panes and switches them to connected mode.
async fn collect_reattach_data(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Vec<(PaneId, Vec<u8>)> {
    let mgr = session_manager.lock().await;
    let mut data: Vec<(PaneId, Vec<u8>)> = Vec::new();
    if let Some(session) = mgr.get_session(session_id) {
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
                *target = PaneOutputTarget::Connected(pane_output_tx.clone());
                data.push((pane.id, buffered));
            }
        }
    }
    data
}

/// Send reattach data (PaneCreated + buffered output) to the client.
async fn send_reattach_data(
    framed: &mut Framed<UnixStream, MuxCodec>,
    reattach_data: &[(PaneId, Vec<u8>)],
) -> Result<(), ()> {
    for (pane_id, buffered) in reattach_data {
        let resp = MuxMessage::control(MessageType::PaneCreated, *pane_id, pane_id);
        if framed.send(resp).await.is_err() {
            return Err(());
        }
        if !buffered.is_empty() {
            let msg = MuxMessage::pty_output(*pane_id, buffered.clone());
            if framed.send(msg).await.is_err() {
                return Err(());
            }
        }
    }
    Ok(())
}

/// Switch panes in a session to detached buffering mode.
async fn detach_session_panes(session_manager: &Arc<Mutex<SessionManager>>, session_id: u32) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        for window in session.windows.values() {
            for pane in window.panes.values() {
                if pane.exited {
                    continue;
                }
                let mut target = pane.output_target.lock().unwrap();
                if let PaneOutputTarget::Connected(_) = &*target {
                    *target = PaneOutputTarget::Detached(DetachRingBuffer::new(
                        crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                    ));
                }
            }
        }
    }
}

/// Handle Attach message: switch the client to a different session.
///
/// Detaches panes from the current session, updates the active session,
/// and reattaches panes from the new session with buffered output replay.
async fn handle_attach(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<UnixStream, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
) -> Result<(), bool> {
    let attach_msg: AttachMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid Attach payload");
            return Ok(());
        }
    };

    let new_session_id = attach_msg.session_id;
    log::info!("Client attaching to session {}", new_session_id);

    // Verify session exists
    {
        let mgr = session_manager.lock().await;
        if mgr.get_session(new_session_id).is_none() {
            log::warn!("Attach: session {} not found", new_session_id);
            let err = ErrorMsg {
                message: format!("Session {} not found", new_session_id),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
            return Ok(());
        }
    }

    // Detach from current session
    detach_session_panes(session_manager, *active_session_id).await;

    // Update active session
    *active_session_id = new_session_id;

    // Reattach to new session's panes
    let reattach_data =
        collect_reattach_data(session_manager, new_session_id, pane_output_tx).await;

    if send_reattach_data(framed, &reattach_data).await.is_err() {
        return Err(true);
    }

    log::info!(
        "Attached to session {} with {} pane(s)",
        new_session_id,
        reattach_data.len()
    );
    Ok(())
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
                // Lock briefly to try non-blocking send or clone the sender.
                // IMPORTANT: release lock before blocking_send to avoid deadlock
                // with session_manager lock held by collect_reattach_data.
                let send_result = {
                    let mut target = output_target.lock().unwrap();
                    match &mut *target {
                        PaneOutputTarget::Connected(tx) => {
                            let chunk = PtyOutputChunk {
                                pane_id,
                                data: data.clone(),
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
                                    ring.write(&data);
                                    *target = PaneOutputTarget::Detached(ring);
                                    Some(Err(()))
                                }
                            }
                        }
                        PaneOutputTarget::Detached(ring) => {
                            ring.write(&data);
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
                        let mut ring = DetachRingBuffer::new(
                            crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                        );
                        ring.write(&data);
                        *target = PaneOutputTarget::Detached(ring);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
    use std::sync::Mutex as StdMutex;

    fn make_test_pane_with_target(
        id: u32,
        output_target: SharedOutputTarget,
    ) -> MuxPane {
        MuxPane::new_test(id, 80, 24, output_target)
    }

    /// Test: collect_reattach_data returns entries for 2 panes in 2 windows.
    /// Simulates the reattach scenario where both panes are in Connected(dead_tx) state.
    #[tokio::test]
    async fn test_collect_reattach_data_two_windows_connected_dead() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        // Set up: 1 session, 2 windows, each with 1 pane (Connected to dead channels)
        let (dead_tx1, _dead_rx1) = mpsc::channel::<PtyOutputChunk>(1);
        let (dead_tx2, _dead_rx2) = mpsc::channel::<PtyOutputChunk>(1);

        // Drop receivers to simulate dead channels
        drop(_dead_rx1);
        drop(_dead_rx2);

        let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx1)));
        let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx2)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            let pane2 = make_test_pane_with_target(2, target2);

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        // Verify session has 2 panes
        {
            let m = mgr.lock().await;
            let session = m.get_session(session_id).unwrap();
            assert_eq!(session.pane_count(), 2, "Session should have 2 panes");
            assert_eq!(session.window_count(), 2, "Session should have 2 windows");
        }

        // Create new channel for reattach
        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);

        // Call collect_reattach_data
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

        // CRITICAL: Must return 2 entries
        assert_eq!(data.len(), 2, "collect_reattach_data should return 2 entries for 2 panes");

        // Verify pane IDs
        let mut pane_ids: Vec<u32> = data.iter().map(|(id, _)| *id).collect();
        pane_ids.sort();
        assert_eq!(pane_ids, vec![1, 2], "Should contain pane IDs 1 and 2");

        // Verify both have empty buffers (were Connected, not Detached)
        for (_, buf) in &data {
            assert!(buf.is_empty(), "Connected panes should have empty buffers");
        }
    }

    /// Test: collect_reattach_data returns entries for panes in Detached state.
    #[tokio::test]
    async fn test_collect_reattach_data_two_windows_detached() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let mut ring1 = DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
        ring1.write(b"hello from pane 1");
        let mut ring2 = DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
        ring2.write(b"hello from pane 2");

        let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached(ring1)));
        let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached(ring2)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            let pane2 = make_test_pane_with_target(2, target2);

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

        assert_eq!(data.len(), 2, "collect_reattach_data should return 2 entries");

        // Verify both have buffered data
        for (_, buf) in &data {
            assert!(!buf.is_empty(), "Detached panes should have buffered data");
        }
    }

    /// Test: collect_reattach_data skips exited panes.
    #[tokio::test]
    async fn test_collect_reattach_data_skips_exited() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let (dead_tx, _) = mpsc::channel::<PtyOutputChunk>(1);
        let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx.clone())));
        let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            let mut pane2 = make_test_pane_with_target(2, target2);
            pane2.mark_exited(); // Mark pane 2 as exited

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

        assert_eq!(data.len(), 1, "Should only return 1 entry (pane 2 is exited)");
        assert_eq!(data[0].0, 1, "Only pane 1 should be included");
    }

    /// Test: session_list reports correct pane_count for multi-window session.
    #[tokio::test]
    async fn test_session_list_pane_count_multi_window() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        {
            let mut m = mgr.lock().await;
            let session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
            let t1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
            let t2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));

            let pane1 = make_test_pane_with_target(1, t1);
            let pane2 = make_test_pane_with_target(2, t2);

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);

            let list = m.session_list();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].pane_count, 2, "Session should report 2 panes");
            assert_eq!(list[0].window_count, 2, "Session should report 2 windows");
        }
    }
}
