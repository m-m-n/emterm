//! eMterm - Cross-platform terminal emulator with rich rendering capabilities.
//!
//! This is the main library for the Tauri backend, providing PTY functionality
//! and IPC commands for the frontend.

pub mod ansi;
pub mod image;
pub mod pty;

// CLI command modules
pub mod commands;
pub mod encoding;
pub mod error;
pub mod protocols;
pub mod validation;

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Payload for terminal_actions event (parsed ANSI sequences).
#[derive(Serialize, Clone)]
pub struct TerminalActionsPayload {
    session_id: String,
    actions: Vec<ansi::TerminalAction>,
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

    // Use atomic method to get session_id and count in one lock (NFR2 compliance)
    let result = state
        .create_session_atomic(shell, cols, rows)
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

/// Debug log command - prints message to stderr.
#[tauri::command]
fn debug_log(message: String) {
    eprintln!("[Frontend] {}", message);
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

            let session = futures::executor::block_on(manager_clone.get_session(&session_id_clone));
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

        // CRITICAL FIX: Remove session FIRST, then emit events
        // This ensures remaining_sessions is accurate when frontend receives pty_exit
        eprintln!(
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
                        eprintln!("PTY reader: session {} - try_wait error: {}", session_id, e);
                        break;
                    }
                }
            }

            (exit_code.unwrap_or(-1), result.count)
        } else {
            eprintln!(
                "PTY reader: session {} not found (already removed)",
                session_id
            );
            // Session already removed (e.g., by pty_kill), get current count
            let current_count = futures::executor::block_on(manager.session_count());
            (-1, current_count)
        };

        eprintln!(
            "PTY reader: session {} exited with code {}, {} sessions remaining",
            session_id, exit_code, remaining_sessions
        );

        // Emit pty_exit with remaining_sessions count (session already removed)
        let payload = PtyExitPayload {
            session_id: session_id.clone(),
            code: exit_code,
            remaining_sessions,
        };
        if let Err(e) = app.emit("pty_exit", payload) {
            eprintln!("PTY reader: failed to emit pty_exit: {}", e);
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
        .manage(PtyManager::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            debug_log,
            session_count,
            tab_close_graceful,
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .target(tauri_plugin_log::Target::new(
                            tauri_plugin_log::TargetKind::Stdout,
                        ))
                        .build(),
                )?;
            }
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
        let session_id = manager.create_session(None, 80, 24).await.unwrap();
        assert_eq!(manager.session_count().await, 1);

        // Create another session
        let session_id2 = manager.create_session(None, 80, 24).await.unwrap();
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
}
