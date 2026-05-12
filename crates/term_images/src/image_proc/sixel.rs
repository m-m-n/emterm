//! SIXEL graphics handler.
//!
//! Implements SIXEL graphics decoding and display.
//!
//! # DECSDM Mode
//!
//! The SIXEL Display Mode (DECSDM) controls how SIXEL graphics interact with
//! the terminal display:
//!
//! - **Enabled (CSI ? 80 h)**: SIXEL graphics render in the full screen area
//! - **Disabled (CSI ? 80 l)**: SIXEL graphics render only within scroll region
//!
//! Default is disabled (scroll region mode).

use crate::ansi::dcs::{self, SixelData};

use super::{DecodedImage, ImageEvent, ImagePlacement, decoder};

/// SIXEL handler.
pub struct SixelHandler {
    /// SIXEL Display Mode (DECSDM).
    ///
    /// When true (enabled via CSI ? 80 h), SIXEL graphics render in full screen.
    /// When false (disabled via CSI ? 80 l), SIXEL graphics respect scroll region.
    decsdm_enabled: bool,
}

impl Default for SixelHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SixelHandler {
    /// Create a new handler.
    pub fn new() -> Self {
        Self {
            decsdm_enabled: false, // Default: scroll region mode
        }
    }

    /// Enable or disable SIXEL Display Mode (DECSDM).
    ///
    /// This is called when the terminal receives CSI ? 80 h (enable) or CSI ? 80 l (disable).
    pub fn set_decsdm_enabled(&mut self, enabled: bool) {
        self.decsdm_enabled = enabled;
        log::debug!(
            "SIXEL DECSDM mode: {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// Check if SIXEL Display Mode (DECSDM) is enabled.
    pub fn is_decsdm_enabled(&self) -> bool {
        self.decsdm_enabled
    }

    /// Process a SIXEL sequence.
    pub fn process(
        &mut self,
        sixel: &SixelData,
        cursor_row: u32,
        cursor_col: u32,
        next_image_id: &mut u32,
        next_placement_id: &mut u32,
    ) -> Vec<ImageEvent> {
        // Decode SIXEL to RGBA
        let (width, height, rgba_data) = dcs::decode_sixel_to_rgba(sixel);

        if width == 0 || height == 0 {
            log::warn!("SIXEL decoded to empty image");
            return vec![];
        }

        let image_id = *next_image_id;
        *next_image_id += 1;

        let placement_id = *next_placement_id;
        *next_placement_id += 1;

        let image = DecodedImage {
            id: image_id,
            width,
            height,
            rgba_base64: decoder::encode_base64(&rgba_data),
            rgba_data,
        };

        let placement = ImagePlacement {
            image_id,
            placement_id,
            row: cursor_row,
            col: cursor_col,
            columns: 0, // Auto-size
            rows: 0,
            x_offset: 0,
            y_offset: 0,
            z_index: -1, // Behind text
        };

        vec![
            ImageEvent::ImageReady { image },
            ImageEvent::Place { placement },
        ]
    }

    /// Reset handler state.
    pub fn reset(&mut self) {
        self.decsdm_enabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::dcs::{
        SixelAspectRatio, SixelBackgroundMode, SixelColor, decode_sixel_to_rgba,
        parse_sixel_sequence,
    };

    // =========================================================================
    // SixelHandler Basic Tests
    // =========================================================================

    #[test]
    fn test_sixel_handler_creation() {
        let _handler = SixelHandler::new();
    }

    #[test]
    fn test_sixel_handler_default() {
        let handler = SixelHandler::default();
        assert!(!handler.is_decsdm_enabled());
    }

    #[test]
    fn test_sixel_process_basic() {
        let mut handler = SixelHandler::new();

        // Create a simple SIXEL data structure
        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::TwoToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![SixelColor {
                index: 0,
                rgba: [255, 0, 0, 255],
            }],
            data: b"~".to_vec(), // Single full column
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        let events = handler.process(&sixel, 5, 10, &mut next_image_id, &mut next_placement_id);

        // Should produce ImageReady and Place events
        assert_eq!(events.len(), 2);

        match &events[0] {
            ImageEvent::ImageReady { image } => {
                assert_eq!(image.id, 1);
                assert_eq!(image.width, 1);
                assert_eq!(image.height, 6);
            }
            _ => panic!("Expected ImageReady"),
        }

        match &events[1] {
            ImageEvent::Place { placement } => {
                assert_eq!(placement.image_id, 1);
                assert_eq!(placement.row, 5);
                assert_eq!(placement.col, 10);
            }
            _ => panic!("Expected Place"),
        }

        // IDs should be incremented
        assert_eq!(next_image_id, 2);
        assert_eq!(next_placement_id, 2);
    }

    #[test]
    fn test_sixel_process_with_colors() {
        let mut handler = SixelHandler::new();

        // SIXEL with red color definition
        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::UseColorZero,
            horizontal_grid: 0,
            colors: vec![
                SixelColor {
                    index: 0,
                    rgba: [255, 0, 0, 255],
                },
                SixelColor {
                    index: 1,
                    rgba: [0, 255, 0, 255],
                },
            ],
            data: b"~~".to_vec(), // Two columns
        };

        let mut next_image_id = 10;
        let mut next_placement_id = 20;

        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);

        assert_eq!(events.len(), 2);

        match &events[0] {
            ImageEvent::ImageReady { image } => {
                assert_eq!(image.id, 10);
                assert_eq!(image.width, 2);
                assert_eq!(image.height, 6);
                // Check that RGBA data is properly encoded
                assert!(!image.rgba_base64.is_empty());
            }
            _ => panic!("Expected ImageReady"),
        }
    }

    #[test]
    fn test_sixel_reset() {
        let mut handler = SixelHandler::new();
        handler.set_decsdm_enabled(true);
        assert!(handler.is_decsdm_enabled());
        handler.reset();
        assert!(!handler.is_decsdm_enabled());
    }

    #[test]
    fn test_sixel_decsdm_mode() {
        let mut handler = SixelHandler::new();

        // Default is disabled
        assert!(!handler.is_decsdm_enabled());

        // Enable DECSDM
        handler.set_decsdm_enabled(true);
        assert!(handler.is_decsdm_enabled());

        // Disable DECSDM
        handler.set_decsdm_enabled(false);
        assert!(!handler.is_decsdm_enabled());
    }

    // =========================================================================
    // DECSDM Mode Tests
    // =========================================================================

    #[test]
    fn test_decsdm_mode_toggle_multiple_times() {
        let mut handler = SixelHandler::new();

        // Toggle multiple times
        for _ in 0..5 {
            handler.set_decsdm_enabled(true);
            assert!(handler.is_decsdm_enabled());
            handler.set_decsdm_enabled(false);
            assert!(!handler.is_decsdm_enabled());
        }
    }

    #[test]
    fn test_decsdm_mode_set_same_value() {
        let mut handler = SixelHandler::new();

        // Set to same value multiple times should be idempotent
        handler.set_decsdm_enabled(true);
        handler.set_decsdm_enabled(true);
        assert!(handler.is_decsdm_enabled());

        handler.set_decsdm_enabled(false);
        handler.set_decsdm_enabled(false);
        assert!(!handler.is_decsdm_enabled());
    }

    #[test]
    fn test_decsdm_preserved_during_process() {
        let mut handler = SixelHandler::new();
        handler.set_decsdm_enabled(true);

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![SixelColor {
                index: 0,
                rgba: [255, 0, 0, 255],
            }],
            data: b"~".to_vec(),
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        // Process should not change DECSDM state
        let _events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
        assert!(handler.is_decsdm_enabled());
    }

    // =========================================================================
    // Edge Cases - Empty and Invalid Data
    // =========================================================================

    #[test]
    fn test_process_empty_sixel_data() {
        let mut handler = SixelHandler::new();

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![],
            data: vec![], // Empty data
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);

        // decode_sixel_to_rgba returns minimum 1x6, so events are generated
        // This tests that empty data is handled gracefully (no panic)
        assert_eq!(events.len(), 2);

        match &events[0] {
            ImageEvent::ImageReady { image } => {
                // Minimum size is 1x6 even for empty data
                assert_eq!(image.width, 1);
                assert_eq!(image.height, 6);
            }
            _ => panic!("Expected ImageReady"),
        }
    }

