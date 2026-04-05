/**
 * Terminal state management.
 *
 * Processes terminal actions and maintains screen state.
 * Handler implementations are delegated to the handlers module.
 * Buffer switching, WASM sync, action processing, and response management
 * are delegated to extracted modules.
 */

import { DataViewerSessionManager } from "../data-viewer/session.ts";
import { MarkdownSessionManager } from "../markdown/session.ts";
import type { CharSet, TerminalAction } from "../types/terminal.ts";
import { UnifiedBuffer } from "./unified-buffer.ts";
import { CursorState } from "./cursor.ts";
import { FoldManager } from "./fold-manager.ts";
import { Line } from "./grid.ts";
import { createDefaultModes, setDecPrivateMode, syncModesFromWasm, syncModesToWasm, type TerminalModes } from "./modes.ts";
import { SemanticZoneTracker } from "./semantic-zone.ts";
import { WasmGrid } from "./wasm/terminal-core.ts";
import { TerminalCore } from "../../wasm/pkg/emterm_wasm.js";
import type { TerminalStateAccessor, ActiveCharSet } from "./handlers/types.ts";

// Extracted modules
import {
  takePendingResponse as takePendingResponseFn,
  addPendingResponse as addPendingResponseFn,
} from "./state-response.ts";
import {
  switchToAlternateBuffer as switchToAltFn,
  switchToPrimaryBuffer as switchToPrimaryFn,
  type BufferSwitchContext,
} from "./state-buffer.ts";
import {
  syncModesToWasm as syncModesToWasmFn,
  syncModesFromWasm as syncModesFromWasmFn,
  setCellSizePxOnGrid,
  setCursorShowInterrupt as setCursorShowInterruptFn,
  syncTabStopToWasm as syncTabStopToWasmFn,
  syncClearTabStopToWasm as syncClearTabStopToWasmFn,
  syncClearAllTabStopsToWasm as syncClearAllTabStopsToWasmFn,
} from "./state-wasm-sync.ts";
import {
  processAction as processActionFn,
  flushGraphemeBuffer as flushGraphemeBufferFn,
  type ActionContext,
} from "./state-actions.ts";

// ── WASM Mode Action Codes (mirror Rust constants) ──────
/**
 * Snapshot of a mux pane's grid state (primary + alternate screen).
 * Used by mux window manager to save/restore pane state on window switch.
 */
export interface MuxPaneGridState {
  primaryGrid: WasmGrid;
  alternateGrid: WasmGrid | null;
  useAlternate: boolean;
}

