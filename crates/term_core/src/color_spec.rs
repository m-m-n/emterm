#![allow(dead_code)]

// Color specification parser for OSC 4/10/11/12 sequences.
// Parses: rgb:r/g/b, #RGB, #RRGGBB, #RRRRGGGGBBBB, ? (query)

/// Result of parsing a color specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorSpecResult {
    /// Parsed color as 8-bit RGB.
    Color(u8, u8, u8),
    /// Query token (`?`).
    Query,
}

/// Parse a color specification string into an 8-bit RGB value or query token.
///
/// Returns `None` for invalid/unrecognized formats.
pub fn parse_color_spec(spec: &str) -> Option<ColorSpecResult> {
    let spec = spec.trim();
    if spec == "?" {
        return Some(ColorSpecResult::Query);
    }

    if let Some(rest) = spec.strip_prefix("rgb:") {
        parse_rgb_colon(rest)
    } else if let Some(rest) = spec.strip_prefix('#') {
        parse_hash(rest)
    } else {
        None
    }
}

/// Parse `r/g/b` format where each component is 1, 2, or 4 hex digits.
fn parse_rgb_colon(s: &str) -> Option<ColorSpecResult> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parse_component(parts[0])?;
    let g = parse_component(parts[1])?;
    let b = parse_component(parts[2])?;
    Some(ColorSpecResult::Color(r, g, b))
}

/// Parse a single hex component (1, 2, or 4 hex digits) to 8-bit value.
fn parse_component(s: &str) -> Option<u8> {
    let len = s.len();
    let val = u16::from_str_radix(s, 16).ok()?;
    match len {
        1 => Some((val as u8) * 17), // 0xF -> 0xFF
        2 => Some(val as u8),
        4 => Some((val >> 8) as u8), // Downscale 16-bit to 8-bit
        _ => None,
    }
}

/// Parse `#` prefixed formats: `#RGB`, `#RRGGBB`, `#RRRRGGGGBBBB`.
fn parse_hash(s: &str) -> Option<ColorSpecResult> {
    match s.len() {
        3 => {
            // #RGB
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            Some(ColorSpecResult::Color(r, g, b))
        }
        6 => {
            // #RRGGBB
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(ColorSpecResult::Color(r, g, b))
        }
        12 => {
            // #RRRRGGGGBBBB
            let r = u16::from_str_radix(&s[0..4], 16).ok()?;
            let g = u16::from_str_radix(&s[4..8], 16).ok()?;
            let b = u16::from_str_radix(&s[8..12], 16).ok()?;
            Some(ColorSpecResult::Color(
                (r >> 8) as u8,
                (g >> 8) as u8,
                (b >> 8) as u8,
            ))
        }
        _ => None,
    }
}

/// Format an 8-bit RGB color as a 16-bit xterm query response: `rgb:rrrr/gggg/bbbb`.
pub fn format_color_response(r: u8, g: u8, b: u8) -> String {
    // Expand 8-bit to 16-bit: 0xAB -> 0xABAB
    let r16 = (r as u16) << 8 | r as u16;
    let g16 = (g as u16) << 8 | g as u16;
    let b16 = (b as u16) << 8 | b as u16;
    format!("rgb:{:04x}/{:04x}/{:04x}", r16, g16, b16)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_color_spec tests ──────────────────────────

    #[test]
    fn test_query_token() {
        assert_eq!(parse_color_spec("?"), Some(ColorSpecResult::Query));
    }

    #[test]
    fn test_rgb_colon_2digit() {
        assert_eq!(
            parse_color_spec("rgb:ff/00/80"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn test_rgb_colon_4digit() {
        // 16-bit values downscaled: 0xFFFF -> 0xFF, 0x0000 -> 0x00, 0x8080 -> 0x80
        assert_eq!(
            parse_color_spec("rgb:ffff/0000/8080"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn test_rgb_colon_1digit() {
        // 1-digit: 0xF -> 0xFF, 0x0 -> 0x00, 0x8 -> 0x88
        assert_eq!(
            parse_color_spec("rgb:f/0/8"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x88))
        );
    }

    #[test]
    fn test_hash_rgb() {
        assert_eq!(
            parse_color_spec("#F08"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x88))
        );
    }

    #[test]
    fn test_hash_rrggbb() {
        assert_eq!(
            parse_color_spec("#ff0080"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn test_hash_rrrrggggbbbb() {
        assert_eq!(
            parse_color_spec("#ffff00008080"),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x80))
        );
    }

    #[test]
    fn test_invalid_formats() {
        assert_eq!(parse_color_spec(""), None);
        assert_eq!(parse_color_spec("invalid"), None);
        assert_eq!(parse_color_spec("rgb:"), None);
        assert_eq!(parse_color_spec("rgb:ff/gg/00"), None);
        assert_eq!(parse_color_spec("#ZZZZZZ"), None);
        assert_eq!(parse_color_spec("#12345"), None); // 5 digits (invalid)
    }

    #[test]
    fn test_whitespace_trimming() {
        assert_eq!(
            parse_color_spec("  rgb:ff/00/80  "),
            Some(ColorSpecResult::Color(0xff, 0x00, 0x80))
        );
        assert_eq!(parse_color_spec(" ? "), Some(ColorSpecResult::Query));
    }

    // ── format_color_response tests ─────────────────────

    #[test]
    fn test_format_response_black() {
        assert_eq!(format_color_response(0, 0, 0), "rgb:0000/0000/0000");
    }

    #[test]
    fn test_format_response_white() {
        assert_eq!(format_color_response(255, 255, 255), "rgb:ffff/ffff/ffff");
    }

    #[test]
    fn test_format_response_red() {
        assert_eq!(format_color_response(255, 0, 0), "rgb:ffff/0000/0000");
    }

    #[test]
    fn test_format_response_mixed() {
        // 0x80 -> 0x8080
        assert_eq!(
            format_color_response(0x80, 0x40, 0xc0),
            "rgb:8080/4040/c0c0"
        );
    }

    // ── roundtrip tests ─────────────────────────────────

    #[test]
    fn test_parse_roundtrip() {
        let response = format_color_response(0xab, 0xcd, 0xef);
        // Response is "rgb:abab/cdcd/efef"
        // Parsing back: 0xabab >> 8 = 0xab, etc.
        let parsed = parse_color_spec(&response);
        assert_eq!(parsed, Some(ColorSpecResult::Color(0xab, 0xcd, 0xef)));
    }
}
