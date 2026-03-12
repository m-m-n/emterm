#[cfg(feature = "gui")]
use {
    crate::payloads::*,
    crate::pty::{PtyError, PtyManager},
    crate::reader::spawn_reader_thread,
    crate::state::{ImageProcessorState, LARGE_IMAGE_DATA_THRESHOLD, LargeImageDataStore},
    crate::{ansi, image, logging},
    std::collections::HashMap,
    tauri::ipc::{Channel, InvokeResponseBody},
    tauri::{AppHandle, Emitter, State},
};

#[cfg(feature = "gui")]
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
#[allow(clippy::too_many_arguments)]
pub async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyManager>,
    channel: Channel<InvokeResponseBody>,
    shell: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
    env_vars: Option<HashMap<String, String>>,
    working_directory: Option<String>,
) -> Result<SpawnResult, String> {
    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);

    // Use atomic method to get session_id and count in one lock (NFR2 compliance)
    let result = state
        .create_session_atomic(shell, args, cols, rows, env_vars, working_directory)
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

#[cfg(feature = "gui")]
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
///
/// Maximum allowed write size per call (1 MB).
const PTY_WRITE_MAX_SIZE: usize = 1024 * 1024;

#[cfg(feature = "gui")]
#[tauri::command]
pub fn pty_write(
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

#[cfg(feature = "gui")]
/// Resizes a PTY session.
///
/// # Arguments
///
/// * `session_id` - The target session ID
/// * `cols` - New number of columns
/// * `rows` - New number of rows
#[tauri::command]
pub async fn pty_resize(
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

#[cfg(feature = "gui")]
/// Kills a PTY session.
///
/// # Arguments
///
/// * `session_id` - The session ID to kill
#[tauri::command]
pub async fn pty_kill(
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

#[cfg(feature = "gui")]
/// Console log command - prints message to stdout with [LOG][FRONTEND] prefix.
#[tauri::command]
pub fn console_log(message: String) {
    println!("{}", logging::format_frontend_log("log", &message));
}

#[cfg(feature = "gui")]
/// Console warn command - prints message to stderr with [WARN][FRONTEND] prefix.
#[tauri::command]
pub fn console_warn(message: String) {
    eprintln!("{}", logging::format_frontend_log("warn", &message));
}

#[cfg(feature = "gui")]
/// Console error command - prints message to stderr with [ERROR][FRONTEND] prefix.
#[tauri::command]
pub fn console_error(message: String) {
    eprintln!("{}", logging::format_frontend_log("error", &message));
}

#[cfg(feature = "gui")]
/// Console info command - prints message to stdout with [INFO][FRONTEND] prefix.
#[tauri::command]
pub fn console_info(message: String) {
    println!("{}", logging::format_frontend_log("info", &message));
}

#[cfg(feature = "gui")]
/// Console debug command - prints message to stdout with [DEBUG][FRONTEND] prefix.
#[tauri::command]
pub fn console_debug(message: String) {
    println!("{}", logging::format_frontend_log("debug", &message));
}

/// Read the contents of the log file.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_log_contents() -> Result<String, String> {
    logging::read_log_file()
}

/// Clear the log file.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn clear_log() -> Result<(), String> {
    logging::clear_log_file()
}

/// Read the last N lines from the log file.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_log_tail(lines: usize) -> Result<String, String> {
    logging::read_log_tail(lines)
}

/// Get the log file path.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn get_log_path() -> Option<String> {
    logging::get_log_file_path()
}

/// Set the log recording enabled flag at runtime.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn set_log_recording(enabled: bool) {
    logging::set_log_recording_enabled(enabled);
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
#[cfg(feature = "gui")]
#[tauri::command]
pub fn set_language(language: String) -> Result<(), String> {
    const SUPPORTED: &[&str] = &["en", "ja"];
    if SUPPORTED.contains(&language.as_str()) {
        rust_i18n::set_locale(&language);
        Ok(())
    } else {
        Err(format!("Unsupported language: {}", language))
    }
}

#[cfg(feature = "gui")]
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
#[allow(clippy::too_many_arguments)]
pub async fn process_image_data(
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

#[cfg(feature = "gui")]
/// Fetches large image data that was omitted from the `image_event` payload.
///
/// When an image's `rgba_base64` exceeds [`LARGE_IMAGE_DATA_THRESHOLD`],
/// `process_image_data` stores it here and sends an empty string in the event.
/// The frontend calls this command to retrieve the actual pixel data.
///
/// This is a one-shot retrieval: the data is removed from the store after fetch.
#[tauri::command]
pub async fn fetch_image_data(
    large_image_store: State<'_, LargeImageDataStore>,
    session_id: String,
    image_id: u32,
) -> Result<String, String> {
    let mut store = large_image_store.data.lock().await;
    store
        .remove(&(session_id, image_id))
        .ok_or_else(|| format!("No deferred image data for id={}", image_id))
}

#[cfg(feature = "gui")]
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
pub async fn process_kitty_batch(
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

#[cfg(feature = "gui")]
/// Returns the number of active PTY sessions.
///
/// This command exposes the existing `PtyManager::session_count()` method
/// to the frontend, enabling tab-aware window close logic.
#[tauri::command]
pub async fn session_count(state: State<'_, PtyManager>) -> Result<usize, String> {
    Ok(state.session_count().await)
}

#[cfg(feature = "gui")]
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
pub async fn tab_close_graceful(
    state: State<'_, PtyManager>,
    session_id: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let config = match timeout_ms {
        Some(ms) => crate::pty::graceful_shutdown::ShutdownConfig::from_total_ms(ms),
        None => crate::pty::graceful_shutdown::ShutdownConfig::default(),
    };
    crate::pty::graceful_shutdown::shutdown_with_config(&state, &session_id, config).await
}

/// Shows a save dialog and writes binary data to the user-selected path.
/// The file path is never received from the frontend — the backend opens the
/// native dialog itself, eliminating arbitrary-path-write via IPC.
///
/// Receives file data as a base64 string to avoid the overhead of JSON number
/// array serialization (which would inflate a 10MB file to ~30MB of JSON).
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn write_download_file(
    app: AppHandle,
    filename: String,
    data_base64: String,
) -> Result<Option<String>, String> {
    use base64::{Engine as _, engine::general_purpose};
    use tauri_plugin_dialog::DialogExt;

    let data = general_purpose::STANDARD
        .decode(&data_base64)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    let dialog = app.dialog().file().set_file_name(&filename);
    let path = tokio::task::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| format!("Dialog task failed: {}", e))?;

    match path {
        Some(p) => {
            let file_path = p
                .as_path()
                .ok_or_else(|| format!("Save path is not a local filesystem path: {:?}", p))?;
            tokio::fs::write(&file_path, &data)
                .await
                .map_err(|e| format!("Failed to write file: {}", e))?;
            Ok(Some(file_path.to_string_lossy().into_owned()))
        }
        None => Ok(None), // User cancelled
    }
}
