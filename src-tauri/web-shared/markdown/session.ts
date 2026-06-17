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

/** Allowlisted MIME types for image data URIs. SVG is excluded to prevent XSS. */
const ALLOWED_IMAGE_MIME_TYPES = new Set([
	"image/png",
	"image/jpeg",
	"image/gif",
	"image/webp",
	"image/bmp",
	"image/x-icon",
]);

/** Maximum number of concurrent pending chunked image transfers */
const MAX_PENDING_CHUNKS = 50;

/** Maximum total data size (bytes of base64 text) per chunked image transfer */
const MAX_CHUNK_DATA_SIZE = 100 * 1024 * 1024;

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

	/** PTY write callback for sending requests to CLI */
	private ptyWriteCallback: ((data: string) => void) | null = null;

	/** Basedir from the most recent completed session (for fullscreen view) */
	private activeBasedir: string | undefined = undefined;

	/** Image container element for finding img placeholders */
	private imageContainer: HTMLElement | null = null;

	/** Pending chunked image transfers: request_id -> { mime_type, chunks: Map<seq, data> } */
	private pendingImageChunks = new Map<
		string,
		{ mimeType: string; chunks: Map<number, string>; totalChunks: number }
	>();

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
			case "image-response":
				this.handleImageResponse(parsed);
				break;
			case "image-error":
				this.handleImageError(parsed);
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

		// Clear pending image chunks from previous session (navigation invalidates them)
		this.pendingImageChunks.clear();

		const session: MarkdownSession = {
			id,
			format,
			version: parseInt(params.version || "1", 10) || 1,
			chunks: new Map(),
			lastChunkAt: Date.now(),
			basedir: params.basedir || undefined,
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

		// Store active basedir for fullscreen view
		this.activeBasedir = session.basedir;

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

		this.fullscreenView.show(
			block,
			this.container,
			undefined,
			this.ptyWriteCallback ?? undefined,
			this.activeBasedir,
		);

		// Update image container reference to the fullscreen content area
		const content = this.container.querySelector(".markdown-fullscreen-content");
		if (content instanceof HTMLElement) {
			this.imageContainer = content;
		}
	}

	/**
	 * Assemble chunks in sequence order.
	 */
	private assembleChunks(session: MarkdownSession): string {
		const sortedSeqs = Array.from(session.chunks.keys()).sort((a, b) => a - b);
		return sortedSeqs.map((seq) => session.chunks.get(seq)!).join("");
	}

	/**
	 * Handle image-response verb: find img placeholder by request_id,
	 * assemble chunks if needed, set src to data: URI.
	 */
	private handleImageResponse(params: Record<string, string>): void {
		const requestId = params.request_id;
		if (!requestId) {
			console.warn("Markdown image-response: missing request_id");
			return;
		}

		const data = params.data || "";
		const mimeType = params.mime_type;
		const chunkSeq = params.chunk_seq !== undefined ? parseInt(params.chunk_seq, 10) : undefined;
		const chunkTotal = params.chunk_total !== undefined ? parseInt(params.chunk_total, 10) : undefined;

		// Chunked transfer
		if (chunkSeq !== undefined && chunkTotal !== undefined && chunkTotal > 1) {
			let pending = this.pendingImageChunks.get(requestId);
			if (!pending) {
				// Check concurrent transfer limit
				if (this.pendingImageChunks.size >= MAX_PENDING_CHUNKS) {
					console.warn(`Markdown image-response: too many pending chunked transfers (${MAX_PENDING_CHUNKS}), discarding ${requestId}`);
					return;
				}
				if (!mimeType) {
					console.warn("Markdown image-response: missing mime_type for first chunk");
					return;
				}
				pending = {
					mimeType,
					chunks: new Map(),
					totalChunks: chunkTotal,
				};
				this.pendingImageChunks.set(requestId, pending);
			}

			// Check accumulated data size limit
			let currentSize = 0;
			for (const chunk of pending.chunks.values()) {
				currentSize += chunk.length;
			}
			if (currentSize + data.length > MAX_CHUNK_DATA_SIZE) {
				console.warn(`Markdown image-response: chunk data size limit exceeded for ${requestId}, discarding`);
				this.pendingImageChunks.delete(requestId);
				return;
			}

			pending.chunks.set(chunkSeq, data);

			// Check if all chunks received
			if (pending.chunks.size < pending.totalChunks) {
				return; // Wait for more chunks
			}

			// Assemble all chunks in order
			const sortedSeqs = Array.from(pending.chunks.keys()).sort((a, b) => a - b);
			const assembledData = sortedSeqs.map((seq) => pending!.chunks.get(seq)!).join("");
			this.pendingImageChunks.delete(requestId);

			this.setImageSrc(requestId, pending.mimeType, assembledData);
			return;
		}

		// Single-shot transfer
		if (!mimeType) {
			console.warn("Markdown image-response: missing mime_type");
			return;
		}
		this.setImageSrc(requestId, mimeType, data);
	}

	/**
	 * Set the src of an img element with matching request_id.
	 */
	private setImageSrc(requestId: string, mimeType: string, base64Data: string): void {
		// Validate requestId format to prevent CSS selector injection
		if (!/^img-\d+$/.test(requestId)) {
			console.warn(`Markdown image-response: invalid request_id format: ${requestId}`);
			return;
		}

		// Validate MIME type against allowlist (blocks SVG XSS)
		if (!ALLOWED_IMAGE_MIME_TYPES.has(mimeType)) {
			console.warn(`Markdown image-response: rejected MIME type: ${mimeType}`);
			const container = this.imageContainer;
			if (container) {
				const img = container.querySelector(`img[data-request-id="${requestId}"]`);
				if (img) {
					const errorEl = document.createElement("span");
					errorEl.setAttribute("data-request-id", requestId);
					errorEl.className = "markdown-image-error";
					errorEl.textContent = `[Image error: unsupported format ${mimeType}]`;
					img.replaceWith(errorEl);
				}
			}
			return;
		}

		const container = this.imageContainer;
		if (!container) {
			console.warn(`Markdown image-response: no image container for ${requestId}`);
			return;
		}

		const img = container.querySelector(`img[data-request-id="${requestId}"]`);
		if (!img) {
			console.warn(`Markdown image-response: no placeholder found for ${requestId}`);
			return;
		}

		(img as HTMLImageElement).src = `data:${mimeType};base64,${base64Data}`;
	}

	/**
	 * Handle image-error verb: find img placeholder by request_id,
	 * display error message.
	 */
	private handleImageError(params: Record<string, string>): void {
		const requestId = params.request_id;
		if (!requestId) {
			console.warn("Markdown image-error: missing request_id");
			return;
		}

		// Validate requestId format to prevent CSS selector injection
		if (!/^img-\d+$/.test(requestId)) {
			console.warn(`Markdown image-error: invalid request_id format: ${requestId}`);
			return;
		}

		const errorMsg = params.error || "Unknown error";

		const container = this.imageContainer;
		if (!container) {
			console.warn(`Markdown image-error: no image container for ${requestId}`);
			return;
		}

		const img = container.querySelector(`img[data-request-id="${requestId}"]`);
		if (!img) {
			console.warn(`Markdown image-error: no placeholder found for ${requestId}`);
			return;
		}

		// Replace img with error indicator
		const errorEl = document.createElement("span");
		errorEl.setAttribute("data-request-id", requestId);
		errorEl.className = "markdown-image-error";
		errorEl.textContent = `[Image error: ${errorMsg}]`;
		img.replaceWith(errorEl);
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
	 * Set the PTY write callback for sending requests to CLI.
	 * @param callback - Function to write data to PTY, or null to clear
	 */
	setPtyWriteCallback(callback: ((data: string) => void) | null): void {
		this.ptyWriteCallback = callback;
	}

	/**
	 * Get the current PTY write callback.
	 */
	getPtyWriteCallback(): ((data: string) => void) | null {
		return this.ptyWriteCallback;
	}

	/**
	 * Get the basedir from the most recently completed session.
	 */
	getActiveBasedir(): string | undefined {
		return this.activeBasedir;
	}

	/**
	 * Set the image container for finding img placeholders.
	 * Used by session manager to locate images for image-response/error handling.
	 */
	setImageContainer(container: HTMLElement | null): void {
		this.imageContainer = container;
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
		this.pendingImageChunks.clear();
		this.ptyWriteCallback = null;
		this.activeBasedir = undefined;
		this.imageContainer = null;
		this.renderer.dispose();
		this.fullscreenView.dispose();
	}
}
