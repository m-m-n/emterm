//! Kitty Graphics Protocol handler.
//!
//! Implements the Kitty terminal graphics protocol for inline image display.
//!
//! # Supported Actions
//!
//! - `a=t` (Transmit): Store image data for later display
//! - `a=T` (Transmit and Display): Store and immediately display image
//! - `a=p` (Put): Display a previously transmitted image
//! - `a=d` (Delete): Remove images from display
//! - `a=q` (Query): Check protocol support
//!
//! # Error Codes
//!
//! - `EINVAL`: Invalid parameters
//! - `ENOENT`: Image/placement not found
//! - `ENOSPC`: Storage quota exceeded
//! - `EFAILED`: General failure

use std::collections::HashMap;

use serde::Serialize;

use crate::ansi::apc::{
    KittyAction, KittyCommand, KittyCompression, KittyDeleteTarget, KittyFormat,
};

use super::animation::{AnimationFrame, AnimationManager, AnimationState, CompositionMode};
use super::{DecodedImage, ImageDelete, ImageEvent, ImagePlacement, decoder};

/// Kitty Graphics Protocol version string.
const KITTY_GRAPHICS_VERSION: &str = "OK";

/// Kitty error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum KittyErrorCode {
    /// Invalid parameters
    EINVAL,
    /// Image/placement not found
    ENOENT,
    /// Storage quota exceeded
    ENOSPC,
    /// General failure
    EFAILED,
}

impl std::fmt::Display for KittyErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KittyErrorCode::EINVAL => write!(f, "EINVAL"),
            KittyErrorCode::ENOENT => write!(f, "ENOENT"),
            KittyErrorCode::ENOSPC => write!(f, "ENOSPC"),
            KittyErrorCode::EFAILED => write!(f, "EFAILED"),
        }
    }
}

/// Kitty protocol response.
#[derive(Debug, Clone, Serialize)]
pub struct KittyResponse {
    /// Image ID (if applicable).
    pub image_id: Option<u32>,
    /// Placement ID (if applicable).
    pub placement_id: Option<u32>,
    /// Success flag.
    pub ok: bool,
    /// Error code (if not ok).
    pub error_code: Option<KittyErrorCode>,
    /// Error message (if not ok).
    pub error_message: Option<String>,
}

impl KittyResponse {
    /// Create a success response.
    pub fn ok(image_id: Option<u32>, placement_id: Option<u32>) -> Self {
        Self {
            image_id,
            placement_id,
            ok: true,
            error_code: None,
            error_message: None,
        }
    }

    /// Create an error response.
    pub fn error(image_id: Option<u32>, code: KittyErrorCode, message: impl Into<String>) -> Self {
        Self {
            image_id,
            placement_id: None,
            ok: false,
            error_code: Some(code),
            error_message: Some(message.into()),
        }
    }

    /// Generate the escape sequence for this response.
    ///
    /// Format:
    /// - Success: `ESC _ G i=<id> [,p=<placement_id>] ; OK ESC \`
    /// - Error: `ESC _ G i=<id> ; ERROR:<code> ESC \`
    pub fn to_escape_sequence(&self) -> String {
        let mut parts = Vec::new();

        if let Some(id) = self.image_id {
            parts.push(format!("i={}", id));
        }

        if self.ok {
            if let Some(pid) = self.placement_id {
                parts.push(format!("p={}", pid));
            }
            format!("\x1b_G{};{}\x1b\\", parts.join(","), KITTY_GRAPHICS_VERSION)
        } else {
            let code = self
                .error_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "EFAILED".to_string());
            format!("\x1b_G{};ERROR:{}\x1b\\", parts.join(","), code)
        }
    }

    /// Check if response should be suppressed based on quiet mode.
    ///
    /// Per Kitty Graphics Protocol spec:
    /// - q=1: suppress OK responses only (errors still sent)
    /// - q=2: suppress ALL responses (both OK and errors)
    pub fn should_suppress(&self, quiet: Option<u8>) -> bool {
        match quiet {
            Some(1) => self.ok, // Suppress OK responses only
            Some(2) => true,    // Suppress ALL responses
            _ => false,
        }
    }
}

