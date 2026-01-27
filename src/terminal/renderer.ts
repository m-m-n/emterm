/**
 * Terminal renderer for DOM output.
 *
 * Renders terminal state to HTML elements with color and style support.
 * Optimized for performance with CSS class-based styling and DOM reuse.
 */

import type { CellAttributes } from "./attributes.ts";
import {
	attributesEqual,
	getEffectiveBackground,
	getEffectiveForeground,
} from "./attributes.ts";
import { DEFAULT_BACKGROUND, DEFAULT_FOREGROUND, rgbToCSS } from "./colors.ts";
import type { CursorStyle } from "./cursor.ts";
import type { Cell, Line } from "./grid.ts";
import {
	checkFrameBudget,
	getPerformanceMonitor,
	RenderTimer,
} from "./performance.ts";
import type { TerminalState } from "./state.ts";
import { getStyleCache, type StyleCache } from "./style-cache.ts";
import type { RendererSettings } from "../settings/settings-applier";

/**
 * Terminal renderer that outputs to DOM.
 */
export class TerminalRenderer {
	/** Container element. */
	private container: HTMLElement;

	/** Font family. */
	private fontFamily: string;

	/** Font size in pixels. */
	private fontSize: number;

	/** Number of columns. */
	private cols: number = 80;

	/** Number of rows. */
	private rows: number = 24;

	/** Line elements for each row. */
	private lineElements: HTMLDivElement[] = [];

	/** Cursor element. */
	private cursorElement: HTMLDivElement | null = null;

	/** Character width in pixels. */
	private charWidth: number = 0;

	/** Character height in pixels. */
	private charHeight: number = 0;

	/** Pending render flag. */
	private renderPending: boolean = false;

	/** Current state to render. */
	private pendingState: TerminalState | null = null;

	/** Cursor blink animation interval. */
	private blinkInterval: ReturnType<typeof setInterval> | null = null;

	/** Style cache for CSS class-based rendering. */
	private styleCache: StyleCache;

	/** Performance timer. */
	private renderTimer: RenderTimer = new RenderTimer();

	/** Reusable span pool for DOM element reuse. */
	private spanPool: HTMLSpanElement[] = [];

	/** Maximum spans to keep in pool. */
	private readonly maxPoolSize: number = 500;

	/** Padding offset for cursor positioning. */
	private paddingOffset: number = 0;

	/** Selection overlay container. */
	private selectionContainer: HTMLDivElement | null = null;

	/** Selection overlay elements for each line. */
	private selectionOverlays: HTMLDivElement[] = [];

	/** Last rendered content hash per row (for skip optimization). */
	private lastRowHash: Map<number, string> = new Map();

	/** Use optimized rendering mode. */
	private useOptimizedRendering: boolean = true;

	/** Track which buffer was last rendered (to detect buffer switches). */
	private lastRenderedAlternateBuffer: boolean = false;

	/**
	 * Create a new terminal renderer.
	 *
	 * @param container - Container element
	 * @param fontFamily - Font family for terminal text
	 * @param fontSize - Font size in pixels
	 */
	constructor(container: HTMLElement, fontFamily: string, fontSize: number) {
		this.container = container;
		this.fontFamily = fontFamily;
		this.fontSize = fontSize;

		// Apply styles to container
		this.container.style.fontFamily = fontFamily;
		this.container.style.fontSize = `${fontSize}px`;
		// Read lineHeight from CSS (CSS variables as single source of truth)
		const containerStyle = window.getComputedStyle(container);
		const lineHeight = containerStyle.lineHeight || "1.2";
		this.container.style.lineHeight = lineHeight;
		this.container.style.whiteSpace = "pre";
		this.container.style.overflow = "hidden";
		this.container.style.position = "relative";

		// Initialize style cache
		this.styleCache = getStyleCache();

		// Get padding offset for cursor positioning
		this.paddingOffset = parseFloat(containerStyle.paddingLeft) || 0;

		// Measure character dimensions
		this.measureCharacterSize();

		// Create cursor element
		this.createCursorElement();

		// Add CSS for cursor blink animation
		this.addCursorStyles();
	}

	/**
	 * Measure the size of a single character.
	 */
	private measureCharacterSize(): void {
		// Read lineHeight from container's computed style
		const computedStyle = window.getComputedStyle(this.container);
		const lineHeight = computedStyle.lineHeight || "1.2";

		const measureSpan = document.createElement("span");
		measureSpan.style.fontFamily = this.fontFamily;
		measureSpan.style.fontSize = `${this.fontSize}px`;
		measureSpan.style.lineHeight = lineHeight;
		measureSpan.style.visibility = "hidden";
		measureSpan.style.position = "absolute";
		measureSpan.textContent = "W";
		document.body.appendChild(measureSpan);

		const rect = measureSpan.getBoundingClientRect();
		this.charWidth = rect.width;
		this.charHeight = rect.height;

		document.body.removeChild(measureSpan);
	}

