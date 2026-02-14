// Unicode character width calculation.
//
// Based on Unicode 17.0 / Emoji 17.0 (2025-09-09)
// Port of src/terminal/unicode.ts

// Bit flag constants for packed byte layout
#[allow(dead_code)]
pub const WIDTH_MASK: u8 = 0b0000_0011;
pub const COMBINING: u8 = 0b0000_0100;
pub const EMOJI_PRES: u8 = 0b0000_1000;
pub const EXT_PICTOGRAPHIC: u8 = 0b0001_0000;
pub const REGIONAL_IND: u8 = 0b0010_0000;
pub const SKIN_TONE: u8 = 0b0100_0000;
pub const VARIATION_SEL: u8 = 0b1000_0000;

/// Get the display width of a codepoint in terminal cells.
///
/// Returns 0 for control/combining/zero-width, 1 for narrow, 2 for wide/emoji.
pub fn char_width(cp: u32) -> u8 {
    // Fast path: ASCII printable characters (0x20-0x7E) - most common case
    if (0x20..0x7F).contains(&cp) {
        return 1;
    }

    // C0 control characters (0x00-0x1F)
    if cp <= 0x1F {
        return 0;
    }

    // DEL and C1 control characters (0x7F-0x9F)
    if (0x7F..=0x9F).contains(&cp) {
        return 0;
    }

    // Zero-width characters (must come before Emoji and Latin-1 ranges)
    if is_zero_width(cp) {
        return 0;
    }

    // Emoji_Presentation=Yes characters (must come before Latin-1 range to catch BMP emojis)
    if is_emoji_presentation(cp) {
        return 2;
    }

    // Latin-1 Supplement and common Latin Extended (0xA0-0x2DFF) - narrow
    if (0xA0..0x2E00).contains(&cp) {
        if is_combining_char(cp) {
            return 0;
        }
        return 1;
    }

    // Wide characters (East Asian Width: F, W)
    if is_wide_code_point(cp) {
        return 2;
    }

    // Combining characters (various ranges)
    if is_combining_char(cp) {
        return 0;
    }

    1
}

/// Pack all Unicode properties into a single byte for a codepoint.
///
/// Byte layout:
/// - bits 0-1: width (0, 1, or 2; value 3 is reserved, must not be produced)
/// - bit 2: COMBINING
/// - bit 3: EMOJI_PRES
/// - bit 4: EXT_PICTOGRAPHIC
/// - bit 5: REGIONAL_IND
/// - bit 6: SKIN_TONE
/// - bit 7: VARIATION_SEL
pub fn classify_codepoint(cp: u32) -> u8 {
    let mut byte = char_width(cp);

    if is_combining_char(cp) {
        byte |= COMBINING;
    }
    if is_emoji_presentation(cp) {
        byte |= EMOJI_PRES;
    }
    if is_extended_pictographic(cp) {
        byte |= EXT_PICTOGRAPHIC;
    }
    if is_regional_indicator(cp) {
        byte |= REGIONAL_IND;
    }
    if is_skin_tone_modifier(cp) {
        byte |= SKIN_TONE;
    }
    if is_variation_selector(cp) {
        byte |= VARIATION_SEL;
    }

    byte
}

