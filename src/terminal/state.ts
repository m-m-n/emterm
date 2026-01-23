/**
 * Terminal state management.
 *
 * Processes terminal actions and maintains screen state.
 * Handler implementations are delegated to the handlers module.
 */

import { MarkdownSessionManager } from "../markdown/session.ts";
import type { CharSet, TerminalAction } from "../types/terminal.ts";
import { ScreenBuffer } from "./buffer.ts";
import { CursorState } from "./cursor.ts";
import { createDefaultModes, type TerminalModes } from "./modes.ts";

// Import handlers from the handlers module
import {
  handlePrint,
  handleExecute,
  handleCsi,
  handleEsc,
  handleOsc,
  handleApc,
  handleDcs,
} from "./handlers/index.ts";
import type { TerminalStateAccessor, ActiveCharSet } from "./handlers/types.ts";

/**
 * Terminal state manager.
 *
 * Receives parsed terminal actions and updates the screen buffer.
 * Implements TerminalStateAccessor for handler access.
 */
export class TerminalState implements TerminalStateAccessor {
  /** Primary screen buffer. */
  private primaryBuffer: ScreenBuffer;

  /** Alternate screen buffer. */
  private alternateBuffer: ScreenBuffer | null = null;

  /** Whether alternate buffer is active. */
  private useAlternate: boolean = false;

  /** Cursor state for primary buffer. */
  private primaryCursor: CursorState;

  /** Cursor state for alternate buffer (saved when switching). */
  private alternateCursor: CursorState | null = null;

  /** Current cursor (points to active buffer's cursor). */
  cursor: CursorState;

  /** Terminal modes (public for handler access). */
  modes: TerminalModes;

  /** Pending wrap flag - next character will wrap (public for handler access). */
  wrapPending: boolean = false;

  /** Tab stops (column indices where tab stops are set, public for handler access). */
  tabStops: Set<number>;

  /** G0 character set (public for handler access). */
  g0CharSet: CharSet = "Ascii";

  /** G1 character set (public for handler access). */
  g1CharSet: CharSet = "Ascii";

  /** Active character set (G0 or G1, public for handler access). */
  activeCharSet: ActiveCharSet = "G0";

  /** Saved cursor for alternate buffer switch (1049). */
  private savedCursorForAlt: CursorState | null = null;

  /** Window title (public for handler access). */
  _title: string = "";

  /** Window icon name (public for handler access). */
  _iconName: string = "";

  /** Current working directory (from OSC 7, public for handler access). */
  _workingDirectory: string = "";

  /** Pending response bytes to write back to PTY (buffered to handle multiple DSRs). */
  private _pendingResponses: Uint8Array[] = [];

  /** Active hyperlink (from OSC 8, public for handler access). */
  _activeHyperlink: { params: string; uri: string } | null = null;

  /** Markdown session manager. */
  private markdownManager: MarkdownSessionManager;

  /**
   * Create a new terminal state.
   *
   * @param cols - Number of columns
   * @param rows - Number of rows
   */
  constructor(cols: number, rows: number) {
    this.primaryBuffer = new ScreenBuffer(cols, rows);
    this.primaryCursor = new CursorState(cols, rows);
    this.cursor = this.primaryCursor;
    this.modes = createDefaultModes();
    this.tabStops = this.createDefaultTabStops(cols);
    this.markdownManager = new MarkdownSessionManager();
  }

  /**
   * Create default tab stops (every 8 columns).
   */
  private createDefaultTabStops(cols: number): Set<number> {
    const stops = new Set<number>();
    for (let i = 8; i < cols; i += 8) {
      stops.add(i);
    }
    return stops;
  }

  /** Get number of columns. */
  get cols(): number {
    return this.cursor.cols;
  }

  /** Get number of rows. */
  get rows(): number {
    return this.cursor.rows;
  }

  /** Get current cursor column. */
  get cursorCol(): number {
    return this.cursor.col;
  }

  /** Get current cursor row. */
  get cursorRow(): number {
    return this.cursor.row;
  }

  /** Get cursor visibility. */
  get cursorVisible(): boolean {
    return this.modes.cursorVisible;
  }

  /** Get cursor blink mode. */
  get cursorBlink(): boolean {
    return this.modes.cursorBlink;
  }

  /** Get cursor style. */
  get cursorStyle(): "block" | "underline" | "bar" {
    return this.cursor.style;
  }

  /** Get terminal modes. */
  getModes(): Readonly<TerminalModes> {
    return this.modes;
  }

