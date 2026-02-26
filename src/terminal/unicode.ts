/**
 * Unicode character width calculation.
 *
 * Based on Unicode 17.0 / Emoji 17.0 (2025-09-09)
 * Emoji_Presentation data from: https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt
 *
 * To update for a new Unicode version:
 * 1. Download the new emoji-data.txt
 * 2. Extract Emoji_Presentation=Yes entries
 * 3. Update isEmojiPresentation() ranges
 * 4. Update this version comment
 */

/**
 * Get the display width of a character in terminal cells.
 *
 * @param char - A single character (or first character of string)
 * @returns 0 for control/combining/zero-width chars, 1 for narrow, 2 for wide/emoji
 */
export function charWidth(char: string): number {
	if (char.length === 0) {
		return 0;
	}

	const codePoint = char.codePointAt(0);
	if (codePoint === undefined) {
		return 0;
	}

	// Fast path: ASCII printable characters (0x20-0x7E) - most common case
	if (codePoint >= 0x20 && codePoint < 0x7f) {
		return 1;
	}

	// C0 control characters (0x00-0x1F)
	if (codePoint <= 0x1f) {
		return 0;
	}

	// DEL and C1 control characters (0x7F-0x9F)
	if (codePoint >= 0x7f && codePoint <= 0x9f) {
		return 0;
	}

	// Zero-width characters (must come before Emoji and Latin-1 ranges)
	if (isZeroWidth(codePoint)) {
		return 0;
	}

	// Emoji_Presentation=Yes characters (must come before Latin-1 range to catch BMP emojis)
	if (isEmojiPresentation(codePoint)) {
		return 2;
	}

	// Latin-1 Supplement and common Latin Extended (0xA0-0x2DFF) - narrow
	// Covers Latin, Greek, Cyrillic, and most European scripts
	if (codePoint >= 0xa0 && codePoint < 0x2e00) {
		// Check combining characters in this range
		if (isCombiningChar(codePoint)) {
			return 0;
		}
		return 1;
	}

	// Wide characters (East Asian Width: F, W)
	if (isWideCodePoint(codePoint)) {
		return 2;
	}

	// Combining characters (various ranges)
	if (isCombiningChar(codePoint)) {
		return 0;
	}

	return 1;
}

/**
 * Check if a character is wide (takes 2 cells).
 *
 * @param char - A single character (or first character of string)
 * @returns true if the character is wide
 */
export function isWideChar(char: string): boolean {
	return charWidth(char) === 2;
}

/**
 * Check if a code point has Emoji_Presentation=Yes property (Unicode 17.0).
 *
 * These characters default to emoji (colorful) presentation and are width 2.
 */