const MODE_ACTION_SWITCH_TO_ALT = 1;
const MODE_ACTION_SAVE_AND_SWITCH_TO_ALT = 2;
const MODE_ACTION_SWITCH_TO_MAIN = 3;
const MODE_ACTION_SAVE_CURSOR = 4;
const MODE_ACTION_RESTORE_CURSOR = 5;

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

  /** Data viewer session manager (JSON/YAML). */
  private dataViewerManager: DataViewerSessionManager;

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
    this.dataViewerManager = new DataViewerSessionManager();
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
   * Save the current pane's full grid state (primary + alternate) for mux switching.
   * Returns a snapshot that can be restored later via restoreMuxPaneState.
   */
  saveMuxPaneState(): MuxPaneGridState {
    // Return references to current grids. The caller will immediately replace
    // them via swapPrimaryGrid or restoreMuxPaneState, so shared references
    // are safe (no concurrent mutation).
    return {
      primaryGrid: this.primaryWasmGrid!,
      alternateGrid: this.alternateWasmGrid,
      useAlternate: this.useAlternate,
    };
  }

  /**
   * Restore a previously saved mux pane state (primary + alternate).
   * Rebuilds buffers and cursors around the restored grids.
   */
  restoreMuxPaneState(paneState: MuxPaneGridState): void {
    // Note: we do NOT dispose existing grids here — they may be shared
    // references saved by saveMuxPaneState for another pane.

    // Restore grids
    this.primaryWasmGrid = paneState.primaryGrid;
    this.alternateWasmGrid = paneState.alternateGrid;
    this.useAlternate = paneState.useAlternate;

    // Rebuild primary buffer and cursor
    const cols = paneState.primaryGrid.core.cols();
    const rows = paneState.primaryGrid.core.rows();
    this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines, paneState.primaryGrid);
    this.primaryBuffer.onEvict = (count: number) => {
      this.semanticZoneTracker.pruneBeforeLine(count);
      this.foldManager.pruneBeforeLine(count);
    };
    this.primaryCursor = new CursorState(cols, rows, paneState.primaryGrid.core);
    this.primaryCursor.moveTo(paneState.primaryGrid.core.get_cursor_col(), paneState.primaryGrid.core.get_cursor_row());

    // Rebuild alternate buffer and cursor if alternate screen was active
    if (paneState.alternateGrid) {
      const altCols = paneState.alternateGrid.core.cols();
      const altRows = paneState.alternateGrid.core.rows();
      this.alternateBuffer = new UnifiedBuffer(altCols, altRows, 0, paneState.alternateGrid);
      this.alternateCursor = new CursorState(altCols, altRows, paneState.alternateGrid.core);
      this.alternateCursor.moveTo(paneState.alternateGrid.core.get_cursor_col(), paneState.alternateGrid.core.get_cursor_row());
    } else {
      this.alternateBuffer = null;
      this.alternateCursor = null;
    }

    // Set active cursor
    this.cursor = this.useAlternate && this.alternateCursor
      ? this.alternateCursor
      : this.primaryCursor;

    // Sync modes from the active core
    const activeCore = this.useAlternate && paneState.alternateGrid
      ? paneState.alternateGrid.core
      : paneState.primaryGrid.core;
    syncModesFromWasm(this.modes, activeCore);

    // Propagate cell size to all grids
    setCellSizePxOnGrid(paneState.primaryGrid, this.cellWidthPx, this.cellHeightPx);
    if (paneState.alternateGrid) {
      setCellSizePxOnGrid(paneState.alternateGrid, this.cellWidthPx, this.cellHeightPx);
    }

    // Mark all rows dirty for full repaint
    activeCore.mark_all_dirty();
  }

  /**
   * Swap the primary WASM grid with a fresh one (for new mux pane creation).
   * Resets alternate screen state. Returns the old primary grid.
   */
  swapPrimaryGrid(newGrid: WasmGrid): WasmGrid | null {
    const oldGrid = this.primaryWasmGrid;
    this.primaryWasmGrid = newGrid;

    // Reset alternate screen state — new pane starts fresh.
    if (this.alternateWasmGrid) {
      this.alternateWasmGrid.dispose();
      this.alternateWasmGrid = null;
    }
    this.alternateBuffer = null;
    this.alternateCursor = null;
    this.savedCursorForAlt = null;
    this.useAlternate = false;

    const cols = newGrid.core.cols();
    const rows = newGrid.core.rows();

    // Rebuild buffer and cursor around the new grid
    this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines, newGrid);
    this.primaryBuffer.onEvict = (count: number) => {
      this.semanticZoneTracker.pruneBeforeLine(count);
      this.foldManager.pruneBeforeLine(count);
    };
    this.primaryCursor = new CursorState(cols, rows, newGrid.core);
    this.primaryCursor.moveTo(newGrid.core.get_cursor_col(), newGrid.core.get_cursor_row());
    this.cursor = this.primaryCursor;

    // Sync modes from the new core
    syncModesFromWasm(this.modes, newGrid.core);

    // Propagate cell size
    setCellSizePxOnGrid(newGrid, this.cellWidthPx, this.cellHeightPx);

    // Mark all rows dirty for full repaint
    newGrid.core.mark_all_dirty();

    return oldGrid;
  }

  /**
   * Get the current primary WASM grid (for mux pane storage).
   */
  getPrimaryGrid(): WasmGrid | null {
    return this.primaryWasmGrid;
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

    // Dispose session managers
    this.dataViewerManager.dispose();
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
   * Delegates to state-response module.
   */
  takePendingResponse(): Uint8Array | null {
    const { result, remaining } = takePendingResponseFn(this._pendingResponses);
    this._pendingResponses = remaining;
    return result;
  }

  /**
   * Add a response to the pending response buffer.
   * Delegates to state-response module.
   */
  addPendingResponse(response: Uint8Array): void {
    this._pendingResponses = addPendingResponseFn(this._pendingResponses, response);
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
   */
  getScrollbackLine(index: number): Line {
    return this.primaryBuffer.getScrollbackLine(index);
  }

  /**
   * Get number of lines in scrollback buffer.
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
   * Delegates to state-wasm-sync module.
   */
  syncModesToWasm(): void {
    syncModesToWasmFn(this.modes, this.getActiveWasmGrid());
  }

  /**
   * Sync boolean modes from WASM bitfield to TS TerminalModes.
   * Delegates to state-wasm-sync module.
   */
  syncModesFromWasm(): void {
    syncModesFromWasmFn(this.modes, this.getActiveWasmGrid());
  }

  /**
   * Set cell size in pixels and propagate to active WASM core.
   * Delegates to state-wasm-sync module.
   */
  setCellSizePx(width: number, height: number): void {
    this.cellWidthPx = width;
    this.cellHeightPx = height;
    setCellSizePxOnGrid(this.getActiveWasmGrid(), width, height);
  }

  /**
   * Enable/disable cursor hidden->visible interrupt in WASM parser.
   * Delegates to state-wasm-sync module.
   */
  setCursorShowInterrupt(enable: boolean): void {
    setCursorShowInterruptFn(this.primaryWasmGrid, this.alternateWasmGrid, enable);
  }

  /**
   * Sync a tab stop addition to WASM core.
   * Delegates to state-wasm-sync module.
   */
  syncTabStopToWasm(col: number): void {
    syncTabStopToWasmFn(this.getActiveWasmGrid(), col);
  }

  /**
   * Sync a tab stop removal to WASM core.
   * Delegates to state-wasm-sync module.
   */
  syncClearTabStopToWasm(col: number): void {
    syncClearTabStopToWasmFn(this.getActiveWasmGrid(), col);
  }

  /**
   * Sync clearing all tab stops to WASM core.
   * Delegates to state-wasm-sync module.
   */
  syncClearAllTabStopsToWasm(): void {
    syncClearAllTabStopsToWasmFn(this.getActiveWasmGrid());
  }

  /**
   * Switch to alternate screen buffer.
   * Delegates to state-buffer module.
   */
  switchToAlternateBuffer(saveCursor: boolean = false): void {
    const result = switchToAltFn(this.getBufferSwitchContext(), saveCursor);
    this.applyBufferSwitchResult(result);
  }

  /**
   * Switch to primary screen buffer.
   * Delegates to state-buffer module.
   */
  switchToPrimaryBuffer(restoreCursor: boolean = false): void {
    const result = switchToPrimaryFn(this.getBufferSwitchContext(), restoreCursor);
    this.applyBufferSwitchResult(result);
  }

  /**
   * Build the context object for buffer switch operations.
   */
  private getBufferSwitchContext(): BufferSwitchContext {
    return {
      primaryBuffer: this.primaryBuffer,
      alternateBuffer: this.alternateBuffer,
      useAlternate: this.useAlternate,
      primaryCursor: this.primaryCursor,
      alternateCursor: this.alternateCursor,
      cursor: this.cursor,
      savedCursorForAlt: this.savedCursorForAlt,
      primaryWasmGrid: this.primaryWasmGrid,
      alternateWasmGrid: this.alternateWasmGrid,
      cols: this.cols,
      rows: this.rows,
      modes: this.modes,
      cellWidthPx: this.cellWidthPx,
      cellHeightPx: this.cellHeightPx,
      wrapPending: this._wrapPending,
    };
  }

  /**
   * Apply the result of a buffer switch operation to this state.
   */
  private applyBufferSwitchResult(result: import("./state-buffer.ts").BufferSwitchResult): void {
    this.alternateBuffer = result.alternateBuffer;
    this.alternateWasmGrid = result.alternateWasmGrid;
    this.alternateCursor = result.alternateCursor;
    this.useAlternate = result.useAlternate;
    this.cursor = result.cursor;
    this.savedCursorForAlt = result.savedCursorForAlt;
    this.wrapPending = result.wrapPending;
  }

  /**
   * Flush the grapheme cluster buffer.
   * Delegates to state-actions module.
   */
  flushGraphemeBuffer(): void {
    flushGraphemeBufferFn(this.getActionContext());
  }

  /**
   * Process a terminal action.
   * Delegates to state-actions module.
   */
  processAction(action: TerminalAction): void {
    processActionFn(this, this.getActionContext(), action);
  }

  /**
   * Build the action context for action processing operations.
   * Uses Object.defineProperty to proxy mutable properties back to this instance.
   */
  private getActionContext(): ActionContext {
    const self = this;
    const ctx: ActionContext = {
      getActiveWasmGrid: () => self.getActiveWasmGrid(),
      getActiveBuffer: () => self.getActiveBuffer(),
      get cursor() { return self.cursor; },
      get modes() { return self.modes; },
      get cols() { return self.cols; },
      get rows() { return self.rows; },
      get graphemeBuffer() { return self.graphemeBuffer; },
      set graphemeBuffer(v: number[]) { self.graphemeBuffer = v; },
      get wrapPending() { return self.wrapPending; },
      set wrapPending(v: boolean) { self.wrapPending = v; },
      get onBell() { return self.onBell; },
      switchToAlternateBuffer: (saveCursor: boolean) => self.switchToAlternateBuffer(saveCursor),
      switchToPrimaryBuffer: (restoreCursor: boolean) => self.switchToPrimaryBuffer(restoreCursor),
      addPendingResponse: (response: Uint8Array) => self.addPendingResponse(response),
      reset: () => self.reset(),
    };
    return ctx;
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
   * Check if the state's WASM grids are still alive (not disposed).
   */
  isReady(): boolean {
    const grid = this.getActiveWasmGrid();
    return grid != null && !grid.isDisposed;
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
   * Get the data viewer session manager.
   */
  getDataViewerManager(): DataViewerSessionManager {
    return this.dataViewerManager;
  }

  getMarkdownManager(): MarkdownSessionManager {
    return this.markdownManager;
  }

  /**
   * Get the semantic zone tracker.
   */
  getSemanticZoneTracker(): SemanticZoneTracker {
    return this.semanticZoneTracker;
  }

  /**
   * Get the fold manager.
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

    // Reset session managers (preserve container/callbacks)
    this.dataViewerManager.resetSessions();
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
      this.dataViewerManager.resetSessions();
      this.markdownManager.dispose();
      this.markdownManager = new MarkdownSessionManager();
      this.semanticZoneTracker.clear();
      this.foldManager.unfoldAll();

      console.warn("[WARN][FRONTEND] WASM core recreated after crash — terminal content lost");
      return true;
    } catch (e) {
      console.error("[ERROR][FRONTEND] Failed to recreate WASM core:", e);
      // Null out all WASM-dependent references to prevent dangling pointers.
      this.primaryWasmGrid = null;
      this.alternateWasmGrid = null;
      this.alternateBuffer = null;
      this.alternateCursor = null;
      this.useAlternate = false;
      // Replace primary buffer/cursor with JS-only fallbacks (no WASM backing)
      this.primaryBuffer = new UnifiedBuffer(cols, rows, 0);
      this.primaryCursor = new CursorState(cols, rows);
      this.cursor = this.primaryCursor;
      return false;
    }
  }

  /**
   * Restore terminal state from a binary snapshot.
   * Replaces the primary WASM grid with the restored core.
   * Returns true if restoration was successful.
   */
  restoreFromSnapshot(bytes: Uint8Array): boolean {
    try {
      const restoredCore = TerminalCore.wasm_restore_from_bytes(bytes);
      if (!restoredCore) {
        console.warn("[WARN][FRONTEND] Snapshot restore returned null (version mismatch or corruption)");
        return false;
      }

      const cols = restoredCore.cols();
      const rows = restoredCore.rows();

      // Dispose old grids
      try { this.primaryWasmGrid?.dispose(); } catch { /* ignore */ }
      try { this.alternateWasmGrid?.dispose(); } catch { /* ignore */ }
      this.alternateWasmGrid = null;
      this.alternateBuffer = null;
      this.useAlternate = false;

      // Wrap restored core in WasmGrid
      this.primaryWasmGrid = WasmGrid.fromCore(restoredCore);

      // Rebuild primary buffer with restored grid
      this.primaryBuffer = new UnifiedBuffer(cols, rows, this.maxScrollbackLines, this.primaryWasmGrid);
      this.primaryBuffer.onEvict = (count: number) => {
        this.semanticZoneTracker.pruneBeforeLine(count);
        this.foldManager.pruneBeforeLine(count);
      };

      // Restore cursor from WASM core state
      this.primaryCursor = new CursorState(cols, rows, restoredCore);
      this.primaryCursor.moveTo(restoredCore.get_cursor_col(), restoredCore.get_cursor_row());
      this.alternateCursor = null;
      this.cursor = this.primaryCursor;
      this.savedCursorForAlt = null;

      // Sync modes from restored WASM core
      syncModesFromWasm(this.modes, this.primaryWasmGrid.core);

      // Propagate cell size to restored core
      restoredCore.set_cell_size_px(this.cellWidthPx, this.cellHeightPx);

      // Mark all rows dirty so renderer repaints everything
      this.primaryWasmGrid.markAllDirty();

      // Reset TS-side state that isn't part of snapshot
      this.wrapPending = false;
      this.graphemeBuffer = [];

      return true;
    } catch (e) {
      console.error("[ERROR][FRONTEND] Failed to restore from snapshot:", e);
      return false;
    }
  }

  /**
   * Extract plain text from a grid range for copy operations.
   *
   * Coordinates are automatically normalized (start comes before end).
   * Trailing spaces on each line are removed.
   * Lines are joined with '\n'.
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
