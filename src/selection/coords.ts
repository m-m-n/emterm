/**
 * Coordinate system utilities for terminal grid.
 * Converts pixel coordinates to grid positions (column, row).
 */

/**
 * Grid position in the terminal (column and row).
 */
export interface GridPosition {
	col: number;
	row: number;
}

/**
 * Convert pixel coordinates to grid position.
 *
 * @param x - Pixel x coordinate
 * @param y - Pixel y coordinate
 * @param charWidth - Width of a single character in pixels
 * @param charHeight - Height of a single character in pixels
 * @param maxCols - Maximum number of columns in the grid
 * @param maxRows - Maximum number of rows in the grid
 * @returns Grid position clamped to terminal bounds
 *
 * @example
 * ```ts
 * const pos = coordsToGrid(100, 50, 10, 20, 80, 40);
 * // Returns { col: 10, row: 2 }
 * ```
 */
export function coordsToGrid(
	x: number,
	y: number,
	charWidth: number,
	charHeight: number,
	maxCols: number,
	maxRows: number,
): GridPosition {
	// Validate character dimensions to prevent division by zero/NaN
	if (
		!Number.isFinite(charWidth) ||
		!Number.isFinite(charHeight) ||
		charWidth <= 0 ||
		charHeight <= 0
	) {
		// Return safe default position if invalid dimensions
		return { col: 0, row: 0 };
	}

	// Calculate grid position
	const col = Math.floor(x / charWidth);
	const row = Math.floor(y / charHeight);

	// Clamp to grid bounds
	const clampedCol = Math.max(0, Math.min(col, maxCols - 1));
	const clampedRow = Math.max(0, Math.min(row, maxRows - 1));

	return {
		col: clampedCol,
		row: clampedRow,
	};
}
