use crate::payloads::*;
use crate::pty::PtyManager;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};

/// Spawns a dedicated thread to read output from a PTY session.
///
/// This thread continuously reads from the PTY and sends raw bytes via Channel:
/// - Binary data is sent via `Channel<InvokeResponseBody>` as raw bytes for WASM processing
/// - `pty_error`: When an error occurs
/// - `pty_exit`: When the process exits
///
/// Uses a separate monitoring thread to detect process exit, since PTY read()
/// on Linux may not return EOF even after the shell process terminates.
pub fn spawn_reader_thread(
    app: AppHandle,
    manager: PtyManager,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
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

        log::trace!("PTY reader: starting read loop for session {}", session_id);

        // Use a helper thread for blocking read + mpsc channel so that
        // the main reader loop can periodically check process_exited.
        // This fixes Windows PTY exit detection where read() blocks
        // indefinitely after the shell exits.
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(16);
        let helper_session_id = session_id.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            #[cfg(unix)]
            let mut kitty_scanner = crate::pty::kitty_scanner::KittyScanner::new();

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        log::debug!(
                            "PTY reader helper: EOF received for session {}",
                            helper_session_id
                        );
                        break;
                    }
                    Ok(n) => {
                        // Scan for Kitty APC sequences and write responses directly
                        // to the master fd via libc::write() (zero latency).
                        #[cfg(unix)]
                        if let Some(fd) = master_fd {
                            kitty_scanner.process(&buf[..n], fd);
                        }

                        if tx.send(buf[..n].to_vec()).is_err() {
                            // Receiver dropped (main loop exited)
                            break;
                        }
                    }
                    #[cfg(unix)]
                    Err(e) if e.raw_os_error() == Some(libc::EIO) => {
                        log::debug!(
                            "PTY reader helper: EIO (slave closed) for session {}",
                            helper_session_id
                        );
                        break;
                    }
                    Err(e) => {
                        log::warn!(
                            "PTY reader helper: read error for session {}: {}",
                            helper_session_id,
                            e
                        );
                        break;
                    }
                }
            }
        });

        // Main reader loop: receive data from helper thread, check process_exited on timeout
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(first) => {
                    // Drain all available chunks and concatenate to reduce IPC calls.
                    // Larger batches reduce IPC overhead at the cost of latency.
                    // For bulk output (e.g., seq 10M), fewer IPC messages is critical.
                    let mut batch = first;
                    while let Ok(more) = rx.try_recv() {
                        batch.extend_from_slice(&more);
                        if batch.len() >= 1024 * 1024 {
                            break;
                        }
                    }
                    let len = batch.len();
                    if let Err(e) = channel.send(InvokeResponseBody::Raw(batch)) {
                        log::warn!(
                            "PTY reader: channel.send failed for session {} ({} bytes lost): {}",
                            session_id,
                            len,
                            e
                        );
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Check if process has exited (signaled by monitoring thread)
                    if process_exited.load(Ordering::SeqCst) {
                        log::debug!(
                            "PTY reader: process exit detected for session {}",
                            session_id
                        );
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Helper thread exited (EOF, EIO, or read error)
                    log::debug!(
                        "PTY reader: helper thread exited for session {}",
                        session_id
                    );
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
