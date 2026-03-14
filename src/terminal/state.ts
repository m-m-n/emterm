/**
 * Terminal state management.
 *
 * Processes terminal actions and maintains screen state.
 * Handler implementations are delegated to the handlers module.
 */

import { MarkdownSessionManager } from "../markdown/session.ts";
import type { CharSet, CsiAction, EscAction, EraseMode, TerminalAction } from "../types/terminal.ts";
import { UnifiedBuffer } from "./unified-buffer.ts";
import { CursorState } from "./cursor.ts";
import { cloneAttributes } from "./attributes.ts";
import { FoldManager } from "./fold-manager.ts";
import { Line, type Cell } from "./grid.ts";
import { createDefaultModes, setDecPrivateMode, syncModesFromWasm, syncModesToWasm, type TerminalModes } from "./modes.ts";
import { SemanticZoneTracker } from "./semantic-zone.ts";
import { isEmojiPresentation } from "./wasm/unicode.ts";
import { WasmGrid } from "./wasm/terminal-core.ts";

// Import handlers from the handlers module
import {
  handleOsc,
  handleApc,
  handleDcs,
} from "./handlers/index.ts";
import type { TerminalStateAccessor, ActiveCharSet } from "./handlers/types.ts";

// ── WASM Sentinel Constants ─────────────────────────────
const WASM_BEL_SENTINEL = 0xFE;
const WASM_SCROLLBACK_SENTINEL = 0xFF;

// ── WASM Mode Action Codes (mirror Rust constants) ──────
const MODE_ACTION_NONE = 0;
const MODE_ACTION_SWITCH_TO_ALT = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT = 2;
const MODE_ACTION_SWITCH_TO_MAIN = 3;
const MODE_ACTION_SAVE_CURSOR = 4;
const MODE_ACTION_RESTORE_CURSOR = 5;
const MODE_ACTION_TS_FALLBACK = 0xFF;

/**
 * Convert CharSet string to numeric byte for WASM ESC handler.
 */
function charSetToByte(charset: CharSet): number {
  switch (charset) {
    case "Ascii": return 0;
    case "DecLineDrawing": return 1;
    case "Uk": return 2;
    default: return 0;
  }
}

/**
 * Convert EraseMode string to numeric mode byte for WASM.
 */
