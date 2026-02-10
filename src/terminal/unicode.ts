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
 * Check if a code point is a combining character.
 */
export function isCombiningChar(cp: number): boolean {
	// Combining Diacritical Marks
	if (cp >= 0x0300 && cp <= 0x036f) return true;
	// Combining Diacritical Marks Extended
	if (cp >= 0x1ab0 && cp <= 0x1aff) return true;
	// Combining Diacritical Marks Supplement
	if (cp >= 0x1dc0 && cp <= 0x1dff) return true;
	// Combining Diacritical Marks for Symbols
	if (cp >= 0x20d0 && cp <= 0x20ff) return true;
	// Combining Half Marks
	if (cp >= 0xfe20 && cp <= 0xfe2f) return true;

	return false;
}

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
