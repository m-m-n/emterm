/**
 * Cell attributes for terminal styling.
 *
 * Defines text styling attributes like bold, colors, etc.
 */

import {
  type Rgb,
  type SgrColor,
  sgrColorToRgb,
  DEFAULT_FOREGROUND,
  DEFAULT_BACKGROUND,
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
export function applySgrAttrs(attrs: CellAttributes, sgrAttrs: SgrAttr[]): void {
  for (const sgrAttr of sgrAttrs) {
    applySgrAttr(attrs, sgrAttr);
  }
}

/**
 * Get the effective foreground RGB color for rendering.
 *
 * Takes into account the reverse attribute.
 *
 * @param attrs - Cell attributes
 * @returns RGB color for foreground
 */
export function getEffectiveForeground(attrs: CellAttributes): Rgb {
  if (attrs.reverse) {
    return attrs.bg
      ? { r: (attrs.bg as { r: number; g: number; b: number }).r, g: (attrs.bg as { r: number; g: number; b: number }).g, b: (attrs.bg as { r: number; g: number; b: number }).b }
      : DEFAULT_BACKGROUND;
  }
  return attrs.fg
    ? { r: (attrs.fg as { r: number; g: number; b: number }).r, g: (attrs.fg as { r: number; g: number; b: number }).g, b: (attrs.fg as { r: number; g: number; b: number }).b }
    : DEFAULT_FOREGROUND;
}

/**
 * Get the effective background RGB color for rendering.
 *
 * Takes into account the reverse attribute.
 *
 * @param attrs - Cell attributes
 * @returns RGB color for background
 */
export function getEffectiveBackground(attrs: CellAttributes): Rgb | null {
  if (attrs.reverse) {
    return attrs.fg
      ? { r: (attrs.fg as { r: number; g: number; b: number }).r, g: (attrs.fg as { r: number; g: number; b: number }).g, b: (attrs.fg as { r: number; g: number; b: number }).b }
      : DEFAULT_FOREGROUND;
  }
  return attrs.bg
    ? { r: (attrs.bg as { r: number; g: number; b: number }).r, g: (attrs.bg as { r: number; g: number; b: number }).g, b: (attrs.bg as { r: number; g: number; b: number }).b }
    : null; // null means use default/transparent
}
