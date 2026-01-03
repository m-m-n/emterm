/**
 * PTY Client - Manages communication with the Tauri PTY backend.
 *
 * This module provides the PtyClient class for spawning and interacting
 * with pseudo-terminal sessions.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  SpawnResult,
  PtyOutputPayload,
  PtyExitPayload,
  PtyErrorPayload,
  PtySpawnOptions,
  PtyOutputCallback,
  PtyExitCallback,
  PtyErrorCallback,
} from "../types/pty";
import type { TerminalActionsPayload } from "../types/terminal";

/**
 * Client for managing PTY (Pseudo Terminal) sessions.
 *
 * Provides methods for spawning shells, sending input, resizing,
 * and listening to output events.
 *
 * @example
 * ```typescript
 * const client = new PtyClient();
 *
 * // Set up listeners before spawning
 * await client.onOutput((data) => {
 *   console.log('Output:', new TextDecoder().decode(data));
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

  /** Pending terminal action events that arrived before sessionId was set */
  private pendingTerminalActions: TerminalActionsPayload[] = [];

  /** Callback for terminal actions (stored for replay) */
  private terminalActionsCallback: ((payload: TerminalActionsPayload) => void) | null = null;

  /**
   * Returns the current session ID, or null if no session is active.
   */
  getSessionId(): string | null {
    return this.sessionId;
  }

  /**
   * Spawns a new PTY session with the specified options.
   *
   * @param options - Configuration options for the session
   * @returns The session ID of the spawned session
   * @throws Error if a session is already active or spawn fails
   */
  async spawn(options: PtySpawnOptions = {}): Promise<string> {
    if (this.sessionId !== null) {
      throw new Error("PTY session already active. Call kill() first.");
    }

    const result = await invoke<SpawnResult>("pty_spawn", {
      shell: options.shell,
      cols: options.cols ?? 80,
      rows: options.rows ?? 24,
    });

    this.sessionId = result.session_id;
    return this.sessionId;
  }

  /**
   * Writes data to the PTY session.
   *
   * @param data - String or byte array to send to the shell
   * @throws Error if no session is active
   */
  async write(data: Uint8Array | string): Promise<void> {
    if (!this.sessionId) {
      throw new Error("PTY session not started");
    }

    const bytes =
      typeof data === "string" ? new TextEncoder().encode(data) : data;

    await invoke("pty_write", {
      sessionId: this.sessionId,
      data: Array.from(bytes),
    });
  }

  /**
   * Resizes the PTY session to the specified dimensions.
   *
   * @param cols - New number of columns
   * @param rows - New number of rows
   * @throws Error if no session is active
   */
  async resize(cols: number, rows: number): Promise<void> {
    if (!this.sessionId) {
      throw new Error("PTY session not started");
    }

    await invoke("pty_resize", {
      sessionId: this.sessionId,
      cols,
      rows,
    });
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
  }

  /**
   * Registers a callback for PTY output data.
   *
   * @param callback - Function called with output data as Uint8Array
   */
  async onOutput(callback: PtyOutputCallback): Promise<void> {
    const currentSessionId = this.sessionId;
    const unlisten = await listen<PtyOutputPayload>("pty_output", (event) => {
      // Only process events for the current session
      if (
        currentSessionId !== null &&
        event.payload.session_id === currentSessionId
      ) {
        callback(new Uint8Array(event.payload.data));
      } else if (this.sessionId !== null && event.payload.session_id === this.sessionId) {
        // Handle case where session was spawned after listener registration
        callback(new Uint8Array(event.payload.data));
      }
    });
    this.unlisteners.push(unlisten);
  }

  /**
   * Registers a callback for PTY exit events.
   *
   * @param callback - Function called with the exit code
   */
  async onExit(callback: PtyExitCallback): Promise<void> {
    const unlisten = await listen<PtyExitPayload>("pty_exit", (event) => {
      // Check sessionId at event time, not registration time
      if (this.sessionId !== null && event.payload.session_id === this.sessionId) {
        callback(event.payload.code);
        this.sessionId = null;
      }
    });
    this.unlisteners.push(unlisten);
  }

  /**
   * Registers a callback for PTY error events.
   *
   * @param callback - Function called with the error message
   */
  async onError(callback: PtyErrorCallback): Promise<void> {
    const unlisten = await listen<PtyErrorPayload>("pty_error", (event) => {
      if (event.payload.session_id === this.sessionId) {
        callback(event.payload.message);
      }
    });
    this.unlisteners.push(unlisten);
  }

  /**
   * Registers a callback for terminal actions events.
   * This will be used when the ANSI parser is integrated (Phase 1).
   *
   * @param callback - Function called with parsed terminal actions
   */
  async onTerminalActions(
    callback: (payload: TerminalActionsPayload) => void
  ): Promise<void> {
    this.terminalActionsCallback = callback;

    const unlisten = await listen<TerminalActionsPayload>(
      "terminal_actions",
      (event) => {
        if (this.sessionId === null) {
          // sessionId not yet set (spawn hasn't returned yet), buffer the event
          this.pendingTerminalActions.push(event.payload);
        } else if (event.payload.session_id === this.sessionId) {
          callback(event.payload);
        }
      }
    );
    this.unlisteners.push(unlisten);
  }

  /**
   * Flushes pending terminal action events that arrived before sessionId was set.
   * Should be called immediately after spawn() returns.
   */
  flushPendingTerminalActions(): void {
    if (this.sessionId === null || this.terminalActionsCallback === null) {
      return;
    }

    for (const pending of this.pendingTerminalActions) {
      if (pending.session_id === this.sessionId) {
        this.terminalActionsCallback(pending);
      }
    }

    this.pendingTerminalActions = [];
  }

  /**
   * Cleans up all event listeners.
   *
   * Should be called when the client is no longer needed to prevent
   * memory leaks.
   */
  dispose(): void {
    for (const unlisten of this.unlisteners) {
      unlisten();
    }
    this.unlisteners = [];
  }
}
