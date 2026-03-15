/**
 * Data viewer session manager.
 *
 * Manages JSON/YAML rendering sessions for OSC 777 extension.
 * Handles begin/chunk/end lifecycle, timeout cleanup, and size limits.
 *
 * @module data-viewer/session
 */

import { DataViewerFullscreen } from "./fullscreen.ts";
import { parseData } from "./parser.ts";
import { buildTree } from "./tree-builder.ts";
import type { DataFormat, DataViewerSession } from "./types.ts";

/**
 * Manages data viewer (JSON/YAML) rendering sessions.
 */
export class DataViewerSessionManager {
  /** Session timeout in milliseconds (30 seconds) */
  static readonly SESSION_TIMEOUT = 30 * 1000;

  /** Maximum concurrent sessions */
  static readonly MAX_SESSIONS = 10;

  /** Cleanup interval in milliseconds */
  private static readonly CLEANUP_INTERVAL = 5000;

  /** Active sessions indexed by ID */
  private sessions = new Map<string, DataViewerSession>();

  /** Fullscreen view instance */
  private fullscreenView: DataViewerFullscreen;

  /** Container element (overlay-root) for rendering */
  private container: HTMLElement | null = null;

  /** Cleanup timer handle */
  private cleanupTimer: ReturnType<typeof setInterval> | null = null;

  constructor() {
    this.fullscreenView = new DataViewerFullscreen();
    this.startCleanupTimer();
  }

  /**
   * Set container for fullscreen view rendering.
   */
  setContainer(container: HTMLElement): void {
    this.container = container;
  }

  /**
   * Handle an EmtermExtension OSC action for json or yaml.
   *
   * @param verb - The command verb from OSC 777 (should be "emterm")
   * @param params - Command parameters as strings
   *   - params[0]: command type ("json" or "yaml")
   *   - params[1]: verb (begin, chunk, end)
   *   - params[2...]: key=value parameters
   */
  handleCommand(verb: string, params: string[]): void {
    if (verb !== "emterm") return;
    if (params.length < 2) return;

    const format = params[0] as DataFormat;
    if (format !== "json" && format !== "yaml") return;

    const dataVerb = params[1];
    const keyValueParams = params.slice(2);
    const parsed = this.parseParams(keyValueParams);

    switch (dataVerb) {
      case "begin":
        this.handleBegin(parsed, format);
        break;
      case "chunk":
        this.handleChunk(parsed);
        break;
      case "end":
        this.handleEnd(parsed);
        break;
    }
  }

  private handleBegin(
    params: Record<string, string>,
    format: DataFormat,
  ): void {
    const id = params.id;
    if (!id) {
      console.warn("[WARN][FRONTEND] DataViewer begin: missing id");
      return;
    }
    if (this.sessions.size >= DataViewerSessionManager.MAX_SESSIONS) {
      console.warn("[WARN][FRONTEND] DataViewer begin: max sessions reached");
      return;
    }

    const session: DataViewerSession = {
      id,
      format,
      version: parseInt(params.version || "1", 10) || 1,
      chunks: new Map(),
      lastChunkAt: Date.now(),
    };

    this.sessions.set(id, session);
  }

  private handleChunk(params: Record<string, string>): void {
    const id = params.id;
    if (!id) {
      console.warn("[WARN][FRONTEND] DataViewer chunk: missing id");
      return;
    }

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`[WARN][FRONTEND] DataViewer chunk: unknown session ${id}`);
      return;
    }

    const seq = params.seq;
    if (!seq) {
      console.warn("[WARN][FRONTEND] DataViewer chunk: missing seq");
      return;
    }
    const seqNum = parseInt(seq, 10);
    if (isNaN(seqNum)) {
      console.warn("[WARN][FRONTEND] DataViewer chunk: invalid seq");
      return;
    }

    const data = params.data;
    if (!data) {
      console.warn("[WARN][FRONTEND] DataViewer chunk: missing data");
      return;
    }

    let decoded: string;
    try {
      decoded = this.decodeBase64Utf8(data);
    } catch {
      console.warn("[WARN][FRONTEND] DataViewer chunk: invalid base64 or UTF-8");
      return;
    }

    session.chunks.set(seqNum, decoded);
    session.lastChunkAt = Date.now();
  }

  private handleEnd(params: Record<string, string>): void {
    const id = params.id;
    if (!id) {
      console.warn("[WARN][FRONTEND] DataViewer end: missing id");
      return;
    }

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`[WARN][FRONTEND] DataViewer end: unknown session ${id}`);
      return;
    }

    // Assemble chunks in order
    const rawText = this.assembleChunks(session);
    const format = session.format;

    // Cleanup session
    this.sessions.delete(id);

    // Parse data
    const result = parseData(rawText, format);

    if (!this.container) {
      console.error(
        "[ERROR][FRONTEND] DataViewerSessionManager: container not set",
      );
      return;
    }

    if (result.ok) {
      const tree = buildTree(result.data);
      this.fullscreenView.show({
        format,
        rawText,
        parsedData: result.data,
        tree,
        error: null,
        container: this.container,
      });
    } else {
      this.fullscreenView.show({
        format,
        rawText,
        parsedData: null,
        tree: [],
        error: result.error,
        container: this.container,
      });
    }
  }

  private assembleChunks(session: DataViewerSession): string {
    const sortedSeqs = Array.from(session.chunks.keys()).sort(
      (a, b) => a - b,
    );
    return sortedSeqs.map((seq) => session.chunks.get(seq)!).join("");
  }

  private decodeBase64Utf8(data: string): string {
    if (!/^[A-Za-z0-9+/]*={0,2}$/.test(data)) {
      throw new Error("Invalid Base64 format");
    }
    const binary = atob(data);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  }

  private parseParams(params: string[]): Record<string, string> {
    const result: Record<string, string> = {};
    for (const param of params) {
      const eqIndex = param.indexOf("=");
      if (eqIndex > 0) {
        result[param.substring(0, eqIndex)] = param.substring(eqIndex + 1);
      }
    }
    return result;
  }

  private startCleanupTimer(): void {
    this.cleanupTimer = setInterval(() => {
      this.cleanupExpiredSessions();
    }, DataViewerSessionManager.CLEANUP_INTERVAL);
  }

  cleanupExpiredSessions(): void {
    const now = Date.now();
    for (const [id, session] of this.sessions) {
      if (
        now - session.lastChunkAt >
        DataViewerSessionManager.SESSION_TIMEOUT
      ) {
        this.sessions.delete(id);
      }
    }
  }

  getSession(id: string): DataViewerSession | undefined {
    return this.sessions.get(id);
  }

  get sessionCount(): number {
    return this.sessions.size;
  }

  getFullscreenView(): DataViewerFullscreen {
    return this.fullscreenView;
  }

  /**
   * Reset sessions without destroying the manager.
   * Preserves container reference and callbacks.
   */
  resetSessions(): void {
    this.sessions.clear();
    this.fullscreenView.close();
  }

  dispose(): void {
    if (this.cleanupTimer !== null) {
      clearInterval(this.cleanupTimer);
      this.cleanupTimer = null;
    }
    this.sessions.clear();
    this.fullscreenView.dispose();
  }
}
