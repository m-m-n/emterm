/**
 * SGR (Select Graphic Rendition) parameter parsing.
 *
 * This module parses SGR parameters from the raw numeric parameters
 * sent by the Rust backend and converts them to structured SgrAttr types.
 *
 * Note: The Rust backend could send pre-parsed SgrAttr, but for now
 * we parse them on the TypeScript side for flexibility.
 */

import type { SgrAttr } from "./attributes.ts";
import type { SgrColor } from "./colors.ts";

/**
 * Parse SGR parameters into SgrAttr array.
 *
 * @param params - Raw numeric parameters from CSI m sequence
 * @returns Array of parsed SGR attributes
 */
export function parseSgrParams(params: number[]): SgrAttr[] {
	const attrs: SgrAttr[] = [];

	// Empty params means reset
	if (params.length === 0) {
		attrs.push({ attr: "Reset" });
		return attrs;
	}

	let i = 0;
	while (i < params.length) {
		const param = params[i]!;

		switch (param) {
			// Reset
			case 0:
				attrs.push({ attr: "Reset" });
				break;

			// Text attributes
			case 1:
				attrs.push({ attr: "Bold" });
				break;
			case 2:
				attrs.push({ attr: "Dim" });
				break;
			case 3:
				attrs.push({ attr: "Italic" });
				break;
			case 4:
				attrs.push({ attr: "Underline" });
				break;
			case 5:
				attrs.push({ attr: "Blink" });
				break;
			case 7:
				attrs.push({ attr: "Reverse" });
				break;
			case 8:
				attrs.push({ attr: "Hidden" });
				break;
			case 9:
				attrs.push({ attr: "Strikethrough" });
				break;

			// Attribute resets
			case 22:
				attrs.push({ attr: "NormalIntensity" });
				break;
			case 23:
				attrs.push({ attr: "NotItalic" });
				break;
			case 24:
				attrs.push({ attr: "NotUnderline" });
				break;
			case 25:
				attrs.push({ attr: "NotBlink" });
				break;
			case 27:
				attrs.push({ attr: "NotReverse" });
				break;
			case 28:
				attrs.push({ attr: "NotHidden" });
				break;
			case 29:
				attrs.push({ attr: "NotStrikethrough" });
				break;

			// Standard foreground colors (30-37)
			case 30:
			case 31:
			case 32:
			case 33:
			case 34:
			case 35:
			case 36:
			case 37:
				attrs.push({
					attr: "Foreground",
					value: { type: "Standard", value: param - 30 },
				});
				break;

			// Extended foreground color
			case 38: {
				const [color, consumed] = parseExtendedColor(params, i + 1);
				if (color) {
					attrs.push({ attr: "Foreground", value: color });
				}
				i += consumed;
				break;
			}

			// Default foreground
			case 39:
				attrs.push({ attr: "DefaultForeground" });
				break;

			// Standard background colors (40-47)
			case 40:
			case 41:
			case 42:
			case 43:
			case 44:
			case 45:
			case 46:
			case 47:
				attrs.push({
					attr: "Background",
					value: { type: "Standard", value: param - 40 },
				});
				break;

			// Extended background color
			case 48: {
				const [color, consumed] = parseExtendedColor(params, i + 1);
				if (color) {
					attrs.push({ attr: "Background", value: color });
				}
				i += consumed;
				break;
			}

			// Default background
			case 49:
				attrs.push({ attr: "DefaultBackground" });
				break;

			// Bright foreground colors (90-97)
			case 90:
			case 91:
			case 92:
			case 93:
			case 94:
			case 95:
			case 96:
			case 97:
				attrs.push({
					attr: "Foreground",
					value: { type: "Bright", value: param - 90 },
				});
				break;

			// Bright background colors (100-107)
			case 100:
			case 101:
			case 102:
			case 103:
			case 104:
			case 105:
			case 106:
			case 107:
				attrs.push({
					attr: "Background",
					value: { type: "Bright", value: param - 100 },
				});
				break;

			// Unknown parameters are ignored
			default:
				break;
		}

		i++;
	}

	return attrs;
}

/**
 * Parse extended color (256-color or RGB) from parameters.
 *
 * @param params - Full parameter array
 * @param startIndex - Index to start parsing from
 * @returns Tuple of [color or null, number of params consumed]
 */
function parseExtendedColor(
	params: number[],
	startIndex: number,
): [SgrColor | null, number] {
	if (startIndex >= params.length) {
		return [null, 0];
	}

	const mode = params[startIndex];

	// 256-color mode (5;n)
	if (mode === 5) {
		if (startIndex + 1 < params.length) {
			const index = params[startIndex + 1]!;
			return [{ type: "Indexed", value: index }, 2];
		}
		return [{ type: "Indexed", value: 0 }, 1];
	}

	// RGB mode (2;r;g;b)
	if (mode === 2) {
		const r = startIndex + 1 < params.length ? params[startIndex + 1]! : 0;
		const g = startIndex + 2 < params.length ? params[startIndex + 2]! : 0;
		const b = startIndex + 3 < params.length ? params[startIndex + 3]! : 0;
		const consumed = Math.min(4, params.length - startIndex);
		return [{ type: "Rgb", value: { r, g, b } }, consumed];
	}

	return [null, 1];
}
