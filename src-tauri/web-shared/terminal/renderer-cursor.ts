/**
 * Cursor rendering functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for drawing cursors
 * in block, underline, and bar styles with blink support.
 */

import {
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
import type { Rgb } from "./colors.ts";
import { rgbToCSS } from "./colors.ts";
import type { CursorStyle } from "./cursor.ts";
import type { TerminalState } from "./state.ts";
import type { LineRenderContext } from "./renderer-line.ts";
import { drawWideCharacter, drawFittedCharacter, buildFontStringInternal } from "./renderer-line.ts";

/**
 * Context needed by cursor rendering functions.
 */
export interface CursorRenderContext extends LineRenderContext {
	currentCursorColor: Rgb;
	scrollOffset: number;
	cursorBlinkVisible: boolean;
}

/**
 * Render cursor at the specified position.
 */
export function renderCursor(
	rctx: CursorRenderContext,
	col: number,
	row: number,
	visible: boolean,
	style: CursorStyle,
	blink: boolean = true,
	state?: TerminalState,
): void {
	const { ctx, charWidth, charHeight, fontAscent, fontDescent, currentCursorColor } = rctx;

	if (!visible || (blink && !rctx.cursorBlinkVisible)) {
		return;
	}

	const x = col * charWidth;
	const y = row * charHeight;

	const cursorColorCSS = rgbToCSS(currentCursorColor);
	ctx.fillStyle = cursorColorCSS;
	ctx.strokeStyle = cursorColorCSS;

	switch (style) {
		case "block": {
			let cursorPixelWidth = charWidth;
			let cell: ReturnType<ReturnType<TerminalState["getActiveBuffer"]>["getLine"]> extends { getCell(i: number): infer C } ? C : never;
			if (state) {
				const buffer = state.getActiveBuffer();
				const line = buffer.getLine(row);
				cell = line.getCell(col);
				if (cell.width >= 2) {
					cursorPixelWidth = cell.width * charWidth;
				}
			}
			ctx.fillRect(x, y, cursorPixelWidth, charHeight);
			if (cell! && cell.char !== " " && cell.char !== "") {
				const bg = getEffectiveBackground(cell.attrs, rctx.currentForeground, rctx.currentPalette256);
				ctx.fillStyle = rgbToCSS(bg ?? rctx.currentBackground);
				ctx.font = buildFontStringInternal(rctx, cell.attrs);
				const textY = y + (charHeight + fontAscent - fontDescent) / 2;
				if (cell.width >= 2) {
					drawWideCharacter(rctx, cell.char, x, textY, cell.width);
				} else if (cell.char.charCodeAt(0) > 0x7F) {
					drawFittedCharacter(rctx, cell.char, x, textY);
				} else {
					ctx.fillText(cell.char, x, textY);
				}
			}
			break;
		}
		case "underline":
			ctx.fillRect(x, y + charHeight - 2, charWidth, 2);
			break;
		case "bar":
			ctx.fillRect(x, y, 2, charHeight);
			break;
	}
}

/**
 * Re-render cursor area for blink effect.
 */
export function renderCursorArea(
	rctx: CursorRenderContext,
	state: TerminalState,
): void {
	const { ctx, charWidth, charHeight, fontAscent, fontDescent, currentBackground, currentForeground, currentPalette256, boldBrightensAnsiColors } = rctx;

	if (rctx.scrollOffset > 0) {
		return;
	}

	const buffer = state.getActiveBuffer();
	const row = state.cursorRow;
	const col = state.cursorCol;

	const line = buffer.getLine(row);
	const y = row * charHeight;
	const x = col * charWidth;

	const cell = line.getCell(col);
	const cellPixelWidth = cell.width >= 2 ? cell.width * charWidth : charWidth;
	const fillY = Math.floor(y);
	const fillNextY = Math.ceil((row + 1) * charHeight);
	const fillHeight = fillNextY - fillY;
	ctx.fillStyle = rgbToCSS(currentBackground);
	ctx.fillRect(x, fillY, cellPixelWidth, fillHeight);

	if (cell.char !== " " && cell.char !== "") {
		const fg = getEffectiveForeground(cell.attrs, currentForeground, currentBackground, currentPalette256, boldBrightensAnsiColors);
		ctx.fillStyle = rgbToCSS(fg);
		ctx.font = buildFontStringInternal(rctx, cell.attrs);
		const textY = y + (charHeight + fontAscent - fontDescent) / 2;
		if (cell.width >= 2) {
			drawWideCharacter(rctx, cell.char, x, textY, cell.width);
		} else if (cell.char.charCodeAt(0) > 0x7F) {
			drawFittedCharacter(rctx, cell.char, x, textY);
		} else {
			ctx.fillText(cell.char, x, textY);
		}
	}

	renderCursor(
		rctx,
		col,
		row,
		state.cursorVisible,
		state.cursorStyle,
		state.cursorBlink,
		state,
	);
}

/**
 * Cursor blink state manager.
 */
export interface CursorBlinkState {
	cursorBlinkTimer: ReturnType<typeof setInterval> | null;
	cursorBlinkVisible: boolean;
}

/**
 * Start cursor blink timer.
 */
export function startCursorBlink(
	blinkState: CursorBlinkState,
	onBlink: () => void,
): void {
	stopCursorBlink(blinkState);

	blinkState.cursorBlinkTimer = setInterval(() => {
		blinkState.cursorBlinkVisible = !blinkState.cursorBlinkVisible;
		onBlink();
	}, 500);
}

/**
 * Stop cursor blink timer.
 */
export function stopCursorBlink(blinkState: CursorBlinkState): void {
	if (blinkState.cursorBlinkTimer !== null) {
		clearInterval(blinkState.cursorBlinkTimer);
		blinkState.cursorBlinkTimer = null;
	}
	blinkState.cursorBlinkVisible = true;
}
