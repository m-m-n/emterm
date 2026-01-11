/**
 * Tests for unicode character width calculation.
 */
import { describe, expect, test } from "bun:test";
import { charWidth, isWideChar } from "./unicode.ts";

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