  /** Check if using alternate buffer. */
  get isAlternateBuffer(): boolean {
    return this.useAlternate;
  }

  /** Get window title. */
  get title(): string {
    return this._title;
  }

  /** Get icon name. */
  get iconName(): string {
    return this._iconName;
  }

  /** Get working directory. */
  get workingDirectory(): string {
    return this._workingDirectory;
  }

  /** Get active hyperlink. */
  get activeHyperlink(): { params: string; uri: string } | null {
    return this._activeHyperlink;
  }

  /**
   * Get and clear pending response bytes.
   * Call this after processing actions to get data that should be written back to PTY.
   *
   * Returns all buffered responses concatenated together to handle multiple DSRs.
   */
  takePendingResponse(): Uint8Array | null {
    if (this._pendingResponses.length === 0) {
      return null;
    }

    // Concatenate all pending responses
    const totalLength = this._pendingResponses.reduce(
      (sum, r) => sum + r.length,
      0,
    );
    const combined = new Uint8Array(totalLength);
    let offset = 0;
    for (const response of this._pendingResponses) {
      combined.set(response, offset);
      offset += response.length;
    }

    // Clear the buffer
    this._pendingResponses = [];
    return combined;
  }

  /**
   * Add a response to the pending response buffer.
   * Used by handlers to queue responses for PTY write-back.
   *
   * @param response - Response bytes to add
   */
  addPendingResponse(response: Uint8Array): void {
    this._pendingResponses.push(response);
  }

  /**
   * Get the active screen buffer.
   */
  getActiveBuffer(): ScreenBuffer {
    return this.useAlternate && this.alternateBuffer
      ? this.alternateBuffer
      : this.primaryBuffer;
  }

  /**
   * Switch to alternate screen buffer.
   *
   * @param saveCursor - Whether to save cursor before switching
   *
   * Ensures consistent state:
   * - Cursor is saved before switching if requested
   * - Alternate buffer is cleared on each switch
   * - Cursor is reset to home position (0, 0)
   */
  switchToAlternateBuffer(saveCursor: boolean = false): void {
    if (this.useAlternate) return;

    if (saveCursor) {
      // Save primary cursor for 1049 mode
      this.savedCursorForAlt = this.primaryCursor.clone();
    }

    // Create or reset alternate buffer
    if (!this.alternateBuffer) {
      this.alternateBuffer = new ScreenBuffer(this.cols, this.rows);
      this.alternateCursor = new CursorState(this.cols, this.rows);
    } else {
      // Clear alternate buffer on switch
      this.alternateBuffer.clearAll();
      // Reset alternate cursor to home position
      if (!this.alternateCursor) {
        this.alternateCursor = new CursorState(this.cols, this.rows);
      } else {
        this.alternateCursor.moveTo(0, 0);
      }
    }

    // Switch to alternate buffer
    this.useAlternate = true;
    this.cursor = this.alternateCursor!;
    this.wrapPending = false;

    // Mark all lines as dirty to force redraw
    for (let row = 0; row < this.rows; row++) {
      this.alternateBuffer.getLine(row).dirty = true;
    }
  }

  /**
   * Switch to primary screen buffer.
   *
   * @param restoreCursor - Whether to restore cursor after switching
   *
   * Ensures consistent state:
   * - Cursor is restored if requested (mode 1049)
   * - All lines marked dirty for redraw
   * - Wrap state is cleared
   */
  switchToPrimaryBuffer(restoreCursor: boolean = false): void {
    if (!this.useAlternate) return;

    // Switch to primary buffer
    this.useAlternate = false;
    this.cursor = this.primaryCursor;

    // Restore cursor if requested (for mode 1049)
    if (restoreCursor && this.savedCursorForAlt) {
      this.primaryCursor.restoreFrom(this.savedCursorForAlt);
      this.savedCursorForAlt = null;
    }

    this.wrapPending = false;

    // Mark all lines as dirty to force redraw
    const buffer = this.getActiveBuffer();
    for (let row = 0; row < this.rows; row++) {
      buffer.getLine(row).dirty = true;
    }
  }

  /**
   * Process a terminal action.
   *
   * Delegates to external handlers in the handlers module.
   *
   * @param action - The action to process
   */
  processAction(action: TerminalAction): void {
    switch (action.type) {
      case "Print":
        handlePrint(this, action.value);
        break;
      case "Execute":
        handleExecute(this, action.value);
        break;
      case "Csi":
        handleCsi(this, action.value);
        break;
      case "Esc":
        handleEsc(this, action.value);
        break;
      case "Osc":
        handleOsc(this, action.value);
        break;
      case "Apc":
        handleApc(this, action.value);
        break;
      case "Dcs":
        handleDcs(this, action.value);
        break;
    }
  }

