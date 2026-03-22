/**
 * Mux IPC client — connects to daemon via Tauri bridge commands.
 *
 * The WebView cannot access Unix sockets directly, so communication
 * goes through Tauri invoke commands (bridge.rs on the Rust side).
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** IPC message type constants (must match protocol.rs MessageType). */
export const MuxMessageType = {
  CreatePane: 0x05,
  DestroyPane: 0x07,
  Resize: 0x08,
  Attach: 0x09,
  Detach: 0x0a,
  SplitPane: 0x11,
  CreateWindow: 0x12,
  SwitchWindow: 0x13,
  RenameWindow: 0x14,
  DestroyWindow: 0x15,
} as const;

/** Connection state for the mux client. */
export type MuxConnectionState = "disconnected" | "connecting" | "connected" | "error";

/** Session info returned from daemon. */
export interface MuxSessionInfo {
  id: number;
  name: string;
  window_count: number;
  pane_count: number;
}

/** Validate that a socket path is safe (no path traversal, allowed directory). */
export function validateSocketPath(path: string): boolean {
  if (path.includes("../") || path.includes("..\\")) {
    return false;
  }
  // Additional validation is done server-side in bridge.rs
  return path.includes("emterm") && path.endsWith(".sock");
}

/** Parse an OSC 777 mux command.
 *  Format: "emterm;mux;action;param1;param2"
 *  Returns null if not a valid mux command.
 */
export function parseMuxOsc(
  verb: string,
  params: string[],
): { action: "attach" | "detach"; socketPath?: string; sessionId?: number } | null {
  if (verb !== "emterm" || params.length < 1 || params[0] !== "mux") {
    return null;
  }
  const action = params[1];
  if (action === "attach" && params.length >= 4) {
    const socketPath = params[2]!;
    const sessionId = parseInt(params[3]!, 10);
    if (!validateSocketPath(socketPath)) {
      console.error("[ERROR][FRONTEND] Invalid mux socket path:", socketPath);
      return null;
    }
    return { action: "attach", socketPath, sessionId: isNaN(sessionId) ? 0 : sessionId };
  }
  if (action === "detach") {
    return { action: "detach" };
  }
  return null;
}

/** Mux IPC client for connecting to the daemon. */
export class MuxClient {
  private connId: string | null = null;
  private _state: MuxConnectionState = "disconnected";
  private onStateChange: ((state: MuxConnectionState) => void) | null = null;
  private outputUnlisten: UnlistenFn | null = null;
  private exitedUnlisten: UnlistenFn | null = null;
  private createdUnlisten: UnlistenFn | null = null;
  private onPtyOutput: ((paneId: number, data: Uint8Array) => void) | null = null;
  private onPtyExited: ((paneId: number) => void) | null = null;
  private onPaneCreated: ((paneId: number) => void) | null = null;

  get state(): MuxConnectionState {
    return this._state;
  }

  /** Register a state change callback. */
  setOnStateChange(callback: (state: MuxConnectionState) => void): void {
    this.onStateChange = callback;
  }

  private setState(state: MuxConnectionState): void {
    this._state = state;
    this.onStateChange?.(state);
  }

  /** Connect to the daemon and perform handshake. */
  async connect(socketPath: string): Promise<MuxSessionInfo[]> {
    this.setState("connecting");
    try {
      this.connId = await invoke<string>("mux_connect", { socketPath });
      const sessions = await invoke<MuxSessionInfo[]>("mux_handshake", {
        connId: this.connId,
      });
      this.setState("connected");
      return sessions;
    } catch (e) {
      this.setState("error");
      throw e;
    }
  }

  /** Set callback for PTY output from daemon panes. */
  setOnPtyOutput(callback: (paneId: number, data: Uint8Array) => void): void {
    this.onPtyOutput = callback;
  }

  /** Set callback for PTY exit notification. */
  setOnPtyExited(callback: (paneId: number) => void): void {
    this.onPtyExited = callback;
  }

  /** Set callback for pane creation (receives actual pane ID from daemon). */
  setOnPaneCreated(callback: (paneId: number) => void): void {
    this.onPaneCreated = callback;
  }

  /** Start the output stream -- calls mux_start_output_stream and listens for events. */
  async startOutputStream(): Promise<void> {
    if (!this.connId) throw new Error("Not connected");

    // Listen for mux-pty-output events
    this.outputUnlisten = await listen<{ pane_id: number; data: number[] }>(
      "mux-pty-output",
      (event) => {
        if (this.onPtyOutput) {
          this.onPtyOutput(event.payload.pane_id, new Uint8Array(event.payload.data));
        }
      },
    );

    // Listen for mux-pty-exited events
    this.exitedUnlisten = await listen<{ pane_id: number; exit_code: number | null }>(
      "mux-pty-exited",
      (event) => {
        if (this.onPtyExited) {
          this.onPtyExited(event.payload.pane_id);
        }
      },
    );

    // Listen for mux-pane-created events (actual pane ID from daemon)
    this.createdUnlisten = await listen<{ pane_id: number }>(
      "mux-pane-created",
      (event) => {
        if (this.onPaneCreated) {
          this.onPaneCreated(event.payload.pane_id);
        }
      },
    );

    // Tell backend to start reading output from daemon
    await invoke("mux_start_output_stream", { connId: this.connId });
  }

  /** Disconnect from the daemon. */
  async disconnect(): Promise<void> {
    if (this.outputUnlisten) {
      this.outputUnlisten();
      this.outputUnlisten = null;
    }
    if (this.exitedUnlisten) {
      this.exitedUnlisten();
      this.exitedUnlisten = null;
    }
    if (this.createdUnlisten) {
      this.createdUnlisten();
      this.createdUnlisten = null;
    }
    if (this.connId) {
      try {
        await invoke("mux_disconnect", { connId: this.connId });
      } catch {
        // Ignore disconnect errors
      }
      this.connId = null;
    }
    this.setState("disconnected");
  }

  /** Send PTY input to a pane. */
  async sendInput(paneId: number, data: Uint8Array): Promise<void> {
    if (!this.connId) throw new Error("Not connected");
    await invoke("mux_send_input", {
      connId: this.connId,
      paneId,
      data: Array.from(data),
    });
  }

  /** Send a control message to the daemon and optionally receive a response. */
  async sendControl(
    msgType: number,
    paneId: number,
    payload: Uint8Array = new Uint8Array(),
  ): Promise<Uint8Array | null> {
    if (!this.connId) throw new Error("Not connected");
    const result = await invoke<number[] | null>("mux_send_control", {
      connId: this.connId,
      msgType,
      paneId,
      payload: Array.from(payload),
    });
    return result ? new Uint8Array(result) : null;
  }

  /** Check if connected. */
  get isConnected(): boolean {
    return this._state === "connected" && this.connId !== null;
  }
}
