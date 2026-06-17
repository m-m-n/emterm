/**
 * Decoration drawing functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for drawing underlines,
 * strikethroughs, and URL/file path detection underlines.
 */

import {
	getEffectiveForeground,
} from "./attributes.ts";
import type { Rgb } from "./colors.ts";
import { rgbToCSS } from "./colors.ts";
import type { LineAccessor } from "./grid.ts";
import { detectUrls, detectFilePaths, getLogicalLine, physicalToLogicalCol, type LogicalLine, type UrlMatch, type FilePathMatch } from "./url-detector.ts";
import { SettingsService } from "../settings/settings-service.ts";

/**
 * Per-frame detection cache entry.
 */
export interface DetectionCacheEntry {
	logical: LogicalLine;
	urls: UrlMatch[];
	fps: FilePathMatch[];
}

/**
 * Context needed by decoration rendering functions.
 */
export interface DecorationRenderContext {
	ctx: CanvasRenderingContext2D;
	charWidth: number;
	charHeight: number;
	cols: number;
	rows: number;
	currentForeground: Rgb;
	currentBackground: Rgb;
	currentPalette256: readonly Rgb[];
	boldBrightensAnsiColors: boolean;
	hoverRow: number;
	hoverCol: number;
	/** Visible lines for scroll-aware detection (null when using buffer directly). */
	renderVisibleLines: (LineAccessor | null)[] | null;
	/** Per-frame detection cache (keyed by startRow). */
	detectionCache: Map<number, DetectionCacheEntry>;
	/** Get line from active buffer (for non-scroll-aware path). */
	getBufferLine: (row: number) => LineAccessor | null;
}

/**
 * Draw underlines for detected URLs and file paths from pre-parsed spans.
 */
export function renderDetectionUnderlinesFromSpans(
	dctx: DecorationRenderContext,
	rowIndex: number,
	_spans: unknown[],
): void {
	renderDetectionUnderlinesLogical(dctx, rowIndex);
}

/**
 * Draw underlines for detected URLs and file paths in a line.
 */
export function renderDetectionUnderlines(
	dctx: DecorationRenderContext,
	rowIndex: number,
	_line: LineAccessor,
): void {
	renderDetectionUnderlinesLogical(dctx, rowIndex);
}

/**
 * Shared logic for drawing detection underlines with logical line support.
 */
export function renderDetectionUnderlinesLogical(
	dctx: DecorationRenderContext,
	rowIndex: number,
): void {
	const cachedSettings = SettingsService.getCached();

	const getLine = dctx.renderVisibleLines
		? (r: number): LineAccessor | null => {
			if (r < 0 || r >= dctx.renderVisibleLines!.length) return null;
			return dctx.renderVisibleLines![r] ?? null;
		}
		: (r: number): LineAccessor | null => {
			if (r < 0 || r >= dctx.rows) return null;
			return dctx.getBufferLine(r);
		};

	const logical = getLogicalLine(getLine, rowIndex, dctx.rows);
	if (logical.rowCount === 0) return;

	let cached = dctx.detectionCache.get(logical.startRow);
	if (!cached) {
		const urls = (!cachedSettings || cachedSettings.url_detection)
			? detectUrls(logical.text) : [];
		const fps = (!cachedSettings || cachedSettings.file_path_detection)
			? detectFilePaths(logical.text) : [];
		cached = { logical, urls, fps };
		dctx.detectionCache.set(logical.startRow, cached);
	}

	if (dctx.hoverRow < 0 || dctx.hoverCol < 0) return;
	if (dctx.hoverRow < logical.startRow || dctx.hoverRow >= logical.startRow + logical.rowCount) return;

	const hoverLogicalCol = physicalToLogicalCol(dctx.hoverRow, dctx.hoverCol, logical);

	let hoveredMatch: { startCol: number; endCol: number } | null = null;
	for (const match of cached.urls) {
		if (hoverLogicalCol >= match.startCol && hoverLogicalCol < match.endCol) {
			hoveredMatch = match;
			break;
		}
	}
	if (!hoveredMatch) {
		for (const match of cached.fps) {
			if (hoverLogicalCol >= match.startCol && hoverLogicalCol < match.endCol) {
				hoveredMatch = match;
				break;
			}
		}
	}
	if (!hoveredMatch) return;

	drawClippedUnderlineWithCellColors(dctx, hoveredMatch.startCol, hoveredMatch.endCol, rowIndex, cached.logical, getLine);
}

