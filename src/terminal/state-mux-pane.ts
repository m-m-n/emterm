/**
 * Mux pane grid state save/restore/swap.
 *
 * Manages the multi-pane WASM grid bookkeeping that the mux window manager
 * uses to swap pane state on tab switches.
 *
 * Extracted from TerminalState for separation of concerns.
 */

import { CursorState } from "./cursor.ts";
import { syncModesFromWasm, type CursorKeysMode, type MouseEncoding, type MouseTrackingMode, type TerminalModes } from "./modes.ts";
import { UnifiedBuffer } from "./unified-buffer.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";
import { setCellSizePxOnGrid } from "./state-wasm-sync.ts";
import type { SemanticZoneTracker } from "./semantic-zone.ts";
import type { FoldManager } from "./fold-manager.ts";

/**
 * Snapshot of a mux pane's grid state (primary + alternate screen).
 * Used by mux window manager to save/restore pane state on window switch.
 */
export interface MuxPaneGridState {
  primaryGrid: WasmGrid;
  alternateGrid: WasmGrid | null;
  useAlternate: boolean;
  /** Window title at the time of save (OSC 0/2). Restored on switch so tabs
   *  don't leak titles across panes (e.g. Claude Code's title appearing on
   *  unrelated tabs). */
  title: string;
  /** Window icon name at the time of save (OSC 0/1). */
  iconName: string;
  /** TS-only modes not stored in WASM (mouseTracking, mouseEncoding, cursorKeys). */
  tsModes: {
    mouseTracking: MouseTrackingMode;
    mouseEncoding: MouseEncoding;
    cursorKeys: CursorKeysMode;
  };
}

/**
 * Inputs needed to build a snapshot of the active pane.
 */
export interface MuxSaveContext {
  primaryWasmGrid: WasmGrid | null;
  alternateWasmGrid: WasmGrid | null;
  useAlternate: boolean;
  title: string;
  iconName: string;
  modes: TerminalModes;
}

/**
 * Build a MuxPaneGridState snapshot from the current state.
 * The returned grids are shared references — the caller is expected to
 * immediately replace them via swap or restore so no concurrent mutation
 * occurs.
 */
export function saveMuxPaneState(ctx: MuxSaveContext): MuxPaneGridState {
  return {
    primaryGrid: ctx.primaryWasmGrid!,
    alternateGrid: ctx.alternateWasmGrid,
    useAlternate: ctx.useAlternate,
    title: ctx.title,
    iconName: ctx.iconName,
    tsModes: {
      mouseTracking: ctx.modes.mouseTracking,
      mouseEncoding: ctx.modes.mouseEncoding,
      cursorKeys: ctx.modes.cursorKeys,
    },
  };
}

/**
 * Result of a restore operation. Caller assigns each field back onto
 * the TerminalState instance. Includes title/iconName because restore
 * pulls them from the saved pane snapshot.
 */
export interface MuxRestoreResult {
  primaryWasmGrid: WasmGrid;
  alternateWasmGrid: WasmGrid | null;
  primaryBuffer: UnifiedBuffer;
  alternateBuffer: UnifiedBuffer | null;
  primaryCursor: CursorState;
  alternateCursor: CursorState | null;
  cursor: CursorState;
  useAlternate: boolean;
  title: string;
  iconName: string;
  savedCursorForAlt: CursorState | null;
}

/**
 * Result of a swap operation (new pane creation). title/iconName are NOT
 * touched on swap — existing values are preserved by the caller.
 */
export interface MuxSwapResult {
  primaryWasmGrid: WasmGrid;
  alternateWasmGrid: null;
  primaryBuffer: UnifiedBuffer;
  alternateBuffer: null;
  primaryCursor: CursorState;
  alternateCursor: null;
  cursor: CursorState;
  useAlternate: false;
  savedCursorForAlt: null;
}

/**
 * Bind an eviction callback so scrollback overflow keeps zone/fold trackers
 * pruned. Same shape used by the constructor and recovery paths.
 */
function bindEvictCallback(
  buffer: UnifiedBuffer,
  semanticZoneTracker: SemanticZoneTracker,
  foldManager: FoldManager,
): void {
  buffer.onEvict = (count: number) => {
    semanticZoneTracker.pruneBeforeLine(count);
    foldManager.pruneBeforeLine(count);
  };
}

/**
 * Restore a previously saved mux pane state (primary + alternate).
 * Rebuilds buffers and cursors around the restored grids.
 *
 * Note: callers must NOT dispose existing grids before calling — they may be
 * shared references saved by saveMuxPaneState for another pane.
 */
