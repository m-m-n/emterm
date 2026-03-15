/**
 * Canvas 2D Renderer for terminal output.
 *
 * Renders terminal state to a Canvas element using the 2D API.
 * Optimized for high-performance scrolling with High DPI support.
 */

import type { CellAttributes } from "./attributes.ts";
import type { Cell } from "./grid.ts";
import {
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
import { drawCustomGlyph, isCustomGlyph } from "./custom-glyphs.ts";
import {
	buildPalette256,
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	getColorSchemePreset,
	hexToRgb,
	PALETTE_16,
	PALETTE_256,
	type Rgb,
	rgbToCSS,
} from "./colors.ts";
import type { UserColorScheme } from "../settings/types";
import type { CursorStyle } from "./cursor.ts";
import type { LineAccessor } from "./grid.ts";
import {
	checkFrameBudget,
	getPerformanceMonitor,
	RenderTimer,
} from "./performance.ts";
import type { ITerminalRenderer } from "./renderer-interface.ts";
import type { RendererSettings } from "../settings/settings-applier";
import type { TerminalState } from "./state.ts";
import type { SearchMatch } from "./search/search-state.ts";
import type { FoldRegion } from "./fold-manager.ts";
import { detectUrls, detectFilePaths, getLogicalLine, physicalToLogicalCol, type LogicalLine, type UrlMatch, type FilePathMatch } from "./url-detector.ts";
import { SettingsService } from "../settings/settings-service.ts";
import { isExtendedPictographic, hasVariationSelector } from "./wasm/unicode.ts";
import {
	type TextSpan,
	groupCellsIntoSpans,
	groupPackedCellsIntoSpans,
	getVisibleLines,
	applyTextAttributes,
	buildFontString,
} from "./renderer-utils.ts";

// Re-export utilities for backward compatibility
export {
	type TextSpan,
	type TextAttributeStyles,
	type SelectionRange,
	groupCellsIntoSpans,
	packedAttrsEqual,
	unpackAttrsFromBinary,
	groupPackedCellsIntoSpans,
	getVisibleLines,
	calculateScrollPosition,
	buildFontString,
	applyTextAttributes,
	normalizeSelection,
} from "./renderer-utils.ts";

/**
 * Canvas 2D terminal renderer.
 */
export class CanvasRenderer implements ITerminalRenderer {
	/** Container element. */
	private container: HTMLElement;

	/** Canvas element. */
	private canvas: HTMLCanvasElement;

	/** 2D rendering context. */
	private ctx: CanvasRenderingContext2D;

	/** Font family. */
	private fontFamily: string;

	/** Font size in pixels. */
	private fontSize: number;

	/** Number of columns. */
	private cols: number = 80;

	/** Number of rows. */
	private rows: number = 24;

	/** Character width in pixels. */
	private charWidth: number = 0;

	/** Character height in pixels. */
	private charHeight: number = 0;

	/** Font ascent in pixels (for baseline positioning). */
	private fontAscent: number = 0;

	/** Font descent in pixels (for baseline positioning). */
	private fontDescent: number = 0;

	/** Current device pixel ratio. */
	private dpr: number = 1;

	/** Pending render flag. */
	private renderPending: boolean = false;

	/** Current state to render. */
	private pendingState: TerminalState | null = null;

	/** Performance timer. */
	private renderTimer: RenderTimer = new RenderTimer();

	/** Media query for DPR changes. */
	private dprMediaQuery: MediaQueryList | null = null;

	/** DPR change handler. */
	private dprChangeHandler: (() => void) | null = null;

	/** Selection overlay container. */
	private selectionContainer: HTMLDivElement | null = null;

	/** Selection overlay elements for each line. */
	private selectionOverlays: HTMLDivElement[] = [];

	/** Cursor blink timer ID. */
	private cursorBlinkTimer: ReturnType<typeof setInterval> | null = null;

	/** Cursor visible state for blinking. */
	private cursorBlinkVisible: boolean = true;

	/** Blink text timer ID. */
	private blinkTextTimer: ReturnType<typeof setInterval> | null = null;

	/** Blink text visible state. */
	private blinkTextVisible: boolean = true;

	/** Previous cursor position for clearing. */
	private prevCursorCol: number = -1;

	/** Previous cursor row for clearing. */
	private prevCursorRow: number = -1;

	/** Previous cursor visible state for detecting visibility changes. */
	private prevCursorVisible: boolean = true;

	/** Current foreground color (can be changed by color scheme). */
	private currentForeground: Rgb = DEFAULT_FOREGROUND;

	/** Current background color (can be changed by color scheme). */
	private currentBackground: Rgb = DEFAULT_BACKGROUND;

	/** Current cursor color (can be changed by color scheme). */
	private currentCursorColor: Rgb = { r: 0, g: 128, b: 0 };

	/** Current 16-color palette (can be changed by color scheme). */
	private currentPalette16: readonly Rgb[] = PALETTE_16;

	/** Current full 256-color palette (first 16 entries from currentPalette16, rest from static). */
	private currentPalette256: readonly Rgb[] = PALETTE_256;

	/** Whether bold attribute brightens standard ANSI colors (0-7 -> 8-15). */
	private boldBrightensAnsiColors: boolean = true;

	/** Glyph width cache: outer key = ctx.font string, inner key = character. */
	private glyphWidthCache: Map<string, Map<string, number>> = new Map();

	/** Current scroll offset (number of lines scrolled back from bottom). */
	private scrollOffset: number = 0;

	/** Visible lines resolved for the current render pass (scroll-aware). */
	private renderVisibleLines: (LineAccessor | null)[] | null = null;

	/** Hover position for link underline (display row). -1 = no hover. */
	private hoverRow: number = -1;

	/** Hover position for link underline (display col). -1 = no hover. */
	private hoverCol: number = -1;

	/** Previous hover position for differential redraw. */
	private prevHoverRow: number = -1;
	private prevHoverCol: number = -1;

	/** Search matches to highlight (set externally). */
	private searchMatches: SearchMatch[] = [];

	/** Current search match index (-1 if none). */
	private searchCurrentIndex: number = -1;

	/**
	 * Create a new canvas renderer.
	 *
	 * @param container - Container element
	 * @param fontFamily - Font family for terminal text
	 * @param fontSize - Font size in pixels
	 */
	constructor(container: HTMLElement, fontFamily: string, fontSize: number) {
		this.container = container;
		this.fontFamily = fontFamily;
		this.fontSize = fontSize;

		// Create canvas element
		this.canvas = document.createElement("canvas");
		this.canvas.style.display = "block";
		this.container.appendChild(this.canvas);

		// Get 2D context
		const ctx = this.canvas.getContext("2d");
		if (!ctx) {
			throw new Error("Failed to get 2D rendering context");
		}
		this.ctx = ctx;

		// Initialize canvas with DPR support
		this.setupCanvas();

		// Measure character dimensions
		this.measureCharacterSize();

		// Watch for DPR changes
		this.watchDPRChanges();

		// Start cursor blink timer
		this.startCursorBlink();
	}

	/**
	 * Set up canvas with High DPI support.
	 */
	private setupCanvas(): void {
		this.dpr = window.devicePixelRatio || 1;

		// Get container dimensions
		const rect = this.container.getBoundingClientRect();
		const width = rect.width || 800;
		const height = rect.height || 600;

		// Set canvas size with DPR scaling
		this.canvas.width = Math.floor(width * this.dpr);
		this.canvas.height = Math.floor(height * this.dpr);
		this.canvas.style.width = `${width}px`;
		this.canvas.style.height = `${height}px`;

		// Scale context for DPR
		this.ctx.scale(this.dpr, this.dpr);

		// Set default text rendering settings
		this.ctx.textBaseline = "alphabetic";
		this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;
	}

	/**
	 * Measure character dimensions using the canvas context.
	 */
	private measureCharacterSize(): void {
		// Ensure font is set
		this.ctx.font = `${this.fontSize}px ${this.fontFamily}`;

		// Measure width using 'M' (consistent with size.ts measureCharacterSize)
		const metrics = this.ctx.measureText("M");
		this.charWidth = metrics.width;

		// Calculate height from font metrics
		const ascent = metrics.fontBoundingBoxAscent ?? this.fontSize * 0.8;
		const descent = metrics.fontBoundingBoxDescent ?? this.fontSize * 0.2;
		this.fontAscent = ascent;
		this.fontDescent = descent;

		// Use font metrics (ascent + descent) as the natural line height.
		// Ceil to integer so drawImage scroll shift aligns with Math.floor row positions.
		this.charHeight = Math.ceil(ascent + descent);

		// Clear glyph width cache on font change
		this.glyphWidthCache.clear();
	}

	/**
	 * Watch for devicePixelRatio changes.
	 */
	private watchDPRChanges(): void {
		const updateDPR = () => {
			const newDpr = window.devicePixelRatio || 1;
			if (newDpr !== this.dpr) {
				this.setupCanvas();
				this.measureCharacterSize();
				if (this.pendingState) {
					this.forceRender(this.pendingState);
				}
			}
			// Re-register for the new DPR value
			this.registerDPRListener();
		};

		this.dprChangeHandler = updateDPR;
		this.registerDPRListener();
	}

	/**
	 * Register DPR change listener.
	 */
	private registerDPRListener(): void {
		// Remove old listener if exists
		if (this.dprMediaQuery && this.dprChangeHandler) {
			this.dprMediaQuery.removeEventListener("change", this.dprChangeHandler);
		}

		// Create new media query for current DPR
		this.dprMediaQuery = window.matchMedia(`(resolution: ${this.dpr}dppx)`);
		if (this.dprChangeHandler) {
			this.dprMediaQuery.addEventListener("change", this.dprChangeHandler);
		}
	}

	/**
	 * Schedule a render of the terminal state.
	 * Uses requestAnimationFrame for batching.
	 *
	 * @param state - Terminal state to render
	 */
	scheduleRender(state: TerminalState): void {
		this.pendingState = state;

		if (!this.renderPending) {
			this.renderPending = true;
			requestAnimationFrame(() => {
				try {
					this.render();
				} catch (error) {
					console.error("[ERROR][FRONTEND] Render failed:", error);
					this.detectionCache.clear();
				} finally {
					this.renderPending = false;
				}
			});
		}
	}

	/**
	 * Render immediately (synchronously) using dirty-row differential path.
	 * Used by frame-budgeted processing to render within the same rAF frame.
	 */
	renderImmediate(state: TerminalState): void {
		this.pendingState = state;
		try {
			this.render();
		} catch (error) {
			console.error("[ERROR][FRONTEND] Render failed:", error);
			this.detectionCache.clear();
		}
		// Cancel any pending scheduled render since we just rendered
		this.renderPending = false;
	}

	/**
	 * Perform the actual render.
	 */
	private render(): void {
		if (!this.pendingState) {
			return;
		}

		this.renderTimer.start();

		const state = this.pendingState;

		// When scrolled back, always do a full render
		if (this.scrollOffset > 0) {
			this.forceRender(state);
			const duration = this.renderTimer.end();
			const monitor = getPerformanceMonitor();
			if (monitor.isEnabled()) {
				monitor.recordRender(duration);
			}
			return;
		}

		// Check for scroll event before processing dirty rows.
		// If a full-screen scroll(1) occurred, shift existing canvas content
		// up and only draw the new row instead of redrawing everything.
		const scrollDir = state.getScrollEventDirection();
		if (scrollDir === 1) {
			const scrollCount = state.getScrollEventCount();
			state.clearScrollEvent();
			const shiftPx = scrollCount * this.charHeight;
			const canvasW = this.canvas.width / this.dpr;
			const canvasH = this.canvas.height / this.dpr;
			if (shiftPx > 0 && shiftPx < canvasH) {
				// Shift existing content up by shiftPx (pixel-aligned source offset)
				const srcOffsetPx = Math.round(shiftPx * this.dpr);
				this.ctx.drawImage(
					this.canvas,
					0, srcOffsetPx,
					this.canvas.width, this.canvas.height - srcOffsetPx,
					0, 0,
					canvasW, canvasH - shiftPx,
				);
				// Clear the vacated area at the bottom
				this.ctx.fillStyle = rgbToCSS(this.currentBackground);
				this.ctx.fillRect(0, canvasH - shiftPx, canvasW, shiftPx);
			}
		}

		const buffer = state.getActiveBuffer();
		const dirtyRows = state.getDirtyRows();
		const bufferRows = buffer.rows;

		// Pre-parse packed data for dirty rows
		const parsedRows: { rowIndex: number; spans: TextSpan[] | null; line: LineAccessor | null }[] = [];
		for (const rowIndex of dirtyRows) {
			// Guard: skip rows that exceed buffer bounds (WASM/TS desync)
			if (rowIndex < 0 || rowIndex >= bufferRows) {
				console.warn(`[WARN][RENDERER] Dirty row ${rowIndex} out of bounds (buffer rows=${bufferRows}), skipping`);
				continue;
			}
			const packed = state.getRowPacked(rowIndex);
			if (packed && packed.length > 0) {
				const spans = groupPackedCellsIntoSpans(packed, this.cols);
				if (spans.length === 0) {
					console.warn(`[WARN][RENDERER] Empty spans from non-empty packed data for row ${rowIndex}, packed.length=${packed.length}`);
				}
				parsedRows.push({ rowIndex, spans, line: null });
			} else {
				if (packed && packed.length === 0) {
					console.warn(`[WARN][RENDERER] Empty packed data for dirty row ${rowIndex}, falling back to LineAccessor`);
				}
				parsedRows.push({ rowIndex, spans: null, line: buffer.getLine(rowIndex) });
			}
		}

		// Two-pass rendering to prevent descender clipping
		// Pass 1: backgrounds
		for (const { rowIndex, spans, line } of parsedRows) {
			if (spans) {
				this.renderLineBackgroundFromSpans(rowIndex, spans);
			} else if (line) {
				this.renderLineBackground(rowIndex, line);
			}
		}
		// Pass 2: text
		for (const { rowIndex, spans, line } of parsedRows) {
			if (spans) {
				this.renderLineTextFromSpans(rowIndex, spans);
			} else if (line) {
				this.renderLineText(rowIndex, line);
			}
		}

		// Clear dirty flags
		state.clearDirty();

		// Clear previous cursor position if it moved or became invisible
		const cursorMoved =
			this.prevCursorCol !== state.cursorCol ||
			this.prevCursorRow !== state.cursorRow;
		const cursorBecameInvisible =
			this.prevCursorVisible && !state.cursorVisible;
		const prevRowNeedsRedraw =
			this.prevCursorRow >= 0 &&
			(cursorMoved || cursorBecameInvisible) &&
			!dirtyRows.includes(this.prevCursorRow);

		if (prevRowNeedsRedraw) {
			// Two-pass rendering consistent with dirty-row path above
			const prevPacked = state.getRowPacked(this.prevCursorRow);
			if (prevPacked && prevPacked.length > 0) {
				const prevSpans = groupPackedCellsIntoSpans(prevPacked, this.cols);
				this.renderLineBackgroundFromSpans(this.prevCursorRow, prevSpans);
				this.renderLineTextFromSpans(this.prevCursorRow, prevSpans);
			} else {
				const prevLine = buffer.getLine(this.prevCursorRow);
				this.renderLineBackground(this.prevCursorRow, prevLine);
				this.renderLineText(this.prevCursorRow, prevLine);
			}
		}

		// Update cursor
		this.renderCursor(
			state.cursorCol,
			state.cursorRow,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
			state,
		);

		// Save current cursor position for next render
		this.prevCursorCol = state.cursorCol;
		this.prevCursorRow = state.cursorRow;
		this.prevCursorVisible = state.cursorVisible;

		// Hover underline pass: redraw rows affected by hover position change.
		// Must redraw all physical rows of the logical lines involved,
		// because a link may span multiple wrapped rows.
		const hoverChanged = this.hoverRow !== this.prevHoverRow || this.hoverCol !== this.prevHoverCol;
		if (hoverChanged) {
			const hoverRedrawRows = new Set<number>();
			const getLineForHover = (r: number): LineAccessor | null =>
				(r >= 0 && r < this.rows) ? buffer.getLine(r) : null;

			// Collect all physical rows of the logical line for prev and current hover
			for (const seedRow of [this.prevHoverRow, this.hoverRow]) {
				if (seedRow < 0 || seedRow >= this.rows) continue;
				const logical = getLogicalLine(getLineForHover, seedRow, this.rows);
				for (let r = logical.startRow; r < logical.startRow + logical.rowCount; r++) {
					hoverRedrawRows.add(r);
				}
			}

			for (const row of hoverRedrawRows) {
				if (dirtyRows.includes(row)) continue;
				const packed = state.getRowPacked(row);
				if (packed && packed.length > 0) {
					const spans = groupPackedCellsIntoSpans(packed, this.cols);
					this.renderLineBackgroundFromSpans(row, spans);
					this.renderLineTextFromSpans(row, spans);
				} else {
					const line = buffer.getLine(row);
					this.renderLineBackground(row, line);
					this.renderLineText(row, line);
				}
			}
			this.prevHoverRow = this.hoverRow;
			this.prevHoverCol = this.hoverCol;
		}

		// Clear per-frame detection cache
		this.detectionCache.clear();

		// Record performance metrics
		const duration = this.renderTimer.end();
		const monitor = getPerformanceMonitor();
		if (monitor.isEnabled()) {
			monitor.recordRender(duration);
			if (duration > 16) {
				checkFrameBudget(duration, `canvas render ${dirtyRows.length} rows`);
			}
		}
	}

	/**
	 * Render a single line (both background and text).
	 */
	private renderLine(rowIndex: number, line: LineAccessor): void {
		this.renderLineBackground(rowIndex, line);
		this.renderLineText(rowIndex, line);
	}

	/**
	 * Render a line from packed binary data (single pass: background + text).
	 */
	private renderLinePacked(rowIndex: number, packed: Uint8Array): void {
		const spans = groupPackedCellsIntoSpans(packed, this.cols);
		this.renderLineBackgroundFromSpans(rowIndex, spans);
		this.renderLineTextFromSpans(rowIndex, spans);
	}

	/**
	 * Render only the background of a line.
	 *
	 * @param rowIndex - Row index (0-based)
	 * @param line - Line to render
	 */
	private renderLineBackground(rowIndex: number, line: LineAccessor): void {
		const y = rowIndex * this.charHeight;

		// Use integer-aligned coordinates to avoid sub-pixel gaps between rows
		const fillY = Math.floor(y);
		const fillNextY = Math.ceil((rowIndex + 1) * this.charHeight);
		const fillHeight = fillNextY - fillY;
		const canvasWidth = this.canvas.width / this.dpr;

		// Clear the row with current background, full canvas width
		this.ctx.fillStyle = rgbToCSS(this.currentBackground);
		this.ctx.fillRect(0, fillY, canvasWidth, fillHeight);

		// Group cells into spans and render colored backgrounds
		const spans = groupCellsIntoSpans(line);
		for (const span of spans) {
			const bg = getEffectiveBackground(span.attrs, this.currentForeground, this.currentPalette256);
			if (bg !== null) {
				const x = span.startCol * this.charWidth;
				const width = span.cellCount * this.charWidth;
				this.ctx.fillStyle = rgbToCSS(bg);
				this.ctx.fillRect(x, fillY, width, fillHeight);
			}
		}
	}

	/**
	 * Render only the text of a line (no background clearing).
	 *
	 * @param rowIndex - Row index (0-based)
	 * @param line - Line to render
	 */
	private renderLineText(rowIndex: number, line: LineAccessor): void {
		// Group cells into spans
		const spans = groupCellsIntoSpans(line);

		// Render text for each span
		for (const span of spans) {
			this.renderSpanText(span, rowIndex);
		}

		// Draw underlines for detected URLs and file paths
		this.renderDetectionUnderlines(rowIndex, line);
	}

	/**
	 * Render backgrounds from pre-parsed spans (packed path).
	 * Same logic as renderLineBackground but avoids re-parsing line.
	 */
	private renderLineBackgroundFromSpans(rowIndex: number, spans: TextSpan[]): void {
		const y = rowIndex * this.charHeight;
		const fillY = Math.floor(y);
		const fillNextY = Math.ceil((rowIndex + 1) * this.charHeight);
		const fillHeight = fillNextY - fillY;
		const canvasWidth = this.canvas.width / this.dpr;

		this.ctx.fillStyle = rgbToCSS(this.currentBackground);
		this.ctx.fillRect(0, fillY, canvasWidth, fillHeight);

		for (const span of spans) {
			const bg = getEffectiveBackground(span.attrs, this.currentForeground, this.currentPalette256);
			if (bg !== null) {
				const x = span.startCol * this.charWidth;
				const width = span.cellCount * this.charWidth;
				this.ctx.fillStyle = rgbToCSS(bg);
				this.ctx.fillRect(x, fillY, width, fillHeight);
			}
		}
	}

	/**
	 * Render text from pre-parsed spans (packed path).
	 * Same logic as renderLineText but avoids re-parsing line.
	 */
	private renderLineTextFromSpans(rowIndex: number, spans: TextSpan[]): void {
		for (const span of spans) {
			this.renderSpanText(span, rowIndex);
		}
		this.renderDetectionUnderlinesFromSpans(rowIndex, spans);
	}

	/**
	 * Draw underlines for detected URLs and file paths from pre-parsed spans.
	 * Joins soft-wrapped lines into a logical line for cross-line URL detection.
	 */
	private renderDetectionUnderlinesFromSpans(rowIndex: number, _spans: TextSpan[]): void {
		this.renderDetectionUnderlinesLogical(rowIndex);
	}

	/**
	 * Draw underlines for detected URLs and file paths in a line.
	 * Joins soft-wrapped lines into a logical line for cross-line URL detection.
	 */
	private renderDetectionUnderlines(rowIndex: number, _line: LineAccessor): void {
		this.renderDetectionUnderlinesLogical(rowIndex);
	}

	/** Per-frame cache for logical line detection (keyed by startRow). */
	private detectionCache: Map<number, { logical: LogicalLine; urls: UrlMatch[]; fps: FilePathMatch[] }> = new Map();

	/**
	 * Shared logic for drawing detection underlines with logical line support.
	 * Builds a logical line by joining soft-wrapped physical lines,
	 * detects URLs/file paths in the full logical text, then clips
	 * each match to the current physical row for drawing.
	 *
	 * Uses renderVisibleLines when available (forceRender/scrollback path)
	 * to ensure correct line data regardless of scroll position.
	 * Results are cached per logical line startRow within a frame.
	 */
	private renderDetectionUnderlinesLogical(rowIndex: number): void {
		const cachedSettings = SettingsService.getCached();
		if (!this.pendingState) return;

		// Use scroll-aware visible lines when available, otherwise fall back to buffer
		const getLine = this.renderVisibleLines
			? (r: number): LineAccessor | null => {
				if (r < 0 || r >= this.renderVisibleLines!.length) return null;
				return this.renderVisibleLines![r] ?? null;
			}
			: (r: number): LineAccessor | null => {
				if (r < 0 || r >= this.rows) return null;
				try {
					return this.pendingState!.getActiveBuffer().getLine(r);
				} catch (e) {
					console.warn("[WARN][FRONTEND] Unexpected getLine error at row", r, e);
					return null;
				}
			};

		const logical = getLogicalLine(getLine, rowIndex, this.rows);
		if (logical.rowCount === 0) return;

		// Check per-frame cache to avoid recomputing for the same logical line
		let cached = this.detectionCache.get(logical.startRow);
		if (!cached) {
			const urls = (!cachedSettings || cachedSettings.url_detection)
				? detectUrls(logical.text) : [];
			const fps = (!cachedSettings || cachedSettings.file_path_detection)
				? detectFilePaths(logical.text) : [];
			cached = { logical, urls, fps };
			this.detectionCache.set(logical.startRow, cached);
		}

		// Only draw underline for the link under the hover position
		if (this.hoverRow < 0 || this.hoverCol < 0) return;
		// Check if hover row is part of this logical line
		if (this.hoverRow < logical.startRow || this.hoverRow >= logical.startRow + logical.rowCount) return;

		const hoverLogicalCol = physicalToLogicalCol(this.hoverRow, this.hoverCol, logical);

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

		this.drawClippedUnderlineWithCellColors(hoveredMatch.startCol, hoveredMatch.endCol, rowIndex, cached.logical, getLine);
	}

	/**
	 * Draw underline for a match clipped to a single physical row.
	 * Converts logical column range to the physical row's column range.
	 */
	private drawClippedUnderline(
		matchStart: number,
		matchEnd: number,
		rowIndex: number,
		logical: LogicalLine,
		y: number,
		color: Rgb,
	): void {
		const rowStartLogical = (rowIndex - logical.startRow) * logical.cols;
		const rowEndLogical = rowStartLogical + logical.cols;

		const clippedStart = Math.max(matchStart, rowStartLogical);
		const clippedEnd = Math.min(matchEnd, rowEndLogical);
		if (clippedStart >= clippedEnd) return;

		const physStartCol = clippedStart - rowStartLogical;
		const physEndCol = clippedEnd - rowStartLogical;
		const x = physStartCol * this.charWidth;
		const width = (physEndCol - physStartCol) * this.charWidth;
		this.drawUnderline(x, y, width, color);
	}

	/**
	 * Draw underline for a hovered link with per-cell foreground colors.
	 * Clips the match to the current physical row and resolves each cell's color.
	 */
	private drawClippedUnderlineWithCellColors(
		matchStart: number,
		matchEnd: number,
		rowIndex: number,
		logical: LogicalLine,
		getLine: (r: number) => LineAccessor | null,
	): void {
		const rowStartLogical = (rowIndex - logical.startRow) * logical.cols;
		const rowEndLogical = rowStartLogical + logical.cols;
		const clippedStart = Math.max(matchStart, rowStartLogical);
		const clippedEnd = Math.min(matchEnd, rowEndLogical);
		if (clippedStart >= clippedEnd) return;

		const y = Math.floor(rowIndex * this.charHeight);
		const line = getLine(rowIndex);
		if (!line) return;

		for (let logCol = clippedStart; logCol < clippedEnd; logCol++) {
			const physCol = logCol - rowStartLogical;
			const cell = line.getCell(physCol);
			const fg = getEffectiveForeground(cell.attrs, this.currentForeground, this.currentBackground, this.currentPalette256, this.boldBrightensAnsiColors);
			const x = physCol * this.charWidth;
			this.drawUnderline(x, y, this.charWidth, fg);
		}
	}

	/**
	 * Render only the text portion of a span (no background).
	 *
	 * @param span - Text span to render
	 * @param rowIndex - Row index for Y position calculation
	 */
	private renderSpanText(span: TextSpan, rowIndex: number): void {
		const x = span.startCol * this.charWidth;
		// Use integer-aligned Y coordinate to match background rendering
		// This prevents gaps between block characters and their backgrounds
		const y = Math.floor(rowIndex * this.charHeight);
		const width = span.cellCount * this.charWidth;

		// Get foreground color (use current theme colors for defaults)
		const fg = getEffectiveForeground(span.attrs, this.currentForeground, this.currentBackground, this.currentPalette256, this.boldBrightensAnsiColors);

		// Get text attribute styles
		const styles = applyTextAttributes(span.attrs);

		// Skip text rendering for hidden attribute
		if (styles.hidden) {
			return;
		}

		// Save context state for dim effect
		const originalAlpha = this.ctx.globalAlpha;
		if (styles.globalAlpha !== 1) {
			this.ctx.globalAlpha = styles.globalAlpha;
		}

		// Set font style
		this.ctx.font = this.buildFontStringInternal(span.attrs);

		// Set foreground color
		this.ctx.fillStyle = rgbToCSS(fg);

		// Calculate text baseline position (vertically centered)
		const textY = y + (this.charHeight + this.fontAscent - this.fontDescent) / 2;

		// Draw each cell, using custom glyphs for block/box drawing characters
		// Uses cell boundary info to correctly handle multi-codepoint cluster strings
		let col = span.startCol;
		for (const [cellChar, cellWidth] of span.cells) {
			const charX = col * this.charWidth;
			// Try custom glyph rendering first (for block elements and box drawing)
			if (cellChar.length === 1 && isCustomGlyph(cellChar)) {
				drawCustomGlyph(this.ctx, cellChar, charX, y, this.charWidth, this.charHeight);
			} else if (cellWidth >= 2) {
				// Wide character (emoji/CJK) - fit glyph within allocated cells
				this.drawWideCharacter(cellChar, charX, textY, cellWidth);
			} else if (cellChar.charCodeAt(0) > 0x7F) {
				// Non-ASCII narrow character: may overflow 1 cell (e.g. ■, ○, △)
				this.drawFittedCharacter(cellChar, charX, textY);
			} else {
				// ASCII character - always fits in 1 cell with monospace font
				this.ctx.fillText(cellChar, charX, textY);
			}
			// Advance by cell width (1 for narrow, 2 for wide/emoji)
			col += cellWidth > 0 ? cellWidth : 1;
		}

		// Draw underline (SGR underline or OSC 8 hyperlink)
		if (styles.underline || (span.attrs.hyperlinkId && span.attrs.hyperlinkId > 0)) {
			this.drawUnderline(x, y, width, fg);
		}

		// Draw strikethrough
		if (styles.strikethrough) {
			this.drawStrikethrough(x, y, width, fg);
		}

		// Restore context state
		if (styles.globalAlpha !== 1) {
			this.ctx.globalAlpha = originalAlpha;
		}
	}

	/**
	 * Draw a wide character (emoji/CJK) fitted within its allocated cell space.
	 *
	 * Emoji glyphs from color emoji fonts often have different widths than
	 * the terminal grid expects (e.g., 22px glyph for 18px allocated space).
	 * This method scales oversized glyphs to fit and centers undersized ones.
	 *
	 * @param char - Character string to draw
	 * @param x - X position (left edge of allocated space)
	 * @param textY - Y position for text baseline
	 * @param cellWidth - Width in terminal cells (typically 2)
	 */
	private drawWideCharacter(char: string, x: number, textY: number, cellWidth: number): void {
		const allocatedWidth = cellWidth * this.charWidth;
		const measured = this.ctx.measureText(char).width;

		if (measured <= allocatedWidth) {
			// Glyph fits - center horizontally within allocated space
			const offset = (allocatedWidth - measured) / 2;
			this.ctx.fillText(char, x + offset, textY);
		} else {
			// Glyph is too wide - scale uniformly to preserve aspect ratio
			const scale = allocatedWidth / measured;
			this.ctx.save();
			this.ctx.translate(x + allocatedWidth / 2, textY);
			this.ctx.scale(scale, scale);
			this.ctx.fillText(char, -measured / 2, 0);
			this.ctx.restore();
		}
	}

	/**
	 * Draw a non-ASCII narrow character, shrinking it to fit 1 cell if needed.
	 *
	 * Characters like ■, ○, △ may have glyphs wider than 1 cell in some fonts.
	 * Uses measureText() with caching to detect oversized glyphs and scales them
	 * to fit within a single cell width.
	 *
	 * @param char - Character string to draw
	 * @param x - X position (left edge of cell)
	 * @param textY - Y position for text baseline
	 */
	private drawFittedCharacter(char: string, x: number, textY: number): void {
		// Force text presentation for Extended_Pictographic without VS
		const cp = char.codePointAt(0)!;
		if (isExtendedPictographic(cp) && !hasVariationSelector(char)) {
			char = char + "\uFE0E";
		}

		const fontKey = this.ctx.font;
		let fontCache = this.glyphWidthCache.get(fontKey);
		if (!fontCache) {
			fontCache = new Map();
			this.glyphWidthCache.set(fontKey, fontCache);
		}

		let measured = fontCache.get(char);
		if (measured === undefined) {
			measured = this.ctx.measureText(char).width;
			fontCache.set(char, measured);
		}

		if (measured <= this.charWidth) {
			this.ctx.fillText(char, x, textY);
		} else {
			// Shrink to fit 1 cell (same scaling technique as drawWideCharacter)
			const scale = this.charWidth / measured;
			this.ctx.save();
			this.ctx.translate(x + this.charWidth / 2, textY);
			this.ctx.scale(scale, scale);
			this.ctx.fillText(char, -measured / 2, 0);
			this.ctx.restore();
		}
	}

	/**
	 * Draw underline decoration.
	 *
	 * @param x - X position
	 * @param y - Y position (top of cell)
	 * @param width - Width of underline
	 * @param color - Color as RGB
	 */
	private drawUnderline(x: number, y: number, width: number, color: { r: number; g: number; b: number }): void {
		const underlineY = y + this.charHeight - 2;
		this.ctx.fillStyle = rgbToCSS(color);
		this.ctx.fillRect(x, underlineY, width, 1);
	}

	/**
	 * Draw strikethrough decoration.
	 *
	 * @param x - X position
	 * @param y - Y position (top of cell)
	 * @param width - Width of strikethrough
	 * @param color - Color as RGB
	 */
	private drawStrikethrough(x: number, y: number, width: number, color: { r: number; g: number; b: number }): void {
		const strikeY = y + this.charHeight / 2;
		this.ctx.fillStyle = rgbToCSS(color);
		this.ctx.fillRect(x, strikeY, width, 1);
	}

	/**
	 * Build font string from attributes (internal method).
	 *
	 * @param attrs - Cell attributes
	 * @returns CSS font string
	 */
	private buildFontStringInternal(attrs: CellAttributes): string {
		return buildFontString(attrs, this.fontSize, this.fontFamily);
	}

	/**
	 * Render cursor at the specified position.
	 *
	 * @param col - Cursor column
	 * @param row - Cursor row
	 * @param visible - Whether cursor is visible
	 * @param style - Cursor style
	 * @param blink - Whether cursor should blink
	 */
	private renderCursor(
		col: number,
		row: number,
		visible: boolean,
		style: CursorStyle,
		blink: boolean = true,
		state?: TerminalState,
	): void {
		// Check if cursor should be visible (considering blink state)
		if (!visible || (blink && !this.cursorBlinkVisible)) {
			return;
		}

		const x = col * this.charWidth;
		const y = row * this.charHeight;

		// Use current cursor color
		const cursorColorCSS = rgbToCSS(this.currentCursorColor);
		this.ctx.fillStyle = cursorColorCSS;
		this.ctx.strokeStyle = cursorColorCSS;

		switch (style) {
			case "block": {
				// Determine cursor width based on character at cursor position
				let cursorPixelWidth = this.charWidth;
				let cell: Cell | undefined;
				if (state) {
					const buffer = state.getActiveBuffer();
					const line = buffer.getLine(row);
					cell = line.getCell(col);
					if (cell.width >= 2) {
						cursorPixelWidth = cell.width * this.charWidth;
					}
				}
				this.ctx.fillRect(x, y, cursorPixelWidth, this.charHeight);
				// Draw the character underneath with inverted color
				if (cell && cell.char !== " " && cell.char !== "") {
					const bg = getEffectiveBackground(cell.attrs, this.currentForeground, this.currentPalette256);
					this.ctx.fillStyle = rgbToCSS(bg ?? this.currentBackground);
					this.ctx.font = this.buildFontStringInternal(cell.attrs);
					const textY = y + (this.charHeight + this.fontAscent - this.fontDescent) / 2;
					if (cell.width >= 2) {
						this.drawWideCharacter(cell.char, x, textY, cell.width);
					} else if (cell.char.charCodeAt(0) > 0x7F) {
						this.drawFittedCharacter(cell.char, x, textY);
					} else {
						this.ctx.fillText(cell.char, x, textY);
					}
				}
				break;
			}
			case "underline":
				this.ctx.fillRect(x, y + this.charHeight - 2, this.charWidth, 2);
				break;
			case "bar":
				this.ctx.fillRect(x, y, 2, this.charHeight);
				break;
		}
	}

	/**
	 * Start cursor blink timer.
	 */
	startCursorBlink(): void {
		// Stop existing timer if any
		this.stopCursorBlink();

		// Start new timer (500ms interval)
		this.cursorBlinkTimer = setInterval(() => {
			this.cursorBlinkVisible = !this.cursorBlinkVisible;
			// Re-render cursor area
			if (this.pendingState) {
				// Force cursor row to be re-rendered for blink
				this.renderCursorArea(this.pendingState);
			}
		}, 500);
	}

	/**
	 * Re-render cursor area for blink effect.
	 * This clears the cursor cell and redraws it based on blink state.
	 */
	private renderCursorArea(state: TerminalState): void {
		if (this.scrollOffset > 0) {
			return;
		}

		const buffer = state.getActiveBuffer();
		const row = state.cursorRow;
		const col = state.cursorCol;

		// Re-render the cursor cell to clear previous cursor
		const line = buffer.getLine(row);
		const y = row * this.charHeight;
		const x = col * this.charWidth;

		// Clear just the cursor cell with current background
		// Use integer-aligned Y to match renderLine
		const cell = line.getCell(col);
		const cellPixelWidth = cell.width >= 2 ? cell.width * this.charWidth : this.charWidth;
		const fillY = Math.floor(y);
		const fillNextY = Math.ceil((row + 1) * this.charHeight);
		const fillHeight = fillNextY - fillY;
		this.ctx.fillStyle = rgbToCSS(this.currentBackground);
		this.ctx.fillRect(x, fillY, cellPixelWidth, fillHeight);

		// Re-draw the character at cursor position if any
		if (cell.char !== " " && cell.char !== "") {
			const fg = getEffectiveForeground(cell.attrs, this.currentForeground, this.currentBackground, this.currentPalette256, this.boldBrightensAnsiColors);
			this.ctx.fillStyle = rgbToCSS(fg);
			this.ctx.font = this.buildFontStringInternal(cell.attrs);
			const textY = y + (this.charHeight + this.fontAscent - this.fontDescent) / 2;
			if (cell.width >= 2) {
				this.drawWideCharacter(cell.char, x, textY, cell.width);
			} else if (cell.char.charCodeAt(0) > 0x7F) {
				this.drawFittedCharacter(cell.char, x, textY);
			} else {
				this.ctx.fillText(cell.char, x, textY);
			}
		}

		// Draw cursor if blink state is visible
		this.renderCursor(
			col,
			row,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
			state,
		);
	}

	/**
	 * Stop cursor blink timer.
	 */
	stopCursorBlink(): void {
		if (this.cursorBlinkTimer !== null) {
			clearInterval(this.cursorBlinkTimer);
			this.cursorBlinkTimer = null;
		}
		// Reset to visible state
		this.cursorBlinkVisible = true;
	}

	/**
	 * Force a full re-render.
	 * Uses two-pass rendering to prevent descenders from being clipped:
	 * 1. First pass: Render all backgrounds
	 * 2. Second pass: Render all text (so descenders aren't overwritten)
	 *
	 * @param state - Terminal state to render
	 */
	forceRender(state: TerminalState): void {
		this.pendingState = state;

		const foldManager = state.getFoldManager();
		const collapsedRegions = foldManager.getCollapsedRegions();
		const hasFolds = collapsedRegions.length > 0;

		// Get visible lines based on scroll offset (fold-aware)
		const visibleLines = hasFolds
			? this.getVisibleLinesWithFolding(state, foldManager)
			: getVisibleLines(state, this.scrollOffset);

		// Store visible lines for scroll-aware URL detection in renderDetectionUnderlinesLogical
		this.renderVisibleLines = visibleLines;

		// Clear entire canvas including bottom/right remainder
		this.ctx.fillStyle = rgbToCSS(this.currentBackground);
		const canvasWidth = this.canvas.width / this.dpr;
		const canvasHeight = this.canvas.height / this.dpr;
		this.ctx.fillRect(0, 0, canvasWidth, canvasHeight);

		// Pre-parse packed data for visible rows (parse once per row, FR10)
		const packedSpans: (TextSpan[] | null)[] = new Array(visibleLines.length).fill(null);
		if (!hasFolds) {
			const packedRows = this.getVisibleRowsPacked(state, this.scrollOffset, visibleLines.length);
			for (let row = 0; row < visibleLines.length; row++) {
				const packed = packedRows[row];
				if (packed) {
					packedSpans[row] = groupPackedCellsIntoSpans(packed, this.cols);
				}
			}
		}

		// Two-pass rendering to prevent descender clipping:
		// First pass: Render all backgrounds
		for (let row = 0; row < visibleLines.length; row++) {
			const line = visibleLines[row];
			if (line === null) {
				// Summary line placeholder - rendered in summary pass
			} else if (line) {
				const spans = packedSpans[row];
				if (spans) {
					this.renderLineBackgroundFromSpans(row, spans);
				} else {
					this.renderLineBackground(row, line);
				}
			}
		}

		// Second pass: Render all text (descenders won't be overwritten)
		for (let row = 0; row < visibleLines.length; row++) {
			const line = visibleLines[row];
			if (line === null) {
				// Summary line placeholder - rendered in summary pass
			} else if (line) {
				const spans = packedSpans[row];
				if (spans) {
					this.renderLineTextFromSpans(row, spans);
				} else {
					this.renderLineText(row, line);
				}
			}
		}

		// Fold summary line pass: Render summary lines for collapsed regions
		if (hasFolds) {
			this.renderFoldSummaryLines(state, visibleLines, foldManager);
		}

		// Third pass: Render search highlights over text
		if (this.searchMatches.length > 0) {
			this.renderSearchHighlights(state);
		}

		// Clear render-pass state
		this.renderVisibleLines = null;
		this.detectionCache.clear();

		// Clear dirty flags
		state.clearDirty();

		// Only render cursor when at bottom (scrollOffset = 0)
		if (this.scrollOffset === 0) {
			this.renderCursor(
				state.cursorCol,
				state.cursorRow,
				state.cursorVisible,
				state.cursorStyle,
				state.cursorBlink,
				state,
			);

			// Save current cursor position for next render
			this.prevCursorCol = state.cursorCol;
			this.prevCursorRow = state.cursorRow;
			this.prevCursorVisible = state.cursorVisible;
		}
	}

	/**
	 * Get packed binary data for visible rows, accounting for scroll offset.
	 * Returns null entries when packed data is unavailable.
	 */
	private getVisibleRowsPacked(
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
	 * Collapsed regions are replaced with null markers (summary lines).
	 */
	private getVisibleLinesWithFolding(
		state: TerminalState,
		foldManager: ReturnType<TerminalState["getFoldManager"]>,
	): (LineAccessor | null)[] {
		const buffer = state.getActiveBuffer();
		const scrollbackLength = state.getScrollbackLength();
		const visibleRows = state.rows;

		// Build combined buffer
		const totalActualLines = scrollbackLength + visibleRows;
		const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);

		// Calculate display start based on scroll offset
		const displayStart = Math.max(0, totalDisplayLines - visibleRows - this.scrollOffset);

		const result: (LineAccessor | null)[] = [];
		for (let displayRow = 0; displayRow < visibleRows; displayRow++) {
			const displayLine = displayStart + displayRow;

			// Check if this display line is a summary line
			const summaryRegion = foldManager.getSummaryRegion(displayLine);
			if (summaryRegion) {
				result.push(null); // null = summary line placeholder
				continue;
			}

			// Map display line to actual line
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
	private renderFoldSummaryLines(
		state: TerminalState,
		visibleLines: (LineAccessor | null)[],
		foldManager: ReturnType<TerminalState["getFoldManager"]>,
	): void {
		const scrollbackLength = state.getScrollbackLength();
		const totalActualLines = scrollbackLength + state.rows;
		const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
		const displayStart = Math.max(0, totalDisplayLines - state.rows - this.scrollOffset);

		for (let row = 0; row < visibleLines.length; row++) {
			if (visibleLines[row] !== null) continue;

			const displayLine = displayStart + row;
			const region = foldManager.getSummaryRegion(displayLine);
			if (!region) continue;

			this.renderSummaryLine(row, region);
		}
	}

	/**
	 * Render a single fold summary line.
	 */
	private renderSummaryLine(rowIndex: number, region: FoldRegion): void {
		const y = rowIndex * this.charHeight;
		const width = this.cols * this.charWidth;

		// Semi-transparent bar background
		this.ctx.fillStyle = "rgba(60, 60, 80, 0.3)";
		this.ctx.fillRect(0, y, width, this.charHeight);

		// Build summary text
		const icon = "\u25B6"; // ▶
		const name = region.source === "custom"
			? (region.label || "...")
			: (region.commandText || "...");
		const truncatedName = name.length > 80 ? name.substring(0, 77) + "..." : name;

		let rightText = `\u2014 ${region.lineCount} lines`;
		if (region.source === "osc133" && region.exitCode !== undefined) {
			rightText += ` (exit ${region.exitCode})`;
		}

		// Text color based on exit code
		const isError = region.source === "osc133" && region.exitCode !== undefined && region.exitCode !== 0;
		const textColor = isError ? "#ff6b6b" : "rgba(200, 200, 210, 0.7)";

		// Set font
		this.ctx.font = `${this.fontSize}px "${this.fontFamily}"`;
		this.ctx.textBaseline = "top";

		// Draw icon
		this.ctx.fillStyle = textColor;
		const textY = y + (this.charHeight - this.fontSize) / 2;
		this.ctx.fillText(`${icon} ${truncatedName}`, this.charWidth * 0.5, textY);

		// Draw right-aligned info
		const rightWidth = this.ctx.measureText(rightText).width;
		const rightX = width - rightWidth - this.charWidth * 0.5;
		this.ctx.fillStyle = textColor;
		this.ctx.fillText(rightText, rightX, textY);
	}

	/**
	 * Resize the renderer.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resize(cols: number, rows: number): void {
		this.cols = cols;
		this.rows = rows;

		// Re-setup canvas (recalculate size)
		this.setupCanvas();

		// Force re-render if we have state
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Get the font family.
	 */
	getFontFamily(): string {
		return this.fontFamily;
	}

	/**
	 * Get the font size in points.
	 */
	getFontSize(): number {
		// Convert px to pt (1pt = 1.333px at 96dpi)
		return this.fontSize * (72 / 96);
	}

	/**
	 * Set the font size dynamically.
	 * @param fontSize - New font size in points (will be converted to pixels)
	 */
	setFontSize(fontSize: number): void {
		// Convert pt to px (1pt = 1.333px at 96dpi)
		const fontSizePx = fontSize * (96 / 72);
		this.fontSize = fontSizePx;
		this.measureCharacterSize();
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Apply a setting change to the renderer.
	 * @param setting - The setting key
	 * @param value - The new value
	 */
	applySetting<K extends keyof RendererSettings>(
		setting: K,
		value: RendererSettings[K],
	): void {
		switch (setting) {
			case "fontSize":
				this.setFontSize(value as number);
				break;
			case "fontFamily":
				this.setFontFamily(value as string);
				break;
			case "cursorStyle":
				this.setCursorStyle(value as CursorStyle);
				break;
			case "cursorBlink":
				this.setCursorBlink(value as boolean);
				break;
			case "colorScheme":
				this.setColorScheme(value as string);
				break;
			case "userColorScheme":
				if (value) {
					this.setUserColorScheme(value as UserColorScheme);
				}
				break;
			case "boldBrightensAnsiColors":
				this.setBoldBrightensAnsiColors(value as boolean);
				break;
		}
	}

	/**
	 * Set the font family dynamically.
	 * @param fontFamily - New font family (empty string falls back to "monospace")
	 */
	setFontFamily(fontFamily: string): void {
		this.fontFamily = fontFamily || "monospace";
		this.measureCharacterSize();
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Set the cursor style.
	 * @param style - Cursor style ("block", "underline", or "bar")
	 */
	setCursorStyle(style: CursorStyle): void {
		if (this.pendingState) {
			this.pendingState.cursor.style = style;
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Set cursor blink mode.
	 * @param blink - Whether cursor should blink
	 */
	setCursorBlink(blink: boolean): void {
		if (this.pendingState) {
			this.pendingState.modes.cursorBlink = blink;
		}
		if (blink) {
			this.startCursorBlink();
		} else {
			this.stopCursorBlink();
			if (this.pendingState) {
				this.forceRender(this.pendingState);
			}
		}
	}

	/**
	 * Set the color scheme.
	 * @param schemeName - Color scheme name (e.g., "emterm", "solarized-dark")
	 */
	setColorScheme(schemeName: string): void {
		const preset = getColorSchemePreset(schemeName);

		if (!preset || schemeName === "emterm") {
			// Reset to defaults
			this.currentForeground = DEFAULT_FOREGROUND;
			this.currentBackground = DEFAULT_BACKGROUND;
			this.currentCursorColor = { r: 0, g: 128, b: 0 };
			this.currentPalette16 = PALETTE_16;
		} else {
			// Apply preset colors
			this.currentForeground = preset.foreground;
			this.currentBackground = preset.background;
			this.currentCursorColor = preset.cursor;
			this.currentPalette16 = preset.ansiColors;
		}

		// Rebuild full 256-palette with updated ANSI 16 colors
		this.currentPalette256 = buildPalette256(this.currentPalette16);

		// Force full re-render
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Set a user-defined color scheme.
	 * Used for custom color schemes stored in settings.
	 * @param scheme - User color scheme with hex color values
	 */
	setUserColorScheme(scheme: UserColorScheme): void {
		const fg = hexToRgb(scheme.foreground);
		const bg = hexToRgb(scheme.background);
		const cursor = hexToRgb(scheme.cursor);

		if (fg) this.currentForeground = fg;
		if (bg) this.currentBackground = bg;
		if (cursor) this.currentCursorColor = cursor;

		// Convert ANSI colors from hex to Rgb
		const ansiColors: Rgb[] = [];
		for (const hex of scheme.ansi_colors) {
			const rgb = hexToRgb(hex);
			if (rgb) {
				ansiColors.push(rgb);
			}
		}
		if (ansiColors.length === 16) {
			this.currentPalette16 = ansiColors;
		}

		// Rebuild full 256-palette with updated ANSI 16 colors
		this.currentPalette256 = buildPalette256(this.currentPalette16);

		// Force full re-render
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Set bold-brightens ANSI colors behavior.
	 * @param enabled - Whether bold should brighten standard ANSI colors
	 */
	setBoldBrightensAnsiColors(enabled: boolean): void {
		this.boldBrightensAnsiColors = enabled;
		if (this.pendingState) {
			this.forceRender(this.pendingState);
		}
	}

	/**
	 * Get character width in pixels.
	 */
	getCharWidth(): number {
		return this.charWidth;
	}

	/**
	 * Get character height in pixels.
	 */
	getCharHeight(): number {
		return this.charHeight;
	}

	/**
	 * Render visual selection highlight.
	 *
	 * @param selection - Selection range to highlight
	 */
	renderSelection(selection: {
		start: { col: number; row: number };
		end: { col: number; row: number };
	}): void {
		// Ensure selection container exists
		if (!this.selectionContainer) {
			this.selectionContainer = document.createElement("div");
			this.selectionContainer.className = "terminal-selection-container";
			this.selectionContainer.style.cssText = `
				position: absolute;
				top: 0;
				left: 0;
				right: 0;
				bottom: 0;
				pointer-events: none;
				z-index: 1;
			`;
			// Only set position if not already set to avoid overriding existing layout
			const computedPosition = window.getComputedStyle(this.container).position;
			if (computedPosition === "static") {
				this.container.style.position = "relative";
			}
			this.container.appendChild(this.selectionContainer);
		}

		// Clear existing overlays
		this.clearSelectionOverlays();

		// Normalize selection (ensure start comes before end)
		let { start, end } = selection;
		if (start.row > end.row || (start.row === end.row && start.col > end.col)) {
			[start, end] = [end, start];
		}

		// Create overlay for each line in selection
		for (let row = start.row; row <= end.row; row++) {
			let colStart: number;
			let colEnd: number;

			if (row === start.row && row === end.row) {
				// Single line selection
				colStart = start.col;
				colEnd = end.col;
			} else if (row === start.row) {
				// First line - from start to end of line
				colStart = start.col;
				colEnd = this.cols - 1;
			} else if (row === end.row) {
				// Last line - from beginning to end position
				colStart = 0;
				colEnd = end.col;
			} else {
				// Middle line - entire line
				colStart = 0;
				colEnd = this.cols - 1;
			}

			// Create overlay element
			const overlay = document.createElement("div");
			overlay.className = "terminal-selection-overlay";
			overlay.style.cssText = `
				position: absolute;
				left: ${colStart * this.charWidth}px;
				top: ${row * this.charHeight}px;
				width: ${(colEnd - colStart + 1) * this.charWidth}px;
				height: ${this.charHeight}px;
				background-color: rgba(50, 150, 250, 0.3);
				pointer-events: none;
			`;

			this.selectionContainer.appendChild(overlay);
			this.selectionOverlays.push(overlay);
		}
	}

	/**
	 * Clear selection overlay elements.
	 */
	private clearSelectionOverlays(): void {
		for (const overlay of this.selectionOverlays) {
			overlay.remove();
		}
		this.selectionOverlays = [];
	}

	/**
	 * Clear all selection highlights.
	 */
	clearSelectionHighlight(): void {
		this.clearSelectionOverlays();
	}

	/**
	 * Scroll up in the scrollback buffer (toward past).
	 * @param lines - Number of lines to scroll up
	 */
	scrollUp(lines: number): void {
		if (!this.pendingState) return;

		const maxOffset = this.pendingState.getScrollbackLength();
		this.scrollOffset = Math.min(this.scrollOffset + lines, maxOffset);
	}

	/**
	 * Scroll down in the scrollback buffer (toward present).
	 * @param lines - Number of lines to scroll down
	 */
	scrollDown(lines: number): void {
		this.scrollOffset = Math.max(this.scrollOffset - lines, 0);
	}

	/**
	 * Get current scroll offset.
	 * @returns Number of lines scrolled back (0 = at bottom/present)
	 */
	getScrollOffset(): number {
		return this.scrollOffset;
	}

	/**
	 * Set scroll offset directly for programmatic scroll positioning.
	 * @param offset - Number of lines to scroll back (0 = at bottom)
	 */
	setScrollOffset(offset: number): void {
		if (!this.pendingState) return;

		const maxOffset = this.pendingState.getScrollbackLength();
		this.scrollOffset = Math.max(0, Math.min(offset, maxOffset));
	}

	/**
	 * Set search matches for highlight rendering.
	 * @param matches - Array of search matches
	 * @param currentIndex - Index of the current/active match (-1 for none)
	 */
	setSearchHighlights(matches: SearchMatch[], currentIndex: number): void {
		this.searchMatches = matches;
		this.searchCurrentIndex = currentIndex;
	}

	/**
	 * Clear all search highlights.
	 */
	clearSearchHighlights(): void {
		this.searchMatches = [];
		this.searchCurrentIndex = -1;
	}

	/**
	 * Set the hover position for link underline rendering.
	 * Triggers a re-render only when the cell position changes.
	 */
	setHoverPosition(row: number, col: number): void {
		if (row === this.hoverRow && col === this.hoverCol) return;
		this.hoverRow = row;
		this.hoverCol = col;
		if (this.pendingState) {
			this.scheduleRender(this.pendingState);
		}
	}

	/**
	 * Render search match highlights on the canvas.
	 * Called after text rendering in forceRender.
	 */
	private renderSearchHighlights(state: TerminalState): void {
		const scrollbackLength = state.getScrollbackLength();
		const foldManager = state.getFoldManager();
		const hasFolds = foldManager.getCollapsedRegions().length > 0;

		// Calculate visible range in display coordinates
		const totalActualLines = scrollbackLength + state.rows;
		const totalDisplayLines = hasFolds
			? foldManager.getTotalDisplayLines(totalActualLines)
			: totalActualLines;
		const displayStart = Math.max(0, totalDisplayLines - state.rows - this.scrollOffset);
		const displayEnd = displayStart + state.rows;

		for (let i = 0; i < this.searchMatches.length; i++) {
			const match = this.searchMatches[i];
			if (!match) continue;

			// Skip if match is inside a collapsed region
			if (hasFolds) {
				const region = foldManager.getRegionAtLine(match.lineIndex);
				if (region && region.collapsed) continue;
			}

			// Convert actual line index to display line
			const displayLine = hasFolds
				? foldManager.actualLineToDisplay(match.lineIndex)
				: match.lineIndex;

			// Skip if outside visible display range
			if (displayLine < displayStart || displayLine >= displayEnd) {
				continue;
			}

			// Convert to screen row
			const screenRow = displayLine - displayStart;

			const x = match.startCol * this.charWidth;
			const y = Math.floor(screenRow * this.charHeight);
			const width = (match.endCol - match.startCol) * this.charWidth;
			const height = Math.ceil(this.charHeight);

			if (i === this.searchCurrentIndex) {
				// Current match: orange highlight
				this.ctx.fillStyle = "rgba(230, 150, 30, 0.45)";
			} else {
				// Other matches: yellow highlight
				this.ctx.fillStyle = "rgba(230, 230, 50, 0.3)";
			}
			this.ctx.fillRect(x, y, width, height);
		}
	}

	/**
	 * Dispose of the renderer and clean up resources.
	 */
	dispose(): void {
		// Stop cursor blink timer
		this.stopCursorBlink();

		// Stop blink text timer
		if (this.blinkTextTimer !== null) {
			clearInterval(this.blinkTextTimer);
			this.blinkTextTimer = null;
		}

		// Remove DPR listener
		if (this.dprMediaQuery && this.dprChangeHandler) {
			this.dprMediaQuery.removeEventListener("change", this.dprChangeHandler);
		}

		// Remove canvas from DOM
		if (this.canvas.parentNode) {
			this.canvas.parentNode.removeChild(this.canvas);
		}

		// Clear selection container
		if (this.selectionContainer?.parentNode) {
			this.selectionContainer.parentNode.removeChild(this.selectionContainer);
		}
	}
}
