//! DCS (Device Control String) sequence parsing.
//!
//! This module handles DCS sequences which start with `ESC P` and end with ST (`ESC \`).
//! The primary use case is SIXEL graphics sequences.
//!
//! # SIXEL Format
//!
//! ```text
//! ESC P [P1];[P2];[P3] q [sixel_data] ESC \
//! ```
//!
//! Where:
//! - P1: Pixel aspect ratio (0=2:1, 1=5:1, 2=3:1, 3-6=2:1, 7-9=1:1)
//! - P2: Background select (0=no background, 1=set to 0, 2=current bg)
//! - P3: Horizontal grid size (not commonly used)
//!
//! # Example
//!
//! ```
//! use term_images::ansi::dcs::{DcsAction, SixelData, parse_sixel_sequence};
//!
//! let data = b"0;1;0q#0;2;0;0;0#1;2;100;100;100~@?~-~@?~";
//! let sixel = parse_sixel_sequence(data);
//! assert!(sixel.is_some());
//! ```

use serde::Serialize;

/// Maximum size for DCS data buffer.
pub const MAX_DCS_LEN: usize = 16 * 1024 * 1024; // 16MB max for SIXEL data

/// DCS sequence action.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum DcsAction {
    /// SIXEL graphics data.
    Sixel(SixelData),

    /// Unknown or malformed DCS sequence.
    Unknown(String),
}

/// SIXEL image data.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SixelData {
    /// Pixel aspect ratio parameter (P1).
    pub aspect_ratio: SixelAspectRatio,

    /// Background mode parameter (P2).
    pub background_mode: SixelBackgroundMode,

    /// Horizontal grid size parameter (P3).
    pub horizontal_grid: u16,

    /// Color definitions from # color commands.
    #[serde(skip)]
    pub colors: Vec<SixelColor>,

    /// Raw SIXEL data (after 'q' introducer).
    #[serde(skip)]
    pub data: Vec<u8>,
}

/// SIXEL pixel aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SixelAspectRatio {
    /// 2:1 ratio (P1 = 0, 3-6)
    TwoToOne,
    /// 5:1 ratio (P1 = 1)
    FiveToOne,
    /// 3:1 ratio (P1 = 2)
    ThreeToOne,
    /// 1:1 ratio (P1 = 7-9)
    OneToOne,
}

impl From<u8> for SixelAspectRatio {
    fn from(value: u8) -> Self {
        match value {
            0 | 3..=6 => Self::TwoToOne,
            1 => Self::FiveToOne,
            2 => Self::ThreeToOne,
            7..=9 => Self::OneToOne,
            _ => Self::TwoToOne, // Default
        }
    }
}

/// SIXEL background mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SixelBackgroundMode {
    /// Transparent background (P2 = 0 or 2)
    Transparent,
    /// Set background to color 0 (P2 = 1)
    UseColorZero,
}

impl From<u8> for SixelBackgroundMode {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::UseColorZero,
            _ => Self::Transparent,
        }
    }
}

/// SIXEL color definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SixelColor {
    /// Color index (0-255 typically).
    pub index: u16,

    /// Color value in RGBA format.
    pub rgba: [u8; 4],
}

/// Color coordinate system for SIXEL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SixelColorSystem {
    /// HLS (Hue, Lightness, Saturation) - coordinate 1
    Hls,
    /// RGB - coordinate 2
    Rgb,
}

impl Default for SixelData {
    fn default() -> Self {
        Self {
            aspect_ratio: SixelAspectRatio::TwoToOne,
            background_mode: SixelBackgroundMode::Transparent,
            horizontal_grid: 0,
            colors: Vec::new(),
            data: Vec::new(),
        }
    }
}