/// Check if a code point has Emoji_Presentation=Yes property (Unicode 17.0).
///
/// These characters default to emoji (colorful) presentation and are width 2.
pub fn is_emoji_presentation(cp: u32) -> bool {
    // BMP ranges
    if cp == 0x231A || cp == 0x231B {
        return true;
    }
    if (0x23E9..=0x23EC).contains(&cp) {
        return true;
    }
    if cp == 0x23F0 {
        return true;
    }
    if cp == 0x23F3 {
        return true;
    }
    if cp == 0x25FD || cp == 0x25FE {
        return true;
    }
    if cp == 0x2614 || cp == 0x2615 {
        return true;
    }
    if (0x2648..=0x2653).contains(&cp) {
        return true;
    }
    if cp == 0x267F {
        return true;
    }
    if cp == 0x2693 {
        return true;
    }
    if cp == 0x26A1 {
        return true;
    }
    if cp == 0x26AA || cp == 0x26AB {
        return true;
    }
    if cp == 0x26BD || cp == 0x26BE {
        return true;
    }
    if cp == 0x26C4 || cp == 0x26C5 {
        return true;
    }
    if cp == 0x26CE {
        return true;
    }
    if cp == 0x26D4 {
        return true;
    }
    if cp == 0x26EA {
        return true;
    }
    if cp == 0x26F2 || cp == 0x26F3 {
        return true;
    }
    if cp == 0x26F5 {
        return true;
    }
    if cp == 0x26FA {
        return true;
    }
    if cp == 0x26FD {
        return true;
    }
    if cp == 0x2705 {
        return true;
    }
    if cp == 0x270A || cp == 0x270B {
        return true;
    }
    if cp == 0x2728 {
        return true;
    }
    if cp == 0x274C {
        return true;
    }
    if cp == 0x274E {
        return true;
    }
    if (0x2753..=0x2755).contains(&cp) {
        return true;
    }
    if cp == 0x2757 {
        return true;
    }
    if (0x2795..=0x2797).contains(&cp) {
        return true;
    }
    if cp == 0x27B0 {
        return true;
    }
    if cp == 0x27BF {
        return true;
    }
    if cp == 0x2B1B || cp == 0x2B1C {
        return true;
    }
    if cp == 0x2B50 {
        return true;
    }
    if cp == 0x2B55 {
        return true;
    }

    // SMP ranges (U+1F000+)
    if cp == 0x1F004 {
        return true;
    }
    if cp == 0x1F0CF {
        return true;
    }
    if cp == 0x1F18E {
        return true;
    }
    if (0x1F191..=0x1F19A).contains(&cp) {
        return true;
    }
    if (0x1F1E6..=0x1F1FF).contains(&cp) {
        return true;
    }
    if cp == 0x1F201 {
        return true;
    }
    if cp == 0x1F21A {
        return true;
    }
    if cp == 0x1F22F {
        return true;
    }
    if (0x1F232..=0x1F236).contains(&cp) {
        return true;
    }
    if (0x1F238..=0x1F23A).contains(&cp) {
        return true;
    }
    if cp == 0x1F250 || cp == 0x1F251 {
        return true;
    }
    if (0x1F300..=0x1F320).contains(&cp) {
        return true;
    }
    if (0x1F32D..=0x1F335).contains(&cp) {
        return true;
    }
    if (0x1F337..=0x1F37C).contains(&cp) {
        return true;
    }
    if (0x1F37E..=0x1F393).contains(&cp) {
        return true;
    }
    if (0x1F3A0..=0x1F3CA).contains(&cp) {
        return true;
    }
    if (0x1F3CF..=0x1F3D3).contains(&cp) {
        return true;
    }
    if (0x1F3E0..=0x1F3F0).contains(&cp) {
        return true;
    }
    if cp == 0x1F3F4 {
        return true;
    }
    if (0x1F3F8..=0x1F43E).contains(&cp) {
        return true;
    }
    if cp == 0x1F440 {
        return true;
    }
    if (0x1F442..=0x1F4FC).contains(&cp) {
        return true;
    }
    if (0x1F4FF..=0x1F53D).contains(&cp) {
        return true;
    }
    if (0x1F54B..=0x1F54E).contains(&cp) {
        return true;
    }
    if (0x1F550..=0x1F567).contains(&cp) {
        return true;
    }
    if cp == 0x1F57A {
        return true;
    }
    if cp == 0x1F595 || cp == 0x1F596 {
        return true;
    }
    if cp == 0x1F5A4 {
        return true;
    }
    if (0x1F5FB..=0x1F64F).contains(&cp) {
        return true;
    }
    if (0x1F680..=0x1F6C5).contains(&cp) {
        return true;
    }
    if cp == 0x1F6CC {
        return true;
    }
    if (0x1F6D0..=0x1F6D2).contains(&cp) {
        return true;
    }
    if (0x1F6D5..=0x1F6D8).contains(&cp) {
        return true;
    }
    if (0x1F6DC..=0x1F6DF).contains(&cp) {
        return true;
    }
    if cp == 0x1F6EB || cp == 0x1F6EC {
        return true;
    }
    if (0x1F6F4..=0x1F6FC).contains(&cp) {
        return true;
    }
    if (0x1F7E0..=0x1F7EB).contains(&cp) {
        return true;
    }
    if cp == 0x1F7F0 {
        return true;
    }
    if (0x1F90C..=0x1F93A).contains(&cp) {
        return true;
    }
    if (0x1F93C..=0x1F945).contains(&cp) {
        return true;
    }
    if (0x1F947..=0x1F9FF).contains(&cp) {
        return true;
    }
    if (0x1FA70..=0x1FA77).contains(&cp) {
        return true;
    }
    if (0x1FA78..=0x1FA7C).contains(&cp) {
        return true;
    }
    if (0x1FA80..=0x1FA8A).contains(&cp) {
        return true;
    }
    if (0x1FA8E..=0x1FA8F).contains(&cp) {
        return true;
    }
    if (0x1FA90..=0x1FABD).contains(&cp) {
        return true;
    }
    if (0x1FABE..=0x1FABF).contains(&cp) {
        return true;
    }
    if (0x1FAC0..=0x1FAC6).contains(&cp) {
        return true;
    }
    if cp == 0x1FAC8 {
        return true;
    }
    if (0x1FACD..=0x1FACF).contains(&cp) {
        return true;
    }
    if (0x1FAD0..=0x1FADC).contains(&cp) {
        return true;
    }
    if cp == 0x1FADF {
        return true;
    }
    if (0x1FAE0..=0x1FAEA).contains(&cp) {
        return true;
    }
    if cp == 0x1FAEF {
        return true;
    }
    if (0x1FAF0..=0x1FAF8).contains(&cp) {
        return true;
    }

    false
}

