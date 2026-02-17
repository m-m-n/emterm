//! eMterm - Cross-platform terminal emulator with rich rendering capabilities.
//!
//! This is the main library for the Tauri backend, providing PTY functionality
//! and IPC commands for the frontend.

rust_i18n::i18n!("locales", fallback = "en");

pub mod ansi;
pub mod image;
pub mod logging;
pub mod pty;

// CLI command modules
pub mod commands;
pub mod encoding;
pub mod error;
pub mod protocols;
pub mod validation;

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use pty::{PtyError, PtyManager};

/// Per-session image processor state.
///
/// Maintains `ImageProcessor` instances per PTY session to preserve
/// state across multiple `process_image_data` calls (e.g., chunked
/// Kitty transfers that require accumulating data across APC sequences).
#[derive(Default)]
pub struct ImageProcessorState {
    processors: Mutex<HashMap<String, image::ImageProcessor>>,
}

impl ImageProcessorState {
    pub fn new() -> Self {
        Self {
            processors: Mutex::new(HashMap::new()),
        }
    }

    /// Remove processor state for a session (cleanup on exit).
    pub async fn remove(&self, session_id: &str) {
        self.processors.lock().await.remove(session_id);
    }
}

// ============================================================================
// Payload Types
// ============================================================================

/// Result returned from pty_spawn command.
#[derive(Serialize, Deserialize)]
pub struct SpawnResult {
    session_id: String,
}

/// Payload for pty_exit event.
#[derive(Serialize, Clone)]
pub struct PtyExitPayload {
    session_id: String,
    code: i32,
    /// Number of remaining sessions after this session is removed.
    /// Used by frontend to determine if window should close.
    remaining_sessions: usize,
}

/// Payload for pty_error event.
#[derive(Serialize, Clone)]
pub struct PtyErrorPayload {
    session_id: String,
    message: String,
}

/// Payload for tab_created event.
#[derive(Serialize, Clone)]
pub struct TabCreatedPayload {
    session_id: String,
}

/// Payload for tab_closed event.
#[derive(Serialize, Clone)]
pub struct TabClosedPayload {
    session_id: String,
    exit_code: i32,
}

/// Payload for tab_count_changed event.
#[derive(Serialize, Clone)]
pub struct TabCountChangedPayload {
    count: usize,
}

/// Payload for image_event IPC channel.
///
/// Wraps an `ImageEvent` with the associated session ID for routing
/// events to the correct terminal session in the frontend.
#[derive(Serialize, Clone)]
pub struct ImageEventPayload {
    /// Session ID for event routing.
    pub session_id: String,

    /// The image event.
    #[serde(flatten)]
    pub event: image::ImageEvent,
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Spawns a new PTY session with the specified shell and dimensions.
///
/// # Arguments
///
/// * `shell` - Optional path to the shell executable. If None, uses default shell.
/// * `args` - Optional arguments to pass to the shell.
/// * `cols` - Number of columns (default: 80)
/// * `rows` - Number of rows (default: 24)
///
/// # Returns
///
/// A `SpawnResult` containing the session ID, or an error message.
#[tauri::command]
async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyManager>,
    channel: Channel<Vec<u8>>,
    shell: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SpawnResult, String> {
    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);

    // Use atomic method to get session_id and count in one lock (NFR2 compliance)
    let result = state
        .create_session_atomic(shell, args, cols, rows)
        .await
        .map_err(|e| e.to_string())?;

    let session_id = result.session_id;
    let count = result.count;

    // Emit tab lifecycle events (count was captured inside the lock)
    let _ = app.emit(
        "tab_created",
        TabCreatedPayload {
            session_id: session_id.clone(),
        },
    );
    let _ = app.emit("tab_count_changed", TabCountChangedPayload { count });

    // Start output reader thread with binary channel
    spawn_reader_thread(app, state.inner().clone(), session_id.clone(), channel);

    Ok(SpawnResult { session_id })
}

