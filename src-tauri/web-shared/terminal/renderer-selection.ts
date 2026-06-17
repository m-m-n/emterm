/**
 * Selection rendering functions for the terminal canvas renderer.
 *
 * Extracted from CanvasRenderer: functions for rendering selection
 * overlays using DOM elements.
 */

/**
 * Context needed by selection rendering functions.
 */
export interface SelectionRenderContext {
	container: HTMLElement;
	charWidth: number;
	charHeight: number;
	cols: number;
}

/**
 * Selection overlay state.
 */
export interface SelectionOverlayState {
	selectionContainer: HTMLDivElement | null;
	selectionOverlays: HTMLDivElement[];
}

/**
 * Render visual selection highlight.
 */
export function renderSelection(
	sctx: SelectionRenderContext,
	state: SelectionOverlayState,
	selection: {
		start: { col: number; row: number };
		end: { col: number; row: number };
	},
): void {
	const { container, charWidth, charHeight, cols } = sctx;

	// Ensure selection container exists
	if (!state.selectionContainer) {
		state.selectionContainer = document.createElement("div");
		state.selectionContainer.className = "terminal-selection-container";
		state.selectionContainer.style.cssText = `
			position: absolute;
			top: 0;
			left: 0;
			right: 0;
			bottom: 0;
			pointer-events: none;
			z-index: 1;
		`;
		const computedPosition = window.getComputedStyle(container).position;
		if (computedPosition === "static") {
			container.style.position = "relative";
		}
		container.appendChild(state.selectionContainer);
	}

	// Clear existing overlays
	clearSelectionOverlays(state);

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
			colStart = start.col;
			colEnd = end.col;
		} else if (row === start.row) {
			colStart = start.col;
			colEnd = cols - 1;
		} else if (row === end.row) {
			colStart = 0;
			colEnd = end.col;
		} else {
			colStart = 0;
			colEnd = cols - 1;
		}

		const overlay = document.createElement("div");
		overlay.className = "terminal-selection-overlay";
		overlay.style.cssText = `
			position: absolute;
			left: ${colStart * charWidth}px;
			top: ${row * charHeight}px;
			width: ${(colEnd - colStart + 1) * charWidth}px;
			height: ${charHeight}px;
			background-color: rgba(50, 150, 250, 0.3);
			pointer-events: none;
		`;

		state.selectionContainer.appendChild(overlay);
		state.selectionOverlays.push(overlay);
	}
}

/**
 * Clear selection overlay elements.
 */
export function clearSelectionOverlays(state: SelectionOverlayState): void {
	for (const overlay of state.selectionOverlays) {
		overlay.remove();
	}
	state.selectionOverlays = [];
}

/**
 * Clear all selection highlights.
 */
export function clearSelectionHighlight(state: SelectionOverlayState): void {
	clearSelectionOverlays(state);
}