/// Check if a code point is zero-width.
///
/// Covers ZWJ, Variation Selectors, and other invisible formatting characters.
pub fn is_zero_width(cp: u32) -> bool {
    // Zero Width Space
    if cp == 0x200B {
        return true;
    }
    // Zero Width Non-Joiner
    if cp == 0x200C {
        return true;
    }
    // Zero Width Joiner
    if cp == 0x200D {
        return true;
    }
    // Word Joiner
    if cp == 0x2060 {
        return true;
    }
    // Zero Width No-Break Space / BOM
    if cp == 0xFEFF {
        return true;
    }
    // Variation Selectors (VS1-VS16)
    if (0xFE00..=0xFE0F).contains(&cp) {
        return true;
    }
    // Variation Selectors Supplement (VS17-VS256)
    if (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }

    false
}

/// Check if a code point is Extended_Pictographic (Unicode 17.0).
///
/// Used for grapheme cluster boundary detection in emoji sequences.
pub fn is_extended_pictographic(cp: u32) -> bool {
    // Specific BMP codepoints
    if cp == 0x00A9 || cp == 0x00AE {
        return true;
    }
    if cp == 0x203C || cp == 0x2049 {
        return true;
    }
    if cp == 0x2122 || cp == 0x2139 {
        return true;
    }
    if (0x2194..=0x2199).contains(&cp) {
        return true;
    }
    if cp == 0x21A9 || cp == 0x21AA {
        return true;
    }
    if cp == 0x231A || cp == 0x231B {
        return true;
    }
    if cp == 0x2328 {
        return true;
    }
    if cp == 0x23CF {
        return true;
    }
    if (0x23E9..=0x23F3).contains(&cp) {
        return true;
    }
    if (0x23F8..=0x23FA).contains(&cp) {
        return true;
    }
    if cp == 0x24C2 {
        return true;
    }
    if cp == 0x25AA || cp == 0x25AB {
        return true;
    }
    if cp == 0x25B6 {
        return true;
    }
    if cp == 0x25C0 {
        return true;
    }
    if (0x25FB..=0x25FE).contains(&cp) {
        return true;
    }
    if (0x2600..=0x27BF).contains(&cp) {
        return true;
    }
    if cp == 0x2934 || cp == 0x2935 {
        return true;
    }
    if (0x2B05..=0x2B07).contains(&cp) {
        return true;
    }
    if cp == 0x2B1B || cp == 0x2B1C {
        return true;
    }
    if cp == 0x2B50 {
        return true;
    }
    if cp == 0x2B55 {
        return true;
    }
    if cp == 0x3030 {
        return true;
    }
    if cp == 0x303D {
        return true;
    }
    if cp == 0x3297 {
        return true;
    }
    if cp == 0x3299 {
        return true;
    }

    // SMP range: U+1F000..U+1FFFD (covers all SMP emoji blocks)
    if (0x1F000..=0x1FFFD).contains(&cp) {
        return true;
    }

    false
}

