/**
 * Markdown session manager.
 *
 * Manages Markdown rendering sessions for OSC 777 extension.
 * Handles begin/chunk/end lifecycle, timeout cleanup, and size limits.
 *
 * @module markdown/session
 */

import type {
  MarkdownSession,
  MarkdownBlock,
  MarkdownFormat,
  RenderMode,
} from "./types.ts";
import { MarkdownRenderer } from "./renderer.ts";

/**
 * Manages Markdown rendering sessions.
 *
 * @example
 * ```typescript
 * const manager = new MarkdownSessionManager();
 *
 * // Handle OSC 777 emterm;markdown commands
 * manager.handleCommand("emterm", ["markdown", "begin", "id=xxx", "format=gfm"]);
 * manager.handleCommand("emterm", ["markdown", "chunk", "id=xxx", "seq=0", "data=..."]);
 * const block = manager.handleCommand("emterm", ["markdown", "end", "id=xxx"]);
 *
 * // Clean up
 * manager.dispose();
 * ```
 */
export class MarkdownSessionManager {
  /** Maximum data size per session (2MB) */
  static readonly MAX_SESSION_SIZE = 2 * 1024 * 1024;

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

  /** Cleanup timer handle */
  private cleanupTimer: ReturnType<typeof setInterval> | null = null;

  /**
   * Create a new session manager.
   */
  constructor() {
    this.renderer = new MarkdownRenderer();
    this.startCleanupTimer();
  }

  /**
   * Handle an EmtermExtension OSC action for markdown.
   *
   * @param verb - The command verb from OSC 777 (should be "emterm")
   * @param params - Command parameters as strings
   *   - params[0]: command type (should be "markdown")
   *   - params[1]: markdown verb (begin, chunk, end)
   *   - params[2...]: key=value parameters
   * @returns Rendered MarkdownBlock if end verb completes successfully, null otherwise
   */
  handleCommand(verb: string, params: string[]): MarkdownBlock | null {
    // Validate emterm namespace
    if (verb !== "emterm") {
      return null;
    }

    // Validate markdown command
    if (params.length < 2 || params[0] !== "markdown") {
      return null;
    }

    const markdownVerb = params[1];
    const keyValueParams = params.slice(2);
    const parsed = this.parseParams(keyValueParams);

    switch (markdownVerb) {
      case "begin":
        return this.handleBegin(parsed);
      case "chunk":
        return this.handleChunk(parsed);
      case "end":
        return this.handleEnd(parsed);
      default:
        console.warn(`Unknown markdown verb: ${markdownVerb}`);
        return null;
    }
  }

  /**
   * Handle begin command - create new session.
   */
  private handleBegin(params: Record<string, string>): null {
    const id = params.id;
    if (!id) {
      console.warn("Markdown begin: missing id");
      return null;
    }

    if (this.sessions.size >= MarkdownSessionManager.MAX_SESSIONS) {
      console.warn("Markdown begin: max sessions reached");
      return null;
    }

    // Validate format
    let format: MarkdownFormat = "commonmark";
    if (params.format === "gfm" || params.format === "commonmark") {
      format = params.format;
    }

    // Validate render mode
    let render: RenderMode = "block";
    if (params.render === "inline" || params.render === "block") {
      render = params.render;
    }

    const session: MarkdownSession = {
      id,
      format,
      version: parseInt(params.version || "1", 10) || 1,
      render,
      chunks: new Map(),
      nextSeq: 0,
      createdAt: Date.now(),
      dataSize: 0,
    };

    this.sessions.set(id, session);
    return null;
  }

  /**
   * Handle chunk command - append data to session.
   */
  private handleChunk(params: Record<string, string>): null {
    const id = params.id;
    const seq = params.seq;
    const data = params.data;

    if (!id) {
      console.warn("Markdown chunk: missing id");
      return null;
    }

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`Markdown chunk: unknown session ${id}`);
      return null;
    }

    if (!seq) {
      console.warn("Markdown chunk: missing seq");
      return null;
    }

    const seqNum = parseInt(seq, 10);
    if (isNaN(seqNum)) {
      console.warn("Markdown chunk: invalid seq");
      return null;
    }

    if (!data) {
      console.warn("Markdown chunk: missing data");
      return null;
    }

    // Decode Base64 with UTF-8 support
    let decoded: string;
    try {
      decoded = this.decodeBase64Utf8(data);
    } catch {
      console.warn("Markdown chunk: invalid base64 or UTF-8");
      return null;
    }

    // Check size limit
    if (
      session.dataSize + decoded.length >
      MarkdownSessionManager.MAX_SESSION_SIZE
    ) {
      console.warn("Markdown chunk: session size limit exceeded");
      this.sessions.delete(id);
      return null;
    }

    session.chunks.set(seqNum, decoded);
    session.dataSize += decoded.length;

    return null;
  }

  /**
   * Handle end command - assemble chunks and render.
   */
  private handleEnd(params: Record<string, string>): MarkdownBlock | null {
    const id = params.id;

    if (!id) {
      console.warn("Markdown end: missing id");
      return null;
    }

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`Markdown end: unknown session ${id}`);
      return null;
    }

    // Assemble chunks in order
    const markdown = this.assembleChunks(session);

    // Render
    const html = this.renderer.render(markdown, session.format);

    // Cleanup session
    this.sessions.delete(id);

    return {
      id,
      html,
      startRow: 0, // To be set by caller
      rowCount: 0, // To be calculated after insertion
      visible: true,
    };
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
    const binary = atob(data);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
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
      if (now - session.createdAt > MarkdownSessionManager.SESSION_TIMEOUT) {
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
   * Dispose the session manager and clean up resources.
   */
  dispose(): void {
    if (this.cleanupTimer !== null) {
      clearInterval(this.cleanupTimer);
      this.cleanupTimer = null;
    }
    this.sessions.clear();
    this.renderer.dispose();
  }
}