/// Writes data to a PTY session.
///
/// # Arguments
///
/// * `session_id` - The target session ID
/// * `data` - Bytes to write to the PTY
#[tauri::command]
async fn pty_write(
    state: State<'_, PtyManager>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| PtyError::SessionNotFound(session_id.clone()).to_string())?;

    let session = session.lock().await;
    session.write(&data).map_err(|e| e.to_string())
}

/// Resizes a PTY session.
///
/// # Arguments
///
/// * `session_id` - The target session ID
/// * `cols` - New number of columns
/// * `rows` - New number of rows
#[tauri::command]
async fn pty_resize(
    state: State<'_, PtyManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = state
        .get_session(&session_id)
        .await
        .ok_or_else(|| PtyError::SessionNotFound(session_id).to_string())?;

    let session = session.lock().await;
    session.resize(cols, rows).map_err(|e| e.to_string())
}

/// Kills a PTY session.
///
/// # Arguments
///
/// * `session_id` - The session ID to kill
#[tauri::command]
async fn pty_kill(
    app: AppHandle,
    state: State<'_, PtyManager>,
    session_id: String,
) -> Result<(), String> {
    if let Some((session, result)) = state.remove_session_atomic(&session_id).await {
        let mut session = session.lock().await;
        session.kill().map_err(|e| e.to_string())?;

        // Emit tab lifecycle events (NFR2 compliance)
        let _ = app.emit(
            "tab_closed",
            TabClosedPayload {
                session_id: session_id.clone(),
                exit_code: -1, // Killed
            },
        );
        let _ = app.emit(
            "tab_count_changed",
            TabCountChangedPayload {
                count: result.count,
            },
        );
    }
    Ok(())
}

/// Console log command - prints message to stdout with [LOG][FRONTEND] prefix.
#[tauri::command]
fn console_log(message: String) {
    println!("{}", logging::format_frontend_log("log", &message));
}

/// Console warn command - prints message to stderr with [WARN][FRONTEND] prefix.
#[tauri::command]
fn console_warn(message: String) {
    eprintln!("{}", logging::format_frontend_log("warn", &message));
}

/// Console error command - prints message to stderr with [ERROR][FRONTEND] prefix.
#[tauri::command]
fn console_error(message: String) {
    eprintln!("{}", logging::format_frontend_log("error", &message));
}

/// Console info command - prints message to stdout with [INFO][FRONTEND] prefix.
#[tauri::command]
fn console_info(message: String) {
    println!("{}", logging::format_frontend_log("info", &message));
}

/// Console debug command - prints message to stdout with [DEBUG][FRONTEND] prefix.
#[tauri::command]
fn console_debug(message: String) {
    println!("{}", logging::format_frontend_log("debug", &message));
}

/// Sets the backend locale at runtime.
///
/// Called from the frontend to synchronize language settings.
///
/// # Arguments
///
/// * `language` - Language code ("en" or "ja")
///
/// # Returns
///
/// Ok(()) on success, or Err with unsupported language message.
#[tauri::command]
fn set_language(language: String) -> Result<(), String> {
    const SUPPORTED: &[&str] = &["en", "ja"];
    if SUPPORTED.contains(&language.as_str()) {
        rust_i18n::set_locale(&language);
        Ok(())
    } else {
        Err(format!("Unsupported language: {}", language))
    }
}

