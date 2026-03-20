//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::session::manager::SessionManager;

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

    // Message loop
    while let Some(result) = framed.next().await {
        match result {
            Ok(msg) => {
                if let Err(should_break) = route_message(msg, &session_manager, &mut framed).await {
                    if should_break {
                        break;
                    }
                }
            }
            Err(e) => {
                log::warn!("Connection error: {}", e);
                break;
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
) -> Result<(), bool> {
    match msg.msg_type {
        MessageType::CreateWindow => {
            let mut mgr = session_manager.lock().await;
            // Create window in session 1 (default) for now
            if let Some(window_id) = mgr.create_window(1, "shell".to_string()) {
                log::info!("Created window {} in session 1", window_id);
                let resp = MuxMessage::control(MessageType::PaneCreated, 0, &window_id);
                if framed.send(resp).await.is_err() {
                    return Err(true);
                }
            }
        }
        MessageType::SplitPane => {
            log::info!("SplitPane requested for pane {}", msg.pane_id);
            // TODO: implement pane splitting with PTY spawn
        }
        MessageType::Detach => {
            log::info!("Client requested detach");
            let resp = MuxMessage::control(MessageType::Detached, 0, &());
            let _ = framed.send(resp).await;
            return Err(true);
        }
        MessageType::DestroyPane => {
            log::info!("DestroyPane requested for pane {}", msg.pane_id);
            // TODO: kill pane PTY
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
