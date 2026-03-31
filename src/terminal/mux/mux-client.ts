/**
 * Mux IPC client -- communicates with daemon via APC escape sequences over PTY.
 *
 * The GUI writes APC-encoded MuxMessages to PTY stdin (bridge process reads them)
 * and receives APC-encoded responses from PTY output stream (via WASM APC callback).
 */

import type { PtyClient } from "../../pty/client";
import { muxLog } from "./mux-logger";

/** APC prefix for emterm mux messages (must match Rust APC_PREFIX). */
export const MUX_APC_PREFIX = "emterm-mux;";

/** OSC parameter for emterm mux messages (fallback for Windows ConPTY). */
const MUX_OSC_PARAM = 9999;

/** IPC message type constants (must match protocol.rs MessageType). */
export const MuxMessageType = {
  PtyOutput: 0x01,
  PtyInput: 0x02,
  Hello: 0x03,
  Welcome: 0x04,
  CreatePane: 0x05,
  PaneCreated: 0x06,
  DestroyPane: 0x07,
  Resize: 0x08,
  Attach: 0x09,
  Detach: 0x0a,
  Detached: 0x0b,
  Snapshot: 0x0c,
  SnapshotRestore: 0x0d,
  SessionList: 0x0e,
  Error: 0x0f,
  PtyExited: 0x10,
  SplitPane: 0x11,
  CreateWindow: 0x12,
  SwitchWindow: 0x13,
  RenameWindow: 0x14,
  DestroyWindow: 0x15,
  StatusUpdate: 0x16,
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

/**
 * Encode a MuxMessage frame body as an APC escape sequence string.
 *
 * Frame body format: [type: u8][pane_id: u32 LE][payload: variable]
 * APC format: ESC _ emterm-mux;<base64(frame_body)> ESC \
 */
function encodeApc(msgType: number, paneId: number, payload: Uint8Array = new Uint8Array()): string {
  const frameBody = new Uint8Array(5 + payload.length);
  frameBody[0] = msgType;
  frameBody[1] = paneId & 0xff;
  frameBody[2] = (paneId >> 8) & 0xff;
  frameBody[3] = (paneId >> 16) & 0xff;
  frameBody[4] = (paneId >> 24) & 0xff;
  frameBody.set(payload, 5);

  const base64 = uint8ArrayToBase64(frameBody);
  return `\x1b_${MUX_APC_PREFIX}${base64}\x1b\\`;
}

/**
 * Encode a MuxMessage frame body as an OSC 9999 escape sequence string.
 * Used as fallback transport when ConPTY strips APC sequences (Windows).
 */
function encodeOsc(msgType: number, paneId: number, payload: Uint8Array = new Uint8Array()): string {
  const frameBody = new Uint8Array(5 + payload.length);
  frameBody[0] = msgType;
  frameBody[1] = paneId & 0xff;
  frameBody[2] = (paneId >> 8) & 0xff;
  frameBody[3] = (paneId >> 16) & 0xff;
  frameBody[4] = (paneId >> 24) & 0xff;
  frameBody.set(payload, 5);

  const base64 = uint8ArrayToBase64(frameBody);
  return `\x1b]${MUX_OSC_PARAM};${MUX_APC_PREFIX}${base64}\x1b\\`;
}

/** Detect if running on Windows (ConPTY requires OSC transport). */
const isWindows = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

/**
 * Encode a mux message using the appropriate transport.
 * Windows uses OSC 9999 (ConPTY strips APC), Linux uses APC.
 */
function encodeMuxMessage(msgType: number, paneId: number, payload: Uint8Array = new Uint8Array()): string {
  const transport = isWindows ? "OSC" : "APC";
  const encoded = isWindows ? encodeOsc(msgType, paneId, payload) : encodeApc(msgType, paneId, payload);
  muxLog.debug(`SEND ${transport}: type=0x${msgType.toString(16)} pane=${paneId} payload=${payload.length}B encoded=${encoded.length}B isWindows=${isWindows}`);
  return encoded;
}

/**
 * Decode an APC payload (between ESC_ and ESC\) to a parsed MuxMessage.
 * Returns null if the payload doesn't have the mux prefix or is invalid.
 */
export function decodeApcPayload(payload: string): { msgType: number; paneId: number; data: Uint8Array } | null {
  if (!payload.startsWith(MUX_APC_PREFIX)) return null;

  const base64Str = payload.substring(MUX_APC_PREFIX.length);
  let frameBody: Uint8Array;
  try {
    frameBody = base64ToUint8Array(base64Str);
  } catch {
    return null;
  }

  if (frameBody.length < 5) return null;

  const msgType = frameBody[0]!;
  const paneId = (
    frameBody[1]! |
    (frameBody[2]! << 8) |
    (frameBody[3]! << 16) |
    (frameBody[4]! << 24)
  ) >>> 0;
  const data = frameBody.slice(5);

  return { msgType, paneId, data };
}

/** Base64 encode Uint8Array. */
function uint8ArrayToBase64(bytes: Uint8Array): string {
  // Use chunk-based approach to avoid O(n^2) string concatenation
  const chunks: string[] = [];
  const chunkSize = 8192;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const end = Math.min(i + chunkSize, bytes.length);
    let chunk = "";
    for (let j = i; j < end; j++) {
      chunk += String.fromCharCode(bytes[j]!);
    }
    chunks.push(chunk);
  }
  return btoa(chunks.join(""));
}

