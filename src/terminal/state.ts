/**
 * Terminal state management.
 *
 * Processes terminal actions and maintains screen state.
 * Handler implementations are delegated to the handlers module.
 */

import { MarkdownSessionManager } from "../markdown/session.ts";
import type { CharSet, TerminalAction } from "../types/terminal.ts";
import { UnifiedBuffer } from "./unified-buffer.ts";
import { CursorState } from "./cursor.ts";
import { cloneAttributes } from "./attributes.ts";
import { FoldManager } from "./fold-manager.ts";
import { Line, type Cell } from "./grid.ts";
import { createDefaultModes, type TerminalModes } from "./modes.ts";
import { SemanticZoneTracker } from "./semantic-zone.ts";
import { isEmojiPresentation } from "./unicode.ts";

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
  private primaryBuffer: UnifiedBuffer;

  /** Alternate screen buffer. */
  private alternateBuffer: UnifiedBuffer | null = null;

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

  /** Semantic zone tracker for OSC 133. */
  private semanticZoneTracker: SemanticZoneTracker;

  /** Fold manager for command output folding. */
  private foldManager: FoldManager;

  /** Grapheme cluster buffer for emoji sequences. */
  graphemeBuffer: number[] = [];

  /** Bell callback for BEL character handling. */
  onBell?: () => void;

  /** Maximum number of lines to keep in scrollback. */
  private maxScrollbackLines: number = 10000;

  /**
   * Create a new terminal state.
   *
   * @param cols - Number of columns
   * @param rows - Number of rows
   * @param scrollbackLines - Maximum number of lines to keep in scrollback (default: 10000)
   */
  constructor(cols: number, rows: number, scrollbackLines: number = 10000) {
    this.maxScrollbackLines = scrollbackLines;

    // Create primary buffer with unified scrollback
    this.primaryBuffer = new UnifiedBuffer(cols, rows, scrollbackLines);

    this.primaryCursor = new CursorState(cols, rows);
    this.cursor = this.primaryCursor;
    this.modes = createDefaultModes();
    this.tabStops = this.createDefaultTabStops(cols);
    this.markdownManager = new MarkdownSessionManager();
    this.semanticZoneTracker = new SemanticZoneTracker();
    this.foldManager = new FoldManager();

    // Set eviction callback for scrollback overflow
    this.primaryBuffer.onEvict = (count: number) => {
      this.semanticZoneTracker.pruneBeforeLine(count);
      this.foldManager.pruneBeforeLine(count);
    };
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
  getActiveBuffer(): UnifiedBuffer {
    return this.useAlternate && this.alternateBuffer
      ? this.alternateBuffer
      : this.primaryBuffer;
  }

  /**
   * Get scrollback buffer.
   *
   * Only available for primary buffer (not alternate screen).
   * Clones lines at API boundary to prevent external mutations
   * from corrupting ring buffer contents.
   *
   * @returns Array of lines in scrollback buffer
   */
  getScrollbackBuffer(): Line[] {
    const result: Line[] = [];
    const len = this.primaryBuffer.scrollbackLength;
    for (let i = 0; i < len; i++) {
      result.push(this.primaryBuffer.getScrollbackLine(i).clone());
    }
    return result;
  }

  /**
   * Get number of lines in scrollback buffer.
   *
   * @returns Number of lines currently in scrollback
   */
  getScrollbackLength(): number {
    return this.primaryBuffer.scrollbackLength;
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

    // Create or reset alternate buffer (no scrollback)
    if (!this.alternateBuffer) {
      this.alternateBuffer = new UnifiedBuffer(this.cols, this.rows, 0);
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
   * Flush the grapheme cluster buffer.
   *
   * Converts buffered codepoints to a cell string and places it on the grid.
   * Called when a non-extending codepoint arrives or on non-Print actions.
   */
  flushGraphemeBuffer(): void {
    if (this.graphemeBuffer.length === 0) return;

    const clusterString = String.fromCodePoint(...this.graphemeBuffer);
    // Determine width based on presentation properties
    const hasFE0E = this.graphemeBuffer.includes(0xfe0e);
    const hasFE0F = this.graphemeBuffer.includes(0xfe0f);
    let width: number;
    if (hasFE0E) {
      // Explicit text presentation selector → narrow
      width = 1;
    } else if (hasFE0F) {
      // Explicit emoji presentation selector → wide
      width = 2;
    } else if (this.graphemeBuffer.length === 1) {
      // Single codepoint: only Emoji_Presentation=Yes characters are wide
      width = isEmojiPresentation(this.graphemeBuffer[0]!) ? 2 : 1;
    } else {
      // Multi-codepoint cluster (ZWJ sequence, skin tone, RI pair) → wide
      width = 2;
    }

    this.graphemeBuffer = [];

    const buffer = this.getActiveBuffer();
    const { bottom } = buffer.getEffectiveScrollRegion();

    // Handle wrap pending
    if (this.wrapPending) {
      this.wrapPending = false;
      this.cursor.carriageReturn();
      if (this.cursor.lineFeed(bottom)) {
        buffer.scrollUp();
      }
      buffer.getLine(this.cursor.row).wrapped = true;
    }

    // Wide char wrap: if width 2 and at last column, wrap first
    if (width === 2 && this.cursor.col >= this.cols - 1) {
      if (this.modes.autoWrap) {
        this.cursor.carriageReturn();
        if (this.cursor.lineFeed(bottom)) {
          buffer.scrollUp();
        }
        buffer.getLine(this.cursor.row).wrapped = true;
      }
    }

    // Create cell with cluster string
    const cell: Cell = {
      char: clusterString,
      width: width,
      attrs: cloneAttributes(this.cursor.attrs),
      dirty: true,
    };
    buffer.setCell(this.cursor.col, this.cursor.row, cell);

    // For wide characters, set placeholder in next cell
    if (width === 2 && this.cursor.col < this.cols - 1) {
      const placeholder: Cell = {
        char: "",
        width: 0,
        attrs: cloneAttributes(this.cursor.attrs),
        dirty: true,
      };
      buffer.setCell(this.cursor.col + 1, this.cursor.row, placeholder);
    }

    // Advance cursor
    const newCol = this.cursor.col + width;
    if (newCol >= this.cols) {
      if (this.modes.autoWrap) {
        this.cursor.col = this.cols - 1;
        this.wrapPending = true;
      }
    } else {
      this.cursor.col = newCol;
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
    // Flush grapheme buffer before non-Print actions
    if (action.type !== "Print" && this.graphemeBuffer.length > 0) {
      this.flushGraphemeBuffer();
    }

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
    // Primary buffer: full reflow with cursor tracking
    if (!this.useAlternate) {
      const adjusted = this.primaryBuffer.resize(
        cols, rows,
        this.primaryCursor.row, this.primaryCursor.col
      );
      this.primaryCursor.resize(cols, rows);
      this.primaryCursor.moveTo(adjusted.col, adjusted.row);
    } else {
      // When on alternate screen, just resize primary without cursor tracking
      this.primaryBuffer.resize(cols, rows, 0, 0);
      this.primaryCursor.resize(cols, rows);
    }

    // Alternate buffer: no reflow
    if (this.alternateBuffer) {
      this.alternateBuffer.resizeNoReflow(cols, rows);
      if (this.alternateCursor) {
        this.alternateCursor.resize(cols, rows);
      }
    }

    this.tabStops = this.createDefaultTabStops(cols);
    this.wrapPending = false;
    this.graphemeBuffer = [];
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
   * Get the semantic zone tracker.
   *
   * @returns The semantic zone tracker instance
   */
  getSemanticZoneTracker(): SemanticZoneTracker {
    return this.semanticZoneTracker;
  }

  /**
   * Get the fold manager.
   *
   * @returns The fold manager instance
   */
  getFoldManager(): FoldManager {
    return this.foldManager;
  }

  /**
   * Reset terminal to initial state.
   */
  reset(): void {
    const cols = this.cols;
    const rows = this.rows;

    // Reset buffers (recreate primary buffer with unified scrollback)
    this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines);
    this.primaryBuffer.onEvict = (count: number) => {
      this.semanticZoneTracker.pruneBeforeLine(count);
      this.foldManager.pruneBeforeLine(count);
    };
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

    // Reset grapheme buffer
    this.graphemeBuffer = [];

    // Reset OSC state
    this._title = "";
    this._iconName = "";
    this._workingDirectory = "";
    this._pendingResponses = [];
    this._activeHyperlink = null;

    // Reset markdown state
    this.markdownManager.dispose();
    this.markdownManager = new MarkdownSessionManager();

    // Reset semantic zone tracker
    this.semanticZoneTracker.clear();

    // Reset fold manager (keep enabled state)
    this.foldManager.unfoldAll();
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