export function isEmojiPresentation(cp: number): boolean {
	// BMP ranges
	if (cp === 0x231a || cp === 0x231b) return true;
	if (cp >= 0x23e9 && cp <= 0x23ec) return true;
	if (cp === 0x23f0) return true;
	if (cp === 0x23f3) return true;
	if (cp === 0x25fd || cp === 0x25fe) return true;
	if (cp === 0x2614 || cp === 0x2615) return true;
	if (cp >= 0x2648 && cp <= 0x2653) return true;
	if (cp === 0x267f) return true;
	if (cp === 0x2693) return true;
	if (cp === 0x26a1) return true;
	if (cp === 0x26aa || cp === 0x26ab) return true;
	if (cp === 0x26bd || cp === 0x26be) return true;
	if (cp === 0x26c4 || cp === 0x26c5) return true;
	if (cp === 0x26ce) return true;
	if (cp === 0x26d4) return true;
	if (cp === 0x26ea) return true;
	if (cp === 0x26f2 || cp === 0x26f3) return true;
	if (cp === 0x26f5) return true;
	if (cp === 0x26fa) return true;
	if (cp === 0x26fd) return true;
	if (cp === 0x2705) return true;
	if (cp === 0x270a || cp === 0x270b) return true;
	if (cp === 0x2728) return true;
	if (cp === 0x274c) return true;
	if (cp === 0x274e) return true;
	if (cp >= 0x2753 && cp <= 0x2755) return true;
	if (cp === 0x2757) return true;
	if (cp >= 0x2795 && cp <= 0x2797) return true;
	if (cp === 0x27b0) return true;
	if (cp === 0x27bf) return true;
	if (cp === 0x2b1b || cp === 0x2b1c) return true;
	if (cp === 0x2b50) return true;
	if (cp === 0x2b55) return true;

	// SMP ranges (U+1F000+)
	if (cp === 0x1f004) return true;
	if (cp === 0x1f0cf) return true;
	if (cp === 0x1f18e) return true;
	if (cp >= 0x1f191 && cp <= 0x1f19a) return true;
	if (cp >= 0x1f1e6 && cp <= 0x1f1ff) return true;
	if (cp === 0x1f201) return true;
	if (cp === 0x1f21a) return true;
	if (cp === 0x1f22f) return true;
	if (cp >= 0x1f232 && cp <= 0x1f236) return true;
	if (cp >= 0x1f238 && cp <= 0x1f23a) return true;
	if (cp === 0x1f250 || cp === 0x1f251) return true;
	if (cp >= 0x1f300 && cp <= 0x1f320) return true;
	if (cp >= 0x1f32d && cp <= 0x1f335) return true;
	if (cp >= 0x1f337 && cp <= 0x1f37c) return true;
	if (cp >= 0x1f37e && cp <= 0x1f393) return true;
	if (cp >= 0x1f3a0 && cp <= 0x1f3ca) return true;
	if (cp >= 0x1f3cf && cp <= 0x1f3d3) return true;
	if (cp >= 0x1f3e0 && cp <= 0x1f3f0) return true;
	if (cp === 0x1f3f4) return true;
	if (cp >= 0x1f3f8 && cp <= 0x1f43e) return true;
	if (cp === 0x1f440) return true;
	if (cp >= 0x1f442 && cp <= 0x1f4fc) return true;
	if (cp >= 0x1f4ff && cp <= 0x1f53d) return true;
	if (cp >= 0x1f54b && cp <= 0x1f54e) return true;
	if (cp >= 0x1f550 && cp <= 0x1f567) return true;
	if (cp === 0x1f57a) return true;
	if (cp === 0x1f595 || cp === 0x1f596) return true;
	if (cp === 0x1f5a4) return true;
	if (cp >= 0x1f5fb && cp <= 0x1f64f) return true;
	if (cp >= 0x1f680 && cp <= 0x1f6c5) return true;
	if (cp === 0x1f6cc) return true;
	if (cp >= 0x1f6d0 && cp <= 0x1f6d2) return true;
	if (cp >= 0x1f6d5 && cp <= 0x1f6d8) return true;
	if (cp >= 0x1f6dc && cp <= 0x1f6df) return true;
	if (cp === 0x1f6eb || cp === 0x1f6ec) return true;
	if (cp >= 0x1f6f4 && cp <= 0x1f6fc) return true;
	if (cp >= 0x1f7e0 && cp <= 0x1f7eb) return true;
	if (cp === 0x1f7f0) return true;
	if (cp >= 0x1f90c && cp <= 0x1f93a) return true;
	if (cp >= 0x1f93c && cp <= 0x1f945) return true;
	if (cp >= 0x1f947 && cp <= 0x1f9ff) return true;
	if (cp >= 0x1fa70 && cp <= 0x1fa77) return true;
	if (cp >= 0x1fa78 && cp <= 0x1fa7c) return true;
	if (cp >= 0x1fa80 && cp <= 0x1fa8a) return true;
	if (cp >= 0x1fa8e && cp <= 0x1fa8f) return true;
	if (cp >= 0x1fa90 && cp <= 0x1fabd) return true;
	if (cp >= 0x1fabe && cp <= 0x1fabf) return true;
	if (cp >= 0x1fac0 && cp <= 0x1fac6) return true;
	if (cp === 0x1fac8) return true;
	if (cp >= 0x1facd && cp <= 0x1facf) return true;
	if (cp >= 0x1fad0 && cp <= 0x1fadc) return true;
	if (cp === 0x1fadf) return true;
	if (cp >= 0x1fae0 && cp <= 0x1faea) return true;
	if (cp === 0x1faef) return true;
	if (cp >= 0x1faf0 && cp <= 0x1faf8) return true;

	return false;
}

/**
 * Check if a code point is zero-width.
 *
 * Covers ZWJ, Variation Selectors, and other invisible formatting characters.
 */
function isZeroWidth(cp: number): boolean {
	// Zero Width Space
	if (cp === 0x200b) return true;
	// Zero Width Non-Joiner
	if (cp === 0x200c) return true;
	// Zero Width Joiner
	if (cp === 0x200d) return true;
	// Word Joiner
	if (cp === 0x2060) return true;
	// Zero Width No-Break Space / BOM
	if (cp === 0xfeff) return true;
	// Variation Selectors (VS1-VS16)
	if (cp >= 0xfe00 && cp <= 0xfe0f) return true;
	// Variation Selectors Supplement (VS17-VS256)
	if (cp >= 0xe0100 && cp <= 0xe01ef) return true;

	return false;
}

/**
 * Check if a code point is Extended_Pictographic (Unicode 17.0).
 *
 * Used for grapheme cluster boundary detection in emoji sequences.
 * This is a broader set than Emoji_Presentation.
 */
