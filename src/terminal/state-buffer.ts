/**
 * Terminal state buffer switching.
 *
 * Handles switching between primary and alternate screen buffers.
 * Extracted from TerminalState for separation of concerns.
 */

import { UnifiedBuffer } from "./unified-buffer.ts";
import { CursorState } from "./cursor.ts";
import { syncModesToWasm, type TerminalModes } from "./modes.ts";
import { WasmGrid } from "./wasm/terminal-core.ts";

/**
 * Context needed for buffer switching operations.
 * Provides access to the terminal state fields that buffer switching reads/writes.
 */
export interface BufferSwitchContext {
  // Buffer state
  primaryBuffer: UnifiedBuffer;
  alternateBuffer: UnifiedBuffer | null;
  useAlternate: boolean;

  // Cursor state
  primaryCursor: CursorState;
  alternateCursor: CursorState | null;
  cursor: CursorState;
  savedCursorForAlt: CursorState | null;

  // WASM grids
  primaryWasmGrid: WasmGrid | null;
  alternateWasmGrid: WasmGrid | null;

  // Dimensions and modes
  cols: number;
  rows: number;
  modes: TerminalModes;
  cellWidthPx: number;
  cellHeightPx: number;

  // Wrap state
  wrapPending: boolean;
}

/**
 * Result of a buffer switch operation.
 * Contains updated fields that the caller must apply to its state.
 */
export interface BufferSwitchResult {
  alternateBuffer: UnifiedBuffer | null;
  alternateWasmGrid: WasmGrid | null;
  alternateCursor: CursorState | null;
  useAlternate: boolean;
  cursor: CursorState;
  savedCursorForAlt: CursorState | null;
  wrapPending: boolean;
}

/**
 * Switch to alternate screen buffer.
 *
 * @param ctx - Buffer switch context with current state
 * @param saveCursor - Whether to save cursor before switching
 * @returns Updated state fields to apply
 *
 * Ensures consistent state:
 * - Cursor is saved before switching if requested
 * - Alternate buffer is cleared on each switch
 * - Cursor is reset to home position (0, 0)
 */
export function switchToAlternateBuffer(
  ctx: BufferSwitchContext,
  saveCursor: boolean = false,
): BufferSwitchResult {
  if (ctx.useAlternate) {
    return {
      alternateBuffer: ctx.alternateBuffer,
      alternateWasmGrid: ctx.alternateWasmGrid,
      alternateCursor: ctx.alternateCursor,
      useAlternate: ctx.useAlternate,
      cursor: ctx.cursor,
      savedCursorForAlt: ctx.savedCursorForAlt,
      wrapPending: ctx.wrapPending,
    };
  }

  let savedCursorForAlt = ctx.savedCursorForAlt;
  if (saveCursor) {
    // Save primary cursor for 1049 mode
    savedCursorForAlt = ctx.primaryCursor.clone();
  }

  let alternateBuffer = ctx.alternateBuffer;
  let alternateWasmGrid = ctx.alternateWasmGrid;
  let alternateCursor = ctx.alternateCursor;

  // Create or reset alternate buffer (no scrollback)
  if (!alternateBuffer) {
    // Create WASM grid for alternate buffer if primary uses WASM
    if (ctx.primaryWasmGrid) {
      try {
        alternateWasmGrid = new WasmGrid(ctx.cols, ctx.rows);
      } catch {
        alternateWasmGrid = null;
      }
    }
    alternateBuffer = new UnifiedBuffer(ctx.cols, ctx.rows, 0, alternateWasmGrid ?? undefined);
    alternateCursor = new CursorState(ctx.cols, ctx.rows, alternateWasmGrid?.core);
  } else {
    // Clear alternate buffer on switch
    alternateBuffer.clearAll();
    if (alternateWasmGrid) {
      alternateWasmGrid.reset();
    }
    // Reset alternate cursor to home position
    if (!alternateCursor) {
      alternateCursor = new CursorState(ctx.cols, ctx.rows, alternateWasmGrid?.core);
    } else {
      alternateCursor.moveTo(0, 0);
    }
  }

  // Propagate cell size to alternate core
  if (alternateWasmGrid) {
    alternateWasmGrid.core.set_cell_size_px(
      ctx.cellWidthPx,
      ctx.cellHeightPx,
    );
  }

  // Sync current TS modes to the new alternate WASM core
  // Without this, the alternate core starts with default modes (e.g. cursorVisible=true),
  // and the next syncModesFromWasm() would overwrite TS modes with those defaults.
  if (alternateWasmGrid) {
    syncModesToWasm(ctx.modes, alternateWasmGrid.core);
  }

  // Mark all lines as dirty to force redraw
  // Use markDirty() to propagate to WASM dirty bitset (not just local field)
  for (let row = 0; row < ctx.rows; row++) {
    alternateBuffer.getLine(row).markDirty();
  }

  return {
    alternateBuffer,
    alternateWasmGrid,
    alternateCursor,
    useAlternate: true,
    cursor: alternateCursor!,
    savedCursorForAlt,
    wrapPending: false,
  };
}

/**
 * Switch to primary screen buffer.
 *
 * @param ctx - Buffer switch context with current state
 * @param restoreCursor - Whether to restore cursor after switching
 * @returns Updated state fields to apply
 *
 * Ensures consistent state:
 * - Cursor is restored if requested (mode 1049)
 * - All lines marked dirty for redraw
 * - Wrap state is cleared
 */
export function switchToPrimaryBuffer(
  ctx: BufferSwitchContext,
  restoreCursor: boolean = false,
): BufferSwitchResult {
  if (!ctx.useAlternate) {
    return {
      alternateBuffer: ctx.alternateBuffer,
      alternateWasmGrid: ctx.alternateWasmGrid,
      alternateCursor: ctx.alternateCursor,
      useAlternate: ctx.useAlternate,
      cursor: ctx.cursor,
      savedCursorForAlt: ctx.savedCursorForAlt,
      wrapPending: ctx.wrapPending,
    };
  }

  const primaryCursor = ctx.primaryCursor;
  let savedCursorForAlt = ctx.savedCursorForAlt;

  // Restore cursor if requested (for mode 1049)
  if (restoreCursor && savedCursorForAlt) {
    primaryCursor.restoreFrom(savedCursorForAlt);
    savedCursorForAlt = null;
  }

  // Sync current TS modes to the primary WASM core
  // Prevents stale defaults from overwriting TS modes on next syncModesFromWasm()
  if (ctx.primaryWasmGrid) {
    syncModesToWasm(ctx.modes, ctx.primaryWasmGrid.core);
  }

  // Mark all lines as dirty to force redraw
  // Use markDirty() to propagate to WASM dirty bitset (not just local field)
  for (let row = 0; row < ctx.rows; row++) {
    ctx.primaryBuffer.getLine(row).markDirty();
  }

  return {
    alternateBuffer: ctx.alternateBuffer,
    alternateWasmGrid: ctx.alternateWasmGrid,
    alternateCursor: ctx.alternateCursor,
    useAlternate: false,
    cursor: primaryCursor,
    savedCursorForAlt,
    wrapPending: false,
  };
}
