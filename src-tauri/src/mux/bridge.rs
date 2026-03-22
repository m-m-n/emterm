//! Tauri IPC commands bridging GUI <-> daemon IPC socket.
//!
//! The GUI cannot directly access Unix sockets from WebView JavaScript.
//! These Tauri commands act as a bridge, forwarding messages between
//! the frontend and the daemon's Unix domain socket.

#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::sync::Arc;

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use tokio::sync::Mutex;

use super::ipc::protocol::*;

/// State for managing mux daemon connections from the GUI.
///
/// Stores split read/write halves separately so the background output
/// reader task does not block write operations (input, control messages).
#[cfg(unix)]
pub struct MuxBridgeState {
    writers: Mutex<HashMap<String, Arc<Mutex<WriteHalf<UnixStream>>>>>,
    readers: Mutex<HashMap<String, Arc<Mutex<ReadHalf<UnixStream>>>>>,
}

#[cfg(unix)]
impl MuxBridgeState {
    pub fn new() -> Self {
        Self {
            writers: Mutex::new(HashMap::new()),
            readers: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(unix)]
impl Default for MuxBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub state for platforms where mux is not supported.
#[cfg(not(unix))]
pub struct MuxBridgeState;

#[cfg(not(unix))]
impl MuxBridgeState {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(unix))]
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
    #[cfg(unix)]
    {
        validate_socket_path(&socket_path)?;

        let stream = UnixStream::connect(&socket_path)
            .await
            .map_err(|e| format!("Failed to connect to daemon: {}", e))?;

        let (read_half, write_half) = tokio::io::split(stream);
        let conn_id = uuid::Uuid::new_v4().to_string();

        state
            .writers
            .lock()
            .await
            .insert(conn_id.clone(), Arc::new(Mutex::new(write_half)));
        state
            .readers
            .lock()
            .await
            .insert(conn_id.clone(), Arc::new(Mutex::new(read_half)));

        Ok(conn_id)
    }
    #[cfg(not(unix))]
    {
        let _ = (&state, &socket_path);
        Err("Mux is not supported on this platform".to_string())
    }
}

/// Disconnect from the mux daemon.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_disconnect(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        state.writers.lock().await.remove(&conn_id);
        state.readers.lock().await.remove(&conn_id);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (&state, &conn_id);
        Err("Mux is not supported on this platform".to_string())
    }
}

/// Send a handshake Hello message and receive Welcome response.
///
/// Must be called before `mux_start_output_stream`, which takes
/// ownership of the reader half.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_handshake(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
) -> Result<Vec<SessionInfo>, String> {
    #[cfg(unix)]
    {
        // Send Hello via writer
        {
            let writers = state.writers.lock().await;
            let writer = writers
                .get(&conn_id)
                .ok_or_else(|| "Connection not found".to_string())?
                .clone();
            drop(writers);

            let mut writer = writer.lock().await;
            let hello = HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            };
            let msg = MuxMessage::control(MessageType::Hello, 0, &hello);
            let body = msg.to_frame_body();
            let len = (body.len() as u32).to_be_bytes();
            writer
                .write_all(&len)
                .await
                .map_err(|e| format!("Write error: {}", e))?;
            writer
                .write_all(&body)
                .await
                .map_err(|e| format!("Write error: {}", e))?;
        }

        // Read Welcome via reader
        let readers = state.readers.lock().await;
        let reader = readers
            .get(&conn_id)
            .ok_or_else(|| "Reader not found".to_string())?
            .clone();
        drop(readers);

        let mut reader = reader.lock().await;

        let mut len_buf = [0u8; 4];
        reader
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("Read error: {}", e))?;
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        if frame_len > MAX_FRAME_LENGTH {
            return Err("Frame too large".to_string());
        }

        let mut frame_buf = vec![0u8; frame_len];
        reader
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
    #[cfg(not(unix))]
    {
        let _ = (&state, &conn_id);
        Err("Mux is not supported on this platform".to_string())
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
    #[cfg(unix)]
    {
        let writers = state.writers.lock().await;
        let writer = writers
            .get(&conn_id)
            .ok_or_else(|| "Connection not found".to_string())?
            .clone();
        drop(writers);

        let msg = MuxMessage::pty_input(pane_id, data);
        let body = msg.to_frame_body();
        let len = (body.len() as u32).to_be_bytes();

        let mut writer = writer.lock().await;
        writer
            .write_all(&len)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        writer
            .write_all(&body)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (&state, &conn_id, pane_id, &data);
        Err("Mux is not supported on this platform".to_string())
    }
}

/// Send a control message to the daemon (fire-and-forget).
///
/// Builds a `MuxMessage` from the given type, pane ID, and raw payload,
/// then sends it over the write half. Does not attempt to read a response
/// because the read half is owned by the output stream background task.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_send_control(
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
    msg_type: u8,
    pane_id: u32,
    payload: Vec<u8>,
) -> Result<Option<Vec<u8>>, String> {
    #[cfg(unix)]
    {
        let mt = MessageType::from_u8(msg_type)
            .ok_or_else(|| format!("Unknown message type: 0x{:02X}", msg_type))?;

        let writers = state.writers.lock().await;
        let writer = writers
            .get(&conn_id)
            .ok_or_else(|| "Connection not found".to_string())?
            .clone();
        drop(writers);

        let msg = MuxMessage {
            msg_type: mt,
            pane_id,
            payload,
        };
        let body = msg.to_frame_body();
        let len = (body.len() as u32).to_be_bytes();

        let mut writer = writer.lock().await;
        writer
            .write_all(&len)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        writer
            .write_all(&body)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        // NOTE: Response reading is handled by the output stream background task.
        // Control responses arrive as events via mux_start_output_stream.
        Ok(None)
    }
    #[cfg(not(unix))]
    {
        let _ = (&state, &conn_id, msg_type, pane_id, &payload);
        Err("Mux is not supported on this platform".to_string())
    }
}

