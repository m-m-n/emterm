/**
 * Unicode character width calculation.
 *
 * Determines display width of characters for terminal rendering.
 * Based on Unicode East Asian Width property.
 */

/**
 * Get the display width of a character in terminal cells.
 *
 * @param char - A single character (or first character of string)
 * @returns 0 for control/combining chars, 1 for narrow, 2 for wide
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
 * Check if a code point is a combining character.
 */
function isCombiningChar(cp: number): boolean {
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
