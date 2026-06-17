/**
 * CSI screen erase handlers.
 *
 * Handles screen and line erase CSI sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { EraseMode } from "../../types/terminal.ts";
import { CSI_DEFAULTS } from "./semantics.ts";

/**
 * Handle EraseInDisplay (CSI Ps J).
 *
 * Erase part or all of the display.
 *
 * @param state - Terminal state accessor
 * @param mode - Erase mode: Below, Above, All, or Scrollback
 */
export function handleEraseInDisplay(
  state: TerminalStateAccessor,
  mode: EraseMode
): void {
  const buffer = state.getActiveBuffer();
  const col = state.cursor.col;
  const row = state.cursor.row;

  switch (mode) {
    case "Below":
      buffer.clearBelow(col, row);
      break;
    case "Above":
      buffer.clearAbove(col, row);
      break;
    case "All":
      buffer.clearAll();
      break;
    case "Scrollback":
      // Scrollback not implemented yet, clear all as fallback
      buffer.clearAll();
      break;
  }
}

/**
 * Handle EraseInLine (CSI Ps K).
 *
 * Erase part or all of the current line.
 *
 * @param state - Terminal state accessor
 * @param mode - Erase mode: Below (to end), Above (to start), All
 */
export function handleEraseInLine(
  state: TerminalStateAccessor,
  mode: EraseMode
): void {
  const buffer = state.getActiveBuffer();
  const col = state.cursor.col;
  const row = state.cursor.row;

  switch (mode) {
    case "Below":
      buffer.clearLineFromCursor(row, col);
      break;
    case "Above":
      buffer.clearLineToCursor(row, col);
      break;
    case "All":
      buffer.clearLine(row);
      break;
    default:
      // Scrollback not applicable to line erase
      break;
  }
}

/**
 * Handle EraseCharacters (CSI Ps X).
 *
 * Erase N characters at cursor position without shifting content.
 *
 * @param state - Terminal state accessor
 * @param count - Number of characters to erase (default: 1)
 */
export function handleEraseCharacters(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.eraseCharacters(
    state.cursor.row,
    state.cursor.col,
    count ?? CSI_DEFAULTS.EraseCharacters
  );
}
