//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake → authenticated (GUI streaming or CLI control).

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;

/// Handshake timeout: client must send Hello within this duration.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle a new client connection through handshake and message loop.
pub async fn handle_connection(stream: UnixStream) {
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

    // Send Welcome
    let welcome = WelcomeMsg::Accepted {
        server_version: PROTOCOL_VERSION,
        sessions: vec![], // TODO: populate from session manager
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
                log::debug!(
                    "Received {:?} for pane {} ({} bytes)",
                    msg.msg_type,
                    msg.pane_id,
                    msg.payload.len()
                );
                // TODO: Route messages to session manager
            }
            Err(e) => {
                log::warn!("Connection error: {}", e);
                break;
            }
        }
    }

    log::info!("Client disconnected");
}
