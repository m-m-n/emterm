/**
 * Terminal state recovery and reinitialization helpers.
 *
 * Shared building blocks for the three reset-style flows:
 *   - reset()              — RIS / explicit reset
 *   - recreateWasmCore()   — auto-recovery after WASM crash
 *   - restoreFromSnapshot()— restore primary grid from snapshot bytes
 *
 * Each flow rebuilds the primary buffer/cursor around a (new or restored)
 * WasmGrid. This module factors out the buffer/cursor construction so the
 * entry-point methods on TerminalState stay focused on flow control.
 *
 * Extracted from TerminalState for separation of concerns.
 */

import { CursorState } from "./cursor.ts";
import type { FoldManager } from "./fold-manager.ts";
import type { SemanticZoneTracker } from "./semantic-zone.ts";
import { UnifiedBuffer } from "./unified-buffer.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";

/**
 * Bind the standard eviction callback that prunes semantic zones and folds
 * when scrollback overflows.
 */
export function bindPrimaryEvictCallback(
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
 * Result of building a fresh primary buffer/cursor pair around a WasmGrid.
 */
export interface FreshPrimary {
  primaryBuffer: UnifiedBuffer;
  primaryCursor: CursorState;
}

/**
 * Build a fresh primary buffer + cursor around a (new or existing) WasmGrid.
 * Used after WASM grid replacement (reset / recreate / restore).
 *
 * @param wasmGrid - WasmGrid to back the buffer (may be null for JS-only fallback)
 * @param cols
 * @param rows
 * @param maxScrollbackLines
 * @param semanticZoneTracker
 * @param foldManager
 * @param cursorRow - Initial cursor row (default 0). Used by restore paths.
 * @param cursorCol - Initial cursor col (default 0).
 */
export function buildFreshPrimary(
  wasmGrid: WasmGrid | null,
  cols: number,
  rows: number,
  maxScrollbackLines: number,
  semanticZoneTracker: SemanticZoneTracker,
  foldManager: FoldManager,
  cursorRow: number = 0,
  cursorCol: number = 0,
): FreshPrimary {
  const primaryBuffer = new UnifiedBuffer(
    cols,
    rows,
    maxScrollbackLines,
    wasmGrid ?? undefined,
  );
  bindPrimaryEvictCallback(primaryBuffer, semanticZoneTracker, foldManager);

  const primaryCursor = new CursorState(cols, rows, wasmGrid?.core);
  if (cursorRow !== 0 || cursorCol !== 0) {
    primaryCursor.moveTo(cursorCol, cursorRow);
  }

  return { primaryBuffer, primaryCursor };
}
