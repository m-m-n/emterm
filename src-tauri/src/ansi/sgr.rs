//! SGR (Select Graphic Rendition) attribute parsing.
//!
//! This module handles parsing of SGR parameters from CSI m sequences,
//! converting numeric parameters into structured attribute changes.
//!
//! # Supported SGR Parameters
//!
//! | Parameter | Description |
//! |-----------|-------------|
//! | 0 | Reset all attributes |
//! | 1 | Bold |
//! | 2 | Dim |
//! | 3 | Italic |
//! | 4 | Underline |
//! | 5 | Blink |
//! | 7 | Reverse |
//! | 8 | Hidden |
//! | 9 | Strikethrough |
//! | 22 | Normal intensity (no bold/dim) |
//! | 23 | Not italic |
//! | 24 | Not underline |
//! | 25 | Not blink |
//! | 27 | Not reverse |
//! | 28 | Not hidden |
//! | 29 | Not strikethrough |
//! | 30-37 | Foreground color (standard 8 colors) |
//! | 38;5;n | Foreground color (256-color palette) |
//! | 38;2;r;g;b | Foreground color (RGB true color) |
//! | 39 | Default foreground color |
//! | 40-47 | Background color (standard 8 colors) |
//! | 48;5;n | Background color (256-color palette) |
//! | 48;2;r;g;b | Background color (RGB true color) |
//! | 49 | Default background color |
//! | 90-97 | Bright foreground color |
//! | 100-107 | Bright background color |

use serde::Serialize;

/// A single SGR (Select Graphic Rendition) attribute.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "attr", content = "value")]
pub enum SgrAttr {
    /// Reset all attributes to default.
    Reset,

    /// Bold (increased intensity).
    Bold,

    /// Dim (decreased intensity).
    Dim,

    /// Italic.
    Italic,

    /// Underline.
    Underline,

    /// Blink (slow blink).
    Blink,

    /// Reverse video (swap foreground and background).
    Reverse,

    /// Hidden (invisible text).
    Hidden,

    /// Strikethrough.
    Strikethrough,

    /// Normal intensity (not bold or dim).
    NormalIntensity,

    /// Not italic.
    NotItalic,

    /// Not underline.
    NotUnderline,

    /// Not blink.
    NotBlink,

    /// Not reverse.
    NotReverse,

    /// Not hidden.
    NotHidden,

    /// Not strikethrough.
    NotStrikethrough,

    /// Set foreground color.
    Foreground(Color),

    /// Set background color.
    Background(Color),

    /// Reset foreground to default.
    DefaultForeground,

    /// Reset background to default.
    DefaultBackground,
}

/// Color specification for SGR attributes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", content = "value")]
pub enum Color {
    /// Standard 8 colors (0-7).
    Standard(u8),

    /// Bright colors (8-15).
    Bright(u8),

    /// 256-color palette index.
    Indexed(u8),

    /// RGB true color.
    Rgb { r: u8, g: u8, b: u8 },
}

/// Parse SGR parameters into a list of attributes.
///
/// # Arguments
///
/// * `params` - The CSI parameters (from CSI Ps m)
///
/// # Returns
///
/// A vector of parsed SGR attributes.
///
/// # Example
///
/// ```
/// use app_lib::ansi::sgr::{parse_sgr, SgrAttr, Color};
///
/// // Parse "CSI 1;31m" (bold + red foreground)
/// let attrs = parse_sgr(&[1, 31]);
/// assert_eq!(attrs, vec![
///     SgrAttr::Bold,
///     SgrAttr::Foreground(Color::Standard(1)), // Red is index 1
/// ]);
/// ```
pub fn parse_sgr(params: &[u16]) -> Vec<SgrAttr> {
    let mut attrs = Vec::new();
    let mut iter = params.iter().peekable();

    // Empty params means reset
    if params.is_empty() {
        attrs.push(SgrAttr::Reset);
        return attrs;
    }

    while let Some(&param) = iter.next() {
        match param {
            // Reset
            0 => attrs.push(SgrAttr::Reset),

            // Text attributes
            1 => attrs.push(SgrAttr::Bold),
            2 => attrs.push(SgrAttr::Dim),
            3 => attrs.push(SgrAttr::Italic),
            4 => attrs.push(SgrAttr::Underline),
            5 => attrs.push(SgrAttr::Blink),
            7 => attrs.push(SgrAttr::Reverse),
            8 => attrs.push(SgrAttr::Hidden),
            9 => attrs.push(SgrAttr::Strikethrough),

            // Attribute resets
            22 => attrs.push(SgrAttr::NormalIntensity),
            23 => attrs.push(SgrAttr::NotItalic),
            24 => attrs.push(SgrAttr::NotUnderline),
            25 => attrs.push(SgrAttr::NotBlink),
            27 => attrs.push(SgrAttr::NotReverse),
            28 => attrs.push(SgrAttr::NotHidden),
            29 => attrs.push(SgrAttr::NotStrikethrough),

            // Standard foreground colors (30-37)
            30..=37 => attrs.push(SgrAttr::Foreground(Color::Standard((param - 30) as u8))),

            // Extended foreground color
            38 => {
                if let Some(color) = parse_extended_color(&mut iter) {
                    attrs.push(SgrAttr::Foreground(color));
                }
            }

            // Default foreground
            39 => attrs.push(SgrAttr::DefaultForeground),

            // Standard background colors (40-47)
            40..=47 => attrs.push(SgrAttr::Background(Color::Standard((param - 40) as u8))),

            // Extended background color
            48 => {
                if let Some(color) = parse_extended_color(&mut iter) {
                    attrs.push(SgrAttr::Background(color));
                }
            }

            // Default background
            49 => attrs.push(SgrAttr::DefaultBackground),

            // Bright foreground colors (90-97)
            90..=97 => attrs.push(SgrAttr::Foreground(Color::Bright((param - 90) as u8))),

            // Bright background colors (100-107)
            100..=107 => attrs.push(SgrAttr::Background(Color::Bright((param - 100) as u8))),

            // Unknown parameters are ignored
            _ => {}
        }
    }

    attrs
}

