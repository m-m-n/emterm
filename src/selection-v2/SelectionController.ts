/**
 * Selection controller.
 *
 * Orchestrates mouse events, selection model, rendering, and clipboard operations.
 */

import type { TerminalState } from "../terminal/state";
import { isMouseTrackingEnabled } from "../terminal/mouse";
import { ClipboardBridge } from "./ClipboardBridge";
import { SelectionModel } from "./SelectionModel";
import { SelectionRenderer } from "./SelectionRenderer";
import type { GridPosition, SelectionMode, SelectionRange } from "./types";
import { WordBoundary } from "./WordBoundary";

/**
 * Options for the selection controller.
 */
export interface SelectionControllerOptions {
	/** Container element for the terminal */
	container: HTMLElement;
	/** Width of a character cell in pixels */
	charWidth: number;
	/** Height of a character cell in pixels */
	charHeight: number;
	/** Number of columns in the terminal */
	cols: number;
	/** Number of rows in the terminal */
	rows: number;
	/** Function to get current terminal state */
	getTerminalState: () => TerminalState;
}

/**
 * Double/triple click detection threshold in milliseconds.
 */
const MULTI_CLICK_THRESHOLD = 300;

/**
 * Selection controller.
 *
 * Handles mouse events for text selection, integrates with clipboard,
 * and manages selection rendering.
 *
 * @example
 * ```ts
 * const controller = new SelectionController({
 *   container: terminalElement,
 *   charWidth: 10,
 *   charHeight: 20,
 *   cols: 80,
 *   rows: 24,
 *   getTerminalState: () => terminalState,
 * });
 *
 * controller.attach();
 *
 * // Later...
 * const text = controller.getSelectedText();
 * await controller.copy();
 *
 * // Cleanup
 * controller.detach();
 * controller.dispose();
 * ```
 */
export class SelectionController {
	private container: HTMLElement;
	private charWidth: number;
	private charHeight: number;
	private cols: number;
	private rows: number;
	private getTerminalState: () => TerminalState;

	private model: SelectionModel;
	private renderer: SelectionRenderer;
	private clipboard: ClipboardBridge;
	private wordBoundary: WordBoundary;

	// Click tracking for double/triple click
	private clickCount: number = 0;
	private lastClickTime: number = 0;
	private lastClickPos: GridPosition | null = null;

	// For word/line selection expansion
	private anchorWord: SelectionRange | null = null;
	private anchorRow: number | null = null;

	// Pending selection start for drag detection
	private pendingSelectionStart: GridPosition | null = null;

	// Event listener cleanup
	private cleanupFunctions: (() => void)[] = [];

	/**
	 * Create a new SelectionController.
	 */
	constructor(options: SelectionControllerOptions) {
		this.container = options.container;
		this.charWidth = options.charWidth;
		this.charHeight = options.charHeight;
		this.cols = options.cols;
		this.rows = options.rows;
		this.getTerminalState = options.getTerminalState;

		this.model = new SelectionModel();
		this.renderer = new SelectionRenderer(this.container);
		this.clipboard = new ClipboardBridge();
		this.wordBoundary = new WordBoundary(
			(row) => this.getLineText(row),
			this.cols,
		);

		// Subscribe to model changes for rendering
		this.model.subscribe((event) => {
			this.renderer.render(
				event.range,
				this.charWidth,
				this.charHeight,
				this.cols,
			);
		});
	}

	/**
	 * Get text content of a line.
	 */
	private getLineText(row: number): string {
		const state = this.getTerminalState();
		if (!state) return "";

		const buffer = state.getActiveBuffer();
		const line = buffer.getLine(row);
		if (!line) return "";

		let text = "";
		for (let col = 0; col < line.length; col++) {
			const cell = line.getCell(col);
			text += cell.char;
		}
		return text;
	}

	/**
	 * Convert pixel coordinates to grid position.
	 */
	private pixelToGrid(clientX: number, clientY: number): GridPosition {
		const rect = this.container.getBoundingClientRect();
		const x = clientX - rect.left;
		const y = clientY - rect.top;

		const col = Math.max(0, Math.min(Math.floor(x / this.charWidth), this.cols - 1));
		const row = Math.max(0, Math.min(Math.floor(y / this.charHeight), this.rows - 1));

		return { col, row };
	}

	/**
	 * Check if selection should be handled (not PTY mouse tracking).
	 */
	private shouldHandleSelection(event: MouseEvent): boolean {
		const state = this.getTerminalState();
		if (!state) return true;

		// Shift key always enables selection mode
		if (event.shiftKey) {
			return true;
		}

		// If mouse tracking is enabled, don't handle selection
		const modes = state.getModes();
		if (isMouseTrackingEnabled(modes.mouseTracking)) {
			return false;
		}

		return true;
	}

	/**
	 * Attach event listeners.
	 */
	attach(): void {
		const onMouseDown = this.onMouseDown.bind(this);
		const onMouseMove = this.onMouseMove.bind(this);
		const onMouseUp = this.onMouseUp.bind(this);

		this.container.addEventListener("mousedown", onMouseDown);
		document.addEventListener("mousemove", onMouseMove);
		document.addEventListener("mouseup", onMouseUp);

		this.cleanupFunctions = [
			() => this.container.removeEventListener("mousedown", onMouseDown),
			() => document.removeEventListener("mousemove", onMouseMove),
			() => document.removeEventListener("mouseup", onMouseUp),
		];
	}

	/**
	 * Detach event listeners.
	 */
	detach(): void {
		for (const cleanup of this.cleanupFunctions) {
			cleanup();
		}
		this.cleanupFunctions = [];
	}

