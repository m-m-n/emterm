//! IPC connection handling for mux daemon.
//!
//! Manages per-client connection state machine:
//! handshake -> authenticated (GUI streaming or CLI control).

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{MuxPane, PtyOutputChunk};

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

/// Spawn a PTY, create a pane, and start a reader thread for output streaming.
async fn handle_create_window(
    session_manager: &Arc<Mutex<SessionManager>>,
    framed: &mut Framed<UnixStream, MuxCodec>,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Result<(), bool> {
    let pty_system = portable_pty::native_pty_system();
    let pty_size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = match pty_system.openpty(pty_size) {
        Ok(pair) => pair,
        Err(e) => {
            log::error!("Failed to open PTY: {}", e);
            return Ok(());
        }
    };

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

    match pair.slave.spawn_command(cmd) {
        Ok(_child) => {
            let writer = match pair.master.take_writer() {
                Ok(w) => w,
                Err(e) => {
                    log::error!("Failed to take PTY writer: {}", e);
                    return Ok(());
                }
            };
            let reader = match pair.master.try_clone_reader() {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to clone PTY reader: {}", e);
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

            let pane_id = mgr.alloc_pane_id();
            let session = mgr.get_session_mut(1).unwrap();
            let window = session.windows.get_mut(&window_id).unwrap();

            let output_tx = pane_output_tx.clone();
            let pane = MuxPane::new(pane_id, 80, 24, output_tx.clone(), writer);
            window.add_pane(pane);

            // Release lock before sending response and spawning reader
            drop(mgr);

            // Start PTY reader thread (blocking I/O, must be std::thread)
            let tx = output_tx;
            std::thread::spawn(move || {
                pty_reader_loop(pane_id, reader, tx);
            });

            log::info!(
                "Created window {} with pane {} (PTY spawned)",
                window_id,
                pane_id
            );

            let resp = MuxMessage::control(MessageType::PaneCreated, pane_id, &pane_id);
            if framed.send(resp).await.is_err() {
                return Err(true);
            }
        }
        Err(e) => {
            log::error!("Failed to spawn shell: {}", e);
        }
    }

    Ok(())
}

/// Read PTY output in a blocking loop and forward to the channel.
/// Runs in a dedicated std::thread since PTY reads are blocking I/O.
fn pty_reader_loop(
    pane_id: u32,
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<PtyOutputChunk>,
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                log::info!("PTY reader EOF for pane {}", pane_id);
                break;
            }
            Ok(n) => {
                let chunk = PtyOutputChunk {
                    pane_id,
                    data: buf[..n].to_vec(),
                };
                if tx.blocking_send(chunk).is_err() {
                    log::info!("PTY output channel closed for pane {}", pane_id);
                    break;
                }
            }
            Err(e) => {
                log::info!("PTY reader error for pane {}: {}", pane_id, e);
                break;
            }
        }
    }
    // Send empty chunk to signal PTY exit to the connection handler
    let _ = tx.blocking_send(PtyOutputChunk {
        pane_id,
        data: Vec::new(), // empty = exit signal
    });
}
