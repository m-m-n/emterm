/**
 * CSI edit handlers.
 *
 * Handles insert/delete operations for lines and characters.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { CSI_DEFAULTS } from "./semantics.ts";

/**
 * Handle InsertLines (CSI Ps L).
 *
 * Insert blank lines at cursor row, pushing content down.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to insert (default: 1)
 */
export function handleInsertLines(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.insertLines(state.cursor.row, count ?? CSI_DEFAULTS.InsertLines);
}

/**
 * Handle DeleteLines (CSI Ps M).
 *
 * Delete lines at cursor row, pulling content up.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to delete (default: 1)
 */
export function handleDeleteLines(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.deleteLines(state.cursor.row, count ?? CSI_DEFAULTS.DeleteLines);
}

/**
 * Handle InsertCharacters (CSI Ps @).
 *
 * Insert blank characters at cursor position, shifting content right.
 *
 * @param state - Terminal state accessor
 * @param count - Number of characters to insert (default: 1)
 */
export function handleInsertCharacters(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.insertCharacters(
    state.cursor.row,
    state.cursor.col,
    count ?? CSI_DEFAULTS.InsertCharacters
  );
}

/**
 * Handle DeleteCharacters (CSI Ps P).
 *
 * Delete characters at cursor position, shifting content left.
 *
 * @param state - Terminal state accessor
 * @param count - Number of characters to delete (default: 1)
 */
export function handleDeleteCharacters(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.deleteCharacters(
    state.cursor.row,
    state.cursor.col,
    count ?? CSI_DEFAULTS.DeleteCharacters
  );
}
