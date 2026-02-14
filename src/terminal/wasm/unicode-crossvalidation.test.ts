/**
 * Cross-validation test: WASM vs TypeScript Unicode implementations.
 *
 * Compares the WASM-backed functions against the original TypeScript
 * implementation across comprehensive codepoint ranges.
 */
import { describe, expect, test } from "bun:test";

// WASM implementation (via glue)
import {
	charWidth as wasmCharWidth,
	isEmojiPresentation as wasmIsEmojiPresentation,
	isExtendedPictographic as wasmIsExtendedPictographic,
	isRegionalIndicator as wasmIsRegionalIndicator,
	isSkinToneModifier as wasmIsSkinToneModifier,
	isVariationSelector as wasmIsVariationSelector,
	isCombiningChar as wasmIsCombiningChar,
	stringWidth as wasmStringWidth,
} from "./unicode.ts";

// Original TypeScript implementation
import {
	charWidth as tsCharWidth,
	isEmojiPresentation as tsIsEmojiPresentation,
	isExtendedPictographic as tsIsExtendedPictographic,
	isRegionalIndicator as tsIsRegionalIndicator,
	isSkinToneModifier as tsIsSkinToneModifier,
	isVariationSelector as tsIsVariationSelector,
	isCombiningChar as tsIsCombiningChar,
	stringWidth as tsStringWidth,
} from "../unicode.ts";

describe("Cross-validation: full BMP (U+0000..U+FFFF)", () => {
	test("charWidth matches for entire BMP", () => {
		for (let cp = 0; cp <= 0xFFFF; cp++) {
			// Skip surrogate range (invalid codepoints)
			if (cp >= 0xD800 && cp <= 0xDFFF) continue;

			const ch = String.fromCodePoint(cp);
			const tsResult = tsCharWidth(ch);
			const wasmResult = wasmCharWidth(ch);
			if (tsResult !== wasmResult) {
				throw new Error(
					`charWidth mismatch at U+${cp.toString(16).toUpperCase().padStart(4, "0")}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isEmojiPresentation matches for entire BMP", () => {
		for (let cp = 0; cp <= 0xFFFF; cp++) {
			if (cp >= 0xD800 && cp <= 0xDFFF) continue;
			const tsResult = tsIsEmojiPresentation(cp);
			const wasmResult = wasmIsEmojiPresentation(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isEmojiPresentation mismatch at U+${cp.toString(16).toUpperCase().padStart(4, "0")}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isExtendedPictographic matches for entire BMP", () => {
		for (let cp = 0; cp <= 0xFFFF; cp++) {
			if (cp >= 0xD800 && cp <= 0xDFFF) continue;
			const tsResult = tsIsExtendedPictographic(cp);
			const wasmResult = wasmIsExtendedPictographic(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isExtendedPictographic mismatch at U+${cp.toString(16).toUpperCase().padStart(4, "0")}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isCombiningChar matches for entire BMP", () => {
		for (let cp = 0; cp <= 0xFFFF; cp++) {
			if (cp >= 0xD800 && cp <= 0xDFFF) continue;
			const tsResult = tsIsCombiningChar(cp);
			const wasmResult = wasmIsCombiningChar(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isCombiningChar mismatch at U+${cp.toString(16).toUpperCase().padStart(4, "0")}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isVariationSelector matches for entire BMP", () => {
		for (let cp = 0; cp <= 0xFFFF; cp++) {
			if (cp >= 0xD800 && cp <= 0xDFFF) continue;
			const tsResult = tsIsVariationSelector(cp);
			const wasmResult = wasmIsVariationSelector(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isVariationSelector mismatch at U+${cp.toString(16).toUpperCase().padStart(4, "0")}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});
});

describe("Cross-validation: SMP emoji blocks (U+1F000..U+1FFFF)", () => {
	test("charWidth matches for SMP emoji range", () => {
		for (let cp = 0x1F000; cp <= 0x1FFFF; cp++) {
			const ch = String.fromCodePoint(cp);
			const tsResult = tsCharWidth(ch);
			const wasmResult = wasmCharWidth(ch);
			if (tsResult !== wasmResult) {
				throw new Error(
					`charWidth mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isEmojiPresentation matches for SMP emoji range", () => {
		for (let cp = 0x1F000; cp <= 0x1FFFF; cp++) {
			const tsResult = tsIsEmojiPresentation(cp);
			const wasmResult = wasmIsEmojiPresentation(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isEmojiPresentation mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isExtendedPictographic matches for SMP emoji range", () => {
		for (let cp = 0x1F000; cp <= 0x1FFFF; cp++) {
			const tsResult = tsIsExtendedPictographic(cp);
			const wasmResult = wasmIsExtendedPictographic(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isExtendedPictographic mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isRegionalIndicator matches for SMP range", () => {
		for (let cp = 0x1F000; cp <= 0x1FFFF; cp++) {
			const tsResult = tsIsRegionalIndicator(cp);
			const wasmResult = wasmIsRegionalIndicator(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isRegionalIndicator mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isSkinToneModifier matches for SMP range", () => {
		for (let cp = 0x1F000; cp <= 0x1FFFF; cp++) {
			const tsResult = tsIsSkinToneModifier(cp);
			const wasmResult = wasmIsSkinToneModifier(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isSkinToneModifier mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});
});

describe("Cross-validation: VS Supplement (U+E0100..U+E01EF)", () => {
	test("charWidth matches for VS Supplement range", () => {
		for (let cp = 0xE0100; cp <= 0xE01EF; cp++) {
			const ch = String.fromCodePoint(cp);
			const tsResult = tsCharWidth(ch);
			const wasmResult = wasmCharWidth(ch);
			if (tsResult !== wasmResult) {
				throw new Error(
					`charWidth mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});

	test("isVariationSelector matches for VS Supplement range", () => {
		for (let cp = 0xE0100; cp <= 0xE01EF; cp++) {
			const tsResult = tsIsVariationSelector(cp);
			const wasmResult = wasmIsVariationSelector(cp);
			if (tsResult !== wasmResult) {
				throw new Error(
					`isVariationSelector mismatch at U+${cp.toString(16).toUpperCase()}: TS=${tsResult}, WASM=${wasmResult}`,
				);
			}
		}
	});
});

describe("Cross-validation: stringWidth", () => {
	const testStrings = [
		"hello",
		"漢字テスト",
		"a😀b",
		"abc漢字emoji😀🚀test",
		"",
		"   ",
		"\t\n",
		"Hello, 世界! 🌍",
	];

	for (const str of testStrings) {
		test(`stringWidth matches for "${str.slice(0, 20)}"`, () => {
			expect(wasmStringWidth(str)).toBe(tsStringWidth(str));
		});
	}
});
