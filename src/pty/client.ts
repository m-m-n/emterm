/**
 * PTY Client - Manages communication with the Tauri PTY backend.
 *
 * Uses Tauri Channel for binary IPC (raw PTY data sent as Vec<u8>).
 * WASM parser processes the raw data in the frontend.
 */

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Reusable TextEncoder instance to avoid per-call allocation. */
const textEncoder = new TextEncoder();
import type {
	PtyErrorCallback,
	PtyErrorPayload,
	PtyExitCallback,
	PtyExitPayload,
	PtySpawnOptions,
	SpawnResult,
} from "../types/pty";

/**
 * Callback type for raw PTY data received via Channel.
 */
export type PtyDataCallback = (data: Uint8Array) => void;

/**
 * Client for managing PTY (Pseudo Terminal) sessions.
 *
 * Provides methods for spawning shells, sending input, resizing,
 * and listening to output events via binary Channel IPC.
 *
 * @example
 * ```typescript
 * const client = new PtyClient();
 *
 * // Set up data handler before spawning
 * client.onData((data) => {
 *   // Process raw PTY data through WASM
 *   wasmCore.process_pty_data(data);
 * });
 *
 * await client.onExit((code) => {
 *   console.log('Process exited with code:', code);
 * });
 *
 * // Spawn a shell
 * const sessionId = await client.spawn({ cols: 80, rows: 24 });
 *
 * // Send input
 * await client.write('echo hello\n');
 *
 * // Cleanup when done
 * client.dispose();
 * ```
 */
export class PtyClient {
	/** Current session ID, null if no session is active */
	private sessionId: string | null = null;

	/** List of event unsubscribe functions */
	private unlisteners: UnlistenFn[] = [];

	/** Flag to prevent duplicate exit event processing */
	private exitHandled = false;

	/** Tauri Channel for binary PTY data */
	private channel: Channel<number[]> | null = null;

	/** Callback for raw PTY data */
	private dataCallback: PtyDataCallback | null = null;

	/**
	 * Returns the current session ID, or null if no session is active.
	 */
	getSessionId(): string | null {
		return this.sessionId;
	}

	/**
	 * Registers a callback for raw PTY data received via Channel.
	 *
	 * Must be called before spawn() to avoid missing data.
	 *
	 * @param callback - Function called with raw PTY data as Uint8Array
	 */
	onData(callback: PtyDataCallback): void {
		this.dataCallback = callback;
	}

	/**
	 * Spawns a new PTY session with the specified options.
	 *
	 * Creates a Tauri Channel for binary IPC and passes it to the backend.
	 *
	 * @param options - Configuration options for the session
	 * @returns The session ID of the spawned session
	 * @throws Error if a session is already active or spawn fails
	 */
	async spawn(options: PtySpawnOptions = {}): Promise<string> {
		if (this.sessionId !== null) {
			throw new Error("PTY session already active. Call kill() first.");
		}

		// Reset exitHandled flag for new session
		this.exitHandled = false;

		// Create Channel for binary data transfer
		this.channel = new Channel<number[]>();
		this.channel.onmessage = (data: number[]) => {
			if (this.dataCallback) {
				this.dataCallback(new Uint8Array(data));
			}
		};

		const result = await invoke<SpawnResult>("pty_spawn", {
			channel: this.channel,
			shell: options.shell,
			args: options.args,
			cols: options.cols ?? 80,
			rows: options.rows ?? 24,
		});

		const sessionId = result.session_id;
		this.sessionId = sessionId;
		return sessionId;
	}

	/**
	 * Writes data to the PTY session.
	 *
	 * Non-async to minimize overhead on the key repeat hot path.
	 * The backend pty_write command is synchronous (channel send),
	 * so the returned Promise resolves quickly.
	 *
	 * @param data - String or byte array to send to the shell
	 * @throws Error if no session is active
	 */
	write(data: Uint8Array | string): Promise<void> {
		if (!this.sessionId) {
			return Promise.reject(new Error("PTY session not started"));
		}

		const bytes =
			typeof data === "string" ? textEncoder.encode(data) : data;

		// Array.from() is required: Tauri v2 invoke uses JSON serialization,
		// and JSON.stringify(Uint8Array) produces an object, not an array.
		return invoke<void>("pty_write", {
			sessionId: this.sessionId,
			data: Array.from(bytes),
		});
	}

	/**
	 * Resizes the PTY session to the specified dimensions.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 * @returns true if resize was performed, false if no session is active
	 */
	async resize(cols: number, rows: number): Promise<boolean> {
		if (!this.sessionId) {
			// No session active yet - silently skip (initial size is set on spawn)
			return false;
		}

		await invoke("pty_resize", {
			sessionId: this.sessionId,
			cols,
			rows,
		});
		return true;
	}

	/**
	 * Terminates the current PTY session.
	 *
	 * Safe to call even if no session is active.
	 */
	async kill(): Promise<void> {
		if (!this.sessionId) {
			return;
		}

		await invoke("pty_kill", {
			sessionId: this.sessionId,
		});

		this.sessionId = null;
		this.exitHandled = false;
		this.channel = null;
	}

	/**
	 * Registers a callback for PTY exit events.
	 *
	 * @param callback - Function called with the exit code and remaining session count
	 */
	async onExit(callback: PtyExitCallback): Promise<void> {
		const unlisten = await listen<PtyExitPayload>(
			"pty_exit",
			(event: { payload: PtyExitPayload }) => {
				// Prevent duplicate processing
				if (this.exitHandled) {
					if (import.meta.env?.DEV) {
						console.log(
							"[PtyClient] pty_exit already handled, ignoring duplicate event",
						);
					}
					return;
				}

				// NOTE: This implementation assumes single-session model (one PTY per window).
				// The condition `sessionId === null` handles the race where shell exits before
				// spawn() returns, but this will NOT work correctly in multi-tab scenarios.
				// FUTURE: When implementing multi-tab support, replace this with event buffering
				// to avoid processing events from unrelated sessions. See SPEC.md NFR4.
				if (
					this.sessionId === null ||
					event.payload.session_id === this.sessionId
				) {
					if (import.meta.env?.DEV) {
						console.log(
							`[PtyClient] pty_exit received: code=${event.payload.code}, remaining=${event.payload.remaining_sessions}`,
						);
					}

					this.exitHandled = true; // Mark as handled

					// Ensure listener cleanup even if callback throws
					try {
						callback(event.payload.code, event.payload.remaining_sessions);
					} finally {
						// Remove from unlisteners array and cleanup listener
						const index = this.unlisteners.indexOf(unlisten);
						if (index > -1) {
							this.unlisteners.splice(index, 1);
						}
						unlisten(); // Cleanup listener
						this.sessionId = null;
						this.channel = null;
					}
				}
			},
		);
		this.unlisteners.push(unlisten);
	}

	/**
	 * Registers a callback for PTY error events.
	 *
	 * @param callback - Function called with the error message
	 */
	async onError(callback: PtyErrorCallback): Promise<void> {
		const unlisten = await listen<PtyErrorPayload>(
			"pty_error",
			(event: { payload: PtyErrorPayload }) => {
				if (event.payload.session_id === this.sessionId) {
					callback(event.payload.message);
				}
			},
		);
		this.unlisteners.push(unlisten);
	}

	/**
	 * Cleans up all event listeners and channel.
	 *
	 * Should be called when the client is no longer needed to prevent
	 * memory leaks.
	 */
	dispose(): void {
		for (const unlisten of this.unlisteners) {
			unlisten();
		}
		this.unlisteners = [];
		this.channel = null;
		this.dataCallback = null;
	}
}
