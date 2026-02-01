/**
 * Canvas 2D Renderer for terminal output.
 *
 * Renders terminal state to a Canvas element using the 2D API.
 * Optimized for high-performance scrolling with High DPI support.
 */

import type { CellAttributes } from "./attributes.ts";
import {
	attributesEqual,
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
import { DEFAULT_BACKGROUND, DEFAULT_FOREGROUND, rgbToCSS } from "./colors.ts";
import type { CursorStyle } from "./cursor.ts";
import type { Line } from "./grid.ts";
import {
	checkFrameBudget,
	getPerformanceMonitor,
	RenderTimer,
} from "./performance.ts";
import type { ITerminalRenderer } from "./renderer-interface.ts";
import type { RendererSettings } from "../settings/settings-applier";
import type { TerminalState } from "./state.ts";

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
export function groupCellsIntoSpans(line: Line): TextSpan[] {
	const spans: TextSpan[] = [];
	let currentText = "";
	let currentAttrs: CellAttributes | null = null;
	let currentStartCol = 0;
	let currentCellCount = 0;

	for (let i = 0; i < line.length; i++) {
		const cell = line.getCell(i);

		// Handle zero-width cells
		if (cell.width === 0) {
			// Wide character placeholder (empty char) - skip entirely
			if (cell.char === "" || cell.char === " ") {
				continue;
			}
			// Combining mark (has a character) - merge with previous span/text
			if (currentText.length > 0) {
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
		} else if (attributesEqual(currentAttrs, cell.attrs)) {
			// Same attributes, extend current span
			currentText += cell.char;
			currentCellCount += cell.width;
		} else {
			// Different attributes, save current span and start new one
			spans.push({
				text: currentText,
				attrs: currentAttrs,
				startCol: currentStartCol,
				cellCount: currentCellCount,
			});
			currentText = cell.char;
			currentAttrs = cell.attrs;
			currentStartCol = i;
			currentCellCount = cell.width;
		}
	}

	// Don't forget the last span
	if (currentText.length > 0 && currentAttrs !== null) {
		spans.push({
			text: currentText,
			attrs: currentAttrs,
			startCol: currentStartCol,
			cellCount: currentCellCount,
		});
	}

	return spans;
}

/**
 * Get visible lines based on scroll offset.
 *
 * Note: Current implementation doesn't use scrollback buffer.
 * When scrollback is implemented, this will return lines from the scrollback.
 *
 * @param state - Terminal state
 * @param _scrollOffset - Number of lines scrolled back (0 = current view). Currently unused.
 * @returns Array of lines to render
 */
export function getVisibleLines(state: TerminalState, _scrollOffset: number): Line[] {
	const buffer = state.getActiveBuffer();
	const visibleRows = state.rows;

	// For now, just return current screen buffer lines
	// Scrollback buffer support will be added later
	// When implemented, _scrollOffset will be used to calculate which lines to return
	const linesToRender: Line[] = [];

	for (let screenRow = 0; screenRow < visibleRows; screenRow++) {
		linesToRender.push(buffer.getLine(screenRow));
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
		this.fontDescent = descent;

		// Calculate lineHeight from fontSize directly to avoid CSS computed style timing issues
		// Formula matches settings-applier.ts: lineHeight (pt) = fontSize (pt) + 2
		// this.fontSize is in px, so convert: px -> pt -> add 2 -> back to px
		const fontSizePt = this.fontSize * (72 / 96);
		const lineHeightPt = fontSizePt + 2;
		const lineHeightPx = lineHeightPt * (96 / 72);

		this.charHeight = lineHeightPx;
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
		const buffer = state.getActiveBuffer();
		const dirtyRows = state.getDirtyRows();

		// Render dirty rows
		let renderedCount = 0;
		for (const rowIndex of dirtyRows) {
			const line = buffer.getLine(rowIndex);
			this.renderLine(rowIndex, line);
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
			const prevLine = buffer.getLine(this.prevCursorRow);
			this.renderLine(this.prevCursorRow, prevLine);
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
	 * Render a single line.
	 *
	 * @param rowIndex - Row index (0-based)
	 * @param line - Line to render
	 */
	private renderLine(rowIndex: number, line: Line): void {
		const y = rowIndex * this.charHeight;

		// Clear the row with default background
		this.ctx.fillStyle = rgbToCSS(DEFAULT_BACKGROUND);
		this.ctx.fillRect(0, y, this.cols * this.charWidth, this.charHeight);

		// Group cells into spans
		const spans = groupCellsIntoSpans(line);

		// Render each span
		for (const span of spans) {
			this.renderSpan(span, rowIndex);
		}
	}

	/**
	 * Render a text span.
	 *
	 * @param span - Text span to render
	 * @param rowIndex - Row index for Y position calculation
	 */
	private renderSpan(span: TextSpan, rowIndex: number): void {
		const x = span.startCol * this.charWidth;
		const y = rowIndex * this.charHeight;
		const width = span.cellCount * this.charWidth;

		// Get effective colors
		const fg = getEffectiveForeground(span.attrs);
		const bg = getEffectiveBackground(span.attrs);

		// Draw background if not default
		if (bg !== null) {
			this.ctx.fillStyle = rgbToCSS(bg);
			this.ctx.fillRect(x, y, width, this.charHeight);
		}

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

		// Calculate text baseline position
		const textY = y + (this.charHeight - this.fontDescent);

		// Draw text (skip if blink and currently hidden - handled by blink timer)
		this.ctx.fillText(span.text, x, textY);

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

		// Cursor color (green)
		this.ctx.fillStyle = "#008000";
		this.ctx.strokeStyle = "#008000";

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

		// Clear just the cursor cell with background
		this.ctx.fillStyle = rgbToCSS(DEFAULT_BACKGROUND);
		this.ctx.fillRect(x, y, this.charWidth, this.charHeight);

		// Re-draw the character at cursor position if any
		const cell = line.getCell(col);
		if (cell.char !== " " && cell.char !== "") {
			const fg = getEffectiveForeground(cell.attrs);
			this.ctx.fillStyle = rgbToCSS(fg);
			const textY = y + (this.charHeight - this.fontDescent);
			this.ctx.fillText(cell.char, x, textY);
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
	 *
	 * @param state - Terminal state to render
	 */
	forceRender(state: TerminalState): void {
		this.pendingState = state;
		const buffer = state.getActiveBuffer();

		// Clear entire canvas
		this.ctx.fillStyle = rgbToCSS(DEFAULT_BACKGROUND);
		this.ctx.fillRect(
			0,
			0,
			this.cols * this.charWidth,
			this.rows * this.charHeight,
		);

		// Render all rows
		for (let row = 0; row < state.rows; row++) {
			const line = buffer.getLine(row);
			this.renderLine(row, line);
		}

		// Clear dirty flags
		state.clearDirty();

		// Render cursor
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