/// Stored image data during chunked transfer.
#[derive(Debug, Clone)]
struct ImageTransfer {
    /// Accumulated base64 data.
    data: String,
    /// Image format.
    format: Option<KittyFormat>,
    /// Compression type.
    compression: Option<KittyCompression>,
    /// Width in pixels.
    width: Option<u32>,
    /// Height in pixels.
    height: Option<u32>,
    /// Quiet mode from first chunk (preserved for final chunk response).
    quiet: Option<u8>,
}

/// Kitty Graphics Protocol handler.
pub struct KittyHandler {
    /// Stored images by ID.
    images: HashMap<u32, DecodedImage>,

    /// In-progress transfers by image ID.
    transfers: HashMap<u32, ImageTransfer>,

    /// Animation manager for handling animation frames.
    animations: AnimationManager,
}

impl Default for KittyHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl KittyHandler {
    /// Create a new handler.
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
            transfers: HashMap::new(),
            animations: AnimationManager::new(),
        }
    }

    /// Process a Kitty graphics command.
    pub fn process(
        &mut self,
        cmd: &KittyCommand,
        cursor_row: u32,
        cursor_col: u32,
        next_image_id: &mut u32,
        next_placement_id: &mut u32,
    ) -> Vec<ImageEvent> {
        match cmd.action {
            KittyAction::Transmit => self.handle_transmit(cmd, next_image_id),
            KittyAction::TransmitAndDisplay => self.handle_transmit_and_display(
                cmd,
                cursor_row,
                cursor_col,
                next_image_id,
                next_placement_id,
            ),
            KittyAction::Put => self.handle_put(cmd, cursor_row, cursor_col, next_placement_id),
            KittyAction::Delete => self.handle_delete(cmd),
            KittyAction::Query => {
                // Query returns both the QueryResponse event and a protocol response
                let response = KittyResponse::ok(cmd.image_id, None);
                let mut events = vec![ImageEvent::QueryResponse { supported: true }];

                if !response.should_suppress(cmd.quiet) {
                    events.push(ImageEvent::Response {
                        data: response.to_escape_sequence(),
                    });
                }

                events
            }
            // Animation actions
            KittyAction::Frame => self.handle_frame(cmd, next_image_id),
            KittyAction::Animate => self.handle_animate(cmd),
            KittyAction::Compose => self.handle_compose(cmd),
        }
    }

    /// Handle transmit action (a=t).
    fn handle_transmit(&mut self, cmd: &KittyCommand, next_image_id: &mut u32) -> Vec<ImageEvent> {
        let image_id = cmd.image_id.unwrap_or_else(|| {
            let id = *next_image_id;
            *next_image_id += 1;
            id
        });

        if cmd.more {
            // Chunked transfer - accumulate data
            let transfer = self
                .transfers
                .entry(image_id)
                .or_insert_with(|| ImageTransfer {
                    data: String::new(),
                    format: cmd.format,
                    compression: cmd.compression,
                    width: cmd.width,
                    height: cmd.height,
                    quiet: cmd.quiet,
                });
            transfer.data.push_str(&cmd.payload);
            vec![]
        } else {
            // Final chunk or single transfer
            let (data, format, compression, width, height, quiet) =
                if let Some(mut transfer) = self.transfers.remove(&image_id) {
                    transfer.data.push_str(&cmd.payload);
                    (
                        transfer.data,
                        transfer.format.or(cmd.format),
                        transfer.compression.or(cmd.compression),
                        transfer.width.or(cmd.width),
                        transfer.height.or(cmd.height),
                        transfer.quiet.or(cmd.quiet),
                    )
                } else {
                    (
                        cmd.payload.clone(),
                        cmd.format,
                        cmd.compression,
                        cmd.width,
                        cmd.height,
                        cmd.quiet,
                    )
                };

            // Decode and store image
            match self.decode_image(image_id, &data, format, compression, width, height) {
                Ok(image) => {
                    self.images.insert(image_id, image.clone());
                    let mut events = vec![ImageEvent::ImageReady { image }];

                    // Add response if not suppressed (use quiet from first chunk)
                    let response = KittyResponse::ok(Some(image_id), None);
                    if !response.should_suppress(quiet) {
                        events.push(ImageEvent::Response {
                            data: response.to_escape_sequence(),
                        });
                    }

                    events
                }
                Err(e) => {
                    log::warn!("Kitty image decode failed: {}", e);
                    let response = KittyResponse::error(Some(image_id), KittyErrorCode::EINVAL, e);
                    if response.should_suppress(quiet) {
                        vec![]
                    } else {
                        vec![ImageEvent::Response {
                            data: response.to_escape_sequence(),
                        }]
                    }
                }
            }
        }
    }

    /// Handle transmit and display action (a=T).
    fn handle_transmit_and_display(
        &mut self,
        cmd: &KittyCommand,
        cursor_row: u32,
        cursor_col: u32,
        next_image_id: &mut u32,
        next_placement_id: &mut u32,
    ) -> Vec<ImageEvent> {
        let image_id = cmd.image_id.unwrap_or_else(|| {
            let id = *next_image_id;
            *next_image_id += 1;
            id
        });

        let placement_id = cmd.placement_id.unwrap_or_else(|| {
            let id = *next_placement_id;
            *next_placement_id += 1;
            id
        });

        if cmd.more {
            // Chunked transfer - accumulate data
            let transfer = self
                .transfers
                .entry(image_id)
                .or_insert_with(|| ImageTransfer {
                    data: String::new(),
                    format: cmd.format,
                    compression: cmd.compression,
                    width: cmd.width,
                    height: cmd.height,
                    quiet: cmd.quiet,
                });
            transfer.data.push_str(&cmd.payload);
            vec![]
        } else {
            // Final chunk or single transfer
            let (data, format, compression, width, height, quiet) =
                if let Some(mut transfer) = self.transfers.remove(&image_id) {
                    transfer.data.push_str(&cmd.payload);
                    (
                        transfer.data,
                        transfer.format.or(cmd.format),
                        transfer.compression.or(cmd.compression),
                        transfer.width.or(cmd.width),
                        transfer.height.or(cmd.height),
                        transfer.quiet.or(cmd.quiet),
                    )
                } else {
                    (
                        cmd.payload.clone(),
                        cmd.format,
                        cmd.compression,
                        cmd.width,
                        cmd.height,
                        cmd.quiet,
                    )
                };

            // Decode and store image
            match self.decode_image(image_id, &data, format, compression, width, height) {
                Ok(image) => {
                    self.images.insert(image_id, image.clone());

                    let placement = ImagePlacement {
                        image_id,
                        placement_id,
                        row: cursor_row,
                        col: cursor_col,
                        columns: cmd.columns.unwrap_or(0),
                        rows: cmd.rows.unwrap_or(0),
                        x_offset: cmd.x_offset.unwrap_or(0),
                        y_offset: cmd.y_offset.unwrap_or(0),
                        z_index: cmd.z_index.unwrap_or(-1),
                    };

                    let mut events = vec![
                        ImageEvent::ImageReady { image },
                        ImageEvent::Place { placement },
                    ];

                    // Add response if not suppressed (use quiet from first chunk)
                    let response = KittyResponse::ok(Some(image_id), Some(placement_id));
                    if !response.should_suppress(quiet) {
                        events.push(ImageEvent::Response {
                            data: response.to_escape_sequence(),
                        });
                    }

                    events
                }
                Err(e) => {
                    log::warn!("Kitty image decode failed: {}", e);
                    let response = KittyResponse::error(Some(image_id), KittyErrorCode::EINVAL, e);
                    if response.should_suppress(quiet) {
                        vec![]
                    } else {
                        vec![ImageEvent::Response {
                            data: response.to_escape_sequence(),
                        }]
                    }
                }
            }
        }
    }

    /// Handle put action (a=p).
    fn handle_put(
        &mut self,
        cmd: &KittyCommand,
        cursor_row: u32,
        cursor_col: u32,
        next_placement_id: &mut u32,
    ) -> Vec<ImageEvent> {
        let Some(image_id) = cmd.image_id else {
            log::warn!("Kitty put without image ID");
            let response = KittyResponse::error(None, KittyErrorCode::EINVAL, "Missing image ID");
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        };

        if !self.images.contains_key(&image_id) {
            log::warn!("Kitty put for unknown image ID: {}", image_id);
            let response =
                KittyResponse::error(Some(image_id), KittyErrorCode::ENOENT, "Image not found");
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        }

        let placement_id = cmd.placement_id.unwrap_or_else(|| {
            let id = *next_placement_id;
            *next_placement_id += 1;
            id
        });

        let placement = ImagePlacement {
            image_id,
            placement_id,
            row: cursor_row,
            col: cursor_col,
            columns: cmd.columns.unwrap_or(0),
            rows: cmd.rows.unwrap_or(0),
            x_offset: cmd.x_offset.unwrap_or(0),
            y_offset: cmd.y_offset.unwrap_or(0),
            z_index: cmd.z_index.unwrap_or(-1),
        };

        let mut events = vec![ImageEvent::Place { placement }];

        // Add response if not suppressed
        let response = KittyResponse::ok(Some(image_id), Some(placement_id));
        if !response.should_suppress(cmd.quiet) {
            events.push(ImageEvent::Response {
                data: response.to_escape_sequence(),
            });
        }

        events
    }

    /// Handle delete action (a=d).
    fn handle_delete(&mut self, cmd: &KittyCommand) -> Vec<ImageEvent> {
        let target = match cmd.delete_target {
            Some(KittyDeleteTarget::All) => {
                self.images.clear();
                ImageDelete::All
            }
            Some(KittyDeleteTarget::AllIncludingHidden) => {
                self.images.clear();
                self.transfers.clear();
                ImageDelete::AllIncludingHidden
            }
            Some(KittyDeleteTarget::ById) => {
                if let Some(id) = cmd.image_id {
                    self.images.remove(&id);
                    self.transfers.remove(&id);
                    ImageDelete::ById(id)
                } else {
                    return vec![];
                }
            }
            Some(KittyDeleteTarget::ByPlacement) => {
                let image_id = cmd.image_id.unwrap_or(0);
                let placement_id = cmd.placement_id.unwrap_or(0);
                ImageDelete::ByPlacement {
                    image_id,
                    placement_id,
                }
            }
            Some(KittyDeleteTarget::AtCursor | KittyDeleteTarget::AtCursorByColumns) => {
                // Frontend handles cursor-based deletion
                ImageDelete::AtCursor { row: 0, col: 0 }
            }
            Some(KittyDeleteTarget::ByZIndex) => {
                let z = cmd.z_index.unwrap_or(0);
                ImageDelete::ByZIndex(z)
            }
            Some(KittyDeleteTarget::AtPosition | KittyDeleteTarget::AtCell) => {
                // Not commonly used
                return vec![];
            }
            None => {
                // Default: delete all
                self.images.clear();
                ImageDelete::All
            }
        };

        vec![ImageEvent::Delete { target }]
    }

    /// Decode image data based on format.
    fn decode_image(
        &self,
        id: u32,
        base64_data: &str,
        format: Option<KittyFormat>,
        compression: Option<KittyCompression>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<DecodedImage, String> {
        // Decode base64
        let raw_data = decoder::decode_base64(base64_data)?;

        // Decompress if needed
        let decompressed = match compression {
            Some(KittyCompression::Zlib) => decoder::decompress_zlib(&raw_data)?,
            None => raw_data,
        };

        let (w, h, rgba_data) = match format {
            Some(KittyFormat::Png) | None => {
                // Default to PNG if format not specified
                decoder::decode_png(&decompressed)?
            }
            Some(KittyFormat::Rgb) => {
                let w = width.ok_or("Width required for RGB format")?;
                let h = height.ok_or("Height required for RGB format")?;
                let rgba = decoder::decode_rgb(&decompressed, w, h)?;
                (w, h, rgba)
            }
            Some(KittyFormat::Rgba) => {
                let w = width.ok_or("Width required for RGBA format")?;
                let h = height.ok_or("Height required for RGBA format")?;
                decoder::validate_rgba(&decompressed, w, h)?;
                (w, h, decompressed)
            }
        };

        Ok(DecodedImage {
            id,
            width: w,
            height: h,
            rgba_base64: decoder::encode_base64(&rgba_data),
            rgba_data,
        })
    }

    /// Reset handler state.
    pub fn reset(&mut self) {
        self.images.clear();
        self.transfers.clear();
        self.animations.clear();
    }

    // =========================================================================
    // Animation Handlers
    // =========================================================================

    /// Handle frame action (a=f) - send animation frame.
    fn handle_frame(&mut self, cmd: &KittyCommand, next_image_id: &mut u32) -> Vec<ImageEvent> {
        let image_id = cmd.image_id.unwrap_or_else(|| {
            let id = *next_image_id;
            *next_image_id += 1;
            id
        });

        // Frame number defaults to next frame in sequence
        let frame_number = cmd.frame_number.unwrap_or_else(|| {
            if let Some(anim) = self.animations.get(image_id) {
                anim.frame_count() + 1
            } else {
                1
            }
        });

        // Handle chunked transfer for frame data
        if cmd.more {
            let transfer = self
                .transfers
                .entry(image_id)
                .or_insert_with(|| ImageTransfer {
                    data: String::new(),
                    format: cmd.format,
                    compression: cmd.compression,
                    width: cmd.width,
                    height: cmd.height,
                    quiet: cmd.quiet,
                });
            transfer.data.push_str(&cmd.payload);
            return vec![];
        }

        // Final chunk or single transfer
        let (data, format, compression, width, height, quiet) =
            if let Some(mut transfer) = self.transfers.remove(&image_id) {
                transfer.data.push_str(&cmd.payload);
                (
                    transfer.data,
                    transfer.format.or(cmd.format),
                    transfer.compression.or(cmd.compression),
                    transfer.width.or(cmd.width),
                    transfer.height.or(cmd.height),
                    transfer.quiet.or(cmd.quiet),
                )
            } else {
                (
                    cmd.payload.clone(),
                    cmd.format,
                    cmd.compression,
                    cmd.width,
                    cmd.height,
                    cmd.quiet,
                )
            };

        // Decode the frame data
        let (w, h, rgba_data) =
            match self.decode_frame_data(&data, format, compression, width, height) {
                Ok(result) => result,
                Err(e) => {
                    log::warn!("Kitty frame decode failed: {}", e);
                    let response = KittyResponse::error(Some(image_id), KittyErrorCode::EINVAL, e);
                    if response.should_suppress(quiet) {
                        return vec![];
                    }
                    return vec![ImageEvent::Response {
                        data: response.to_escape_sequence(),
                    }];
                }
            };

        // Determine composition mode
        let composition_mode = match cmd.composition_mode {
            Some(1) => CompositionMode::Replace,
            _ => CompositionMode::AlphaBlend,
        };

        // Determine frame delay
        let delay_ms = match cmd.frame_gap {
            Some(z) if z < 0 => 0, // Negative means no gap
            Some(z) => z as u32,
            None => 40, // Default 25 FPS
        };

        // Parse background color if provided
        let background = cmd.background_color.map(|c| {
            [
                ((c >> 24) & 0xFF) as u8,
                ((c >> 16) & 0xFF) as u8,
                ((c >> 8) & 0xFF) as u8,
                (c & 0xFF) as u8,
            ]
        });

        // Create the animation frame
        let mut frame = AnimationFrame::new(frame_number, w, h, rgba_data)
            .with_delay(delay_ms)
            .with_composition_mode(composition_mode)
            .with_no_gap(cmd.frame_gap.is_some_and(|z| z < 0));

        if let Some(bg) = background {
            frame = frame.with_background(bg);
        }

        if let Some(base) = cmd.base_frame {
            frame = frame.with_base_frame(base);
        }

        // Add frame to animation and emit event
        let events = self.animations.add_frame(image_id, frame);

        // Convert AnimationEvents to ImageEvents
        let mut image_events: Vec<ImageEvent> =
            events.into_iter().map(ImageEvent::Animation).collect();

        // Add response if not suppressed (use quiet from first chunk)
        let response = KittyResponse::ok(Some(image_id), None);
        if !response.should_suppress(quiet) {
            image_events.push(ImageEvent::Response {
                data: response.to_escape_sequence(),
            });
        }

        image_events
    }

    /// Handle animate action (a=a) - control animation playback.
    fn handle_animate(&mut self, cmd: &KittyCommand) -> Vec<ImageEvent> {
        let Some(image_id) = cmd.image_id else {
            log::warn!("Kitty animate without image ID");
            let response = KittyResponse::error(None, KittyErrorCode::EINVAL, "Missing image ID");
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        };

        if !self.animations.has_animation(image_id) {
            log::warn!("Kitty animate for unknown animation: {}", image_id);
            let response = KittyResponse::error(
                Some(image_id),
                KittyErrorCode::ENOENT,
                "Animation not found",
            );
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        }

        let mut image_events = Vec::new();

        // Handle animation state change (s=)
        if let Some(state) = cmd.animation_state {
            let new_state = match state {
                1 => AnimationState::Stopped,
                2 => AnimationState::Loading,
                3 => AnimationState::Playing,
                _ => AnimationState::Stopped,
            };

            let events = self.animations.set_state(image_id, new_state);
            image_events.extend(events.into_iter().map(ImageEvent::Animation));
        }

        // Handle loop count (v=)
        if let Some(loops) = cmd.animation_loops {
            self.animations.set_loop_count(image_id, loops);
        }

        // Handle current frame setting (c=)
        if let Some(frame) = cmd.base_frame {
            let events = self.animations.set_current_frame(image_id, frame);
            image_events.extend(events.into_iter().map(ImageEvent::Animation));
        }

        // Add response if not suppressed
        let response = KittyResponse::ok(Some(image_id), None);
        if !response.should_suppress(cmd.quiet) {
            image_events.push(ImageEvent::Response {
                data: response.to_escape_sequence(),
            });
        }

        image_events
    }

    /// Handle compose action (a=c) - compose frames together.
    fn handle_compose(&mut self, cmd: &KittyCommand) -> Vec<ImageEvent> {
        let Some(image_id) = cmd.image_id else {
            log::warn!("Kitty compose without image ID");
            let response = KittyResponse::error(None, KittyErrorCode::EINVAL, "Missing image ID");
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        };

        let Some(src_frame) = cmd.source_frame else {
            log::warn!("Kitty compose without source frame");
            let response = KittyResponse::error(
                Some(image_id),
                KittyErrorCode::EINVAL,
                "Missing source frame",
            );
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        };

        let Some(dst_frame) = cmd.target_frame else {
            log::warn!("Kitty compose without target frame");
            let response = KittyResponse::error(
                Some(image_id),
                KittyErrorCode::EINVAL,
                "Missing target frame",
            );
            if response.should_suppress(cmd.quiet) {
                return vec![];
            }
            return vec![ImageEvent::Response {
                data: response.to_escape_sequence(),
            }];
        };

        match self
            .animations
            .compose_frames(image_id, src_frame, dst_frame)
        {
            Ok(events) => {
                let mut image_events: Vec<ImageEvent> =
                    events.into_iter().map(ImageEvent::Animation).collect();

                let response = KittyResponse::ok(Some(image_id), None);
                if !response.should_suppress(cmd.quiet) {
                    image_events.push(ImageEvent::Response {
                        data: response.to_escape_sequence(),
                    });
                }

                image_events
            }
            Err(e) => {
                log::warn!("Kitty compose failed: {}", e);
                let response = KittyResponse::error(Some(image_id), KittyErrorCode::EFAILED, e);
                if response.should_suppress(cmd.quiet) {
                    return vec![];
                }
                vec![ImageEvent::Response {
                    data: response.to_escape_sequence(),
                }]
            }
        }
    }

    /// Decode frame data (similar to decode_image but returns components).
    fn decode_frame_data(
        &self,
        base64_data: &str,
        format: Option<KittyFormat>,
        compression: Option<KittyCompression>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<(u32, u32, Vec<u8>), String> {
        // Decode base64
        let raw_data = decoder::decode_base64(base64_data)?;

        // Decompress if needed
        let decompressed = match compression {
            Some(KittyCompression::Zlib) => decoder::decompress_zlib(&raw_data)?,
            None => raw_data,
        };

        match format {
            Some(KittyFormat::Png) | None => {
                // Default to PNG if format not specified
                decoder::decode_png(&decompressed)
            }
            Some(KittyFormat::Rgb) => {
                let w = width.ok_or("Width required for RGB format")?;
                let h = height.ok_or("Height required for RGB format")?;
                let rgba = decoder::decode_rgb(&decompressed, w, h)?;
                Ok((w, h, rgba))
            }
            Some(KittyFormat::Rgba) => {
                let w = width.ok_or("Width required for RGBA format")?;
                let h = height.ok_or("Height required for RGBA format")?;
                decoder::validate_rgba(&decompressed, w, h)?;
                Ok((w, h, decompressed))
            }
        }
    }

    /// Get animation manager reference (for external tick calls).
    pub fn animations(&self) -> &AnimationManager {
        &self.animations
    }

    /// Get mutable animation manager reference.
    pub fn animations_mut(&mut self) -> &mut AnimationManager {
        &mut self.animations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::apc::KittyCommand;

    #[test]
    fn test_kitty_handler_creation() {
        let handler = KittyHandler::new();
        assert!(handler.images.is_empty());
        assert!(handler.transfers.is_empty());
    }

    #[test]
    fn test_kitty_query() {
        let mut handler = KittyHandler::new();
        let cmd = KittyCommand {
            action: KittyAction::Query,
            ..Default::default()
        };

        let mut next_id = 1;
        let mut next_placement = 1;
        let events = handler.process(&cmd, 0, 0, &mut next_id, &mut next_placement);

        // Query returns QueryResponse + Response events
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            ImageEvent::QueryResponse { supported: true }
        ));
        assert!(matches!(events[1], ImageEvent::Response { .. }));
    }

    #[test]
    fn test_kitty_delete_all() {
        let mut handler = KittyHandler::new();
        let cmd = KittyCommand {
            action: KittyAction::Delete,
            delete_target: Some(KittyDeleteTarget::All),
            ..Default::default()
        };

        let mut next_id = 1;
        let mut next_placement = 1;
        let events = handler.process(&cmd, 0, 0, &mut next_id, &mut next_placement);

        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ImageEvent::Delete {
                target: ImageDelete::All
            }
        ));
    }

    #[test]
    fn test_kitty_put_without_image() {
        let mut handler = KittyHandler::new();
        let cmd = KittyCommand {
            action: KittyAction::Put,
            image_id: Some(999),
            ..Default::default()
        };

        let mut next_id = 1;
        let mut next_placement = 1;
        let events = handler.process(&cmd, 5, 10, &mut next_id, &mut next_placement);

        // Returns error response (image doesn't exist)
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ImageEvent::Response { .. }));
    }

    #[test]
    fn test_kitty_chunked_transfer() {
        let mut handler = KittyHandler::new();

        // First chunk
        let cmd1 = KittyCommand {
            action: KittyAction::Transmit,
            image_id: Some(1),
            format: Some(KittyFormat::Png),
            more: true,
            payload: "iVBORw".to_string(),
            ..Default::default()
        };

        let mut next_id = 1;
        let mut next_placement = 1;
        let events1 = handler.process(&cmd1, 0, 0, &mut next_id, &mut next_placement);
        assert!(events1.is_empty()); // No event until complete

        // Transfer should be stored
        assert!(handler.transfers.contains_key(&1));

        // Final chunk (invalid PNG data, will fail decode)
        let cmd2 = KittyCommand {
            action: KittyAction::Transmit,
            image_id: Some(1),
            more: false,
            payload: "0KGgo=".to_string(),
            ..Default::default()
        };

        let events2 = handler.process(&cmd2, 0, 0, &mut next_id, &mut next_placement);
        // Decode will fail, returns error response
        assert_eq!(events2.len(), 1);
        assert!(matches!(events2[0], ImageEvent::Response { .. }));
        // Transfer should be cleared
        assert!(!handler.transfers.contains_key(&1));
    }

    #[test]
    fn test_kitty_reset() {
        let mut handler = KittyHandler::new();

        // Add some state
        handler.transfers.insert(
            1,
            ImageTransfer {
                data: "test".to_string(),
                format: None,
                compression: None,
                width: None,
                height: None,
                quiet: None,
            },
        );

        handler.reset();

        assert!(handler.images.is_empty());
        assert!(handler.transfers.is_empty());
    }

    // =========================================================================
    // Response Tests
    // =========================================================================

    #[test]
    fn test_kitty_response_ok() {
        let response = KittyResponse::ok(Some(42), Some(5));
        assert!(response.ok);
        assert_eq!(response.image_id, Some(42));
        assert_eq!(response.placement_id, Some(5));

        let seq = response.to_escape_sequence();
        assert!(seq.contains("i=42"));
        assert!(seq.contains("p=5"));
        assert!(seq.contains("OK"));
    }

    #[test]
    fn test_kitty_response_error() {
        let response = KittyResponse::error(Some(42), KittyErrorCode::ENOENT, "Image not found");
        assert!(!response.ok);
        assert_eq!(response.error_code, Some(KittyErrorCode::ENOENT));

        let seq = response.to_escape_sequence();
        assert!(seq.contains("i=42"));
        assert!(seq.contains("ERROR:ENOENT"));
    }

    #[test]
    fn test_kitty_response_suppression() {
        let ok_response = KittyResponse::ok(Some(1), None);
        let error_response = KittyResponse::error(Some(1), KittyErrorCode::EINVAL, "test");

        // q=1 suppresses OK responses only
        assert!(ok_response.should_suppress(Some(1)));
        assert!(!error_response.should_suppress(Some(1)));

        // q=2 suppresses ALL responses (both OK and ERROR)
        assert!(ok_response.should_suppress(Some(2)));
        assert!(error_response.should_suppress(Some(2)));

        // No quiet mode — nothing suppressed
        assert!(!ok_response.should_suppress(None));
        assert!(!error_response.should_suppress(None));
    }

    #[test]
    fn test_kitty_error_codes_display() {
        assert_eq!(KittyErrorCode::EINVAL.to_string(), "EINVAL");
        assert_eq!(KittyErrorCode::ENOENT.to_string(), "ENOENT");
        assert_eq!(KittyErrorCode::ENOSPC.to_string(), "ENOSPC");
        assert_eq!(KittyErrorCode::EFAILED.to_string(), "EFAILED");
    }
}
