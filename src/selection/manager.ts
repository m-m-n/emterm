/**
 * Selection state management for terminal text selection.
 * Tracks selection start/end positions and provides normalization.
 */

import type { GridPosition } from "./coords";

/**
 * Selection range in the terminal grid.
 */
export interface Selection {
	start: GridPosition;
	end: GridPosition;
}

/**
 * Manages terminal text selection state.
 *
 * Responsibilities:
 * - Track selection start and end positions
 * - Normalize selections (ensure start comes before end)
 * - Provide selection state queries
 *
 * @example
 * ```ts
 * const manager = new SelectionManager();
 * manager.startSelection(5, 10);
 * manager.updateSelection(15, 12);
 * const selection = manager.getSelection();
 * // selection = { start: { col: 5, row: 10 }, end: { col: 15, row: 12 } }
 * ```
 */
export class SelectionManager {
	private selection: Selection | null = null;

	/**
	 * Check if a selection is currently active.
	 */
	isActive(): boolean {
		return this.selection !== null;
	}

	/**
	 * Get the current selection.
	 * @returns Current selection or null if no selection is active
	 */
	getSelection(): Selection | null {
		return this.selection;
	}

	/**
	 * Start a new selection at the given grid position.
	 * Both start and end are initially set to the same position.
	 *
	 * @param col - Column position
	 * @param row - Row position
	 */
	startSelection(col: number, row: number): void {
		this.selection = {
			start: { col, row },
			end: { col, row },
		};
	}

	/**
	 * Update the end position of the current selection.
	 * Does nothing if no selection is active.
	 *
	 * @param col - Column position
	 * @param row - Row position
	 */
	updateSelection(col: number, row: number): void {
		if (!this.selection) {
			return;
		}

		this.selection.end = { col, row };
	}

	/**
	 * Clear the current selection.
	 */
	clearSelection(): void {
		this.selection = null;
	}

	/**
	 * Normalize the selection so that start comes before end in grid order.
	 * Returns a new Selection object with start <= end.
	 *
	 * Grid order: Earlier rows come first, then earlier columns on same row.
	 *
	 * @returns Normalized selection with start <= end
	 * @throws Error if no selection is active
	 *
	 * @example
	 * ```ts
	 * manager.startSelection(15, 12);
	 * manager.updateSelection(5, 10);
	 * const normalized = manager.normalizeSelection();
	 * // normalized = { start: { col: 5, row: 10 }, end: { col: 15, row: 12 } }
	 * ```
	 */
	normalizeSelection(): Selection {
		if (!this.selection) {
			throw new Error("No active selection to normalize");
		}

		const { start, end } = this.selection;

		// Check if we need to swap
		const needsSwap =
			start.row > end.row || (start.row === end.row && start.col > end.col);

		if (needsSwap) {
			return {
				start: end,
				end: start,
			};
		}

		return {
			start,
			end,
		};
	}
}
