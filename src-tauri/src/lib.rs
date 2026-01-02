//! eMterm - Cross-platform terminal emulator with rich rendering capabilities.
//!
//! This is the main library for the Tauri backend, providing PTY functionality
//! and IPC commands for the frontend.

pub mod pty;

use std::io::Read;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use pty::{PtyError, PtyManager};

// ============================================================================
// Payload Types
// ============================================================================

/// Result returned from pty_spawn command.
#[derive(Serialize, Deserialize)]
pub struct SpawnResult {
    session_id: String,
}

/// Payload for pty_output event.
#[derive(Serialize, Clone)]
pub struct PtyOutputPayload {
    session_id: String,
    data: Vec<u8>,
}

/// Payload for pty_exit event.
#[derive(Serialize, Clone)]
pub struct PtyExitPayload {
    session_id: String,
    code: i32,
}

/// Payload for pty_error event.
#[derive(Serialize, Clone)]
pub struct PtyErrorPayload {
    session_id: String,
    message: String,
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Spawns a new PTY session with the specified shell and dimensions.
///
/// # Arguments
///
/// * `shell` - Optional path to the shell executable. If None, uses default shell.
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
    shell: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SpawnResult, String> {
    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);

    let session_id = state
        .create_session(shell, cols, rows)
        .await
        .map_err(|e| e.to_string())?;

    // Start output reader thread
    spawn_reader_thread(app, state.inner().clone(), session_id.clone());

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
async fn pty_kill(state: State<'_, PtyManager>, session_id: String) -> Result<(), String> {
    if let Some(session) = state.remove_session(&session_id).await {
        let mut session = session.lock().await;
        session.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ============================================================================
// Reader Thread
// ============================================================================

/// Spawns a dedicated thread to read output from a PTY session.
///
/// This thread continuously reads from the PTY and emits events to the frontend:
/// - `pty_output`: When data is available
/// - `pty_error`: When an error occurs
/// - `pty_exit`: When the process exits
fn spawn_reader_thread(app: AppHandle, manager: PtyManager, session_id: String) {
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
        drop(session_guard);

        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let payload = PtyOutputPayload {
                        session_id: session_id.clone(),
                        data: buf[..n].to_vec(),
                    };
                    let _ = app.emit("pty_output", payload);
                }
                Err(e) => {
                    let payload = PtyErrorPayload {
                        session_id: session_id.clone(),
                        message: e.to_string(),
                    };
                    let _ = app.emit("pty_error", payload);
                    break;
                }
            }
        }

        // Check exit status
        if let Some(session) = futures::executor::block_on(manager.get_session(&session_id)) {
            let mut session = futures::executor::block_on(session.lock());
            if let Ok(Some(status)) = session.try_wait() {
                let code = status.exit_code() as i32;
                let payload = PtyExitPayload {
                    session_id: session_id.clone(),
                    code,
                };
                let _ = app.emit("pty_exit", payload);
            }
        }

        // Cleanup
        futures::executor::block_on(manager.remove_session(&session_id));
    });
}

// ============================================================================
// Application Entry Point
// ============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(PtyManager::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn, pty_write, pty_resize, pty_kill,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
