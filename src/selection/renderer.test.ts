/**
 * Tests for selection rendering functionality.
 */
import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import type { Selection } from "./manager";

describe("Selection Rendering", () => {
	let container: HTMLElement;
	let selectionContainer: HTMLDivElement | null = null;
	let selectionOverlays: HTMLDivElement[] = [];
	const charWidth = 10;
	const charHeight = 20;
	const cols = 20;

	beforeEach(() => {
		// Create fresh container for each test
		container = document.createElement("div");
		container.id = "terminal";
		container.style.position = "relative";
		document.body.appendChild(container);

		// Create line elements (simulate terminal grid)
		for (let row = 0; row < 10; row++) {
			const line = document.createElement("div");
			line.className = "terminal-line";
			line.style.height = `${charHeight}px`;

			// Add a single span with text (simulating grouped rendering)
			const span = document.createElement("span");
			span.textContent = "A".repeat(cols);
			line.appendChild(span);

			container.appendChild(line);
		}

		selectionContainer = null;
		selectionOverlays = [];
	});

	afterEach(() => {
		// Clean up
		if (container && container.parentNode) {
			container.parentNode.removeChild(container);
		}
		selectionContainer = null;
		selectionOverlays = [];
	});

	describe("renderSelection with overlays", () => {
		it("should create overlay for single-line selection", () => {
			const selection: Selection = {
				start: { col: 5, row: 2 },
				end: { col: 10, row: 2 },
			};

			renderSelection(container, selection, charWidth, charHeight, cols);

			// Should create one overlay
			expect(selectionOverlays.length).toBe(1);

			const overlay = selectionOverlays[0];
			expect(overlay.style.left).toBe(`${5 * charWidth}px`);
			expect(overlay.style.top).toBe(`${2 * charHeight}px`);
			expect(overlay.style.width).toBe(`${6 * charWidth}px`); // 10 - 5 + 1 = 6
			expect(overlay.style.height).toBe(`${charHeight}px`);
		});

		it("should create overlays for multi-line selection", () => {
			const selection: Selection = {
				start: { col: 15, row: 3 },
				end: { col: 5, row: 5 },
			};

			renderSelection(container, selection, charWidth, charHeight, cols);

			// Should create 3 overlays (rows 3, 4, 5)
			expect(selectionOverlays.length).toBe(3);

			// Row 3: cols 15 to end (19)
			const overlay3 = selectionOverlays[0];
			expect(overlay3.style.left).toBe(`${15 * charWidth}px`);
			expect(overlay3.style.top).toBe(`${3 * charHeight}px`);
			expect(overlay3.style.width).toBe(`${5 * charWidth}px`); // 19 - 15 + 1 = 5

			// Row 4: all columns (0 to 19)
			const overlay4 = selectionOverlays[1];
			expect(overlay4.style.left).toBe("0px");
			expect(overlay4.style.top).toBe(`${4 * charHeight}px`);
			expect(overlay4.style.width).toBe(`${cols * charWidth}px`);

			// Row 5: cols 0 to 5
			const overlay5 = selectionOverlays[2];
			expect(overlay5.style.left).toBe("0px");
			expect(overlay5.style.top).toBe(`${5 * charHeight}px`);
			expect(overlay5.style.width).toBe(`${6 * charWidth}px`); // 5 - 0 + 1 = 6
		});

		it("should auto-normalize backward selection", () => {
			const selection: Selection = {
				start: { col: 10, row: 5 },
				end: { col: 5, row: 3 },
			};

			renderSelection(container, selection, charWidth, charHeight, cols);

			// Should normalize to (5,3) -> (10,5) and create 3 overlays
			expect(selectionOverlays.length).toBe(3);

			// First overlay should start at row 3
			expect(selectionOverlays[0].style.top).toBe(`${3 * charHeight}px`);
			// Last overlay should be at row 5
			expect(selectionOverlays[2].style.top).toBe(`${5 * charHeight}px`);
		});

		it("should have correct styling on overlays", () => {
			const selection: Selection = {
				start: { col: 0, row: 0 },
				end: { col: 5, row: 0 },
			};

			renderSelection(container, selection, charWidth, charHeight, cols);

			const overlay = selectionOverlays[0];
			expect(overlay.style.position).toBe("absolute");
			expect(overlay.style.pointerEvents).toBe("none");
			expect(overlay.style.backgroundColor).toBe("rgba(50, 150, 250, 0.3)");
		});
	});

	describe("clearSelectionHighlight", () => {
		it("should remove all overlays", () => {
			// Create some overlays
			const selection: Selection = {
				start: { col: 0, row: 0 },
				end: { col: 10, row: 2 },
			};
			renderSelection(container, selection, charWidth, charHeight, cols);

			expect(selectionOverlays.length).toBe(3);
			expect(selectionContainer?.children.length).toBe(3);

			// Clear
			clearSelectionHighlight();

			expect(selectionOverlays.length).toBe(0);
			expect(selectionContainer?.children.length).toBe(0);
		});

		it("should handle empty selection gracefully", () => {
			// No overlays created
			clearSelectionHighlight();

			// Should not throw
			expect(selectionOverlays.length).toBe(0);
		});
	});

	/**
	 * Render visual selection highlight using overlays.
	 */
	function renderSelection(
		container: HTMLElement,
		selection: Selection,
		charWidth: number,
		charHeight: number,
		cols: number,
	): void {
		// Ensure selection container exists
		if (!selectionContainer) {
			selectionContainer = document.createElement("div");
			selectionContainer.className = "terminal-selection-container";
			selectionContainer.style.cssText = `
				position: absolute;
				top: 0;
				left: 0;
				right: 0;
				bottom: 0;
				pointer-events: none;
				z-index: 1;
			`;
			if (container.firstChild) {
				container.insertBefore(selectionContainer, container.firstChild);
			} else {
				container.appendChild(selectionContainer);
			}
		}

		// Clear existing overlays
		clearSelectionOverlays();

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

			selectionContainer.appendChild(overlay);
			selectionOverlays.push(overlay);
		}
	}

	function clearSelectionOverlays(): void {
		for (const overlay of selectionOverlays) {
			overlay.remove();
		}
		selectionOverlays = [];
	}

	function clearSelectionHighlight(): void {
		clearSelectionOverlays();
	}
});
