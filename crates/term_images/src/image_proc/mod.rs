//! Image processing module for terminal graphics.
//!
//! This module handles image display via Kitty Graphics Protocol and SIXEL.
//!
//! # Architecture
//!
//! ```text
//! ANSI Parser → Image Processor → IPC Event → Frontend Image Layer
//! ```
//!
//! # Supported Protocols
//!
//! - **Kitty Graphics Protocol**: PNG, RGB, RGBA transmission
//! - **SIXEL**: Legacy graphics format with color palettes
//!
//! # Example
//!
//! ```ignore
//! use app_lib::image::{ImageProcessor, ImagePlacement};
//!
//! let mut processor = ImageProcessor::new();
//!
//! // Process Kitty graphics command
//! use app_lib::ansi::apc::KittyCommand;
//! let cmd = KittyCommand::default();
//! let events = processor.process_kitty_command(&cmd);
//! for event in events {
//!     // Send to frontend
//! }
//! ```

pub mod animation;
pub mod decoder;
pub mod kitty;
pub mod limiter;
pub mod placement;
pub mod sixel;
pub mod store;

use serde::Serialize;

/// Represents a decoded image ready for display.
#[derive(Debug, Clone, Serialize)]
pub struct DecodedImage {
    /// Unique image ID.
    pub id: u32,

    /// Image width in pixels.
    pub width: u32,

    /// Image height in pixels.
    pub height: u32,

    /// RGBA pixel data (4 bytes per pixel).
    #[serde(skip)]
    pub rgba_data: Vec<u8>,

    /// Base64-encoded RGBA data for IPC transfer.
    pub rgba_base64: String,
}

/// Represents where and how an image should be displayed.
#[derive(Debug, Clone, Serialize)]
pub struct ImagePlacement {
    /// Image ID to display.
    pub image_id: u32,

    /// Placement ID (for multiple placements of same image).
    pub placement_id: u32,

    /// Display position: row (0-based).
    pub row: u32,

    /// Display position: column (0-based).
    pub col: u32,

    /// Display width in terminal columns (0 = auto).
    pub columns: u32,

    /// Display height in terminal rows (0 = auto).
    pub rows: u32,

    /// X offset within cell in pixels.
    pub x_offset: u32,

    /// Y offset within cell in pixels.
    pub y_offset: u32,

    /// Z-index for layering (negative = behind text).
    pub z_index: i32,
}

impl Default for ImagePlacement {
    fn default() -> Self {
        Self {
            image_id: 0,
            placement_id: 0,
            row: 0,
            col: 0,
            columns: 0,
            rows: 0,
            x_offset: 0,
            y_offset: 0,
            z_index: -1, // Behind text by default
        }
    }
}

/// Image deletion specification.
#[derive(Debug, Clone, Serialize)]
pub enum ImageDelete {
    /// Delete all visible images.
    All,
    /// Delete all images including hidden.
    AllIncludingHidden,
    /// Delete by image ID.
    ById(u32),
    /// Delete by placement ID.
    ByPlacement { image_id: u32, placement_id: u32 },
    /// Delete at cursor position.
    AtCursor { row: u32, col: u32 },
    /// Delete by z-index.
    ByZIndex(i32),
}

/// IPC event for image display.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ImageEvent {
    /// New image data is available.
    ImageReady { image: DecodedImage },

    /// Display image at position.
    Place { placement: ImagePlacement },

    /// Delete images.
    Delete { target: ImageDelete },

    /// Query response (for Kitty protocol).
    QueryResponse { supported: bool },

    /// Protocol response to send back to PTY.
    Response { data: String },

    /// Animation event (frame ready, state change, etc.).
    Animation(animation::AnimationEvent),
}

/// Main image processor that handles Kitty and SIXEL commands.
pub struct ImageProcessor {
    /// Kitty protocol handler.
    kitty_handler: kitty::KittyHandler,

    /// SIXEL handler.
    sixel_handler: sixel::SixelHandler,

    /// Next image ID to assign.
    next_image_id: u32,

    /// Next placement ID to assign.
    next_placement_id: u32,
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProcessor {
    /// Create a new image processor.
    pub fn new() -> Self {
        Self {
            kitty_handler: kitty::KittyHandler::new(),
            sixel_handler: sixel::SixelHandler::new(),
            next_image_id: 1,
            next_placement_id: 1,
        }
    }

    /// Process a Kitty graphics command.
    ///
    /// Returns image events to emit to frontend.
    pub fn process_kitty_command(
        &mut self,
        cmd: &crate::ansi::apc::KittyCommand,
        cursor_row: u32,
        cursor_col: u32,
    ) -> Vec<ImageEvent> {
        self.kitty_handler.process(
            cmd,
            cursor_row,
            cursor_col,
            &mut self.next_image_id,
            &mut self.next_placement_id,
        )
    }

    /// Process a SIXEL sequence.
    ///
    /// Returns image events to emit to frontend.
    pub fn process_sixel(
        &mut self,
        sixel: &crate::ansi::dcs::SixelData,
        cursor_row: u32,
        cursor_col: u32,
    ) -> Vec<ImageEvent> {
        self.sixel_handler.process(
            sixel,
            cursor_row,
            cursor_col,
            &mut self.next_image_id,
            &mut self.next_placement_id,
        )
    }

    /// Reset the processor state.
    pub fn reset(&mut self) {
        self.kitty_handler.reset();
        self.sixel_handler.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_processor_creation() {
        let processor = ImageProcessor::new();
        assert_eq!(processor.next_image_id, 1);
        assert_eq!(processor.next_placement_id, 1);
    }

    #[test]
    fn test_decoded_image_serialization() {
        let image = DecodedImage {
            id: 1,
            width: 10,
            height: 10,
            rgba_data: vec![0; 400],
            rgba_base64: "AAAA".to_string(),
        };

        let json = serde_json::to_string(&image).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"width\":10"));
        // rgba_data is skipped
        assert!(!json.contains("rgba_data"));
    }

    #[test]
    fn test_image_placement_default() {
        let placement = ImagePlacement::default();
        assert_eq!(placement.z_index, -1);
        assert_eq!(placement.columns, 0);
    }
}