export function isExtendedPictographic(cp: number): boolean {
	// Specific BMP codepoints
	if (cp === 0x00a9 || cp === 0x00ae) return true;
	if (cp === 0x203c || cp === 0x2049) return true;
	if (cp === 0x2122 || cp === 0x2139) return true;
	if (cp >= 0x2194 && cp <= 0x2199) return true;
	if (cp === 0x21a9 || cp === 0x21aa) return true;
	if (cp === 0x231a || cp === 0x231b) return true;
	if (cp === 0x2328) return true;
	if (cp === 0x23cf) return true;
	if (cp >= 0x23e9 && cp <= 0x23f3) return true;
	if (cp >= 0x23f8 && cp <= 0x23fa) return true;
	if (cp === 0x24c2) return true;
	if (cp === 0x25aa || cp === 0x25ab) return true;
	if (cp === 0x25b6) return true;
	if (cp === 0x25c0) return true;
	if (cp >= 0x25fb && cp <= 0x25fe) return true;
	if (cp >= 0x2600 && cp <= 0x27bf) return true;
	if (cp === 0x2934 || cp === 0x2935) return true;
	if (cp >= 0x2b05 && cp <= 0x2b07) return true;
	if (cp === 0x2b1b || cp === 0x2b1c) return true;
	if (cp === 0x2b50) return true;
	if (cp === 0x2b55) return true;
	if (cp === 0x3030) return true;
	if (cp === 0x303d) return true;
	if (cp === 0x3297) return true;
	if (cp === 0x3299) return true;

	// SMP range: U+1F000..U+1FFFD (covers all SMP emoji blocks)
	if (cp >= 0x1f000 && cp <= 0x1fffd) return true;

	return false;
}

/**
 * Check if a code point is a Regional Indicator symbol (U+1F1E6..U+1F1FF).
 */
export function isRegionalIndicator(cp: number): boolean {
	return cp >= 0x1f1e6 && cp <= 0x1f1ff;
}

/**
 * Check if a code point is a skin tone modifier (U+1F3FB..U+1F3FF).
 */
export function isSkinToneModifier(cp: number): boolean {
	return cp >= 0x1f3fb && cp <= 0x1f3ff;
}

/**
 * Check if a code point is a Variation Selector (VS1-VS16 or VS17-VS256).
 */
export function isVariationSelector(cp: number): boolean {
	return (cp >= 0xfe00 && cp <= 0xfe0f) || (cp >= 0xe0100 && cp <= 0xe01ef);
}

/**
 * Check if a string contains a variation selector (VS15 U+FE0E or VS16 U+FE0F).
 */
export function hasVariationSelector(s: string): boolean {
	for (let i = 0; i < s.length; i++) {
		const c = s.charCodeAt(i);
		if (c === 0xfe0e || c === 0xfe0f) return true;
	}
	return false;
}

/**
 * Check if a code point is a combining character.
 */
export function isCombiningChar(cp: number): boolean {
	let lo = 0;
	let hi = COMBINING_RANGES_SORTED.length - 1;
	while (lo <= hi) {
		const mid = (lo + hi) >>> 1;
		const start = COMBINING_RANGES_SORTED[mid]![0];
		const end = COMBINING_RANGES_SORTED[mid]![1];
		if (cp < start) {
			hi = mid - 1;
		} else if (cp > end) {
			lo = mid + 1;
		} else {
			return true;
		}
	}
	return false;
}

/**
 * Sorted table of Unicode combining character ranges (Mn/Me categories).
 * Each entry is [start, end] inclusive. Based on Unicode 17.0.
 *
 * Note: Variation Selectors (FE00-FE0F, E0100-E01EF) are handled separately
 * by isVariationSelector() / isZeroWidth() and not included here.
 */
