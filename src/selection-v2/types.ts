/**
 * Type definitions for selection system.
 */

/**
 * Grid position in the terminal (column and row).
 */
export interface GridPosition {
	col: number;
	row: number;
}

/**
 * Selection range with start and end positions.
 */
export interface SelectionRange {
	start: GridPosition;
	end: GridPosition;
}

/**
 * Selection mode.
 * - "none": No selection active
 * - "char": Character-by-character selection (normal drag)
 * - "word": Word-level selection (double-click)
 * - "line": Line-level selection (triple-click)
 */
export type SelectionMode = "none" | "char" | "word" | "line";

/**
 * Selection state.
 */
export interface SelectionState {
	/** Current selection range or null if no selection */
	range: SelectionRange | null;
	/** Selection mode */
	mode: SelectionMode;
	/** Whether currently selecting (dragging) */
	isSelecting: boolean;
}

/**
 * Selection event types.
 */
export type SelectionEventType = "start" | "update" | "end" | "clear";

/**
 * Selection event.
 */
export interface SelectionEvent {
	type: SelectionEventType;
	range: SelectionRange | null;
	mode: SelectionMode;
}

/**
 * Selection event listener.
 */
export type SelectionEventListener = (event: SelectionEvent) => void;

/**
 * Normalize a selection range so that start comes before end.
 * @param range - Selection range to normalize
 * @returns Normalized range with start <= end in grid order
 */
export function normalizeRange(range: SelectionRange): SelectionRange {
	const { start, end } = range;

	// Check if we need to swap (start is after end in grid order)
	const needsSwap =
		start.row > end.row || (start.row === end.row && start.col > end.col);

	if (needsSwap) {
		return { start: end, end: start };
	}

	return { start, end };
}

/**
 * Check if a position is within a selection range.
 * @param pos - Position to check
 * @param range - Selection range (should be normalized)
 * @returns True if position is within the range
 */
export function isPositionInRange(
	pos: GridPosition,
	range: SelectionRange,
): boolean {
	const { start, end } = range;

	// Check row bounds
	if (pos.row < start.row || pos.row > end.row) {
		return false;
	}

	// Single row case
	if (start.row === end.row) {
		return pos.col >= start.col && pos.col <= end.col;
	}

	// First row - must be after start col
	if (pos.row === start.row) {
		return pos.col >= start.col;
	}

	// Last row - must be before end col
	if (pos.row === end.row) {
		return pos.col <= end.col;
	}

	// Middle rows - always in range
	return true;
}
