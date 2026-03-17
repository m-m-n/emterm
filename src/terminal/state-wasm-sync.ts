/**
 * Terminal state WASM synchronization.
 *
 * Functions for syncing terminal modes, cell dimensions, cursor flags,
 * and tab stops between TypeScript state and WASM cores.
 * Extracted from TerminalState for separation of concerns.
 */

import { syncModesToWasm as syncModes, syncModesFromWasm as syncModesFrom, type TerminalModes } from "./modes.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";

/**
 * Sync boolean modes from JS TerminalModes to WASM bitfield.
 * No-op when WASM grid is null.
 *
 * @param modes - Terminal modes to push
 * @param grid - Active WASM grid (may be null)
 */
export function syncModesToWasm(modes: TerminalModes, grid: WasmGrid | null): void {
  if (grid) {
    syncModes(modes, grid.core);
  }
}

/**
 * Sync boolean modes from WASM bitfield to JS TerminalModes.
 * Call after process_pty_data() to pick up mode changes made inside WASM.
 * No-op when WASM grid is null.
 *
 * @param modes - Terminal modes to update
 * @param grid - Active WASM grid (may be null)
 */
export function syncModesFromWasm(modes: TerminalModes, grid: WasmGrid | null): void {
  if (grid) {
    syncModesFrom(modes, grid.core);
  }
}

/**
 * Set cell size in pixels on a WASM grid.
 * No-op when grid is null.
 *
 * @param grid - Active WASM grid (may be null)
 * @param width - Cell width in pixels
 * @param height - Cell height in pixels
 */
export function setCellSizePxOnGrid(grid: WasmGrid | null, width: number, height: number): void {
  grid?.core.set_cell_size_px(width, height);
}

/**
 * Enable/disable cursor hidden-to-visible interrupt in WASM parser.
 * Applied to both primary and alternate grids.
 *
 * @param primaryGrid - Primary WASM grid (may be null)
 * @param alternateGrid - Alternate WASM grid (may be null)
 * @param enable - Whether to enable the interrupt
 */
export function setCursorShowInterrupt(
  primaryGrid: WasmGrid | null,
  alternateGrid: WasmGrid | null,
  enable: boolean,
): void {
  primaryGrid?.core.set_cursor_show_interrupt(enable);
  alternateGrid?.core.set_cursor_show_interrupt(enable);
}

/**
 * Set a tab stop in WASM core.
 * No-op when grid is null.
 *
 * @param grid - Active WASM grid (may be null)
 * @param col - Column to set tab stop at
 */
export function syncTabStopToWasm(grid: WasmGrid | null, col: number): void {
  grid?.core.set_tab_stop(col);
}

/**
 * Clear a tab stop in WASM core.
 * No-op when grid is null.
 *
 * @param grid - Active WASM grid (may be null)
 * @param col - Column to clear tab stop at
 */
export function syncClearTabStopToWasm(grid: WasmGrid | null, col: number): void {
  grid?.core.clear_tab_stop(col);
}

/**
 * Clear all tab stops in WASM core.
 * No-op when grid is null.
 *
 * @param grid - Active WASM grid (may be null)
 */
export function syncClearAllTabStopsToWasm(grid: WasmGrid | null): void {
  grid?.core.clear_all_tab_stops();
}
