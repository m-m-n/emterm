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
 *
 * Colors 0-7 are the standard colors.
 * Colors 8-15 are the bright variants.
 */
export const PALETTE_16: readonly Rgb[] = Object.freeze([
  // Standard colors (0-7)
  { r: 0, g: 0, b: 0 }, // 0: Black
  { r: 205, g: 49, b: 49 }, // 1: Red
  { r: 13, g: 188, b: 121 }, // 2: Green
  { r: 229, g: 229, b: 16 }, // 3: Yellow
  { r: 36, g: 114, b: 200 }, // 4: Blue
  { r: 188, g: 63, b: 188 }, // 5: Magenta
  { r: 17, g: 168, b: 205 }, // 6: Cyan
  { r: 229, g: 229, b: 229 }, // 7: White

  // Bright colors (8-15)
  { r: 102, g: 102, b: 102 }, // 8: Bright Black (Gray)
  { r: 241, g: 76, b: 76 }, // 9: Bright Red
  { r: 35, g: 209, b: 139 }, // 10: Bright Green
  { r: 245, g: 245, b: 67 }, // 11: Bright Yellow
  { r: 59, g: 142, b: 234 }, // 12: Bright Blue
  { r: 214, g: 112, b: 214 }, // 13: Bright Magenta
  { r: 41, g: 184, b: 219 }, // 14: Bright Cyan
  { r: 255, g: 255, b: 255 }, // 15: Bright White
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
 * Default foreground color.
 */
export const DEFAULT_FOREGROUND: Rgb = { r: 229, g: 229, b: 229 };

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
