/**
 * ANSI sequence semantics utilities.
 *
 * Provides default values for CSI parameters and coordinate utilities.
 */

/**
 * Default parameter values for CSI sequences.
 *
 * Most CSI commands default to 1 when no parameter is provided.
 */
export const CSI_DEFAULTS = {
  // Cursor movement
  CursorUp: 1,
  CursorDown: 1,
  CursorForward: 1,
  CursorBack: 1,
  CursorNextLine: 1,
  CursorPreviousLine: 1,

  // Absolute positioning
  CursorHorizontalAbsolute: 1,
  CursorVerticalAbsolute: 1,

  // Erase operations
  EraseCharacters: 1,

  // Insert/delete operations
  InsertLines: 1,
  DeleteLines: 1,
  InsertCharacters: 1,
  DeleteCharacters: 1,

  // Scroll operations
  ScrollUp: 1,
  ScrollDown: 1,
} as const;

/**
 * Convert 1-indexed ANSI value to 0-indexed internal value.
 *
 * ANSI terminals use 1-indexed coordinates, while our internal
 * representation uses 0-indexed. This function also handles:
 * - undefined -> treated as 1 (default)
 * - 0 -> treated as 1 (ANSI behavior)
 * - negative -> clamped to 0
 *
 * @param value - The 1-indexed ANSI value (or undefined/0)
 * @returns 0-indexed internal value
 */
export function toZeroIndexed(value: number | undefined): number {
  if (value === undefined || value <= 0) {
    return 0;
  }
  return value - 1;
}

/**
 * Clamp coordinates to valid screen bounds.
 *
 * Ensures both column and row are within the screen dimensions.
 *
 * @param col - Column (0-indexed)
 * @param row - Row (0-indexed)
 * @param cols - Total number of columns
 * @param rows - Total number of rows
 * @returns Clamped coordinates
 */
export function clampPosition(
  col: number,
  row: number,
  cols: number,
  rows: number
): { col: number; row: number } {
  return {
    col: Math.max(0, Math.min(col, cols - 1)),
    row: Math.max(0, Math.min(row, rows - 1)),
  };
}
