// Unicode width and combining character lookup functions.
//
// Based on Unicode 17.0 (2025-09-09)

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
pub(crate) static COMBINING_RANGES: &[(u32, u32)] = &[
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
pub(crate) static AMBIGUOUS_WIDTH_RANGES: &[(u32, u32)] = &[
    (0x00A1, 0x00A1),     // INVERTED EXCLAMATION MARK
    (0x00A4, 0x00A4),     // CURRENCY SIGN
    (0x00A7, 0x00A8),     // SECTION SIGN..DIAERESIS
    (0x00AA, 0x00AA),     // FEMININE ORDINAL INDICATOR
    (0x00AD, 0x00AE),     // SOFT HYPHEN..REGISTERED SIGN
    (0x00B0, 0x00B4),     // DEGREE SIGN..ACUTE ACCENT
    (0x00B6, 0x00BA),     // PILCROW SIGN..MASCULINE ORDINAL INDICATOR
    (0x00BC, 0x00BF),     // VULGAR FRACTION ONE QUARTER..INVERTED QUESTION MARK
    (0x00C6, 0x00C6),     // LATIN CAPITAL LETTER AE
    (0x00D0, 0x00D0),     // LATIN CAPITAL LETTER ETH
    (0x00D7, 0x00D8),     // MULTIPLICATION SIGN..LATIN CAPITAL LETTER O WITH STROKE
    (0x00DE, 0x00E1),     // LATIN CAPITAL LETTER THORN..LATIN SMALL LETTER A WITH ACUTE
    (0x00E6, 0x00E6),     // LATIN SMALL LETTER AE
    (0x00E8, 0x00EA),     // LATIN SMALL LETTER E WITH GRAVE..LATIN SMALL LETTER E WITH CIRCUMFLEX
    (0x00EC, 0x00ED),     // LATIN SMALL LETTER I WITH GRAVE..LATIN SMALL LETTER I WITH ACUTE
    (0x00F0, 0x00F0),     // LATIN SMALL LETTER ETH
    (0x00F2, 0x00F3),     // LATIN SMALL LETTER O WITH GRAVE..LATIN SMALL LETTER O WITH ACUTE
    (0x00F7, 0x00FA),     // DIVISION SIGN..LATIN SMALL LETTER U WITH ACUTE
    (0x00FC, 0x00FC),     // LATIN SMALL LETTER U WITH DIAERESIS
    (0x00FE, 0x00FE),     // LATIN SMALL LETTER THORN
    (0x0101, 0x0101),     // LATIN SMALL LETTER A WITH MACRON
    (0x0111, 0x0111),     // LATIN SMALL LETTER D WITH STROKE
    (0x0113, 0x0113),     // LATIN SMALL LETTER E WITH MACRON
    (0x011B, 0x011B),     // LATIN SMALL LETTER E WITH CARON
    (0x0126, 0x0127),     // LATIN CAPITAL LETTER H WITH STROKE..LATIN SMALL LETTER H WITH STROKE
    (0x012B, 0x012B),     // LATIN SMALL LETTER I WITH MACRON
    (0x0131, 0x0133),     // LATIN SMALL LETTER DOTLESS I..LATIN SMALL LIGATURE IJ
    (0x0138, 0x0138),     // LATIN SMALL LETTER KRA
    (0x013F, 0x0142), // LATIN CAPITAL LETTER L WITH MIDDLE DOT..LATIN SMALL LETTER L WITH STROKE
    (0x0144, 0x0144), // LATIN SMALL LETTER N WITH ACUTE
    (0x0148, 0x014B), // LATIN SMALL LETTER N WITH CARON..LATIN SMALL LETTER ENG
    (0x014D, 0x014D), // LATIN SMALL LETTER O WITH MACRON
    (0x0152, 0x0153), // LATIN CAPITAL LIGATURE OE..LATIN SMALL LIGATURE OE
    (0x0166, 0x0167), // LATIN CAPITAL LETTER T WITH STROKE..LATIN SMALL LETTER T WITH STROKE
    (0x016B, 0x016B), // LATIN SMALL LETTER U WITH MACRON
    (0x01CE, 0x01CE), // LATIN SMALL LETTER A WITH CARON
    (0x01D0, 0x01D0), // LATIN SMALL LETTER I WITH CARON
    (0x01D2, 0x01D2), // LATIN SMALL LETTER O WITH CARON
    (0x01D4, 0x01D4), // LATIN SMALL LETTER U WITH CARON
    (0x01D6, 0x01D6), // LATIN SMALL LETTER U WITH DIAERESIS AND MACRON
    (0x01D8, 0x01D8), // LATIN SMALL LETTER U WITH DIAERESIS AND ACUTE
    (0x01DA, 0x01DA), // LATIN SMALL LETTER U WITH DIAERESIS AND CARON
    (0x01DC, 0x01DC), // LATIN SMALL LETTER U WITH DIAERESIS AND GRAVE
    (0x0251, 0x0251), // LATIN SMALL LETTER ALPHA
    (0x0261, 0x0261), // LATIN SMALL LETTER SCRIPT G
    (0x02C4, 0x02C4), // MODIFIER LETTER UP ARROWHEAD
    (0x02C7, 0x02C7), // CARON
    (0x02C9, 0x02CB), // MODIFIER LETTER MACRON..MODIFIER LETTER GRAVE ACCENT
    (0x02CD, 0x02CD), // MODIFIER LETTER LOW MACRON
    (0x02D0, 0x02D0), // MODIFIER LETTER TRIANGULAR COLON
    (0x02D8, 0x02DB), // BREVE..OGONEK
    (0x02DD, 0x02DD), // DOUBLE ACUTE ACCENT
    (0x02DF, 0x02DF), // MODIFIER LETTER CROSS ACCENT
    (0x0300, 0x036F), // COMBINING DIACRITICAL MARKS
    (0x0391, 0x03A1), // GREEK CAPITAL LETTER ALPHA..GREEK CAPITAL LETTER RHO
    (0x03A3, 0x03A9), // GREEK CAPITAL LETTER SIGMA..GREEK CAPITAL LETTER OMEGA
    (0x03B1, 0x03C1), // GREEK SMALL LETTER ALPHA..GREEK SMALL LETTER RHO
    (0x03C3, 0x03C9), // GREEK SMALL LETTER SIGMA..GREEK SMALL LETTER OMEGA
    (0x0401, 0x0401), // CYRILLIC CAPITAL LETTER IO
    (0x0410, 0x044F), // CYRILLIC CAPITAL LETTER A..CYRILLIC SMALL LETTER YA
    (0x0451, 0x0451), // CYRILLIC SMALL LETTER IO
    (0x2010, 0x2010), // HYPHEN
    (0x2013, 0x2016), // EN DASH..DOUBLE VERTICAL LINE
    (0x2018, 0x2019), // LEFT SINGLE QUOTATION MARK..RIGHT SINGLE QUOTATION MARK
    (0x201C, 0x201D), // LEFT DOUBLE QUOTATION MARK..RIGHT DOUBLE QUOTATION MARK
    (0x2020, 0x2022), // DAGGER..BULLET
    (0x2024, 0x2027), // ONE DOT LEADER..HYPHENATION POINT
    (0x2030, 0x2030), // PER MILLE SIGN
    (0x2032, 0x2033), // PRIME..DOUBLE PRIME
    (0x2035, 0x2035), // REVERSED PRIME
    (0x203B, 0x203B), // REFERENCE MARK
    (0x203E, 0x203E), // OVERLINE
    (0x2074, 0x2074), // SUPERSCRIPT FOUR
    (0x207F, 0x207F), // SUPERSCRIPT LATIN SMALL LETTER N
    (0x2081, 0x2084), // SUBSCRIPT ONE..SUBSCRIPT FOUR
    (0x20AC, 0x20AC), // EURO SIGN
    (0x2103, 0x2103), // DEGREE CELSIUS
    (0x2105, 0x2105), // CARE OF
    (0x2109, 0x2109), // DEGREE FAHRENHEIT
    (0x2113, 0x2113), // SCRIPT SMALL L
    (0x2116, 0x2116), // NUMERO SIGN
    (0x2121, 0x2122), // TELEPHONE SIGN..TRADE MARK SIGN
    (0x2126, 0x2126), // OHM SIGN
    (0x212B, 0x212B), // ANGSTROM SIGN
    (0x2153, 0x2154), // VULGAR FRACTION ONE THIRD..VULGAR FRACTION TWO THIRDS
    (0x215B, 0x215E), // VULGAR FRACTION ONE EIGHTH..VULGAR FRACTION SEVEN EIGHTHS
    (0x2160, 0x216B), // ROMAN NUMERAL ONE..ROMAN NUMERAL TWELVE
    (0x2170, 0x2179), // SMALL ROMAN NUMERAL ONE..SMALL ROMAN NUMERAL TEN
    (0x2189, 0x2189), // VULGAR FRACTION ZERO THIRDS
    (0x2190, 0x2199), // LEFTWARDS ARROW..SOUTH WEST ARROW
    (0x21B8, 0x21B9), // NORTH WEST ARROW TO LONG BAR..LEFTWARDS ARROW TO BAR OVER RIGHTWARDS ARROW TO BAR
    (0x21D2, 0x21D2), // RIGHTWARDS DOUBLE ARROW
    (0x21D4, 0x21D4), // LEFT RIGHT DOUBLE ARROW
    (0x21E7, 0x21E7), // UPWARDS WHITE ARROW
    (0x2200, 0x2200), // FOR ALL
    (0x2202, 0x2203), // PARTIAL DIFFERENTIAL..THERE EXISTS
    (0x2207, 0x2208), // NABLA..ELEMENT OF
    (0x220B, 0x220B), // CONTAINS AS MEMBER
    (0x220F, 0x220F), // N-ARY PRODUCT
    (0x2211, 0x2211), // N-ARY SUMMATION
    (0x2215, 0x2215), // DIVISION SLASH
    (0x221A, 0x221A), // SQUARE ROOT
    (0x221D, 0x2220), // PROPORTIONAL TO..ANGLE
    (0x2223, 0x2223), // DIVIDES
    (0x2225, 0x2225), // PARALLEL TO
    (0x2227, 0x222C), // LOGICAL AND..DOUBLE INTEGRAL
    (0x222E, 0x222E), // CONTOUR INTEGRAL
    (0x2234, 0x2237), // THEREFORE..PROPORTION
    (0x223C, 0x223D), // TILDE OPERATOR..REVERSED TILDE
    (0x2248, 0x2248), // ALMOST EQUAL TO
    (0x224C, 0x224C), // ALL EQUAL TO
    (0x2252, 0x2252), // APPROXIMATELY EQUAL TO OR THE IMAGE OF
    (0x2260, 0x2261), // NOT EQUAL TO..IDENTICAL TO
    (0x2264, 0x2267), // LESS-THAN OR EQUAL TO..GREATER-THAN OVER EQUAL TO
    (0x226A, 0x226B), // MUCH LESS-THAN..MUCH GREATER-THAN
    (0x226E, 0x226F), // NOT LESS-THAN..NOT GREATER-THAN
    (0x2282, 0x2283), // SUBSET OF..SUPERSET OF
    (0x2286, 0x2287), // SUBSET OF OR EQUAL TO..SUPERSET OF OR EQUAL TO
    (0x2295, 0x2295), // CIRCLED PLUS
    (0x2299, 0x2299), // CIRCLED DOT OPERATOR
    (0x22A5, 0x22A5), // UP TACK
    (0x22BF, 0x22BF), // RIGHT TRIANGLE
    (0x2312, 0x2312), // ARC
    (0x2460, 0x24E9), // CIRCLED DIGIT ONE..CIRCLED LATIN SMALL LETTER Z
    (0x24EB, 0x254B), // NEGATIVE CIRCLED NUMBER ELEVEN..BOX DRAWINGS HEAVY VERTICAL AND HORIZONTAL
    (0x2550, 0x2573), // BOX DRAWINGS DOUBLE HORIZONTAL..BOX DRAWINGS LIGHT DIAGONAL CROSS
    (0x2580, 0x258F), // UPPER HALF BLOCK..LEFT ONE EIGHTH BLOCK
    (0x2592, 0x2595), // MEDIUM SHADE..RIGHT ONE EIGHTH BLOCK
    (0x25A0, 0x25A1), // BLACK SQUARE..WHITE SQUARE
    (0x25A3, 0x25A9), // WHITE SQUARE CONTAINING BLACK SMALL SQUARE..SQUARE WITH DIAGONAL CROSSHATCH FILL
    (0x25B2, 0x25B3), // BLACK UP-POINTING TRIANGLE..WHITE UP-POINTING TRIANGLE
    (0x25B6, 0x25B7), // BLACK RIGHT-POINTING TRIANGLE..WHITE RIGHT-POINTING TRIANGLE
    (0x25BC, 0x25BD), // BLACK DOWN-POINTING TRIANGLE..WHITE DOWN-POINTING TRIANGLE
    (0x25C0, 0x25C1), // BLACK LEFT-POINTING TRIANGLE..WHITE LEFT-POINTING TRIANGLE
    (0x25C6, 0x25C8), // BLACK DIAMOND..WHITE DIAMOND CONTAINING BLACK SMALL DIAMOND
    (0x25CB, 0x25CB), // WHITE CIRCLE
    (0x25CE, 0x25D1), // BULLSEYE..CIRCLE WITH RIGHT HALF BLACK
    (0x25E2, 0x25E5), // BLACK LOWER RIGHT TRIANGLE..BLACK UPPER RIGHT TRIANGLE
    (0x25EF, 0x25EF), // LARGE CIRCLE
    (0x2605, 0x2606), // BLACK STAR..WHITE STAR
    (0x2609, 0x2609), // SUN
    (0x260E, 0x260F), // BLACK TELEPHONE..WHITE TELEPHONE
    (0x261C, 0x261C), // WHITE LEFT POINTING INDEX
    (0x261E, 0x261E), // WHITE RIGHT POINTING INDEX
    (0x2640, 0x2640), // FEMALE SIGN
    (0x2642, 0x2642), // MALE SIGN
    (0x2660, 0x2661), // BLACK SPADE SUIT..WHITE HEART SUIT
    (0x2663, 0x2665), // BLACK CLUB SUIT..BLACK HEART SUIT
    (0x2667, 0x266A), // WHITE CLUB SUIT..EIGHTH NOTE
    (0x266C, 0x266D), // BEAMED SIXTEENTH NOTES..MUSIC FLAT SIGN
    (0x266F, 0x266F), // MUSIC SHARP SIGN
    (0x269E, 0x269F), // THREE LINES CONVERGING RIGHT..THREE LINES CONVERGING LEFT
    (0x26BF, 0x26BF), // SQUARED KEY
    (0x26C6, 0x26CD), // RAIN..DISABLED CAR
    (0x26CF, 0x26D3), // PICK..CHAINS
    (0x26D5, 0x26E1), // ALTERNATE ONE-WAY LEFT WAY TRAFFIC..RESTRICTED LEFT ENTRY-2
    (0x26E3, 0x26E3), // HEAVY CIRCLE WITH STROKE AND TWO DOTS ABOVE
    (0x26E8, 0x26E9), // BLACK CROSS ON SHIELD..SHINTO SHRINE
    (0x26EB, 0x26F1), // CASTLE..UMBRELLA ON GROUND
    (0x26F4, 0x26F4), // FERRY
    (0x26F6, 0x26F9), // SQUARE FOUR CORNERS..PERSON WITH BALL
    (0x26FB, 0x26FC), // JAPANESE BANK SYMBOL..HEADSTONE GRAVEYARD SYMBOL
    (0x26FE, 0x26FF), // CUP ON BLACK SQUARE..WHITE FLAG WITH HORIZONTAL MIDDLE BLACK STRIPE
    (0x273D, 0x273D), // HEAVY TEARDROP-SPOKED ASTERISK
    (0x2776, 0x277F), // DINGBAT NEGATIVE CIRCLED DIGIT ONE..DINGBAT NEGATIVE CIRCLED NUMBER TEN
    (0x2B56, 0x2B59), // HEAVY OVAL WITH OVAL INSIDE..HEAVY CIRCLED SALTIRE
    (0xE000, 0xF8FF), // Private Use Area
    (0xFE00, 0xFE0F), // VARIATION SELECTORS
    (0xFFFD, 0xFFFD), // REPLACEMENT CHARACTER
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
