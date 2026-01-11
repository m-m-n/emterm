/**
 * Markdown display module.
 *
 * Provides functionality for rendering Markdown content in the terminal
 * via OSC 777 extension sequences.
 *
 * @module markdown
 */

// Renderer
export { MarkdownRenderer } from "./renderer.ts";

// Session Manager
export { MarkdownSessionManager } from "./session.ts";
// Theme
export type { MarkdownTheme } from "./theme.ts";
export {
	applyMarkdownTheme,
	generateMarkdownTheme,
	getDarkTheme,
	getLightTheme,
} from "./theme.ts";
// Types
export type {
	BeginParams,
	ChunkParams,
	EndParams,
	MarkdownBlock,
	MarkdownCommand,
	MarkdownCommandResult,
	MarkdownFormat,
	MarkdownSession,
	MarkdownVerb,
	RenderMode,
} from "./types.ts";
