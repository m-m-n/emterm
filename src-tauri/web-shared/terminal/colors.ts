/**
 * Terminal color palette and color utilities.
 *
 * Provides the standard terminal color palette (16 + 256 + RGB)
 * and utilities for converting colors to CSS values.
 */

/**
 * RGB color value.
 */
export interface Rgb {
	r: number;
	g: number;
	b: number;
}

/**
 * Standard 16 terminal colors (indices 0-15).
 * WezTerm color scheme.
 *
 * Colors 0-7 are the standard colors.
 * Colors 8-15 are the bright variants.
 */
export const PALETTE_16: readonly Rgb[] = Object.freeze([
	// Standard colors (0-7) - WezTerm scheme
	{ r: 0x00, g: 0x00, b: 0x00 }, // 0: Black (#000000)
	{ r: 0xff, g: 0x00, b: 0x00 }, // 1: Red (#ff0000)
	{ r: 0x00, g: 0xdd, b: 0x00 }, // 2: Green (#00dd00)
	{ r: 0xee, g: 0xee, b: 0x00 }, // 3: Yellow (#eeee00)
	{ r: 0x40, g: 0x40, b: 0xff }, // 4: Blue (#4040ff)
	{ r: 0xff, g: 0x00, b: 0xff }, // 5: Magenta (#ff00ff)
	{ r: 0x00, g: 0xdd, b: 0xdd }, // 6: Cyan (#00dddd)
	{ r: 0xde, g: 0xda, b: 0xcf }, // 7: White (#dedacf)

	// Bright colors (8-15) - WezTerm scheme
	{ r: 0x55, g: 0x55, b: 0x55 }, // 8: Bright Black (#555555)
	{ r: 0xff, g: 0x60, b: 0x60 }, // 9: Bright Red (#ff6060)
	{ r: 0x60, g: 0xff, b: 0x60 }, // 10: Bright Green (#60ff60)
	{ r: 0xff, g: 0xff, b: 0x60 }, // 11: Bright Yellow (#ffff60)
	{ r: 0x60, g: 0x60, b: 0xff }, // 12: Bright Blue (#6060ff)
	{ r: 0xff, g: 0x60, b: 0xff }, // 13: Bright Magenta (#ff60ff)
	{ r: 0x60, g: 0xff, b: 0xff }, // 14: Bright Cyan (#60ffff)
	{ r: 0xff, g: 0xff, b: 0xff }, // 15: Bright White (#ffffff)
]);

/**
 * Full 256-color palette.
 *
 * - Indices 0-15: Standard colors (same as PALETTE_16)
 * - Indices 16-231: 6x6x6 color cube
 * - Indices 232-255: Grayscale ramp
 */
export const PALETTE_256: readonly Rgb[] = Object.freeze(generate256Palette());

/**
 * Generate the full 256-color palette.
 */
function generate256Palette(): Rgb[] {
	const palette: Rgb[] = [];

	// Copy standard 16 colors
	for (const color of PALETTE_16) {
		palette.push({ ...color });
	}

	// 6x6x6 color cube (indices 16-231)
	const cubeValues = [0, 95, 135, 175, 215, 255];
	for (let r = 0; r < 6; r++) {
		for (let g = 0; g < 6; g++) {
			for (let b = 0; b < 6; b++) {
				palette.push({
					r: cubeValues[r]!,
					g: cubeValues[g]!,
					b: cubeValues[b]!,
				});
			}
		}
	}

	// Grayscale ramp (indices 232-255)
	// 24 steps from dark to light (8, 18, 28, ... 238)
	for (let i = 0; i < 24; i++) {
		const gray = 8 + i * 10;
		palette.push({ r: gray, g: gray, b: gray });
	}

	return palette;
}

/**
 * Convert a 256-color palette index to RGB.
 *
 * @param index - Palette index (0-255)
 * @returns RGB color value
 */
export function indexToRgb(index: number): Rgb {
	if (index < 0 || index > 255) {
		// Return black for invalid indices
		return { r: 0, g: 0, b: 0 };
	}
	return PALETTE_256[index]!;
}

/**
 * Build a full 256-color palette with custom ANSI 16 colors.
 *
 * Replaces the first 16 entries of PALETTE_256 with the given ANSI colors,
 * keeping indices 16-255 (color cube + grayscale) from the static palette.
 *
 * @param ansi16 - Custom 16-color ANSI palette
 * @returns Full 256-color palette
 */