  /**
   * Get indices of dirty rows.
   */
  getDirtyRows(): number[] {
    return this.getActiveBuffer().getDirtyRows();
  }

  /**
   * Clear all dirty flags.
   */
  clearDirty(): void {
    this.getActiveBuffer().clearAllDirty();
  }

  /**
   * Resize the terminal.
   *
   * @param cols - New number of columns
   * @param rows - New number of rows
   */
  resize(cols: number, rows: number): void {
    this.primaryBuffer.resize(cols, rows);
    if (this.alternateBuffer) {
      this.alternateBuffer.resize(cols, rows);
    }
    this.primaryCursor.resize(cols, rows);
    if (this.alternateCursor) {
      this.alternateCursor.resize(cols, rows);
    }
    this.tabStops = this.createDefaultTabStops(cols);
    this.wrapPending = false;
  }

  /**
   * Get the markdown session manager.
   *
   * @returns The markdown session manager instance
   */
  getMarkdownManager(): MarkdownSessionManager {
    return this.markdownManager;
  }

  /**
   * Reset terminal to initial state.
   */
  reset(): void {
    const cols = this.cols;
    const rows = this.rows;

    // Reset buffers
    this.primaryBuffer = new ScreenBuffer(cols, rows);
    this.alternateBuffer = null;
    this.useAlternate = false;

    // Reset cursors
    this.primaryCursor = new CursorState(cols, rows);
    this.alternateCursor = null;
    this.cursor = this.primaryCursor;
    this.savedCursorForAlt = null;

    // Reset modes
    this.modes = createDefaultModes();

    // Reset other state
    this.wrapPending = false;
    this.tabStops = this.createDefaultTabStops(cols);
    this.g0CharSet = "Ascii";
    this.g1CharSet = "Ascii";
    this.activeCharSet = "G0";

    // Reset OSC state
    this._title = "";
    this._iconName = "";
    this._workingDirectory = "";
    this._pendingResponses = [];
    this._activeHyperlink = null;

    // Reset markdown state
    this.markdownManager.dispose();
    this.markdownManager = new MarkdownSessionManager();
  }

  /**
   * Extract plain text from a grid range for copy operations.
   *
   * Coordinates are automatically normalized (start comes before end).
   * Trailing spaces on each line are removed.
   * Lines are joined with '\n'.
   *
   * @param startCol - Start column (0-indexed)
   * @param startRow - Start row (0-indexed)
   * @param endCol - End column (0-indexed, inclusive)
   * @param endRow - End row (0-indexed, inclusive)
   * @returns Extracted text with newlines between rows
   *
   * @example
   * ```ts
   * // Extract "Hello" from cells (0,0) to (4,0)
   * const text = state.extractText(0, 0, 4, 0);
   * ```
   */
  extractText(
    startCol: number,
    startRow: number,
    endCol: number,
    endRow: number,
  ): string {
    // Normalize coordinates (ensure start comes before end)
    if (startRow > endRow || (startRow === endRow && startCol > endCol)) {
      [startCol, startRow, endCol, endRow] = [
        endCol,
        endRow,
        startCol,
        startRow,
      ];
    }

    const buffer = this.getActiveBuffer();
    const lines: string[] = [];

    // Extract text row by row
    for (let row = startRow; row <= endRow; row++) {
      const line = buffer.getLine(row);
      const lineLength = line.length;

      let rowStartCol: number;
      let rowEndCol: number;

      if (row === startRow && row === endRow) {
        // Single line selection
        rowStartCol = startCol;
        rowEndCol = endCol;
      } else if (row === startRow) {
        // First line of multi-line selection
        rowStartCol = startCol;
        rowEndCol = lineLength - 1;
      } else if (row === endRow) {
        // Last line of multi-line selection
        rowStartCol = 0;
        rowEndCol = endCol;
      } else {
        // Middle line of multi-line selection
        rowStartCol = 0;
        rowEndCol = lineLength - 1;
      }

      // Extract characters from this row
      let rowText = "";
      for (let col = rowStartCol; col <= rowEndCol && col < lineLength; col++) {
        const cell = line.getCell(col);
        rowText += cell.char;
      }

      // Remove trailing spaces
      rowText = rowText.replace(/\s+$/, "");

      lines.push(rowText);
    }

    // Join lines with newline
    return lines.join("\n");
  }
}