const COMBINING_RANGES_SORTED: ReadonlyArray<[number, number]> = [
	// Combining Diacritical Marks
	[0x0300, 0x036f],
	// Cyrillic combining marks
	[0x0483, 0x0489],
	// Hebrew accents and marks
	[0x0591, 0x05bd],
	[0x05bf, 0x05bf],
	[0x05c1, 0x05c2],
	[0x05c4, 0x05c5],
	[0x05c7, 0x05c7],
	// Arabic combining marks
	[0x0610, 0x061a],
	[0x064b, 0x065f],
	[0x0670, 0x0670],
	[0x06d6, 0x06dc],
	[0x06df, 0x06e4],
	[0x06e7, 0x06e8],
	[0x06ea, 0x06ed],
	// Syriac
	[0x0711, 0x0711],
	[0x0730, 0x074a],
	// Thaana
	[0x07a6, 0x07b0],
	// NKo
	[0x07eb, 0x07f3],
	[0x07fd, 0x07fd],
	// Samaritan
	[0x0816, 0x0819],
	[0x081b, 0x0823],
	[0x0825, 0x0827],
	[0x0829, 0x082d],
	// Mandaic
	[0x0859, 0x085b],
	// Arabic Extended-B
	[0x0898, 0x089f],
	// Arabic Extended-A / Devanagari signs
	[0x08ca, 0x0903],
	// Devanagari
	[0x093a, 0x093c],
	[0x093e, 0x094f],
	[0x0951, 0x0957],
	[0x0962, 0x0963],
	// Bengali
	[0x0981, 0x0983],
	[0x09bc, 0x09bc],
	[0x09be, 0x09c4],
	[0x09c7, 0x09c8],
	[0x09cb, 0x09cd],
	[0x09d7, 0x09d7],
	[0x09e2, 0x09e3],
	[0x09fe, 0x09fe],
	// Gurmukhi
	[0x0a01, 0x0a03],
	[0x0a3c, 0x0a3c],
	[0x0a3e, 0x0a42],
	[0x0a47, 0x0a48],
	[0x0a4b, 0x0a4d],
	[0x0a51, 0x0a51],
	[0x0a70, 0x0a71],
	[0x0a75, 0x0a75],
	// Gujarati
	[0x0a81, 0x0a83],
	[0x0abc, 0x0abc],
	[0x0abe, 0x0ac5],
	[0x0ac7, 0x0ac9],
	[0x0acb, 0x0acd],
	[0x0ae2, 0x0ae3],
	[0x0afa, 0x0aff],
	// Oriya
	[0x0b01, 0x0b03],
	[0x0b3c, 0x0b3c],
	[0x0b3e, 0x0b44],
	[0x0b47, 0x0b48],
	[0x0b4b, 0x0b4d],
	[0x0b55, 0x0b57],
	[0x0b62, 0x0b63],
	// Tamil
	[0x0b82, 0x0b82],
	[0x0bbe, 0x0bc2],
	[0x0bc6, 0x0bc8],
	[0x0bca, 0x0bcd],
	[0x0bd7, 0x0bd7],
	// Telugu
	[0x0c00, 0x0c04],
	[0x0c3c, 0x0c3c],
	[0x0c3e, 0x0c44],
	[0x0c46, 0x0c48],
	[0x0c4a, 0x0c4d],
	[0x0c55, 0x0c56],
	[0x0c62, 0x0c63],
	// Kannada
	[0x0c81, 0x0c83],
	[0x0cbc, 0x0cbc],
	[0x0cbe, 0x0cc4],
	[0x0cc6, 0x0cc8],
	[0x0cca, 0x0ccd],
	[0x0cd5, 0x0cd6],
	[0x0ce2, 0x0ce3],
	[0x0cf3, 0x0cf3],
	// Malayalam
	[0x0d00, 0x0d03],
	[0x0d3b, 0x0d3c],
	[0x0d3e, 0x0d44],
	[0x0d46, 0x0d48],
	[0x0d4a, 0x0d4d],
	[0x0d57, 0x0d57],
	[0x0d62, 0x0d63],
	// Sinhala
	[0x0d81, 0x0d83],
	[0x0dca, 0x0dca],
	[0x0dcf, 0x0dd4],
	[0x0dd6, 0x0dd6],
	[0x0dd8, 0x0ddf],
	[0x0df2, 0x0df3],
	// Thai
	[0x0e31, 0x0e31],
	[0x0e34, 0x0e3a],
	[0x0e47, 0x0e4e],
	// Lao
	[0x0eb1, 0x0eb1],
	[0x0eb4, 0x0ebc],
	[0x0ec8, 0x0ece],
	// Tibetan
	[0x0f18, 0x0f19],
	[0x0f35, 0x0f35],
	[0x0f37, 0x0f37],
	[0x0f39, 0x0f39],
	[0x0f71, 0x0f84],
	[0x0f86, 0x0f87],
	[0x0f8d, 0x0f97],
	[0x0f99, 0x0fbc],
	[0x0fc6, 0x0fc6],
	// Myanmar
	[0x102b, 0x103e],
	[0x1056, 0x1059],
	[0x105e, 0x1060],
	[0x1062, 0x1064],
	[0x1067, 0x106d],
	[0x1071, 0x1074],
	[0x1082, 0x108d],
	[0x108f, 0x108f],
	[0x109a, 0x109d],
	// Ethiopic
	[0x135d, 0x135f],
	// Tagalog
	[0x1712, 0x1715],
	// Hanunoo
	[0x1732, 0x1734],
	// Buhid
	[0x1752, 0x1753],
	// Tagbanwa
	[0x1772, 0x1773],
	// Khmer
	[0x17b4, 0x17d3],
	[0x17dd, 0x17dd],
	// Mongolian
	[0x180b, 0x180d],
	[0x180f, 0x180f],
	[0x1885, 0x1886],
	[0x18a9, 0x18a9],
	// Limbu
	[0x1920, 0x192b],
	[0x1930, 0x193b],
	// Buginese
	[0x1a17, 0x1a1b],
	// Tai Tham
	[0x1a55, 0x1a5e],
	[0x1a60, 0x1a7c],
	[0x1a7f, 0x1a7f],
	// Combining Diacritical Marks Extended
	[0x1ab0, 0x1ace],
	// Balinese
	[0x1b00, 0x1b04],
	[0x1b34, 0x1b44],
	[0x1b6b, 0x1b73],
	// Sundanese
	[0x1b80, 0x1b82],
	[0x1ba1, 0x1bad],
	// Batak
	[0x1be6, 0x1bf3],
	// Lepcha
	[0x1c24, 0x1c37],
	// Vedic
	[0x1cd0, 0x1cd2],
	[0x1cd4, 0x1ce8],
	[0x1ced, 0x1ced],
	[0x1cf4, 0x1cf4],
	[0x1cf7, 0x1cf9],
	// Combining Diacritical Marks Supplement
	[0x1dc0, 0x1dff],
	// Combining Diacritical Marks for Symbols
	[0x20d0, 0x20f0],
	// Coptic
	[0x2cef, 0x2cf1],
	// Tifinagh
	[0x2d7f, 0x2d7f],
	// Cyrillic Extended-A
	[0x2de0, 0x2dff],
	// CJK ideographic combining marks
	[0x302a, 0x302f],
	// Japanese dakuten / handakuten
	[0x3099, 0x309a],
	// Cyrillic Extended-B combining marks
	[0xa66f, 0xa672],
	[0xa674, 0xa67d],
	[0xa69e, 0xa69f],
	// Bamum
	[0xa6f0, 0xa6f1],
	// Syloti Nagri
	[0xa802, 0xa802],
	[0xa806, 0xa806],
	[0xa80b, 0xa80b],
	[0xa823, 0xa827],
	[0xa82c, 0xa82c],
	// Saurashtra
	[0xa880, 0xa881],
	[0xa8b4, 0xa8c5],
	// Devanagari Extended
	[0xa8e0, 0xa8f1],
	[0xa8ff, 0xa8ff],
	// Kayah Li
	[0xa926, 0xa92d],
	// Rejang
	[0xa947, 0xa953],
	// Javanese
	[0xa980, 0xa983],
	[0xa9b3, 0xa9c0],
	[0xa9e5, 0xa9e5],
	// Cham
	[0xaa29, 0xaa36],
	[0xaa43, 0xaa43],
	[0xaa4c, 0xaa4d],
	[0xaa7b, 0xaa7d],
	// Tai Viet
	[0xaab0, 0xaab0],
	[0xaab2, 0xaab4],
	[0xaab7, 0xaab8],
	[0xaabe, 0xaabf],
	[0xaac1, 0xaac1],
	// Meetei Mayek
	[0xaaeb, 0xaaef],
	[0xaaf5, 0xaaf6],
	[0xabe3, 0xabea],
	[0xabec, 0xabed],
	// Hebrew point
	[0xfb1e, 0xfb1e],
	// Combining Half Marks
	[0xfe20, 0xfe2f],
	// Phaistos Disc
	[0x101fd, 0x101fd],
	// Coptic Epact
	[0x102e0, 0x102e0],
	// Old Permic
	[0x10376, 0x1037a],
	// Kharoshthi
	[0x10a01, 0x10a03],
	[0x10a05, 0x10a06],
	[0x10a0c, 0x10a0f],
	[0x10a38, 0x10a3a],
	[0x10a3f, 0x10a3f],
	// Manichaean
	[0x10ae5, 0x10ae6],
	// Hanifi Rohingya
	[0x10d24, 0x10d27],
	// Yezidi
	[0x10eab, 0x10eac],
	// Arabic Extended-C
	[0x10efd, 0x10eff],
	// Sogdian
	[0x10f46, 0x10f50],
	// Old Uyghur
	[0x10f82, 0x10f85],
	// Brahmi
	[0x11000, 0x11002],
	[0x11038, 0x11046],
	[0x11070, 0x11070],
	[0x11073, 0x11074],
	[0x1107f, 0x11082],
	// Kaithi
	[0x110b0, 0x110ba],
	[0x110c2, 0x110c2],
	// Chakma
	[0x11100, 0x11102],
	[0x11127, 0x11134],
	[0x11145, 0x11146],
	// Mahajani
	[0x11173, 0x11173],
	// Sharada
	[0x11180, 0x11182],
	[0x111b3, 0x111c0],
	[0x111c9, 0x111cc],
	[0x111ce, 0x111cf],
	// Khojki
	[0x1122c, 0x11237],
	[0x1123e, 0x1123e],
	[0x11241, 0x11241],
	// Khudawadi
	[0x112df, 0x112ea],
	// Grantha
	[0x11300, 0x11303],
	[0x1133b, 0x1133c],
	[0x1133e, 0x11344],
	[0x11347, 0x11348],
	[0x1134b, 0x1134d],
	[0x11357, 0x11357],
	[0x11362, 0x11363],
	[0x11366, 0x1136c],
	[0x11370, 0x11374],
	// Newa
	[0x11435, 0x11446],
	[0x1145e, 0x1145e],
	// Tirhuta
	[0x114b0, 0x114c3],
	// Siddham
	[0x115af, 0x115b5],
	[0x115b8, 0x115c0],
	[0x115dc, 0x115dd],
	// Modi
	[0x11630, 0x11640],
	// Takri
	[0x116ab, 0x116b7],
	// Ahom
	[0x1171d, 0x1172b],
	// Dogra
	[0x1182c, 0x1183a],
	// Dives Akuru
	[0x11930, 0x11935],
	[0x11937, 0x11938],
	[0x1193b, 0x1193e],
	[0x11940, 0x11940],
	[0x11942, 0x11943],
	// Nandinagari
	[0x119d1, 0x119d7],
	[0x119da, 0x119e0],
	[0x119e4, 0x119e4],
	// Zanabazar Square
	[0x11a01, 0x11a0a],
	[0x11a33, 0x11a39],
	[0x11a3b, 0x11a3e],
	[0x11a47, 0x11a47],
	// Soyombo
	[0x11a51, 0x11a5b],
	[0x11a8a, 0x11a99],
	// Bhaiksuki
	[0x11c2f, 0x11c36],
	[0x11c38, 0x11c3f],
	// Marchen
	[0x11c92, 0x11ca7],
	[0x11ca9, 0x11cb6],
	// Masaram Gondi
	[0x11d31, 0x11d36],
	[0x11d3a, 0x11d3a],
	[0x11d3c, 0x11d3d],
	[0x11d3f, 0x11d45],
	[0x11d47, 0x11d47],
	// Gunjala Gondi
	[0x11d8a, 0x11d8e],
	[0x11d90, 0x11d91],
	[0x11d93, 0x11d97],
	// Makasar
	[0x11ef3, 0x11ef6],
	// Kawi
	[0x11f00, 0x11f01],
	[0x11f34, 0x11f3a],
	[0x11f3e, 0x11f42],
	// Egyptian Hieroglyphs
	[0x13440, 0x13440],
	[0x13447, 0x13455],
	// Bassa Vah
	[0x16af0, 0x16af4],
	// Pahawh Hmong
	[0x16b30, 0x16b36],
	// Miao
	[0x16f4f, 0x16f4f],
	[0x16f8f, 0x16f92],
	// Khitan Small Script
	[0x16fe4, 0x16fe4],
	// Duployan
	[0x1bc9d, 0x1bc9e],
	// Znamenny Musical Notation
	[0x1cf00, 0x1cf2d],
	[0x1cf30, 0x1cf46],
	// Musical Symbols
	[0x1d165, 0x1d169],
	[0x1d16d, 0x1d172],
	[0x1d17b, 0x1d182],
	[0x1d185, 0x1d18b],
	[0x1d1aa, 0x1d1ad],
	// Combining Greek Musical Notation
	[0x1d242, 0x1d244],
	// Signwriting
	[0x1da00, 0x1da36],
	[0x1da3b, 0x1da6c],
	[0x1da75, 0x1da75],
	[0x1da84, 0x1da84],
	[0x1da9b, 0x1da9f],
	[0x1daa1, 0x1daaf],
	// Glagolitic Supplement
	[0x1e000, 0x1e006],
	[0x1e008, 0x1e018],
	[0x1e01b, 0x1e021],
	[0x1e023, 0x1e024],
	[0x1e026, 0x1e02a],
	// Cyrillic Extended-D
	[0x1e08f, 0x1e08f],
	// Nyiakeng Puachue Hmong
	[0x1e130, 0x1e136],
	// Wancho
	[0x1e2ae, 0x1e2ae],
	[0x1e2ec, 0x1e2ef],
	// Cypro-Minoan
	[0x1e4ec, 0x1e4ef],
	// Mende Kikakui
	[0x1e8d0, 0x1e8d6],
	// Adlam
	[0x1e944, 0x1e94a],
];

