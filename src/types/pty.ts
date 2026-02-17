/**
 * PTY (Pseudo Terminal) type definitions for IPC communication.
 *
 * These types correspond to the Rust backend payload structures
 * defined in src-tauri/src/lib.rs.
 */

/**
 * Result returned from the pty_spawn command.
 */
export interface SpawnResult {
	session_id: string;
}

/**
 * Payload for the pty_exit event.
 * Emitted when the shell process terminates.
 */
export interface PtyExitPayload {
	session_id: string;
	/** Exit code of the process (0 = success) */
	code: number;
	/** Number of remaining sessions after this session was removed */
	remaining_sessions: number;
}

/**
 * Payload for the pty_error event.
 * Emitted when an error occurs in the PTY session.
 */
export interface PtyErrorPayload {
	session_id: string;
	/** Human-readable error message */
	message: string;
}

/**
 * Options for spawning a new PTY session.
 */
export interface PtySpawnOptions {
	/**
	 * Path to the shell executable.
	 * If not specified, the default shell for the platform is used.
	 */
	shell?: string;

	/**
	 * Arguments to pass to the shell executable.
	 */
	args?: string[];

	/**
	 * Number of terminal columns.
	 * @default 80
	 */
	cols?: number;

	/**
	 * Number of terminal rows.
	 * @default 24
	 */
	rows?: number;
}

/**
 * Callback type for PTY exit events.
 * @param code - Exit code of the process
 * @param remainingSessions - Number of remaining sessions after this one exited
 */
export type PtyExitCallback = (code: number, remainingSessions: number) => void;

/**
 * Callback type for PTY error events.
 */
export type PtyErrorCallback = (message: string) => void;
