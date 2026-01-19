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
