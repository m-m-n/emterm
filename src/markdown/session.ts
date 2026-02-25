/**
 * Markdown session manager.
 *
 * Manages Markdown rendering sessions for OSC 777 extension.
 * Handles begin/chunk/end lifecycle, timeout cleanup, and size limits.
 *
 * @module markdown/session
 */

import { FullscreenMarkdownView } from "./fullscreen.ts";
import { MarkdownRenderer } from "./renderer.ts";

import type {
	MarkdownBlock,
	MarkdownFormat,
	MarkdownSession,
} from "./types.ts";

/**
 * Manages Markdown rendering sessions.
 *
 * Always displays Markdown in fullscreen mode (like `less` command).
 * Use Escape or 'q' to close the fullscreen view.
 *
 * @example
 * ```typescript
 * const manager = new MarkdownSessionManager();
 *
 * // Handle OSC 777 emterm;markdown commands
 * manager.handleCommand("emterm", ["markdown", "begin", "id=xxx", "format=gfm"]);
 * manager.handleCommand("emterm", ["markdown", "chunk", "id=xxx", "seq=0", "data=..."]);
 * manager.handleCommand("emterm", ["markdown", "end", "id=xxx"]);
 * // Fullscreen view is automatically shown on "end"
 *
 * // Clean up
 * manager.dispose();
 * ```
 */
export class MarkdownSessionManager {
	/** Session timeout in milliseconds (30 seconds) */
	static readonly SESSION_TIMEOUT = 30 * 1000;

	/** Maximum concurrent sessions */
	static readonly MAX_SESSIONS = 10;

	/** Cleanup interval in milliseconds */
	private static readonly CLEANUP_INTERVAL = 5000;

	/** Active sessions indexed by ID */
	private sessions = new Map<string, MarkdownSession>();

	/** Markdown renderer instance */
	private renderer: MarkdownRenderer;

	/** Fullscreen view instance */
	private fullscreenView: FullscreenMarkdownView;

	/** Container element (overlay-root) for rendering */
	private container: HTMLElement | null = null;

	/** Cleanup timer handle */
	private cleanupTimer: ReturnType<typeof setInterval> | null = null;

	/**
	 * Create a new session manager.
	 */
	constructor() {
		this.renderer = new MarkdownRenderer();
		this.fullscreenView = new FullscreenMarkdownView();
		this.startCleanupTimer();
	}

	/**
	 * Set container for fullscreen view rendering.
	 * @param container - Container element (overlay-root) to append fullscreen view to
	 */
	setContainer(container: HTMLElement): void {
		this.container = container;
	}

	/**
	 * Handle an EmtermExtension OSC action for markdown.
	 *
	 * Markdown is always displayed in fullscreen mode (like `less` command).
	 * The fullscreen view is shown automatically when the end command is received.
	 *
	 * @param verb - The command verb from OSC 777 (should be "emterm")
	 * @param params - Command parameters as strings
	 *   - params[0]: command type (should be "markdown")
	 *   - params[1]: markdown verb (begin, chunk, end)
	 *   - params[2...]: key=value parameters
	 */
	handleCommand(verb: string, params: string[]): void {
		// Validate emterm namespace
		if (verb !== "emterm") {
			return;
		}

		// Validate markdown command
		if (params.length < 2 || params[0] !== "markdown") {
			return;
		}

		const markdownVerb = params[1];
		const keyValueParams = params.slice(2);
		const parsed = this.parseParams(keyValueParams);

		switch (markdownVerb) {
			case "begin":
				this.handleBegin(parsed);
				break;
			case "chunk":
				this.handleChunk(parsed);
				break;
			case "end":
				this.handleEnd(parsed);
				break;
			default:
				console.warn(`Unknown markdown verb: ${markdownVerb}`);
		}
	}

	/**
	 * Handle begin command - create new session.
	 */
	private handleBegin(params: Record<string, string>): void {
		const id = params.id;
		if (!id) {
			console.warn("Markdown begin: missing id");
			return;
		}

		if (this.sessions.size >= MarkdownSessionManager.MAX_SESSIONS) {
			console.warn("Markdown begin: max sessions reached");
			return;
		}

		// Validate format
		let format: MarkdownFormat = "commonmark";
		if (params.format === "gfm" || params.format === "commonmark") {
			format = params.format;
		}

		const session: MarkdownSession = {
			id,
			format,
			version: parseInt(params.version || "1", 10) || 1,
			chunks: new Map(),
			lastChunkAt: Date.now(),
		};

		this.sessions.set(id, session);
	}