	/**
	 * Create the cursor element.
	 */
	private createCursorElement(): void {
		this.cursorElement = document.createElement("div");
		this.cursorElement.className = "terminal-cursor";
		this.cursorElement.style.position = "absolute";
		this.cursorElement.style.width = `${this.charWidth}px`;
		this.cursorElement.style.height = `${this.charHeight}px`;
		this.cursorElement.style.pointerEvents = "none";
		this.container.appendChild(this.cursorElement);
	}

	/**
	 * Add CSS styles for cursor.
	 */
	private addCursorStyles(): void {
		// Check if styles already exist
		if (document.getElementById("terminal-cursor-styles")) {
			return;
		}

		const style = document.createElement("style");
		style.id = "terminal-cursor-styles";
		style.textContent = `
      @keyframes cursor-blink {
        0%, 50% { opacity: 1; }
        51%, 100% { opacity: 0; }
      }
      .terminal-cursor.blink {
        animation: cursor-blink 1s step-end infinite;
      }
      .terminal-cursor.block {
        background-color: #008000;
      }
      .terminal-cursor.underline {
        background-color: transparent;
        border-bottom: 2px solid #008000;
      }
      .terminal-cursor.bar {
        background-color: transparent;
        border-left: 2px solid #008000;
        width: 2px !important;
      }
    `;
		document.head.appendChild(style);
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

		// Detect buffer switch (primary <-> alternate) and clear hash cache
		// This prevents stale hashes from one buffer incorrectly skipping renders in the other
		const isAlternate = state.isAlternateBuffer;
		if (isAlternate !== this.lastRenderedAlternateBuffer) {
			this.lastRowHash.clear();
			this.lastRenderedAlternateBuffer = isAlternate;
		}

		// Ensure we have the right number of line elements
		this.ensureLineElements(state.rows);

		// Update only dirty rows
		let renderedCount = 0;
		for (const rowIndex of dirtyRows) {
			const line = buffer.getLine(rowIndex);
			if (this.useOptimizedRendering) {
				this.renderLineOptimized(rowIndex, line);
			} else {
				this.renderLine(rowIndex, line);
			}
			renderedCount++;
		}

		// Flush pending CSS rules to stylesheet
		this.styleCache.flush();

		// Clear dirty flags
		state.clearDirty();

		// Update cursor display with full styling
		this.updateCursor(
			state.cursorCol,
			state.cursorRow,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
		);

		// Record performance metrics
		const duration = this.renderTimer.end();
		const monitor = getPerformanceMonitor();
		if (monitor.isEnabled()) {
			monitor.recordRender(duration);
			if (duration > 16) {
				checkFrameBudget(duration, `render ${dirtyRows.length} rows`);
			}
		}
	}

	/**
	 * Ensure we have the correct number of line elements.
	 */
	private ensureLineElements(rows: number): void {
		const prevCount = this.lineElements.length;

		// Add missing line elements
		while (this.lineElements.length < rows) {
			const div = document.createElement("div");
			div.className = "terminal-line";
			this.container.appendChild(div);
			this.lineElements.push(div);
		}

		// Remove excess line elements
		while (this.lineElements.length > rows) {
			const div = this.lineElements.pop();
			if (div) {
				this.container.removeChild(div);
			}
		}

		// Debug logging
		if (import.meta.env?.DEV && prevCount !== rows) {
			console.log("[Renderer Debug] ensureLineElements:", {
				requestedRows: rows,
				previousLineCount: prevCount,
				newLineCount: this.lineElements.length,
				containerHeight: this.container.clientHeight,
				charHeight: this.charHeight,
				expectedHeight: rows * this.charHeight,
				actualLineElements:
					this.container.querySelectorAll(".terminal-line").length,
			});
		}
	}

	/**
	 * Render a single line with color and style support.
	 */
	private renderLine(rowIndex: number, line: Line): void {
		const div = this.lineElements[rowIndex];
		if (!div) return;

		// Clear existing content
		div.innerHTML = "";

		// Group consecutive cells with the same attributes into spans
		const spans = this.groupCellsIntoSpans(line);

		for (const span of spans) {
			const element = document.createElement("span");
			element.textContent = span.text;
			this.applyStyles(element, span.attrs);
			div.appendChild(element);
		}
	}

