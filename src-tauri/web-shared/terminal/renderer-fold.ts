/**
 * Fold rendering functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for handling collapsed
 * fold regions and rendering summary lines.
 */

import type { Rgb } from "./colors.ts";
import { rgbToCSS } from "./colors.ts";
import type { LineAccessor } from "./grid.ts";
import type { TerminalState } from "./state.ts";
import type { FoldRegion } from "./fold-manager.ts";


/**
 * Context needed by fold rendering functions.
 */
export interface FoldRenderContext {
	ctx: CanvasRenderingContext2D;
	charWidth: number;
	charHeight: number;
	fontSize: number;
	fontFamily: string;
	cols: number;
	scrollOffset: number;
}

/**
 * Get packed binary data for visible rows, accounting for scroll offset.
 */
export function getVisibleRowsPacked(
	state: TerminalState,
	scrollOffset: number,
	count: number,
): (Uint8Array | null)[] {
	const result: (Uint8Array | null)[] = [];

	if (scrollOffset === 0) {
		for (let row = 0; row < count; row++) {
			result.push(state.getRowPacked(row));
		}
	} else {
		const scrollbackLength = state.getScrollbackLength();
		const startIndex = Math.max(0, scrollbackLength - scrollOffset);
		for (let i = 0; i < count; i++) {
			const lineIndex = startIndex + i;
			if (lineIndex < scrollbackLength) {
				result.push(state.getScrollbackRowPacked(lineIndex));
			} else {
				result.push(state.getRowPacked(lineIndex - scrollbackLength));
			}
		}
	}

	return result;
}

/**
 * Get visible lines accounting for collapsed fold regions.
 */
export function getVisibleLinesWithFolding(
	state: TerminalState,
	foldManager: ReturnType<TerminalState["getFoldManager"]>,
	scrollOffset: number,
): (LineAccessor | null)[] {
	const buffer = state.getActiveBuffer();
	const scrollbackLength = state.getScrollbackLength();
	const visibleRows = state.rows;

	const totalActualLines = scrollbackLength + visibleRows;
	const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);

	const displayStart = Math.max(0, totalDisplayLines - visibleRows - scrollOffset);

	const result: (LineAccessor | null)[] = [];
	for (let displayRow = 0; displayRow < visibleRows; displayRow++) {
		const displayLine = displayStart + displayRow;

		const summaryRegion = foldManager.getSummaryRegion(displayLine);
		if (summaryRegion) {
			result.push(null);
			continue;
		}

		const actualLine = foldManager.displayLineToActual(displayLine);

		if (actualLine < scrollbackLength) {
			result.push(state.getScrollbackLine(actualLine));
		} else {
			const screenRow = actualLine - scrollbackLength;
			if (screenRow >= 0 && screenRow < visibleRows) {
				result.push(buffer.getLine(screenRow));
			} else {
				result.push(null);
			}
		}
	}

	return result;
}

/**
 * Render fold summary lines on the canvas.
 */
export function renderFoldSummaryLines(
	fctx: FoldRenderContext,
	state: TerminalState,
	visibleLines: (LineAccessor | null)[],
	foldManager: ReturnType<TerminalState["getFoldManager"]>,
): void {
	const scrollbackLength = state.getScrollbackLength();
	const totalActualLines = scrollbackLength + state.rows;
	const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
	const displayStart = Math.max(0, totalDisplayLines - state.rows - fctx.scrollOffset);

	for (let row = 0; row < visibleLines.length; row++) {
		if (visibleLines[row] !== null) continue;

		const displayLine = displayStart + row;
		const region = foldManager.getSummaryRegion(displayLine);
		if (!region) continue;

		renderSummaryLine(fctx, row, region);
	}
}

/**
 * Render a single fold summary line.
 */
export function renderSummaryLine(
	fctx: FoldRenderContext,
	rowIndex: number,
	region: FoldRegion,
): void {
	const { ctx, charWidth, charHeight, fontSize, fontFamily, cols } = fctx;
	const y = rowIndex * charHeight;
	const width = cols * charWidth;

	ctx.fillStyle = "rgba(60, 60, 80, 0.3)";
	ctx.fillRect(0, y, width, charHeight);

	const icon = "\u25B6"; // triangle right
	const name = region.source === "custom"
		? (region.label || "...")
		: (region.commandText || "...");
	const truncatedName = name.length > 80 ? name.substring(0, 77) + "..." : name;

	let rightText = `\u2014 ${region.lineCount} lines`;
	if (region.source === "osc133" && region.exitCode !== undefined) {
		rightText += ` (exit ${region.exitCode})`;
	}

	const isError = region.source === "osc133" && region.exitCode !== undefined && region.exitCode !== 0;
	const textColor = isError ? "#ff6b6b" : "rgba(200, 200, 210, 0.7)";

	ctx.font = `${fontSize}px "${fontFamily}"`;
	ctx.textBaseline = "top";

	ctx.fillStyle = textColor;
	const textY = y + (charHeight - fontSize) / 2;
	ctx.fillText(`${icon} ${truncatedName}`, charWidth * 0.5, textY);

	const rightWidth = ctx.measureText(rightText).width;
	const rightX = width - rightWidth - charWidth * 0.5;
	ctx.fillStyle = textColor;
	ctx.fillText(rightText, rightX, textY);
}
