/**
 * Cell attributes for terminal styling.
 *
 * Defines text styling attributes like bold, colors, etc.
 */

import {
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	indexToRgb,
	type Rgb,
	type SgrColor,
	sgrColorToRgb,
} from "./colors.ts";

/**
 * Color type for terminal colors.
 */
export type Color =
	| { type: "default" }
	| { type: "indexed"; index: number }
	| { type: "rgb"; r: number; g: number; b: number };

/**
 * Cell styling attributes.
 */
export interface CellAttributes {
	bold: boolean;
	dim: boolean;
	italic: boolean;
	underline: boolean;
	blink: boolean;
	reverse: boolean;
	hidden: boolean;
	strikethrough: boolean;
	fg: Color | null;
	bg: Color | null;
}

/**
 * Default attributes for new cells.
 */
export const DEFAULT_ATTRIBUTES: Readonly<CellAttributes> = Object.freeze({
	bold: false,
	dim: false,
	italic: false,
	underline: false,
	blink: false,
	reverse: false,
	hidden: false,
	strikethrough: false,
	fg: null,
	bg: null,
});

/**
 * Create a new CellAttributes object with default values.
 */
export function createDefaultAttributes(): CellAttributes {
	return {
		bold: false,
		dim: false,
		italic: false,
		underline: false,
		blink: false,
		reverse: false,
		hidden: false,
		strikethrough: false,
		fg: null,
		bg: null,
	};
}

/**
 * Check if two Color values are equal.
 */
function colorsEqual(a: Color | null, b: Color | null): boolean {
	if (a === null && b === null) return true;
	if (a === null || b === null) return false;
	if (a.type !== b.type) return false;

	if (a.type === "default" && b.type === "default") return true;
	if (a.type === "indexed" && b.type === "indexed") return a.index === b.index;
	if (a.type === "rgb" && b.type === "rgb") {
		return a.r === b.r && a.g === b.g && a.b === b.b;
	}

	return false;
}

/**
 * Clone a Color value.
 */
function cloneColor(color: Color | null): Color | null {
	if (color === null) return null;
	if (color.type === "default") return { type: "default" };
	if (color.type === "indexed") return { type: "indexed", index: color.index };
	if (color.type === "rgb") {
		return { type: "rgb", r: color.r, g: color.g, b: color.b };
	}
	return null;
}

/**
 * Check if two CellAttributes are equal.
 */
export function attributesEqual(a: CellAttributes, b: CellAttributes): boolean {
	return (
		a.bold === b.bold &&
		a.dim === b.dim &&
		a.italic === b.italic &&
		a.underline === b.underline &&
		a.blink === b.blink &&
		a.reverse === b.reverse &&
		a.hidden === b.hidden &&
		a.strikethrough === b.strikethrough &&
		colorsEqual(a.fg, b.fg) &&
		colorsEqual(a.bg, b.bg)
	);
}

/**
 * Create a deep clone of CellAttributes.
 */
export function cloneAttributes(attrs: CellAttributes): CellAttributes {
	return {
		bold: attrs.bold,
		dim: attrs.dim,
		italic: attrs.italic,
		underline: attrs.underline,
		blink: attrs.blink,
		reverse: attrs.reverse,
		hidden: attrs.hidden,
		strikethrough: attrs.strikethrough,
		fg: cloneColor(attrs.fg),
		bg: cloneColor(attrs.bg),
	};
}

// ── WASM pack/unpack utilities ──────────────────────────

/** Style flag constants matching Rust STYLE_* in cell.rs */
const STYLE_BOLD = 0x0001;
const STYLE_DIM = 0x0002;
const STYLE_ITALIC = 0x0004;
const STYLE_UNDERLINE = 0x0008;
const STYLE_BLINK = 0x0010;
const STYLE_REVERSE = 0x0020;
const STYLE_HIDDEN = 0x0040;
const STYLE_STRIKETHROUGH = 0x0080;