/// Processes image data (Kitty/SIXEL) from the frontend WASM parser.
///
/// Called when the WASM APC or DCS callback fires with image protocol data.
/// Parses the raw data, runs it through the per-session ImageProcessor,
/// and emits `image_event` IPC events to the frontend.
///
/// # Arguments
///
/// * `session_id` - The PTY session ID for event routing
/// * `protocol` - Image protocol: "kitty" or "sixel"
/// * `data` - Raw protocol data bytes
/// * `cursor_row` - Current cursor row (0-based)
/// * `cursor_col` - Current cursor column (0-based)
#[tauri::command]
async fn process_image_data(
    app: AppHandle,
    image_state: State<'_, ImageProcessorState>,
    session_id: String,
    protocol: String,
    data: Vec<u8>,
    cursor_row: u32,
    cursor_col: u32,
) -> Result<(), String> {
    let mut processors = image_state.processors.lock().await;
    let processor = processors
        .entry(session_id.clone())
        .or_insert_with(image::ImageProcessor::new);

    let events = match protocol.as_str() {
        "kitty" => {
            if let Some(cmd) = ansi::apc::parse_kitty_command(&data) {
                processor.process_kitty_command(&cmd, cursor_row, cursor_col)
            } else {
                return Ok(());
            }
        }
        "sixel" => {
            if let Some(sixel) = ansi::dcs::parse_sixel_sequence(&data) {
                processor.process_sixel(&sixel, cursor_row, cursor_col)
            } else {
                return Ok(());
            }
        }
        _ => {
            return Err(format!("Unknown image protocol: {}", protocol));
        }
    };

    for event in events {
        let payload = ImageEventPayload {
            session_id: session_id.clone(),
            event,
        };
        app.emit("image_event", payload)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Returns the number of active PTY sessions.
///
/// This command exposes the existing `PtyManager::session_count()` method
/// to the frontend, enabling tab-aware window close logic.
#[tauri::command]
async fn session_count(state: State<'_, PtyManager>) -> Result<usize, String> {
    Ok(state.session_count().await)
}

/// Gracefully closes a PTY session using a 3-stage shutdown sequence.
///
/// # Stages
///
/// 1. Send "exit\n" command and wait (configurable timeout)
/// 2. Send EOF (0x04) and wait (configurable timeout)
/// 3. Force kill the process
///
/// # Arguments
///
/// * `session_id` - The session ID to close gracefully
/// * `timeout_ms` - Optional total timeout in milliseconds (default: 7000ms)
///                  The timeout is distributed proportionally between stages.
#[tauri::command]
async fn tab_close_graceful(
    state: State<'_, PtyManager>,
    session_id: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let config = match timeout_ms {
        Some(ms) => pty::graceful_shutdown::ShutdownConfig::from_total_ms(ms),
        None => pty::graceful_shutdown::ShutdownConfig::default(),
    };
    pty::graceful_shutdown::shutdown_with_config(&state, &session_id, config).await
}

// ============================================================================
// Reader Thread
// ============================================================================

/// Spawns a dedicated thread to read output from a PTY session.
///
/// This thread continuously reads from the PTY and sends raw bytes via Channel:
/// - Binary data is sent via `Channel<Vec<u8>>` for WASM processing
/// - `pty_error`: When an error occurs
/// - `pty_exit`: When the process exits
///
/// Uses a separate monitoring thread to detect process exit, since PTY read()
/// on Linux may not return EOF even after the shell process terminates.
fn spawn_reader_thread(
    app: AppHandle,
    manager: PtyManager,
    session_id: String,
    channel: Channel<Vec<u8>>,
) {
    // Shared flag to signal reader to stop when process exits
    let process_exited = Arc::new(AtomicBool::new(false));
    let process_exited_clone = Arc::clone(&process_exited);
    let manager_clone = manager.clone();
    let session_id_clone = session_id.clone();

    // Spawn a monitoring thread to check process exit status periodically
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));

            let session = futures::executor::block_on(manager_clone.get_session(&session_id_clone));
            let Some(session) = session else {
                // Session removed, exit monitoring
                break;
            };

            let mut session = futures::executor::block_on(session.lock());
            match session.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited
                    log::debug!(
                        "PTY monitor: detected process exit for session {}",
                        session_id_clone
                    );
                    process_exited_clone.store(true, Ordering::SeqCst);
                    break;
                }
                Ok(None) => {
                    // Process still running, continue monitoring
                }
                Err(e) => {
                    log::warn!(
                        "PTY monitor: try_wait error for session {}: {}",
                        session_id_clone,
                        e
                    );
                    break;
                }
            }
        }
    });

    std::thread::spawn(move || {
        // Get the session and take the reader
        let session = futures::executor::block_on(manager.get_session(&session_id));
        let Some(session) = session else {
            return;
        };

        let session_guard = futures::executor::block_on(session.lock());
        let Ok(mut reader) = session_guard.take_reader() else {
            let _ = app.emit(
                "pty_error",
                PtyErrorPayload {
                    session_id: session_id.clone(),
                    message: "Failed to take reader".to_string(),
                },
            );
            return;
        };

        // Get the master fd before dropping the session guard (Unix only)
        #[cfg(unix)]
        let master_fd = session_guard.master_fd();
        drop(session_guard);

        // Set non-blocking mode on the PTY master (Unix only)
        #[cfg(unix)]
        if let Some(fd) = master_fd {
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }

        let mut buf = [0u8; 4096];

        log::trace!("PTY reader: starting read loop for session {}", session_id);

        loop {
            // Check if process has exited (signaled by monitoring thread)
            if process_exited.load(Ordering::SeqCst) {
                log::debug!(
                    "PTY reader: process exit detected for session {}",
                    session_id
                );
                break;
            }

            match reader.read(&mut buf) {
                Ok(0) => {
                    log::debug!("PTY reader: EOF received for session {}", session_id);
                    break;
                }
                Ok(n) => {
                    // Send raw bytes via Channel for WASM processing
                    let _ = channel.send(buf[..n].to_vec());
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, check if process exited then sleep briefly
                    if process_exited.load(Ordering::SeqCst) {
                        log::debug!(
                            "PTY reader: process exit detected (no data) for session {}",
                            session_id
                        );
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                #[cfg(unix)]
                Err(e) if e.raw_os_error() == Some(libc::EIO) => {
                    // EIO typically means the PTY slave was closed (shell exited)
                    log::debug!("PTY reader: EIO (slave closed) for session {}", session_id);
                    break;
                }
                Err(e) => {
                    log::warn!("PTY reader: read error for session {}: {}", session_id, e);
                    let payload = PtyErrorPayload {
                        session_id: session_id.clone(),
                        message: e.to_string(),
                    };
                    let _ = app.emit("pty_error", payload);
                    break;
                }
            }
        }

        // CRITICAL FIX: Remove session FIRST, then emit events
        // This ensures remaining_sessions is accurate when frontend receives pty_exit
        log::debug!(
            "PTY reader: removing session and checking exit status for {}",
            session_id
        );

        // Use atomic method to remove session and get remaining count (NFR2 compliance)
        let (exit_code, remaining_sessions) = if let Some((session, result)) =
            futures::executor::block_on(manager.remove_session_atomic(&session_id))
        {
            // Session is now removed from HashMap, but we still have ownership via Arc
            let mut session_guard = futures::executor::block_on(session.lock());

            // Retry up to 10 times with 50ms delay (total 500ms max wait)
            let mut exit_code: Option<i32> = None;
            for attempt in 0..10 {
                match session_guard.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = Some(status.exit_code() as i32);
                        break;
                    }
                    Ok(None) => {
                        if attempt < 9 {
                            // Release lock during sleep, but we keep the Arc
                            drop(session_guard);
                            std::thread::sleep(Duration::from_millis(50));
                            session_guard = futures::executor::block_on(session.lock());
                        }
                    }
                    Err(e) => {
                        log::warn!("PTY reader: session {} - try_wait error: {}", session_id, e);
                        break;
                    }
                }
            }

            (exit_code.unwrap_or(-1), result.count)
        } else {
            log::debug!(
                "PTY reader: session {} not found (already removed)",
                session_id
            );
            // Session already removed (e.g., by pty_kill), get current count
            let current_count = futures::executor::block_on(manager.session_count());
            (-1, current_count)
        };

        log::debug!(
            "PTY reader: session {} exited with code {}, {} sessions remaining",
            session_id,
            exit_code,
            remaining_sessions
        );

        // Emit pty_exit with remaining_sessions count (session already removed)
        log::debug!(
            "PTY reader: emitting pty_exit event for session {}",
            session_id
        );
        let payload = PtyExitPayload {
            session_id: session_id.clone(),
            code: exit_code,
            remaining_sessions,
        };
        if let Err(e) = app.emit("pty_exit", payload) {
            log::error!("PTY reader: failed to emit pty_exit: {}", e);
        }

        // Emit tab_closed and tab_count_changed events
        let _ = app.emit(
            "tab_closed",
            TabClosedPayload {
                session_id: session_id.clone(),
                exit_code,
            },
        );
        let _ = app.emit(
            "tab_count_changed",
            TabCountChangedPayload {
                count: remaining_sessions,
            },
        );
    });
}

