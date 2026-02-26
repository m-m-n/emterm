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

/// Check if a code point is a combining character (Unicode 17.0, General Category Mn/Me).
///
/// Uses binary search on a comprehensive table of combining mark ranges.
/// This covers all major scripts including Arabic, Hebrew, Indic, Tibetan, etc.
/// which are needed for correct terminal width calculation and Kitty Graphics
/// Protocol Unicode placeholder suppression.
pub fn is_combining_char(cp: u32) -> bool {
    COMBINING_RANGES
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Sorted table of Unicode combining character ranges (Mn/Me categories).
/// Each entry is (start, end) inclusive. Based on Unicode 17.0.
///
/// Note: Variation Selectors (FE00-FE0F, E0100-E01EF) are handled separately
/// by `is_variation_selector()` / `is_zero_width()` and not included here.
static COMBINING_RANGES: &[(u32, u32)] = &[
    // Combining Diacritical Marks
    (0x0300, 0x036F),
    // Cyrillic combining marks
    (0x0483, 0x0489),
    // Hebrew accents and marks
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    // Arabic combining marks
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    // Syriac
    (0x0711, 0x0711),
    (0x0730, 0x074A),
    // Thaana
    (0x07A6, 0x07B0),
    // NKo
    (0x07EB, 0x07F3),
    (0x07FD, 0x07FD),
    // Samaritan
    (0x0816, 0x0819),
    (0x081B, 0x0823),
    (0x0825, 0x0827),
    (0x0829, 0x082D),
    // Mandaic
    (0x0859, 0x085B),
    // Arabic Extended-B
    (0x0898, 0x089F),
    // Arabic Extended-A / Devanagari signs
    (0x08CA, 0x0903),
    // Devanagari
    (0x093A, 0x093C),
    (0x093E, 0x094F),
    (0x0951, 0x0957),
    (0x0962, 0x0963),
    // Bengali
    (0x0981, 0x0983),
    (0x09BC, 0x09BC),
    (0x09BE, 0x09C4),
    (0x09C7, 0x09C8),
    (0x09CB, 0x09CD),
    (0x09D7, 0x09D7),
    (0x09E2, 0x09E3),
    (0x09FE, 0x09FE),
    // Gurmukhi
    (0x0A01, 0x0A03),
    (0x0A3C, 0x0A3C),
    (0x0A3E, 0x0A42),
    (0x0A47, 0x0A48),
    (0x0A4B, 0x0A4D),
    (0x0A51, 0x0A51),
    (0x0A70, 0x0A71),
    (0x0A75, 0x0A75),
    // Gujarati
    (0x0A81, 0x0A83),
    (0x0ABC, 0x0ABC),
    (0x0ABE, 0x0AC5),
    (0x0AC7, 0x0AC9),
    (0x0ACB, 0x0ACD),
    (0x0AE2, 0x0AE3),
    (0x0AFA, 0x0AFF),
    // Oriya
    (0x0B01, 0x0B03),
    (0x0B3C, 0x0B3C),
    (0x0B3E, 0x0B44),
    (0x0B47, 0x0B48),
    (0x0B4B, 0x0B4D),
    (0x0B55, 0x0B57),
    (0x0B62, 0x0B63),
    // Tamil
    (0x0B82, 0x0B82),
    (0x0BBE, 0x0BC2),
    (0x0BC6, 0x0BC8),
    (0x0BCA, 0x0BCD),
    (0x0BD7, 0x0BD7),
    // Telugu
    (0x0C00, 0x0C04),
    (0x0C3C, 0x0C3C),
    (0x0C3E, 0x0C44),
    (0x0C46, 0x0C48),
    (0x0C4A, 0x0C4D),
    (0x0C55, 0x0C56),
    (0x0C62, 0x0C63),
    // Kannada
    (0x0C81, 0x0C83),
    (0x0CBC, 0x0CBC),
    (0x0CBE, 0x0CC4),
    (0x0CC6, 0x0CC8),
    (0x0CCA, 0x0CCD),
    (0x0CD5, 0x0CD6),
    (0x0CE2, 0x0CE3),
    (0x0CF3, 0x0CF3),
    // Malayalam
    (0x0D00, 0x0D03),
    (0x0D3B, 0x0D3C),
    (0x0D3E, 0x0D44),
    (0x0D46, 0x0D48),
    (0x0D4A, 0x0D4D),
    (0x0D57, 0x0D57),
    (0x0D62, 0x0D63),
    // Sinhala
    (0x0D81, 0x0D83),
    (0x0DCA, 0x0DCA),
    (0x0DCF, 0x0DD4),
    (0x0DD6, 0x0DD6),
    (0x0DD8, 0x0DDF),
    (0x0DF2, 0x0DF3),
    // Thai
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    // Lao
    (0x0EB1, 0x0EB1),
    (0x0EB4, 0x0EBC),
    (0x0EC8, 0x0ECE),
    // Tibetan
    (0x0F18, 0x0F19),
    (0x0F35, 0x0F35),
    (0x0F37, 0x0F37),
    (0x0F39, 0x0F39),
    (0x0F71, 0x0F84),
    (0x0F86, 0x0F87),
    (0x0F8D, 0x0F97),
    (0x0F99, 0x0FBC),
    (0x0FC6, 0x0FC6),
    // Myanmar
    (0x102B, 0x103E),
    (0x1056, 0x1059),
    (0x105E, 0x1060),
    (0x1062, 0x1064),
    (0x1067, 0x106D),
    (0x1071, 0x1074),
    (0x1082, 0x108D),
    (0x108F, 0x108F),
    (0x109A, 0x109D),
    // Ethiopic
    (0x135D, 0x135F),
    // Tagalog
    (0x1712, 0x1715),
    // Hanunoo
    (0x1732, 0x1734),
    // Buhid
    (0x1752, 0x1753),
    // Tagbanwa
    (0x1772, 0x1773),
    // Khmer
    (0x17B4, 0x17D3),
    (0x17DD, 0x17DD),
    // Mongolian
    (0x180B, 0x180D),
    (0x180F, 0x180F),
    (0x1885, 0x1886),
    (0x18A9, 0x18A9),
    // Limbu
    (0x1920, 0x192B),
    (0x1930, 0x193B),
    // Buginese
    (0x1A17, 0x1A1B),
    // Tai Tham
    (0x1A55, 0x1A5E),
    (0x1A60, 0x1A7C),
    (0x1A7F, 0x1A7F),
    // Combining Diacritical Marks Extended
    (0x1AB0, 0x1ACE),
    // Balinese
    (0x1B00, 0x1B04),
    (0x1B34, 0x1B44),
    (0x1B6B, 0x1B73),
    // Sundanese
    (0x1B80, 0x1B82),
    (0x1BA1, 0x1BAD),
    // Batak
    (0x1BE6, 0x1BF3),
    // Lepcha
    (0x1C24, 0x1C37),
    // Vedic
    (0x1CD0, 0x1CD2),
    (0x1CD4, 0x1CE8),
    (0x1CED, 0x1CED),
    (0x1CF4, 0x1CF4),
    (0x1CF7, 0x1CF9),
    // Combining Diacritical Marks Supplement
    (0x1DC0, 0x1DFF),
    // Combining Diacritical Marks for Symbols
    (0x20D0, 0x20F0),
    // Coptic
    (0x2CEF, 0x2CF1),
    // Tifinagh
    (0x2D7F, 0x2D7F),
    // Cyrillic Extended-A
    (0x2DE0, 0x2DFF),
    // CJK ideographic combining marks
    (0x302A, 0x302F),
    // Japanese dakuten / handakuten
    (0x3099, 0x309A),
    // Cyrillic Extended-B combining marks
    (0xA66F, 0xA672),
    (0xA674, 0xA67D),
    (0xA69E, 0xA69F),
    // Bamum
    (0xA6F0, 0xA6F1),
    // Syloti Nagri
    (0xA802, 0xA802),
    (0xA806, 0xA806),
    (0xA80B, 0xA80B),
    (0xA823, 0xA827),
    (0xA82C, 0xA82C),
    // Saurashtra
    (0xA880, 0xA881),
    (0xA8B4, 0xA8C5),
    // Devanagari Extended
    (0xA8E0, 0xA8F1),
    (0xA8FF, 0xA8FF),
    // Kayah Li
    (0xA926, 0xA92D),
    // Rejang
    (0xA947, 0xA953),
    // Javanese
    (0xA980, 0xA983),
    (0xA9B3, 0xA9C0),
    (0xA9E5, 0xA9E5),
    // Cham
    (0xAA29, 0xAA36),
    (0xAA43, 0xAA43),
    (0xAA4C, 0xAA4D),
    (0xAA7B, 0xAA7D),
    // Tai Viet
    (0xAAB0, 0xAAB0),
    (0xAAB2, 0xAAB4),
    (0xAAB7, 0xAAB8),
    (0xAABE, 0xAABF),
    (0xAAC1, 0xAAC1),
    // Meetei Mayek
    (0xAAEB, 0xAAEF),
    (0xAAF5, 0xAAF6),
    (0xABE3, 0xABEA),
    (0xABEC, 0xABED),
    // Hebrew point
    (0xFB1E, 0xFB1E),
    // Combining Half Marks
    (0xFE20, 0xFE2F),
    // Phaistos Disc
    (0x101FD, 0x101FD),
    // Coptic Epact
    (0x102E0, 0x102E0),
    // Old Permic
    (0x10376, 0x1037A),
    // Kharoshthi
    (0x10A01, 0x10A03),
    (0x10A05, 0x10A06),
    (0x10A0C, 0x10A0F),
    (0x10A38, 0x10A3A),
    (0x10A3F, 0x10A3F),
    // Manichaean
    (0x10AE5, 0x10AE6),
    // Hanifi Rohingya
    (0x10D24, 0x10D27),
    // Yezidi
    (0x10EAB, 0x10EAC),
    // Arabic Extended-C
    (0x10EFD, 0x10EFF),
    // Sogdian
    (0x10F46, 0x10F50),
    // Old Uyghur
    (0x10F82, 0x10F85),
    // Brahmi
    (0x11000, 0x11002),
    (0x11038, 0x11046),
    (0x11070, 0x11070),
    (0x11073, 0x11074),
    (0x1107F, 0x11082),
    // Kaithi
    (0x110B0, 0x110BA),
    (0x110C2, 0x110C2),
    // Chakma
    (0x11100, 0x11102),
    (0x11127, 0x11134),
    (0x11145, 0x11146),
    // Mahajani
    (0x11173, 0x11173),
    // Sharada
    (0x11180, 0x11182),
    (0x111B3, 0x111C0),
    (0x111C9, 0x111CC),
    (0x111CE, 0x111CF),
    // Khojki
    (0x1122C, 0x11237),
    (0x1123E, 0x1123E),
    (0x11241, 0x11241),
    // Khudawadi
    (0x112DF, 0x112EA),
    // Grantha
    (0x11300, 0x11303),
    (0x1133B, 0x1133C),
    (0x1133E, 0x11344),
    (0x11347, 0x11348),
    (0x1134B, 0x1134D),
    (0x11357, 0x11357),
    (0x11362, 0x11363),
    (0x11366, 0x1136C),
    (0x11370, 0x11374),
    // Newa
    (0x11435, 0x11446),
    (0x1145E, 0x1145E),
    // Tirhuta
    (0x114B0, 0x114C3),
    // Siddham
    (0x115AF, 0x115B5),
    (0x115B8, 0x115C0),
    (0x115DC, 0x115DD),
    // Modi
    (0x11630, 0x11640),
    // Takri
    (0x116AB, 0x116B7),
    // Ahom
    (0x1171D, 0x1172B),
    // Dogra
    (0x1182C, 0x1183A),
    // Dives Akuru
    (0x11930, 0x11935),
    (0x11937, 0x11938),
    (0x1193B, 0x1193E),
    (0x11940, 0x11940),
    (0x11942, 0x11943),
    // Nandinagari
    (0x119D1, 0x119D7),
    (0x119DA, 0x119E0),
    (0x119E4, 0x119E4),
    // Zanabazar Square
    (0x11A01, 0x11A0A),
    (0x11A33, 0x11A39),
    (0x11A3B, 0x11A3E),
    (0x11A47, 0x11A47),
    // Soyombo
    (0x11A51, 0x11A5B),
    (0x11A8A, 0x11A99),
    // Bhaiksuki
    (0x11C2F, 0x11C36),
    (0x11C38, 0x11C3F),
    // Marchen
    (0x11C92, 0x11CA7),
    (0x11CA9, 0x11CB6),
    // Masaram Gondi
    (0x11D31, 0x11D36),
    (0x11D3A, 0x11D3A),
    (0x11D3C, 0x11D3D),
    (0x11D3F, 0x11D45),
    (0x11D47, 0x11D47),
    // Gunjala Gondi
    (0x11D8A, 0x11D8E),
    (0x11D90, 0x11D91),
    (0x11D93, 0x11D97),
    // Makasar
    (0x11EF3, 0x11EF6),
    // Kawi
    (0x11F00, 0x11F01),
    (0x11F34, 0x11F3A),
    (0x11F3E, 0x11F42),
    // Egyptian Hieroglyphs
    (0x13440, 0x13440),
    (0x13447, 0x13455),
    // Bassa Vah
    (0x16AF0, 0x16AF4),
    // Pahawh Hmong
    (0x16B30, 0x16B36),
    // Miao
    (0x16F4F, 0x16F4F),
    (0x16F8F, 0x16F92),
    // Khitan Small Script
    (0x16FE4, 0x16FE4),
    // Duployan
    (0x1BC9D, 0x1BC9E),
    // Znamenny Musical Notation
    (0x1CF00, 0x1CF2D),
    (0x1CF30, 0x1CF46),
    // Musical Symbols
    (0x1D165, 0x1D169),
    (0x1D16D, 0x1D172),
    (0x1D17B, 0x1D182),
    (0x1D185, 0x1D18B),
    (0x1D1AA, 0x1D1AD),
    // Combining Greek Musical Notation
    (0x1D242, 0x1D244),
    // Signwriting
    (0x1DA00, 0x1DA36),
    (0x1DA3B, 0x1DA6C),
    (0x1DA75, 0x1DA75),
    (0x1DA84, 0x1DA84),
    (0x1DA9B, 0x1DA9F),
    (0x1DAA1, 0x1DAAF),
    // Glagolitic Supplement
    (0x1E000, 0x1E006),
    (0x1E008, 0x1E018),
    (0x1E01B, 0x1E021),
    (0x1E023, 0x1E024),
    (0x1E026, 0x1E02A),
    // Cyrillic Extended-D
    (0x1E08F, 0x1E08F),
    // Nyiakeng Puachue Hmong
    (0x1E130, 0x1E136),
    // Wancho
    (0x1E2AE, 0x1E2AE),
    (0x1E2EC, 0x1E2EF),
    // Cypro-Minoan
    (0x1E4EC, 0x1E4EF),
    // Mende Kikakui
    (0x1E8D0, 0x1E8D6),
    // Adlam
    (0x1E944, 0x1E94A),
];

/// Check if a code point has East Asian Width = Ambiguous (Unicode 17.0).
///
/// These characters are displayed as either narrow (1 cell) or wide (2 cells)
/// depending on the context. CJK environments typically treat them as wide.
/// Uses binary search on a comprehensive table of EAW=A ranges.
pub fn is_ambiguous_width(cp: u32) -> bool {
    AMBIGUOUS_WIDTH_RANGES
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Sorted table of Unicode East Asian Width = Ambiguous ranges.
/// Each entry is (start, end) inclusive. Based on Unicode 17.0.
///
/// Source: https://www.unicode.org/Public/17.0.0/ucd/EastAsianWidth.txt
static AMBIGUOUS_WIDTH_RANGES: &[(u32, u32)] = &[
    (0x00A1, 0x00A1),   // INVERTED EXCLAMATION MARK
    (0x00A4, 0x00A4),   // CURRENCY SIGN
    (0x00A7, 0x00A8),   // SECTION SIGN..DIAERESIS
    (0x00AA, 0x00AA),   // FEMININE ORDINAL INDICATOR
    (0x00AD, 0x00AE),   // SOFT HYPHEN..REGISTERED SIGN
    (0x00B0, 0x00B4),   // DEGREE SIGN..ACUTE ACCENT
    (0x00B6, 0x00BA),   // PILCROW SIGN..MASCULINE ORDINAL INDICATOR
    (0x00BC, 0x00BF),   // VULGAR FRACTION ONE QUARTER..INVERTED QUESTION MARK
    (0x00C6, 0x00C6),   // LATIN CAPITAL LETTER AE
    (0x00D0, 0x00D0),   // LATIN CAPITAL LETTER ETH
    (0x00D7, 0x00D8),   // MULTIPLICATION SIGN..LATIN CAPITAL LETTER O WITH STROKE
    (0x00DE, 0x00E1),   // LATIN CAPITAL LETTER THORN..LATIN SMALL LETTER A WITH ACUTE
    (0x00E6, 0x00E6),   // LATIN SMALL LETTER AE
    (0x00E8, 0x00EA),   // LATIN SMALL LETTER E WITH GRAVE..LATIN SMALL LETTER E WITH CIRCUMFLEX
    (0x00EC, 0x00ED),   // LATIN SMALL LETTER I WITH GRAVE..LATIN SMALL LETTER I WITH ACUTE
    (0x00F0, 0x00F0),   // LATIN SMALL LETTER ETH
    (0x00F2, 0x00F3),   // LATIN SMALL LETTER O WITH GRAVE..LATIN SMALL LETTER O WITH ACUTE
    (0x00F7, 0x00FA),   // DIVISION SIGN..LATIN SMALL LETTER U WITH ACUTE
    (0x00FC, 0x00FC),   // LATIN SMALL LETTER U WITH DIAERESIS
    (0x00FE, 0x00FE),   // LATIN SMALL LETTER THORN
    (0x0101, 0x0101),   // LATIN SMALL LETTER A WITH MACRON
    (0x0111, 0x0111),   // LATIN SMALL LETTER D WITH STROKE
    (0x0113, 0x0113),   // LATIN SMALL LETTER E WITH MACRON
    (0x011B, 0x011B),   // LATIN SMALL LETTER E WITH CARON
    (0x0126, 0x0127),   // LATIN CAPITAL LETTER H WITH STROKE..LATIN SMALL LETTER H WITH STROKE
    (0x012B, 0x012B),   // LATIN SMALL LETTER I WITH MACRON
    (0x0131, 0x0133),   // LATIN SMALL LETTER DOTLESS I..LATIN SMALL LIGATURE IJ
    (0x0138, 0x0138),   // LATIN SMALL LETTER KRA
    (0x013F, 0x0142),   // LATIN CAPITAL LETTER L WITH MIDDLE DOT..LATIN SMALL LETTER L WITH STROKE
    (0x0144, 0x0144),   // LATIN SMALL LETTER N WITH ACUTE
    (0x0148, 0x014B),   // LATIN SMALL LETTER N WITH CARON..LATIN SMALL LETTER ENG
    (0x014D, 0x014D),   // LATIN SMALL LETTER O WITH MACRON
    (0x0152, 0x0153),   // LATIN CAPITAL LIGATURE OE..LATIN SMALL LIGATURE OE
    (0x0166, 0x0167),   // LATIN CAPITAL LETTER T WITH STROKE..LATIN SMALL LETTER T WITH STROKE
    (0x016B, 0x016B),   // LATIN SMALL LETTER U WITH MACRON
    (0x01CE, 0x01CE),   // LATIN SMALL LETTER A WITH CARON
    (0x01D0, 0x01D0),   // LATIN SMALL LETTER I WITH CARON
    (0x01D2, 0x01D2),   // LATIN SMALL LETTER O WITH CARON
    (0x01D4, 0x01D4),   // LATIN SMALL LETTER U WITH CARON
    (0x01D6, 0x01D6),   // LATIN SMALL LETTER U WITH DIAERESIS AND MACRON
    (0x01D8, 0x01D8),   // LATIN SMALL LETTER U WITH DIAERESIS AND ACUTE
    (0x01DA, 0x01DA),   // LATIN SMALL LETTER U WITH DIAERESIS AND CARON
    (0x01DC, 0x01DC),   // LATIN SMALL LETTER U WITH DIAERESIS AND GRAVE
    (0x0251, 0x0251),   // LATIN SMALL LETTER ALPHA
    (0x0261, 0x0261),   // LATIN SMALL LETTER SCRIPT G
    (0x02C4, 0x02C4),   // MODIFIER LETTER UP ARROWHEAD
    (0x02C7, 0x02C7),   // CARON
    (0x02C9, 0x02CB),   // MODIFIER LETTER MACRON..MODIFIER LETTER GRAVE ACCENT
    (0x02CD, 0x02CD),   // MODIFIER LETTER LOW MACRON
    (0x02D0, 0x02D0),   // MODIFIER LETTER TRIANGULAR COLON
    (0x02D8, 0x02DB),   // BREVE..OGONEK
    (0x02DD, 0x02DD),   // DOUBLE ACUTE ACCENT
    (0x02DF, 0x02DF),   // MODIFIER LETTER CROSS ACCENT
    (0x0300, 0x036F),   // COMBINING DIACRITICAL MARKS
    (0x0391, 0x03A1),   // GREEK CAPITAL LETTER ALPHA..GREEK CAPITAL LETTER RHO
    (0x03A3, 0x03A9),   // GREEK CAPITAL LETTER SIGMA..GREEK CAPITAL LETTER OMEGA
    (0x03B1, 0x03C1),   // GREEK SMALL LETTER ALPHA..GREEK SMALL LETTER RHO
    (0x03C3, 0x03C9),   // GREEK SMALL LETTER SIGMA..GREEK SMALL LETTER OMEGA
    (0x0401, 0x0401),   // CYRILLIC CAPITAL LETTER IO
    (0x0410, 0x044F),   // CYRILLIC CAPITAL LETTER A..CYRILLIC SMALL LETTER YA
    (0x0451, 0x0451),   // CYRILLIC SMALL LETTER IO
    (0x2010, 0x2010),   // HYPHEN
    (0x2013, 0x2016),   // EN DASH..DOUBLE VERTICAL LINE
    (0x2018, 0x2019),   // LEFT SINGLE QUOTATION MARK..RIGHT SINGLE QUOTATION MARK
    (0x201C, 0x201D),   // LEFT DOUBLE QUOTATION MARK..RIGHT DOUBLE QUOTATION MARK
    (0x2020, 0x2022),   // DAGGER..BULLET
    (0x2024, 0x2027),   // ONE DOT LEADER..HYPHENATION POINT
    (0x2030, 0x2030),   // PER MILLE SIGN
    (0x2032, 0x2033),   // PRIME..DOUBLE PRIME
    (0x2035, 0x2035),   // REVERSED PRIME
    (0x203B, 0x203B),   // REFERENCE MARK
    (0x203E, 0x203E),   // OVERLINE
    (0x2074, 0x2074),   // SUPERSCRIPT FOUR
    (0x207F, 0x207F),   // SUPERSCRIPT LATIN SMALL LETTER N
    (0x2081, 0x2084),   // SUBSCRIPT ONE..SUBSCRIPT FOUR
    (0x20AC, 0x20AC),   // EURO SIGN
    (0x2103, 0x2103),   // DEGREE CELSIUS
    (0x2105, 0x2105),   // CARE OF
    (0x2109, 0x2109),   // DEGREE FAHRENHEIT
    (0x2113, 0x2113),   // SCRIPT SMALL L
    (0x2116, 0x2116),   // NUMERO SIGN
    (0x2121, 0x2122),   // TELEPHONE SIGN..TRADE MARK SIGN
    (0x2126, 0x2126),   // OHM SIGN
    (0x212B, 0x212B),   // ANGSTROM SIGN
    (0x2153, 0x2154),   // VULGAR FRACTION ONE THIRD..VULGAR FRACTION TWO THIRDS
    (0x215B, 0x215E),   // VULGAR FRACTION ONE EIGHTH..VULGAR FRACTION SEVEN EIGHTHS
    (0x2160, 0x216B),   // ROMAN NUMERAL ONE..ROMAN NUMERAL TWELVE
    (0x2170, 0x2179),   // SMALL ROMAN NUMERAL ONE..SMALL ROMAN NUMERAL TEN
    (0x2189, 0x2189),   // VULGAR FRACTION ZERO THIRDS
    (0x2190, 0x2199),   // LEFTWARDS ARROW..SOUTH WEST ARROW
    (0x21B8, 0x21B9),   // NORTH WEST ARROW TO LONG BAR..LEFTWARDS ARROW TO BAR OVER RIGHTWARDS ARROW TO BAR
    (0x21D2, 0x21D2),   // RIGHTWARDS DOUBLE ARROW
    (0x21D4, 0x21D4),   // LEFT RIGHT DOUBLE ARROW
    (0x21E7, 0x21E7),   // UPWARDS WHITE ARROW
    (0x2200, 0x2200),   // FOR ALL
    (0x2202, 0x2203),   // PARTIAL DIFFERENTIAL..THERE EXISTS
    (0x2207, 0x2208),   // NABLA..ELEMENT OF
    (0x220B, 0x220B),   // CONTAINS AS MEMBER
    (0x220F, 0x220F),   // N-ARY PRODUCT
    (0x2211, 0x2211),   // N-ARY SUMMATION
    (0x2215, 0x2215),   // DIVISION SLASH
    (0x221A, 0x221A),   // SQUARE ROOT
    (0x221D, 0x2220),   // PROPORTIONAL TO..ANGLE
    (0x2223, 0x2223),   // DIVIDES
    (0x2225, 0x2225),   // PARALLEL TO
    (0x2227, 0x222C),   // LOGICAL AND..DOUBLE INTEGRAL
    (0x222E, 0x222E),   // CONTOUR INTEGRAL
    (0x2234, 0x2237),   // THEREFORE..PROPORTION
    (0x223C, 0x223D),   // TILDE OPERATOR..REVERSED TILDE
    (0x2248, 0x2248),   // ALMOST EQUAL TO
    (0x224C, 0x224C),   // ALL EQUAL TO
    (0x2252, 0x2252),   // APPROXIMATELY EQUAL TO OR THE IMAGE OF
    (0x2260, 0x2261),   // NOT EQUAL TO..IDENTICAL TO
    (0x2264, 0x2267),   // LESS-THAN OR EQUAL TO..GREATER-THAN OVER EQUAL TO
    (0x226A, 0x226B),   // MUCH LESS-THAN..MUCH GREATER-THAN
    (0x226E, 0x226F),   // NOT LESS-THAN..NOT GREATER-THAN
    (0x2282, 0x2283),   // SUBSET OF..SUPERSET OF
    (0x2286, 0x2287),   // SUBSET OF OR EQUAL TO..SUPERSET OF OR EQUAL TO
    (0x2295, 0x2295),   // CIRCLED PLUS
    (0x2299, 0x2299),   // CIRCLED DOT OPERATOR
    (0x22A5, 0x22A5),   // UP TACK
    (0x22BF, 0x22BF),   // RIGHT TRIANGLE
    (0x2312, 0x2312),   // ARC
    (0x2460, 0x24E9),   // CIRCLED DIGIT ONE..CIRCLED LATIN SMALL LETTER Z
    (0x24EB, 0x254B),   // NEGATIVE CIRCLED NUMBER ELEVEN..BOX DRAWINGS HEAVY VERTICAL AND HORIZONTAL
    (0x2550, 0x2573),   // BOX DRAWINGS DOUBLE HORIZONTAL..BOX DRAWINGS LIGHT DIAGONAL CROSS
    (0x2580, 0x258F),   // UPPER HALF BLOCK..LEFT ONE EIGHTH BLOCK
    (0x2592, 0x2595),   // MEDIUM SHADE..RIGHT ONE EIGHTH BLOCK
    (0x25A0, 0x25A1),   // BLACK SQUARE..WHITE SQUARE
    (0x25A3, 0x25A9),   // WHITE SQUARE CONTAINING BLACK SMALL SQUARE..SQUARE WITH DIAGONAL CROSSHATCH FILL
    (0x25B2, 0x25B3),   // BLACK UP-POINTING TRIANGLE..WHITE UP-POINTING TRIANGLE
    (0x25B6, 0x25B7),   // BLACK RIGHT-POINTING TRIANGLE..WHITE RIGHT-POINTING TRIANGLE
    (0x25BC, 0x25BD),   // BLACK DOWN-POINTING TRIANGLE..WHITE DOWN-POINTING TRIANGLE
    (0x25C0, 0x25C1),   // BLACK LEFT-POINTING TRIANGLE..WHITE LEFT-POINTING TRIANGLE
    (0x25C6, 0x25C8),   // BLACK DIAMOND..WHITE DIAMOND CONTAINING BLACK SMALL DIAMOND
    (0x25CB, 0x25CB),   // WHITE CIRCLE
    (0x25CE, 0x25D1),   // BULLSEYE..CIRCLE WITH RIGHT HALF BLACK
    (0x25E2, 0x25E5),   // BLACK LOWER RIGHT TRIANGLE..BLACK UPPER RIGHT TRIANGLE
    (0x25EF, 0x25EF),   // LARGE CIRCLE
    (0x2605, 0x2606),   // BLACK STAR..WHITE STAR
    (0x2609, 0x2609),   // SUN
    (0x260E, 0x260F),   // BLACK TELEPHONE..WHITE TELEPHONE
    (0x261C, 0x261C),   // WHITE LEFT POINTING INDEX
    (0x261E, 0x261E),   // WHITE RIGHT POINTING INDEX
    (0x2640, 0x2640),   // FEMALE SIGN
    (0x2642, 0x2642),   // MALE SIGN
    (0x2660, 0x2661),   // BLACK SPADE SUIT..WHITE HEART SUIT
    (0x2663, 0x2665),   // BLACK CLUB SUIT..BLACK HEART SUIT
    (0x2667, 0x266A),   // WHITE CLUB SUIT..EIGHTH NOTE
    (0x266C, 0x266D),   // BEAMED SIXTEENTH NOTES..MUSIC FLAT SIGN
    (0x266F, 0x266F),   // MUSIC SHARP SIGN
    (0x269E, 0x269F),   // THREE LINES CONVERGING RIGHT..THREE LINES CONVERGING LEFT
    (0x26BF, 0x26BF),   // SQUARED KEY
    (0x26C6, 0x26CD),   // RAIN..DISABLED CAR
    (0x26CF, 0x26D3),   // PICK..CHAINS
    (0x26D5, 0x26E1),   // ALTERNATE ONE-WAY LEFT WAY TRAFFIC..RESTRICTED LEFT ENTRY-2
    (0x26E3, 0x26E3),   // HEAVY CIRCLE WITH STROKE AND TWO DOTS ABOVE
    (0x26E8, 0x26E9),   // BLACK CROSS ON SHIELD..SHINTO SHRINE
    (0x26EB, 0x26F1),   // CASTLE..UMBRELLA ON GROUND
    (0x26F4, 0x26F4),   // FERRY
    (0x26F6, 0x26F9),   // SQUARE FOUR CORNERS..PERSON WITH BALL
    (0x26FB, 0x26FC),   // JAPANESE BANK SYMBOL..HEADSTONE GRAVEYARD SYMBOL
    (0x26FE, 0x26FF),   // CUP ON BLACK SQUARE..WHITE FLAG WITH HORIZONTAL MIDDLE BLACK STRIPE
    (0x273D, 0x273D),   // HEAVY TEARDROP-SPOKED ASTERISK
    (0x2776, 0x277F),   // DINGBAT NEGATIVE CIRCLED DIGIT ONE..DINGBAT NEGATIVE CIRCLED NUMBER TEN
    (0x2B56, 0x2B59),   // HEAVY OVAL WITH OVAL INSIDE..HEAVY CIRCLED SALTIRE
    (0xE000, 0xF8FF),   // Private Use Area
    (0xFE00, 0xFE0F),   // VARIATION SELECTORS
    (0xFFFD, 0xFFFD),   // REPLACEMENT CHARACTER
    (0x1F100, 0x1F10A), // DIGIT ZERO FULL STOP..DIGIT NINE COMMA
    (0x1F110, 0x1F12D), // PARENTHESIZED LATIN CAPITAL LETTER A..CIRCLED CD
    (0x1F130, 0x1F169), // SQUARED LATIN CAPITAL LETTER A..NEGATIVE CIRCLED LATIN CAPITAL LETTER Z
    (0x1F170, 0x1F18D), // NEGATIVE SQUARED LATIN CAPITAL LETTER A..NEGATIVE SQUARED SA
    (0x1F18F, 0x1F190), // NEGATIVE SQUARED WC..SQUARE DJ
    (0x1F19B, 0x1F1AC), // SQUARED THREE D..SQUARED VOD
    (0xF0000, 0xFFFFD), // Supplementary Private Use Area-A
    (0x100000, 0x10FFFD), // Supplementary Private Use Area-B
];

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

    #[test]
    fn combining_char_arabic() {
        // Arabic combining marks used by Kitty Unicode placeholders
        assert!(is_combining_char(0x0610)); // Arabic sign sallallahou alayhe wassallam
        assert!(is_combining_char(0x0615)); // Arabic small high tah
        assert!(is_combining_char(0x061A)); // Arabic small kasra
        assert!(is_combining_char(0x064B)); // Arabic fathatan
        assert!(is_combining_char(0x0651)); // Arabic shadda
        assert!(is_combining_char(0x065F)); // Arabic wavy hamza below
        assert!(is_combining_char(0x0670)); // Arabic letter superscript alef
    }

    #[test]
    fn combining_char_hebrew_cyrillic() {
        // Hebrew accents
        assert!(is_combining_char(0x0591)); // Hebrew accent etnahta
        assert!(is_combining_char(0x05BD)); // Hebrew point meteg
        assert!(is_combining_char(0x05C4)); // Hebrew mark upper dot
        // Cyrillic
        assert!(is_combining_char(0x0483)); // Cyrillic combining titlo
    }

    #[test]
    fn combining_char_syriac_nko() {
        // Syriac
        assert!(is_combining_char(0x0711)); // Syriac letter superscript alaph
        assert!(is_combining_char(0x0730)); // Syriac pthaha above
        assert!(is_combining_char(0x074A)); // Syriac barrekh
        // NKo
        assert!(is_combining_char(0x07EB)); // NKo combining short high tone
        assert!(is_combining_char(0x07F3)); // NKo combining double dot above
    }

    #[test]
    fn combining_char_indic() {
        // Devanagari
        assert!(is_combining_char(0x093C)); // Devanagari sign nukta
        assert!(is_combining_char(0x094D)); // Devanagari sign virama
        assert!(is_combining_char(0x0951)); // Devanagari stress sign udatta
    }

    #[test]
    fn combining_char_cjk_japanese() {
        // CJK ideographic combining marks
        assert!(is_combining_char(0x302A)); // Ideographic level tone mark
        assert!(is_combining_char(0x302F)); // Hangul double dot tone mark
        // Japanese dakuten / handakuten
        assert!(is_combining_char(0x3099)); // Combining katakana-hiragana voiced sound mark
        assert!(is_combining_char(0x309A)); // Combining katakana-hiragana semi-voiced sound mark
    }

    #[test]
    fn combining_char_width_zero() {
        // Arabic combining marks should have width 0
        assert_eq!(char_width(0x0651), 0); // Arabic shadda
        assert_eq!(char_width(0x0615), 0); // Arabic small high tah
        assert_eq!(char_width(0x064B), 0); // Arabic fathatan
        // Hebrew
        assert_eq!(char_width(0x0591), 0); // Hebrew accent etnahta
        // Devanagari
        assert_eq!(char_width(0x094D), 0); // Devanagari sign virama
    }

    #[test]
    fn combining_ranges_table_sorted() {
        // Verify the COMBINING_RANGES table is properly sorted
        for window in COMBINING_RANGES.windows(2) {
            assert!(
                window[0].1 < window[1].0,
                "COMBINING_RANGES not sorted or overlapping: ({:#X}, {:#X}) and ({:#X}, {:#X})",
                window[0].0, window[0].1, window[1].0, window[1].1
            );
        }
    }

    // ── is_ambiguous_width ──────────────────────────────────

    #[test]
    fn ambiguous_width_true() {
        assert!(is_ambiguous_width(0x25A0)); // BLACK SQUARE ■
        assert!(is_ambiguous_width(0x00A1)); // INVERTED EXCLAMATION MARK
        assert!(is_ambiguous_width(0x00A4)); // CURRENCY SIGN
        assert!(is_ambiguous_width(0x2605)); // BLACK STAR ★
        assert!(is_ambiguous_width(0x2660)); // BLACK SPADE SUIT ♠
        assert!(is_ambiguous_width(0x00D7)); // MULTIPLICATION SIGN ×
        assert!(is_ambiguous_width(0x00F7)); // DIVISION SIGN ÷
        assert!(is_ambiguous_width(0x2190)); // LEFTWARDS ARROW ←
        assert!(is_ambiguous_width(0xFFFD)); // REPLACEMENT CHARACTER
    }

    #[test]
    fn ambiguous_width_false() {
        assert!(!is_ambiguous_width(0x41));   // 'A' (narrow)
        assert!(!is_ambiguous_width(0x4E00)); // CJK unified ideograph (wide)
        assert!(!is_ambiguous_width(0x20));   // Space
        assert!(!is_ambiguous_width(0x3042)); // Hiragana 'a' (wide, not ambiguous)
    }

    #[test]
    fn ambiguous_width_boundaries() {
        // First entry
        assert!(is_ambiguous_width(0x00A1));
        assert!(!is_ambiguous_width(0x00A0));
        // Last entry
        assert!(is_ambiguous_width(0x10FFFD));
        assert!(!is_ambiguous_width(0x10FFFE));
        // BLACK SQUARE range
        assert!(is_ambiguous_width(0x25A0));
        assert!(is_ambiguous_width(0x25A1));
        assert!(!is_ambiguous_width(0x25A2)); // gap before 0x25A3
    }

    #[test]
    fn ambiguous_ranges_table_sorted() {
        for window in AMBIGUOUS_WIDTH_RANGES.windows(2) {
            assert!(
                window[0].1 < window[1].0,
                "AMBIGUOUS_WIDTH_RANGES not sorted or overlapping: ({:#X}, {:#X}) and ({:#X}, {:#X})",
                window[0].0, window[0].1, window[1].0, window[1].1
            );
        }
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