	/**
	 * Update terminal dimensions.
	 */
	resize(
		cols: number,
		rows: number,
		charWidth: number,
		charHeight: number,
	): void {
		this.cols = cols;
		this.rows = rows;
		this.charWidth = charWidth;
		this.charHeight = charHeight;
		this.wordBoundary.updateCols(cols);

		// Clear selection on resize
		this.clearSelection();
	}

	/**
	 * Handle mouse down events.
	 */
	private onMouseDown(event: MouseEvent): void {
		// Only handle left button
		if (event.button !== 0) return;

		// Check if we should handle selection
		if (!this.shouldHandleSelection(event)) {
			return;
		}

		const pos = this.pixelToGrid(event.clientX, event.clientY);
		const now = Date.now();

		// Detect double/triple clicks
		if (
			this.lastClickPos &&
			now - this.lastClickTime < MULTI_CLICK_THRESHOLD &&
			this.lastClickPos.col === pos.col &&
			this.lastClickPos.row === pos.row
		) {
			this.clickCount++;
		} else {
			this.clickCount = 1;
		}

		this.lastClickTime = now;
		this.lastClickPos = pos;

		// Handle based on click count
		let mode: SelectionMode = "char";

		if (this.clickCount === 2) {
			// Double click - word selection with drag enabled
			mode = "word";
			const wordRange = this.wordBoundary.getWordAt(pos.col, pos.row);
			this.anchorWord = wordRange;
			this.model.setSelection(wordRange, mode, true);
		} else if (this.clickCount >= 3) {
			// Triple click - line selection with drag enabled
			mode = "line";
			this.anchorRow = pos.row;
			const lineRange = this.wordBoundary.getLineAt(pos.row);
			this.model.setSelection(lineRange, mode, true);
			this.clickCount = 3; // Cap at 3
		} else {
			// Single click - clear existing selection and prepare for potential drag
			// Don't start selection immediately; wait for drag (mousemove)
			mode = "char";
			this.anchorWord = null;
			this.anchorRow = null;
			this.clearSelection();
			this.pendingSelectionStart = pos;
		}

		event.preventDefault();
	}

	/**
	 * Handle mouse move events.
	 */
	private onMouseMove(event: MouseEvent): void {
		const pos = this.pixelToGrid(event.clientX, event.clientY);

		// Handle pending selection start (drag detection for single click)
		if (this.pendingSelectionStart && !this.model.isActivelySelecting()) {
			// Only start selection if mouse has actually moved to a different cell
			if (
				pos.col !== this.pendingSelectionStart.col ||
				pos.row !== this.pendingSelectionStart.row
			) {
				this.model.startSelection(this.pendingSelectionStart, "char");
				this.model.updateSelection(pos);
				event.preventDefault();
			}
			return;
		}

		if (!this.model.isActivelySelecting()) {
			return;
		}
		const state = this.model.getState();

		if (state.mode === "word" && this.anchorWord) {
			// Expand word selection
			const expanded = this.wordBoundary.expandWordSelection(this.anchorWord, pos);
			this.model.updateSelectionRange(expanded);
		} else if (state.mode === "line" && this.anchorRow !== null) {
			// Expand line selection
			const expanded = this.wordBoundary.expandLineSelection(
				this.anchorRow,
				pos.row,
			);
			this.model.updateSelectionRange(expanded);
		} else {
			// Character selection
			this.model.updateSelection(pos);
		}

		event.preventDefault();
	}

	/**
	 * Handle mouse up events.
	 */
	private onMouseUp(event: MouseEvent): void {
		if (event.button !== 0) return;

		// Clear pending selection (click without drag)
		this.pendingSelectionStart = null;

		if (this.model.isActivelySelecting()) {
			this.model.endSelection();
		}
	}

	/**
	 * Get the currently selected text.
	 *
	 * @returns Selected text or empty string if no selection
	 */
	getSelectedText(): string {
		const range = this.model.getNormalizedRange();
		if (!range) {
			return "";
		}

		const state = this.getTerminalState();
		if (!state) {
			return "";
		}

		return state.extractText(
			range.start.col,
			range.start.row,
			range.end.col,
			range.end.row,
		);
	}

	/**
	 * Copy the current selection to clipboard.
	 *
	 * @returns True if copy succeeded
	 */
	async copy(): Promise<boolean> {
		const text = this.getSelectedText();
		if (!text) {
			return false;
		}
		return this.clipboard.write(text);
	}

	/**
	 * Read text from clipboard.
	 *
	 * @returns Clipboard text
	 */
	async paste(): Promise<string> {
		return this.clipboard.read();
	}

	/**
	 * Check if clipboard contains multi-line text.
	 *
	 * @param text - Text to check
	 * @returns True if multi-line
	 */
	isMultiLinePaste(text: string): boolean {
		return this.clipboard.isMultiLine(text);
	}

	/**
	 * Count lines in text.
	 *
	 * @param text - Text to count
	 * @returns Line count
	 */
	countPasteLines(text: string): number {
		return this.clipboard.countLines(text);
	}

	/**
	 * Clear the current selection.
	 */
	clearSelection(): void {
		this.model.clearSelection();
		this.anchorWord = null;
		this.anchorRow = null;
	}

	/**
	 * Check if there is an active selection.
	 *
	 * @returns True if there is a selection
	 */
	hasSelection(): boolean {
		return this.model.hasSelection();
	}

	/**
	 * Get the selection model.
	 *
	 * @returns Selection model instance
	 */
	getModel(): SelectionModel {
		return this.model;
	}

	/**
	 * Dispose the controller and clean up resources.
	 */
	dispose(): void {
		this.detach();
		this.renderer.dispose();
	}
}
