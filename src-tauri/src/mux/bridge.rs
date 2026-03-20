//! Tauri IPC commands bridging GUI ↔ daemon IPC socket.
//!
//! The GUI cannot directly access Unix sockets from WebView JavaScript.
//! These Tauri commands act as a bridge, forwarding messages between
//! the frontend and the daemon's Unix domain socket.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use super::ipc::protocol::*;

/// State for managing mux daemon connections from the GUI.
pub struct MuxBridgeState {
    /// Active connections keyed by a connection ID.
    connections: Mutex<HashMap<String, Arc<Mutex<UnixStream>>>>,
}

impl MuxBridgeState {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MuxBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Connect to the mux daemon socket.
/// Returns a connection ID for subsequent operations.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_connect(
    state: tauri::State<'_, MuxBridgeState>,
    socket_path: String,
) -> Result<String, String> {
    // Validate socket path
    validate_socket_path(&socket_path)?;

    let stream = UnixStream::connect(&socket_path)
        .await
        .map_err(|e| format!("Failed to connect to daemon: {}", e))?;

    let conn_id = uuid::Uuid::new_v4().to_string();
    let mut conns = state.connections.lock().await;
    conns.insert(conn_id.clone(), Arc::new(Mutex::new(stream)));

    Ok(conn_id)
}

/// Disconnect from the mux daemon.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_disconnect(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
) -> Result<(), String> {
    let mut conns = state.connections.lock().await;
    conns.remove(&conn_id);
    Ok(())
}

/// Send a handshake Hello message and receive Welcome response.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_handshake(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
) -> Result<Vec<SessionInfo>, String> {
    let conns = state.connections.lock().await;
    let stream = conns
        .get(&conn_id)
        .ok_or_else(|| "Connection not found".to_string())?
        .clone();
    drop(conns);

    let mut stream = stream.lock().await;

    // Send Hello
    let hello = HelloMsg {
        client_type: ClientType::Gui,
        protocol_version: PROTOCOL_VERSION,
    };
    let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    stream
        .write_all(&body)
        .await
        .map_err(|e| format!("Write error: {}", e))?;

    // Read Welcome response
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_LENGTH {
        return Err("Frame too large".to_string());
    }

    let mut frame_buf = vec![0u8; frame_len];
    stream
        .read_exact(&mut frame_buf)
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    let welcome_msg =
        MuxMessage::from_frame_body(&frame_buf).ok_or_else(|| "Invalid frame".to_string())?;
    if welcome_msg.msg_type != MessageType::Welcome {
        return Err(format!("Expected Welcome, got {:?}", welcome_msg.msg_type));
    }

    let welcome: WelcomeMsg = welcome_msg
        .decode_payload()
        .ok_or_else(|| "Invalid Welcome payload".to_string())?;

    match welcome {
        WelcomeMsg::Accepted { sessions, .. } => Ok(sessions),
        WelcomeMsg::Rejected { reason } => Err(format!("Connection rejected: {}", reason)),
    }
}

/// Send raw PTY input to a pane via the daemon.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_send_input(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
    pane_id: u32,
    data: Vec<u8>,
) -> Result<(), String> {
    let conns = state.connections.lock().await;
    let stream = conns
        .get(&conn_id)
        .ok_or_else(|| "Connection not found".to_string())?
        .clone();
    drop(conns);

    let msg = MuxMessage::pty_input(pane_id, data);
    let body = msg.to_frame_body();
    let len = (body.len() as u32).to_be_bytes();

    let mut stream = stream.lock().await;
    stream
        .write_all(&len)
        .await
        .map_err(|e| format!("Write error: {}", e))?;
    stream
        .write_all(&body)
        .await
        .map_err(|e| format!("Write error: {}", e))?;

    Ok(())
}

/// Validate that a socket path is in an allowed directory using canonicalization.
fn validate_socket_path(path: &str) -> Result<(), String> {
    // Reject null bytes (could bypass C-level path operations)
    if path.as_bytes().contains(&0) {
        return Err("Socket path contains null byte".to_string());
    }

    let path = std::path::Path::new(path);

    // Canonicalize: if the path exists, canonicalize it directly.
    // Otherwise, canonicalize the parent directory and append the file name.
    let canonical = if path.exists() {
        path.canonicalize()
            .map_err(|e| format!("Failed to canonicalize socket path: {}", e))?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| "Socket path has no parent directory".to_string())?;
        let file_name = path
            .file_name()
            .ok_or_else(|| "Socket path has no file name".to_string())?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize parent directory: {}", e))?;
        canonical_parent.join(file_name)
    };

    // Check canonicalized path against allowed directories
    let allowed = allowed_socket_dirs();
    for dir in &allowed {
        if canonical.starts_with(dir) {
            return Ok(());
        }
    }

    Err(format!(
        "Socket path not in allowed directory. Allowed: {:?}",
        allowed
    ))
}

/// Get list of allowed socket directories.
fn allowed_socket_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            dirs.push(std::path::PathBuf::from(runtime_dir).join("emterm").into());
        }
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(
                std::path::PathBuf::from(home)
                    .join(".local")
                    .join("run")
                    .join("emterm"),
            );
        }
        dirs.push(std::path::PathBuf::from("/tmp").join("emterm"));
    }

    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(std::path::PathBuf::from(local).join("emterm"));
        }
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_socket_path_null_byte_rejected() {
        assert!(validate_socket_path("/tmp/emterm/foo\0bar.sock").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_socket_path_traversal_rejected() {
        // Create the /tmp/emterm directory so canonicalization works
        let _ = std::fs::create_dir_all("/tmp/emterm");
        // Path traversal that resolves outside allowed dirs should be rejected
        assert!(validate_socket_path("/tmp/emterm/../etc/passwd").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_socket_path_allowed() {
        // /tmp/emterm is always in allowed dirs on Unix
        let _ = std::fs::create_dir_all("/tmp/emterm");
        assert!(validate_socket_path("/tmp/emterm/mux-default.sock").is_ok());
    }

    #[test]
    fn test_validate_socket_path_disallowed_dir() {
        // /var/run/other likely doesn't exist, so canonicalize will fail
        assert!(validate_socket_path("/var/run/other/socket.sock").is_err());
    }

    #[test]
    fn test_allowed_socket_dirs_not_empty() {
        let dirs = allowed_socket_dirs();
        assert!(!dirs.is_empty());
    }
}