/// Check if a code point is a Regional Indicator symbol (U+1F1E6..U+1F1FF).
pub fn is_regional_indicator(cp: u32) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&cp)
}

/// Check if a code point is a skin tone modifier (U+1F3FB..U+1F3FF).
pub fn is_skin_tone_modifier(cp: u32) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&cp)
}

/// Check if a code point is a Variation Selector (VS1-VS16 or VS17-VS256).
pub fn is_variation_selector(cp: u32) -> bool {
    (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp)
}

/// Check if a code point is a combining character.
pub fn is_combining_char(cp: u32) -> bool {
    // Combining Diacritical Marks
    if (0x0300..=0x036F).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks Extended
    if (0x1AB0..=0x1AFF).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks Supplement
    if (0x1DC0..=0x1DFF).contains(&cp) {
        return true;
    }
    // Combining Diacritical Marks for Symbols
    if (0x20D0..=0x20FF).contains(&cp) {
        return true;
    }
    // Combining Half Marks
    if (0xFE20..=0xFE2F).contains(&cp) {
        return true;
    }

    false
}

/// Check if a code point is wide (East Asian Width: F or W).
pub fn is_wide_code_point(cp: u32) -> bool {
    // CJK Radicals Supplement
    if (0x2E80..=0x2EFF).contains(&cp) {
        return true;
    }
    // Kangxi Radicals
    if (0x2F00..=0x2FDF).contains(&cp) {
        return true;
    }
    // CJK Symbols and Punctuation
    if (0x3000..=0x303F).contains(&cp) {
        return true;
    }
    // Hiragana
    if (0x3040..=0x309F).contains(&cp) {
        return true;
    }
    // Katakana
    if (0x30A0..=0x30FF).contains(&cp) {
        return true;
    }
    // Bopomofo
    if (0x3100..=0x312F).contains(&cp) {
        return true;
    }
    // Hangul Compatibility Jamo
    if (0x3130..=0x318F).contains(&cp) {
        return true;
    }
    // Kanbun
    if (0x3190..=0x319F).contains(&cp) {
        return true;
    }
    // Bopomofo Extended
    if (0x31A0..=0x31BF).contains(&cp) {
        return true;
    }
    // CJK Strokes
    if (0x31C0..=0x31EF).contains(&cp) {
        return true;
    }
    // Katakana Phonetic Extensions
    if (0x31F0..=0x31FF).contains(&cp) {
        return true;
    }
    // Enclosed CJK Letters and Months
    if (0x3200..=0x32FF).contains(&cp) {
        return true;
    }
    // CJK Compatibility
    if (0x3300..=0x33FF).contains(&cp) {
        return true;
    }
    // CJK Unified Ideographs Extension A
    if (0x3400..=0x4DBF).contains(&cp) {
        return true;
    }
    // CJK Unified Ideographs
    if (0x4E00..=0x9FFF).contains(&cp) {
        return true;
    }
    // Yi Syllables
    if (0xA000..=0xA48F).contains(&cp) {
        return true;
    }
    // Yi Radicals
    if (0xA490..=0xA4CF).contains(&cp) {
        return true;
    }
    // Hangul Syllables
    if (0xAC00..=0xD7A3).contains(&cp) {
        return true;
    }
    // CJK Compatibility Ideographs
    if (0xF900..=0xFAFF).contains(&cp) {
        return true;
    }
    // Vertical Forms
    if (0xFE10..=0xFE1F).contains(&cp) {
        return true;
    }
    // CJK Compatibility Forms
    if (0xFE30..=0xFE4F).contains(&cp) {
        return true;
    }
    // Fullwidth Forms (excluding halfwidth)
    if (0xFF00..=0xFF60).contains(&cp) {
        return true;
    }
    if (0xFFE0..=0xFFE6).contains(&cp) {
        return true;
    }
    // CJK Unified Ideographs Extension B and beyond
    if (0x20000..=0x2FFFF).contains(&cp) {
        return true;
    }
    // CJK Compatibility Ideographs Supplement
    if (0x2F800..=0x2FA1F).contains(&cp) {
        return true;
    }

    false
}

/// Calculate the display width of a string.
pub fn string_width(s: &str) -> u32 {
    s.chars().map(|c| char_width(c as u32) as u32).sum()
}

