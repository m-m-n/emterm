//! Message handlers for mux IPC commands.

use std::sync::Arc;

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use super::pty_spawn::{register_pane_and_start_reader, spawn_pty};
use super::reattach::{collect_reattach_data, detach_session_panes, send_reattach_data};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{PaneId, PtyOutputChunk};

/// Spawn a PTY, create a pane, and start a reader thread for output streaming.
///
/// Decodes optional `CreateWindowPayload` from the message to set window name
/// and execute an initial command. Empty or missing payload defaults to
/// name="shell" with no command (backward compatible with GUI).
pub(super) async fn handle_create_window<S>(
    msg: &MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: u32,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // Decode payload; empty/invalid payload -> defaults (backward compat)
    let payload = msg
        .decode_payload::<CreateWindowPayload>()
        .unwrap_or_default();

    let window_name = payload
        .name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("shell")
        .to_string();

    let spawned = match spawn_pty(80, 24) {
        Ok(s) => s,
        Err(e) => {
            log::error!("{}", e);
            let err = ErrorMsg {
                message: format!("Failed to spawn PTY: {}", e),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
            return Ok(());
        }
    };

    let mut mgr = session_manager.lock().await;
    let window_id = match mgr.create_window(active_session_id, window_name.clone()) {
        Some(id) => id,
        None => {
            log::error!("Failed to create window in session {}", active_session_id);
            drop(mgr);
            let err = ErrorMsg {
                message: "Failed to create window".to_string(),
            };
            let resp = MuxMessage::control(MessageType::Error, 0, &err);
            let _ = framed.send(resp).await;
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

    let command = payload.command.filter(|s| !s.is_empty());
    drop(mgr);

    // Write initial command to PTY after short delay for shell readiness
    if let Some(ref cmd) = command {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mgr = session_manager.lock().await;
        if let Some(session) = mgr.get_session(active_session_id) {
            if let Some(window) = session.windows.get(&window_id) {
                if let Some(pane) = window.panes.get(&pane_id) {
                    let cmd_with_newline = format!("{}\n", cmd);
                    if let Err(e) = pane.write_input(cmd_with_newline.as_bytes()) {
                        log::warn!("Failed to write initial command to pane {}: {}", pane_id, e);
                    }
                }
            }
        }
    }

    log::info!(
        "Created window {} '{}' with pane {} (PTY spawned{})",
        window_id,
        window_name,
        pane_id,
        if command.is_some() {
            ", command sent"
        } else {
            ""
        }
    );

    let resp = MuxMessage::control(MessageType::PaneCreated, pane_id, &pane_id);
    if framed.send(resp).await.is_err() {
        return Err(true);
    }

    Ok(())
}

/// Split an existing pane by spawning a new PTY in the same window.
pub(super) async fn handle_split_pane<S>(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
pub(super) async fn handle_destroy_pane(
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
///
/// The `id` field may be either a pane ID (from GUI OSC title sync) or a
/// window ID (from CLI). Tries pane lookup first; falls back to window lookup.
pub(super) async fn handle_rename_window(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
) {
    let rename_msg: RenameWindowMsg = match msg.decode_payload() {
        Some(m) => m,
        None => {
            log::warn!("Invalid RenameWindow payload");
            return;
        }
    };
    let id = msg.pane_id;

    let mut mgr = session_manager.lock().await;

    // Try as pane_id first (GUI sends active pane_id)
    if let Some((sid, wid)) = mgr.find_pane(id) {
        log::info!(
            "RenameWindow: pane {} -> window {} -> '{}'",
            id,
            wid,
            rename_msg.name
        );
        mgr.rename_window(sid, wid, rename_msg.name);
        return;
    }

    // Fall back to window_id
    if let Some(sid) = mgr.find_window_session(id) {
        log::info!(
            "RenameWindow: window {} -> '{}'",
            id,
            rename_msg.name
        );
        mgr.rename_window(sid, id, rename_msg.name);
    } else {
        log::warn!("RenameWindow: id {} not found as pane or window", id);
    }
}

/// Switch the active window in the session.
///
/// The `id` may be either a pane ID (from GUI) or a window ID (from CLI).
/// Tries pane lookup first; falls back to window lookup.
pub(super) async fn handle_switch_window(
    id: u32,
    session_manager: &Arc<Mutex<SessionManager>>,
) {
    let mut mgr = session_manager.lock().await;

    // Try as pane_id first (GUI sends pane_id)
    if let Some((sid, wid)) = mgr.find_pane(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.active_window_id = Some(wid);
            log::info!(
                "SwitchWindow: pane {} -> session {} active window -> {}",
                id,
                sid,
                wid
            );
        }
        return;
    }

    // Fall back to window_id (CLI sends window_id)
    if let Some(sid) = mgr.find_window_session(id) {
        if let Some(session) = mgr.get_session_mut(sid) {
            session.active_window_id = Some(id);
            log::info!(
                "SwitchWindow: session {} active window -> {}",
                sid,
                id
            );
        }
    } else {
        log::warn!("SwitchWindow: id {} not found as pane or window", id);
    }
}

/// Destroy a window and all its panes, cleaning up empty sessions.
/// Signals daemon shutdown when all sessions become empty.
pub(super) async fn handle_destroy_window(
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
pub(super) async fn handle_resize(msg: MuxMessage, session_manager: &Arc<Mutex<SessionManager>>) {
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
                    log::debug!(
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

/// Handle Attach message: switch the client to a different session.
///
/// Detaches panes from the current session, updates the active session,
/// and reattaches panes from the new session with buffered output replay.
pub(super) async fn handle_attach<S>(
    msg: MuxMessage,
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<S, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    active_session_id: &mut u32,
) -> Result<(), bool>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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
