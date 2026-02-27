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

/// Threshold above which image data is sent via on-demand fetch instead of events.
///
/// Tauri's event system broadcasts payloads through webview eval/postMessage,
/// which can stall or fail for very large JSON strings. By storing large
/// `rgba_base64` data separately and letting the frontend fetch it via a
/// dedicated Tauri command, we avoid passing multi-megabyte payloads through
/// the event channel.
///
/// 2 MB of base64 ≈ 1.5 MB of raw pixel data ≈ ~600×600 RGBA image.
const LARGE_IMAGE_DATA_THRESHOLD: usize = 2_000_000;

/// Temporary storage for image data too large for Tauri events.
///
/// When `rgba_base64` exceeds [`LARGE_IMAGE_DATA_THRESHOLD`], it is moved
/// here and the event payload carries an empty string. The frontend detects
/// the empty field and calls `fetch_image_data` to retrieve the data via
/// a regular Tauri command (invoke), which handles large responses reliably.
#[derive(Default)]
pub struct LargeImageDataStore {
    data: Mutex<HashMap<(String, u32), String>>,
}

impl LargeImageDataStore {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
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

/// Writes data to a PTY session via the dedicated writer channel.
///
/// This is a synchronous (non-async) command that performs a single read-lock
/// lookup in the WriterRegistry and a lock-free channel send, minimizing
/// per-keystroke overhead for fast key repeat.
///
/// # Arguments
///
/// * `session_id` - The target session ID
/// * `data` - Bytes to write to the PTY
/// Maximum allowed write size per call (1 MB).
const PTY_WRITE_MAX_SIZE: usize = 1024 * 1024;

#[tauri::command]
fn pty_write(
    state: State<'_, PtyManager>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    if data.len() > PTY_WRITE_MAX_SIZE {
        return Err(format!(
            "Write data too large: {} bytes (max {} bytes)",
            data.len(),
            PTY_WRITE_MAX_SIZE
        ));
    }
    state
        .writer_registry()
        .send(&session_id, data)
        .map_err(|e| e.to_string())
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
    large_image_store: State<'_, LargeImageDataStore>,
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

    let mut events = match protocol.as_str() {
        "kitty" => {
            if let Some(cmd) = ansi::apc::parse_kitty_command(&data) {
                processor.process_kitty_command(&cmd, cursor_row, cursor_col)
            } else {
                log::warn!("Failed to parse Kitty command ({} bytes)", data.len());
                return Ok(());
            }
        }
        "sixel" => {
            if let Some(sixel) = ansi::dcs::parse_sixel_sequence(&data) {
                processor.process_sixel(&sixel, cursor_row, cursor_col)
            } else {
                log::warn!(
                    "Failed to parse SIXEL sequence ({} bytes, first={:?})",
                    data.len(),
                    &data[..data.len().min(20)]
                );
                return Ok(());
            }
        }
        _ => {
            return Err(format!("Unknown image protocol: {}", protocol));
        }
    };

