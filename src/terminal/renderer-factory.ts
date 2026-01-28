/**
 * Renderer factory for creating terminal renderers.
 *
 * Creates CanvasRenderer instances for terminal rendering.
 */

import { CanvasRenderer } from "./canvas-renderer.ts";
import type { ITerminalRenderer } from "./renderer-interface.ts";

/**
 * Create a terminal renderer.
 *
 * @param container - Container element for the renderer
 * @param fontFamily - Font family for terminal text
 * @param fontSize - Font size in pixels
 * @returns A terminal renderer instance
 */
export function createRenderer(
	container: HTMLElement,
	fontFamily: string,
	fontSize: number,
): ITerminalRenderer {
	return new CanvasRenderer(container, fontFamily, fontSize);
}

/**
 * Create a terminal renderer asynchronously.
 *
 * @param container - Container element for the renderer
 * @param fontFamily - Font family for terminal text
 * @param fontSize - Font size in pixels
 * @returns Promise resolving to a terminal renderer instance
 */
export async function createRendererAsync(
	container: HTMLElement,
	fontFamily: string,
	fontSize: number,
): Promise<ITerminalRenderer> {
	return new CanvasRenderer(container, fontFamily, fontSize);
}
