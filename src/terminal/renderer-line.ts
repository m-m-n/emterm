/**
 * Line rendering functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for rendering individual lines
 * including backgrounds, text spans, wide characters, and fitted characters.
 */

import type { CellAttributes } from "./attributes.ts";
import {
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
import { drawCustomGlyph, isCustomGlyph } from "./custom-glyphs.ts";
import type { Rgb } from "./colors.ts";
import { rgbToCSS } from "./colors.ts";
import type { LineAccessor } from "./grid.ts";
import { isExtendedPictographic, hasVariationSelector } from "./wasm/unicode.ts";
import {
	type TextSpan,
	groupCellsIntoSpans,
	applyTextAttributes,
	buildFontString,
} from "./renderer-utils.ts";
import {
	drawUnderline,
	drawStrikethrough,
} from "./renderer-decorations.ts";

/**
 * Context needed by line rendering functions.
 */
export interface LineRenderContext {
	ctx: CanvasRenderingContext2D;
	charWidth: number;
	charHeight: number;
	fontAscent: number;
	fontDescent: number;
	fontSize: number;
	fontFamily: string;
	dpr: number;
	canvas: HTMLCanvasElement;
	currentForeground: Rgb;
	currentBackground: Rgb;
	currentPalette256: readonly Rgb[];
	boldBrightensAnsiColors: boolean;
	glyphWidthCache: Map<string, Map<string, number>>;
	cols: number;
	/** Callback to render detection underlines for a row */
	renderDetectionUnderlines: (rowIndex: number) => void;
	/** Callback to render detection underlines from pre-parsed spans */
	renderDetectionUnderlinesFromSpans: (rowIndex: number, spans: TextSpan[]) => void;
}

/**
 * Render a single line (both background and text).
 */
export function renderLine(
	rctx: LineRenderContext,
	rowIndex: number,
	line: LineAccessor,
): void {
	renderLineBackground(rctx, rowIndex, line);
	renderLineText(rctx, rowIndex, line);
}

/**
 * Render a line from packed binary data (single pass: background + text).
 */
export function renderLinePacked(
	rctx: LineRenderContext,
	rowIndex: number,
	spans: TextSpan[],
): void {
	renderLineBackgroundFromSpans(rctx, rowIndex, spans);
	renderLineTextFromSpans(rctx, rowIndex, spans);
}

/**
 * Render only the background of a line.
 */
export function renderLineBackground(
	rctx: LineRenderContext,
	rowIndex: number,
	line: LineAccessor,
): void {
	const { ctx, charWidth, charHeight, dpr, canvas, currentForeground, currentBackground, currentPalette256 } = rctx;
	const y = rowIndex * charHeight;
	const fillY = Math.floor(y);
	const fillNextY = Math.ceil((rowIndex + 1) * charHeight);
	const fillHeight = fillNextY - fillY;
	const canvasWidth = canvas.width / dpr;

	ctx.fillStyle = rgbToCSS(currentBackground);
	ctx.fillRect(0, fillY, canvasWidth, fillHeight);

	const spans = groupCellsIntoSpans(line);
	for (const span of spans) {
		const bg = getEffectiveBackground(span.attrs, currentForeground, currentPalette256);
		if (bg !== null) {
			const x = span.startCol * charWidth;
			const width = span.cellCount * charWidth;
			ctx.fillStyle = rgbToCSS(bg);
			ctx.fillRect(x, fillY, width, fillHeight);
		}
	}
}

/**
 * Render only the text of a line (no background clearing).
 */
export function renderLineText(
	rctx: LineRenderContext,
	rowIndex: number,
	line: LineAccessor,
): void {
	const spans = groupCellsIntoSpans(line);
	for (const span of spans) {
		renderSpanText(rctx, span, rowIndex);
	}
	rctx.renderDetectionUnderlines(rowIndex);
}

/**
 * Render backgrounds from pre-parsed spans (packed path).
 */
export function renderLineBackgroundFromSpans(
	rctx: LineRenderContext,
	rowIndex: number,
	spans: TextSpan[],
): void {
	const { ctx, charWidth, charHeight, dpr, canvas, currentForeground, currentBackground, currentPalette256 } = rctx;
	const y = rowIndex * charHeight;
	const fillY = Math.floor(y);
	const fillNextY = Math.ceil((rowIndex + 1) * charHeight);
	const fillHeight = fillNextY - fillY;
	const canvasWidth = canvas.width / dpr;

	ctx.fillStyle = rgbToCSS(currentBackground);
	ctx.fillRect(0, fillY, canvasWidth, fillHeight);

	for (const span of spans) {
		const bg = getEffectiveBackground(span.attrs, currentForeground, currentPalette256);
		if (bg !== null) {
			const x = span.startCol * charWidth;
			const width = span.cellCount * charWidth;
			ctx.fillStyle = rgbToCSS(bg);
			ctx.fillRect(x, fillY, width, fillHeight);
		}
	}
}

/**
 * Render text from pre-parsed spans (packed path).
 */
export function renderLineTextFromSpans(
	rctx: LineRenderContext,
	rowIndex: number,
	spans: TextSpan[],
): void {
	for (const span of spans) {
		renderSpanText(rctx, span, rowIndex);
	}
	rctx.renderDetectionUnderlinesFromSpans(rowIndex, spans);
}

/**
 * Render only the text portion of a span (no background).
 */
export function renderSpanText(
	rctx: LineRenderContext,
	span: TextSpan,
	rowIndex: number,
): void {
	const { ctx, charWidth, charHeight, fontAscent, fontDescent, currentForeground, currentBackground, currentPalette256, boldBrightensAnsiColors } = rctx;
	const x = span.startCol * charWidth;
	const y = Math.floor(rowIndex * charHeight);
	const width = span.cellCount * charWidth;

	const fg = getEffectiveForeground(span.attrs, currentForeground, currentBackground, currentPalette256, boldBrightensAnsiColors);
	const styles = applyTextAttributes(span.attrs);

	if (styles.hidden) {
		return;
	}

	const originalAlpha = ctx.globalAlpha;
	if (styles.globalAlpha !== 1) {
		ctx.globalAlpha = styles.globalAlpha;
	}

	ctx.font = buildFontStringInternal(rctx, span.attrs);
	ctx.fillStyle = rgbToCSS(fg);

	const textY = y + (charHeight + fontAscent - fontDescent) / 2;

	let col = span.startCol;
	for (const [cellChar, cellWidth] of span.cells) {
		const charX = col * charWidth;
		if (cellChar.length === 1 && isCustomGlyph(cellChar)) {
			drawCustomGlyph(ctx, cellChar, charX, y, charWidth, charHeight);
		} else if (cellWidth >= 2) {
			drawWideCharacter(rctx, cellChar, charX, textY, cellWidth);
		} else if (cellChar.charCodeAt(0) > 0x7F) {
			drawFittedCharacter(rctx, cellChar, charX, textY);
		} else {
			ctx.fillText(cellChar, charX, textY);
		}
		col += cellWidth > 0 ? cellWidth : 1;
	}

	if (styles.underline || (span.attrs.hyperlinkId && span.attrs.hyperlinkId > 0)) {
		drawUnderline(ctx, x, y, width, fg, charHeight);
	}

	if (styles.strikethrough) {
		drawStrikethrough(ctx, x, y, width, fg, charHeight);
	}

	if (styles.globalAlpha !== 1) {
		ctx.globalAlpha = originalAlpha;
	}
}

/**
 * Draw a wide character (emoji/CJK) fitted within its allocated cell space.
 */
export function drawWideCharacter(
	rctx: LineRenderContext,
	char: string,
	x: number,
	textY: number,
	cellWidth: number,
): void {
	const { ctx, charWidth } = rctx;
	const allocatedWidth = cellWidth * charWidth;
	const measured = ctx.measureText(char).width;

	if (measured <= allocatedWidth) {
		const offset = (allocatedWidth - measured) / 2;
		ctx.fillText(char, x + offset, textY);
	} else {
		const scale = allocatedWidth / measured;
		ctx.save();
		ctx.translate(x + allocatedWidth / 2, textY);
		ctx.scale(scale, scale);
		ctx.fillText(char, -measured / 2, 0);
		ctx.restore();
	}
}

/**
 * Draw a non-ASCII narrow character, shrinking it to fit 1 cell if needed.
 */
export function drawFittedCharacter(
	rctx: LineRenderContext,
	char: string,
	x: number,
	textY: number,
): void {
	const { ctx, charWidth, glyphWidthCache } = rctx;

	// Force text presentation for Extended_Pictographic without VS
	const cp = char.codePointAt(0)!;
	if (isExtendedPictographic(cp) && !hasVariationSelector(char)) {
		char = char + "\uFE0E";
	}

	const fontKey = ctx.font;
	let fontCache = glyphWidthCache.get(fontKey);
	if (!fontCache) {
		fontCache = new Map();
		glyphWidthCache.set(fontKey, fontCache);
	}

	let measured = fontCache.get(char);
	if (measured === undefined) {
		measured = ctx.measureText(char).width;
		fontCache.set(char, measured);
	}

	if (measured <= charWidth) {
		ctx.fillText(char, x, textY);
	} else {
		const scale = charWidth / measured;
		ctx.save();
		ctx.translate(x + charWidth / 2, textY);
		ctx.scale(scale, scale);
		ctx.fillText(char, -measured / 2, 0);
		ctx.restore();
	}
}

/**
 * Build font string from attributes (internal helper).
 */
export function buildFontStringInternal(
	rctx: LineRenderContext,
	attrs: CellAttributes,
): string {
	return buildFontString(attrs, rctx.fontSize, rctx.fontFamily);
}