/** Base64 decode string to Uint8Array. */
function base64ToUint8Array(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * Decode a bincode-serialized WelcomeMsg from raw bytes.
 *
 * bincode format for Accepted variant:
 * - variant index: u32 LE (0 = Accepted)
 * - server_version: u32 LE
 * - sessions: Vec<SessionInfo> (length u64 LE, then items)
 *   - id: u32 LE
 *   - name: String (length u64 LE, then UTF-8 bytes)
 *   - window_count: u32 LE
 *   - pane_count: u32 LE
 *   - active_window_index: u32 LE
 */
export function decodeWelcomeMsg(data: Uint8Array): MuxSessionInfo[] | null {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let offset = 0;

  if (data.length < 4) return null;

  const variant = view.getUint32(offset, true);
  offset += 4;

  if (variant !== 0) {
    // Rejected variant
    return null;
  }

  // server_version
  if (offset + 4 > data.length) return null;
  // const serverVersion = view.getUint32(offset, true);
  offset += 4;

  // sessions Vec length (u64 LE)
  if (offset + 8 > data.length) return null;
  const sessionsLen = Number(view.getBigUint64(offset, true));
  offset += 8;

  const sessions: MuxSessionInfo[] = [];
  for (let i = 0; i < sessionsLen; i++) {
    // id: u32
    if (offset + 4 > data.length) return null;
    const id = view.getUint32(offset, true);
    offset += 4;

    // name: String (u64 len + bytes)
    if (offset + 8 > data.length) return null;
    const nameLen = Number(view.getBigUint64(offset, true));
    offset += 8;
    if (offset + nameLen > data.length) return null;
    const nameBytes = data.slice(offset, offset + nameLen);
    const name = new TextDecoder().decode(nameBytes);
    offset += nameLen;

    // window_count: u32
    if (offset + 4 > data.length) return null;
    const window_count = view.getUint32(offset, true);
    offset += 4;

    // pane_count: u32
    if (offset + 4 > data.length) return null;
    const pane_count = view.getUint32(offset, true);
    offset += 4;

    // active_window_index: u32
    if (offset + 4 > data.length) return null;
    // const activeWindowIndex = view.getUint32(offset, true);
    offset += 4;

    sessions.push({ id, name, window_count, pane_count });
  }

  return sessions;
}

/** Validate that a socket path is safe (no path traversal, allowed directory). */
export function validateSocketPath(path: string): boolean {
  if (path.includes("../") || path.includes("..\\")) {
    return false;
  }
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
      muxLog.error(`Invalid mux socket path: ${socketPath}`);
      return null;
    }
    return { action: "attach", socketPath, sessionId: isNaN(sessionId) ? 0 : sessionId };
  }
  if (action === "detach") {
    return { action: "detach" };
  }
  return null;
}

/** Mux IPC client communicating via APC over PTY. */
export class MuxClient {
  private ptyClient: PtyClient | null = null;
  private _state: MuxConnectionState = "disconnected";
  private onStateChange: ((state: MuxConnectionState) => void) | null = null;
  private onPtyOutput: ((paneId: number, data: Uint8Array) => void) | null = null;
  private onPtyExited: ((paneId: number) => void) | null = null;
  private onPaneCreated: ((paneId: number) => void) | null = null;
  private onDetached: (() => void) | null = null;
  private onStatusUpdate: ((msg: { session_name: string; window_names: string[]; active_window_index: number }) => void) | null = null;

