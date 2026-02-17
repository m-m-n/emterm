/**
 * Canvas 2D Renderer for terminal output.
 *
 * Renders terminal state to a Canvas element using the 2D API.
 * Optimized for high-performance scrolling with High DPI support.
 */

import type { CellAttributes, Color } from "./attributes.ts";
import {
	attributesEqual,
	getEffectiveBackground,
	getEffectiveForeground,
	unpackStyleFlags,
} from "./attributes.ts";
import { drawCustomGlyph, isCustomGlyph } from "./custom-glyphs.ts";
import {
	DEFAULT_BACKGROUND,
	DEFAULT_FOREGROUND,
	getColorSchemePreset,
	hexToRgb,
	PALETTE_16,
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
import { detectUrls, detectFilePaths } from "./url-detector.ts";
import { SettingsService } from "../settings/settings-service.ts";

/**
 * A span of text with uniform attributes.
 */
export interface TextSpan {
	text: string;
	attrs: CellAttributes;
	/** Starting column index of the span. */
	startCol: number;
	/** Number of cells this span occupies (for wide chars, this may differ from text length). */
	cellCount: number;
	/** Cell boundaries: array of [charString, cellWidth] for each cell in the span. */
	cells: Array<[string, number]>;
}

/**
 * Group cells in a line into spans with uniform attributes.
 *
 * Handles:
 * - Wide character placeholders (width=0): skipped
 * - Combining marks (width=0 with non-empty char): merged with previous span
 *
 * @param line - The line to process
 * @returns Array of text spans with their attributes
 */
export function groupCellsIntoSpans(line: LineAccessor): TextSpan[] {
	const spans: TextSpan[] = [];
	let currentText = "";
	let currentAttrs: CellAttributes | null = null;
	let currentStartCol = 0;
	let currentCellCount = 0;
	let currentCells: Array<[string, number]> = [];

	for (let i = 0; i < line.length; i++) {
		const cell = line.getCell(i);

		// Handle zero-width cells
		if (cell.width === 0) {
			// Wide character placeholder (empty char) - skip entirely
			if (cell.char === "" || cell.char === " ") {
				continue;
			}
			// Combining mark (has a character) - merge with last cell entry
			if (currentCells.length > 0) {
				const last = currentCells[currentCells.length - 1]!;
				last[0] += cell.char;
				currentText += cell.char;
			}
			continue;
		}

		if (currentAttrs === null) {
			// First cell
			currentAttrs = cell.attrs;
			currentText = cell.char;
			currentStartCol = i;
			currentCellCount = cell.width;
			currentCells = [[cell.char, cell.width]];
		} else if (attributesEqual(currentAttrs, cell.attrs)) {
			// Same attributes, extend current span
			currentText += cell.char;
			currentCellCount += cell.width;
			currentCells.push([cell.char, cell.width]);
		} else {
			// Different attributes, save current span and start new one
			spans.push({
				text: currentText,
				attrs: currentAttrs,
				startCol: currentStartCol,
				cellCount: currentCellCount,
				cells: currentCells,
			});
			currentText = cell.char;
			currentAttrs = cell.attrs;
			currentStartCol = i;
			currentCellCount = cell.width;
			currentCells = [[cell.char, cell.width]];
		}
	}

	// Don't forget the last span
	if (currentText.length > 0 && currentAttrs !== null) {
		spans.push({
			text: currentText,
			attrs: currentAttrs,
			startCol: currentStartCol,
			cellCount: currentCellCount,
			cells: currentCells,
		});
	}

	return spans;
}

/** Shared TextDecoder for UTF-8 parsing in packed binary parser. */
const utf8Decoder = new TextDecoder("utf-8");

/**
 * Compare 10 attribute bytes (fg 4 + bg 4 + flags 2) at two offsets.
 */
export function packedAttrsEqual(buf: Uint8Array, a: number, b: number): boolean {
	return buf[a] === buf[b] && buf[a + 1] === buf[b + 1] && buf[a + 2] === buf[b + 2] &&
		buf[a + 3] === buf[b + 3] && buf[a + 4] === buf[b + 4] && buf[a + 5] === buf[b + 5] &&
		buf[a + 6] === buf[b + 6] && buf[a + 7] === buf[b + 7] && buf[a + 8] === buf[b + 8] &&
		buf[a + 9] === buf[b + 9];
}

/**
 * Unpack CellAttributes from 10 binary bytes at the given offset.
 * Layout: fg(4: tag,r,g,b) + bg(4: tag,r,g,b) + flags(2: LE u16)
 */
export function unpackAttrsFromBinary(buf: Uint8Array, offset: number): CellAttributes {
	const fgTag = buf[offset]!;
	const fgR = buf[offset + 1]!;
	const fgG = buf[offset + 2]!;
	const fgB = buf[offset + 3]!;
	let fg: Color | null;
	if (fgTag === 0) fg = null;
	else if (fgTag === 1) fg = { type: "indexed", index: fgR };
	else fg = { type: "rgb", r: fgR, g: fgG, b: fgB };

	const bgTag = buf[offset + 4]!;
	const bgR = buf[offset + 5]!;
	const bgG = buf[offset + 6]!;
	const bgB = buf[offset + 7]!;
	let bg: Color | null;
	if (bgTag === 0) bg = null;
	else if (bgTag === 1) bg = { type: "indexed", index: bgR };
	else bg = { type: "rgb", r: bgR, g: bgG, b: bgB };

	const flagsLo = buf[offset + 8]!;
	const flagsHi = buf[offset + 9]!;
	const flags = flagsLo | (flagsHi << 8);

	return { ...unpackStyleFlags(flags), fg, bg };
}

/**
 * Parse packed binary row data directly into TextSpan array.
 * Avoids creating Cell, CellAttributes, or Line objects except for span attributes.
 *
 * Binary format per cell:
 *   Inline: char_len(1) + char_data(char_len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 *   Overflow: 0xFF(1) + len_hi(1) + len_lo(1) + utf8_data(len) + width(1) + fg(4) + bg(4) + flags(2 LE)
 */
export function groupPackedCellsIntoSpans(packed: Uint8Array, cols: number): TextSpan[] {
	const spans: TextSpan[] = [];
	let offset = 0;

	let currentText = "";
	let currentStartCol = 0;
	let currentCellCount = 0;
	let currentCells: Array<[string, number]> = [];
	let prevAttrOffset = -1;
	let currentAttrs: CellAttributes | null = null;

	for (let col = 0; col < cols; col++) {
		if (offset + 12 > packed.length) break;

		// Parse character
		const charLen = packed[offset++]!;
		let ch: string;
		if (charLen === 0xFF) {
			if (offset + 2 > packed.length) break;
			const lenHi = packed[offset++]!;
			const lenLo = packed[offset++]!;
			const byteLen = (lenHi << 8) | lenLo;
			if (offset + byteLen + 11 > packed.length) break; // byteLen + width(1) + attrs(10)
			ch = utf8Decoder.decode(packed.subarray(offset, offset + byteLen));
			offset += byteLen;
		} else if (charLen === 0) {
			ch = "";
		} else if (charLen === 1) {
			ch = String.fromCharCode(packed[offset++]!);
		} else {
			ch = utf8Decoder.decode(packed.subarray(offset, offset + charLen));
			offset += charLen;
		}

		// Read width
		const width = packed[offset++]!;

		// Attribute bytes start here (10 bytes)
		const attrStart = offset;
		offset += 10;

		// Handle zero-width cells
		if (width === 0) {
			if (ch === "" || ch === " ") continue; // wide char placeholder
			// Combining mark - merge with previous cell
			if (currentCells.length > 0) {
				const last = currentCells[currentCells.length - 1]!;
				last[0] += ch;
				currentText += ch;
			}
			continue;
		}

		// Fast attribute comparison: compare 10 bytes inline
		const attrsMatch = prevAttrOffset >= 0 &&
			packedAttrsEqual(packed, prevAttrOffset, attrStart);

		if (currentAttrs === null || !attrsMatch) {
			// Save previous span
			if (currentAttrs !== null) {
				spans.push({
					text: currentText,
					attrs: currentAttrs,
					startCol: currentStartCol,
					cellCount: currentCellCount,
					cells: currentCells,
				});
			}
			// Start new span
			currentAttrs = unpackAttrsFromBinary(packed, attrStart);
			currentText = ch;
			currentStartCol = col;
			currentCellCount = width;
			currentCells = [[ch, width]];
		} else {
			// Extend current span
			currentText += ch;
			currentCellCount += width;
			currentCells.push([ch, width]);
		}

		prevAttrOffset = attrStart;
	}

	// Final span
	if (currentAttrs !== null && currentText.length > 0) {
		spans.push({
			text: currentText,
			attrs: currentAttrs,
			startCol: currentStartCol,
			cellCount: currentCellCount,
			cells: currentCells,
		});
	}

	return spans;
}

/**
 * Get visible lines based on scroll offset.
 *
 * @param state - Terminal state
 * @param scrollOffset - Number of lines scrolled back (0 = current view)
 * @returns Array of lines to render
 */
export function getVisibleLines(state: TerminalState, scrollOffset: number): LineAccessor[] {
	const buffer = state.getActiveBuffer();
	const visibleRows = state.rows;

	// If not scrolled (at bottom), return current screen buffer
	if (scrollOffset === 0) {
		const linesToRender: LineAccessor[] = [];
		for (let screenRow = 0; screenRow < visibleRows; screenRow++) {
			linesToRender.push(buffer.getLine(screenRow));
		}
		return linesToRender;
	}

	// When scrolled back, use index-based access (O(visibleRows), not O(scrollbackLength))
	const scrollbackLength = state.getScrollbackLength();
	// Clamp startIndex to 0 in case scrollOffset > scrollbackLength (stale offset after clear)
	const startIndex = Math.max(0, scrollbackLength - scrollOffset);

	const linesToRender: LineAccessor[] = [];
	for (let i = 0; i < visibleRows; i++) {
		const lineIndex = startIndex + i;
		if (lineIndex < scrollbackLength) {
			linesToRender.push(state.getScrollbackLine(lineIndex));
		} else {
			linesToRender.push(buffer.getLine(lineIndex - scrollbackLength));
		}
	}

	return linesToRender;
}

/**
 * Calculate the starting index for rendering based on scroll position.
 *
 * @param scrollOffset - Number of lines scrolled back
 * @param scrollbackLength - Total number of lines in scrollback
 * @returns Starting index in the combined buffer
 */
export function calculateScrollPosition(scrollOffset: number, scrollbackLength: number): number {
	return scrollbackLength - scrollOffset;
}

/**
 * Text attribute styles for rendering.
 */
export interface TextAttributeStyles {
	/** Global alpha for dim effect (0.5 for dim, 1 otherwise). */
	globalAlpha: number;
	/** Whether text should be hidden. */
	hidden: boolean;
	/** Whether to draw underline. */
	underline: boolean;
	/** Whether to draw strikethrough. */
	strikethrough: boolean;
	/** Whether text should blink. */
	blink: boolean;
}

/**
 * Build a CSS font string from cell attributes.
 *
 * @param attrs - Cell attributes
 * @param fontSize - Font size in pixels
 * @param fontFamily - Font family name
 * @returns CSS font string (e.g., "italic bold 13px monospace")
 */
export function buildFontString(
	attrs: CellAttributes,
	fontSize: number,
	fontFamily: string,
): string {
	const parts: string[] = [];

	if (attrs.italic) {
		parts.push("italic");
	}
	if (attrs.bold) {
		parts.push("bold");
	}
	parts.push(`${fontSize}px`);
	parts.push(fontFamily);

	return parts.join(" ");
}

/**
 * Apply text attributes and return style information.
 *
 * @param attrs - Cell attributes
 * @returns Style information for rendering
 */
export function applyTextAttributes(attrs: CellAttributes): TextAttributeStyles {
	return {
		globalAlpha: attrs.dim ? 0.5 : 1,
		hidden: attrs.hidden,
		underline: attrs.underline,
		strikethrough: attrs.strikethrough,
		blink: attrs.blink,
	};
}

/**
 * Selection range with start and end positions.
 */
export interface SelectionRange {
	start: { col: number; row: number };
	end: { col: number; row: number };
}

/**
 * Normalize selection range so that start comes before end.
 *
 * @param selection - Selection range to normalize
 * @returns Normalized selection range
 */
export function normalizeSelection(selection: SelectionRange): SelectionRange {
	const { start, end } = selection;

	// If start is before or equal to end, return as-is
	if (
		start.row < end.row ||
		(start.row === end.row && start.col <= end.col)
	) {
		return { start, end };
	}

	// Swap start and end
	return { start: end, end: start };
}

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

	/** Current foreground color (can be changed by color scheme). */
	private currentForeground: Rgb = DEFAULT_FOREGROUND;

	/** Current background color (can be changed by color scheme). */
	private currentBackground: Rgb = DEFAULT_BACKGROUND;

	/** Current cursor color (can be changed by color scheme). */
	private currentCursorColor: Rgb = { r: 0, g: 128, b: 0 };

	/** Current 16-color palette (can be changed by color scheme). */
	private currentPalette16: readonly Rgb[] = PALETTE_16;

	/** Current scroll offset (number of lines scrolled back from bottom). */
	private scrollOffset: number = 0;

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

		// Measure width using 'W' (widest common character)
		const metrics = this.ctx.measureText("W");
		this.charWidth = metrics.width;

		// Calculate height from font metrics
		const ascent = metrics.fontBoundingBoxAscent ?? this.fontSize * 0.8;
		const descent = metrics.fontBoundingBoxDescent ?? this.fontSize * 0.2;
		this.fontAscent = ascent;
		this.fontDescent = descent;

		// Use font metrics (ascent + descent) as the natural line height
		this.charHeight = ascent + descent;
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
				this.render();
				this.renderPending = false;
			});
		}
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

		const buffer = state.getActiveBuffer();
		const dirtyRows = state.getDirtyRows();

		// Render dirty rows (packed path with LineAccessor fallback)
		let renderedCount = 0;
		for (const rowIndex of dirtyRows) {
			const packed = state.getRowPacked(rowIndex);
			if (packed) {
				this.renderLinePacked(rowIndex, packed);
			} else {
				const line = buffer.getLine(rowIndex);
				this.renderLine(rowIndex, line);
			}
			renderedCount++;
		}

		// Clear dirty flags
		state.clearDirty();

		// Clear previous cursor position if it moved
		// Need to re-render the previous row if:
		// 1. Cursor moved to a different row AND that row wasn't already dirty
		// 2. Cursor moved within the same row AND that row wasn't already dirty
		const cursorMoved =
			this.prevCursorCol !== state.cursorCol ||
			this.prevCursorRow !== state.cursorRow;
		const prevRowNeedsRedraw =
			this.prevCursorRow >= 0 &&
			cursorMoved &&
			!dirtyRows.includes(this.prevCursorRow);

		if (prevRowNeedsRedraw) {
			// Re-render the previous cursor row to clear the old cursor
			const prevPacked = state.getRowPacked(this.prevCursorRow);
			if (prevPacked) {
				this.renderLinePacked(this.prevCursorRow, prevPacked);
			} else {
				const prevLine = buffer.getLine(this.prevCursorRow);
				this.renderLine(this.prevCursorRow, prevLine);
			}
		}

		// Update cursor
		this.renderCursor(
			state.cursorCol,
			state.cursorRow,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
		);

		// Save current cursor position for next render
		this.prevCursorCol = state.cursorCol;
		this.prevCursorRow = state.cursorRow;

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
	 * Used for incremental rendering of dirty rows.
	 *
	 * @param rowIndex - Row index (0-based)
	 * @param line - Line to render
	 */
	private renderLine(rowIndex: number, line: LineAccessor): void {
		this.renderLineBackground(rowIndex, line);
		this.renderLineText(rowIndex, line);
	}

	/**
	 * Render a line from packed binary data (single pass: background + text).
	 * Parses packed data once via groupPackedCellsIntoSpans and uses the result
	 * for both background and text rendering (FR10).
	 *
	 * @param rowIndex - Row index (0-based)
	 * @param packed - Packed binary row data from WASM
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
			const bg = getEffectiveBackground(span.attrs, this.currentForeground);
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
			const bg = getEffectiveBackground(span.attrs, this.currentForeground);
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
	 */
	private renderDetectionUnderlinesFromSpans(rowIndex: number, spans: TextSpan[]): void {
		const cachedSettings = SettingsService.getCached();

		// Build text string from spans (same as cell-by-cell iteration)
		const textChars: string[] = new Array(this.cols).fill(" ");
		for (const span of spans) {
			let col = span.startCol;
			for (const [cellChar, cellWidth] of span.cells) {
				if (col >= 0 && col < this.cols) {
					textChars[col] = cellChar || " ";
				}
				col += cellWidth > 0 ? cellWidth : 1;
			}
		}
		const text = textChars.join("");

		const y = Math.floor(rowIndex * this.charHeight);
		const underlineColor = this.currentForeground;

		if (!cachedSettings || cachedSettings.url_detection) {
			const urlMatches = detectUrls(text);
			for (const match of urlMatches) {
				const x = match.startCol * this.charWidth;
				const width = (match.endCol - match.startCol) * this.charWidth;
				this.drawUnderline(x, y, width, underlineColor);
			}
		}

		if (!cachedSettings || cachedSettings.file_path_detection) {
			const fpMatches = detectFilePaths(text);
			for (const match of fpMatches) {
				const x = match.startCol * this.charWidth;
				const width = (match.endCol - match.startCol) * this.charWidth;
				this.drawUnderline(x, y, width, underlineColor);
			}
		}
	}

	/**
	 * Draw underlines for detected URLs and file paths in a line.
	 */
	private renderDetectionUnderlines(rowIndex: number, line: LineAccessor): void {
		const cachedSettings = SettingsService.getCached();

		// Build text string from line cells
		let text = "";
		for (let c = 0; c < line.length; c++) {
			text += line.getCell(c).char || " ";
		}

		const y = Math.floor(rowIndex * this.charHeight);
		const underlineColor = this.currentForeground;

		// URL underlines
		if (!cachedSettings || cachedSettings.url_detection) {
			const urlMatches = detectUrls(text);
			for (const match of urlMatches) {
				const x = match.startCol * this.charWidth;
				const width = (match.endCol - match.startCol) * this.charWidth;
				this.drawUnderline(x, y, width, underlineColor);
			}
		}

		// File path underlines
		if (!cachedSettings || cachedSettings.file_path_detection) {
			const fpMatches = detectFilePaths(text);
			for (const match of fpMatches) {
				const x = match.startCol * this.charWidth;
				const width = (match.endCol - match.startCol) * this.charWidth;
				this.drawUnderline(x, y, width, underlineColor);
			}
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
		const fg = getEffectiveForeground(span.attrs, this.currentForeground, this.currentBackground);

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
			} else {
				// Narrow character (ASCII, Latin, etc.)
				this.ctx.fillText(cellChar, charX, textY);
			}
			// Advance by cell width (1 for narrow, 2 for wide/emoji)
			col += cellWidth > 0 ? cellWidth : 1;
		}

		// Draw underline
		if (styles.underline) {
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
			case "block":
				this.ctx.fillRect(x, y, this.charWidth, this.charHeight);
				break;
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
		const buffer = state.getActiveBuffer();
		const row = state.cursorRow;
		const col = state.cursorCol;

		// Re-render the cursor cell to clear previous cursor
		const line = buffer.getLine(row);
		const y = row * this.charHeight;
		const x = col * this.charWidth;

		// Clear just the cursor cell with current background
		// Use integer-aligned Y to match renderLine
		const fillY = Math.floor(y);
		const fillNextY = Math.ceil((row + 1) * this.charHeight);
		const fillHeight = fillNextY - fillY;
		this.ctx.fillStyle = rgbToCSS(this.currentBackground);
		this.ctx.fillRect(x, fillY, this.charWidth, fillHeight);

		// Re-draw the character at cursor position if any
		const cell = line.getCell(col);
		if (cell.char !== " " && cell.char !== "") {
			const fg = getEffectiveForeground(cell.attrs, this.currentForeground, this.currentBackground);
			this.ctx.fillStyle = rgbToCSS(fg);
			this.ctx.font = this.buildFontStringInternal(cell.attrs);
			const textY = y + (this.charHeight + this.fontAscent - this.fontDescent) / 2;
			if (cell.width >= 2) {
				this.drawWideCharacter(cell.char, x, textY, cell.width);
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
			);

			// Save current cursor position for next render
			this.prevCursorCol = state.cursorCol;
			this.prevCursorRow = state.cursorRow;
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

		// Force full re-render
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
