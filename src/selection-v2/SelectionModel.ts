/**
 * Selection state model.
 *
 * Pure data model for managing selection state.
 * Emits events when selection changes.
 */

import type {
	GridPosition,
	SelectionEvent,
	SelectionEventListener,
	SelectionMode,
	SelectionRange,
	SelectionState,
} from "./types";
import { normalizeRange } from "./types";

/**
 * Selection state model.
 *
 * Manages selection state and emits events on changes.
 * Uses observer pattern for loose coupling with UI components.
 *
 * @example
 * ```ts
 * const model = new SelectionModel();
 *
 * // Subscribe to changes
 * const unsubscribe = model.subscribe((event) => {
 *   console.log("Selection changed:", event.type);
 * });
 *
 * // Start selection
 * model.startSelection({ col: 5, row: 10 }, "char");
 *
 * // Update as mouse moves
 * model.updateSelection({ col: 15, row: 12 });
 *
 * // End selection
 * model.endSelection();
 * ```
 */
export class SelectionModel {
	private state: SelectionState;
	private listeners: Set<SelectionEventListener>;

	/**
	 * Create a new SelectionModel.
	 */
	constructor() {
		this.state = {
			range: null,
			mode: "none",
			isSelecting: false,
		};
		this.listeners = new Set();
	}

	/**
	 * Start a new selection.
	 *
	 * @param pos - Starting grid position
	 * @param mode - Selection mode (char, word, or line)
	 */
	startSelection(pos: GridPosition, mode: SelectionMode = "char"): void {
		this.state = {
			range: {
				start: { ...pos },
				end: { ...pos },
			},
			mode,
			isSelecting: true,
		};

		this.emit({
			type: "start",
			range: this.state.range,
			mode: this.state.mode,
		});
	}

	/**
	 * Update the selection end position.
	 *
	 * Does nothing if not currently selecting.
	 *
	 * @param pos - New end position
	 */
	updateSelection(pos: GridPosition): void {
		if (!this.state.isSelecting || !this.state.range) {
			return;
		}

		this.state.range.end = { ...pos };

		this.emit({
			type: "update",
			range: this.state.range,
			mode: this.state.mode,
		});
	}

	/**
	 * End the current selection.
	 *
	 * The selection remains visible but is no longer being actively modified.
	 */
	endSelection(): void {
		if (!this.state.isSelecting) {
			return;
		}

		this.state.isSelecting = false;

		this.emit({
			type: "end",
			range: this.state.range,
			mode: this.state.mode,
		});
	}

	/**
	 * Clear the current selection.
	 */
	clearSelection(): void {
		if (this.state.range === null && this.state.mode === "none") {
			return; // Already cleared
		}

		this.state = {
			range: null,
			mode: "none",
			isSelecting: false,
		};

		this.emit({
			type: "clear",
			range: null,
			mode: "none",
		});
	}

	/**
	 * Set a selection range directly.
	 *
	 * Useful for programmatic selection (e.g., word/line selection).
	 *
	 * @param range - Selection range
	 * @param mode - Selection mode
	 */
	setSelection(range: SelectionRange, mode: SelectionMode = "char"): void {
		this.state = {
			range: { ...range },
			mode,
			isSelecting: false,
		};

		this.emit({
			type: "start",
			range: this.state.range,
			mode: this.state.mode,
		});

		this.emit({
			type: "end",
			range: this.state.range,
			mode: this.state.mode,
		});
	}

	/**
	 * Get the current selection range, normalized so start comes before end.
	 *
	 * @returns Normalized selection range or null if no selection
	 */
	getNormalizedRange(): SelectionRange | null {
		if (!this.state.range) {
			return null;
		}
		return normalizeRange(this.state.range);
	}

	/**
	 * Get the current selection state.
	 *
	 * @returns Read-only selection state
	 */
	getState(): Readonly<SelectionState> {
		return this.state;
	}

	/**
	 * Check if a selection is currently active.
	 *
	 * @returns True if there is an active selection
	 */
	hasSelection(): boolean {
		return this.state.range !== null;
	}

	/**
	 * Check if currently in the process of selecting.
	 *
	 * @returns True if actively selecting (mouse button held)
	 */
	isActivelySelecting(): boolean {
		return this.state.isSelecting;
	}

	/**
	 * Subscribe to selection events.
	 *
	 * @param listener - Event listener function
	 * @returns Unsubscribe function
	 */
	subscribe(listener: SelectionEventListener): () => void {
		this.listeners.add(listener);
		return () => {
			this.listeners.delete(listener);
		};
	}

	/**
	 * Emit a selection event to all listeners.
	 */
	private emit(event: SelectionEvent): void {
		for (const listener of this.listeners) {
			listener(event);
		}
	}
}