    // For large images, move rgba_base64 out of the event payload to avoid
    // overwhelming Tauri's event system. The frontend will fetch it on demand.
    for event in &mut events {
        if let image::ImageEvent::ImageReady { image } = event {
            if image.rgba_base64.len() > LARGE_IMAGE_DATA_THRESHOLD {
                let moved = std::mem::take(&mut image.rgba_base64);
                let mut store = large_image_store.data.lock().await;
                store.insert((session_id.clone(), image.id), moved);
            }
        }
    }

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

/// Fetches large image data that was omitted from the `image_event` payload.
///
/// When an image's `rgba_base64` exceeds [`LARGE_IMAGE_DATA_THRESHOLD`],
/// `process_image_data` stores it here and sends an empty string in the event.
/// The frontend calls this command to retrieve the actual pixel data.
///
/// This is a one-shot retrieval: the data is removed from the store after fetch.
#[tauri::command]
async fn fetch_image_data(
    large_image_store: State<'_, LargeImageDataStore>,
    session_id: String,
    image_id: u32,
) -> Result<String, String> {
    let mut store = large_image_store.data.lock().await;
    store
        .remove(&(session_id, image_id))
        .ok_or_else(|| format!("No deferred image data for id={}", image_id))
}

/// Processes a batch of Kitty Graphics Protocol APC sequences in a single IPC call.
///
/// Instead of sending 600+ individual `process_image_data` calls for a chunked
/// Kitty transfer (one per APC sequence), the frontend accumulates the APC bodies
/// and sends them all at once. This reduces IPC overhead from O(N) round-trips
/// to O(1), which is critical for large images (600 chunks × ~50ms/invoke = 30s).
///
/// Each string in `chunks` is the raw APC body (bytes between `ESC _` and `ESC \`),
/// e.g. `"Gi=1,f=100,a=T,m=1;base64data..."`.
#[tauri::command]
async fn process_kitty_batch(
    app: AppHandle,
    image_state: State<'_, ImageProcessorState>,
    large_image_store: State<'_, LargeImageDataStore>,
    session_id: String,
    chunks: Vec<String>,
    cursor_row: u32,
    cursor_col: u32,
) -> Result<(), String> {
    let mut processors = image_state.processors.lock().await;
    let processor = processors
        .entry(session_id.clone())
        .or_insert_with(image::ImageProcessor::new);

    let mut all_events: Vec<image::ImageEvent> = Vec::new();

    for chunk in &chunks {
        if let Some(cmd) = ansi::apc::parse_kitty_command(chunk.as_bytes()) {
            let events = processor.process_kitty_command(&cmd, cursor_row, cursor_col);
            all_events.extend(events);
        } else {
            log::warn!(
                "process_kitty_batch: parse_kitty_command failed (len={}, first_byte={:?})",
                chunk.len(),
                chunk.as_bytes().first()
            );
        }
    }

    // For large images, move rgba_base64 out of the event payload
    for event in &mut all_events {
        if let image::ImageEvent::ImageReady { image } = event {
            if image.rgba_base64.len() > LARGE_IMAGE_DATA_THRESHOLD {
                let moved = std::mem::take(&mut image.rgba_base64);
                let mut store = large_image_store.data.lock().await;
                store.insert((session_id.clone(), image.id), moved);
            }
        }
    }

    for event in all_events {
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

        // Set up Kitty APC scanner for immediate protocol response delivery.
        // This scans raw PTY output for Kitty Graphics Protocol sequences and
        // writes OK responses directly to the master fd via libc::write(),
        // bypassing ALL intermediate layers (writer channel, writer thread,
        // WebView, WASM, Tauri IPC) for true zero-latency response delivery.
        #[cfg(unix)]
        let mut kitty_scanner = pty::kitty_scanner::KittyScanner::new();

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
                    // Scan for Kitty APC sequences and write responses directly
                    // to the master fd via libc::write() (zero latency).
                    #[cfg(unix)]
                    if let Some(fd) = master_fd {
                        kitty_scanner.process(&buf[..n], fd);
                    }

                    // Send raw bytes via Channel for WASM processing
                    if let Err(e) = channel.send(buf[..n].to_vec()) {
                        log::warn!(
                            "PTY reader: channel.send failed for session {} ({} bytes lost): {}",
                            session_id,
                            n,
                            e
                        );
                        break;
                    }
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

/// Set the taskbar icon from the embedded ICO resource on Windows.
///
/// Works around a bug in tao's `CreateIcon()` where the AND mask is created with
/// 1 byte per pixel instead of 1 bit per pixel, causing alpha transparency to be
/// lost. By loading the icon directly from the embedded resource via `LoadImageW`,
/// Windows handles the ICO's alpha channel correctly.
#[cfg(windows)]
fn set_taskbar_icon(window: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        IMAGE_ICON, LR_DEFAULTSIZE, LoadImageW, SendMessageW, WM_SETICON,
    };
    use windows::core::PCWSTR;

    const ICON_BIG: usize = 1;
    const MAINICON_ID: u16 = 32512;

    let window_handle = window.window_handle()?;
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return Err("not a Win32 window".into());
    };

    unsafe {
        let hmodule = GetModuleHandleW(PCWSTR::null())?;
        let hicon = LoadImageW(
            Some(hmodule.into()),
            PCWSTR::from_raw(MAINICON_ID as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        )?;
        let hwnd = windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _);
        SendMessageW(
            hwnd,
            WM_SETICON,
            windows::Win32::Foundation::WPARAM(ICON_BIG),
            windows::Win32::Foundation::LPARAM(hicon.0 as isize),
        );
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[cfg(not(test))]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .manage(PtyManager::new())
        .manage(ImageProcessorState::new())
        .manage(LargeImageDataStore::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            process_image_data,
            process_kitty_batch,
            fetch_image_data,
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
        .setup(|app| {
            // Initialize custom logger for backend
            // Use Debug level in debug builds, Info level in release builds
            let level = if cfg!(debug_assertions) {
                log::Level::Debug
            } else {
                log::Level::Info
            };
            logging::BackendLogger::init(level);

            // On Windows, set ICON_BIG from the embedded ICO resource to fix
            // taskbar icon transparency. tao's CreateIcon() has a bug where the
            // AND mask format is incorrect, causing alpha transparency to be lost.
            // Loading directly from the resource via LoadImageW bypasses this.
            #[cfg(windows)]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(e) = set_taskbar_icon(&window) {
                        log::warn!("Failed to set taskbar icon: {e}");
                    }
                }
            }

            #[cfg(not(windows))]
            let _ = app;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(deprecated)]
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

    #[tokio::test]
    async fn test_large_image_data_store() {
        let store = LargeImageDataStore::new();

        // Store data
        {
            let mut data = store.data.lock().await;
            data.insert(
                ("session1".to_string(), 42),
                "large_base64_data".to_string(),
            );
        }

        // Retrieve data (one-shot: removes from store)
        {
            let mut data = store.data.lock().await;
            let result = data.remove(&("session1".to_string(), 42));
            assert_eq!(result, Some("large_base64_data".to_string()));
        }

        // Second retrieval should return None
        {
            let mut data = store.data.lock().await;
            let result = data.remove(&("session1".to_string(), 42));
            assert_eq!(result, None);
        }
    }

    #[test]
    fn test_large_image_data_threshold() {
        // Verify the threshold is reasonable (2MB base64 ≈ 1.5MB raw)
        assert_eq!(LARGE_IMAGE_DATA_THRESHOLD, 2_000_000);
    }

    /// End-to-end test for the batch Kitty chunk processing flow.
    ///
    /// Simulates the exact data flow: CLI generates Kitty sequence → WASM parser
    /// extracts APC bodies → frontend batches strings → backend processes via
    /// parse_kitty_command + ImageProcessor.
    #[test]
    fn test_kitty_batch_flow_end_to_end() {
        use ::image::{DynamicImage, RgbaImage};

        // Create a 100x100 test image (produces ~350 bytes PNG → 1 chunk)
        let img = DynamicImage::ImageRgba8(RgbaImage::new(100, 100));

        // Step 1: CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&img).unwrap();

        // Step 2: Extract APC bodies (simulating WASM parser)
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(!apc_bodies.is_empty(), "Should have at least one APC body");

        // Step 3: Process through batch path (simulating process_kitty_batch)
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Step 4: Verify image was decoded successfully
        let has_image_ready = all_events
            .iter()
            .any(|e| matches!(e, image::ImageEvent::ImageReady { .. }));
        assert!(has_image_ready, "Should have ImageReady event");
    }

    /// End-to-end test for large multi-chunk Kitty batch processing.
    ///
    /// Uses a larger image that produces multiple APC chunks (~4096 bytes each).
    #[test]
    fn test_kitty_batch_flow_large_image() {
        use ::image::{DynamicImage, RgbaImage};

        // Create a 400x400 image (produces ~4KB+ PNG → multiple chunks)
        let img = DynamicImage::ImageRgba8(RgbaImage::new(400, 400));

        // CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&img).unwrap();

        // Extract APC bodies
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(
            apc_bodies.len() > 1,
            "Large image should produce multiple chunks, got {}",
            apc_bodies.len()
        );

        // Process through batch path
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Verify image was decoded successfully
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(image_ready.is_some(), "Should have ImageReady event");
        let img = image_ready.unwrap();
        assert_eq!(img.width, 400);
        assert_eq!(img.height, 400);
    }

    /// Test batch flow with a very large image producing hundreds of chunks.
    /// This simulates the actual scenario: 1080x1920 image → ~2.4MB base64 → ~600 chunks.
    #[test]
    fn test_kitty_batch_flow_very_large_image() {
        use ::image::{DynamicImage, RgbaImage, Rgba};

        // Create a 1080x1920 image (matching the failing test case dimensions)
        // Fill with varied pixel data to prevent extreme compression
        let mut img = RgbaImage::new(1080, 1920);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        // CLI generates Kitty sequence
        let (sequence, _image_id) = protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();

        // Extract APC bodies
        let apc_bodies = extract_apc_bodies(&sequence);
        assert!(
            apc_bodies.len() > 100,
            "Very large image should produce many chunks, got {}",
            apc_bodies.len()
        );

        // Process through batch path
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();

        for body in &apc_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }

        // Verify image was decoded successfully
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(image_ready.is_some(), "Should have ImageReady event");
        let img = image_ready.unwrap();
        assert_eq!(img.width, 1080);
        assert_eq!(img.height, 1920);
        assert!(!img.rgba_base64.is_empty());
    }

