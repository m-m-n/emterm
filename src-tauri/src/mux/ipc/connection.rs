//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::handlers::{
    handle_attach, handle_create_window, handle_destroy_pane, handle_destroy_window,
    handle_rename_window, handle_request_pane_snapshot, handle_resize, handle_split_pane,
    handle_switch_window,
};
use super::protocol::*;
use super::reattach::detach_session_panes;
use super::statusbar::{StatusBarEngine, execute_command};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{PtyOutputChunk, TitleChangeSender};

/// Handshake timeout: client must send Hello within this duration.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum number of PTY output chunks to drain per select! iteration.
/// Balances batch efficiency (fewer syscalls) against input responsiveness
/// (returning to select! to check for PtyInput). At 64 chunks × 65KB max
/// each, worst-case batch memory is ~4MB (transient, freed after flush).
const DRAIN_BATCH_LIMIT: usize = 64;

/// Handle a new client connection through handshake and message loop.
pub async fn handle_connection<S>(
    stream: S,
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
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

    // CLI clients: serve session list + optionally process one control message.
    // Skip reattach and full message loop to avoid stealing panes from GUI.
    if hello.client_type == ClientType::Cli {
        handle_cli_client(&mut framed, &session_manager, &shutdown_tx).await;
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

    // Channel for pane title change notifications from reader threads.
    // Bounded: title changes are idempotent (only latest matters), so dropping is safe.
    let (title_tx, mut title_rx) = mpsc::channel::<(u32, String)>(16);

    // Subscribe to cross-client notifications (e.g., CLI-triggered SwitchWindow)
    let mut notify_rx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().subscribe()
    };

    // NOTE: Reattach data is NOT sent here. The client must send an Attach
    // message after its output stream is ready. This eliminates the timing
    // dependency where reattach data could arrive before the client is listening.

    // Status bar engine setup
    let active_pane_id: super::statusbar::SharedActivePaneId =
        Arc::new(std::sync::Mutex::new(None));
    let pane_cwd_map: super::statusbar::SharedPaneCwdMap =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut statusbar_engine = StatusBarEngine::new(active_pane_id.clone(), pane_cwd_map.clone());

    // Send initial error message if settings failed to load
    if let Some(err_msg) = statusbar_engine.initial_error_update() {
        let _ = framed.send(err_msg).await;
    }

    // Set up status bar timers only if enabled and templates contain variables
    let statusbar_enabled = statusbar_engine.is_enabled();
    let mut render_interval = if statusbar_enabled && statusbar_engine.has_template_variables() {
        Some(statusbar_engine.render_interval())
    } else {
        None
    };
    let command_intervals = if statusbar_enabled {
        statusbar_engine.command_intervals()
    } else {
        Vec::new()
    };

    // Create per-command timers using mpsc channel for aggregation.
    // Each command timer runs as a separate task, sending its name when it fires.
    let (cmd_tick_tx, mut cmd_tick_rx) = mpsc::channel::<String>(16);
    for (name, dur) in command_intervals {
        // Trigger immediate first execution so status bar populates without waiting
        let _ = cmd_tick_tx.try_send(name.clone());
        let tx = cmd_tick_tx.clone();
        let cmd_name = name.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(dur);
            interval.tick().await; // skip first immediate tick (already sent above)
            loop {
                interval.tick().await;
                if tx.send(cmd_name.clone()).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(cmd_tick_tx); // Drop the original sender; spawned tasks hold clones

    // Channel for receiving command execution results from spawned tasks
    let (cmd_result_tx, mut cmd_result_rx) = mpsc::channel::<(String, Option<String>)>(16);

    // Per-command JoinHandle for single-flight control: skip if previous execution is still running
    let mut cmd_handles: std::collections::HashMap<
        String,
        tokio::task::JoinHandle<()>,
    > = std::collections::HashMap::new();

    // Message + output loop using select! to handle both directions concurrently
    loop {
        // Build a future for the render timer (if enabled)
        let render_tick = async {
            if let Some(ref mut interval) = render_interval {
                interval.tick().await;
            } else {
                // Never resolves if disabled
                std::future::pending::<()>().await;
            }
        };

        tokio::select! {
            // biased: prioritize client messages (PtyInput) over PTY output
            biased;

            msg = framed.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        // Track active pane from PtyInput and SwitchWindow messages
                        if msg.msg_type == MessageType::PtyInput
                            || msg.msg_type == MessageType::SwitchWindow
                        {
                            *active_pane_id.lock().unwrap() = Some(msg.pane_id);
                        }

                        if let Err(should_break) = route_message(
                            msg,
                            &session_manager,
                            &mut framed,
                            &pane_output_tx,
                            &mut active_session_id,
                            &shutdown_tx,
                            &mut statusbar_engine,
                            &pane_cwd_map,
                            &title_tx,
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
                if let Some(first) = chunk {
                    let batch_start = std::time::Instant::now();

                    // Drain: collect all pending chunks non-blocking (up to limit)
                    let mut chunks = vec![first];
                    while chunks.len() < DRAIN_BATCH_LIMIT {
                        match pane_output_rx.try_recv() {
                            Ok(c) => chunks.push(c),
                            Err(_) => break,
                        }
                    }
                    let drained_count = chunks.len();

                    // Merge consecutive same-pane chunks to reduce IPC frames
                    let merged = merge_consecutive_chunks(chunks);
                    let merged_count = merged.len();
                    let total_bytes: usize = merged.iter().map(|c| c.data.len()).sum();

                    if drained_count >= DRAIN_BATCH_LIMIT {
                        log::warn!(
                            "pty-batch-full: drained={} (limit hit) | merged={} | {}bytes",
                            drained_count, merged_count, total_bytes
                        );
                    } else if drained_count > 1 {
                        log::info!(
                            "pty-batch: drained={} | merged={} | {}bytes",
                            drained_count, merged_count, total_bytes
                        );
                    }

                    // Batch send: feed all into buffer, then flush once
                    let mut send_err = false;
                    for chunk in merged {
                        if chunk.data.is_empty() {
                            log::info!("PTY exited for pane {}", chunk.pane_id);
                            let exit_msg = PtyExitedMsg { exit_code: Some(0) };
                            let msg = MuxMessage::control(MessageType::PtyExited, chunk.pane_id, &exit_msg);
                            if framed.feed(msg).await.is_err() {
                                log::warn!("pty-batch feed error: merged_count={}", merged_count);
                                send_err = true;
                                break;
                            }
                        } else {
                            let msg = MuxMessage::pty_output(chunk.pane_id, chunk.data);
                            if framed.feed(msg).await.is_err() {
                                log::warn!("pty-batch feed error: merged_count={}", merged_count);
                                send_err = true;
                                break;
                            }
                        }
                    }
                    if send_err || framed.flush().await.is_err() {
                        if !send_err {
                            log::warn!("pty-batch flush error: merged_count={}", merged_count);
                        }
                        break;
                    }

                    let elapsed = batch_start.elapsed();
                    if elapsed.as_millis() > 50 {
                        log::warn!(
                            "slow-pty-batch: {}ms | drained={} merged={} | {}bytes",
                            elapsed.as_millis(), drained_count, merged_count, total_bytes
                        );
                    }
                }
            }
            _ = render_tick => {
                if let Some(update_msg) = statusbar_engine.render() {
                    if framed.send(update_msg).await.is_err() {
                        break;
                    }
                }
            }
            Some(cmd_name) = cmd_tick_rx.recv() => {
                // Single-flight: skip if previous execution is still running
                if let Some(handle) = cmd_handles.get(&cmd_name) {
                    if !handle.is_finished() {
                        log::debug!("Skipping command '{}': previous execution still running", cmd_name);
                        continue;
                    }
                }
                if let Some(executable) = statusbar_engine.get_command_executable(&cmd_name) {
                    let tx = cmd_result_tx.clone();
                    let cwd = statusbar_engine.active_cwd();
                    let name = cmd_name.clone();
                    let handle = tokio::spawn(async move {
                        let result = execute_command(&executable, &cwd).await;
                        let _ = tx.send((name, result)).await;
                    });
                    cmd_handles.insert(cmd_name, handle);
                }
            }
            Some((name, output)) = cmd_result_rx.recv() => {
                statusbar_engine.update_command_cache(&name, output);
            }
            Some((pane_id, new_title)) = title_rx.recv() => {
                let mut mgr = session_manager.lock().await;
                if let Some((sid, wid)) = mgr.find_pane(pane_id) {
                    let old_name = mgr.get_session(sid)
                        .and_then(|s| s.windows.get(&wid))
                        .map(|w| w.name.clone());
                    if old_name.as_deref() != Some(&new_title) {
                        log::info!("Title change: pane {} -> window {} -> '{}'", pane_id, wid, new_title);
                        mgr.rename_window(sid, wid, new_title.clone());
                        drop(mgr);
                        let rename_payload = RenameWindowMsg { name: new_title };
                        let msg = MuxMessage::control(MessageType::RenameWindow, wid, &rename_payload);
                        if framed.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
            }
            notification = notify_rx.recv() => {
                if let Ok(msg) = notification {
                    // Forward cross-client notification (e.g., CLI SwitchWindow) to GUI
                    log::info!("Forwarding notification to GUI: {:?} pane={}", msg.msg_type, msg.pane_id);
                    if msg.msg_type == MessageType::SwitchWindow {
                        *active_pane_id.lock().unwrap() = Some(msg.pane_id);
                        if statusbar_engine.is_enabled() {
                            // Send SwitchWindow + status bar update as a batch
                            if framed.feed(msg).await.is_err() {
                                break;
                            }
                            let update_msg = statusbar_engine.force_render();
                            if framed.send(update_msg).await.is_err() {
                                break;
                            }
                        } else if framed.send(msg).await.is_err() {
                            break;
                        }
                    } else if framed.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    // Switch all panes in the active session to detached buffering mode.
    // This prevents pty_reader_loop from racing with the next connection's
    // collect_reattach_data when the output_target is still Connected(dead_tx).
    log::info!(
        "Client disconnecting, detaching panes for session {}",
        active_session_id
    );
    detach_session_panes(&session_manager, active_session_id).await;

    log::info!(
        "Client disconnected, session {} panes detached",
        active_session_id
    );
}

/// Handle a CLI client after handshake.
///
/// Reads at most one control message (e.g., CreateWindow), processes it,
/// sends a response, and disconnects. If no message arrives within 5 seconds,
/// disconnects gracefully (this is the normal `mux ls` path).
async fn handle_cli_client<S>(
    framed: &mut Framed<S, MuxCodec>,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // Wait for one optional control message with timeout
    let msg_result = tokio::time::timeout(Duration::from_secs(5), framed.next()).await;

    let msg = match msg_result {
        Ok(Some(Ok(msg))) => msg,
        Ok(Some(Err(e))) => {
            log::warn!("CLI client read error: {}", e);
            return;
        }
        Ok(None) | Err(_) => {
            // Connection closed or timeout - normal for ls/kill commands
            log::info!("CLI client served (no control message), disconnecting");
            return;
        }
    };

    log::info!("CLI client control message: {:?}", msg.msg_type);

    // Determine active session for the control message
    let active_session_id = {
        let mgr = session_manager.lock().await;
        mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1)
    };

    // Create a temporary pane output channel (CLI doesn't stream PTY output)
    let (pane_output_tx, _pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    // Temporary title channel (CLI doesn't process title notifications)
    let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);

    match msg.msg_type {
        MessageType::CreateWindow => {
            let _ = handle_create_window(
                &msg,
                session_manager,
                framed,
                &pane_output_tx,
                active_session_id,
                &title_tx,
            )
            .await;

            // Log the CLI-initiated window creation
            log_cli_window_creation(session_manager, active_session_id).await;
        }
        MessageType::SwitchWindow => {
            let target_id = msg.pane_id;
            handle_switch_window(target_id, session_manager).await;
            // Broadcast to GUI clients so they switch windows too.
            // Resolve the active pane_id of the target window for the GUI.
            let notify_pane_id = {
                let mgr = session_manager.lock().await;
                // Try as pane_id first, then as window_id (same logic as handle_switch_window)
                if let Some((sid, wid)) = mgr.find_pane(target_id) {
                    mgr.get_session(sid)
                        .and_then(|s| s.windows.get(&wid))
                        .and_then(|w| w.active_pane_id)
                        .unwrap_or(target_id)
                } else if let Some(sid) = mgr.find_window_session(target_id) {
                    mgr.get_session(sid)
                        .and_then(|s| s.windows.get(&target_id))
                        .and_then(|w| w.active_pane_id)
                        .unwrap_or(target_id)
                } else {
                    target_id
                }
            };
            let notify_msg = MuxMessage {
                msg_type: MessageType::SwitchWindow,
                pane_id: notify_pane_id,
                payload: vec![],
            };
            let mgr = session_manager.lock().await;
            let _ = mgr.notify_tx().send(notify_msg);
        }
        MessageType::PtyInput => {
            let pane_id = msg.pane_id;
            let mgr = session_manager.lock().await;
            if let Some((session_id, window_id)) = mgr.find_pane(pane_id) {
                if let Some(session) = mgr.get_session(session_id) {
                    if let Some(window) = session.windows.get(&window_id) {
                        if let Some(pane) = window.panes.get(&pane_id) {
                            if let Err(e) = pane.write_input(&msg.payload) {
                                log::warn!("CLI send-keys: failed to write to pane {}: {}", pane_id, e);
                            }
                        }
                    }
                }
            } else {
                log::warn!("CLI send-keys: pane {} not found", pane_id);
            }
        }
        MessageType::Shutdown => {
            log::info!("CLI client requested daemon shutdown");
            let _ = shutdown_tx.send(true);
        }
        _ => {
            log::warn!(
                "CLI client sent unsupported message type: {:?}",
                msg.msg_type
            );
            let err = ErrorMsg {
                message: "Unsupported CLI control message".to_string(),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
        }
    }

    log::info!("CLI client control message processed, disconnecting");
}

/// Log CLI-initiated window creation for debugging.
async fn log_cli_window_creation(session_manager: &Arc<Mutex<SessionManager>>, session_id: u32) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        let window_names: Vec<String> = session.windows.values().map(|w| w.name.clone()).collect();
        log::info!(
            "CLI created window in session {} '{}': windows = {:?}",
            session_id,
            session.name,
            window_names
        );
    }
}

/// Route a single message to the appropriate handler.
///
/// Returns `Err(true)` when the connection should be closed,
/// `Err(false)` on a non-fatal send error, and `Ok(())` otherwise.
async fn route_message<S>(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    statusbar_engine: &mut StatusBarEngine,
    pane_cwd_map: &super::statusbar::SharedPaneCwdMap,
    title_tx: &TitleChangeSender,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match msg.msg_type {
        MessageType::CreateWindow => {
            handle_create_window(
                &msg,
                session_manager,
                framed,
                pane_output_tx,
                *active_session_id,
                title_tx,
            )
            .await?;
            // Register pane cwd Arcs for newly created panes
            register_session_pane_cwds(session_manager, *active_session_id, pane_cwd_map).await;
        }
        MessageType::Attach => {
            handle_attach(
                msg,
                session_manager,
                framed,
                pane_output_tx,
                active_session_id,
                title_tx,
            )
            .await?;
            // Register pane cwd Arcs for all panes in the new session
            register_session_pane_cwds(session_manager, *active_session_id, pane_cwd_map).await;
            // Send status bar content immediately after attach
            if statusbar_engine.is_enabled() {
                let update_msg = statusbar_engine.force_render();
                if framed.send(update_msg).await.is_err() {
                    return Err(false);
                }
            }
        }
        MessageType::SplitPane => {
            handle_split_pane(msg, session_manager, framed, pane_output_tx, title_tx).await?;
            // Register pane cwd Arcs for newly split panes
            register_session_pane_cwds(session_manager, *active_session_id, pane_cwd_map).await;
        }
        MessageType::Detach => {
            log::info!("Client requested detach");
            let resp = MuxMessage::control(MessageType::Detached, 0, &());
            let _ = framed.send(resp).await;
            return Err(true);
        }
        MessageType::DestroyPane => {
            pane_cwd_map.lock().unwrap().remove(&msg.pane_id);
            handle_destroy_pane(msg.pane_id, session_manager, shutdown_tx).await;
        }
        MessageType::SwitchWindow => {
            handle_switch_window(msg.pane_id, session_manager).await;
            // Force status bar re-render with new pane's cwd
            if statusbar_engine.is_enabled() {
                let update_msg = statusbar_engine.force_render();
                if framed.send(update_msg).await.is_err() {
                    return Err(false);
                }
            }
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
        MessageType::RequestStatusUpdate => {
            let update_msg = statusbar_engine.force_render();
            if framed.send(update_msg).await.is_err() {
                return Err(false);
            }
        }
        MessageType::RequestPaneSnapshot => {
            handle_request_pane_snapshot(&msg, session_manager, pane_output_tx).await?;
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

/// Merge consecutive PTY output chunks from the same pane into a single chunk.
///
/// Preserves ordering across panes. Empty-data chunks (PTY exit signals) are
/// never merged — they remain as separate entries to ensure correct exit handling.
fn merge_consecutive_chunks(chunks: Vec<PtyOutputChunk>) -> Vec<PtyOutputChunk> {
    if chunks.len() <= 1 {
        return chunks;
    }
    let mut merged: Vec<PtyOutputChunk> = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        if chunk.data.is_empty() {
            // Exit signal: never merge
            merged.push(chunk);
        } else if let Some(last) = merged.last_mut() {
            if last.pane_id == chunk.pane_id && !last.data.is_empty() {
                // Same pane, both non-empty: concatenate data
                last.data.extend_from_slice(&chunk.data);
            } else {
                merged.push(chunk);
            }
        } else {
            merged.push(chunk);
        }
    }
    merged
}

/// Register pane cwd Arcs from session_manager into pane_cwd_map.
/// Called once per pane creation / reattach (very rare), not per output chunk.
async fn register_session_pane_cwds(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_cwd_map: &super::statusbar::SharedPaneCwdMap,
) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        let mut map = pane_cwd_map.lock().unwrap();
        for window in session.windows.values() {
            for pane in window.panes.values() {
                map.entry(pane.id).or_insert_with(|| pane.cwd.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(pane_id: u32, data: &[u8]) -> PtyOutputChunk {
        PtyOutputChunk {
            pane_id,
            data: data.to_vec(),
        }
    }

    fn exit_chunk(pane_id: u32) -> PtyOutputChunk {
        PtyOutputChunk {
            pane_id,
            data: Vec::new(),
        }
    }

    #[test]
    fn merge_single_chunk() {
        let chunks = vec![chunk(1, b"hello")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"hello");
    }

    #[test]
    fn merge_same_pane_consecutive() {
        let chunks = vec![chunk(1, b"hel"), chunk(1, b"lo"), chunk(1, b"!")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"hello!");
    }

    #[test]
    fn merge_different_panes_not_merged() {
        let chunks = vec![chunk(1, b"a"), chunk(2, b"b"), chunk(1, b"c")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"a");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"b");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"c");
    }

    #[test]
    fn merge_exit_signal_not_merged() {
        let chunks = vec![chunk(1, b"data"), exit_chunk(1)];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].data, b"data");
        assert!(merged[1].data.is_empty());
    }

    #[test]
    fn merge_exit_signal_mid_batch() {
        // pane 1 data, pane 1 exit, pane 1 data (from new process or leftover)
        let chunks = vec![chunk(1, b"before"), exit_chunk(1), chunk(1, b"after")];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].data, b"before");
        assert!(merged[1].data.is_empty());
        assert_eq!(merged[2].data, b"after");
    }

    #[test]
    fn merge_mixed_pane_ordering_preserved() {
        // Interleaved panes: A, B, A, B — ordering must be preserved
        let chunks = vec![
            chunk(1, b"a1"),
            chunk(1, b"a2"),
            chunk(2, b"b1"),
            chunk(2, b"b2"),
            chunk(1, b"a3"),
            chunk(3, b"c1"),
        ];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"a1a2");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"b1b2");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"a3");
        assert_eq!(merged[3].pane_id, 3);
        assert_eq!(merged[3].data, b"c1");
    }
}