export function buildPalette256(ansi16: readonly Rgb[]): Rgb[] {
	const palette: Rgb[] = [];
	for (let i = 0; i < 16; i++) {
		palette.push(ansi16[i] ?? PALETTE_256[i]!);
	}
	for (let i = 16; i < 256; i++) {
		palette.push(PALETTE_256[i]!);
	}
	return palette;
}

/**
 * Convert a standard color index (0-7) to RGB.
 *
 * @param index - Standard color index (0-7)
 * @returns RGB color value
 */
export function standardColorToRgb(index: number): Rgb {
	if (index < 0 || index > 7) {
		return { r: 0, g: 0, b: 0 };
	}
	return PALETTE_16[index]!;
}

/**
 * Convert a bright color index (0-7) to RGB.
 *
 * @param index - Bright color index (0-7)
 * @returns RGB color value (corresponds to palette indices 8-15)
 */
export function brightColorToRgb(index: number): Rgb {
	if (index < 0 || index > 7) {
		return { r: 0, g: 0, b: 0 };
	}
	return PALETTE_16[index + 8]!;
}

/**
 * Convert an RGB color to CSS color string.
 *
 * @param rgb - RGB color value
 * @returns CSS color string (e.g., "rgb(255, 0, 0)")
 */
export function rgbToCSS(rgb: Rgb): string {
	return `rgb(${rgb.r}, ${rgb.g}, ${rgb.b})`;
}

/**
 * Convert a palette index to CSS color string.
 *
 * @param index - Palette index (0-255)
 * @returns CSS color string
 */
export function indexToCSS(index: number): string {
	return rgbToCSS(indexToRgb(index));
}

/**
 * Default foreground color (WezTerm: bright green #40ff40).
 */
export const DEFAULT_FOREGROUND: Rgb = { r: 0x40, g: 0xff, b: 0x40 };

/**
 * Default background color.
 */
export const DEFAULT_BACKGROUND: Rgb = { r: 0, g: 0, b: 0 };

/**
 * Color type from the Rust parser (matches SgrAttr color values).
 */
export type SgrColor =
	| { type: "Standard"; value: number }
	| { type: "Bright"; value: number }
	| { type: "Indexed"; value: number }
	| { type: "Rgb"; value: { r: number; g: number; b: number } };

/**
 * Convert an SGR color from the parser to RGB.
 *
 * @param color - SGR color from parser
 * @returns RGB color value
 */
export function sgrColorToRgb(color: SgrColor): Rgb {
	switch (color.type) {
		case "Standard":
			return standardColorToRgb(color.value);
		case "Bright":
			return brightColorToRgb(color.value);
		case "Indexed":
			return indexToRgb(color.value);
		case "Rgb":
			return { r: color.value.r, g: color.value.g, b: color.value.b };
	}
}

/**
 * Color scheme preset definition.
 */
export interface ColorSchemePreset {
	name: string;
	foreground: Rgb;
	background: Rgb;
	cursor: Rgb;
	selection: Rgb;
	ansiColors: Rgb[];
}

/**
 * Color scheme presets.
 */
