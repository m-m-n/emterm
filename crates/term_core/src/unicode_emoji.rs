// Unicode emoji property lookup functions.
//
// Based on Unicode 17.0 / Emoji 17.0 (2025-09-09)

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