/** Pack a Color into WASM-friendly components: {tag, r, g, b} */
export function packColor(color: Color | null): {
	tag: number;
	r: number;
	g: number;
	b: number;
} {
	if (color === null || color.type === "default") {
		return { tag: 0, r: 0, g: 0, b: 0 };
	}
	if (color.type === "indexed") {
		return { tag: 1, r: color.index, g: 0, b: 0 };
	}
	// rgb
	return { tag: 2, r: color.r, g: color.g, b: color.b };
}

/** Unpack a u32 (tag<<24 | r<<16 | g<<8 | b) into a Color or null */
export function unpackColor(packed: number): Color | null {
	const tag = (packed >>> 24) & 0xff;
	if (tag === 0) return null;
	const r = (packed >>> 16) & 0xff;
	if (tag === 1) return { type: "indexed", index: r };
	const g = (packed >>> 8) & 0xff;
	const b = packed & 0xff;
	return { type: "rgb", r, g, b };
}

/** Pack CellAttributes style booleans into a u16 bitfield */
export function packStyleFlags(attrs: CellAttributes): number {
	let flags = 0;
	if (attrs.bold) flags |= STYLE_BOLD;
	if (attrs.dim) flags |= STYLE_DIM;
	if (attrs.italic) flags |= STYLE_ITALIC;
	if (attrs.underline) flags |= STYLE_UNDERLINE;
	if (attrs.blink) flags |= STYLE_BLINK;
	if (attrs.reverse) flags |= STYLE_REVERSE;
	if (attrs.hidden) flags |= STYLE_HIDDEN;
	if (attrs.strikethrough) flags |= STYLE_STRIKETHROUGH;
	return flags;
}

/** Unpack a u16 bitfield into CellAttributes style booleans (partial) */
export function unpackStyleFlags(flags: number): Pick<
	CellAttributes,
	| "bold"
	| "dim"
	| "italic"
	| "underline"
	| "blink"
	| "reverse"
	| "hidden"
	| "strikethrough"
> {
	return {
		bold: (flags & STYLE_BOLD) !== 0,
		dim: (flags & STYLE_DIM) !== 0,
		italic: (flags & STYLE_ITALIC) !== 0,
		underline: (flags & STYLE_UNDERLINE) !== 0,
		blink: (flags & STYLE_BLINK) !== 0,
		reverse: (flags & STYLE_REVERSE) !== 0,
		hidden: (flags & STYLE_HIDDEN) !== 0,
		strikethrough: (flags & STYLE_STRIKETHROUGH) !== 0,
	};
}

/**
 * SGR attribute from the Rust parser.
 */
export type SgrAttr =
	| { attr: "Reset" }
	| { attr: "Bold" }
	| { attr: "Dim" }
	| { attr: "Italic" }
	| { attr: "Underline" }
	| { attr: "Blink" }
	| { attr: "Reverse" }
	| { attr: "Hidden" }
	| { attr: "Strikethrough" }
	| { attr: "NormalIntensity" }
	| { attr: "NotItalic" }
	| { attr: "NotUnderline" }
	| { attr: "NotBlink" }
	| { attr: "NotReverse" }
	| { attr: "NotHidden" }
	| { attr: "NotStrikethrough" }
	| { attr: "Foreground"; value: SgrColor }
	| { attr: "Background"; value: SgrColor }
	| { attr: "DefaultForeground" }
	| { attr: "DefaultBackground" };

/**
 * Convert an SgrColor to our Color type.
 */
function sgrColorToColor(color: SgrColor): Color {
	const rgb = sgrColorToRgb(color);
	return { type: "rgb", r: rgb.r, g: rgb.g, b: rgb.b };
}

/**
 * Apply a single SGR attribute to CellAttributes.
 *
 * Mutates the attributes in place.
 *
 * @param attrs - The attributes to modify
 * @param sgrAttr - The SGR attribute to apply
 */