export const COLOR_SCHEME_PRESETS: readonly ColorSchemePreset[] = Object.freeze([
	{
		name: "emterm",
		foreground: DEFAULT_FOREGROUND,
		background: DEFAULT_BACKGROUND,
		cursor: { r: 0, g: 128, b: 0 },
		selection: { r: 50, g: 150, b: 250 },
		ansiColors: [...PALETTE_16],
	},
	{
		name: "solarized-dark",
		foreground: { r: 0x83, g: 0x94, b: 0x96 },
		background: { r: 0x00, g: 0x2b, b: 0x36 },
		cursor: { r: 0x83, g: 0x94, b: 0x96 },
		selection: { r: 0x07, g: 0x36, b: 0x42 },
		ansiColors: [
			{ r: 0x07, g: 0x36, b: 0x42 }, // black
			{ r: 0xdc, g: 0x32, b: 0x2f }, // red
			{ r: 0x85, g: 0x99, b: 0x00 }, // green
			{ r: 0xb5, g: 0x89, b: 0x00 }, // yellow
			{ r: 0x26, g: 0x8b, b: 0xd2 }, // blue
			{ r: 0xd3, g: 0x36, b: 0x82 }, // magenta
			{ r: 0x2a, g: 0xa1, b: 0x98 }, // cyan
			{ r: 0xee, g: 0xe8, b: 0xd5 }, // white
			{ r: 0x00, g: 0x2b, b: 0x36 }, // bright black
			{ r: 0xcb, g: 0x4b, b: 0x16 }, // bright red
			{ r: 0x58, g: 0x6e, b: 0x75 }, // bright green
			{ r: 0x65, g: 0x7b, b: 0x83 }, // bright yellow
			{ r: 0x83, g: 0x94, b: 0x96 }, // bright blue
			{ r: 0x6c, g: 0x71, b: 0xc4 }, // bright magenta
			{ r: 0x93, g: 0xa1, b: 0xa1 }, // bright cyan
			{ r: 0xfd, g: 0xf6, b: 0xe3 }, // bright white
		],
	},
	{
		name: "solarized-light",
		foreground: { r: 0x65, g: 0x7b, b: 0x83 },
		background: { r: 0xfd, g: 0xf6, b: 0xe3 },
		cursor: { r: 0x65, g: 0x7b, b: 0x83 },
		selection: { r: 0xee, g: 0xe8, b: 0xd5 },
		ansiColors: [
			{ r: 0x07, g: 0x36, b: 0x42 }, // black
			{ r: 0xdc, g: 0x32, b: 0x2f }, // red
			{ r: 0x85, g: 0x99, b: 0x00 }, // green
			{ r: 0xb5, g: 0x89, b: 0x00 }, // yellow
			{ r: 0x26, g: 0x8b, b: 0xd2 }, // blue
			{ r: 0xd3, g: 0x36, b: 0x82 }, // magenta
			{ r: 0x2a, g: 0xa1, b: 0x98 }, // cyan
			{ r: 0xee, g: 0xe8, b: 0xd5 }, // white
			{ r: 0x00, g: 0x2b, b: 0x36 }, // bright black
			{ r: 0xcb, g: 0x4b, b: 0x16 }, // bright red
			{ r: 0x58, g: 0x6e, b: 0x75 }, // bright green
			{ r: 0x65, g: 0x7b, b: 0x83 }, // bright yellow
			{ r: 0x83, g: 0x94, b: 0x96 }, // bright blue
			{ r: 0x6c, g: 0x71, b: 0xc4 }, // bright magenta
			{ r: 0x93, g: 0xa1, b: 0xa1 }, // bright cyan
			{ r: 0xfd, g: 0xf6, b: 0xe3 }, // bright white
		],
	},
	{
		name: "monokai",
		foreground: { r: 0xf8, g: 0xf8, b: 0xf2 },
		background: { r: 0x27, g: 0x28, b: 0x22 },
		cursor: { r: 0xf8, g: 0xf8, b: 0xf0 },
		selection: { r: 0x49, g: 0x48, b: 0x3e },
		ansiColors: [
			{ r: 0x27, g: 0x28, b: 0x22 }, // black
			{ r: 0xf9, g: 0x26, b: 0x72 }, // red
			{ r: 0xa6, g: 0xe2, b: 0x2e }, // green
			{ r: 0xf4, g: 0xbf, b: 0x75 }, // yellow
			{ r: 0x66, g: 0xd9, b: 0xef }, // blue
			{ r: 0xae, g: 0x81, b: 0xff }, // magenta
			{ r: 0xa1, g: 0xef, b: 0xe4 }, // cyan
			{ r: 0xf8, g: 0xf8, b: 0xf2 }, // white
			{ r: 0x75, g: 0x71, b: 0x5e }, // bright black
			{ r: 0xf9, g: 0x26, b: 0x72 }, // bright red
			{ r: 0xa6, g: 0xe2, b: 0x2e }, // bright green
			{ r: 0xf4, g: 0xbf, b: 0x75 }, // bright yellow
			{ r: 0x66, g: 0xd9, b: 0xef }, // bright blue
			{ r: 0xae, g: 0x81, b: 0xff }, // bright magenta
			{ r: 0xa1, g: 0xef, b: 0xe4 }, // bright cyan
			{ r: 0xf9, g: 0xf8, b: 0xf5 }, // bright white
		],
	},
	{
		name: "dracula",
		foreground: { r: 0xf8, g: 0xf8, b: 0xf2 },
		background: { r: 0x28, g: 0x2a, b: 0x36 },
		cursor: { r: 0xf8, g: 0xf8, b: 0xf2 },
		selection: { r: 0x44, g: 0x47, b: 0x5a },
		ansiColors: [
			{ r: 0x21, g: 0x22, b: 0x2c }, // black
			{ r: 0xff, g: 0x55, b: 0x55 }, // red
			{ r: 0x50, g: 0xfa, b: 0x7b }, // green
			{ r: 0xf1, g: 0xfa, b: 0x8c }, // yellow
			{ r: 0xbd, g: 0x93, b: 0xf9 }, // blue
			{ r: 0xff, g: 0x79, b: 0xc6 }, // magenta
			{ r: 0x8b, g: 0xe9, b: 0xfd }, // cyan
			{ r: 0xf8, g: 0xf8, b: 0xf2 }, // white
			{ r: 0x6c, g: 0x71, b: 0xc4 }, // bright black
			{ r: 0xff, g: 0x66, b: 0x66 }, // bright red
			{ r: 0x69, g: 0xff, b: 0x94 }, // bright green
			{ r: 0xff, g: 0xff, b: 0xb6 }, // bright yellow
			{ r: 0xd6, g: 0xac, b: 0xff }, // bright blue
			{ r: 0xff, g: 0x92, b: 0xdf }, // bright magenta
			{ r: 0xa4, g: 0xff, b: 0xff }, // bright cyan
			{ r: 0xff, g: 0xff, b: 0xff }, // bright white
		],
	},
	{
		name: "nord",
		foreground: { r: 0xd8, g: 0xde, b: 0xe9 },
		background: { r: 0x2e, g: 0x34, b: 0x40 },
		cursor: { r: 0xd8, g: 0xde, b: 0xe9 },
		selection: { r: 0x4c, g: 0x56, b: 0x6a },
		ansiColors: [
			{ r: 0x3b, g: 0x42, b: 0x52 }, // black
			{ r: 0xbf, g: 0x61, b: 0x6a }, // red
			{ r: 0xa3, g: 0xbe, b: 0x8c }, // green
			{ r: 0xeb, g: 0xcb, b: 0x8b }, // yellow
			{ r: 0x81, g: 0xa1, b: 0xc1 }, // blue
			{ r: 0xb4, g: 0x8e, b: 0xad }, // magenta
			{ r: 0x88, g: 0xc0, b: 0xd0 }, // cyan
			{ r: 0xe5, g: 0xe9, b: 0xf0 }, // white
			{ r: 0x4c, g: 0x56, b: 0x6a }, // bright black
			{ r: 0xbf, g: 0x61, b: 0x6a }, // bright red
			{ r: 0xa3, g: 0xbe, b: 0x8c }, // bright green
			{ r: 0xeb, g: 0xcb, b: 0x8b }, // bright yellow
			{ r: 0x81, g: 0xa1, b: 0xc1 }, // bright blue
			{ r: 0xb4, g: 0x8e, b: 0xad }, // bright magenta
			{ r: 0x8f, g: 0xbc, b: 0xbb }, // bright cyan
			{ r: 0xec, g: 0xef, b: 0xf4 }, // bright white
		],
	},
]);