/// Parse SIXEL sequence from DCS data.
///
/// The data should be the content after `ESC P` and before `ESC \`.
///
/// # Format
///
/// ```text
/// [P1];[P2];[P3]q[sixel_data]
/// ```
pub fn parse_sixel_sequence(data: &[u8]) -> Option<SixelData> {
    // Find the 'q' introducer that marks the start of SIXEL data
    let q_pos = data.iter().position(|&b| b == b'q')?;

    // Parse DCS parameters before 'q'
    let params_str = String::from_utf8_lossy(&data[..q_pos]);
    let params: Vec<u16> = params_str
        .split(';')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let mut sixel = SixelData::default();

    // P1: Aspect ratio
    if let Some(&p1) = params.first() {
        sixel.aspect_ratio = SixelAspectRatio::from(p1 as u8);
    }

    // P2: Background mode
    if let Some(&p2) = params.get(1) {
        sixel.background_mode = SixelBackgroundMode::from(p2 as u8);
    }

    // P3: Horizontal grid
    if let Some(&p3) = params.get(2) {
        sixel.horizontal_grid = p3;
    }

    // Store raw SIXEL data (after 'q')
    sixel.data = data[q_pos + 1..].to_vec();

    // Pre-parse color definitions from the data
    sixel.colors = parse_sixel_colors(&sixel.data);

    Some(sixel)
}

/// Parse SIXEL color definitions from data.
///
/// Color definitions have the format:
/// - `#Pc;Pu;Px;Py;Pz` - Define color Pc using coordinate system Pu
///
/// Where:
/// - Pc: Color register number
/// - Pu: Color coordinate system (1=HLS, 2=RGB)
/// - Px, Py, Pz: Color values (0-100 for HLS, 0-100 for RGB percentage)
fn parse_sixel_colors(data: &[u8]) -> Vec<SixelColor> {
    let mut colors = Vec::new();
    let mut i = 0;

    while i < data.len() {
        if data[i] == b'#' {
            // Found color definition start
            i += 1;

            // Parse parameters until we hit a non-parameter character
            let mut params: Vec<u16> = Vec::new();
            let mut num_buf = String::new();

            while i < data.len() {
                let ch = data[i];
                if ch.is_ascii_digit() {
                    num_buf.push(ch as char);
                    i += 1;
                } else if ch == b';' {
                    if !num_buf.is_empty() {
                        if let Ok(n) = num_buf.parse() {
                            params.push(n);
                        }
                        num_buf.clear();
                    }
                    i += 1;
                } else {
                    // End of parameters
                    if !num_buf.is_empty() {
                        if let Ok(n) = num_buf.parse() {
                            params.push(n);
                        }
                    }
                    break;
                }
            }

            // Parse color definition if we have enough params
            // Format: #Pc;Pu;Px;Py;Pz (5 params for definition)
            // Or: #Pc (1 param for selection)
            if params.len() >= 5 {
                let color_index = params[0];
                let coord_system = params[1];
                let x = params[2];
                let y = params[3];
                let z = params[4];

                let rgba = if coord_system == 2 {
                    // RGB (values are 0-100 percentage)
                    let r = ((x as u32 * 255) / 100) as u8;
                    let g = ((y as u32 * 255) / 100) as u8;
                    let b = ((z as u32 * 255) / 100) as u8;
                    [r, g, b, 255]
                } else {
                    // HLS (H: 0-360, L: 0-100, S: 0-100)
                    hls_to_rgb(x, y, z)
                };

                colors.push(SixelColor {
                    index: color_index,
                    rgba,
                });
            }
        } else {
            i += 1;
        }
    }

    colors
}

/// Convert HLS color to RGBA.
///
/// # Parameters
/// - h: Hue (0-360)
/// - l: Lightness (0-100)
/// - s: Saturation (0-100)
fn hls_to_rgb(h: u16, l: u16, s: u16) -> [u8; 4] {
    let h = (h % 360) as f32;
    let l = (l.min(100) as f32) / 100.0;
    let s = (s.min(100) as f32) / 100.0;

    if s == 0.0 {
        // Achromatic (gray)
        let v = (l * 255.0) as u8;
        return [v, v, v, 255];
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let r = hue_to_rgb(p, q, h + 120.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 120.0);

    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]
}

