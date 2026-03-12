/**
 * Download session manager.
 *
 * Manages file download sessions for OSC 777 download extension.
 * Handles begin/chunk/end lifecycle, progress tracking, and save dialog.
 *
 * @module download/session
 */

import { invoke } from "@tauri-apps/api/core";

import { DownloadProgressDisplay } from "./progress.ts";

interface DownloadSession {
	id: string;
	filename: string;
	expectedSize: number;
	chunks: Map<number, string>;
	receivedBytes: number;
	lastChunkAt: number;
}

export class DownloadSessionManager {
	static readonly SESSION_TIMEOUT = 60 * 1000;
	static readonly MAX_SESSIONS = 10;
	/** Maximum download size in bytes (500MB) */
	static readonly MAX_DOWNLOAD_SIZE = 500 * 1024 * 1024;
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
		if (expectedSize > DownloadSessionManager.MAX_DOWNLOAD_SIZE) {
			console.warn(
				`[WARN][FRONTEND] Download: rejected, size ${expectedSize} exceeds limit`,
			);
			return;
		}

		const session: DownloadSession = {
			id,
			filename: this.sanitizeFilename(params.name || "download"),
			expectedSize,
			chunks: new Map(),
			receivedBytes: 0,
			lastChunkAt: Date.now(),
		};

		this.sessions.set(id, session);
		this.progressDisplay.show(session.filename, 0);
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
		if (seq !== session.chunks.size) {
			// Out-of-order: discard session
			this.discardSession(id);
			return;
		}

		session.chunks.set(seq, data);
		session.receivedBytes += data.length;
		session.lastChunkAt = Date.now();

		// Guard against size=0 bypass: check accumulated size regardless of expectedSize
		const maxEncodedSize = Math.ceil(
			(DownloadSessionManager.MAX_DOWNLOAD_SIZE * 4) / 3,
		);
		if (session.receivedBytes > maxEncodedSize) {
			console.warn(
				"[WARN][FRONTEND] Download: accumulated size exceeds limit, discarding",
			);
			this.discardSession(id);
			return;
		}

		// Update progress
		if (session.expectedSize > 0) {
			// Approximate: base64 encoded size is ~4/3 of original
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

	private async handleEnd(params: Record<string, string>): Promise<void> {
		const id = params.id;
		if (!id) return;

		const session = this.sessions.get(id);
		if (!session) return;

		this.sessions.delete(id);

		// Concatenate all base64 chunks (already base64 encoded)
		const sortedSeqs = Array.from(session.chunks.keys()).sort(
			(a, b) => a - b,
		);
		const base64Data = sortedSeqs
			.map((seq) => session.chunks.get(seq)!)
			.join("");

		// Validate base64 is decodable
		try {
			atob(base64Data);
		} catch {
			console.error(
				"[ERROR][FRONTEND] Download: invalid base64 data",
			);
			this.progressDisplay.hide();
			return;
		}

		this.progressDisplay.show(session.filename, 100);

		// Send base64 string directly to backend for decoding + save dialog
		// (avoids Array.from(Uint8Array) which creates a huge JSON number array)
		try {
			const savedPath = await invoke<string | null>("write_download_file", {
				filename: session.filename,
				data_base64: base64Data,
			});

			if (savedPath) {
				this.progressDisplay.showCompleted(session.filename);
			} else {
				// User cancelled
				this.progressDisplay.showCancelled();
			}
		} catch (err) {
			console.error("[ERROR][FRONTEND] Download: save failed", err);
			this.progressDisplay.hide();
		}
	}

	private discardSession(id: string): void {
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
		this.sessions.clear();
		this.progressDisplay.dispose();
	}
}
