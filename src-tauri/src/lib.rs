//! eMterm - Cross-platform terminal emulator with rich rendering capabilities.
//!
//! This is the main library for the Tauri backend, providing PTY functionality
//! and IPC commands for the frontend.

pub mod ansi;
pub mod pty;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

/// Payload for terminal_actions event (parsed ANSI sequences).
#[derive(Serialize, Clone)]
pub struct TerminalActionsPayload {
    session_id: String,
    actions: Vec<ansi::TerminalAction>,
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
/// - `pty_output`: Raw data (for backward compatibility)
/// - `terminal_actions`: Parsed ANSI sequences as TerminalAction array
/// - `pty_error`: When an error occurs
/// - `pty_exit`: When the process exits
///
/// Uses a separate monitoring thread to detect process exit, since PTY read()
/// on Linux may not return EOF even after the shell process terminates.
fn spawn_reader_thread(app: AppHandle, manager: PtyManager, session_id: String) {
    // Shared flag to signal reader to stop when process exits
    let process_exited = Arc::new(AtomicBool::new(false));
    let process_exited_clone = Arc::clone(&process_exited);
    let manager_clone = manager.clone();
    let session_id_clone = session_id.clone();

    // Spawn a monitoring thread to check process exit status periodically
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));

            let session =
                futures::executor::block_on(manager_clone.get_session(&session_id_clone));
            let Some(session) = session else {
                // Session removed, exit monitoring
                break;
            };

            let mut session = futures::executor::block_on(session.lock());
            match session.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited
                    eprintln!(
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
                    eprintln!(
                        "PTY monitor: try_wait error for session {}: {}",
                        session_id_clone, e
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
        let mut parser = ansi::Parser::new();

        eprintln!("PTY reader: starting read loop for session {}", session_id);

        loop {
            // Check if process has exited (signaled by monitoring thread)
            if process_exited.load(Ordering::SeqCst) {
                eprintln!(
                    "PTY reader: process exit detected for session {}",
                    session_id
                );
                break;
            }

            match reader.read(&mut buf) {
                Ok(0) => {
                    eprintln!("PTY reader: EOF received for session {}", session_id);
                    break;
                }
                Ok(n) => {
                    // Parse ANSI sequences and emit terminal_actions event
                    let mut actions = Vec::new();
                    parser.parse(&buf[..n], |action| {
                        actions.push(action);
                    });

                    // Always emit if we have actions
                    if !actions.is_empty() {
                        let payload = TerminalActionsPayload {
                            session_id: session_id.clone(),
                            actions,
                        };
                        let _ = app.emit("terminal_actions", payload);
                    }

                    // Also emit raw data for backward compatibility
                    let payload = PtyOutputPayload {
                        session_id: session_id.clone(),
                        data: buf[..n].to_vec(),
                    };
                    let _ = app.emit("pty_output", payload);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available, check if process exited then sleep briefly
                    if process_exited.load(Ordering::SeqCst) {
                        eprintln!(
                            "PTY reader: process exit detected (no data) for session {}",
                            session_id
                        );
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) if e.raw_os_error() == Some(libc::EIO) => {
                    // EIO typically means the PTY slave was closed (shell exited)
                    eprintln!("PTY reader: EIO (slave closed) for session {}", session_id);
                    break;
                }
                Err(e) => {
                    eprintln!("PTY reader: read error for session {}: {}", session_id, e);
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
        eprintln!(
            "PTY reader: checking exit status for session {}",
            session_id
        );
        if let Some(session) = futures::executor::block_on(manager.get_session(&session_id)) {
            let mut session = futures::executor::block_on(session.lock());

            // Retry up to 10 times with 50ms delay (total 500ms max wait)
            let mut exit_code: Option<i32> = None;
            for attempt in 0..10 {
                match session.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = Some(status.exit_code() as i32);
                        break;
                    }
                    Ok(None) => {
                        if attempt < 9 {
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "PTY reader: session {} - try_wait error: {}",
                            session_id, e
                        );
                        break;
                    }
                }
            }

            if let Some(code) = exit_code {
                eprintln!(
                    "PTY reader: session {} exited with code {}",
                    session_id, code
                );
                let payload = PtyExitPayload {
                    session_id: session_id.clone(),
                    code,
                };
                if let Err(e) = app.emit("pty_exit", payload) {
                    eprintln!("PTY reader: failed to emit pty_exit: {}", e);
                }
            } else {
                // Process didn't exit within timeout, emit with code -1
                eprintln!(
                    "PTY reader: session {} - process did not exit within timeout",
                    session_id
                );
                let payload = PtyExitPayload {
                    session_id: session_id.clone(),
                    code: -1,
                };
                let _ = app.emit("pty_exit", payload);
            }
        } else {
            eprintln!(
                "PTY reader: session {} not found when checking exit status",
                session_id
            );
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
