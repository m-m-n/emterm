/**
 * Tests for SGR parameter parsing.
 */
import { describe, expect, it } from "bun:test";
import type { SgrAttr } from "./attributes.ts";
import { parseSgrParams } from "./sgr.ts";

describe("parseSgrParams", () => {
	describe("Reset", () => {
		it("should parse empty params as reset", () => {
			const attrs = parseSgrParams([]);
			expect(attrs).toEqual([{ attr: "Reset" }]);
		});

		it("should parse 0 as reset", () => {
			const attrs = parseSgrParams([0]);
			expect(attrs).toEqual([{ attr: "Reset" }]);
		});
	});

	describe("Text attributes", () => {
		it("should parse bold (1)", () => {
			expect(parseSgrParams([1])).toEqual([{ attr: "Bold" }]);
		});

		it("should parse dim (2)", () => {
			expect(parseSgrParams([2])).toEqual([{ attr: "Dim" }]);
		});

		it("should parse italic (3)", () => {
			expect(parseSgrParams([3])).toEqual([{ attr: "Italic" }]);
		});

		it("should parse underline (4)", () => {
			expect(parseSgrParams([4])).toEqual([{ attr: "Underline" }]);
		});

		it("should parse blink (5)", () => {
			expect(parseSgrParams([5])).toEqual([{ attr: "Blink" }]);
		});

		it("should parse reverse (7)", () => {
			expect(parseSgrParams([7])).toEqual([{ attr: "Reverse" }]);
		});

		it("should parse hidden (8)", () => {
			expect(parseSgrParams([8])).toEqual([{ attr: "Hidden" }]);
		});

		it("should parse strikethrough (9)", () => {
			expect(parseSgrParams([9])).toEqual([{ attr: "Strikethrough" }]);
		});
	});

	describe("Attribute resets", () => {
		it("should parse normal intensity (22)", () => {
			expect(parseSgrParams([22])).toEqual([{ attr: "NormalIntensity" }]);
		});

		it("should parse not italic (23)", () => {
			expect(parseSgrParams([23])).toEqual([{ attr: "NotItalic" }]);
		});

		it("should parse not underline (24)", () => {
			expect(parseSgrParams([24])).toEqual([{ attr: "NotUnderline" }]);
		});

		it("should parse not blink (25)", () => {
			expect(parseSgrParams([25])).toEqual([{ attr: "NotBlink" }]);
		});

		it("should parse not reverse (27)", () => {
			expect(parseSgrParams([27])).toEqual([{ attr: "NotReverse" }]);
		});

		it("should parse not hidden (28)", () => {
			expect(parseSgrParams([28])).toEqual([{ attr: "NotHidden" }]);
		});

		it("should parse not strikethrough (29)", () => {
			expect(parseSgrParams([29])).toEqual([{ attr: "NotStrikethrough" }]);
		});
	});

	describe("Standard foreground colors (30-37)", () => {
		it("should parse black foreground (30)", () => {
			expect(parseSgrParams([30])).toEqual([
				{ attr: "Foreground", value: { type: "Standard", value: 0 } },
			]);
		});

		it("should parse red foreground (31)", () => {
			expect(parseSgrParams([31])).toEqual([
				{ attr: "Foreground", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should parse white foreground (37)", () => {
			expect(parseSgrParams([37])).toEqual([
				{ attr: "Foreground", value: { type: "Standard", value: 7 } },
			]);
		});

		it("should parse default foreground (39)", () => {
			expect(parseSgrParams([39])).toEqual([{ attr: "DefaultForeground" }]);
		});
	});

	describe("Standard background colors (40-47)", () => {
		it("should parse black background (40)", () => {
			expect(parseSgrParams([40])).toEqual([
				{ attr: "Background", value: { type: "Standard", value: 0 } },
			]);
		});

		it("should parse red background (41)", () => {
			expect(parseSgrParams([41])).toEqual([
				{ attr: "Background", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should parse default background (49)", () => {
			expect(parseSgrParams([49])).toEqual([{ attr: "DefaultBackground" }]);
		});
	});

	describe("Bright foreground colors (90-97)", () => {
		it("should parse bright black foreground (90)", () => {
			expect(parseSgrParams([90])).toEqual([
				{ attr: "Foreground", value: { type: "Bright", value: 0 } },
			]);
		});

		it("should parse bright red foreground (91)", () => {
			expect(parseSgrParams([91])).toEqual([
				{ attr: "Foreground", value: { type: "Bright", value: 1 } },
			]);
		});
	});

	describe("Bright background colors (100-107)", () => {
		it("should parse bright red background (101)", () => {
			expect(parseSgrParams([101])).toEqual([
				{ attr: "Background", value: { type: "Bright", value: 1 } },
			]);
		});
	});

	describe("256-color mode", () => {
		it("should parse 256-color foreground (38;5;n)", () => {
			expect(parseSgrParams([38, 5, 196])).toEqual([
				{ attr: "Foreground", value: { type: "Indexed", value: 196 } },
			]);
		});

		it("should parse 256-color background (48;5;n)", () => {
			expect(parseSgrParams([48, 5, 100])).toEqual([
				{ attr: "Background", value: { type: "Indexed", value: 100 } },
			]);
		});

		it("should handle incomplete 256-color sequence", () => {
			expect(parseSgrParams([38, 5])).toEqual([
				{ attr: "Foreground", value: { type: "Indexed", value: 0 } },
			]);
		});
	});

	describe("RGB mode", () => {
		it("should parse RGB foreground (38;2;r;g;b)", () => {
			expect(parseSgrParams([38, 2, 255, 128, 0])).toEqual([
				{
					attr: "Foreground",
					value: { type: "Rgb", value: { r: 255, g: 128, b: 0 } },
				},
			]);
		});

		it("should parse RGB background (48;2;r;g;b)", () => {
			expect(parseSgrParams([48, 2, 0, 128, 255])).toEqual([
				{
					attr: "Background",
					value: { type: "Rgb", value: { r: 0, g: 128, b: 255 } },
				},
			]);
		});

		it("should handle incomplete RGB sequence", () => {
			expect(parseSgrParams([38, 2, 255])).toEqual([
				{
					attr: "Foreground",
					value: { type: "Rgb", value: { r: 255, g: 0, b: 0 } },
				},
			]);
		});
	});

	describe("Combined parameters", () => {
		it("should parse bold and red (1;31)", () => {
			expect(parseSgrParams([1, 31])).toEqual([
				{ attr: "Bold" },
				{ attr: "Foreground", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should parse bold, underline, and red (1;4;31)", () => {
			expect(parseSgrParams([1, 4, 31])).toEqual([
				{ attr: "Bold" },
				{ attr: "Underline" },
				{ attr: "Foreground", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should parse reset followed by style (0;1;31)", () => {
			expect(parseSgrParams([0, 1, 31])).toEqual([
				{ attr: "Reset" },
				{ attr: "Bold" },
				{ attr: "Foreground", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should parse complex sequence with RGB", () => {
			expect(parseSgrParams([1, 3, 38, 2, 255, 0, 128])).toEqual([
				{ attr: "Bold" },
				{ attr: "Italic" },
				{
					attr: "Foreground",
					value: { type: "Rgb", value: { r: 255, g: 0, b: 128 } },
				},
			]);
		});
	});

	describe("Edge cases", () => {
		it("should ignore unknown parameters", () => {
			expect(parseSgrParams([99])).toEqual([]);
		});

		it("should skip unknown and continue with known", () => {
			expect(parseSgrParams([1, 99, 31])).toEqual([
				{ attr: "Bold" },
				{ attr: "Foreground", value: { type: "Standard", value: 1 } },
			]);
		});

		it("should handle malformed extended color", () => {
			// 38 without 5 or 2 following
			expect(parseSgrParams([38])).toEqual([]);
		});
	});
});
