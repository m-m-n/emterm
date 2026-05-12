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
/// Acknowledges that the frontend has consumed `bytes` bytes of PTY output for
/// the given session. Decrements the per-session backpressure counter so the
/// reader thread knows it can forward more data without flooding.
///
/// This is the only signal the Rust side has that the frontend is keeping up,
/// because the Tauri Channel used for PTY data is one-way. When the frontend
/// stalls (e.g. WebKitGTK background tab freezes rAF), unacked bytes accumulate
/// and the reader pauses at the high water mark.
#[tauri::command]
pub fn pty_ack(state: State<'_, PtyManager>, session_id: String, bytes: usize) {
    if let Some(bp) = state.backpressure().get(&session_id) {
        bp.ack(bytes);
    }
}

#[cfg(feature = "gui")]
/// Diagnostic-only: returns the cumulative `channel.send` count and bytes
/// that the reader thread has recorded for `session_id`. Used by E2E specs
/// to verify that the reader stops emitting data while the session is hidden
/// (TS-29 / TS-15). Returns `(-1, 0)` if the session is not registered.
///
/// Frontend code does NOT call this command (FR15 撤去対象). It exists for
/// E2E specs and on-demand manual debugging only.
#[tauri::command]
pub fn pty_get_send_stats(state: State<'_, PtyManager>, session_id: String) -> (i64, u64) {
    if let Some(bp) = state.backpressure().get(&session_id) {
        (bp.sent_count() as i64, bp.sent_bytes())
    } else {
        (-1, 0)
    }
}

