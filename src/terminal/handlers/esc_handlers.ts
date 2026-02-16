/**
 * ESC sequence handlers.
 *
 * Handles ESC escape sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { EscAction, CharSet } from "../../types/terminal.ts";

/**
 * Dispatch ESC action to specific handler.
 *
 * @param state - Terminal state accessor
 * @param action - ESC action to dispatch
 */
export function handleEscDispatch(
  state: TerminalStateAccessor,
  action: EscAction
): void {
  switch (action.action) {
    case "SaveCursor":
      handleSaveCursor(state);
      break;
    case "RestoreCursor":
      handleRestoreCursor(state);
      break;
    case "Index":
      handleIndex(state);
      break;
    case "NextLine":
      handleNextLine(state);
      break;
    case "ReverseIndex":
      handleReverseIndex(state);
      break;
    case "HorizontalTabSet":
      handleHorizontalTabSet(state);
      break;
    case "ResetToInitialState":
      handleResetToInitialState(state);
      break;
    case "SetG0CharSet":
      handleSetG0CharSet(state, action.data as CharSet);
      break;
    case "SetG1CharSet":
      handleSetG1CharSet(state, action.data as CharSet);
      break;
    case "Unknown":
      // Log unknown sequences for debugging
      // console.debug("Unknown ESC:", action.data);
      break;
  }
}

/**
 * Handle SaveCursor (ESC 7 / DECSC).
 *
 * Save cursor position and attributes.
 *
 * @param state - Terminal state accessor
 */
export function handleSaveCursor(state: TerminalStateAccessor): void {
  state.cursor.save();
}

/**
 * Handle RestoreCursor (ESC 8 / DECRC).
 *
 * Restore cursor position and attributes.
 *
 * @param state - Terminal state accessor
 */
export function handleRestoreCursor(state: TerminalStateAccessor): void {
  state.cursor.restore();
  state.wrapPending = false;
}

/**
 * Handle Index (ESC D / IND).
 *
 * Move cursor down, scroll if at bottom.
 *
 * @param state - Terminal state accessor
 */
export function handleIndex(state: TerminalStateAccessor): void {
  const buffer = state.getActiveBuffer();
  const { bottom } = buffer.getEffectiveScrollRegion();
  if (state.cursor.lineFeed(bottom)) {
    buffer.scrollUp();
  }
}

/**
 * Handle NextLine (ESC E / NEL).
 *
 * Move to column 0 of next line, scroll if needed.
 *
 * @param state - Terminal state accessor
 */
export function handleNextLine(state: TerminalStateAccessor): void {
  const buffer = state.getActiveBuffer();
  const { bottom } = buffer.getEffectiveScrollRegion();
  state.cursor.carriageReturn();
  if (state.cursor.lineFeed(bottom)) {
    buffer.scrollUp();
  }
}

/**
 * Handle ReverseIndex (ESC M / RI).
 *
 * Move cursor up, scroll down if at top.
 *
 * @param state - Terminal state accessor
 */
export function handleReverseIndex(state: TerminalStateAccessor): void {
  const buffer = state.getActiveBuffer();
  const { top } = buffer.getEffectiveScrollRegion();
  if (state.cursor.row === top) {
    buffer.scrollDown();
  } else if (state.cursor.row > 0) {
    state.cursor.moveUp();
  }
}

/**
 * Handle HorizontalTabSet (ESC H / HTS).
 *
 * Set tab stop at current cursor column.
 *
 * @param state - Terminal state accessor
 */
export function handleHorizontalTabSet(state: TerminalStateAccessor): void {
  state.tabStops.add(state.cursor.col);
  state.syncTabStopToWasm(state.cursor.col);
}

/**
 * Handle ResetToInitialState (ESC c / RIS).
 *
 * Reset terminal to initial state.
 *
 * @param state - Terminal state accessor
 */
export function handleResetToInitialState(state: TerminalStateAccessor): void {
  state.reset();
}

/**
 * Handle SetG0CharSet (ESC ( X).
 *
 * Set G0 character set.
 *
 * @param state - Terminal state accessor
 * @param charset - Character set to use
 */
export function handleSetG0CharSet(
  state: TerminalStateAccessor,
  charset: CharSet
): void {
  state.g0CharSet = charset;
}

/**
 * Handle SetG1CharSet (ESC ) X).
 *
 * Set G1 character set.
 *
 * @param state - Terminal state accessor
 * @param charset - Character set to use
 */
export function handleSetG1CharSet(
  state: TerminalStateAccessor,
  charset: CharSet
): void {
  state.g1CharSet = charset;
}
