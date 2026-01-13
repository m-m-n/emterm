/**
 * Terminal renderer for DOM output.
 *
 * Renders terminal state to HTML elements with color and style support.
 * Optimized for performance with CSS class-based styling and DOM reuse.
 */

import { MarkdownRenderer } from "../markdown/renderer.ts";
import type { MarkdownBlock } from "../markdown/types.ts";
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

	/** Markdown renderer instance. */
	private markdownRenderer: MarkdownRenderer;

	/** Markdown container element (overlay for rich content). */
	private markdownContainer: HTMLDivElement | null = null;

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

		// Initialize Markdown renderer and container
		this.markdownRenderer = new MarkdownRenderer();
		this.createMarkdownContainer();
		this.addMarkdownStyles();
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
        background-color: #c0c0c0;
      }
      .terminal-cursor.underline {
        background-color: transparent;
        border-bottom: 2px solid #c0c0c0;
      }
      .terminal-cursor.bar {
        background-color: transparent;
        border-left: 2px solid #c0c0c0;
        width: 2px !important;
      }
    `;
		document.head.appendChild(style);
	}

	/**
	 * Create the Markdown container element.
	 */
	private createMarkdownContainer(): void {
		this.markdownContainer = document.createElement("div");
		this.markdownContainer.className = "markdown-overlay";
		this.markdownContainer.style.position = "absolute";
		this.markdownContainer.style.top = "0";
		this.markdownContainer.style.left = "0";
		this.markdownContainer.style.right = "0";
		this.markdownContainer.style.zIndex = "100";
		this.markdownContainer.style.pointerEvents = "auto";
		this.markdownContainer.style.overflow = "visible";
		this.container.appendChild(this.markdownContainer);
	}

	/**
	 * Add CSS styles for Markdown rendering.
	 */
	private addMarkdownStyles(): void {
		if (document.getElementById("markdown-styles")) {
			return;
		}

		const style = document.createElement("style");
		style.id = "markdown-styles";
		style.textContent = `
      .markdown-overlay {
        z-index: 10;
      }
      .markdown-block {
        position: relative;
        background-color: var(--markdown-bg, #1e1e1e);
        border-radius: 6px;
        padding: 16px;
        margin: 8px 0;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
        color: var(--markdown-fg, #e0e0e0);
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
        font-size: 14px;
        line-height: 1.5;
      }
      .markdown-content {
        overflow-wrap: break-word;
        word-wrap: break-word;
      }
      .markdown-content h1,
      .markdown-content h2,
      .markdown-content h3,
      .markdown-content h4,
      .markdown-content h5,
      .markdown-content h6 {
        margin-top: 24px;
        margin-bottom: 16px;
        font-weight: 600;
        line-height: 1.25;
        color: var(--markdown-heading, #ffffff);
      }
      .markdown-content h1 { font-size: 2em; margin-top: 0; border-bottom: 1px solid var(--markdown-border, #30363d); padding-bottom: 0.3em; }
      .markdown-content h2 { font-size: 1.5em; border-bottom: 1px solid var(--markdown-border, #30363d); padding-bottom: 0.3em; }
      .markdown-content h3 { font-size: 1.25em; }
      .markdown-content h4 { font-size: 1em; }
      .markdown-content h5 { font-size: 0.875em; }
      .markdown-content h6 { font-size: 0.85em; color: var(--markdown-muted, #8b949e); }
      .markdown-content p { margin-top: 0; margin-bottom: 16px; }
      .markdown-content a { color: var(--markdown-link, #58a6ff); text-decoration: none; }
      .markdown-content a:hover { text-decoration: underline; }
      .markdown-content code {
        font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
        padding: 0.2em 0.4em;
        margin: 0;
        font-size: 85%;
        background-color: var(--markdown-code-bg, rgba(110, 118, 129, 0.4));
        border-radius: 6px;
      }
      .markdown-content pre {
        margin-top: 0;
        margin-bottom: 16px;
        padding: 16px;
        overflow: auto;
        font-size: 85%;
        line-height: 1.45;
        background-color: var(--markdown-pre-bg, #161b22);
        border-radius: 6px;
      }
      .markdown-content pre code {
        padding: 0;
        margin: 0;
        background-color: transparent;
        border-radius: 0;
        font-size: 100%;
      }
      .markdown-content blockquote {
        margin: 0;
        padding: 0 1em;
        color: var(--markdown-muted, #8b949e);
        border-left: 0.25em solid var(--markdown-border, #30363d);
        margin-bottom: 16px;
      }
      .markdown-content ul,
      .markdown-content ol {
        margin-top: 0;
        margin-bottom: 16px;
        padding-left: 2em;
      }
      .markdown-content li + li {
        margin-top: 0.25em;
      }
      .markdown-content table {
        border-collapse: collapse;
        margin-bottom: 16px;
        width: 100%;
        overflow: auto;
      }
      .markdown-content table th,
      .markdown-content table td {
        padding: 6px 13px;
        border: 1px solid var(--markdown-border, #30363d);
      }
      .markdown-content table tr {
        background-color: var(--markdown-table-bg, transparent);
        border-top: 1px solid var(--markdown-border, #30363d);
      }
      .markdown-content table tr:nth-child(2n) {
        background-color: var(--markdown-table-stripe, rgba(110, 118, 129, 0.1));
      }
      .markdown-content hr {
        height: 0.25em;
        padding: 0;
        margin: 24px 0;
        background-color: var(--markdown-border, #30363d);
        border: 0;
      }
      .markdown-content img {
        max-width: 100%;
        box-sizing: content-box;
        background-color: var(--markdown-bg, #1e1e1e);
      }
      /* Highlight.js theme integration */
      .markdown-content .hljs {
        color: var(--markdown-code-fg, #e0e0e0);
        background: transparent;
      }
      /* Mermaid diagrams */
      .mermaid-diagram {
        text-align: center;
        margin: 16px 0;
      }
      .mermaid-diagram svg {
        max-width: 100%;
        height: auto;
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

		// Render pending Markdown blocks
		this.renderPendingMarkdownBlocks(state);

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

		// Re-create cursor element and markdown container
		this.createCursorElement();
		this.createMarkdownContainer();
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

		// Re-create cursor element and markdown container
		this.createCursorElement();
		this.createMarkdownContainer();

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
	 * Get the font size.
	 */
	getFontSize(): number {
		return this.fontSize;
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
	 * Render pending Markdown blocks from terminal state.
	 *
	 * @param state - Terminal state to get pending blocks from
	 */
	private renderPendingMarkdownBlocks(state: TerminalState): void {
		const blocks = state.takePendingMarkdownBlocks();
		if (blocks.length === 0 || !this.markdownContainer) {
			return;
		}

		for (const block of blocks) {
			// Position block based on terminal row
			const topOffset = block.startRow * this.charHeight + this.paddingOffset;

			// Insert block into DOM
			const element = this.markdownRenderer.insertBlock(
				block,
				this.markdownContainer,
			);

			// Position the block
			element.style.marginTop = `${topOffset}px`;

			// Calculate row count after insertion (based on actual height)
			const rect = element.getBoundingClientRect();
			block.rowCount = Math.ceil(rect.height / this.charHeight);
		}
	}

	/**
	 * Insert a Markdown block at the specified position.
	 *
	 * @param block - Markdown block to insert
	 * @returns The created DOM element
	 */
	insertMarkdownBlock(block: MarkdownBlock): HTMLElement | null {
		if (!this.markdownContainer) {
			return null;
		}

		const topOffset = block.startRow * this.charHeight + this.paddingOffset;
		const element = this.markdownRenderer.insertBlock(
			block,
			this.markdownContainer,
		);
		element.style.marginTop = `${topOffset}px`;

		// Calculate row count after insertion
		const rect = element.getBoundingClientRect();
		block.rowCount = Math.ceil(rect.height / this.charHeight);

		return element;
	}

	/**
	 * Remove a Markdown block by ID.
	 *
	 * @param id - Block identifier
	 */
	removeMarkdownBlock(id: string): void {
		this.markdownRenderer.removeBlock(id);
	}

	/**
	 * Get a Markdown block element by ID.
	 *
	 * @param id - Block identifier
	 * @returns Block element or undefined
	 */
	getMarkdownBlock(id: string): HTMLElement | undefined {
		return this.markdownRenderer.getBlock(id);
	}

	/**
	 * Update Markdown block visibility based on scroll position.
	 * Implements virtual scrolling for performance.
	 *
	 * @param visibleRange - Currently visible row range
	 */
	updateMarkdownVisibility(visibleRange: { start: number; end: number }): void {
		this.markdownRenderer.updateVisibility(visibleRange);
	}

	/**
	 * Get the Markdown renderer instance.
	 *
	 * @returns The Markdown renderer
	 */
	getMarkdownRenderer(): MarkdownRenderer {
		return this.markdownRenderer;
	}

	/**
	 * Clear all Markdown blocks.
	 */
	clearMarkdownBlocks(): void {
		this.markdownRenderer.dispose();
		// Recreate the container
		if (this.markdownContainer) {
			this.markdownContainer.innerHTML = "";
		}
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
}