  get state(): MuxConnectionState {
    return this._state;
  }

  /** Register a state change callback. */
  setOnStateChange(callback: (state: MuxConnectionState) => void): void {
    this.onStateChange = callback;
  }

  private setState(state: MuxConnectionState): void {
    muxLog.info(`State: ${this._state} → ${state}`);
    this._state = state;
    this.onStateChange?.(state);
  }

  /** Set the PtyClient used for APC communication. */
  setPtyClient(ptyClient: PtyClient): void {
    this.ptyClient = ptyClient;
  }

  /** Handle Welcome from bridge process and transition to connected state.
   *  Called when the bridge's initial Welcome APC is received.
   */
  handleWelcome(sessions: MuxSessionInfo[]): void {
    if (this._state === "connected") return; // ignore duplicate Welcome
    this.setState("connected");
    // Sessions are returned to the caller via the Promise in waitForWelcome
    this._pendingWelcomeResolve?.(sessions);
    this._pendingWelcomeResolve = null;
  }

  private _pendingWelcomeResolve: ((sessions: MuxSessionInfo[]) => void) | null = null;
  private _pendingWelcomeReject: ((reason: string) => void) | null = null;

  private _welcomeTimeoutId: ReturnType<typeof setTimeout> | null = null;

  /** Wait for the bridge to send a Welcome APC. Returns sessions list. */
  waitForWelcome(timeoutMs: number = 10000): Promise<MuxSessionInfo[]> {
    this.setState("connecting");
    return new Promise((resolve, reject) => {
      this._pendingWelcomeResolve = (sessions) => {
        if (this._welcomeTimeoutId !== null) {
          clearTimeout(this._welcomeTimeoutId);
          this._welcomeTimeoutId = null;
        }
        resolve(sessions);
      };
      this._pendingWelcomeReject = (reason) => {
        if (this._welcomeTimeoutId !== null) {
          clearTimeout(this._welcomeTimeoutId);
          this._welcomeTimeoutId = null;
        }
        reject(reason);
      };
      this._welcomeTimeoutId = setTimeout(() => {
        if (this._pendingWelcomeResolve) {
          this._pendingWelcomeResolve = null;
          this._pendingWelcomeReject = null;
          this._welcomeTimeoutId = null;
          this.setState("error");
          reject("Mux handshake timeout");
        }
      }, timeoutMs);
    });
  }

  // Dedup state: during transport negotiation the bridge sends both OSC and APC,
  // causing identical messages to arrive twice on Linux. Skip the duplicate.
  private _lastDedupType: number = -1;
  private _lastDedupPaneId: number = -1;
  private _lastDedupDataLen: number = -1;
  private _lastDedupDataHead: number = 0;