#[cfg(feature = "gui")]
/// Notify the backend of the frontend's effective visibility for a session.
///
/// Visible -> hidden: the reader stops forwarding PTY bytes to the frontend
/// channel and instead feeds the bytes to a per-session shadow VT100 parser
/// plus a raw-passthrough scanner (for image / Markdown OSC sequences). It
/// also raises a hidden wake on the backpressure waiter so the reader can
/// re-evaluate without waiting for an ack the frontend will not send.
///
/// Hidden -> visible: the backend builds a 1-message snapshot
/// (`ESC[H ESC[2J` + shadow contents + raw passthrough) and sends it on the
/// reader channel. Subsequent batches go through the normal visible path.
///
/// Unknown `session_id` -> warn + no-op (frontend sometimes notifies before
/// `pty_spawn` resolves on a fresh window).
#[tauri::command]
pub fn pty_set_visibility(state: State<'_, PtyManager>, session_id: String, visible: bool) {
    let Some(vis) = state.visibility().get(&session_id) else {
        log::warn!(
            "[WARN][BACKEND] pty_set_visibility: session {} not found",
            session_id
        );
        return;
    };
    if visible {
        let dispatched = vis.dispatch_resume_snapshot();
        if dispatched {
            log::debug!(
                "[DEBUG][BACKEND] pty_set_visibility: session {} -> visible (snapshot sent)",
                session_id
            );
        }
    } else {
        let was_visible = vis.set_hidden();
        if was_visible {
            // Wake any reader sitting in wait_for_drain so it stops
            // expecting acks and proceeds to the hidden short-circuit.
            if let Some(bp) = state.backpressure().get(&session_id) {
                bp.set_hidden_wake();
            }
        }
    }
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
        .ok_or_else(|| PtyError::SessionNotFound(session_id.clone()).to_string())?;

    // Keep the visibility shadow parser in sync with the PTY size so
    // hidden-mode VT100 processing operates against the same dimensions
    // the foreground frontend sees (FR4).
    if let Some(vis) = state.visibility().get(&session_id) {
        vis.resize(cols, rows);
    }

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

/// Write a log line to mux-client.log file.
/// Uses the same directory as mux-daemon.log and mux-bridge.log.
///
/// Opens the log via `open_mux_log_append`, which applies `O_NOFOLLOW` +
/// `mode(0o600)` on Unix to refuse pre-placed symlink attacks redirecting
/// writes to an attacker-controlled file.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn mux_client_log(line: String) {
    use std::io::Write;

    let log_path = crate::mux::daemon::socket_path()
        .parent()
        .map(|p| p.join("mux-client.log"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/mux-client.log"));

    if let Ok(mut file) = crate::mux::daemon::open_mux_log_append(&log_path) {
        let _ = writeln!(file, "{}", line);
    }
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

#[cfg(feature = "gui")]
/// Decodes a base64-encoded image (any format supported by `image` crate) to RGBA.
///
/// Used by OSC 1337;File (iTerm2 inline image protocol).
/// Returns width, height, and RGBA pixel data as base64.
#[tauri::command]
pub async fn decode_iterm2_image(base64_data: String) -> Result<serde_json::Value, String> {
    use base64::{Engine, engine::general_purpose::STANDARD};

    let raw = STANDARD
        .decode(&base64_data)
        .map_err(|e| format!("Base64 decode error: {}", e))?;

    // Limit input size to 10MB to prevent memory abuse
    if raw.len() > 10 * 1024 * 1024 {
        return Err(format!(
            "Image data too large: {} bytes (max 10MB)",
            raw.len()
        ));
    }

    let img = ::image::load_from_memory(&raw).map_err(|e| format!("Image decode error: {}", e))?;

    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();

    // Dimension check to prevent decompression bombs
    if width > 8192 || height > 8192 {
        return Err(format!(
            "Image too large: {}x{} (max 8192x8192)",
            width, height
        ));
    }

    let rgba_base64 = STANDARD.encode(rgba.as_raw());

    Ok(serde_json::json!({
        "width": width,
        "height": height,
        "rgba_base64": rgba_base64,
    }))
}

/// Starts a streaming download: show save dialog, open file, register handle.
///
/// Returns `{ id, path }` on confirm, or `null` if user cancels.
#[cfg(feature = "gui")]
#[tauri::command]
pub async fn start_download_file(
    app: AppHandle,
    registry: State<'_, std::sync::Arc<crate::download_registry::DownloadRegistry>>,
    filename: String,
) -> Result<Option<serde_json::Value>, String> {
    use tauri_plugin_dialog::DialogExt;

    let safe_filename = crate::commands::download::sanitize_filename(&filename);
    let dialog = app.dialog().file().set_file_name(&safe_filename);
    let path = tokio::task::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| format!("Dialog task failed: {}", e))?;

    match path {
        Some(p) => {
            let file_path = p
                .as_path()
                .ok_or_else(|| format!("Save path is not a local filesystem path: {:?}", p))?
                .to_path_buf();

            let file = std::fs::File::create(&file_path)
                .map_err(|e| format!("Failed to create file: {}", e))?;

            let id = uuid::Uuid::new_v4().to_string();
            let path_str = file_path.to_string_lossy().into_owned();

            registry
                .insert(id.clone(), file, file_path)
                .map_err(|e| e.to_string())?;

            Ok(Some(serde_json::json!({ "id": id, "path": path_str })))
        }
        None => Ok(None), // User cancelled
    }
}

/// Appends a base64-encoded chunk to an open download session.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn append_download_chunk(
    registry: State<'_, std::sync::Arc<crate::download_registry::DownloadRegistry>>,
    id: String,
    data_base64: String,
) -> Result<(), String> {
    use base64::{Engine as _, engine::general_purpose};

    let data = general_purpose::STANDARD
        .decode(&data_base64)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    registry.write(&id, &data)
}

/// Finishes a download session: flush, close, remove from registry.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn finish_download_file(
    registry: State<'_, std::sync::Arc<crate::download_registry::DownloadRegistry>>,
    id: String,
) -> Result<String, String> {
    let path = registry.finish(&id)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Cancels a download session: close handle, delete partial file.
#[cfg(feature = "gui")]
#[tauri::command]
pub fn cancel_download_file(
    registry: State<'_, std::sync::Arc<crate::download_registry::DownloadRegistry>>,
    id: String,
) -> Result<(), String> {
    registry.cancel(&id)
}

/// Returns diagnostic flags from environment variables.
/// Used to toggle rendering/debug behavior without code changes.
///
/// Recognized env vars:
/// - `EMTERM_FORCE_FULL_RENDER=1`: Bypass differential rendering
#[tauri::command]
pub fn get_diagnostic_flags() -> HashMap<String, bool> {
    let mut flags = HashMap::new();
    flags.insert(
        "forceFullRender".to_string(),
        std::env::var("EMTERM_FORCE_FULL_RENDER").map_or(false, |v| v == "1"),
    );
    flags
}
