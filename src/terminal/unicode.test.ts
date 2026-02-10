/**
 * Tests for unicode character width calculation.
 */
import { describe, expect, test } from "bun:test";
import { charWidth, isWideChar, isEmojiPresentation, isExtendedPictographic } from "./unicode.ts";

describe("charWidth", () => {
	test("returns 1 for ASCII characters", () => {
		expect(charWidth("a")).toBe(1);
		expect(charWidth("A")).toBe(1);
		expect(charWidth("0")).toBe(1);
		expect(charWidth(" ")).toBe(1);
		expect(charWidth("~")).toBe(1);
	});

	test("returns 0 for control characters", () => {
		expect(charWidth("\x00")).toBe(0); // NUL
		expect(charWidth("\x07")).toBe(0); // BEL
		expect(charWidth("\x1b")).toBe(0); // ESC
	});

	test("returns 2 for CJK characters", () => {
		// CJK Unified Ideographs
		expect(charWidth("\u4e00")).toBe(2); // first CJK unified ideograph
		expect(charWidth("\u9fff")).toBe(2); // last CJK unified ideograph
		expect(charWidth("\u3042")).toBe(2); // Hiragana 'a'
		expect(charWidth("\u30a2")).toBe(2); // Katakana 'a'
	});

	test("returns 2 for fullwidth characters", () => {
		expect(charWidth("\uff01")).toBe(2); // Fullwidth exclamation mark
		expect(charWidth("\uff21")).toBe(2); // Fullwidth 'A'
		expect(charWidth("\uff10")).toBe(2); // Fullwidth '0'
	});

	test("returns 2 for Korean Hangul", () => {
		expect(charWidth("\uac00")).toBe(2); // First Hangul syllable
		expect(charWidth("\ud7a3")).toBe(2); // Last Hangul syllable
	});

	test("returns 1 for halfwidth characters", () => {
		expect(charWidth("\uff61")).toBe(1); // Halfwidth ideographic full stop
		expect(charWidth("\uff66")).toBe(1); // Halfwidth Katakana 'wo'
	});

	test("returns 0 for combining characters", () => {
		expect(charWidth("\u0300")).toBe(0); // Combining grave accent
		expect(charWidth("\u0301")).toBe(0); // Combining acute accent
	});

	test("handles empty string", () => {
		expect(charWidth("")).toBe(0);
	});

	test("uses first character for multi-char strings", () => {
		expect(charWidth("abc")).toBe(1);
		expect(charWidth("\u4e00\u4e01")).toBe(2);
	});
});

describe("charWidth - Emoji_Presentation=Yes", () => {
	test("returns 2 for SMP emoji (Emoji_Presentation=Yes)", () => {
		expect(charWidth("📁")).toBe(2); // U+1F4C1
		expect(charWidth("🔋")).toBe(2); // U+1F50B
		expect(charWidth("😀")).toBe(2); // U+1F600
		expect(charWidth("🚀")).toBe(2); // U+1F680
	});

	test("returns 2 for BMP emoji (Emoji_Presentation=Yes)", () => {
		expect(charWidth("⌚")).toBe(2); // U+231A
		expect(charWidth("⏰")).toBe(2); // U+23F0
		expect(charWidth("☕")).toBe(2); // U+2615
		expect(charWidth("⭐")).toBe(2); // U+2B50
		expect(charWidth("⌛")).toBe(2); // U+231B
		expect(charWidth("♿")).toBe(2); // U+267F
		expect(charWidth("⛔")).toBe(2); // U+26D4
		expect(charWidth("✅")).toBe(2); // U+2705
		expect(charWidth("❌")).toBe(2); // U+274C
		expect(charWidth("❗")).toBe(2); // U+2757
	});

	test("returns 1 for emoji-like characters that are NOT Emoji_Presentation=Yes", () => {
		expect(charWidth("☀")).toBe(1); // U+2600 - NOT Emoji_Presentation=Yes
		expect(charWidth("☎")).toBe(1); // U+260E - NOT Emoji_Presentation=Yes
		expect(charWidth("✉")).toBe(1); // U+2709 - NOT Emoji_Presentation=Yes
	});
});

describe("charWidth - zero-width characters", () => {
	test("returns 0 for ZWJ", () => {
		expect(charWidth("\u200D")).toBe(0);
	});

	test("returns 0 for Variation Selectors", () => {
		expect(charWidth("\uFE0F")).toBe(0); // VS16
		expect(charWidth("\uFE0E")).toBe(0); // VS15
		expect(charWidth("\uFE00")).toBe(0); // VS1
	});

	test("returns 0 for other zero-width characters", () => {
		expect(charWidth("\u200B")).toBe(0); // Zero Width Space
		expect(charWidth("\u200C")).toBe(0); // Zero Width Non-Joiner
		expect(charWidth("\u2060")).toBe(0); // Word Joiner
		expect(charWidth("\uFEFF")).toBe(0); // BOM / ZWNBS
	});
});

describe("charWidth - unchanged behavior", () => {
	test("ASCII unchanged", () => {
		expect(charWidth("A")).toBe(1);
		expect(charWidth("z")).toBe(1);
		expect(charWidth("0")).toBe(1);
	});

	test("CJK unchanged", () => {
		expect(charWidth("あ")).toBe(2);
		expect(charWidth("漢")).toBe(2);
		expect(charWidth("ア")).toBe(2);
	});
});

describe("isEmojiPresentation", () => {
	test("returns true for Emoji_Presentation=Yes codepoints", () => {
		expect(isEmojiPresentation(0x1F4C1)).toBe(true); // 📁
		expect(isEmojiPresentation(0x1F600)).toBe(true); // 😀
		expect(isEmojiPresentation(0x231A)).toBe(true);  // ⌚
		expect(isEmojiPresentation(0x2615)).toBe(true);  // ☕
		expect(isEmojiPresentation(0x2B50)).toBe(true);  // ⭐
	});

	test("returns false for non-emoji codepoints", () => {
		expect(isEmojiPresentation(0x41)).toBe(false);   // 'A'
		expect(isEmojiPresentation(0x2600)).toBe(false); // ☀
		expect(isEmojiPresentation(0x4E00)).toBe(false); // CJK
	});
});

describe("isExtendedPictographic", () => {
	test("returns true for Extended_Pictographic codepoints", () => {
		expect(isExtendedPictographic(0x00A9)).toBe(true);  // ©
		expect(isExtendedPictographic(0x00AE)).toBe(true);  // ®
		expect(isExtendedPictographic(0x2600)).toBe(true);  // ☀
		expect(isExtendedPictographic(0x1F600)).toBe(true); // 😀
		expect(isExtendedPictographic(0x1F4C1)).toBe(true); // 📁
	});

	test("returns false for non-pictographic codepoints", () => {
		expect(isExtendedPictographic(0x41)).toBe(false);   // 'A'
		expect(isExtendedPictographic(0x4E00)).toBe(false); // CJK
	});
});

describe("isWideChar", () => {
	test("returns false for ASCII", () => {
		expect(isWideChar("a")).toBe(false);
		expect(isWideChar("Z")).toBe(false);
	});

	test("returns true for CJK characters", () => {
		expect(isWideChar("\u4e00")).toBe(true);
		expect(isWideChar("\u3042")).toBe(true);
	});

	test("returns false for empty string", () => {
		expect(isWideChar("")).toBe(false);
	});
});