/// Start a background task that continuously reads PTY output from the daemon
/// and emits Tauri events to the frontend.
///
/// Takes ownership of the reader half for this connection. The handshake
/// must be completed before calling this function.
///
/// The task reads length-prefixed frames, filters for PtyOutput and
/// PtyExited messages, and emits corresponding Tauri events. The task
/// runs until the connection closes or an error occurs.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn mux_start_output_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, MuxBridgeState>,
    conn_id: String,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        // Take ownership of the reader -- only the background task will use it
        let reader = {
            let mut readers = state.readers.lock().await;
            readers
                .remove(&conn_id)
                .ok_or_else(|| "Reader not found".to_string())?
        };

        tokio::spawn(async move {
            let mut reader = reader.lock().await;
            loop {
                let mut len_buf = [0u8; 4];
                if AsyncReadExt::read_exact(&mut *reader, &mut len_buf)
                    .await
                    .is_err()
                {
                    break;
                }
                let frame_len = u32::from_be_bytes(len_buf) as usize;
                if frame_len > MAX_FRAME_LENGTH || frame_len == 0 {
                    break;
                }

                let mut frame_buf = vec![0u8; frame_len];
                if AsyncReadExt::read_exact(&mut *reader, &mut frame_buf)
                    .await
                    .is_err()
                {
                    break;
                }

                if let Some(msg) = MuxMessage::from_frame_body(&frame_buf) {
                    log::info!(
                        "Bridge output stream: received {:?} for pane {} ({} bytes)",
                        msg.msg_type,
                        msg.pane_id,
                        msg.payload.len()
                    );
                    match msg.msg_type {
                        MessageType::PtyOutput => {
                            use tauri::Emitter;
                            let _ = app.emit(
                                "mux-pty-output",
                                MuxPtyOutputEvent {
                                    pane_id: msg.pane_id,
                                    data: msg.payload,
                                },
                            );
                        }
                        MessageType::PtyExited => {
                            use tauri::Emitter;
                            let exit_msg: Option<PtyExitedMsg> = msg.decode_payload();
                            let _ = app.emit(
                                "mux-pty-exited",
                                MuxPtyExitedEvent {
                                    pane_id: msg.pane_id,
                                    exit_code: exit_msg.and_then(|m| m.exit_code),
                                },
                            );
                        }
                        MessageType::PaneCreated => {
                            use tauri::Emitter;
                            let _ = app.emit(
                                "mux-pane-created",
                                MuxPaneCreatedEvent {
                                    pane_id: msg.pane_id,
                                },
                            );
                        }
                        MessageType::Detached => {
                            use tauri::Emitter;
                            let _ = app.emit("mux-detached", ());
                            break; // Daemon closed connection
                        }
                        _ => {
                            log::debug!(
                                "Output stream ignoring {:?} for pane {}",
                                msg.msg_type,
                                msg.pane_id
                            );
                        }
                    }
                }
            }
            log::info!("Mux output stream ended for connection");
        });

        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (&app, &state, &conn_id);
        Err("Mux is not supported on this platform".to_string())
    }
}

/// Pane created event emitted to the frontend.
#[derive(Clone, serde::Serialize)]
struct MuxPaneCreatedEvent {
    pane_id: u32,
}

/// PTY output event emitted to the frontend.
#[derive(Clone, serde::Serialize)]
struct MuxPtyOutputEvent {
    pane_id: u32,
    data: Vec<u8>,
}

/// PTY exit event emitted to the frontend.
#[derive(Clone, serde::Serialize)]
struct MuxPtyExitedEvent {
    pane_id: u32,
    exit_code: Option<u32>,
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
            dirs.push(std::path::PathBuf::from(runtime_dir).join("emterm"));
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

/// Start or locate the mux daemon and return the socket path.
///
/// If the daemon is not running, spawns it as a background process.
/// Returns the socket path for the frontend to connect to directly,
/// bypassing the CLI → OSC → PTY parser roundtrip.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn mux_start_daemon() -> Result<String, String> {
    #[cfg(unix)]
    {
        use super::daemon;

        let sock_path = daemon::socket_path();

        // Start daemon if not running (check socket file only, avoid ghost connections)
        if !sock_path.exists() {
            // Ensure parent directory exists
            if let Some(parent) = sock_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create socket directory: {}", e))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        parent,
                        std::fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let exe = std::env::current_exe()
                .map_err(|e| format!("Failed to get executable path: {}", e))?;

            let log_path = sock_path.with_file_name("mux-daemon.log");
            let log_file = std::fs::File::create(&log_path)
                .unwrap_or_else(|_| std::fs::File::create("/tmp/emterm-mux-daemon.log").unwrap());

            std::process::Command::new(&exe)
                .args(["mux", "--daemon"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::from(log_file))
                .spawn()
                .map_err(|e| format!("Failed to spawn daemon: {}", e))?;

            // Wait for daemon to start with exponential backoff
            let mut started = false;
            for i in 0..50 {
                if daemon::is_daemon_running(&sock_path) {
                    started = true;
                    break;
                }
                let delay = std::cmp::min(10 * (1 << i.min(4)), 100);
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            if !started {
                return Err("Failed to start mux daemon".to_string());
            }
        }

        Ok(sock_path.to_string_lossy().to_string())
    }
    #[cfg(not(unix))]
    {
        Err("Mux is not supported on this platform".to_string())
    }
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