function eraseModeToByte(mode: EraseMode): number {
  switch (mode) {
    case "Below": return 0;
    case "Above": return 1;
    case "All": return 2;
    case "Scrollback": return 3;
  }
}

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

  /** Pending wrap flag backing field. */
  private _wrapPending: boolean = false;

  /** Tab stops (column indices where tab stops are set, public for handler access). */
  tabStops: Set<number>;

  /** G0 character set backing field. */
  private _g0CharSet: CharSet = "Ascii";

  /** G1 character set backing field. */
  private _g1CharSet: CharSet = "Ascii";

  /** Active character set backing field. */
  private _activeCharSet: ActiveCharSet = "G0";

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

  /** Progress bar state from OSC 9;4 (0=remove, 1=normal, 2=error, 3=indeterminate, 4=warning). */
  _progressState: number = 0;

  /** Progress bar percentage (0-100, or -1 for indeterminate). */
  _progressPercentage: number = -1;

  /** User variables set via OSC 1337;SetUserVar. */
  _userVariables: Map<string, string> = new Map();

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

  /** Primary WASM grid for viewport backing. */
  private primaryWasmGrid: WasmGrid | null = null;

  /** Alternate WASM grid for alternate screen. */
  private alternateWasmGrid: WasmGrid | null = null;

  /** Cell width in pixels (for XTWINOPS responses, propagated to new cores). */
  private cellWidthPx: number = 8;

  /** Cell height in pixels (for XTWINOPS responses, propagated to new cores). */
  private cellHeightPx: number = 16;

  /**
   * Create a new terminal state.
   *
   * @param cols - Number of columns
   * @param rows - Number of rows
   * @param scrollbackLines - Maximum number of lines to keep in scrollback (default: 10000)
   * @param useWasm - Whether to use WASM-backed viewport (default: true)
   */
  constructor(cols: number, rows: number, scrollbackLines: number = 10000, useWasm: boolean = true) {
    this.maxScrollbackLines = scrollbackLines;

    // Create WASM grid for viewport (if WASM is available)
    if (useWasm) {
      try {
        this.primaryWasmGrid = new WasmGrid(cols, rows, scrollbackLines);
      } catch {
        // WASM not available - fall back to JS-only mode
        this.primaryWasmGrid = null;
      }
    }

    // Create primary buffer with unified scrollback
    this.primaryBuffer = new UnifiedBuffer(cols, rows, scrollbackLines, this.primaryWasmGrid ?? undefined);

    this.primaryCursor = new CursorState(cols, rows, this.primaryWasmGrid?.core);
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

  /** Wrap pending with WASM auto-sync. */
  get wrapPending(): boolean { return this._wrapPending; }
  set wrapPending(value: boolean) {
    this._wrapPending = value;
    this.getActiveWasmGrid()?.core.set_wrap_pending(value);
  }

  /** G0 character set with WASM auto-sync. */
  get g0CharSet(): CharSet { return this._g0CharSet; }
  set g0CharSet(value: CharSet) {
    this._g0CharSet = value;
    this.getActiveWasmGrid()?.core.set_g0_charset(value === "DecLineDrawing" ? 1 : 0);
  }

  /** G1 character set with WASM auto-sync. */
  get g1CharSet(): CharSet { return this._g1CharSet; }
  set g1CharSet(value: CharSet) {
    this._g1CharSet = value;
    this.getActiveWasmGrid()?.core.set_g1_charset(value === "DecLineDrawing" ? 1 : 0);
  }

  /** Active character set with WASM auto-sync. */
  get activeCharSet(): ActiveCharSet { return this._activeCharSet; }
  set activeCharSet(value: ActiveCharSet) {
    this._activeCharSet = value;
    this.getActiveWasmGrid()?.core.set_active_charset(value === "G1" ? 1 : 0);
  }

  /** Get the active WASM grid (primary or alternate). */
  private getActiveWasmGrid(): WasmGrid | null {
    return this.useAlternate ? this.alternateWasmGrid : this.primaryWasmGrid;
  }

  /**
   * Get the primary WASM TerminalCore for process_pty_data().
   * The parser lives in the primary core and is used for all data processing.
   */
  getWasmCore(): import("../../wasm/pkg/emterm_wasm.js").TerminalCore {
    if (!this.primaryWasmGrid) throw new Error("WASM not initialized");
    return this.primaryWasmGrid.core;
  }

  /**
   * Get the active WASM TerminalCore (primary or alternate).
   * Used by setupPtyHandlers to route data to the correct core after buffer switch.
   */
  getActiveCore(): import("../../wasm/pkg/emterm_wasm.js").TerminalCore {
    if (this.useAlternate && this.alternateWasmGrid) {
      return this.alternateWasmGrid.core;
    }
    if (!this.primaryWasmGrid) throw new Error("WASM not initialized");
    return this.primaryWasmGrid.core;
  }

  /**
   * Handle a mode action code returned by WASM take_mode_actions().
   * Standard actions (1-5) map to buffer switch and cursor save/restore.
   */
  handleModeAction(actionCode: number): void {
    switch (actionCode) {
      case MODE_ACTION_SWITCH_TO_ALT:
        this.switchToAlternateBuffer(false);
        break;
      case MODE_ACTION_SAVE_AND_SWITCH_TO_ALT:
        this.switchToAlternateBuffer(true);
        break;
      case MODE_ACTION_SWITCH_TO_MAIN:
        this.switchToPrimaryBuffer(true);
        break;
      case MODE_ACTION_SAVE_CURSOR:
        this.cursor.save();
        break;
      case MODE_ACTION_RESTORE_CURSOR:
        this.cursor.restore();
        break;
    }
  }

  /**
   * Set a DEC private mode from TS-side (for TS_FALLBACK modes).
   * Used by the setupPtyHandlers mode action decoder.
   */
  setDecPrivateMode(mode: number, enable: boolean): void {
    setDecPrivateMode(this.modes, mode, enable);
    // Sync to WASM if available
    const grid = this.getActiveWasmGrid();
    if (grid) {
      syncModesToWasm(this.modes, grid.core);
    }
  }

  /**
   * Dispose WASM resources and callbacks.
   */
  dispose(): void {
    // Clear WASM callbacks
    const core = this.primaryWasmGrid?.core;
    if (core) {
      core.clear_callbacks();
    }

    // Free WASM grids
    this.primaryWasmGrid?.dispose();
    this.primaryWasmGrid = null;

    if (this.alternateWasmGrid) {
      this.alternateWasmGrid.dispose();
      this.alternateWasmGrid = null;
    }

    // Dispose markdown manager
    this.markdownManager.dispose();
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
   * Get a single scrollback line by index.
   * Returns a direct reference for performance (no clone).
   * Used by the renderer for index-based scrollback access.
   *
   * @param index - Scrollback index (0 = oldest)
   * @returns Line at that scrollback position
   */
  getScrollbackLine(index: number): Line {
    return this.primaryBuffer.getScrollbackLine(index);
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
   * Get packed binary data for a viewport row.
   * Returns null if WASM is not available.
   */
  getRowPacked(row: number): Uint8Array | null {
    return this.getActiveBuffer().getRowPacked(row);
  }

  /**
   * Get packed binary data for a scrollback row.
   * Returns null if WASM is not available.
   */
  getScrollbackRowPacked(index: number): Uint8Array | null {
    return this.getActiveBuffer().getScrollbackRowPacked(index);
  }

  /**
   * Sync boolean modes to WASM bitfield.
   * No-op when WASM is not active.
   */
  syncModesToWasm(): void {
    const grid = this.useAlternate ? this.alternateWasmGrid : this.primaryWasmGrid;
    if (grid) {
      syncModesToWasm(this.modes, grid.core);
    }
  }

  /**
   * Sync boolean modes from WASM bitfield to TS TerminalModes.
   * Call after process_pty_data() to pick up mode changes made inside WASM
   * (e.g. DECTCEM cursor visibility, ATT160 cursor blink).
   * No-op when WASM is not active.
   */
  syncModesFromWasm(): void {
    const grid = this.useAlternate ? this.alternateWasmGrid : this.primaryWasmGrid;
    if (grid) {
      syncModesFromWasm(this.modes, grid.core);
    }
  }

  /**
   * Set cell size in pixels and propagate to active WASM core.
   * Stored locally so alternate buffer cores receive the correct size.
   */
  setCellSizePx(width: number, height: number): void {
    this.cellWidthPx = width;
    this.cellHeightPx = height;
    this.getActiveWasmGrid()?.core.set_cell_size_px(width, height);
  }

  /**
   * Sync a tab stop addition to WASM core.
   * No-op when WASM is not active.
   */
  syncTabStopToWasm(col: number): void {
    this.getActiveWasmGrid()?.core.set_tab_stop(col);
  }

  /**
   * Sync a tab stop removal to WASM core.
   * No-op when WASM is not active.
   */
  syncClearTabStopToWasm(col: number): void {
    this.getActiveWasmGrid()?.core.clear_tab_stop(col);
  }

  /**
   * Sync clearing all tab stops to WASM core.
   * No-op when WASM is not active.
   */
  syncClearAllTabStopsToWasm(): void {
    this.getActiveWasmGrid()?.core.clear_all_tab_stops();
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
      // Create WASM grid for alternate buffer if primary uses WASM
      if (this.primaryWasmGrid) {
        try {
          this.alternateWasmGrid = new WasmGrid(this.cols, this.rows);
        } catch {
          this.alternateWasmGrid = null;
        }
      }
      this.alternateBuffer = new UnifiedBuffer(this.cols, this.rows, 0, this.alternateWasmGrid ?? undefined);
      this.alternateCursor = new CursorState(this.cols, this.rows, this.alternateWasmGrid?.core);
    } else {
      // Clear alternate buffer on switch
      this.alternateBuffer.clearAll();
      if (this.alternateWasmGrid) {
        this.alternateWasmGrid.reset();
      }
      // Reset alternate cursor to home position
      if (!this.alternateCursor) {
        this.alternateCursor = new CursorState(this.cols, this.rows, this.alternateWasmGrid?.core);
      } else {
        this.alternateCursor.moveTo(0, 0);
      }
    }

    // Propagate cell size to alternate core
    if (this.alternateWasmGrid) {
      this.alternateWasmGrid.core.set_cell_size_px(
        this.cellWidthPx,
        this.cellHeightPx,
      );
    }

    // Switch to alternate buffer
    this.useAlternate = true;
    this.cursor = this.alternateCursor!;
    this.wrapPending = false;

    // Mark all lines as dirty to force redraw
    // Use markDirty() to propagate to WASM dirty bitset (not just local field)
    for (let row = 0; row < this.rows; row++) {
      this.alternateBuffer.getLine(row).markDirty();
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
    // Use markDirty() to propagate to WASM dirty bitset (not just local field)
    const buffer = this.getActiveBuffer();
    for (let row = 0; row < this.rows; row++) {
      buffer.getLine(row).markDirty();
    }
  }

  /**
   * Flush the grapheme cluster buffer.
   *
   * Converts buffered codepoints to a cell string and places it on the grid.
   * Called when a non-extending codepoint arrives or on non-Print actions.
   */
  flushGraphemeBuffer(): void {
    // WASM path: delegate to WASM core
    const wasmGrid = this.getActiveWasmGrid();
    if (wasmGrid) {
      if (wasmGrid.core.get_grapheme_buffer_len() === 0) return;
      const scrollCount = wasmGrid.core.flush_grapheme_buffer();
      if (scrollCount > 0) {
        const buffer = this.getActiveBuffer();
        for (let i = 0; i < scrollCount; i++) {
          buffer.scrollUp();
        }
      }
      return;
    }

    // JS fallback path
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
    if (action.type !== "Print") {
      const grid = this.getActiveWasmGrid();
      const hasBufferedContent = grid
        ? grid.core.get_grapheme_buffer_len() > 0
        : this.graphemeBuffer.length > 0;
      if (hasBufferedContent) {
        this.flushGraphemeBuffer();
      }
    }

    switch (action.type) {
      case "Print": {
        const grid = this.getActiveWasmGrid();
        const cp = action.value.codePointAt(0);
        if (grid && cp !== undefined) {
          const scrollCount = grid.core.handle_print(cp);
          if (scrollCount > 0) {
            const buffer = this.getActiveBuffer();
            for (let i = 0; i < scrollCount; i++) {
              buffer.scrollUp();
            }
          }
        }
        break;
      }
      case "Execute": {
        const grid = this.getActiveWasmGrid();
        if (grid) {
          const result = grid.core.handle_execute(action.value);
          if (result === WASM_BEL_SENTINEL) {
            this.onBell?.();
          } else if (result > 0) {
            const buffer = this.getActiveBuffer();
            for (let i = 0; i < result; i++) {
              buffer.scrollUp();
            }
          }
        }
        break;
      }
      case "Csi": {
        const grid = this.getActiveWasmGrid();
        if (grid) {
          this.handleCsiWasm(grid, action.value);
        }
        break;
      }
      case "Esc": {
        const grid = this.getActiveWasmGrid();
        if (grid) {
          this.handleEscWasm(grid, action.value);
        }
        break;
      }
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
   * Get scroll event direction from WASM (1=Up, 0=none).
   */
  getScrollEventDirection(): number {
    return this.getActiveWasmGrid()?.getScrollEventDirection() ?? 0;
  }

  /**
   * Get scroll event count from WASM (0 if no event).
   */
  getScrollEventCount(): number {
    return this.getActiveWasmGrid()?.getScrollEventCount() ?? 0;
  }

  /**
   * Clear the pending scroll event in WASM.
   */
  clearScrollEvent(): void {
    this.getActiveWasmGrid()?.clearScrollEvent();
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
   * Try to handle a CSI action via WASM.
   * Returns true if handled, false if TS fallback needed.
   */
  private handleCsiWasm(grid: WasmGrid, action: CsiAction): boolean {
    switch (action.action) {
      case "CursorUp":
        grid.core.handle_cursor_up(action.data || 1);
        return true;
      case "CursorDown":
        grid.core.handle_cursor_down(action.data || 1);
        return true;
      case "CursorForward":
        grid.core.handle_cursor_forward(action.data || 1);
        return true;
      case "CursorBack":
        grid.core.handle_cursor_back(action.data || 1);
        return true;
      case "CursorNextLine":
        grid.core.handle_cursor_next_line(action.data || 1);
        return true;
      case "CursorPreviousLine":
        grid.core.handle_cursor_previous_line(action.data || 1);
        return true;
      case "CursorHorizontalAbsolute":
        grid.core.handle_cursor_horizontal_absolute(action.data || 1);
        return true;
      case "CursorPosition":
        grid.core.handle_cursor_position(
          action.data.row || 1,
          action.data.col || 1
        );
        return true;
      case "CursorVerticalAbsolute":
        grid.core.handle_cursor_vertical_absolute(action.data || 1);
        return true;
      case "EraseInDisplay": {
        const mode = eraseModeToByte(action.data);
        const result = grid.core.handle_erase_in_display(mode);
        if (result === WASM_SCROLLBACK_SENTINEL) {
          // Scrollback: call clearScrollback() directly
          // (existing TS handler has a bug calling clearAll() instead)
          const buffer = this.getActiveBuffer();
          buffer.clearScrollback();
        }
        return true;
      }
      case "EraseInLine": {
        const mode = eraseModeToByte(action.data);
        grid.core.handle_erase_in_line(mode);
        return true;
      }
      case "EraseCharacters":
        grid.core.handle_erase_characters(action.data || 1);
        return true;

      // ── Sprint 4: SGR ───────────────────────────────
      case "Sgr": {
        const params = new Uint16Array(action.data);
        grid.core.handle_sgr(params);
        return true;
      }

      // ── Sprint 4: Edit operations ───────────────────
      case "InsertLines":
        grid.core.handle_insert_lines(action.data || 1);
        return true;
      case "DeleteLines":
        grid.core.handle_delete_lines(action.data || 1);
        return true;
      case "InsertCharacters":
        grid.core.handle_insert_characters(action.data || 1);
        return true;
      case "DeleteCharacters":
        grid.core.handle_delete_characters(action.data || 1);
        return true;

      // ── Sprint 4: Scroll operations ─────────────────
      case "ScrollUp": {
        const scrollCount = grid.core.handle_scroll_up(action.data || 1);
        if (scrollCount > 0) {
          const buffer = this.getActiveBuffer();
          for (let i = 0; i < scrollCount; i++) {
            buffer.scrollUp();
          }
        }
        return true;
      }
      case "ScrollDown":
        grid.core.handle_scroll_down(action.data || 1);
        return true;
      case "SetScrollRegion": {
        grid.core.handle_decstbm(action.data.top, action.data.bottom);
        // Sync scroll region to UnifiedBuffer (WASM sets its own, buffer needs its copy)
        const top = action.data.top === 0 ? 0 : action.data.top - 1;
        const bottom = action.data.bottom === 0 ? this.rows - 1 : action.data.bottom - 1;
        this.getActiveBuffer().setScrollRegion(top, bottom);
        return true;
      }

      // ── Sprint 4: Mode handling ─────────────────────
      case "SetMode":
        return this.handleModesWasm(grid, action.data, true);
      case "ResetMode":
        return this.handleModesWasm(grid, action.data, false);

      // ── Sprint 4: Device responses ──────────────────
      case "DeviceStatusReport": {
        const len = grid.core.handle_device_status_report(action.data);
        if (len > 0) {
          this.readAndSendResponse(grid);
        }
        return true;
      }
      case "PrimaryDeviceAttributes": {
        const len = grid.core.handle_primary_device_attributes();
        if (len > 0) {
          this.readAndSendResponse(grid);
        }
        return true;
      }
      case "SecondaryDeviceAttributes": {
        const len = grid.core.handle_secondary_device_attributes();
        if (len > 0) {
          this.readAndSendResponse(grid);
        }
        return true;
      }
      case "TertiaryDeviceAttributes":
        return false; // No WASM handler, fallback to TS

      default:
        return false;
    }
  }

  /**
   * Handle an ESC action via WASM.
   * Maps ESC action names to WASM action codes and dispatches.
   */
  private handleEscWasm(grid: WasmGrid, action: EscAction): void {
    let actionCode: number;
    let data = 0;

    switch (action.action) {
      case "SaveCursor":
        actionCode = 0;
        break;
      case "RestoreCursor":
        actionCode = 1;
        break;
      case "Index":
        actionCode = 2;
        break;
      case "NextLine":
        actionCode = 3;
        break;
      case "ReverseIndex":
        actionCode = 4;
        break;
      case "HorizontalTabSet":
        actionCode = 5;
        break;
      case "ResetToInitialState":
        actionCode = 6;
        break;
      case "SetG0CharSet":
        actionCode = 7;
        data = charSetToByte(action.data);
        break;
      case "SetG1CharSet":
        actionCode = 8;
        data = charSetToByte(action.data);
        break;
      case "Unknown":
        return; // No-op for unknown ESC sequences
    }

    grid.core.handle_esc(actionCode, data);

    // RIS: also reset TS-side state
    if (actionCode === 6) {
      this.reset();
    }
  }

  /**
   * Process SetMode/ResetMode via WASM with action code dispatch.
   * Handles boolean modes in WASM, falls back to TS for multi-valued modes.
   */
  private handleModesWasm(grid: WasmGrid, modes: number[], enable: boolean): boolean {
    const actions: number[] = [];

    for (const mode of modes) {
      const code = grid.core.handle_set_mode(mode, enable);
      if (code === MODE_ACTION_TS_FALLBACK) {
        // Multi-valued mode (mouse, cursor keys, etc.) - handle in TS
        setDecPrivateMode(this.modes, mode, enable);
      } else if (code !== MODE_ACTION_NONE) {
        actions.push(code);
      }
    }

    // Execute collected actions after all mode state is updated
    for (const code of actions) {
      this.executeModAction(code);
    }

    // Sync boolean modes from WASM to TS
    syncModesFromWasm(this.modes, grid.core);

    // Sync TS-only multi-valued modes back to WASM
    syncModesToWasm(this.modes, grid.core);

    return true;
  }

  /**
   * Execute a mode action code from WASM.
   */
  private executeModAction(code: number): void {
    switch (code) {
      case MODE_ACTION_SWITCH_TO_ALT:
        this.switchToAlternateBuffer(false);
        break;
      case MODE_ACTION_SAVE_AND_SWITCH_TO_ALT:
        this.switchToAlternateBuffer(true);
        break;
      case MODE_ACTION_SWITCH_TO_MAIN:
        this.switchToPrimaryBuffer(true);
        break;
      case MODE_ACTION_SAVE_CURSOR:
        this.cursor.save();
        break;
      case MODE_ACTION_RESTORE_CURSOR:
        this.cursor.restore();
        break;
    }
  }

  /**
   * Read device response from WASM and add to pending responses.
   */
  private readAndSendResponse(grid: WasmGrid): void {
    const bytes = grid.core.get_response_bytes();
    if (bytes.length > 0) {
      this.addPendingResponse(bytes);
    }
  }

  /**
   * Reset terminal to initial state.
   */
  reset(): void {
    const cols = this.cols;
    const rows = this.rows;

    // Reset WASM grids
    if (this.primaryWasmGrid) {
      this.primaryWasmGrid.reset();
    }
    if (this.alternateWasmGrid) {
      this.alternateWasmGrid.dispose();
      this.alternateWasmGrid = null;
    }

    // Reset buffers (recreate primary buffer with unified scrollback)
    this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines, this.primaryWasmGrid ?? undefined);
    this.primaryBuffer.onEvict = (count: number) => {
      this.semanticZoneTracker.pruneBeforeLine(count);
      this.foldManager.pruneBeforeLine(count);
    };
    this.alternateBuffer = null;
    this.useAlternate = false;

    // Reset cursors
    this.primaryCursor = new CursorState(cols, rows, this.primaryWasmGrid?.core);
    this.alternateCursor = null;
    this.cursor = this.primaryCursor;
    this.savedCursorForAlt = null;

    // Reset modes
    this.modes = createDefaultModes();

    // Sync default modes to WASM
    if (this.primaryWasmGrid) {
      syncModesToWasm(this.modes, this.primaryWasmGrid.core);
    }

    // Reset other state
    this.wrapPending = false;
    this.tabStops = this.createDefaultTabStops(cols);
    this.g0CharSet = "Ascii";
    this.g1CharSet = "Ascii";
    this.activeCharSet = "G0";

    // Reset grapheme buffer
    this.graphemeBuffer = [];
    // WASM grapheme buffer already cleared by primaryWasmGrid.reset() above

    // Reset OSC state
    this._title = "";
    this._iconName = "";
    this._workingDirectory = "";
    this._pendingResponses = [];
    this._activeHyperlink = null;
    this._progressState = 0;
    this._progressPercentage = -1;
    this._userVariables.clear();

    // Reset markdown state
    this.markdownManager.dispose();
    this.markdownManager = new MarkdownSessionManager();

    // Reset semantic zone tracker
    this.semanticZoneTracker.clear();

    // Reset fold manager (keep enabled state)
    this.foldManager.unfoldAll();
  }

  /**
   * Recreate WASM core after a crash (e.g., memory access out of bounds).
   * This creates a fresh TerminalCore, losing current terminal content
   * but recovering the ability to process new data.
   * Returns true if recovery was successful.
   */
  recreateWasmCore(): boolean {
    const cols = this.cols;
    const rows = this.rows;

    try {
      // Dispose broken grids
      try { this.primaryWasmGrid?.dispose(); } catch { /* already broken */ }
      try { this.alternateWasmGrid?.dispose(); } catch { /* already broken */ }
      this.alternateWasmGrid = null;

      // Create fresh WASM grid
      this.primaryWasmGrid = new WasmGrid(cols, rows, this.maxScrollbackLines);

      // Rebuild primary buffer with new grid
      this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines, this.primaryWasmGrid);
      this.primaryBuffer.onEvict = (count: number) => {
        this.semanticZoneTracker.pruneBeforeLine(count);
        this.foldManager.pruneBeforeLine(count);
      };

      // Reset cursors
      this.primaryCursor = new CursorState(cols, rows, this.primaryWasmGrid.core);
      this.alternateCursor = null;
      this.cursor = this.primaryCursor;
      this.savedCursorForAlt = null;

      // Reset alternate buffer state
      this.alternateBuffer = null;
      this.useAlternate = false;

      // Reset modes and sync to WASM
      this.modes = createDefaultModes();
      syncModesToWasm(this.modes, this.primaryWasmGrid.core);

      // Propagate cell size
      this.primaryWasmGrid.core.set_cell_size_px(this.cellWidthPx, this.cellHeightPx);

      // Reset other state (matching reset() coverage)
      this.wrapPending = false;
      this.tabStops = this.createDefaultTabStops(cols);
      this.graphemeBuffer = [];
      this._g0CharSet = "Ascii";
      this._g1CharSet = "Ascii";
      this._activeCharSet = "G0";
      this._title = "";
      this._iconName = "";
      this._workingDirectory = "";
      this._pendingResponses = [];
      this._activeHyperlink = null;
      this._progressState = 0;
      this._progressPercentage = -1;
      this._userVariables.clear();
      this.markdownManager.dispose();
      this.markdownManager = new MarkdownSessionManager();
      this.semanticZoneTracker.clear();
      this.foldManager.unfoldAll();

      console.warn("[WARN][FRONTEND] WASM core recreated after crash — terminal content lost");
      return true;
    } catch (e) {
      console.error("[ERROR][FRONTEND] Failed to recreate WASM core:", e);
      this.primaryWasmGrid = null;
      return false;
    }
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
