use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::image;

/// Per-session image processor state.
///
/// Maintains `ImageProcessor` instances per PTY session to preserve
/// state across multiple `process_image_data` calls (e.g., chunked
/// Kitty transfers that require accumulating data across APC sequences).
#[derive(Default)]
pub struct ImageProcessorState {
    pub(crate) processors: Mutex<HashMap<String, image::ImageProcessor>>,
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
pub const LARGE_IMAGE_DATA_THRESHOLD: usize = 2_000_000;

/// Temporary storage for image data too large for Tauri events.
///
/// When `rgba_base64` exceeds [`LARGE_IMAGE_DATA_THRESHOLD`], it is moved
/// here and the event payload carries an empty string. The frontend detects
/// the empty field and calls `fetch_image_data` to retrieve the data via
/// a regular Tauri command (invoke), which handles large responses reliably.
#[derive(Default)]
pub struct LargeImageDataStore {
    pub(crate) data: Mutex<HashMap<(String, u32), String>>,
}

impl LargeImageDataStore {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
    }
}