	/**
	 * Group consecutive cells with the same attributes into spans.
	 */
	private groupCellsIntoSpans(
		line: Line,
	): Array<{ text: string; attrs: CellAttributes }> {
		const spans: Array<{ text: string; attrs: CellAttributes }> = [];
		let currentText = "";
		let currentAttrs: CellAttributes | null = null;

		for (let i = 0; i < line.length; i++) {
			const cell = line.getCell(i);

			// Skip zero-width cells (placeholders for wide characters)
			if (cell.width === 0) {
				continue;
			}

			if (currentAttrs === null) {
				// First cell
				currentAttrs = cell.attrs;
				currentText = cell.char;
			} else if (attributesEqual(currentAttrs, cell.attrs)) {
				// Same attributes, extend current span
				currentText += cell.char;
			} else {
				// Different attributes, save current span and start new one
				spans.push({ text: currentText, attrs: currentAttrs });
				currentText = cell.char;
				currentAttrs = cell.attrs;
			}
		}

		// Don't forget the last span
		if (currentText.length > 0 && currentAttrs !== null) {
			spans.push({ text: currentText, attrs: currentAttrs });
		}

		return spans;
	}

	/**
	 * Apply CSS styles to a span element based on cell attributes.
	 */
	private applyStyles(element: HTMLSpanElement, attrs: CellAttributes): void {
		const styles: string[] = [];

		// Foreground color
		const fg = getEffectiveForeground(attrs);
		styles.push(`color: ${rgbToCSS(fg)}`);

		// Background color
		const bg = getEffectiveBackground(attrs);
		if (bg !== null) {
			styles.push(`background-color: ${rgbToCSS(bg)}`);
		}

		// Bold
		if (attrs.bold) {
			styles.push("font-weight: bold");
		}

		// Dim
		if (attrs.dim) {
			styles.push("opacity: 0.5");
		}

		// Italic
		if (attrs.italic) {
			styles.push("font-style: italic");
		}

		// Underline
		if (attrs.underline) {
			styles.push("text-decoration: underline");
		}

		// Strikethrough
		if (attrs.strikethrough) {
			if (attrs.underline) {
				styles.push("text-decoration: underline line-through");
			} else {
				styles.push("text-decoration: line-through");
			}
		}

		// Blink
		if (attrs.blink) {
			styles.push("animation: blink 1s step-end infinite");
		}

		// Hidden
		if (attrs.hidden) {
			styles.push("visibility: hidden");
		}

		if (styles.length > 0) {
			element.style.cssText = styles.join("; ");
		}
	}

	/**
	 * Optimized line rendering using CSS classes and DOM reuse.
	 *
	 * This method provides significant performance improvements by:
	 * 1. Using cached CSS classes instead of inline styles
	 * 2. Reusing DOM elements from a pool
	 * 3. Skipping unchanged rows based on content hash
	 */
	private renderLineOptimized(rowIndex: number, line: Line): void {
		const div = this.lineElements[rowIndex];
		if (!div) {
			return;
		}

		// Compute content hash for skip optimization
		const contentHash = this.computeLineHash(line);
		const lastHash = this.lastRowHash.get(rowIndex);

		// Skip rendering if content hasn't changed
		if (lastHash === contentHash) {
			return;
		}
		this.lastRowHash.set(rowIndex, contentHash);

		// Group cells into spans
		const spans = this.groupCellsIntoSpans(line);

		// Reuse or create span elements
		const existingSpans = div.children;
		const neededSpans = spans.length;

		// Update existing spans in place when possible
		for (let i = 0; i < neededSpans; i++) {
			const spanData = spans[i]!;
			let element: HTMLSpanElement;

			if (i < existingSpans.length) {
				// Reuse existing span
				element = existingSpans[i] as HTMLSpanElement;
			} else {
				// Get from pool or create new
				element = this.getSpanFromPool();
				div.appendChild(element);
			}

			// Update content
			element.textContent = spanData.text;

			// Apply CSS classes
			this.applyStylesOptimized(element, spanData.attrs);
		}

		// Remove excess spans (return to pool)
		while (div.children.length > neededSpans) {
			const lastChild = div.lastChild as HTMLSpanElement;
			if (lastChild) {
				div.removeChild(lastChild);
				this.returnSpanToPool(lastChild);
			}
		}
	}

