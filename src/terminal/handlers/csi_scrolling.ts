/**
 * CSI scrolling handlers.
 *
 * Handles scroll operations and scroll region setting.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { CSI_DEFAULTS, toZeroIndexed } from "./semantics.ts";

/**
 * Handle ScrollUp (CSI Ps S).
 *
 * Scroll content up by count lines.
 * New empty lines are added at the bottom.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to scroll (default: 1)
 */
export function handleScrollUp(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.scrollUp(count ?? CSI_DEFAULTS.ScrollUp);
}

/**
 * Handle ScrollDown (CSI Ps T).
 *
 * Scroll content down by count lines.
 * New empty lines are added at the top.
 *
 * @param state - Terminal state accessor
 * @param count - Number of lines to scroll (default: 1)
 */
export function handleScrollDown(
  state: TerminalStateAccessor,
  count: number | undefined
): void {
  const buffer = state.getActiveBuffer();
  buffer.scrollDown(count ?? CSI_DEFAULTS.ScrollDown);
}

/**
 * Handle SetScrollRegion (CSI top ; bottom r / DECSTBM).
 *
 * Set the scrolling region. The cursor is moved to home position.
 *
 * @param state - Terminal state accessor
 * @param top - Top margin (1-indexed)
 * @param bottom - Bottom margin (1-indexed, 0 means use screen height)
 */
export function handleSetScrollRegion(
  state: TerminalStateAccessor,
  top: number | undefined,
  bottom: number | undefined
): void {
  const buffer = state.getActiveBuffer();

  const topRow = toZeroIndexed(top ?? 1);
  // bottom 0 means use screen height
  const bottomRow = bottom === 0 || bottom === undefined
    ? state.rows - 1
    : toZeroIndexed(bottom);

  buffer.setScrollRegion(topRow, bottomRow);

  // DECSTBM moves cursor to home position
  state.cursor.moveTo(0, 0);
  state.wrapPending = false;
}