/**
 * Check if a code point is wide (East Asian Width: F or W).
 *
 * This is a simplified implementation covering common ranges.
 * For full Unicode compliance, a lookup table would be needed.
 */
function isWideCodePoint(cp: number): boolean {
	// CJK Radicals Supplement
	if (cp >= 0x2e80 && cp <= 0x2eff) return true;
	// Kangxi Radicals
	if (cp >= 0x2f00 && cp <= 0x2fdf) return true;
	// CJK Symbols and Punctuation
	if (cp >= 0x3000 && cp <= 0x303f) return true;
	// Hiragana
	if (cp >= 0x3040 && cp <= 0x309f) return true;
	// Katakana
	if (cp >= 0x30a0 && cp <= 0x30ff) return true;
	// Bopomofo
	if (cp >= 0x3100 && cp <= 0x312f) return true;
	// Hangul Compatibility Jamo
	if (cp >= 0x3130 && cp <= 0x318f) return true;
	// Kanbun
	if (cp >= 0x3190 && cp <= 0x319f) return true;
	// Bopomofo Extended
	if (cp >= 0x31a0 && cp <= 0x31bf) return true;
	// CJK Strokes
	if (cp >= 0x31c0 && cp <= 0x31ef) return true;
	// Katakana Phonetic Extensions
	if (cp >= 0x31f0 && cp <= 0x31ff) return true;
	// Enclosed CJK Letters and Months
	if (cp >= 0x3200 && cp <= 0x32ff) return true;
	// CJK Compatibility
	if (cp >= 0x3300 && cp <= 0x33ff) return true;
	// CJK Unified Ideographs Extension A
	if (cp >= 0x3400 && cp <= 0x4dbf) return true;
	// CJK Unified Ideographs
	if (cp >= 0x4e00 && cp <= 0x9fff) return true;
	// Yi Syllables
	if (cp >= 0xa000 && cp <= 0xa48f) return true;
	// Yi Radicals
	if (cp >= 0xa490 && cp <= 0xa4cf) return true;
	// Hangul Syllables
	if (cp >= 0xac00 && cp <= 0xd7a3) return true;
	// CJK Compatibility Ideographs
	if (cp >= 0xf900 && cp <= 0xfaff) return true;
	// Vertical Forms
	if (cp >= 0xfe10 && cp <= 0xfe1f) return true;
	// CJK Compatibility Forms
	if (cp >= 0xfe30 && cp <= 0xfe4f) return true;
	// Fullwidth Forms (excluding halfwidth)
	if (cp >= 0xff00 && cp <= 0xff60) return true;
	if (cp >= 0xffe0 && cp <= 0xffe6) return true;
	// CJK Unified Ideographs Extension B and beyond
	if (cp >= 0x20000 && cp <= 0x2ffff) return true;
	// CJK Compatibility Ideographs Supplement
	if (cp >= 0x2f800 && cp <= 0x2fa1f) return true;

	return false;
}