/// Parse extended color (256-color or RGB) from parameter iterator.
///
/// Extended colors follow the format:
/// - `5;n` for 256-color palette
/// - `2;r;g;b` for RGB true color
fn parse_extended_color<'a, I>(iter: &mut std::iter::Peekable<I>) -> Option<Color>
where
    I: Iterator<Item = &'a u16>,
{
    match iter.next() {
        // 256-color mode
        Some(&5) => {
            let index = iter.next().copied().unwrap_or(0) as u8;
            Some(Color::Indexed(index))
        }
        // RGB mode
        Some(&2) => {
            let r = iter.next().copied().unwrap_or(0) as u8;
            let g = iter.next().copied().unwrap_or(0) as u8;
            let b = iter.next().copied().unwrap_or(0) as u8;
            Some(Color::Rgb { r, g, b })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Reset Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_empty_is_reset() {
        let attrs = parse_sgr(&[]);
        assert_eq!(attrs, vec![SgrAttr::Reset]);
    }

    #[test]
    fn test_parse_sgr_explicit_reset() {
        let attrs = parse_sgr(&[0]);
        assert_eq!(attrs, vec![SgrAttr::Reset]);
    }

    // =========================================================================
    // Text Attribute Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_bold() {
        let attrs = parse_sgr(&[1]);
        assert_eq!(attrs, vec![SgrAttr::Bold]);
    }

    #[test]
    fn test_parse_sgr_dim() {
        let attrs = parse_sgr(&[2]);
        assert_eq!(attrs, vec![SgrAttr::Dim]);
    }

    #[test]
    fn test_parse_sgr_italic() {
        let attrs = parse_sgr(&[3]);
        assert_eq!(attrs, vec![SgrAttr::Italic]);
    }

    #[test]
    fn test_parse_sgr_underline() {
        let attrs = parse_sgr(&[4]);
        assert_eq!(attrs, vec![SgrAttr::Underline]);
    }

    #[test]
    fn test_parse_sgr_blink() {
        let attrs = parse_sgr(&[5]);
        assert_eq!(attrs, vec![SgrAttr::Blink]);
    }

    #[test]
    fn test_parse_sgr_reverse() {
        let attrs = parse_sgr(&[7]);
        assert_eq!(attrs, vec![SgrAttr::Reverse]);
    }

    #[test]
    fn test_parse_sgr_hidden() {
        let attrs = parse_sgr(&[8]);
        assert_eq!(attrs, vec![SgrAttr::Hidden]);
    }

    #[test]
    fn test_parse_sgr_strikethrough() {
        let attrs = parse_sgr(&[9]);
        assert_eq!(attrs, vec![SgrAttr::Strikethrough]);
    }

    // =========================================================================
    // Attribute Reset Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_normal_intensity() {
        let attrs = parse_sgr(&[22]);
        assert_eq!(attrs, vec![SgrAttr::NormalIntensity]);
    }

    #[test]
    fn test_parse_sgr_not_italic() {
        let attrs = parse_sgr(&[23]);
        assert_eq!(attrs, vec![SgrAttr::NotItalic]);
    }

    #[test]
    fn test_parse_sgr_not_underline() {
        let attrs = parse_sgr(&[24]);
        assert_eq!(attrs, vec![SgrAttr::NotUnderline]);
    }

    #[test]
    fn test_parse_sgr_not_blink() {
        let attrs = parse_sgr(&[25]);
        assert_eq!(attrs, vec![SgrAttr::NotBlink]);
    }

    #[test]
    fn test_parse_sgr_not_reverse() {
        let attrs = parse_sgr(&[27]);
        assert_eq!(attrs, vec![SgrAttr::NotReverse]);
    }

    #[test]
    fn test_parse_sgr_not_hidden() {
        let attrs = parse_sgr(&[28]);
        assert_eq!(attrs, vec![SgrAttr::NotHidden]);
    }

    #[test]
    fn test_parse_sgr_not_strikethrough() {
        let attrs = parse_sgr(&[29]);
        assert_eq!(attrs, vec![SgrAttr::NotStrikethrough]);
    }

    // =========================================================================
    // Standard Foreground Color Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_fg_black() {
        let attrs = parse_sgr(&[30]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(0))]);
    }

    #[test]
    fn test_parse_sgr_fg_red() {
        let attrs = parse_sgr(&[31]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(1))]);
    }

    #[test]
    fn test_parse_sgr_fg_green() {
        let attrs = parse_sgr(&[32]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(2))]);
    }

    #[test]
    fn test_parse_sgr_fg_yellow() {
        let attrs = parse_sgr(&[33]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(3))]);
    }

    #[test]
    fn test_parse_sgr_fg_blue() {
        let attrs = parse_sgr(&[34]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(4))]);
    }

    #[test]
    fn test_parse_sgr_fg_magenta() {
        let attrs = parse_sgr(&[35]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(5))]);
    }

    #[test]
    fn test_parse_sgr_fg_cyan() {
        let attrs = parse_sgr(&[36]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(6))]);
    }

    #[test]
    fn test_parse_sgr_fg_white() {
        let attrs = parse_sgr(&[37]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Standard(7))]);
    }

    #[test]
    fn test_parse_sgr_fg_default() {
        let attrs = parse_sgr(&[39]);
        assert_eq!(attrs, vec![SgrAttr::DefaultForeground]);
    }

    // =========================================================================
    // Standard Background Color Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_bg_black() {
        let attrs = parse_sgr(&[40]);
        assert_eq!(attrs, vec![SgrAttr::Background(Color::Standard(0))]);
    }

    #[test]
    fn test_parse_sgr_bg_red() {
        let attrs = parse_sgr(&[41]);
        assert_eq!(attrs, vec![SgrAttr::Background(Color::Standard(1))]);
    }

    #[test]
    fn test_parse_sgr_bg_default() {
        let attrs = parse_sgr(&[49]);
        assert_eq!(attrs, vec![SgrAttr::DefaultBackground]);
    }

    // =========================================================================
    // Bright Color Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_bright_fg_red() {
        let attrs = parse_sgr(&[91]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Bright(1))]);
    }

    #[test]
    fn test_parse_sgr_bright_fg_all() {
        for i in 0..8 {
            let attrs = parse_sgr(&[90 + i as u16]);
            assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Bright(i))]);
        }
    }

    #[test]
    fn test_parse_sgr_bright_bg_red() {
        let attrs = parse_sgr(&[101]);
        assert_eq!(attrs, vec![SgrAttr::Background(Color::Bright(1))]);
    }

    #[test]
    fn test_parse_sgr_bright_bg_all() {
        for i in 0..8 {
            let attrs = parse_sgr(&[100 + i as u16]);
            assert_eq!(attrs, vec![SgrAttr::Background(Color::Bright(i))]);
        }
    }

    // =========================================================================
    // 256-Color Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_fg_256_red() {
        // CSI 38;5;196 m (bright red in 256-color palette)
        let attrs = parse_sgr(&[38, 5, 196]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Indexed(196))]);
    }

    #[test]
    fn test_parse_sgr_fg_256_black() {
        let attrs = parse_sgr(&[38, 5, 0]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Indexed(0))]);
    }

    #[test]
    fn test_parse_sgr_fg_256_max() {
        let attrs = parse_sgr(&[38, 5, 255]);
        assert_eq!(attrs, vec![SgrAttr::Foreground(Color::Indexed(255))]);
    }

    #[test]
    fn test_parse_sgr_bg_256() {
        let attrs = parse_sgr(&[48, 5, 100]);
        assert_eq!(attrs, vec![SgrAttr::Background(Color::Indexed(100))]);
    }

    // =========================================================================
    // RGB True Color Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_fg_rgb_red() {
        // CSI 38;2;255;0;0 m
        let attrs = parse_sgr(&[38, 2, 255, 0, 0]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb { r: 255, g: 0, b: 0 })]
        );
    }

    #[test]
    fn test_parse_sgr_fg_rgb_green() {
        let attrs = parse_sgr(&[38, 2, 0, 255, 0]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb { r: 0, g: 255, b: 0 })]
        );
    }

    #[test]
    fn test_parse_sgr_fg_rgb_blue() {
        let attrs = parse_sgr(&[38, 2, 0, 0, 255]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb { r: 0, g: 0, b: 255 })]
        );
    }

    #[test]
    fn test_parse_sgr_fg_rgb_white() {
        let attrs = parse_sgr(&[38, 2, 255, 255, 255]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb {
                r: 255,
                g: 255,
                b: 255
            })]
        );
    }

    #[test]
    fn test_parse_sgr_bg_rgb() {
        let attrs = parse_sgr(&[48, 2, 128, 64, 32]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Background(Color::Rgb {
                r: 128,
                g: 64,
                b: 32
            })]
        );
    }

    // =========================================================================
    // Combined Attribute Tests
    // =========================================================================

    #[test]
    fn test_parse_sgr_bold_and_red() {
        // CSI 1;31 m
        let attrs = parse_sgr(&[1, 31]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Bold, SgrAttr::Foreground(Color::Standard(1))]
        );
    }

    #[test]
    fn test_parse_sgr_bold_underline_red() {
        // CSI 1;4;31 m
        let attrs = parse_sgr(&[1, 4, 31]);
        assert_eq!(
            attrs,
            vec![
                SgrAttr::Bold,
                SgrAttr::Underline,
                SgrAttr::Foreground(Color::Standard(1))
            ]
        );
    }

    #[test]
    fn test_parse_sgr_fg_and_bg() {
        // CSI 31;42 m (red on green)
        let attrs = parse_sgr(&[31, 42]);
        assert_eq!(
            attrs,
            vec![
                SgrAttr::Foreground(Color::Standard(1)),
                SgrAttr::Background(Color::Standard(2))
            ]
        );
    }

    #[test]
    fn test_parse_sgr_complex() {
        // CSI 1;3;4;38;2;255;128;0 m (bold, italic, underline, orange RGB foreground)
        let attrs = parse_sgr(&[1, 3, 4, 38, 2, 255, 128, 0]);
        assert_eq!(
            attrs,
            vec![
                SgrAttr::Bold,
                SgrAttr::Italic,
                SgrAttr::Underline,
                SgrAttr::Foreground(Color::Rgb {
                    r: 255,
                    g: 128,
                    b: 0
                })
            ]
        );
    }

    #[test]
    fn test_parse_sgr_reset_followed_by_style() {
        // CSI 0;1;31 m (reset, then bold red)
        let attrs = parse_sgr(&[0, 1, 31]);
        assert_eq!(
            attrs,
            vec![
                SgrAttr::Reset,
                SgrAttr::Bold,
                SgrAttr::Foreground(Color::Standard(1))
            ]
        );
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_parse_sgr_unknown_param_ignored() {
        // Unknown parameter 99 should be ignored
        let attrs = parse_sgr(&[99]);
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_parse_sgr_unknown_with_known() {
        // Unknown parameter mixed with known
        let attrs = parse_sgr(&[1, 99, 31]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Bold, SgrAttr::Foreground(Color::Standard(1))]
        );
    }

    #[test]
    fn test_parse_sgr_malformed_256_color() {
        // 38 without following 5;n or 2;r;g;b
        let attrs = parse_sgr(&[38]);
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_parse_sgr_incomplete_rgb() {
        // 38;2 without all RGB components (should still parse with 0 defaults)
        let attrs = parse_sgr(&[38, 2]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb { r: 0, g: 0, b: 0 })]
        );
    }

    #[test]
    fn test_parse_sgr_partial_rgb() {
        // 38;2;255 (only R provided)
        let attrs = parse_sgr(&[38, 2, 255]);
        assert_eq!(
            attrs,
            vec![SgrAttr::Foreground(Color::Rgb { r: 255, g: 0, b: 0 })]
        );
    }

    // =========================================================================
    // Serialization Tests
    // =========================================================================

    #[test]
    fn test_sgr_attr_serialization() {
        let attr = SgrAttr::Bold;
        let json = serde_json::to_string(&attr).unwrap();
        assert!(json.contains("Bold"));
    }

    #[test]
    fn test_color_serialization() {
        let color = Color::Rgb { r: 255, g: 0, b: 0 };
        let json = serde_json::to_string(&color).unwrap();
        assert!(json.contains("Rgb"));
        assert!(json.contains("255"));
    }
}
