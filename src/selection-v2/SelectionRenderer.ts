/**
 * Selection rendering.
 *
 * Renders selection highlights as DOM overlays.
 */

import type { SelectionRange } from "./types";
import { normalizeRange } from "./types";

/**
 * Selection renderer.
 *
 * Creates and manages DOM elements to highlight the selected region.
 *
 * @example
 * ```ts
 * const renderer = new SelectionRenderer(container);
 *
 * // Render selection
 * renderer.render(range, 10, 20, 80);
 *
 * // Clear selection
 * renderer.clear();
 *
 * // Cleanup
 * renderer.dispose();
 * ```
 */
export class SelectionRenderer {
	private container: HTMLElement;
	private overlayContainer: HTMLDivElement | null = null;
	private overlays: HTMLDivElement[] = [];

	/**
	 * Create a new SelectionRenderer.
	 *
	 * @param container - Container element for the terminal
	 */
	constructor(container: HTMLElement) {
		this.container = container;
	}

	/**
	 * Initialize the overlay container.
	 * Creates the container lazily on first render.
	 */
	private ensureOverlayContainer(): HTMLDivElement {
		// Check if overlayContainer exists AND is still connected to DOM
		// (container.innerHTML = "" during resize removes overlayContainer from DOM)
		if (!this.overlayContainer || !this.overlayContainer.isConnected) {
			this.overlayContainer = document.createElement("div");
			this.overlayContainer.className = "selection-overlay-container";
			this.overlayContainer.style.cssText = `
				position: absolute;
				top: 0;
				left: 0;
				right: 0;
				bottom: 0;
				pointer-events: none;
				overflow: hidden;
			`;
			this.container.appendChild(this.overlayContainer);
		}
		return this.overlayContainer;
	}

	/**
	 * Create a single selection overlay element.
	 */
	private createOverlay(
		x: number,
		y: number,
		width: number,
		height: number,
	): HTMLDivElement {
		const overlay = document.createElement("div");
		overlay.className = "selection-overlay";
		overlay.style.cssText = `
			position: absolute;
			left: ${x}px;
			top: ${y}px;
			width: ${width}px;
			height: ${height}px;
			background-color: rgba(100, 150, 255, 0.3);
			pointer-events: none;
		`;
		return overlay;
	}

	/**
	 * Render the selection highlight.
	 *
	 * @param range - Selection range to render (or null to clear)
	 * @param charWidth - Width of a character cell in pixels
	 * @param charHeight - Height of a character cell in pixels
	 * @param cols - Number of columns in the terminal
	 */
	render(
		range: SelectionRange | null,
		charWidth: number,
		charHeight: number,
		cols: number,
	): void {
		// Clear existing overlays
		this.clear();

		if (!range) {
			return;
		}

		// Normalize range so start is before end
		const normalized = normalizeRange(range);
		const { start, end } = normalized;

		const overlayContainer = this.ensureOverlayContainer();

		// Single row selection
		if (start.row === end.row) {
			const x = start.col * charWidth;
			const y = start.row * charHeight;
			const width = (end.col - start.col + 1) * charWidth;
			const overlay = this.createOverlay(x, y, width, charHeight);
			overlayContainer.appendChild(overlay);
			this.overlays.push(overlay);
			return;
		}

		// Multi-row selection

		// First row: from start.col to end of line
		const firstRowX = start.col * charWidth;
		const firstRowY = start.row * charHeight;
		const firstRowWidth = (cols - start.col) * charWidth;
		const firstOverlay = this.createOverlay(
			firstRowX,
			firstRowY,
			firstRowWidth,
			charHeight,
		);
		overlayContainer.appendChild(firstOverlay);
		this.overlays.push(firstOverlay);

		// Middle rows: full width
		for (let row = start.row + 1; row < end.row; row++) {
			const y = row * charHeight;
			const overlay = this.createOverlay(0, y, cols * charWidth, charHeight);
			overlayContainer.appendChild(overlay);
			this.overlays.push(overlay);
		}

		// Last row: from start to end.col
		const lastRowY = end.row * charHeight;
		const lastRowWidth = (end.col + 1) * charWidth;
		const lastOverlay = this.createOverlay(0, lastRowY, lastRowWidth, charHeight);
		overlayContainer.appendChild(lastOverlay);
		this.overlays.push(lastOverlay);
	}

	/**
	 * Clear all selection overlays.
	 */
	clear(): void {
		for (const overlay of this.overlays) {
			overlay.remove();
		}
		this.overlays = [];
	}

	/**
	 * Dispose the renderer and remove all DOM elements.
	 */
	dispose(): void {
		this.clear();
		if (this.overlayContainer) {
			this.overlayContainer.remove();
			this.overlayContainer = null;
		}
	}
}
