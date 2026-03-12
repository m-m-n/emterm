/**
 * Download session manager.
 *
 * Manages file download sessions for OSC 777 download extension.
 * Handles begin/chunk/end lifecycle with streaming IPC to the backend.
 * No chunk data is accumulated in memory.
 *
 * @module download/session
 */

import { invoke } from "@tauri-apps/api/core";

import { DownloadProgressDisplay } from "./progress.ts";

interface DownloadSession {
	id: string;
	filename: string;
	expectedSize: number;
	handleId: string | null;
	nextSeq: number;
	receivedBytes: number;
	lastChunkAt: number;
	/** Chunks received before save dialog resolved */
	pendingChunks: string[];
}

interface StartDownloadResult {
	id: string;
	path: string;
}

export class DownloadSessionManager {
	static readonly SESSION_TIMEOUT = 60 * 1000;
	static readonly MAX_SESSIONS = 10;
	/** Max pending chunks buffered while save dialog is open */
	static readonly MAX_PENDING_CHUNKS = 5;
	private static readonly CLEANUP_INTERVAL = 10000;

	private sessions = new Map<string, DownloadSession>();
	private cleanupTimer: ReturnType<typeof setInterval> | null = null;
	private progressDisplay: DownloadProgressDisplay;

	constructor() {
		this.progressDisplay = new DownloadProgressDisplay();
		this.startCleanupTimer();
	}

	setContainer(container: HTMLElement): void {
		this.progressDisplay.setContainer(container);
	}

	handleCommand(verb: string, params: string[]): void {
		if (verb !== "emterm") return;
		if (params.length < 2 || params[0] !== "download") return;

		const downloadVerb = params[1];
		const keyValueParams = params.slice(2);
		const parsed = this.parseParams(keyValueParams);

		switch (downloadVerb) {
			case "begin":
				this.handleBegin(parsed);
				break;
			case "chunk":
				this.handleChunk(parsed);
				break;
			case "end":
				this.handleEnd(parsed).catch((err) => {
					console.error(
						"[ERROR][FRONTEND] Download: handleEnd failed",
						err,
					);
				});
				break;
		}
	}

	private sanitizeFilename(name: string): string {
		// Take basename: strip path separators
		const parts = name.split(/[/\\]/);
		const basename = parts[parts.length - 1] || "";
		// Reject pure traversal/dot names
		if (basename === "" || basename === "." || basename === "..") {
			return "download";
		}
		// Remove semicolons (OSC delimiter) and control characters
		const sanitized = basename.replace(/[;\x00-\x1f\x7f]/g, "");
		return sanitized || "download";
	}

	private handleBegin(params: Record<string, string>): void {
		const id = params.id;
		if (!id) return;

		if (this.sessions.has(id)) return;
		if (this.sessions.size >= DownloadSessionManager.MAX_SESSIONS) return;

		const expectedSize = parseInt(params.size || "0", 10) || 0;

		const session: DownloadSession = {
			id,
			filename: this.sanitizeFilename(params.name || "download"),
			expectedSize,
			handleId: null,
			nextSeq: 0,
			receivedBytes: 0,
			lastChunkAt: Date.now(),
			pendingChunks: [],
		};

		this.sessions.set(id, session);
		this.progressDisplay.show(session.filename, 0);

		// Start the download asynchronously (show save dialog)
		this.startDownload(session).catch((err) => {
			console.error(
				"[ERROR][FRONTEND] Download: startDownload failed",
				err,
			);
			this.discardSession(id);
		});
	}

	private async startDownload(session: DownloadSession): Promise<void> {
		const result = await invoke<StartDownloadResult | null>(
			"start_download_file",
			{
				filename: session.filename,
			},
		);

		// Session may have been discarded while dialog was open
		if (!this.sessions.has(session.id)) {
			// Cancel the just-created backend handle to avoid orphaned files
			if (result) {
				invoke("cancel_download_file", { id: result.id }).catch(
					() => {},
				);
			}
			return;
		}

		if (result) {
			session.handleId = result.id;
			// Flush any chunks that arrived while the dialog was open
			await this.flushPendingChunks(session);
		} else {
			// User cancelled save dialog
			this.sessions.delete(session.id);
			this.progressDisplay.showCancelled();
		}
	}