/**
 * Draw underline for a hovered link, rendering on the specified physical row.
 */
function renderHoverUnderline(
	dctx: DecorationRenderContext,
	rowIndex: number,
): void {
	renderDetectionUnderlinesLogical(dctx, rowIndex);
}

/**
 * Draw underline for a match clipped to a single physical row.
 */
function drawClippedUnderline(
	ctx: CanvasRenderingContext2D,
	matchStart: number,
	matchEnd: number,
	rowIndex: number,
	logical: LogicalLine,
	y: number,
	color: Rgb,
	charWidth: number,
	charHeight: number,
): void {
	const rowStartLogical = (rowIndex - logical.startRow) * logical.cols;
	const rowEndLogical = rowStartLogical + logical.cols;

	const clippedStart = Math.max(matchStart, rowStartLogical);
	const clippedEnd = Math.min(matchEnd, rowEndLogical);
	if (clippedStart >= clippedEnd) return;

	const physStartCol = clippedStart - rowStartLogical;
	const physEndCol = clippedEnd - rowStartLogical;
	const x = physStartCol * charWidth;
	const width = (physEndCol - physStartCol) * charWidth;
	drawUnderline(ctx, x, y, width, color, charHeight);
}

/**
 * Draw underline for a hovered link with per-cell foreground colors.
 */
export function drawClippedUnderlineWithCellColors(
	dctx: DecorationRenderContext,
	matchStart: number,
	matchEnd: number,
	rowIndex: number,
	logical: LogicalLine,
	getLine: (r: number) => LineAccessor | null,
): void {
	const { ctx, charWidth, charHeight, currentForeground, currentBackground, currentPalette256, boldBrightensAnsiColors } = dctx;
	const rowStartLogical = (rowIndex - logical.startRow) * logical.cols;
	const rowEndLogical = rowStartLogical + logical.cols;
	const clippedStart = Math.max(matchStart, rowStartLogical);
	const clippedEnd = Math.min(matchEnd, rowEndLogical);
	if (clippedStart >= clippedEnd) return;

	const y = Math.floor(rowIndex * charHeight);
	const line = getLine(rowIndex);
	if (!line) return;

	for (let logCol = clippedStart; logCol < clippedEnd; logCol++) {
		const physCol = logCol - rowStartLogical;
		const cell = line.getCell(physCol);
		const fg = getEffectiveForeground(cell.attrs, currentForeground, currentBackground, currentPalette256, boldBrightensAnsiColors);
		const x = physCol * charWidth;
		drawUnderline(ctx, x, y, charWidth, fg, charHeight);
	}
}

/**
 * Draw underline decoration.
 */
export function drawUnderline(
	ctx: CanvasRenderingContext2D,
	x: number,
	y: number,
	width: number,
	color: { r: number; g: number; b: number },
	charHeight: number,
): void {
	const underlineY = y + charHeight - 2;
	ctx.fillStyle = rgbToCSS(color);
	ctx.fillRect(x, underlineY, width, 1);
}

/**
 * Draw strikethrough decoration.
 */
export function drawStrikethrough(
	ctx: CanvasRenderingContext2D,
	x: number,
	y: number,
	width: number,
	color: { r: number; g: number; b: number },
	charHeight: number,
): void {
	const strikeY = y + charHeight / 2;
	ctx.fillStyle = rgbToCSS(color);
	ctx.fillRect(x, strikeY, width, 1);
}