export function applySgrAttr(attrs: CellAttributes, sgrAttr: SgrAttr): void {
	switch (sgrAttr.attr) {
		case "Reset":
			attrs.bold = false;
			attrs.dim = false;
			attrs.italic = false;
			attrs.underline = false;
			attrs.blink = false;
			attrs.reverse = false;
			attrs.hidden = false;
			attrs.strikethrough = false;
			attrs.fg = null;
			attrs.bg = null;
			break;

		case "Bold":
			attrs.bold = true;
			break;

		case "Dim":
			attrs.dim = true;
			break;

		case "Italic":
			attrs.italic = true;
			break;

		case "Underline":
			attrs.underline = true;
			break;

		case "Blink":
			attrs.blink = true;
			break;

		case "Reverse":
			attrs.reverse = true;
			break;

		case "Hidden":
			attrs.hidden = true;
			break;

		case "Strikethrough":
			attrs.strikethrough = true;
			break;

		case "NormalIntensity":
			attrs.bold = false;
			attrs.dim = false;
			break;

		case "NotItalic":
			attrs.italic = false;
			break;

		case "NotUnderline":
			attrs.underline = false;
			break;

		case "NotBlink":
			attrs.blink = false;
			break;

		case "NotReverse":
			attrs.reverse = false;
			break;

		case "NotHidden":
			attrs.hidden = false;
			break;

		case "NotStrikethrough":
			attrs.strikethrough = false;
			break;

		case "Foreground":
			attrs.fg = sgrColorToColor(sgrAttr.value);
			break;

		case "Background":
			attrs.bg = sgrColorToColor(sgrAttr.value);
			break;

		case "DefaultForeground":
			attrs.fg = null;
			break;

		case "DefaultBackground":
			attrs.bg = null;
			break;
	}
}

/**
 * Apply an array of SGR attributes to CellAttributes.
 *
 * @param attrs - The attributes to modify
 * @param sgrAttrs - Array of SGR attributes to apply
 */
export function applySgrAttrs(
	attrs: CellAttributes,
	sgrAttrs: SgrAttr[],
): void {
	for (const sgrAttr of sgrAttrs) {
		applySgrAttr(attrs, sgrAttr);
	}
}

/**
 * Convert a Color value to RGB.
 * Handles indexed colors via palette lookup and RGB passthrough.
 */
function colorToRgb(color: Color): Rgb {
	if (color.type === "indexed") {
		return indexToRgb(color.index);
	}
	if (color.type === "rgb") {
		return { r: color.r, g: color.g, b: color.b };
	}
	// "default" type - shouldn't reach here but return black as fallback
	return { r: 0, g: 0, b: 0 };
}

/**
 * Get the effective foreground RGB color for rendering.
 *
 * Takes into account the reverse attribute.
 *
 * @param attrs - Cell attributes
 * @param defaultFg - Default foreground color (from current theme), defaults to DEFAULT_FOREGROUND
 * @param defaultBg - Default background color (from current theme), defaults to DEFAULT_BACKGROUND
 * @returns RGB color for foreground
 */
export function getEffectiveForeground(
	attrs: CellAttributes,
	defaultFg: Rgb = DEFAULT_FOREGROUND,
	defaultBg: Rgb = DEFAULT_BACKGROUND,
): Rgb {
	if (attrs.reverse) {
		return attrs.bg ? colorToRgb(attrs.bg) : defaultBg;
	}
	return attrs.fg ? colorToRgb(attrs.fg) : defaultFg;
}

/**
 * Get the effective background RGB color for rendering.
 *
 * Takes into account the reverse attribute.
 *
 * @param attrs - Cell attributes
 * @param defaultFg - Default foreground color (from current theme), defaults to DEFAULT_FOREGROUND
 * @returns RGB color for background, or null to use default/transparent
 */
export function getEffectiveBackground(
	attrs: CellAttributes,
	defaultFg: Rgb = DEFAULT_FOREGROUND,
): Rgb | null {
	if (attrs.reverse) {
		return attrs.fg ? colorToRgb(attrs.fg) : defaultFg;
	}
	return attrs.bg ? colorToRgb(attrs.bg) : null;
}
