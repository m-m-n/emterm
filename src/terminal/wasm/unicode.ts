/**
 * WASM-backed Unicode character width calculation.
 *
 * TypeScript glue that wraps the Rust/WASM unicode module.
 * Provides the same API surface as the original unicode.ts.
 */

import {
	classify_codepoint as wasm_classify_codepoint,
	classify_codepoints as wasm_classify_codepoints,
	char_width as wasm_char_width,
	string_width as wasm_string_width,
	is_emoji_presentation as wasm_is_emoji_presentation,
	is_extended_pictographic as wasm_is_extended_pictographic,
	is_regional_indicator as wasm_is_regional_indicator,
	is_skin_tone_modifier as wasm_is_skin_tone_modifier,
	is_variation_selector as wasm_is_variation_selector,
	is_combining_char as wasm_is_combining_char,
	is_ambiguous_width as wasm_is_ambiguous_width,
} from "../../../wasm/pkg/emterm_wasm.js";

// Bit flag constants for packed byte layout
export const WIDTH_MASK = 0b0000_0011;
export const COMBINING = 0b0000_0100;
export const EMOJI_PRES = 0b0000_1000;
export const EXT_PICTOGRAPHIC = 0b0001_0000;
export const REGIONAL_IND = 0b0010_0000;
export const SKIN_TONE = 0b0100_0000;
export const VARIATION_SEL = 0b1000_0000;

/**
 * Get the display width of a character in terminal cells.
 *
 * @param char - A single character (or first character of string)
 * @returns 0 for control/combining/zero-width chars, 1 for narrow, 2 for wide/emoji
 */
export function charWidth(char: string): number {
	if (char.length === 0) return 0;
	const cp = char.codePointAt(0);
	if (cp === undefined) return 0;
	return wasm_char_width(cp);
}

/**
 * Check if a character is wide (takes 2 cells).
 */
export function isWideChar(char: string): boolean {
	return charWidth(char) === 2;
}

/**
 * Calculate the display width of a string.
 */
export function stringWidth(str: string): number {
	return wasm_string_width(str);
}

/**
 * Pack all Unicode properties into a single byte for a codepoint.
 */
export function classifyCodepoint(cp: number): number {
	return wasm_classify_codepoint(cp);
}

/**
 * Classify all codepoints in a string, returning a packed byte per codepoint.
 */
export function classifyCodepoints(text: string): Uint8Array {
	return wasm_classify_codepoints(text);
}

// Individual property checks (direct WASM wrappers)

export function isEmojiPresentation(cp: number): boolean {
	return wasm_is_emoji_presentation(cp);
}

export function isExtendedPictographic(cp: number): boolean {
	return wasm_is_extended_pictographic(cp);
}

export function isRegionalIndicator(cp: number): boolean {
	return wasm_is_regional_indicator(cp);
}

export function isSkinToneModifier(cp: number): boolean {
	return wasm_is_skin_tone_modifier(cp);
}

export function isVariationSelector(cp: number): boolean {
	return wasm_is_variation_selector(cp);
}

export function isCombiningChar(cp: number): boolean {
	return wasm_is_combining_char(cp);
}

export function isAmbiguousWidth(cp: number): boolean {
	return wasm_is_ambiguous_width(cp);
}
