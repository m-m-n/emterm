// Unicode character width calculation.
//
// Based on Unicode 17.0 / Emoji 17.0 (2025-09-09)
// Port of src/terminal/unicode.ts

pub use crate::unicode_emoji::{
    is_emoji_presentation, is_extended_pictographic, is_regional_indicator, is_skin_tone_modifier,
    is_variation_selector,
};
pub use crate::unicode_width::{
    is_ambiguous_width, is_combining_char, is_wide_code_point, is_zero_width,
};

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
    use crate::unicode_width::{AMBIGUOUS_WIDTH_RANGES, COMBINING_RANGES};

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
                window[0].0,
                window[0].1,
                window[1].0,
                window[1].1
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
        assert!(!is_ambiguous_width(0x41)); // 'A' (narrow)
        assert!(!is_ambiguous_width(0x4E00)); // CJK unified ideograph (wide)
        assert!(!is_ambiguous_width(0x20)); // Space
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
                window[0].0,
                window[0].1,
                window[1].0,
                window[1].1
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
