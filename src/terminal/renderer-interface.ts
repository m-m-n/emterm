/**
 * Common interface for terminal renderers.
 *
 * Both DOM-based and Canvas-based renderers implement this interface,
 * allowing them to be used interchangeably.
 */

import type { TerminalState } from "./state.ts";

/**
 * Terminal renderer interface.
 *
 * Defines the public API that all terminal renderers must implement.
 */
export interface ITerminalRenderer {
	/**
	 * Schedule a render of the terminal state.
	 * Uses requestAnimationFrame for batching.
	 *
	 * @param state - Terminal state to render
	 */
	scheduleRender(state: TerminalState): void;

	/**
	 * Force a full re-render.
	 *
	 * @param state - Terminal state to render
	 */
	forceRender(state: TerminalState): void;

	/**
	 * Resize the renderer.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resize(cols: number, rows: number): void;

	/**
	 * Render visual selection highlight.
	 *
	 * @param selection - Selection range to highlight
	 */
	renderSelection(selection: {
		start: { col: number; row: number };
		end: { col: number; row: number };
	}): void;

	/**
	 * Clear all selection highlights.
	 */
	clearSelectionHighlight(): void;

	/**
	 * Get character width in pixels.
	 */
	getCharWidth(): number;

	/**
	 * Get character height in pixels.
	 */
	getCharHeight(): number;

	/**
	 * Get the font family.
	 */
	getFontFamily(): string;

	/**
	 * Get the font size.
	 */
	getFontSize(): number;

	/**
	 * Dispose of the renderer and clean up resources.
	 */
	dispose(): void;
}

/**
 * Renderer type enumeration.
 */
export type RendererType = "dom" | "canvas";