/**
 * Get a color scheme preset by name.
 *
 * @param name - Preset name
 * @returns Color scheme preset or undefined if not found
 */
export function getColorSchemePreset(name: string): ColorSchemePreset | undefined {
	return COLOR_SCHEME_PRESETS.find((preset) => preset.name === name);
}

// ============================================================
// Hex Color Conversion Utilities
// ============================================================

/** Regex for validating #RRGGBB hex color format */
const HEX_COLOR_REGEX = /^#[0-9a-fA-F]{6}$/;

/**
 * Validate a hex color string.
 *
 * @param hex - Color string to validate
 * @returns True if the string is a valid #RRGGBB format
 */
export function validateHexColor(hex: string): boolean {
	return HEX_COLOR_REGEX.test(hex);
}

/**
 * Convert a hex color string to RGB.
 *
 * @param hex - Hex color string in #RRGGBB format
 * @returns RGB color value or null if invalid
 */
export function hexToRgb(hex: string): Rgb | null {
	if (!validateHexColor(hex)) {
		return null;
	}
	const r = parseInt(hex.slice(1, 3), 16);
	const g = parseInt(hex.slice(3, 5), 16);
	const b = parseInt(hex.slice(5, 7), 16);
	return { r, g, b };
}

/**
 * Convert an RGB color to hex string.
 *
 * @param rgb - RGB color value
 * @returns Hex color string in #rrggbb format (lowercase)
 */
export function rgbToHex(rgb: Rgb): string {
	const r = rgb.r.toString(16).padStart(2, "0");
	const g = rgb.g.toString(16).padStart(2, "0");
	const b = rgb.b.toString(16).padStart(2, "0");
	return `#${r}${g}${b}`;
}