    /// Test that simulates the full tmux DCS passthrough roundtrip.
    ///
    /// Flow: generate_kitty_sequence → wrap_each_sequence (tmux wrap)
    ///       → simulate_tmux_unwrap → extract_apc_bodies → process → verify
    #[test]
    fn test_tmux_passthrough_roundtrip_large_image() {
        use ::image::{DynamicImage, Rgba, RgbaImage};

        // Create a large image (400x400 → multiple chunks)
        let mut img = RgbaImage::new(400, 400);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        // Step 1: Generate Kitty sequence (same as CLI does)
        let (sequence, _image_id) =
            protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();

        // Extract original APC bodies (baseline)
        let original_bodies = extract_apc_bodies(&sequence);
        assert!(
            original_bodies.len() > 1,
            "Should produce multiple chunks, got {}",
            original_bodies.len()
        );

        // Step 2: Wrap for tmux (simulating passthrough_if_needed)
        let wrapped = commands::tmux::wrap_each_sequence_for_test(&sequence);

        // Verify the wrapped output is larger (DCS overhead + ESC doubling)
        assert!(wrapped.len() > sequence.len());

        // Step 3: Simulate tmux unwrapping
        let unwrapped = simulate_tmux_unwrap(&wrapped);

        // Step 4: The unwrapped data should be identical to the original
        assert_eq!(
            unwrapped, sequence,
            "Tmux roundtrip should preserve data exactly"
        );

        // Step 5: Extract APC bodies from unwrapped data
        let roundtrip_bodies = extract_apc_bodies(&unwrapped);
        assert_eq!(
            roundtrip_bodies.len(),
            original_bodies.len(),
            "Roundtrip should preserve chunk count"
        );
        for (i, (orig, rt)) in original_bodies
            .iter()
            .zip(roundtrip_bodies.iter())
            .enumerate()
        {
            assert_eq!(orig, rt, "Chunk {} differs after roundtrip", i);
        }

        // Step 6: Process through batch path → verify ImageReady
        let mut processor = image::ImageProcessor::new();
        let mut all_events: Vec<image::ImageEvent> = Vec::new();
        for body in &roundtrip_bodies {
            if let Some(cmd) = ansi::apc::parse_kitty_command(body.as_bytes()) {
                let events = processor.process_kitty_command(&cmd, 0, 0);
                all_events.extend(events);
            }
        }
        let image_ready = all_events.iter().find_map(|e| {
            if let image::ImageEvent::ImageReady { image } = e {
                Some(image)
            } else {
                None
            }
        });
        assert!(image_ready.is_some(), "Should have ImageReady after tmux roundtrip");
        let decoded = image_ready.unwrap();
        assert_eq!(decoded.width, 400);
        assert_eq!(decoded.height, 400);
    }