    #[test]
    fn test_process_only_invalid_characters() {
        let mut handler = SixelHandler::new();

        // Only contains characters outside sixel range (valid sixel chars are 0x3F-0x7E)
        // Note: some characters like '-' are valid newline commands
        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![],
            data: b"abc123".to_vec(), // Invalid sixel characters
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);

        // Invalid characters are skipped, producing minimum 1x6 image
        // This tests graceful handling of invalid input (no panic)
        assert_eq!(events.len(), 2);

        match &events[0] {
            ImageEvent::ImageReady { image } => {
                // Width depends on how the parser handles mixed invalid content
                // Height should be minimum 6
                assert!(image.width >= 1);
                assert_eq!(image.height, 6);
            }
            _ => panic!("Expected ImageReady"),
        }
    }

    #[test]
    fn test_decode_empty_sixel() {
        let data = b"q";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, _pixels) = decode_sixel_to_rgba(&sixel);

        // Minimum size is 1x6
        assert_eq!(width, 1);
        assert_eq!(height, 6);
    }

    // =========================================================================
    // Color Palette Tests
    // =========================================================================

    #[test]
    fn test_process_with_256_colors() {
        let mut handler = SixelHandler::new();

        // Create 256 color definitions
        let colors: Vec<SixelColor> = (0..=255u16)
            .map(|i| SixelColor {
                index: i,
                rgba: [
                    (i % 256) as u8,
                    ((i * 2) % 256) as u8,
                    ((i * 3) % 256) as u8,
                    255,
                ],
            })
            .collect();

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors,
            data: b"~".to_vec(),
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_color_palette_rgb_boundary_values() {
        // Test RGB with boundary values (0, 50, 100)
        let data = b"q#0;2;0;0;0#1;2;50;50;50#2;2;100;100;100~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 3);

        // Color 0: Black (0, 0, 0)
        assert_eq!(sixel.colors[0].rgba, [0, 0, 0, 255]);

        // Color 1: Gray (50%, 50%, 50%) = (127, 127, 127)
        assert_eq!(sixel.colors[1].rgba, [127, 127, 127, 255]);

        // Color 2: White (100%, 100%, 100%) = (255, 255, 255)
        assert_eq!(sixel.colors[2].rgba, [255, 255, 255, 255]);
    }

    #[test]
    fn test_color_palette_hls_boundary_hue() {
        // Test HLS with boundary hue values (0, 180, 360)
        // H=0 (red), H=180 (cyan-ish), H=360 wraps to 0 (red)
        let data = b"q#0;1;0;50;100#1;1;180;50;100#2;1;360;50;100~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 3);

        // Color 0 and Color 2 should be the same (H=0 and H=360 both are red)
        assert_eq!(sixel.colors[0].rgba, sixel.colors[2].rgba);
    }

    #[test]
    fn test_color_palette_hls_saturation_zero() {
        // S=0 should produce gray regardless of hue
        let data = b"q#0;1;0;50;0#1;1;120;50;0#2;1;240;50;0~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 3);

        // All colors should be the same gray
        assert_eq!(sixel.colors[0].rgba, sixel.colors[1].rgba);
        assert_eq!(sixel.colors[1].rgba, sixel.colors[2].rgba);
        // RGB values should be equal (gray)
        assert_eq!(sixel.colors[0].rgba[0], sixel.colors[0].rgba[1]);
        assert_eq!(sixel.colors[0].rgba[1], sixel.colors[0].rgba[2]);
    }

    #[test]
    fn test_color_palette_hls_lightness_extremes() {
        // L=0 should be black, L=100 should be white
        let data = b"q#0;1;0;0;100#1;1;0;100;100~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 2);

        // Color 0: Black (L=0)
        assert_eq!(sixel.colors[0].rgba, [0, 0, 0, 255]);

        // Color 1: White (L=100)
        assert_eq!(sixel.colors[1].rgba, [255, 255, 255, 255]);
    }

    // =========================================================================
    // Repeat Code (!) Edge Cases
    // =========================================================================

    #[test]
    fn test_repeat_code_zero() {
        // !0~ should be treated as !1~ (minimum 1 repeat)
        let data = b"q#0;2;100;0;0!0~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, _height, _pixels) = decode_sixel_to_rgba(&sixel);

        // Should render at least 1 column
        assert!(width >= 1);
    }

    #[test]
    fn test_repeat_code_large_count() {
        // Test with large repeat count
        let data = b"q#0;2;100;0;0!1000~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 1000);
        assert_eq!(height, 6);
        assert_eq!(pixels.len(), (1000 * 6 * 4) as usize);
    }

    #[test]
    fn test_repeat_code_without_following_char() {
        // !5 without following sixel character
        let data = b"q#0;2;100;0;0!5";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, _height, _pixels) = decode_sixel_to_rgba(&sixel);

        // Should not crash and produce minimal output
        assert!(width >= 1);
    }

    #[test]
    fn test_repeat_code_multiple() {
        // Multiple repeat codes in sequence
        let data = b"q#0;2;100;0;0!3~!2@";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, _height, _pixels) = decode_sixel_to_rgba(&sixel);

        // 3 + 2 = 5 columns
        assert_eq!(width, 5);
    }

    // =========================================================================
    // Complex Sixel Data Tests
    // =========================================================================

    #[test]
    fn test_complex_sixel_with_all_features() {
        // Complex SIXEL using color definitions, selection, repeat, newline, and CR
        let data = b"q#0;2;100;0;0#1;2;0;100;0#0!3~$#1!3~-#0!3~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, _pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 3);
        assert_eq!(height, 12); // Two rows of 6 pixels each
    }

    #[test]
    fn test_process_increments_ids_correctly() {
        let mut handler = SixelHandler::new();

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![SixelColor {
                index: 0,
                rgba: [255, 0, 0, 255],
            }],
            data: b"~".to_vec(),
        };

        let mut next_image_id = 100;
        let mut next_placement_id = 200;

        // First process
        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
        assert_eq!(events.len(), 2);
        assert_eq!(next_image_id, 101);
        assert_eq!(next_placement_id, 201);

        // Second process
        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
        assert_eq!(events.len(), 2);
        assert_eq!(next_image_id, 102);
        assert_eq!(next_placement_id, 202);
    }

    #[test]
    fn test_process_placement_position() {
        let mut handler = SixelHandler::new();

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![SixelColor {
                index: 0,
                rgba: [255, 0, 0, 255],
            }],
            data: b"~".to_vec(),
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        // Test various cursor positions
        let test_cases = [(0, 0), (10, 20), (100, 200), (u32::MAX / 2, u32::MAX / 2)];

        for (row, col) in test_cases {
            let events =
                handler.process(&sixel, row, col, &mut next_image_id, &mut next_placement_id);

            match &events[1] {
                ImageEvent::Place { placement } => {
                    assert_eq!(placement.row, row);
                    assert_eq!(placement.col, col);
                }
                _ => panic!("Expected Place event"),
            }
        }
    }

    #[test]
    fn test_sixel_all_characters_in_range() {
        // Test all valid sixel characters (0x3F to 0x7E = '?' to '~')
        let mut data = b"q#0;2;100;0;0".to_vec();
        for ch in 0x3F..=0x7Eu8 {
            data.push(ch);
        }

        let sixel = parse_sixel_sequence(&data).unwrap();
        let (width, height, _pixels) = decode_sixel_to_rgba(&sixel);

        // Should have 64 columns (0x7E - 0x3F + 1 = 64)
        assert_eq!(width, 64);
        assert_eq!(height, 6);
    }

    #[test]
    fn test_sixel_aspect_ratio_variations() {
        let mut handler = SixelHandler::new();

        let aspect_ratios = [
            SixelAspectRatio::TwoToOne,
            SixelAspectRatio::FiveToOne,
            SixelAspectRatio::ThreeToOne,
            SixelAspectRatio::OneToOne,
        ];

        for ratio in aspect_ratios {
            let sixel = SixelData {
                aspect_ratio: ratio,
                background_mode: SixelBackgroundMode::Transparent,
                horizontal_grid: 0,
                colors: vec![SixelColor {
                    index: 0,
                    rgba: [255, 0, 0, 255],
                }],
                data: b"~".to_vec(),
            };

            let mut next_image_id = 1;
            let mut next_placement_id = 1;

            let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
            assert_eq!(events.len(), 2, "Failed for aspect ratio {:?}", ratio);
        }
    }

    #[test]
    fn test_sixel_background_modes() {
        let mut handler = SixelHandler::new();

        let background_modes = [
            SixelBackgroundMode::Transparent,
            SixelBackgroundMode::UseColorZero,
        ];

        for mode in background_modes {
            let sixel = SixelData {
                aspect_ratio: SixelAspectRatio::OneToOne,
                background_mode: mode,
                horizontal_grid: 0,
                colors: vec![SixelColor {
                    index: 0,
                    rgba: [255, 0, 0, 255],
                }],
                data: b"~".to_vec(),
            };

            let mut next_image_id = 1;
            let mut next_placement_id = 1;

            let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);
            assert_eq!(events.len(), 2, "Failed for background mode {:?}", mode);
        }
    }

    #[test]
    fn test_image_ready_has_valid_base64() {
        let mut handler = SixelHandler::new();

        let sixel = SixelData {
            aspect_ratio: SixelAspectRatio::OneToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: vec![SixelColor {
                index: 0,
                rgba: [255, 0, 0, 255],
            }],
            data: b"~".to_vec(),
        };

        let mut next_image_id = 1;
        let mut next_placement_id = 1;

        let events = handler.process(&sixel, 0, 0, &mut next_image_id, &mut next_placement_id);

        match &events[0] {
            ImageEvent::ImageReady { image } => {
                // Base64 should not be empty
                assert!(!image.rgba_base64.is_empty());
                // Should be valid base64 (only contains valid chars)
                assert!(
                    image
                        .rgba_base64
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
                );
            }
            _ => panic!("Expected ImageReady"),
        }
    }
}