	/**
	 * Compute a hash of line content for change detection.
	 */
	private computeLineHash(line: Line): string {
		// Build a compact string representation of the line
		const parts: string[] = [];

		for (let i = 0; i < line.length; i++) {
			const cell = line.getCell(i);
			if (cell.width === 0) continue;

			// Include char and key attribute flags
			const attrs = cell.attrs;
			const flags =
				(attrs.bold ? 1 : 0) |
				(attrs.dim ? 2 : 0) |
				(attrs.italic ? 4 : 0) |
				(attrs.underline ? 8 : 0) |
				(attrs.blink ? 16 : 0) |
				(attrs.hidden ? 32 : 0) |
				(attrs.strikethrough ? 64 : 0) |
				(attrs.reverse ? 128 : 0);

			// Include color info if set
			const fg = attrs.fg;
			const bg = attrs.bg;
			const fgKey = fg
				? fg.type === "rgb"
					? `${fg.r},${fg.g},${fg.b}`
					: fg.type
				: "";
			const bgKey = bg
				? bg.type === "rgb"
					? `${bg.r},${bg.g},${bg.b}`
					: bg.type
				: "";

			parts.push(`${cell.char}:${flags}:${fgKey}:${bgKey}`);
		}

		return parts.join("|");
	}

	/**
	 * Apply styles using CSS classes (optimized method).
	 */
	private applyStylesOptimized(
		element: HTMLSpanElement,
		attrs: CellAttributes,
	): void {
		// Get color class from cache
		const colorClass = this.styleCache.getClass(attrs);

		// Get decoration classes
		const decorationClasses = this.styleCache.getDecorationClasses(attrs);

		// Build full class list
		const classList = ["term-span", colorClass];
		if (decorationClasses) {
			classList.push(decorationClasses);
		}

		element.className = classList.join(" ");
	}

	/**
	 * Get a span element from the pool or create a new one.
	 */
	private getSpanFromPool(): HTMLSpanElement {
		if (this.spanPool.length > 0) {
			return this.spanPool.pop()!;
		}
		return document.createElement("span");
	}

	/**
	 * Return a span element to the pool for reuse.
	 */
	private returnSpanToPool(span: HTMLSpanElement): void {
		if (this.spanPool.length < this.maxPoolSize) {
			// Clear the span before pooling
			span.textContent = "";
			span.className = "";
			span.style.cssText = "";
			this.spanPool.push(span);
		}
	}

	/**
	 * Update cursor display with styling.
	 *
	 * @param col - Cursor column position
	 * @param row - Cursor row position
	 * @param visible - Whether cursor is visible
	 * @param style - Cursor style (block, underline, bar)
	 * @param blink - Whether cursor should blink
	 */
	private updateCursor(
		col: number,
		row: number,
		visible: boolean,
		style: CursorStyle,
		blink: boolean,
	): void {
		if (!this.cursorElement) return;

		// Position the cursor with padding offset
		// The container has padding, so we need to offset the cursor position
		const left = col * this.charWidth + this.paddingOffset;
		const top = row * this.charHeight + this.paddingOffset;

		this.cursorElement.style.left = `${left}px`;
		this.cursorElement.style.top = `${top}px`;

		// Update visibility
		if (visible) {
			this.cursorElement.style.display = "block";
		} else {
			this.cursorElement.style.display = "none";
			return;
		}

		// Update style classes
		this.cursorElement.className = "terminal-cursor";
		this.cursorElement.classList.add(style);

		if (blink) {
			this.cursorElement.classList.add("blink");
		}

		// Ensure proper dimensions based on style
		if (style === "bar") {
			this.cursorElement.style.width = "2px";
		} else {
			this.cursorElement.style.width = `${this.charWidth}px`;
		}
		this.cursorElement.style.height = `${this.charHeight}px`;
	}