/**
 * Sorted table of Unicode East Asian Width = Ambiguous ranges.
 * Each entry is [start, end] inclusive. Based on Unicode 17.0.
 */
const AMBIGUOUS_WIDTH_RANGES: ReadonlyArray<[number, number]> = [
	[0x00a1, 0x00a1], [0x00a4, 0x00a4], [0x00a7, 0x00a8], [0x00aa, 0x00aa],
	[0x00ad, 0x00ae], [0x00b0, 0x00b4], [0x00b6, 0x00ba], [0x00bc, 0x00bf],
	[0x00c6, 0x00c6], [0x00d0, 0x00d0], [0x00d7, 0x00d8], [0x00de, 0x00e1],
	[0x00e6, 0x00e6], [0x00e8, 0x00ea], [0x00ec, 0x00ed], [0x00f0, 0x00f0],
	[0x00f2, 0x00f3], [0x00f7, 0x00fa], [0x00fc, 0x00fc], [0x00fe, 0x00fe],
	[0x0101, 0x0101], [0x0111, 0x0111], [0x0113, 0x0113], [0x011b, 0x011b],
	[0x0126, 0x0127], [0x012b, 0x012b], [0x0131, 0x0133], [0x0138, 0x0138],
	[0x013f, 0x0142], [0x0144, 0x0144], [0x0148, 0x014b], [0x014d, 0x014d],
	[0x0152, 0x0153], [0x0166, 0x0167], [0x016b, 0x016b], [0x01ce, 0x01ce],
	[0x01d0, 0x01d0], [0x01d2, 0x01d2], [0x01d4, 0x01d4], [0x01d6, 0x01d6],
	[0x01d8, 0x01d8], [0x01da, 0x01da], [0x01dc, 0x01dc], [0x0251, 0x0251],
	[0x0261, 0x0261], [0x02c4, 0x02c4], [0x02c7, 0x02c7], [0x02c9, 0x02cb],
	[0x02cd, 0x02cd], [0x02d0, 0x02d0], [0x02d8, 0x02db], [0x02dd, 0x02dd],
	[0x02df, 0x02df], [0x0300, 0x036f], [0x0391, 0x03a1], [0x03a3, 0x03a9],
	[0x03b1, 0x03c1], [0x03c3, 0x03c9], [0x0401, 0x0401], [0x0410, 0x044f],
	[0x0451, 0x0451], [0x2010, 0x2010], [0x2013, 0x2016], [0x2018, 0x2019],
	[0x201c, 0x201d], [0x2020, 0x2022], [0x2024, 0x2027], [0x2030, 0x2030],
	[0x2032, 0x2033], [0x2035, 0x2035], [0x203b, 0x203b], [0x203e, 0x203e],
	[0x2074, 0x2074], [0x207f, 0x207f], [0x2081, 0x2084], [0x20ac, 0x20ac],
	[0x2103, 0x2103], [0x2105, 0x2105], [0x2109, 0x2109], [0x2113, 0x2113],
	[0x2116, 0x2116], [0x2121, 0x2122], [0x2126, 0x2126], [0x212b, 0x212b],
	[0x2153, 0x2154], [0x215b, 0x215e], [0x2160, 0x216b], [0x2170, 0x2179],
	[0x2189, 0x2189], [0x2190, 0x2199], [0x21b8, 0x21b9], [0x21d2, 0x21d2],
	[0x21d4, 0x21d4], [0x21e7, 0x21e7], [0x2200, 0x2200], [0x2202, 0x2203],
	[0x2207, 0x2208], [0x220b, 0x220b], [0x220f, 0x220f], [0x2211, 0x2211],
	[0x2215, 0x2215], [0x221a, 0x221a], [0x221d, 0x2220], [0x2223, 0x2223],
	[0x2225, 0x2225], [0x2227, 0x222c], [0x222e, 0x222e], [0x2234, 0x2237],
	[0x223c, 0x223d], [0x2248, 0x2248], [0x224c, 0x224c], [0x2252, 0x2252],
	[0x2260, 0x2261], [0x2264, 0x2267], [0x226a, 0x226b], [0x226e, 0x226f],
	[0x2282, 0x2283], [0x2286, 0x2287], [0x2295, 0x2295], [0x2299, 0x2299],
	[0x22a5, 0x22a5], [0x22bf, 0x22bf], [0x2312, 0x2312], [0x2460, 0x24e9],
	[0x24eb, 0x254b], [0x2550, 0x2573], [0x2580, 0x258f], [0x2592, 0x2595],
	[0x25a0, 0x25a1], [0x25a3, 0x25a9], [0x25b2, 0x25b3], [0x25b6, 0x25b7],
	[0x25bc, 0x25bd], [0x25c0, 0x25c1], [0x25c6, 0x25c8], [0x25cb, 0x25cb],
	[0x25ce, 0x25d1], [0x25e2, 0x25e5], [0x25ef, 0x25ef], [0x2605, 0x2606],
	[0x2609, 0x2609], [0x260e, 0x260f], [0x261c, 0x261c], [0x261e, 0x261e],
	[0x2640, 0x2640], [0x2642, 0x2642], [0x2660, 0x2661], [0x2663, 0x2665],
	[0x2667, 0x266a], [0x266c, 0x266d], [0x266f, 0x266f], [0x269e, 0x269f],
	[0x26bf, 0x26bf], [0x26c6, 0x26cd], [0x26cf, 0x26d3], [0x26d5, 0x26e1],
	[0x26e3, 0x26e3], [0x26e8, 0x26e9], [0x26eb, 0x26f1], [0x26f4, 0x26f4],
	[0x26f6, 0x26f9], [0x26fb, 0x26fc], [0x26fe, 0x26ff], [0x273d, 0x273d],
	[0x2776, 0x277f], [0x2b56, 0x2b59], [0xe000, 0xf8ff], [0xfe00, 0xfe0f],
	[0xfffd, 0xfffd], [0x1f100, 0x1f10a], [0x1f110, 0x1f12d],
	[0x1f130, 0x1f169], [0x1f170, 0x1f18d], [0x1f18f, 0x1f190],
	[0x1f19b, 0x1f1ac], [0xf0000, 0xffffd], [0x100000, 0x10fffd],
];

/**
 * Check if a code point has East Asian Width = Ambiguous.
 *
 * Uses binary search on a sorted range table.
 */
export function isAmbiguousWidth(cp: number): boolean {
	let lo = 0;
	let hi = AMBIGUOUS_WIDTH_RANGES.length - 1;
	while (lo <= hi) {
		const mid = (lo + hi) >>> 1;
		const start = AMBIGUOUS_WIDTH_RANGES[mid]![0];
		const end = AMBIGUOUS_WIDTH_RANGES[mid]![1];
		if (cp < start) {
			hi = mid - 1;
		} else if (cp > end) {
			lo = mid + 1;
		} else {
			return true;
		}
	}
	return false;
}

/**
 * Calculate the display width of a string.
 *
 * Note: This iterates by codepoint, not by grapheme cluster.
 * For pre-composed emoji cluster strings (e.g., ZWJ sequences),
 * the result will be the sum of individual codepoint widths,
 * not the display width of the cluster (which is always 2).
 * Use cell.width for grid cells containing cluster strings.
 *
 * @param str - The string to measure
 * @returns Total display width in terminal cells
 */
export function stringWidth(str: string): number {
	let width = 0;
	for (const char of str) {
		width += charWidth(char);
	}
	return width;
}
