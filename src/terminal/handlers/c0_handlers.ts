/**
 * C0 control character handlers.
 *
 * Handles C0 control codes (0x00-0x1F).
 */

import type { TerminalStateAccessor } from "./types.ts";
import { C0 } from "../../types/terminal.ts";

/**
 * Dispatch C0 control code to specific handler.
 *
 * @param state - Terminal state accessor
 * @param code - C0 control code
 */
export function handleExecuteDispatch(
  state: TerminalStateAccessor,
  code: number
): void {
  switch (code) {
    case C0.BEL:
      handleBel(state);
      break;
    case C0.BS:
      handleBackspace(state);
      break;
    case C0.HT:
      handleTab(state);
      break;
    case C0.LF:
    case C0.VT:
    case C0.FF:
      handleLineFeed(state);
      break;
    case C0.CR:
      handleCarriageReturn(state);
      break;
    case C0.SO:
      handleShiftOut(state);
      break;
    case C0.SI:
      handleShiftIn(state);
      break;
    default:
      // Ignore other control characters
      break;
  }
}

/**
 * Handle BEL (0x07).
 *
 * Bell - could emit event, for now do nothing.
 *
 * @param _state - Terminal state accessor (unused)
 */
export function handleBel(_state: TerminalStateAccessor): void {
  // Bell - could emit event, for now do nothing
}

/**
 * Handle BS (0x08).
 *
 * Backspace - move cursor left by 1.
 *
 * @param state - Terminal state accessor
 */
export function handleBackspace(state: TerminalStateAccessor): void {
  state.cursor.backspace();
  state.wrapPending = false;
}

/**
 * Handle HT (0x09).
 *
 * Horizontal tab - move to next tab stop.
 *
 * @param state - Terminal state accessor
 */
export function handleTab(state: TerminalStateAccessor): void {
  // Find next tab stop
  const currentCol = state.cursor.col;
  const sortedStops = Array.from(state.tabStops).sort((a, b) => a - b);

  for (const stop of sortedStops) {
    if (stop > currentCol) {
      state.cursor.col = Math.min(stop, state.cols - 1);
      state.wrapPending = false;
      return;
    }
  }

  // No more tab stops, move to end of line
  state.cursor.col = state.cols - 1;
  state.wrapPending = false;
}

/**
 * Handle LF (0x0A), VT (0x0B), FF (0x0C).
 *
 * Line feed - move cursor down, scroll if at bottom.
 *
 * @param state - Terminal state accessor
 */
export function handleLineFeed(state: TerminalStateAccessor): void {
  const buffer = state.getActiveBuffer();
  if (state.cursor.lineFeed()) {
    buffer.scrollUp();
  }
  state.wrapPending = false;
}

/**
 * Handle CR (0x0D).
 *
 * Carriage return - move cursor to column 0.
 *
 * @param state - Terminal state accessor
 */
export function handleCarriageReturn(state: TerminalStateAccessor): void {
  state.cursor.carriageReturn();
  state.wrapPending = false;
}

/**
 * Handle SO (0x0E).
 *
 * Shift Out - switch to G1 character set.
 *
 * @param state - Terminal state accessor
 */
export function handleShiftOut(state: TerminalStateAccessor): void {
  state.activeCharSet = "G1";
}

/**
 * Handle SI (0x0F).
 *
 * Shift In - switch to G0 character set.
 *
 * @param state - Terminal state accessor
 */
export function handleShiftIn(state: TerminalStateAccessor): void {
  state.activeCharSet = "G0";
}
