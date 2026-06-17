/**
 * CSI cursor movement handlers.
 *
 * Handles cursor positioning CSI sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { CSI_DEFAULTS, toZeroIndexed } from "./semantics.ts";

/**
 * Handle CursorUp (CSI Ps A).
 *
 * Move cursor up by count rows.
 *
 * @param state - Terminal state accessor
 * @param count - Number of rows to move (default: 1)
 */
export function handleCursorUp(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveUp(count ?? CSI_DEFAULTS.CursorUp);
  state.wrapPending = false;
}

/**
 * Handle CursorDown (CSI Ps B).
 *
 * Move cursor down by count rows.
 *
 * @param state - Terminal state accessor
 * @param count - Number of rows to move (default: 1)
 */
export function handleCursorDown(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveDown(count ?? CSI_DEFAULTS.CursorDown);
  state.wrapPending = false;
}

/**
 * Handle CursorForward (CSI Ps C).
 *
 * Move cursor right by count columns.
 *
 * @param state - Terminal state accessor
 * @param count - Number of columns to move (default: 1)
 */
export function handleCursorForward(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveRight(count ?? CSI_DEFAULTS.CursorForward);
  state.wrapPending = false;
}

/**
 * Handle CursorBack (CSI Ps D).
 *
 * Move cursor left by count columns.
 *
 * @param state - Terminal state accessor
 * @param count - Number of columns to move (default: 1)
 */
export function handleCursorBack(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveLeft(count ?? CSI_DEFAULTS.CursorBack);
  state.wrapPending = false;
}

/**
 * Handle CursorNextLine (CSI Ps E).
 *
 * Move cursor down N lines and to column 0.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to move (default: 1)
 */
export function handleCursorNextLine(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveDown(count ?? CSI_DEFAULTS.CursorNextLine);
  state.cursor.carriageReturn();
  state.wrapPending = false;
}

/**
 * Handle CursorPreviousLine (CSI Ps F).
 *
 * Move cursor up N lines and to column 0.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to move (default: 1)
 */
export function handleCursorPreviousLine(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  state.cursor.moveUp(count ?? CSI_DEFAULTS.CursorPreviousLine);
  state.cursor.carriageReturn();
  state.wrapPending = false;
}

/**
 * Handle CursorHorizontalAbsolute (CSI Ps G).
 *
 * Set cursor column (1-indexed ANSI input).
 *
 * @param state - Terminal state accessor
 * @param col - Column number (1-indexed, default: 1)
 */
export function handleCursorHorizontalAbsolute(
  state: TerminalStateAccessor,
  col: number | undefined
): void {
  const targetCol = toZeroIndexed(col ?? CSI_DEFAULTS.CursorHorizontalAbsolute);
  state.cursor.setColumn(targetCol);
  state.wrapPending = false;
}

/**
 * Handle CursorVerticalAbsolute (CSI Ps d).
 *
 * Set cursor row (1-indexed ANSI input).
 *
 * @param state - Terminal state accessor
 * @param row - Row number (1-indexed, default: 1)
 */
export function handleCursorVerticalAbsolute(
  state: TerminalStateAccessor,
  row: number | undefined
): void {
  const targetRow = toZeroIndexed(row ?? CSI_DEFAULTS.CursorVerticalAbsolute);
  state.cursor.setRow(targetRow);
  state.wrapPending = false;
}

/**
 * Handle CursorPosition (CSI row ; col H).
 *
 * Set cursor position (1-indexed ANSI input).
 *
 * @param state - Terminal state accessor
 * @param row - Row number (1-indexed, default: 1)
 * @param col - Column number (1-indexed, default: 1)
 */
export function handleCursorPosition(
  state: TerminalStateAccessor,
  row: number | undefined,
  col: number | undefined
): void {
  const targetRow = toZeroIndexed(row ?? 1);
  const targetCol = toZeroIndexed(col ?? 1);
  state.cursor.moveTo(targetCol, targetRow);
  state.wrapPending = false;
}