// ============================================================================
// Application Entry Point
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(not(test))]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .manage(PtyManager::new())
        .manage(ImageProcessorState::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            process_image_data,
            console_log,
            console_warn,
            console_error,
            console_info,
            console_debug,
            session_count,
            tab_close_graceful,
            commands::config::load_settings,
            commands::config::save_settings,
            commands::editor::check_file_exists,
            commands::editor::open_file_in_editor,
            commands::font::list_fonts,
            set_language,
        ])
        .setup(|_app| {
            // Initialize custom logger for backend
            // Use Debug level in debug builds, Info level in release builds
            let level = if cfg!(debug_assertions) {
                log::Level::Debug
            } else {
                log::Level::Info
            };
            logging::BackendLogger::init(level);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_count_command() {
        let manager = PtyManager::new();

        // Initially, session count should be 0
        assert_eq!(manager.session_count().await, 0);

        // Create a session
        let session_id = manager.create_session(None, None, 80, 24).await.unwrap();
        assert_eq!(manager.session_count().await, 1);

        // Create another session
        let session_id2 = manager.create_session(None, None, 80, 24).await.unwrap();
        assert_eq!(manager.session_count().await, 2);

        // Remove one session
        if let Some(session) = manager.remove_session(&session_id).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
        assert_eq!(manager.session_count().await, 1);

        // Remove the other session
        if let Some(session) = manager.remove_session(&session_id2).await {
            let mut s = session.lock().await;
            let _ = s.kill();
        }
        assert_eq!(manager.session_count().await, 0);
    }

    #[test]
    fn test_image_event_payload_serialization() {
        let payload = ImageEventPayload {
            session_id: "test-session-123".to_string(),
            event: image::ImageEvent::QueryResponse { supported: true },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("test-session-123"));
        assert!(json.contains("QueryResponse"));
        assert!(json.contains("supported"));
    }

    #[test]
    fn test_image_event_payload_image_ready() {
        let decoded_image = image::DecodedImage {
            id: 42,
            width: 100,
            height: 50,
            rgba_data: vec![0; 20000],
            rgba_base64: "AAAA".to_string(),
        };

        let payload = ImageEventPayload {
            session_id: "session-456".to_string(),
            event: image::ImageEvent::ImageReady {
                image: decoded_image,
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-456"));
        assert!(json.contains("ImageReady"));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"width\":100"));
        assert!(json.contains("\"height\":50"));
    }

    #[test]
    fn test_image_event_payload_place() {
        let placement = image::ImagePlacement {
            image_id: 1,
            placement_id: 2,
            row: 10,
            col: 20,
            columns: 80,
            rows: 24,
            x_offset: 0,
            y_offset: 0,
            z_index: -1,
        };

        let payload = ImageEventPayload {
            session_id: "session-789".to_string(),
            event: image::ImageEvent::Place { placement },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-789"));
        assert!(json.contains("Place"));
        assert!(json.contains("\"image_id\":1"));
        assert!(json.contains("\"placement_id\":2"));
    }

    #[test]
    fn test_image_event_payload_delete() {
        let payload = ImageEventPayload {
            session_id: "session-delete".to_string(),
            event: image::ImageEvent::Delete {
                target: image::ImageDelete::All,
            },
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("session-delete"));
        assert!(json.contains("Delete"));
        assert!(json.contains("All"));
    }
}