export function restoreMuxPaneState(
  paneState: MuxPaneGridState,
  modes: TerminalModes,
  semanticZoneTracker: SemanticZoneTracker,
  foldManager: FoldManager,
  maxScrollbackLines: number,
  cellWidthPx: number,
  cellHeightPx: number,
): MuxRestoreResult {
  // Rebuild primary buffer and cursor
  const cols = paneState.primaryGrid.core.cols();
  const rows = paneState.primaryGrid.core.rows();
  const primaryBuffer = new UnifiedBuffer(cols, rows, maxScrollbackLines, paneState.primaryGrid);
  bindEvictCallback(primaryBuffer, semanticZoneTracker, foldManager);
  const primaryCursor = new CursorState(cols, rows, paneState.primaryGrid.core);
  primaryCursor.moveTo(
    paneState.primaryGrid.core.get_cursor_col(),
    paneState.primaryGrid.core.get_cursor_row(),
  );

  // Rebuild alternate buffer and cursor if alternate screen was active
  let alternateBuffer: UnifiedBuffer | null = null;
  let alternateCursor: CursorState | null = null;
  if (paneState.alternateGrid) {
    const altCols = paneState.alternateGrid.core.cols();
    const altRows = paneState.alternateGrid.core.rows();
    alternateBuffer = new UnifiedBuffer(altCols, altRows, 0, paneState.alternateGrid);
    alternateCursor = new CursorState(altCols, altRows, paneState.alternateGrid.core);
    alternateCursor.moveTo(
      paneState.alternateGrid.core.get_cursor_col(),
      paneState.alternateGrid.core.get_cursor_row(),
    );
  }

  // Active cursor
  const cursor = paneState.useAlternate && alternateCursor
    ? alternateCursor
    : primaryCursor;

  // Sync modes from the active core (boolean modes stored in WASM)
  const activeCore = paneState.useAlternate && paneState.alternateGrid
    ? paneState.alternateGrid.core
    : paneState.primaryGrid.core;
  syncModesFromWasm(modes, activeCore);

  // Restore TS-only modes (not stored in WASM bitfield)
  modes.mouseTracking = paneState.tsModes.mouseTracking;
  modes.mouseEncoding = paneState.tsModes.mouseEncoding;
  modes.cursorKeys = paneState.tsModes.cursorKeys;

  // Propagate cell size to all grids
  setCellSizePxOnGrid(paneState.primaryGrid, cellWidthPx, cellHeightPx);
  if (paneState.alternateGrid) {
    setCellSizePxOnGrid(paneState.alternateGrid, cellWidthPx, cellHeightPx);
  }

  // Mark all rows dirty for full repaint
  activeCore.mark_all_dirty();

  return {
    primaryWasmGrid: paneState.primaryGrid,
    alternateWasmGrid: paneState.alternateGrid,
    primaryBuffer,
    alternateBuffer,
    primaryCursor,
    alternateCursor,
    cursor,
    useAlternate: paneState.useAlternate,
    title: paneState.title,
    iconName: paneState.iconName,
    savedCursorForAlt: null,
  };
}

/**
 * Swap the primary WASM grid with a fresh one (for new mux pane creation).
 * Resets alternate screen state. Returns the (rebuilt) result plus the old
 * primary grid that the caller may dispose / hand off to another pane.
 *
 * Do NOT dispose the old alternate grid here — saveMuxPaneState() may hold
 * a reference for the previous pane. Ownership transfers to the caller.
 */
export function swapPrimaryGrid(
  newGrid: WasmGrid,
  modes: TerminalModes,
  semanticZoneTracker: SemanticZoneTracker,
  foldManager: FoldManager,
  maxScrollbackLines: number,
  cellWidthPx: number,
  cellHeightPx: number,
): MuxSwapResult {
  const cols = newGrid.core.cols();
  const rows = newGrid.core.rows();

  const primaryBuffer = new UnifiedBuffer(cols, rows, maxScrollbackLines, newGrid);
  bindEvictCallback(primaryBuffer, semanticZoneTracker, foldManager);
  const primaryCursor = new CursorState(cols, rows, newGrid.core);
  primaryCursor.moveTo(newGrid.core.get_cursor_col(), newGrid.core.get_cursor_row());

  // Sync modes from the new core (boolean modes stored in WASM)
  syncModesFromWasm(modes, newGrid.core);

  // Reset TS-only modes — new pane starts with defaults
  modes.mouseTracking = "none";
  modes.mouseEncoding = "default";
  modes.cursorKeys = "normal";

  // Propagate cell size
  setCellSizePxOnGrid(newGrid, cellWidthPx, cellHeightPx);

  // Mark all rows dirty for full repaint
  newGrid.core.mark_all_dirty();

  return {
    primaryWasmGrid: newGrid,
    alternateWasmGrid: null,
    primaryBuffer,
    alternateBuffer: null,
    primaryCursor,
    alternateCursor: null,
    cursor: primaryCursor,
    useAlternate: false,
    savedCursorForAlt: null,
  };
}
