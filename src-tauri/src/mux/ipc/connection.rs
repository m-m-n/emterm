//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::handlers::{
    handle_attach, handle_create_window, handle_destroy_pane, handle_destroy_window,
    handle_move_window, handle_rename_window, handle_request_pane_snapshot, handle_resize,
    handle_set_visibility, handle_switch_window,
};
use super::protocol::*;
use super::reattach::detach_session_panes;
use super::statusbar::{StatusBarEngine, execute_command};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    NotificationSender, PtyOutputChunk, SharedPaneExitSender, TitleChangeSender,
};

/// Handshake timeout: client must send Hello within this duration.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum number of PTY output chunks to drain per select! iteration.
/// Balances batch efficiency (fewer syscalls) against input responsiveness
/// (returning to select! to check for PtyInput). At 64 chunks × 65KB max
/// each, worst-case batch memory is ~4MB (transient, freed after flush).
const DRAIN_BATCH_LIMIT: usize = 64;

/// Handle a new client connection through handshake and message loop.
#[allow(clippy::too_many_arguments)]
pub async fn handle_connection<S>(
    stream: S,
    session_manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    daemon_title_tx: TitleChangeSender,
    daemon_notification_tx: NotificationSender,
    daemon_pane_exit_sender: SharedPaneExitSender,
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

    // Subscribe to notify_tx before building Welcome so any RenameWindow
    // broadcast emitted between snapshot construction and message-loop entry
    // is captured rather than lost.
    let mut notify_rx = {
        let mgr = session_manager.lock().await;
        mgr.notify_tx().subscribe()
    };

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
        handle_cli_client(
            &mut framed,
            &session_manager,
            &shutdown_tx,
            &daemon_title_tx,
            &daemon_notification_tx,
            &daemon_pane_exit_sender,
        )
        .await;
        return;
    }

    // Determine active session: use first session (auto-created "default")
    let mut active_session_id: u32 = {
        let mgr = session_manager.lock().await;
        let id = mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1);
        id
    };

    // Shared channel: all pane reader threads send output here,
    // and the select! loop forwards it to the client.
    let (pane_output_tx, mut pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    // Reuse the daemon-level title sender so OSC title updates flow to the
    // daemon task regardless of connection lifetime. GUI delivery of
    // RenameWindow happens via notify_rx, which is populated by the daemon
    // task when it updates window.name.
    let title_tx = daemon_title_tx;
    // Daemon-lifetime notification sender: panes created on this connection
    // forward Detached-pane OSC 9 notifications through it; the daemon
    // notification task relays them to the GUI client (FR2).
    let notification_tx = daemon_notification_tx;
    // Daemon-lifetime pane-exit sender: panes created on this connection emit
    // their pane_id here on PTY EOF (regardless of attach state) so the daemon
    // reap task can reap them authoritatively (FR1/FR2). Fixed at pane
    // creation; never swapped on detach.
    let pane_exit_sender = daemon_pane_exit_sender;

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
    let mut cmd_handles: std::collections::HashMap<String, tokio::task::JoinHandle<()>> =
        std::collections::HashMap::new();

    // Kick channel: set by handle_attach. Fires with Ok(()) when another
    // client attaches to the same session and evicts us. Drop-without-send
    // (Err) is treated as a no-op so that cleanly switching sessions does
    // not kick ourselves off.
    let mut kick_rx: Option<oneshot::Receiver<()>> = None;
    let mut was_kicked = false;

    // Per-connection effective-visible state (FR3, FR7). Initially true so
    // newly-attached clients receive PTY output immediately. Updated by
    // SetVisibility messages and consulted on reattach (collect_reattach_data
    // re-evaluates output_target after a session switch).
    let visible_state: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));

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

        // Kick future: resolves when our kick_rx fires. `Ok(())` means
        // another client attached to this session and evicted us. `Err(_)`
        // means the Sender was dropped — in practice this occurs when the
        // active session is destroyed while we're still attached (the
        // session's `active_client_kick` drops along with the MuxSession).
        // Both cases are terminal for this connection; `None` stays pending.
        let kick_fut = async {
            match kick_rx.as_mut() {
                Some(rx) => rx.await,
                None => {
                    std::future::pending::<Result<(), tokio::sync::oneshot::error::RecvError>>()
                        .await
                }
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
                            &notification_tx,
                            &pane_exit_sender,
                            &mut kick_rx,
                            &visible_state,
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
            kick_result = kick_fut => {
                let reason = match kick_result {
                    Ok(()) => "evicted by newer client",
                    Err(_) => "active session destroyed",
                };
                log::info!(
                    "Client disconnecting ({}) from session {}; sending Detached",
                    reason, active_session_id
                );
                was_kicked = true;
                let resp = MuxMessage::control(MessageType::Detached, 0, &());
                let _ = framed.send(resp).await;
                break;
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
                    let mut exited_panes: Vec<u32> = Vec::new();
                    for chunk in merged {
                        if chunk.data.is_empty() {
                            log::info!("PTY exited for pane {}", chunk.pane_id);
                            exited_panes.push(chunk.pane_id);
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
                    let flush_failed = framed.flush().await.is_err();
                    // Reap each exited pane from the daemon's own SessionManager
                    // *regardless* of whether delivery to the client succeeded:
                    // the PTY genuinely exited, so the empty window / session must
                    // be removed and the daemon shut down once the last pane is
                    // gone. Gating this on a successful flush would re-open the
                    // zombie-pane bug under a client-drop race — the GUI window
                    // closing at the same moment the last shell exits via Ctrl+D
                    // both delivers the exit chunk and fails the client write, so
                    // a success-gated reap would skip cleanup and leave a session
                    // that never auto-shuts-down / can't be `mux kill`ed. Mirror
                    // the explicit `DestroyPane` cleanup fully — including dropping
                    // the pane's cwd entry from the status-bar map so it can't
                    // resolve a stale cwd for a dead pane.
                    for pane_id in exited_panes {
                        pane_cwd_map.lock().unwrap().remove(&pane_id);
                        handle_destroy_pane(pane_id, &session_manager, &shutdown_tx).await;
                    }
                    if send_err || flush_failed {
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
            notification = notify_rx.recv() => {
                match notification {
                    Ok(msg) => {
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        log::warn!(
                            "notify_rx lagged: {} notifications dropped; resyncing window names from session_list",
                            skipped
                        );
                        let list = session_manager.lock().await.session_list();
                        for sess in &list {
                            for win in &sess.windows {
                                let payload = RenameWindowMsg { name: win.name.clone() };
                                let msg = MuxMessage::control(
                                    MessageType::RenameWindow,
                                    win.id,
                                    &payload,
                                );
                                if framed.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        log::warn!("notify_rx closed; exiting connection loop");
                        break;
                    }
                }
            }
        }
    }

    // Switch all panes in the active session to detached buffering mode.
    // This prevents pty_reader_loop from racing with the next connection's
    // collect_reattach_data when the output_target is still Connected(dead_tx).
    //
    // Skipped when we were kicked by another attaching client: in that case
    // the newer client has already taken ownership of the panes, and running
    // detach_session_panes would immediately clobber their Connected state
    // back to Detached, stranding them.
    if was_kicked {
        log::info!(
            "Client disconnecting (kicked), leaving session {} panes attached to new client",
            active_session_id
        );
    } else {
        log::info!(
            "Client disconnecting, detaching panes for session {}",
            active_session_id
        );
        // Identity-scoped: detach only panes still owned by our pane_output_tx.
        // Belt-and-suspenders with `was_kicked`: protects against races where
        // framed.next() wins over kick_fut in the select!, or where the socket
        // fails mid-eviction and we exit without observing the kick.
        detach_session_panes(&session_manager, active_session_id, &pane_output_tx).await;

        log::info!(
            "Client disconnected, session {} panes detached",
            active_session_id
        );
    }
}

/// Handle a CLI client after handshake.
///
/// Reads at most one control message (e.g., CreateWindow), processes it,
/// sends a response, and disconnects. If no message arrives within 5 seconds,
/// disconnects gracefully (this is the normal `mux ls` path).
#[allow(clippy::too_many_arguments)]
async fn handle_cli_client<S>(
    framed: &mut Framed<S, MuxCodec>,
    session_manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    daemon_title_tx: &TitleChangeSender,
    daemon_notification_tx: &NotificationSender,
    daemon_pane_exit_sender: &SharedPaneExitSender,
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
        let id = mgr.sessions_iter().next().map(|s| s.id).unwrap_or(1);
        id
    };

    // Create a temporary pane output channel (CLI doesn't stream PTY output)
    let (pane_output_tx, _pane_output_rx) =
        mpsc::channel::<PtyOutputChunk>(crate::mux::session::pane::PTY_CHANNEL_CAPACITY);

    match msg.msg_type {
        MessageType::CreateWindow => {
            let _ = handle_create_window(
                &msg,
                session_manager,
                framed,
                &pane_output_tx,
                active_session_id,
                daemon_title_tx,
                daemon_notification_tx,
                daemon_pane_exit_sender,
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
                                log::warn!(
                                    "CLI send-keys: failed to write to pane {}: {}",
                                    pane_id,
                                    e
                                );
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
#[allow(clippy::too_many_arguments)]
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
    notification_tx: &NotificationSender,
    pane_exit_sender: &SharedPaneExitSender,
    kick_rx: &mut Option<oneshot::Receiver<()>>,
    visible_state: &Arc<AtomicBool>,
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
                notification_tx,
                pane_exit_sender,
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
                kick_rx,
                visible_state,
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
        MessageType::MoveWindow => {
            handle_move_window(msg, session_manager).await;
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
            // WARN-level entry log so release builds capture whether the
            // request even reached the daemon. The reply is logged inside
            // handle_request_pane_snapshot once the snapshot is built.
            log::warn!("RequestPaneSnapshot: received for pane {}", msg.pane_id);
            handle_request_pane_snapshot(&msg, *active_session_id, session_manager, pane_output_tx)
                .await?;
        }
        MessageType::SetVisibility => {
            let payload = match SetVisibilityPayload::from_payload(&msg.payload) {
                Some(p) => p,
                None => {
                    log::warn!("SetVisibility: empty payload, ignoring");
                    return Ok(());
                }
            };
            handle_set_visibility(
                payload.visible,
                session_manager,
                *active_session_id,
                pane_output_tx,
                visible_state,
            )
            .await;
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

    // ── merge / drain efficiency metrics (IPC frame-count regression guard) ──
    //
    // The daemon drains up to `DRAIN_BATCH_LIMIT` chunks per select! iteration
    // and merges consecutive same-pane chunks into one before sending. Each
    // surviving merged chunk becomes one IPC frame (one base64-encoded APC/OSC
    // envelope on the bridge transport), so "output chunk count" is the
    // frame-count metric the perf work optimizes. These tests pin the
    // input-vs-output chunk reduction deterministically (counts only, no
    // timing) so a regression that stops coalescing is caught.

    /// Total bytes are conserved across the merge (no data lost/duplicated)
    /// while the chunk count collapses — the core efficiency invariant.
    fn total_bytes(chunks: &[PtyOutputChunk]) -> usize {
        chunks.iter().map(|c| c.data.len()).sum()
    }

    #[test]
    fn merge_efficiency_single_pane_n_to_one() {
        // N consecutive same-pane chunks → exactly 1 IPC frame.
        for n in [2usize, 8, 64] {
            let chunks: Vec<PtyOutputChunk> = (0..n).map(|_| chunk(1, b"abcd")).collect();
            let bytes_in = total_bytes(&chunks);
            let merged = merge_consecutive_chunks(chunks);
            assert_eq!(
                merged.len(),
                1,
                "{n} consecutive same-pane chunks must merge to 1 frame"
            );
            // Frame-count reduction: N inputs → 1 output.
            assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost in merge");
            assert_eq!(merged[0].data.len(), n * 4, "all payloads concatenated");
        }
    }

    #[test]
    fn merge_efficiency_only_consecutive_same_pane_collapses() {
        // Mixed panes: only *runs* of the same pane collapse. Six input chunks
        // across runs [1,1,1 | 2,2 | 1] → three output frames (one per run).
        let chunks = vec![
            chunk(1, b"a"),
            chunk(1, b"b"),
            chunk(1, b"c"),
            chunk(2, b"d"),
            chunk(2, b"e"),
            chunk(1, b"f"),
        ];
        let bytes_in = total_bytes(&chunks);
        let input_count = chunks.len();
        let merged = merge_consecutive_chunks(chunks);

        // 6 input chunks → 3 output frames (a 50% frame reduction here).
        assert_eq!(input_count, 6);
        assert_eq!(
            merged.len(),
            3,
            "only consecutive same-pane runs collapse; interleaving stays split"
        );
        assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost");
        assert_eq!(merged[0].pane_id, 1);
        assert_eq!(merged[0].data, b"abc");
        assert_eq!(merged[1].pane_id, 2);
        assert_eq!(merged[1].data, b"de");
        assert_eq!(merged[2].pane_id, 1);
        assert_eq!(merged[2].data, b"f");
    }

    #[test]
    fn merge_efficiency_alternating_panes_no_reduction() {
        // Worst case: strictly alternating panes never collapse, so the frame
        // count is unchanged (input count == output count). This pins the
        // lower bound of the optimization (merge never *increases* frames).
        let chunks = vec![
            chunk(1, b"a"),
            chunk(2, b"b"),
            chunk(1, b"c"),
            chunk(2, b"d"),
        ];
        let input_count = chunks.len();
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            input_count,
            "alternating panes cannot merge; frame count is unchanged"
        );
    }

    #[test]
    fn merge_efficiency_full_drain_batch_single_pane() {
        // A full drain batch (DRAIN_BATCH_LIMIT chunks) from one busy pane —
        // the bulk-output hot path — collapses to a single IPC frame. This is
        // the headline win the perf work relies on: 64 drained chunks → 1 send.
        let chunks: Vec<PtyOutputChunk> = (0..DRAIN_BATCH_LIMIT)
            .map(|_| chunk(42, &[0u8; 1024]))
            .collect();
        let bytes_in = total_bytes(&chunks);
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            1,
            "a full {DRAIN_BATCH_LIMIT}-chunk single-pane drain merges to 1 frame"
        );
        assert_eq!(total_bytes(&merged), bytes_in);
        assert_eq!(
            merged[0].data.len(),
            DRAIN_BATCH_LIMIT * 1024,
            "merged frame carries the whole batch"
        );
    }

    #[test]
    fn merge_efficiency_exit_signals_stay_separate() {
        // Exit signals (empty data) are never merged, so they always cost their
        // own frame even amid same-pane data. Pin this so the frame-count model
        // accounts for them: 3 data chunks + 1 exit (same pane) → 2 frames.
        let chunks = vec![
            chunk(1, b"x"),
            chunk(1, b"y"),
            chunk(1, b"z"),
            exit_chunk(1),
        ];
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            2,
            "same-pane data collapses to 1 frame; the exit signal is a 2nd frame"
        );
        assert_eq!(merged[0].data, b"xyz");
        assert!(
            merged[1].data.is_empty(),
            "exit signal preserved separately"
        );
    }
}