  /**
   * Handle an incoming mux APC message from the PTY output stream.
   * Called by the APC handler when it detects the emterm-mux; prefix.
   */
  handleIncomingApc(msgType: number, paneId: number, data: Uint8Array): void {
    muxLog.debug(`RECV: type=0x${msgType.toString(16)} pane=${paneId} data=${data.length}B`);
    // Dedup: skip identical consecutive messages (from dual OSC+APC transport)
    const head = data.length >= 4
      ? (data[0]! | (data[1]! << 8) | (data[2]! << 16) | (data[3]! << 24))
      : data.length > 0 ? data[0]! : 0;
    if (msgType === this._lastDedupType && paneId === this._lastDedupPaneId &&
        data.length === this._lastDedupDataLen && head === this._lastDedupDataHead) {
      muxLog.debug(`RECV dedup: skipping duplicate type=0x${msgType.toString(16)}`);
      return;
    }
    this._lastDedupType = msgType;
    this._lastDedupPaneId = paneId;
    this._lastDedupDataLen = data.length;
    this._lastDedupDataHead = head;
    switch (msgType) {
      case MuxMessageType.Welcome: {
        const sessions = decodeWelcomeMsg(data);
        if (sessions) {
          this.handleWelcome(sessions);
        } else {
          muxLog.warn("Failed to decode Welcome message");
          this.setState("error");
          this._pendingWelcomeReject?.("Invalid Welcome message");
          this._pendingWelcomeResolve = null;
          this._pendingWelcomeReject = null;
        }
        break;
      }
      case MuxMessageType.PtyOutput:
        this.onPtyOutput?.(paneId, data);
        break;
      case MuxMessageType.PtyExited:
        muxLog.info(`PtyExited pane=${paneId}`);
        this.onPtyExited?.(paneId);
        break;
      case MuxMessageType.PaneCreated:
        muxLog.info(`PaneCreated pane=${paneId}`);
        this.onPaneCreated?.(paneId);
        break;
      case MuxMessageType.Detached:
        muxLog.info("Detached from daemon");
        this.onDetached?.();
        break;
      case MuxMessageType.StatusUpdate:
        if (this.onStatusUpdate) {
          // Decode bincode StatusUpdateMsg
          const msg = decodeStatusUpdateMsg(data);
          if (msg) {
            this.onStatusUpdate(msg);
          }
        }
        break;
      default:
        // Other message types - log and ignore
        muxLog.debug(`Mux APC: unhandled type 0x${msgType.toString(16)} pane=${paneId}`);
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

  /** Set callback for detached notification. */
  setOnDetached(callback: () => void): void {
    this.onDetached = callback;
  }

  /** Set callback for status updates. */
  setOnStatusUpdate(callback: (msg: { session_name: string; window_names: string[]; active_window_index: number }) => void): void {
    this.onStatusUpdate = callback;
  }

  /** Disconnect (no-op for APC-based client since bridge handles lifecycle). */
  async disconnect(): Promise<void> {
    this.ptyClient = null;
    this.setState("disconnected");
  }

  /** Send PTY input to a pane via APC.
   *  Uses writeDirect to bypass the writeProxy (which would recurse). */
  async sendInput(paneId: number, data: Uint8Array): Promise<void> {
    if (!this.ptyClient) {
      muxLog.error("sendInput: No PTY client");
      throw new Error("No PTY client");
    }
    const msg = encodeMuxMessage(MuxMessageType.PtyInput, paneId, data);
    await this.ptyClient.writeDirect(new TextEncoder().encode(msg));
  }

  /** Send a control message to the daemon via APC.
   *  Uses writeDirect to bypass the writeProxy. */
  async sendControl(
    msgType: number,
    paneId: number,
    payload: Uint8Array = new Uint8Array(),
  ): Promise<null> {
    if (!this.ptyClient) {
      muxLog.error(`sendControl(0x${msgType.toString(16)}): No PTY client`);
      throw new Error("No PTY client");
    }
    const msg = encodeMuxMessage(msgType, paneId, payload);
    await this.ptyClient.writeDirect(new TextEncoder().encode(msg));
    // APC-based communication is fire-and-forget; responses come via handleIncomingApc
    return null;
  }

  /** Check if connected. */
  get isConnected(): boolean {
    return this._state === "connected" && this.ptyClient !== null;
  }
}

/**
 * Decode a bincode-serialized StatusUpdateMsg.
 *
 * bincode format:
 * - session_name: String (u64 LE len + UTF-8)
 * - window_names: Vec<String> (u64 LE len, then Strings)
 * - active_window_index: u32 LE
 */
function decodeStatusUpdateMsg(data: Uint8Array): { session_name: string; window_names: string[]; active_window_index: number } | null {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let offset = 0;

  // session_name
  if (offset + 8 > data.length) return null;
  const nameLen = Number(view.getBigUint64(offset, true));
  offset += 8;
  if (offset + nameLen > data.length) return null;
  const session_name = new TextDecoder().decode(data.slice(offset, offset + nameLen));
  offset += nameLen;

  // window_names Vec length
  if (offset + 8 > data.length) return null;
  const windowNamesLen = Number(view.getBigUint64(offset, true));
  offset += 8;
  const window_names: string[] = [];
  for (let i = 0; i < windowNamesLen; i++) {
    if (offset + 8 > data.length) return null;
    const wLen = Number(view.getBigUint64(offset, true));
    offset += 8;
    if (offset + wLen > data.length) return null;
    window_names.push(new TextDecoder().decode(data.slice(offset, offset + wLen)));
    offset += wLen;
  }

  // active_window_index
  if (offset + 4 > data.length) return null;
  const active_window_index = view.getUint32(offset, true);

  return { session_name, window_names, active_window_index };
}