/// Helper for HLS to RGB conversion.
fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 360.0;
    }
    if t > 360.0 {
        t -= 360.0;
    }

    if t < 60.0 {
        p + (q - p) * t / 60.0
    } else if t < 180.0 {
        q
    } else if t < 240.0 {
        p + (q - p) * (240.0 - t) / 60.0
    } else {
        p
    }
}

/// Decode SIXEL data to RGBA pixels.
///
/// Returns (width, height, rgba_data).
pub fn decode_sixel_to_rgba(sixel: &SixelData) -> (u32, u32, Vec<u8>) {
    // Build color palette
    let mut palette: Vec<[u8; 4]> = vec![[0, 0, 0, 0]; 256];

    // Default VGA-like palette for first 16 colors
    let default_colors: [[u8; 4]; 16] = [
        [0, 0, 0, 255],       // 0: Black
        [0, 0, 170, 255],     // 1: Blue
        [170, 0, 0, 255],     // 2: Red
        [170, 0, 170, 255],   // 3: Magenta
        [0, 170, 0, 255],     // 4: Green
        [0, 170, 170, 255],   // 5: Cyan
        [170, 170, 0, 255],   // 6: Yellow
        [170, 170, 170, 255], // 7: White
        [85, 85, 85, 255],    // 8: Bright Black
        [85, 85, 255, 255],   // 9: Bright Blue
        [255, 85, 85, 255],   // 10: Bright Red
        [255, 85, 255, 255],  // 11: Bright Magenta
        [85, 255, 85, 255],   // 12: Bright Green
        [85, 255, 255, 255],  // 13: Bright Cyan
        [255, 255, 85, 255],  // 14: Bright Yellow
        [255, 255, 255, 255], // 15: Bright White
    ];

    for (i, color) in default_colors.iter().enumerate() {
        palette[i] = *color;
    }

    // Apply defined colors
    for color in &sixel.colors {
        if (color.index as usize) < palette.len() {
            palette[color.index as usize] = color.rgba;
        }
    }

    // First pass: determine image dimensions
    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0;
    let mut x: u32 = 0;
    let mut y: u32 = 0;

    let data = &sixel.data;
    let mut i = 0;

    while i < data.len() {
        let ch = data[i];

        match ch {
            // Sixel data characters (? to ~, 0x3F to 0x7E)
            0x3F..=0x7E => {
                x += 1;
                max_x = max_x.max(x);
                max_y = max_y.max(y + 6);
                i += 1;
            }
            // Graphics New Line (-)
            b'-' => {
                y += 6;
                x = 0;
                i += 1;
            }
            // Graphics Carriage Return ($)
            b'$' => {
                x = 0;
                i += 1;
            }
            // Repeat introducer (!)
            b'!' => {
                i += 1;
                let mut count: u32 = 0;
                while i < data.len() && data[i].is_ascii_digit() {
                    count = count * 10 + (data[i] - b'0') as u32;
                    i += 1;
                }
                if i < data.len() && (0x3F..=0x7E).contains(&data[i]) {
                    x += count.max(1);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y + 6);
                    i += 1;
                }
            }
            // Color introducer (#) - skip
            b'#' => {
                i += 1;
                while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
                    i += 1;
                }
            }
            // Raster attributes (") - skip
            b'"' => {
                i += 1;
                while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    let width = max_x.max(1);
    let height = max_y.max(6);

    // Second pass: render pixels
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut current_color: usize = 0;
    x = 0;
    y = 0;
    i = 0;

    while i < data.len() {
        let ch = data[i];

        match ch {
            // Sixel data characters
            0x3F..=0x7E => {
                let sixel_value = ch - 0x3F;
                render_sixel(
                    &mut pixels,
                    width,
                    x,
                    y,
                    sixel_value,
                    &palette[current_color],
                );
                x += 1;
                i += 1;
            }
            // Graphics New Line
            b'-' => {
                y += 6;
                x = 0;
                i += 1;
            }
            // Graphics Carriage Return
            b'$' => {
                x = 0;
                i += 1;
            }
            // Repeat
            b'!' => {
                i += 1;
                let mut count: u32 = 0;
                while i < data.len() && data[i].is_ascii_digit() {
                    count = count * 10 + (data[i] - b'0') as u32;
                    i += 1;
                }
                if i < data.len() && (0x3F..=0x7E).contains(&data[i]) {
                    let sixel_value = data[i] - 0x3F;
                    for _ in 0..count.max(1) {
                        render_sixel(
                            &mut pixels,
                            width,
                            x,
                            y,
                            sixel_value,
                            &palette[current_color],
                        );
                        x += 1;
                    }
                    i += 1;
                }
            }
            // Color selection/definition
            b'#' => {
                i += 1;
                let mut params: Vec<u16> = Vec::new();
                let mut num_buf = String::new();

                while i < data.len() {
                    if data[i].is_ascii_digit() {
                        num_buf.push(data[i] as char);
                        i += 1;
                    } else if data[i] == b';' {
                        if !num_buf.is_empty() {
                            if let Ok(n) = num_buf.parse() {
                                params.push(n);
                            }
                            num_buf.clear();
                        }
                        i += 1;
                    } else {
                        if !num_buf.is_empty() {
                            if let Ok(n) = num_buf.parse() {
                                params.push(n);
                            }
                        }
                        break;
                    }
                }

                if !params.is_empty() {
                    current_color = params[0] as usize % 256;
                }
            }
            // Raster attributes
            b'"' => {
                i += 1;
                while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    (width, height, pixels)
}

/// Render a single sixel character to the pixel buffer.
fn render_sixel(pixels: &mut [u8], width: u32, x: u32, y: u32, sixel_value: u8, color: &[u8; 4]) {
    // Each sixel represents 6 vertical pixels
    for bit in 0..6 {
        if (sixel_value >> bit) & 1 == 1 {
            let py = y + bit as u32;
            let px = x;
            let idx = ((py * width + px) * 4) as usize;
            if idx + 3 < pixels.len() {
                pixels[idx] = color[0];
                pixels[idx + 1] = color[1];
                pixels[idx + 2] = color[2];
                pixels[idx + 3] = color[3];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SIXEL Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_sixel_basic() {
        let data = b"0;1;0q#0;2;100;0;0~@?~-~@?~";
        let sixel = parse_sixel_sequence(data);

        assert!(sixel.is_some());
        let sixel = sixel.unwrap();
        assert_eq!(sixel.aspect_ratio, SixelAspectRatio::TwoToOne);
        assert_eq!(sixel.background_mode, SixelBackgroundMode::UseColorZero);
        assert_eq!(sixel.horizontal_grid, 0);
    }

    #[test]
    fn test_parse_sixel_default_params() {
        let data = b"q#0~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.aspect_ratio, SixelAspectRatio::TwoToOne);
        assert_eq!(sixel.background_mode, SixelBackgroundMode::Transparent);
    }

    #[test]
    fn test_parse_sixel_aspect_ratios() {
        // 5:1 ratio
        let data = b"1;0;0q~";
        let sixel = parse_sixel_sequence(data).unwrap();
        assert_eq!(sixel.aspect_ratio, SixelAspectRatio::FiveToOne);

        // 3:1 ratio
        let data = b"2;0;0q~";
        let sixel = parse_sixel_sequence(data).unwrap();
        assert_eq!(sixel.aspect_ratio, SixelAspectRatio::ThreeToOne);

        // 1:1 ratio
        let data = b"7;0;0q~";
        let sixel = parse_sixel_sequence(data).unwrap();
        assert_eq!(sixel.aspect_ratio, SixelAspectRatio::OneToOne);
    }

    #[test]
    fn test_parse_sixel_background_modes() {
        // Transparent
        let data = b"0;0;0q~";
        let sixel = parse_sixel_sequence(data).unwrap();
        assert_eq!(sixel.background_mode, SixelBackgroundMode::Transparent);

        // Use color 0
        let data = b"0;1;0q~";
        let sixel = parse_sixel_sequence(data).unwrap();
        assert_eq!(sixel.background_mode, SixelBackgroundMode::UseColorZero);
    }

    #[test]
    fn test_parse_sixel_no_q() {
        let data = b"0;1;0#0~";
        let sixel = parse_sixel_sequence(data);
        assert!(sixel.is_none());
    }

    #[test]
    fn test_parse_sixel_color_rgb() {
        let data = b"q#0;2;100;50;25~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 1);
        let color = &sixel.colors[0];
        assert_eq!(color.index, 0);
        // RGB: 100%, 50%, 25% -> 255, 127, 63
        assert_eq!(color.rgba[0], 255);
        assert_eq!(color.rgba[1], 127);
        assert_eq!(color.rgba[2], 63);
        assert_eq!(color.rgba[3], 255);
    }

    #[test]
    fn test_parse_sixel_color_hls() {
        let data = b"q#0;1;0;50;100~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 1);
        let color = &sixel.colors[0];
        assert_eq!(color.index, 0);
        // HLS: H=0 (red), L=50, S=100 should give red
        assert!(color.rgba[0] > 200); // Red should be high
        assert!(color.rgba[3] == 255); // Alpha should be full
    }

    #[test]
    fn test_parse_sixel_multiple_colors() {
        let data = b"q#0;2;100;0;0#1;2;0;100;0#2;2;0;0;100~";
        let sixel = parse_sixel_sequence(data).unwrap();

        assert_eq!(sixel.colors.len(), 3);

        // Color 0: Red
        assert_eq!(sixel.colors[0].index, 0);
        assert_eq!(sixel.colors[0].rgba, [255, 0, 0, 255]);

        // Color 1: Green
        assert_eq!(sixel.colors[1].index, 1);
        assert_eq!(sixel.colors[1].rgba, [0, 255, 0, 255]);

        // Color 2: Blue
        assert_eq!(sixel.colors[2].index, 2);
        assert_eq!(sixel.colors[2].rgba, [0, 0, 255, 255]);
    }

    // =========================================================================
    // SIXEL Decoding Tests
    // =========================================================================

    #[test]
    fn test_decode_sixel_simple() {
        // Simple 1x6 red column
        let data = b"q#0;2;100;0;0~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 1);
        assert_eq!(height, 6);
        assert_eq!(pixels.len(), 24); // 1 * 6 * 4 bytes

        // '~' (0x7E) = 63 in sixel value = 0b111111 = all 6 pixels on
        // All pixels should be red
        for y in 0..6 {
            let idx = (y * 4) as usize;
            assert_eq!(pixels[idx], 255, "Red at y={}", y);
            assert_eq!(pixels[idx + 1], 0, "Green at y={}", y);
            assert_eq!(pixels[idx + 2], 0, "Blue at y={}", y);
            assert_eq!(pixels[idx + 3], 255, "Alpha at y={}", y);
        }
    }

    #[test]
    fn test_decode_sixel_partial() {
        // '?' (0x3F) = 0 in sixel value = 0b000000 = all pixels off
        // '@' (0x40) = 1 in sixel value = 0b000001 = only bottom pixel on
        let data = b"q#0;2;100;0;0@";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 1);
        assert_eq!(height, 6);

        // Only first pixel (y=0) should be colored
        assert_eq!(pixels[0], 255); // Red
        assert_eq!(pixels[1], 0); // Green
        assert_eq!(pixels[2], 0); // Blue
        assert_eq!(pixels[3], 255); // Alpha

        // Other pixels should be transparent (default)
        for y in 1..6 {
            let idx = (y * 4) as usize;
            assert_eq!(pixels[idx + 3], 0, "Alpha should be 0 at y={}", y);
        }
    }

    #[test]
    fn test_decode_sixel_repeat() {
        // !5~ means repeat '~' 5 times
        let data = b"q#0;2;100;0;0!5~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, _pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 5);
        assert_eq!(height, 6);
    }

    #[test]
    fn test_decode_sixel_newline() {
        // Two rows of sixel data
        let data = b"q#0;2;100;0;0~-~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, height, _pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 1);
        assert_eq!(height, 12); // 6 + 6
    }

    #[test]
    fn test_decode_sixel_carriage_return() {
        // $ resets x position, allowing overwriting
        let data = b"q#0;2;100;0;0~~$#1;2;0;100;0~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, _height, pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 2);

        // First column should be green (overwritten)
        assert_eq!(pixels[0], 0); // Red
        assert_eq!(pixels[1], 255); // Green
        assert_eq!(pixels[2], 0); // Blue

        // Second column should be red
        assert_eq!(pixels[4], 255); // Red
        assert_eq!(pixels[5], 0); // Green
        assert_eq!(pixels[6], 0); // Blue
    }

    #[test]
    fn test_decode_sixel_color_selection() {
        // Define two colors, select between them
        let data = b"q#0;2;100;0;0#1;2;0;100;0#0~#1~";
        let sixel = parse_sixel_sequence(data).unwrap();
        let (width, _height, pixels) = decode_sixel_to_rgba(&sixel);

        assert_eq!(width, 2);

        // First column: red
        assert_eq!(pixels[0], 255);
        assert_eq!(pixels[1], 0);
        assert_eq!(pixels[2], 0);

        // Second column: green
        assert_eq!(pixels[4], 0);
        assert_eq!(pixels[5], 255);
        assert_eq!(pixels[6], 0);
    }

    // =========================================================================
    // HLS Conversion Tests
    // =========================================================================

    #[test]
    fn test_hls_to_rgb_red() {
        // H=0 (red), L=50, S=100
        let rgba = hls_to_rgb(0, 50, 100);
        assert!(rgba[0] > 200); // High red
        assert!(rgba[1] < 50); // Low green
        assert!(rgba[2] < 50); // Low blue
    }

    #[test]
    fn test_hls_to_rgb_green() {
        // H=120 (green), L=50, S=100
        let rgba = hls_to_rgb(120, 50, 100);
        assert!(rgba[0] < 50); // Low red
        assert!(rgba[1] > 200); // High green
        assert!(rgba[2] < 50); // Low blue
    }

    #[test]
    fn test_hls_to_rgb_blue() {
        // H=240 (blue), L=50, S=100
        let rgba = hls_to_rgb(240, 50, 100);
        assert!(rgba[0] < 50); // Low red
        assert!(rgba[1] < 50); // Low green
        assert!(rgba[2] > 200); // High blue
    }

    #[test]
    fn test_hls_to_rgb_white() {
        // L=100 should be white regardless of H and S
        let rgba = hls_to_rgb(0, 100, 100);
        assert_eq!(rgba[0], 255);
        assert_eq!(rgba[1], 255);
        assert_eq!(rgba[2], 255);
    }

    #[test]
    fn test_hls_to_rgb_black() {
        // L=0 should be black regardless of H and S
        let rgba = hls_to_rgb(0, 0, 100);
        assert_eq!(rgba[0], 0);
        assert_eq!(rgba[1], 0);
        assert_eq!(rgba[2], 0);
    }

    #[test]
    fn test_hls_to_rgb_gray() {
        // S=0 should be gray regardless of H
        let rgba = hls_to_rgb(180, 50, 0);
        // All RGB should be equal (gray)
        assert_eq!(rgba[0], rgba[1]);
        assert_eq!(rgba[1], rgba[2]);
    }
}
