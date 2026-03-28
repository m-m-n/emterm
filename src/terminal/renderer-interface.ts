/**
 * Common interface for terminal renderers.
 *
 * Both DOM-based and Canvas-based renderers implement this interface,
 * allowing them to be used interchangeably.
 */

import type { RendererSettings } from "../settings/settings-applier";
import type { UserColorScheme } from "../settings/types";
import type { TerminalState } from "./state.ts";
import type { SearchMatch } from "./search/search-state.ts";

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
	 * Render immediately (synchronously) using dirty-row differential path.
	 * Used by frame-budgeted processing to render within the same rAF frame.
	 *
	 * @param state - Terminal state to render
	 */
	renderImmediate(state: TerminalState): void;

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
	 * Set the font size dynamically.
	 * @param fontSize - New font size in points
	 */
	setFontSize(fontSize: number): void;

	/**
	 * Apply a setting change to the renderer.
	 * Generic method for handling various settings.
	 * @param setting - The setting key
	 * @param value - The new value
	 */
	applySetting<K extends keyof RendererSettings>(
		setting: K,
		value: RendererSettings[K],
	): void;

	/**
	 * Set a user-defined color scheme.
	 * Used for custom color schemes stored in settings.
	 * @param scheme - User color scheme with hex color values
	 */
	setUserColorScheme(scheme: UserColorScheme): void;

	/**
	 * Dispose of the renderer and clean up resources.
	 */
	dispose(): void;

	/**
	 * Scroll up in the scrollback buffer (toward past).
	 * @param lines - Number of lines to scroll up
	 */
	scrollUp(lines: number): void;

	/**
	 * Scroll down in the scrollback buffer (toward present).
	 * @param lines - Number of lines to scroll down
	 */
	scrollDown(lines: number): void;

	/**
	 * Get current scroll offset.
	 * @returns Number of lines scrolled back (0 = at bottom/present)
	 */
	getScrollOffset(): number;

	/**
	 * Set scroll offset directly for programmatic scroll positioning.
	 * @param offset - Number of lines to scroll back (0 = at bottom)
	 */
	setScrollOffset(offset: number): void;

	/**
	 * Set search matches for highlight rendering.
	 * @param matches - Array of search matches
	 * @param currentIndex - Index of the current/active match (-1 for none)
	 */
	setSearchHighlights(matches: SearchMatch[], currentIndex: number): void;

	/**
	 * Clear all search highlights.
	 */
	clearSearchHighlights(): void;

	/**
	 * Set the hover position for link underline rendering.
	 * Pass (-1, -1) to clear hover state.
	 * @param row - Display row
	 * @param col - Display column
	 */
	setHoverPosition(row: number, col: number): void;

	/**
	 * Set diagnostic flags for debugging rendering issues.
	 */
	setDiagnosticFlags(flags: { forceFullRender?: boolean }): void;

	/**
	 * Start a trivial CSS animation to keep the compositor active.
	 * Used when rAF stops being delivered (degraded mode) to ensure
	 * canvas paints are composited to the screen.
	 */
	startCompositorKeepAlive(): void;

	/**
	 * Stop the compositor keep-alive animation.
	 */
	stopCompositorKeepAlive(): void;
}

/**
 * Renderer type enumeration.
 */
export type RendererType = "canvas";