    /// Test tmux roundtrip with frontend-style accumulation (single assembled chunk).
    ///
    /// Simulates: tmux unwrap → WASM parser extracts APC bodies →
    /// frontend accumulates base64 → sends single chunk → backend decodes.
    #[test]
    fn test_tmux_passthrough_with_frontend_accumulation() {
        use ::image::{DynamicImage, Rgba, RgbaImage};

        let mut img = RgbaImage::new(400, 400);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]);
        }
        let dyn_img = DynamicImage::ImageRgba8(img);

        let (sequence, _) = protocols::kitty::generate_kitty_sequence(&dyn_img).unwrap();
        let wrapped = commands::tmux::wrap_each_sequence_for_test(&sequence);
        let unwrapped = simulate_tmux_unwrap(&wrapped);
        let bodies = extract_apc_bodies(&unwrapped);
        assert!(bodies.len() > 1);

        // Simulate frontend accumulation (handleApcCallback logic)
        let mut first_chunk_body: Option<String> = None;
        let mut accumulated_payload = String::new();

        for body in &bodies {
            let semicolon_idx = body.find(';');
            let params = match semicolon_idx {
                Some(idx) => &body[..idx],
                None => body.as_str(),
            };
            let payload = match semicolon_idx {
                Some(idx) => &body[idx + 1..],
                None => "",
            };
            let is_more = params.contains("m=1");

            if is_more {
                if first_chunk_body.is_none() {
                    first_chunk_body = Some(body.clone());
                }
                accumulated_payload.push_str(payload);
            } else {
                // Final chunk
                accumulated_payload.push_str(payload);

                if let Some(ref first) = first_chunk_body {
                    let first_semi = first.find(';').unwrap_or(first.len());
                    let first_params = &first[..first_semi];
                    let fixed_params = first_params.replace(",m=1", ",m=0");
                    let full_chunk =
                        format!("{};{}", fixed_params, accumulated_payload);

                    // Process the assembled chunk
                    let mut processor = image::ImageProcessor::new();
                    if let Some(cmd) =
                        ansi::apc::parse_kitty_command(full_chunk.as_bytes())
                    {
                        let events = processor.process_kitty_command(&cmd, 0, 0);
                        let image_ready = events.iter().find_map(|e| {
                            if let image::ImageEvent::ImageReady { image } = e {
                                Some(image)
                            } else {
                                None
                            }
                        });
                        assert!(
                            image_ready.is_some(),
                            "Should decode image after frontend accumulation"
                        );
                        let decoded = image_ready.unwrap();
                        assert_eq!(decoded.width, 400);
                        assert_eq!(decoded.height, 400);
                    } else {
                        panic!("Failed to parse assembled chunk");
                    }
                }
            }
        }
    }

    /// Simulate tmux unwrapping: for each DCS passthrough block, strip
    /// the `ESC P tmux;` header and `ESC \` trailer, then undouble ESC bytes.
    fn simulate_tmux_unwrap(input: &str) -> String {
        let mut output = String::new();
        let bytes = input.as_bytes();
        let header = b"\x1bPtmux;";
        let mut i = 0;

        while i < bytes.len() {
            // Look for DCS passthrough header
            if i + header.len() <= bytes.len() && &bytes[i..i + header.len()] == header {
                let body_start = i + header.len();
                // Find DCS ST by scanning for single ESC followed by \
                // (doubled ESC-ESC is content, not terminator)
                let mut j = body_start;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1B {
                        if j + 1 < bytes.len() && bytes[j + 1] == 0x1B {
                            // Doubled ESC: output single ESC, skip pair
                            output.push(0x1B as char);
                            j += 2;
                        } else if j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            // DCS ST found: terminate this block
                            i = j + 2;
                            break;
                        } else {
                            // Bare ESC followed by something else
                            output.push(0x1B as char);
                            j += 1;
                        }
                    } else {
                        output.push(bytes[j] as char);
                        j += 1;
                    }
                }
                if j + 1 >= bytes.len() {
                    break;
                }
            } else {
                // Outside DCS passthrough: copy verbatim
                output.push(bytes[i] as char);
                i += 1;
            }
        }
        output
    }

    /// Extract APC bodies from a Kitty escape sequence string.
    /// Simulates what the WASM parser does: extract bytes between ESC_ and ESC\.
    fn extract_apc_bodies(sequence: &str) -> Vec<String> {
        let mut bodies = Vec::new();
        let bytes = sequence.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            if bytes[i] == 0x1B && bytes[i + 1] == b'_' {
                let start = i + 2;
                let mut j = start;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1B && bytes[j + 1] == b'\\' {
                        bodies.push(String::from_utf8_lossy(&bytes[start..j]).to_string());
                        i = j + 2;
                        break;
                    }
                    j += 1;
                }
                if j + 1 >= bytes.len() {
                    break;
                }
            } else {
                i += 1;
            }
        }
        bodies
    }
}
