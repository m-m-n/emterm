//! Window / pane subcommands: `new-window` / `switch-window` /
//! `send-keys`, plus the target-pane resolver they share.

use mux_ipc::protocol::{
    CreateWindowPayload, ErrorMsg, MAX_FRAME_LENGTH, MessageType, MuxMessage, SessionInfo,
};

use super::connect::cli_handshake;

/// Execute the `emterm mux new-window` command.
///
/// Connects to the daemon, performs handshake, sends CreateWindow with
/// optional name and command, and waits for PaneCreated response.
#[cfg(unix)]
pub fn execute_new_window(
    name: Option<&str>,
    command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let (mut stream, _sessions) = cli_handshake()?;

    // Build CreateWindowPayload
    let payload = CreateWindowPayload {
        name: name.map(|s| s.to_string()),
        command: command.map(|s| s.to_string()),
    };

    // Send CreateWindow message (session_id in pane_id field = 0, daemon uses active session)
    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    // Read response
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let resp = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    match resp.msg_type {
        MessageType::PaneCreated => {
            // Success - window created
            Ok(())
        }
        MessageType::Error => {
            let err: ErrorMsg = resp.decode_payload().unwrap_or(ErrorMsg {
                message: "Unknown error".to_string(),
            });
            Err(format!("Failed to create window: {}", err.message).into())
        }
        _ => Err(format!("Unexpected response: {:?}", resp.msg_type).into()),
    }
}

/// Execute the `emterm mux new-window` command (Windows).
#[cfg(windows)]
pub fn execute_new_window(
    name: Option<&str>,
    command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let (mut stream, _sessions) = cli_handshake()?;

    let payload = CreateWindowPayload {
        name: name.map(|s| s.to_string()),
        command: command.map(|s| s.to_string()),
    };

    let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".into());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream.read_exact(&mut frame_buf)?;

    let resp = MuxMessage::from_frame_body(&frame_buf).ok_or("Invalid frame")?;

    match resp.msg_type {
        MessageType::PaneCreated => Ok(()),
        MessageType::Error => {
            let err: ErrorMsg = resp.decode_payload().unwrap_or(ErrorMsg {
                message: "Unknown error".to_string(),
            });
            Err(format!("Failed to create window: {}", err.message).into())
        }
        _ => Err(format!("Unexpected response: {:?}", resp.msg_type).into()),
    }
}

/// Execute the `emterm mux new-window` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_new_window(
    _name: Option<&str>,
    _command: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Execute the `emterm mux switch-window` command.
///
/// Connects to the daemon and sends SwitchWindow for the given window index.
#[cfg(unix)]
pub fn execute_switch_window(target: u32) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let (mut stream, sessions) = cli_handshake()?;
    let session = sessions.first().ok_or("No active session")?;

    if target as usize >= session.windows.len() {
        return Err(format!(
            "Window index {} out of range (0..{})",
            target,
            session.windows.len()
        )
        .into());
    }

    let window_id = session.windows[target as usize].id;
    let msg = MuxMessage {
        msg_type: MessageType::SwitchWindow,
        pane_id: window_id,
        payload: vec![],
    };
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux switch-window` command (Windows).
#[cfg(windows)]
pub fn execute_switch_window(target: u32) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let (mut stream, sessions) = cli_handshake()?;
    let session = sessions.first().ok_or("No active session")?;

    if target as usize >= session.windows.len() {
        return Err(format!(
            "Window index {} out of range (0..{})",
            target,
            session.windows.len()
        )
        .into());
    }

    let window_id = session.windows[target as usize].id;
    let msg = MuxMessage {
        msg_type: MessageType::SwitchWindow,
        pane_id: window_id,
        payload: vec![],
    };
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux switch-window` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_switch_window(_target: u32) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}

/// Resolve the target pane ID from sessions and optional window index.
///
/// Returns the active pane ID of the resolved window.
pub(in crate::mux::cli) fn resolve_target_pane(
    sessions: &[SessionInfo],
    target: Option<u32>,
) -> Result<u32, Box<dyn std::error::Error>> {
    let session = sessions.first().ok_or("No active session")?;

    let window_index = match target {
        Some(idx) => {
            if idx as usize >= session.windows.len() {
                return Err(format!(
                    "Window index {} out of range (0..{})",
                    idx,
                    session.windows.len()
                )
                .into());
            }
            idx as usize
        }
        None => {
            if session.windows.is_empty() {
                return Err("No windows in session".into());
            }
            let idx = session.active_window_index as usize;
            if idx >= session.windows.len() {
                return Err(format!(
                    "Active window index {} out of range (0..{})",
                    idx,
                    session.windows.len()
                )
                .into());
            }
            idx
        }
    };

    let window = &session.windows[window_index];
    let pane_id = window.active_pane_id;

    if pane_id == 0 {
        return Err(format!("No active pane in window {}", window_index).into());
    }

    Ok(pane_id)
}

/// Execute the `emterm mux send-keys` command.
///
/// Reads stdin, connects to daemon, resolves target pane from window index,
/// and sends PtyInput message.
#[cfg(unix)]
pub fn execute_send_keys(target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    // Read stdin with size limit (MAX_FRAME_LENGTH = 16MB)
    let mut data = Vec::new();
    let bytes_read = std::io::stdin()
        .take(MAX_FRAME_LENGTH as u64 + 1)
        .read_to_end(&mut data)?;
    if bytes_read > MAX_FRAME_LENGTH {
        return Err(format!(
            "stdin data exceeds maximum size ({}MB)",
            MAX_FRAME_LENGTH / 1024 / 1024
        )
        .into());
    }

    // Empty stdin: exit 0 without connecting
    if data.is_empty() {
        return Ok(());
    }

    let (mut stream, sessions) = cli_handshake()?;

    let pane_id = resolve_target_pane(&sessions, target)?;

    // Send PtyInput
    let msg = MuxMessage::pty_input(pane_id, data);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux send-keys` command (Windows).
#[cfg(windows)]
pub fn execute_send_keys(target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};

    let mut data = Vec::new();
    let bytes_read = std::io::stdin()
        .take(MAX_FRAME_LENGTH as u64 + 1)
        .read_to_end(&mut data)?;
    if bytes_read > MAX_FRAME_LENGTH {
        return Err(format!(
            "stdin data exceeds maximum size ({}MB)",
            MAX_FRAME_LENGTH / 1024 / 1024
        )
        .into());
    }

    if data.is_empty() {
        return Ok(());
    }

    let (mut stream, sessions) = cli_handshake()?;

    let pane_id = resolve_target_pane(&sessions, target)?;

    let msg = MuxMessage::pty_input(pane_id, data);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;

    Ok(())
}

/// Execute the `emterm mux send-keys` command (unsupported platform).
#[cfg(all(not(unix), not(windows)))]
pub fn execute_send_keys(_target: Option<u32>) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Mux is not supported on this platform");
    std::process::exit(1);
}