/// Classify all codepoints in a string, returning a packed byte per codepoint.
pub fn classify_codepoints(s: &str) -> Vec<u8> {
    s.chars().map(|c| classify_codepoint(c as u32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── char_width ──────────────────────────────────────────

    #[test]
    fn ascii_printable_width_1() {
        assert_eq!(char_width(b'a' as u32), 1);
        assert_eq!(char_width(b'A' as u32), 1);
        assert_eq!(char_width(b'0' as u32), 1);
        assert_eq!(char_width(b' ' as u32), 1);
        assert_eq!(char_width(b'~' as u32), 1);
        assert_eq!(char_width(b'z' as u32), 1);
    }

    #[test]
    fn control_chars_width_0() {
        assert_eq!(char_width(0x00), 0); // NUL
        assert_eq!(char_width(0x07), 0); // BEL
        assert_eq!(char_width(0x1B), 0); // ESC
        assert_eq!(char_width(0x7F), 0); // DEL
        assert_eq!(char_width(0x9F), 0); // last C1
    }

    #[test]
    fn cjk_width_2() {
        assert_eq!(char_width(0x4E00), 2); // first CJK unified ideograph
        assert_eq!(char_width(0x9FFF), 2); // last CJK unified ideograph
        assert_eq!(char_width(0x3042), 2); // Hiragana 'a'
        assert_eq!(char_width(0x30A2), 2); // Katakana 'a'
    }

    #[test]
    fn fullwidth_width_2() {
        assert_eq!(char_width(0xFF01), 2); // Fullwidth exclamation mark
        assert_eq!(char_width(0xFF21), 2); // Fullwidth 'A'
        assert_eq!(char_width(0xFF10), 2); // Fullwidth '0'
    }

    #[test]
    fn hangul_width_2() {
        assert_eq!(char_width(0xAC00), 2); // First Hangul syllable
        assert_eq!(char_width(0xD7A3), 2); // Last Hangul syllable
    }

    #[test]
    fn halfwidth_width_1() {
        assert_eq!(char_width(0xFF61), 1); // Halfwidth ideographic full stop
        assert_eq!(char_width(0xFF66), 1); // Halfwidth Katakana 'wo'
    }

    #[test]
    fn combining_width_0() {
        assert_eq!(char_width(0x0300), 0); // Combining grave accent
        assert_eq!(char_width(0x0301), 0); // Combining acute accent
    }

    #[test]
    fn smp_emoji_width_2() {
        assert_eq!(char_width(0x1F4C1), 2); // 📁
        assert_eq!(char_width(0x1F50B), 2); // 🔋
        assert_eq!(char_width(0x1F600), 2); // 😀
        assert_eq!(char_width(0x1F680), 2); // 🚀
    }

    #[test]
    fn bmp_emoji_presentation_width_2() {
        assert_eq!(char_width(0x231A), 2); // ⌚
        assert_eq!(char_width(0x23F0), 2); // ⏰
        assert_eq!(char_width(0x2615), 2); // ☕
        assert_eq!(char_width(0x2B50), 2); // ⭐
        assert_eq!(char_width(0x231B), 2); // ⌛
        assert_eq!(char_width(0x267F), 2); // ♿
        assert_eq!(char_width(0x26D4), 2); // ⛔
        assert_eq!(char_width(0x2705), 2); // ✅
        assert_eq!(char_width(0x274C), 2); // ❌
        assert_eq!(char_width(0x2757), 2); // ❗
    }

    #[test]
    fn non_emoji_presentation_width_1() {
        assert_eq!(char_width(0x2600), 1); // ☀ NOT Emoji_Presentation
        assert_eq!(char_width(0x260E), 1); // ☎ NOT Emoji_Presentation
        assert_eq!(char_width(0x2709), 1); // ✉ NOT Emoji_Presentation
    }

    #[test]
    fn zero_width_chars() {
        assert_eq!(char_width(0x200D), 0); // ZWJ
        assert_eq!(char_width(0xFE0F), 0); // VS16
        assert_eq!(char_width(0xFE0E), 0); // VS15
        assert_eq!(char_width(0xFE00), 0); // VS1
        assert_eq!(char_width(0x200B), 0); // Zero Width Space
        assert_eq!(char_width(0x200C), 0); // Zero Width Non-Joiner
        assert_eq!(char_width(0x2060), 0); // Word Joiner
        assert_eq!(char_width(0xFEFF), 0); // BOM
    }

    #[test]
    fn invalid_codepoint_returns_1() {
        // Beyond valid Unicode range defaults to 1 (narrow)
        assert_eq!(char_width(0x10FFFF), 1);
    }

    // ── is_emoji_presentation ───────────────────────────────

    #[test]
    fn emoji_presentation_true() {
        assert!(is_emoji_presentation(0x1F4C1)); // 📁
        assert!(is_emoji_presentation(0x1F600)); // 😀
        assert!(is_emoji_presentation(0x231A)); // ⌚
        assert!(is_emoji_presentation(0x2615)); // ☕
        assert!(is_emoji_presentation(0x2B50)); // ⭐
    }

    #[test]
    fn emoji_presentation_false() {
        assert!(!is_emoji_presentation(0x41)); // 'A'
        assert!(!is_emoji_presentation(0x2600)); // ☀
        assert!(!is_emoji_presentation(0x4E00)); // CJK
    }

    // ── is_extended_pictographic ────────────────────────────

    #[test]
    fn extended_pictographic_true() {
        assert!(is_extended_pictographic(0x00A9)); // ©
        assert!(is_extended_pictographic(0x00AE)); // ®
        assert!(is_extended_pictographic(0x2600)); // ☀
        assert!(is_extended_pictographic(0x1F600)); // 😀
        assert!(is_extended_pictographic(0x1F4C1)); // 📁
    }

    #[test]
    fn extended_pictographic_false() {
        assert!(!is_extended_pictographic(0x41)); // 'A'
        assert!(!is_extended_pictographic(0x4E00)); // CJK
    }

    // ── is_regional_indicator ───────────────────────────────

    #[test]
    fn regional_indicator() {
        assert!(is_regional_indicator(0x1F1E6)); // first
        assert!(is_regional_indicator(0x1F1FF)); // last
        assert!(!is_regional_indicator(0x1F1E5));
        assert!(!is_regional_indicator(0x1F200));
    }

    // ── is_skin_tone_modifier ───────────────────────────────

    #[test]
    fn skin_tone_modifier() {
        assert!(is_skin_tone_modifier(0x1F3FB)); // first
        assert!(is_skin_tone_modifier(0x1F3FF)); // last
        assert!(!is_skin_tone_modifier(0x1F3FA));
        assert!(!is_skin_tone_modifier(0x1F400));
    }

    // ── is_variation_selector ───────────────────────────────

    #[test]
    fn variation_selector() {
        assert!(is_variation_selector(0xFE00)); // VS1
        assert!(is_variation_selector(0xFE0F)); // VS16
        assert!(is_variation_selector(0xE0100)); // VS17
        assert!(is_variation_selector(0xE01EF)); // VS256
        assert!(!is_variation_selector(0xFDFF));
        assert!(!is_variation_selector(0xFE10));
        assert!(!is_variation_selector(0xE00FF));
        assert!(!is_variation_selector(0xE01F0));
    }

    // ── is_combining_char ───────────────────────────────────

    #[test]
    fn combining_char() {
        assert!(is_combining_char(0x0300)); // Combining grave accent
        assert!(is_combining_char(0x0301)); // Combining acute accent
        assert!(is_combining_char(0x036F)); // last in basic range
        assert!(is_combining_char(0x1AB0)); // Combining Diacritical Marks Extended
        assert!(is_combining_char(0x20D0)); // Combining Diacritical Marks for Symbols
        assert!(is_combining_char(0xFE20)); // Combining Half Marks
        assert!(!is_combining_char(0x41)); // 'A'
        assert!(!is_combining_char(0x2FF)); // just before range
    }

    // ── is_wide_code_point ──────────────────────────────────

    #[test]
    fn wide_code_point() {
        assert!(is_wide_code_point(0x4E00)); // CJK
        assert!(is_wide_code_point(0x3042)); // Hiragana
        assert!(is_wide_code_point(0x30A2)); // Katakana
        assert!(is_wide_code_point(0xAC00)); // Hangul
        assert!(is_wide_code_point(0xFF01)); // Fullwidth
        assert!(is_wide_code_point(0x20000)); // CJK Extension B
        assert!(!is_wide_code_point(0x41)); // 'A'
        assert!(!is_wide_code_point(0xFF61)); // Halfwidth
    }

    // ── classify_codepoint ──────────────────────────────────

    #[test]
    fn classify_ascii() {
        let b = classify_codepoint(b'A' as u32);
        assert_eq!(b & WIDTH_MASK, 1);
        assert_eq!(b & COMBINING, 0);
        assert_eq!(b & EMOJI_PRES, 0);
    }

    #[test]
    fn classify_cjk() {
        let b = classify_codepoint(0x4E00);
        assert_eq!(b & WIDTH_MASK, 2);
        assert_eq!(b & EMOJI_PRES, 0);
    }

    #[test]
    fn classify_emoji_presentation() {
        let b = classify_codepoint(0x231A); // ⌚
        assert_eq!(b & WIDTH_MASK, 2);
        assert_ne!(b & EMOJI_PRES, 0);
        assert_ne!(b & EXT_PICTOGRAPHIC, 0);
    }

    #[test]
    fn classify_combining() {
        let b = classify_codepoint(0x0300);
        assert_eq!(b & WIDTH_MASK, 0);
        assert_ne!(b & COMBINING, 0);
    }

    #[test]
    fn classify_variation_selector() {
        let b = classify_codepoint(0xFE0F); // VS16
        assert_eq!(b & WIDTH_MASK, 0);
        assert_ne!(b & VARIATION_SEL, 0);
    }

    #[test]
    fn classify_regional_indicator() {
        let b = classify_codepoint(0x1F1E6);
        assert_ne!(b & REGIONAL_IND, 0);
        assert_ne!(b & EMOJI_PRES, 0); // also has emoji presentation
    }

    #[test]
    fn classify_skin_tone() {
        let b = classify_codepoint(0x1F3FB);
        assert_ne!(b & SKIN_TONE, 0);
        assert_ne!(b & EMOJI_PRES, 0); // skin tone modifiers are in emoji presentation range
    }

    #[test]
    fn classify_width_3_never_produced() {
        // Verify width=3 is never returned for any common codepoint
        for cp in 0u32..0x10000 {
            let w = char_width(cp);
            assert!(w <= 2, "char_width(0x{:04X}) returned {}", cp, w);
        }
    }

    // ── string_width ────────────────────────────────────────

    #[test]
    fn string_width_basic() {
        assert_eq!(string_width("hello"), 5);
        assert_eq!(string_width(""), 0);
        assert_eq!(string_width("abc"), 3);
    }

    #[test]
    fn string_width_cjk() {
        assert_eq!(string_width("漢字"), 4);
        assert_eq!(string_width("あいう"), 6);
    }

    #[test]
    fn string_width_mixed() {
        assert_eq!(string_width("aあb"), 4); // 1 + 2 + 1
    }

    #[test]
    fn string_width_emoji() {
        assert_eq!(string_width("😀"), 2);
        assert_eq!(string_width("a😀b"), 4); // 1 + 2 + 1
    }

    // ── classify_codepoints (batch) ─────────────────────────

    #[test]
    fn classify_codepoints_empty() {
        assert!(classify_codepoints("").is_empty());
    }

    #[test]
    fn classify_codepoints_ascii() {
        let result = classify_codepoints("abc");
        assert_eq!(result.len(), 3);
        for b in &result {
            assert_eq!(b & WIDTH_MASK, 1);
        }
    }

    #[test]
    fn classify_codepoints_mixed() {
        let result = classify_codepoints("aあ😀");
        assert_eq!(result.len(), 3);
        assert_eq!(result[0] & WIDTH_MASK, 1); // 'a'
        assert_eq!(result[1] & WIDTH_MASK, 2); // 'あ'
        assert_eq!(result[2] & WIDTH_MASK, 2); // 😀
        assert_ne!(result[2] & EMOJI_PRES, 0);
    }

    #[test]
    fn classify_codepoints_surrogate_pair() {
        // Rust strings are UTF-8, so surrogate pairs aren't an issue.
        // But SMP characters (>U+FFFF) must still work.
        let result = classify_codepoints("🚀");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0] & WIDTH_MASK, 2);
        assert_ne!(result[0] & EMOJI_PRES, 0);
    }
}
