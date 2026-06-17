/**
 * Type definitions for Markdown display feature.
 *
 * @module markdown/types
 */

/**
 * Supported Markdown format types.
 */
export type MarkdownFormat = "commonmark" | "gfm";

/**
 * Fullscreen view configuration.
 * Note: These settings are managed by the viewer (eMterm application),
 * not controlled via OSC protocol from the sender.
 * Future versions may expose these as application preferences.
 */
export interface FullscreenConfig {
	/** Whether to show close button (X) */
	showCloseButton: boolean;
	/** Whether to show scrollbar always */
	alwaysShowScrollbar: boolean;
	/** Whether to show copy buttons on code blocks */
	showCopyButtons: boolean;
	/**
	 * Link click behavior.
	 * This is a viewer-side setting, not controlled via OSC protocol.
	 * Default: "confirm" (show confirmation dialog before opening links)
	 * Future: May be configurable via application settings.
	 */
	linkBehavior: "confirm" | "direct" | "disabled";
}

/**
 * Fullscreen view state.
 */
export interface FullscreenState {
	/** Whether fullscreen is currently active */
	isActive: boolean;
}

/**
 * Markdown session state.
 *
 * Represents an active Markdown transfer session, accumulating chunks
 * until the `end` verb is received.
 */
export interface MarkdownSession {
	/** Unique session identifier (UUID v4) */
	id: string;
	/** Markdown format (commonmark, gfm) */
	format: MarkdownFormat;
	/** Protocol version */
	version: number;
	/** Accumulated chunks indexed by sequence number */
	chunks: Map<number, string>;
	/** Last chunk receipt timestamp (milliseconds) */
	lastChunkAt: number;
	/** Base directory for resolving relative paths (from CLI) */
	basedir?: string;
}

/**
 * Markdown command verb types.
 */
export type MarkdownVerb = "begin" | "chunk" | "end" | "image-response" | "image-error";

/**
 * Parsed OSC 777 markdown command.
 *
 * Represents a parsed command extracted from the EmtermExtension action.
 */
export interface MarkdownCommand {
	/** Command verb */
	verb: MarkdownVerb;
	/** Key-value parameters */
	params: Record<string, string>;
}

/**
 * Rendered Markdown block for display.
 *
 * Represents a completed, rendered Markdown block ready to be
 * inserted into the DOM.
 */
export interface MarkdownBlock {
	/** Block identifier (matches session id) */
	id: string;
	/** Sanitized HTML content */
	html: string;
	/** Terminal row where block starts */
	startRow: number;
	/** Number of rows occupied */
	rowCount: number;
	/** Whether block is currently visible */
	visible: boolean;
}

/**
 * Begin command parameters.
 */
export interface BeginParams {
	/** Session ID (required) */
	id: string;
	/** Markdown format */
	format?: MarkdownFormat;
	/** Protocol version */
	version?: number;
	/** Base directory for resolving relative paths */
	basedir?: string;
}

/**
 * Chunk command parameters.
 */
export interface ChunkParams {
	/** Session ID (required) */
	id: string;
	/** Sequence number (required) */
	seq: number;
	/** Base64-encoded data (required) */
	data: string;
}

/**
 * End command parameters.
 */
export interface EndParams {
	/** Session ID (required) */
	id: string;
}

/**
 * Result of processing a markdown command.
 */
export type MarkdownCommandResult =
	| { type: "pending" }
	| { type: "complete"; block: MarkdownBlock }
	| { type: "error"; message: string };