	private handleChunk(params: Record<string, string>): void {
		const id = params.id;
		if (!id) return;

		const session = this.sessions.get(id);
		if (!session) return;

		const seq = parseInt(params.seq || "", 10);
		if (isNaN(seq)) return;

		const data = params.data;
		if (!data) return;

		// Validate sequential ordering
		if (seq !== session.nextSeq) {
			// Out-of-order: discard session
			this.discardSession(id);
			return;
		}

		session.nextSeq++;
		session.receivedBytes += data.length;
		session.lastChunkAt = Date.now();

		if (session.handleId) {
			this.appendChunk(session, data).catch((err) => {
				console.error(
					"[ERROR][FRONTEND] Download: appendChunk failed",
					err,
				);
				this.discardSession(id);
			});
		} else {
			// Buffer chunks until save dialog resolves (bounded)
			if (
				session.pendingChunks.length >=
				DownloadSessionManager.MAX_PENDING_CHUNKS
			) {
				console.warn(
					"[WARN][FRONTEND] Download: pending chunks limit reached, discarding session",
				);
				this.discardSession(id);
				return;
			}
			session.pendingChunks.push(data);
		}

		// Update progress
		if (session.expectedSize > 0) {
			const estimatedEncodedSize = Math.ceil(
				(session.expectedSize * 4) / 3,
			);
			const progress = Math.min(
				(session.receivedBytes / estimatedEncodedSize) * 100,
				99,
			);
			this.progressDisplay.show(session.filename, progress);
		}
	}

	private async flushPendingChunks(session: DownloadSession): Promise<void> {
		const chunks = session.pendingChunks;
		session.pendingChunks = [];
		for (const chunk of chunks) {
			await this.appendChunk(session, chunk);
		}
	}

	private async appendChunk(
		session: DownloadSession,
		data: string,
	): Promise<void> {
		await invoke("append_download_chunk", {
			id: session.handleId,
			dataBase64: data,
		});
	}

	private async handleEnd(params: Record<string, string>): Promise<void> {
		const id = params.id;
		if (!id) return;

		const session = this.sessions.get(id);
		if (!session) return;

		this.sessions.delete(id);

		if (!session.handleId) {
			// Handle not yet assigned (dialog may still be open or was cancelled)
			this.progressDisplay.hide();
			// Note: if dialog is still open and later confirms, startDownload
			// will see the session was deleted and return early.
			return;
		}

		// Flush any remaining pending chunks before finishing
		if (session.pendingChunks.length > 0) {
			await this.flushPendingChunks(session);
		}

		try {
			await invoke("finish_download_file", {
				id: session.handleId,
			});
			this.progressDisplay.showCompleted(session.filename);
		} catch (err) {
			console.error("[ERROR][FRONTEND] Download: finish failed", err);
			this.progressDisplay.hide();
		}
	}

	private discardSession(id: string): void {
		const session = this.sessions.get(id);
		if (session?.handleId) {
			invoke("cancel_download_file", { id: session.handleId }).catch(
				() => {},
			);
		}
		this.sessions.delete(id);
		this.progressDisplay.hide();
	}

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

	private startCleanupTimer(): void {
		this.cleanupTimer = setInterval(() => {
			this.cleanupExpiredSessions();
		}, DownloadSessionManager.CLEANUP_INTERVAL);
	}

	cleanupExpiredSessions(): void {
		const now = Date.now();
		for (const [id, session] of this.sessions) {
			if (
				now - session.lastChunkAt >
				DownloadSessionManager.SESSION_TIMEOUT
			) {
				this.discardSession(id);
			}
		}
	}

	getSession(id: string): DownloadSession | undefined {
		return this.sessions.get(id);
	}

	get sessionCount(): number {
		return this.sessions.size;
	}

	dispose(): void {
		if (this.cleanupTimer !== null) {
			clearInterval(this.cleanupTimer);
			this.cleanupTimer = null;
		}
		// Cancel all active sessions
		for (const [id] of this.sessions) {
			const session = this.sessions.get(id);
			if (session?.handleId) {
				invoke("cancel_download_file", {
					id: session.handleId,
				}).catch(() => {});
			}
		}
		this.sessions.clear();
		this.progressDisplay.dispose();
	}
}
