/**
 * Renderer factory for creating terminal renderers.
 *
 * Provides a factory function that creates the appropriate renderer
 * based on configuration or environment variables.
 */

import { invoke } from "@tauri-apps/api/core";
import { CanvasRenderer } from "./canvas-renderer.ts";
import type { ITerminalRenderer, RendererType } from "./renderer-interface.ts";
import { TerminalRenderer } from "./renderer.ts";

/**
 * Get the renderer type from environment variable (build-time).
 *
 * Checks VITE_EMTERM_RENDERER environment variable at build time.
 * This is used for synchronous renderer creation when runtime check is not needed.
 *
 * @returns The renderer type to use
 */
export function getRendererType(): RendererType {
	// Check Vite environment variable (embedded at build time)
	const envRenderer = (import.meta as { env?: { VITE_EMTERM_RENDERER?: string } }).env
		?.VITE_EMTERM_RENDERER;

	if (envRenderer === "canvas") {
		return "canvas";
	}

	// Default to DOM renderer
	return "dom";
}

/**
 * Get the renderer type from runtime environment variable via Tauri command.
 *
 * Checks EMTERM_RENDERER environment variable at runtime.
 * This enables E2E tests to switch renderers without rebuilding.
 *
 * Falls back to build-time VITE_EMTERM_RENDERER if Tauri command fails.
 *
 * @returns Promise resolving to the renderer type to use
 */
export async function getRendererTypeAsync(): Promise<RendererType> {
	try {
		// Call Tauri command to get runtime environment variable
		const runtimeRenderer = await invoke<string>("get_renderer_type");
		if (runtimeRenderer === "canvas") {
			return "canvas";
		}
		if (runtimeRenderer === "dom") {
			return "dom";
		}
		// If runtime env var is not set (returns "dom" as default from Rust),
		// fall back to build-time env var
		return getRendererType();
	} catch (error) {
		// Tauri command failed (e.g., not running in Tauri context)
		// Fall back to build-time env var
		console.warn("Failed to get renderer type from runtime env:", error);
		return getRendererType();
	}
}

/**
 * Create a terminal renderer.
 *
 * @param container - Container element for the renderer
 * @param fontFamily - Font family for terminal text
 * @param fontSize - Font size in pixels
 * @param type - Renderer type override (optional, defaults to env-based selection)
 * @returns A terminal renderer instance
 */
export function createRenderer(
	container: HTMLElement,
	fontFamily: string,
	fontSize: number,
	type?: RendererType,
): ITerminalRenderer {
	const rendererType = type ?? getRendererType();

	if (rendererType === "canvas") {
		return new CanvasRenderer(container, fontFamily, fontSize);
	}

	return new TerminalRenderer(container, fontFamily, fontSize);
}

/**
 * Create a terminal renderer asynchronously with runtime env var support.
 *
 * This version checks the runtime EMTERM_RENDERER environment variable
 * via Tauri command, enabling E2E tests to switch renderers without rebuilding.
 *
 * @param container - Container element for the renderer
 * @param fontFamily - Font family for terminal text
 * @param fontSize - Font size in pixels
 * @param type - Renderer type override (optional, defaults to runtime env-based selection)
 * @returns Promise resolving to a terminal renderer instance
 */
export async function createRendererAsync(
	container: HTMLElement,
	fontFamily: string,
	fontSize: number,
	type?: RendererType,
): Promise<ITerminalRenderer> {
	const rendererType = type ?? (await getRendererTypeAsync());

	if (rendererType === "canvas") {
		return new CanvasRenderer(container, fontFamily, fontSize);
	}

	return new TerminalRenderer(container, fontFamily, fontSize);
}
