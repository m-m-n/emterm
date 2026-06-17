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

// Fullscreen View
export { FullscreenMarkdownView } from "./fullscreen.ts";

// Outline Panel
export { OutlinePanel } from "./outline.ts";

// Mermaid Renderer
export { MermaidRenderer } from "./mermaid-renderer.ts";

// Link Dialog
export { LinkConfirmDialog } from "./link-dialog.ts";

// Session Manager
export { MarkdownSessionManager } from "./session.ts";

// Types
export type {
	BeginParams,
	ChunkParams,
	EndParams,
	FullscreenConfig,
	FullscreenState,
	MarkdownBlock,
	MarkdownCommand,
	MarkdownCommandResult,
	MarkdownFormat,
	MarkdownSession,
	MarkdownVerb,
} from "./types.ts";