	/**
	 * Handle chunk command - append data to session.
	 */
	private handleChunk(params: Record<string, string>): void {
		const id = params.id;
		const seq = params.seq;
		const data = params.data;

		if (!id) {
			console.warn("Markdown chunk: missing id");
			return;
		}

		const session = this.sessions.get(id);
		if (!session) {
			console.warn(`Markdown chunk: unknown session ${id}`);
			return;
		}

		if (!seq) {
			console.warn("Markdown chunk: missing seq");
			return;
		}

		const seqNum = parseInt(seq, 10);
		if (isNaN(seqNum)) {
			console.warn("Markdown chunk: invalid seq");
			return;
		}

		if (!data) {
			console.warn("Markdown chunk: missing data");
			return;
		}

		// Decode Base64 with UTF-8 support
		let decoded: string;
		try {
			decoded = this.decodeBase64Utf8(data);
		} catch {
			console.warn("Markdown chunk: invalid base64 or UTF-8");
			return;
		}

		session.chunks.set(seqNum, decoded);
		session.lastChunkAt = Date.now();
	}

	/**
	 * Handle end command - assemble chunks and render in fullscreen.
	 */
	private handleEnd(params: Record<string, string>): void {
		const id = params.id;

		if (!id) {
			console.warn("Markdown end: missing id");
			return;
		}

		const session = this.sessions.get(id);
		if (!session) {
			console.warn(`Markdown end: unknown session ${id}`);
			return;
		}

		// Assemble chunks in order
		const markdown = this.assembleChunks(session);

		// Render markdown to HTML
		const html = this.renderer.render(markdown, session.format);

		// Cleanup session
		this.sessions.delete(id);

		// Always display in fullscreen mode
		const block: MarkdownBlock = {
			id,
			html,
			startRow: 0,
			rowCount: 0,
			visible: true,
		};

		// Require container to be set
		if (!this.container) {
			console.error(
				"[ERROR][FRONTEND] MarkdownSessionManager: container not set, cannot show fullscreen view",
			);
			return;
		}

		this.fullscreenView.show(block, this.container);
	}

	/**
	 * Assemble chunks in sequence order.
	 */
	private assembleChunks(session: MarkdownSession): string {
		const sortedSeqs = Array.from(session.chunks.keys()).sort((a, b) => a - b);
		return sortedSeqs.map((seq) => session.chunks.get(seq)!).join("");
	}

	/**
	 * Decode Base64 string with proper UTF-8 support.
	 *
	 * The standard atob() returns a binary string that doesn't handle
	 * multi-byte UTF-8 characters correctly. This method properly decodes
	 * UTF-8 encoded Base64 data.
	 *
	 * @param data - Base64 encoded string
	 * @returns Decoded UTF-8 string
	 * @throws Error if data is not valid Base64 or UTF-8
	 */
	private decodeBase64Utf8(data: string): string {
		// Validate Base64 format before decoding
		// Standard Base64 only allows A-Z, a-z, 0-9, +, /, and = for padding
		if (!/^[A-Za-z0-9+/]*={0,2}$/.test(data)) {
			throw new Error("Invalid Base64 format");
		}

		const binary = atob(data);
		const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
		return new TextDecoder().decode(bytes);
	}

	/**
	 * Parse key=value parameters into a Record.
	 */
	private parseParams(params: string[]): Record<string, string> {
		const result: Record<string, string> = {};
		for (const param of params) {
			const eqIndex = param.indexOf("=");
			if (eqIndex > 0) {
				const key = param.substring(0, eqIndex);
				const value = param.substring(eqIndex + 1);
				result[key] = value;
			}
		}
		return result;
	}

	/**
	 * Start the cleanup timer.
	 */
	private startCleanupTimer(): void {
		this.cleanupTimer = setInterval(() => {
			this.cleanupExpiredSessions();
		}, MarkdownSessionManager.CLEANUP_INTERVAL);
	}

	/**
	 * Clean up expired sessions.
	 */
	cleanupExpiredSessions(): void {
		const now = Date.now();
		for (const [id, session] of this.sessions) {
			if (now - session.lastChunkAt > MarkdownSessionManager.SESSION_TIMEOUT) {
				console.warn(`Markdown session ${id} timed out`);
				this.sessions.delete(id);
			}
		}
	}

	/**
	 * Get active session by ID.
	 */
	getSession(id: string): MarkdownSession | undefined {
		return this.sessions.get(id);
	}

	/**
	 * Get count of active sessions.
	 */
	get sessionCount(): number {
		return this.sessions.size;
	}

	/**
	 * Get the renderer instance.
	 */
	getRenderer(): MarkdownRenderer {
		return this.renderer;
	}

	/**
	 * Get the fullscreen view instance.
	 */
	getFullscreenView(): FullscreenMarkdownView {
		return this.fullscreenView;
	}

	/**
	 * Dispose the session manager and clean up resources.
	 */
	dispose(): void {
		if (this.cleanupTimer !== null) {
			clearInterval(this.cleanupTimer);
			this.cleanupTimer = null;
		}
		this.sessions.clear();
		this.renderer.dispose();
		this.fullscreenView.dispose();
	}
}