	/**
	 * Resize the renderer.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resize(cols: number, rows: number): void {
		if (import.meta.env?.DEV) {
			console.log("[Renderer Debug] resize() called:", {
				newCols: cols,
				newRows: rows,
				oldCols: this.cols,
				oldRows: this.rows,
				containerHeight: this.container.clientHeight,
				charHeight: this.charHeight,
			});
		}

		this.cols = cols;
		this.rows = rows;

		// Recalculate padding offset in case CSS changed
		const computedStyle = window.getComputedStyle(this.container);
		this.paddingOffset = parseFloat(computedStyle.paddingLeft) || 0;

		// Clear row hash cache
		this.lastRowHash.clear();

		// Clear span pool (dimensions may have changed)
		this.spanPool = [];

		// Clear line elements - will be recreated by forceRender
		this.lineElements = [];
		this.container.innerHTML = "";

		// Re-create cursor element
		this.createCursorElement();
	}

	/**
	 * Force a full re-render.
	 *
	 * @param state - Terminal state to render
	 */
	forceRender(state: TerminalState): void {
		if (import.meta.env?.DEV) {
			console.log("[Renderer Debug] forceRender() called:", {
				stateRows: state.rows,
				stateCols: state.cols,
				rendererRows: this.rows,
				rendererCols: this.cols,
				containerClientHeight: this.container.clientHeight,
				charHeight: this.charHeight,
			});
		}

		this.pendingState = state;

		const buffer = state.getActiveBuffer();

		// Clear all caches
		this.lastRowHash.clear();

		// Clear all line elements
		this.lineElements = [];
		this.container.innerHTML = "";

		// Re-create cursor element
		this.createCursorElement();

		// Re-render everything
		this.ensureLineElements(state.rows);

		for (let row = 0; row < state.rows; row++) {
			const line = buffer.getLine(row);
			if (this.useOptimizedRendering) {
				this.renderLineOptimized(row, line);
			} else {
				this.renderLine(row, line);
			}
		}

		// Flush CSS rules
		this.styleCache.flush();

		state.clearDirty();
		this.updateCursor(
			state.cursorCol,
			state.cursorRow,
			state.cursorVisible,
			state.cursorStyle,
			state.cursorBlink,
		);
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
		return this.fontSize;
	}

	/**
	 * Set the font size dynamically.
	 * @param fontSize - New font size in points
	 */
	setFontSize(fontSize: number): void {
		this.fontSize = fontSize;
		this.container.style.fontSize = `${fontSize}pt`;
		// Re-measure character size with new font size
		this.measureCharacterSize();
		// Re-render if we have pending state
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
			// Future cases:
			// case "colorScheme":
			//   this.setColorScheme(value as ColorScheme);
			//   break;
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
	 * Enable or disable optimized rendering.
	 *
	 * @param enabled - Whether to use optimized rendering
	 */
	setOptimizedRendering(enabled: boolean): void {
		this.useOptimizedRendering = enabled;
		// Clear hash cache when switching modes
		this.lastRowHash.clear();
	}

	/**
	 * Check if optimized rendering is enabled.
	 */
	isOptimizedRenderingEnabled(): boolean {
		return this.useOptimizedRendering;
	}

	/**
	 * Get style cache metrics for debugging.
	 */
	getStyleCacheMetrics(): {
		cachedClasses: number;
		hits: number;
		misses: number;
		hitRate: number;
	} {
		return this.styleCache.getMetrics();
	}

	/**
	 * Reset style cache (useful after major style changes).
	 */
	resetStyleCache(): void {
		this.styleCache.reset();
		this.lastRowHash.clear();
	}

	/**
	 * Render visual selection highlight.
	 *
	 * Adds the .terminal-selected CSS class to cells in the selection range.
	 * Automatically normalizes the selection (ensures start comes before end).
	 *
	 * @param selection - Selection range to highlight
	 *
	 * @example
	 * ```ts
	 * const selection = { start: { col: 5, row: 2 }, end: { col: 10, row: 2 } };
	 * renderer.renderSelection(selection);
	 * ```
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
			// Insert before first line element or append
			if (this.container.firstChild) {
				this.container.insertBefore(
					this.selectionContainer,
					this.container.firstChild,
				);
			} else {
				this.container.appendChild(this.selectionContainer);
			}
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
	 *
	 * Removes all selection overlay elements.
	 *
	 * @example
	 * ```ts
	 * renderer.clearSelectionHighlight();
	 * ```
	 */
	clearSelectionHighlight(): void {
		this.clearSelectionOverlays();
	}

	/**
	 * Dispose of the renderer and clean up resources.
	 */
	dispose(): void {
		// Stop blink animation if running
		if (this.blinkInterval !== null) {
			clearInterval(this.blinkInterval);
			this.blinkInterval = null;
		}

		// Clear selection overlays
		this.clearSelectionOverlays();

		// Remove selection container
		if (this.selectionContainer?.parentNode) {
			this.selectionContainer.parentNode.removeChild(this.selectionContainer);
		}

		// Remove cursor element
		if (this.cursorElement?.parentNode) {
			this.cursorElement.parentNode.removeChild(this.cursorElement);
		}

		// Clear line elements
		for (const lineElement of this.lineElements) {
			if (lineElement.parentNode) {
				lineElement.parentNode.removeChild(lineElement);
			}
		}
		this.lineElements = [];

		// Clear span pool
		this.spanPool = [];

		// Clear hash cache
		this.lastRowHash.clear();
	}
}
