/**
 * Unified buffer for terminal display.
 *
 * Replaces ScreenBuffer with a ring buffer that unifies scrollback and
 * screen lines into a single data structure. Enables full-buffer reflow
 * on resize with cursor position tracking.
 */
import { type Cell, type LineAccessor, createEmptyCell, Line } from "./grid.ts";
import { type WasmGrid, parsePackedRow } from "./wasm/terminal-core.ts";

/**
 * Scroll region definition.
 * Uses 0-indexed row numbers.
 */
export interface ScrollRegion {
	/** Top margin (inclusive, 0-indexed). */
	top: number;
	/** Bottom margin (inclusive, 0-indexed). */
	bottom: number;
}

/**
 * Unified buffer containing scrollback + screen lines in a ring buffer.
 */
export class UnifiedBuffer {
	/** Ring buffer storage. */
	private ring: (Line | null)[];

	/** Index of oldest line in ring buffer. */
	private head: number;

	/** Current number of lines in the buffer. */
	private _size: number;

	/** Maximum number of lines the buffer can hold. */
	private capacity: number;

	/** Number of columns. */
	private _cols: number;

	/** Number of viewport rows. */
	private _rows: number;

	/** Scroll region (null means full screen). */
	private scrollRegion: ScrollRegion | null = null;

	/** Whether this buffer allows scrollback (primary=true, alternate=false). */
	private allowScrollback: boolean;

	/** Optional WASM-backed viewport grid. When present, viewport data lives in WASM. */
	private wasmGrid: WasmGrid | null = null;

	/** Callback when lines are evicted from ring buffer on capacity overflow. */
	onEvict?: (count: number) => void;

	/**
	 * Create a new unified buffer.
	 *
	 * @param cols - Number of columns
	 * @param rows - Number of viewport rows
	 * @param scrollbackLines - Maximum scrollback lines (0 for alternate buffer)
	 * @param wasmGrid - Optional WASM grid for viewport backing
	 */
	constructor(cols: number, rows: number, scrollbackLines: number, wasmGrid?: WasmGrid) {
		this._cols = Math.max(1, cols);
		this._rows = Math.max(1, rows);
		this.allowScrollback = scrollbackLines > 0;
		this.wasmGrid = wasmGrid ?? null;

		if (this.wasmGrid) {
			// WASM Ring Buffer mode: scrollback is in WASM linear memory, no JS ring needed
			this.capacity = 0;
			this.ring = [];
			this.head = 0;
			this._size = 0;
		} else {
			// JS mode: ring buffer stores scrollback + viewport
			this.capacity = Math.max(1, scrollbackLines + this._rows);
			this.ring = new Array(this.capacity).fill(null);
			this.head = 0;
			this._size = 0;

			// Initialize viewport with empty lines
			for (let i = 0; i < rows; i++) {
				this.push(new Line(cols));
			}
		}
	}

	/** Get number of columns. */
	get cols(): number {
		return this._cols;
	}

	/** Get number of viewport rows. */
	get rows(): number {
		return this._rows;
	}

	/** Get current number of lines in buffer. */
	get size(): number {
		return this._size;
	}

	/** Get number of scrollback lines (lines above viewport). */
	get scrollbackLength(): number {
		if (this.wasmGrid) {
			// WASM mode: scrollback is managed by WASM ring buffer
			return this.wasmGrid.getScrollbackLength();
		}
		return Math.max(0, this._size - this._rows);
	}

	// ===== Ring Buffer Operations =====

	/**
	 * Push a line to the end of the ring buffer.
	 * If at capacity, the oldest line is evicted.
	 *
	 * @param line - Line to push
	 */
	push(line: Line): void {
		if (this._size < this.capacity) {
			// Buffer not full: append
			const index = (this.head + this._size) % this.capacity;
			this.ring[index] = line;
			this._size++;
		} else {
			// Buffer full: overwrite oldest (head advances)
			this.ring[this.head] = line;
			this.head = (this.head + 1) % this.capacity;
			if (this.onEvict) {
				this.onEvict(1);
			}
		}
	}

	/**
	 * Get a line by absolute index (0 = oldest line in buffer).
	 *
	 * @param index - Absolute index
	 * @returns Line at that index
	 */
	private getAbsolute(index: number): Line {
		if (index < 0 || index >= this._size) {
			throw new Error(`Absolute index ${index} out of bounds (0-${this._size - 1})`);
		}
		return this.ring[(this.head + index) % this.capacity]!;
	}

	/**
	 * Set a line by absolute index.
	 *
	 * @param index - Absolute index
	 * @param line - Line to set
	 */
	private setAbsolute(index: number, line: Line): void {
		if (index < 0 || index >= this._size) {
			throw new Error(`Absolute index ${index} out of bounds (0-${this._size - 1})`);
		}
		this.ring[(this.head + index) % this.capacity] = line;
	}

	/**
	 * Drain all lines from the ring buffer.
	 * Returns lines in order (oldest first) and resets the buffer.
	 *
	 * @returns Array of all lines
	 */
	drain(): Line[] {
		const lines: Line[] = [];
		for (let i = 0; i < this._size; i++) {
			lines.push(this.getAbsolute(i));
		}
		this.head = 0;
		this._size = 0;
		return lines;
	}

	// ===== Viewport Access =====

	/**
	 * Get a viewport line by row index (0 = top of screen).
	 *
	 * @param row - Row index within viewport
	 * @returns Line or WasmLineProxy at that row
	 * @throws Error if row is out of bounds
	 */
	getLine(row: number): LineAccessor {
		if (row < 0 || row >= this._rows) {
			throw new Error(`Row ${row} out of bounds (0-${this._rows - 1})`);
		}
		if (this.wasmGrid) {
			return this.wasmGrid.getLine(row);
		}
		return this.getAbsolute(this.scrollbackLength + row);
	}

	/**
	 * Get a cell at the specified viewport position.
	 *
	 * @param col - Column index
	 * @param row - Row index within viewport
	 * @returns Cell at that position
	 */
	getCell(col: number, row: number): Cell {
		return this.getLine(row).getCell(col);
	}

	/**
	 * Set a cell at the specified viewport position.
	 *
	 * @param col - Column index
	 * @param row - Row index within viewport
	 * @param cell - Cell to set
	 */
	setCell(col: number, row: number, cell: Cell): void {
		if (this.wasmGrid) {
			this.wasmGrid.setCell(col, row, cell);
			return;
		}
		this.getLine(row).setCell(col, cell);
	}

	// ===== Packed Data Access =====

	/**
	 * Get packed binary data for a viewport row.
	 * Returns null if WASM grid is not available.
	 *
	 * @param row - Row index within viewport
	 * @returns Uint8Array of packed cell data or null
	 */
	getRowPacked(row: number): Uint8Array | null {
		if (!this.wasmGrid) return null;
		if (row < 0 || row >= this._rows) return null;
		return this.wasmGrid.getRowPacked(row);
	}

	/**
	 * Get packed binary data for a scrollback row.
	 * Returns null if WASM grid is not available.
	 *
	 * @param index - Scrollback index (0 = oldest)
	 * @returns Uint8Array of packed cell data or null
	 */
	getScrollbackRowPacked(index: number): Uint8Array | null {
		if (!this.wasmGrid) return null;
		if (index < 0 || index >= this.scrollbackLength) return null;
		return this.wasmGrid.getScrollbackRowPacked(index);
	}

	// ===== Scrollback Access =====

	/**
	 * Get a scrollback line by index (0 = oldest scrollback line).
	 *
	 * @param index - Scrollback index
	 * @returns Line at that scrollback position
	 */
	getScrollbackLine(index: number): Line {
		if (index < 0 || index >= this.scrollbackLength) {
			throw new Error(`Scrollback index ${index} out of bounds (0-${this.scrollbackLength - 1})`);
		}
		if (this.wasmGrid) {
			// WASM mode: read from WASM ring buffer
			const packed = this.wasmGrid.getScrollbackRowPacked(index);
			const line = parsePackedRow(packed, this._cols);
			line.wrapped = this.wasmGrid.getScrollbackLineWrapped(index);
			return line;
		}
		return this.getAbsolute(index);
	}

	// ===== Scroll Region =====

	/**
	 * Set the scroll region.
	 *
	 * @param top - Top margin (0-indexed, inclusive)
	 * @param bottom - Bottom margin (0-indexed, inclusive)
	 */
	setScrollRegion(top: number, bottom: number): void {
		if (top < 0) top = 0;
		if (bottom >= this._rows) bottom = this._rows - 1;

		if (top === 0 && bottom === this._rows - 1) {
			this.scrollRegion = null;
		} else if (top < bottom) {
			this.scrollRegion = { top, bottom };
		}

		// Sync to WASM scroll region (only valid regions)
		if (this.wasmGrid && top < bottom) {
			this.wasmGrid.core.set_scroll_region(top, bottom);
		}
	}

	/**
	 * Clear the scroll region (reset to full screen).
	 */
	clearScrollRegion(): void {
		this.scrollRegion = null;

		// Sync to WASM: full screen = (0, rows-1)
		if (this.wasmGrid) {
			this.wasmGrid.core.set_scroll_region(0, this._rows - 1);
		}
	}

	/**
	 * Get the current scroll region.
	 *
	 * @returns Scroll region or null if full screen
	 */
	getScrollRegion(): ScrollRegion | null {
		return this.scrollRegion;
	}

	/**
	 * Get effective scroll region bounds.
	 *
	 * @returns Effective scroll region (full screen if none set)
	 */
	getEffectiveScrollRegion(): ScrollRegion {
		return this.scrollRegion ?? { top: 0, bottom: this._rows - 1 };
	}

	// ===== Clear Operations =====

	/**
	 * Clear all viewport lines.
	 */
	clearAll(): void {
		for (let row = 0; row < this._rows; row++) {
			this.getLine(row).clear();
		}
	}

	/**
	 * Clear a single viewport line.
	 *
	 * @param row - Row index
	 */
	clearLine(row: number): void {
		if (row >= 0 && row < this._rows) {
			this.getLine(row).clear();
		}
	}

	/**
	 * Clear from cursor position to end of line.
	 *
	 * @param row - Row index
	 * @param col - Column index (starting point)
	 */
	clearLineFromCursor(row: number, col: number): void {
		if (row >= 0 && row < this._rows) {
			this.getLine(row).clearRange(col, this._cols);
		}
	}

	/**
	 * Clear from start of line to cursor position (inclusive).
	 *
	 * @param row - Row index
	 * @param col - Column index (end point, inclusive)
	 */
	clearLineToCursor(row: number, col: number): void {
		if (row >= 0 && row < this._rows) {
			this.getLine(row).clearRange(0, col + 1);
		}
	}

	/**
	 * Clear from cursor to end of screen (ED 0).
	 *
	 * @param col - Cursor column
	 * @param row - Cursor row
	 */
	clearBelow(col: number, row: number): void {
		this.clearLineFromCursor(row, col);
		for (let r = row + 1; r < this._rows; r++) {
			this.clearLine(r);
		}
	}

	/**
	 * Clear from start of screen to cursor (ED 1).
	 *
	 * @param col - Cursor column
	 * @param row - Cursor row
	 */
	clearAbove(col: number, row: number): void {
		for (let r = 0; r < row; r++) {
			this.clearLine(r);
		}
		this.clearLineToCursor(row, col);
	}

	/**
	 * Clear scrollback buffer, retaining only viewport lines.
	 * Used by ED 3 (Erase Scrollback).
	 */
	clearScrollback(): void {
		if (this.scrollbackLength === 0) return;

		if (this.wasmGrid) {
			// WASM mode: clear scrollback in WASM ring buffer
			this.wasmGrid.clearScrollback();
			return;
		}

		// JS mode: collect viewport lines and rebuild ring
		const viewportLines: Line[] = [];
		for (let row = 0; row < this._rows; row++) {
			viewportLines.push(this.getLine(row) as Line);
		}

		// Reset ring buffer with only viewport lines
		this.ring = new Array(this.capacity).fill(null);
		this.head = 0;
		this._size = 0;
		for (const line of viewportLines) {
			this.push(line);
		}
	}

	// ===== Scroll Operations =====

	/**
	 * Scroll the buffer up by the specified number of lines.
	 * Respects the scroll region if set.
	 *
	 * For full-screen scroll (top=0 AND bottom=rows-1): pushes blank lines
	 * to the ring buffer, making old top lines become scrollback implicitly.
	 *
	 * For partial scroll region: rearranges lines in-place within the region.
	 *
	 * @param count - Number of lines to scroll (default: 1)
	 */
	scrollUp(count: number = 1): void {
		if (count <= 0) return;

		const { top, bottom } = this.getEffectiveScrollRegion();
		const regionHeight = bottom - top + 1;
		const actualCount = Math.min(count, regionHeight);

		if (this.wasmGrid) {
			// WASM Ring Buffer mode: scroll handled entirely within WASM
			this.wasmGrid.core.handle_scroll_up(actualCount);
			return;
		}

		// JS mode: original implementation
		// Full-screen scroll: use ring buffer push (implicit scrollback)
		if (top === 0 && bottom === this._rows - 1) {
			for (let i = 0; i < actualCount; i++) {
				this.push(new Line(this._cols));
			}
			// Mark all viewport lines as dirty
			for (let r = 0; r < this._rows; r++) {
				this.getLine(r).dirty = true;
			}
			return;
		}

		// Partial scroll region: rearrange viewport lines in-place
		const sbLen = this.scrollbackLength;

		// Shift remaining lines up within region
		for (let i = top; i <= bottom - actualCount; i++) {
			this.setAbsolute(sbLen + i, this.getAbsolute(sbLen + i + actualCount));
		}

		// Insert blank lines at bottom of region
		for (let i = 0; i < actualCount; i++) {
			this.setAbsolute(sbLen + bottom - actualCount + 1 + i, new Line(this._cols));
		}

		// Mark affected lines as dirty
		for (let i = top; i <= bottom; i++) {
			this.getLine(i).dirty = true;
		}
	}

	/**
	 * Scroll the buffer down by the specified number of lines.
	 * Respects the scroll region if set.
	 * New empty lines are added at the top of the region.
	 *
	 * @param count - Number of lines to scroll (default: 1)
	 */
	scrollDown(count: number = 1): void {
		if (count <= 0) return;

		const { top, bottom } = this.getEffectiveScrollRegion();
		const regionHeight = bottom - top + 1;
		const actualCount = Math.min(count, regionHeight);

		if (this.wasmGrid) {
			// WASM Ring Buffer mode: scroll handled entirely within WASM
			this.wasmGrid.core.handle_scroll_down(actualCount);
			return;
		}

		// JS mode: original implementation
		const sbLen = this.scrollbackLength;

		// Shift lines down within region
		for (let i = bottom; i >= top + actualCount; i--) {
			this.setAbsolute(sbLen + i, this.getAbsolute(sbLen + i - actualCount));
		}

		// Insert blank lines at top of region
		for (let i = 0; i < actualCount; i++) {
			this.setAbsolute(sbLen + top + i, new Line(this._cols));
		}

		// Mark affected lines as dirty
		for (let i = top; i <= bottom; i++) {
			this.getLine(i).dirty = true;
		}
	}

	// ===== Line Manipulation =====

	/**
	 * Insert blank lines at the specified row.
	 * Lines below are pushed down within the scroll region.
	 *
	 * @param row - Row to insert at
	 * @param count - Number of lines to insert
	 */
	insertLines(row: number, count: number = 1): void {
		if (count <= 0) return;

		const { top, bottom } = this.getEffectiveScrollRegion();
		if (row < top || row > bottom) return;

		const actualCount = Math.min(count, bottom - row + 1);

		if (this.wasmGrid) {
			// WASM mode: shift rows down from cursor, fill with defaults
			this.wasmGrid.shiftRowsDown(row, bottom, actualCount);
			for (let i = 0; i < actualCount; i++) {
				this.wasmGrid.fillRowDefault(row + i);
			}
			for (let i = row; i <= bottom; i++) {
				this.wasmGrid.markRowDirty(i);
			}
			return;
		}

		// JS mode
		const sbLen = this.scrollbackLength;

		// Shift lines down within region (from bottom up)
		for (let i = bottom; i >= row + actualCount; i--) {
			this.setAbsolute(sbLen + i, this.getAbsolute(sbLen + i - actualCount));
		}

		// Insert blank lines at cursor row
		for (let i = 0; i < actualCount; i++) {
			this.setAbsolute(sbLen + row + i, new Line(this._cols));
		}

		// Mark affected lines as dirty
		for (let i = row; i <= bottom; i++) {
			this.getLine(i).dirty = true;
		}
	}

	/**
	 * Delete lines at the specified row.
	 * Lines below are pulled up within the scroll region.
	 *
	 * @param row - Row to delete from
	 * @param count - Number of lines to delete
	 */
	deleteLines(row: number, count: number = 1): void {
		if (count <= 0) return;

		const { top, bottom } = this.getEffectiveScrollRegion();
		if (row < top || row > bottom) return;

		const actualCount = Math.min(count, bottom - row + 1);

		if (this.wasmGrid) {
			// WASM mode: shift rows up from cursor, fill bottom with defaults
			this.wasmGrid.shiftRowsUp(row, bottom, actualCount);
			for (let i = 0; i < actualCount; i++) {
				this.wasmGrid.fillRowDefault(bottom - actualCount + 1 + i);
			}
			for (let i = row; i <= bottom; i++) {
				this.wasmGrid.markRowDirty(i);
			}
			return;
		}

		// JS mode
		const sbLen = this.scrollbackLength;

		// Shift lines up within region
		for (let i = row; i <= bottom - actualCount; i++) {
			this.setAbsolute(sbLen + i, this.getAbsolute(sbLen + i + actualCount));
		}

		// Add blank lines at bottom of region
		for (let i = 0; i < actualCount; i++) {
			this.setAbsolute(sbLen + bottom - actualCount + 1 + i, new Line(this._cols));
		}

		// Mark affected lines as dirty
		for (let i = row; i <= bottom; i++) {
			this.getLine(i).dirty = true;
		}
	}

	// ===== Character Manipulation =====

	/**
	 * Insert blank characters at the specified position.
	 * Characters to the right are shifted right.
	 *
	 * @param row - Row index
	 * @param col - Column to insert at
	 * @param count - Number of characters to insert
	 */
	insertCharacters(row: number, col: number, count: number = 1): void {
		if (count <= 0 || row < 0 || row >= this._rows) return;
		if (col < 0 || col >= this._cols) return;

		const line = this.getLine(row);
		const actualCount = Math.min(count, this._cols - col);

		// Shift characters to the right (starting from the end)
		for (let i = this._cols - 1; i >= col + actualCount; i--) {
			const srcCell = line.getCell(i - actualCount);
			line.setCell(i, {
				char: srcCell.char,
				width: srcCell.width,
				attrs: { ...srcCell.attrs },
				dirty: true,
			});
		}

		// Insert blank characters
		for (let i = col; i < col + actualCount; i++) {
			line.setCell(i, createEmptyCell());
		}

		line.dirty = true;
	}

	/**
	 * Delete characters at the specified position.
	 * Characters to the right are shifted left.
	 *
	 * @param row - Row index
	 * @param col - Column to delete from
	 * @param count - Number of characters to delete
	 */
	deleteCharacters(row: number, col: number, count: number = 1): void {
		if (count <= 0 || row < 0 || row >= this._rows) return;
		if (col < 0 || col >= this._cols) return;

		const line = this.getLine(row);
		const actualCount = Math.min(count, this._cols - col);

		// Shift characters to the left
		for (let i = col; i < this._cols - actualCount; i++) {
			const srcCell = line.getCell(i + actualCount);
			line.setCell(i, {
				char: srcCell.char,
				width: srcCell.width,
				attrs: { ...srcCell.attrs },
				dirty: true,
			});
		}

		// Fill the end with blank characters
		for (let i = this._cols - actualCount; i < this._cols; i++) {
			line.setCell(i, createEmptyCell());
		}

		line.dirty = true;
	}

	/**
	 * Erase characters at the specified position.
	 * Characters are replaced with blanks but not shifted.
	 *
	 * @param row - Row index
	 * @param col - Column to start erasing at
	 * @param count - Number of characters to erase
	 */
	eraseCharacters(row: number, col: number, count: number = 1): void {
		if (count <= 0 || row < 0 || row >= this._rows) return;
		if (col < 0 || col >= this._cols) return;

		const line = this.getLine(row);
		const endCol = Math.min(col + count, this._cols);

		for (let i = col; i < endCol; i++) {
			line.setCell(i, createEmptyCell());
		}

		line.dirty = true;
	}

	// ===== Dirty Tracking =====

	/**
	 * Get indices of dirty viewport rows.
	 *
	 * @returns Array of row indices that are dirty
	 */
	getDirtyRows(): number[] {
		if (this.wasmGrid) {
			return Array.from(this.wasmGrid.getDirtyRows());
		}
		const dirty: number[] = [];
		for (let i = 0; i < this._rows; i++) {
			if (this.getLine(i).dirty) {
				dirty.push(i);
			}
		}
		return dirty;
	}

	/**
	 * Clear the dirty flag on all viewport lines.
	 */
	clearAllDirty(): void {
		if (this.wasmGrid) {
			this.wasmGrid.clearDirty();
			return;
		}
		for (let i = 0; i < this._rows; i++) {
			this.getLine(i).clearDirty();
		}
	}

	// ===== Resize with Reflow =====

	/**
	 * Resize the buffer with full-buffer reflow and cursor tracking.
	 *
	 * Algorithm:
	 * 1. Drain all lines from ring buffer
	 * 2. Join wrapped lines into logical lines (tracking cursor position)
	 * 3. Re-split logical lines at new width
	 * 4. Trim empty lines from bottom if total exceeds new rows
	 * 5. Write reflowed lines back to ring buffer
	 * 6. Return adjusted cursor position
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 * @param cursorRow - Current cursor row (viewport-relative)
	 * @param cursorCol - Current cursor column
	 * @returns Adjusted cursor position { col, row }
	 */
	resize(
		cols: number,
		rows: number,
		cursorRow: number,
		cursorCol: number,
	): { col: number; row: number } {
		cols = Math.max(1, cols);
		rows = Math.max(1, rows);
		const oldCols = this._cols;

		if (this.wasmGrid) {
			return this.resizeWithWasm(cols, rows, cursorRow, cursorCol);
		}

		// Same width: skip reflow, just adjust row count and resize lines
		if (cols === oldCols) {
			// Resize all lines to same width (no-op for same cols, but defensive)
			for (let i = 0; i < this._size; i++) {
				this.getAbsolute(i).resize(cols);
			}
			return this.adjustRowCount(rows, cursorRow, cursorCol);
		}

		// Full reflow: drain, join, re-split, refill
		const allLines = this.drain();

		// Map cursor from viewport-relative to absolute index in drained array
		const scrollbackLen = Math.max(0, allLines.length - this._rows);
		const cursorAbsRow = scrollbackLen + cursorRow;

		// Phase 1: Join wrapped lines into logical lines with cursor tracking
		const reflowed: Line[] = [];
		let adjustedCursor = { col: cursorCol, row: cursorRow };
		let outputLineCount = 0;

		let i = 0;
		while (i < allLines.length) {
			// Collect all cells from a logical line
			const logicalCells: Cell[] = [];
			let cursorInThisLogical = false;
			let logicalCursorX = 0;

			// First physical line of this logical group
			if (i === cursorAbsRow) {
				cursorInThisLogical = true;
				logicalCursorX = cursorCol;
			}
			logicalCells.push(...allLines[i]!.getCells());
			i++;

			// Merge continuation lines (wrapped = true means "I am a continuation")
			while (i < allLines.length && allLines[i]!.wrapped) {
				if (i === cursorAbsRow) {
					cursorInThisLogical = true;
					logicalCursorX = cursorCol + logicalCells.length;
				}
				logicalCells.push(...allLines[i]!.getCells());
				i++;
			}

			// Trim trailing empty cells from logical line
			let endIndex = logicalCells.length;
			while (endIndex > 0) {
				const cell = logicalCells[endIndex - 1]!;
				if (cell.char !== " " || cell.width !== 1) break;
				endIndex--;
			}
			const trimmedCells = endIndex > 0 ? logicalCells.slice(0, endIndex) : [];

			// Phase 2: Re-split logical line at new width
			if (trimmedCells.length === 0) {
				// Empty logical line
				reflowed.push(new Line(cols));

				if (cursorInThisLogical) {
					adjustedCursor = { col: 0, row: outputLineCount };
				}
				outputLineCount++;
			} else {
				// Compute cursor's new position within this logical line
				if (cursorInThisLogical) {
					// Clamp cursor to within trimmed content (allow one past end)
					const clampedX = Math.min(logicalCursorX, trimmedCells.length);
					const newRow = Math.floor(clampedX / cols);
					const newCol = clampedX % cols;
					adjustedCursor = { col: newCol, row: outputLineCount + newRow };
				}

				let offset = 0;
				while (offset < trimmedCells.length) {
					const newLine = new Line(cols);
					const lineLength = Math.min(cols, trimmedCells.length - offset);

					for (let j = 0; j < lineLength; j++) {
						newLine.setCell(j, trimmedCells[offset + j]!);
					}

					if (offset > 0) {
						newLine.wrapped = true;
					}

					reflowed.push(newLine);
					offset += lineLength;
				}
				outputLineCount += Math.ceil(trimmedCells.length / cols);
			}
		}

		// Phase 3: Trim empty lines from bottom (only if we have more than rows)
		while (reflowed.length > rows) {
			const lastLine = reflowed[reflowed.length - 1]!;
			if (lastLine.isEmpty()) {
				reflowed.pop();
				// Adjust cursor if it was on a trimmed line
				if (adjustedCursor.row >= reflowed.length) {
					adjustedCursor.row = Math.max(0, reflowed.length - 1);
					adjustedCursor.col = 0;
				}
			} else {
				break;
			}
		}

		// Ensure we have at least `rows` lines
		while (reflowed.length < rows) {
			reflowed.push(new Line(cols));
		}

		// Phase 4: Refill ring buffer
		const scrollbackLines = this.capacity - this._rows; // preserve original scrollback capacity
		this.capacity = scrollbackLines + rows;
		this._cols = cols;
		this._rows = rows;
		this.ring = new Array(this.capacity).fill(null);
		this.head = 0;
		this._size = 0;

		// If reflowed lines exceed capacity, only keep the last `capacity` lines
		const startIndex = Math.max(0, reflowed.length - this.capacity);
		const evicted = startIndex;

		for (let k = startIndex; k < reflowed.length; k++) {
			this.ring[this._size] = reflowed[k]!;
			this._size++;
		}

		// Notify eviction callback for truncated lines
		if (evicted > 0 && this.onEvict) {
			this.onEvict(evicted);
		}

		// Adjust cursor for evicted lines
		if (evicted > 0) {
			adjustedCursor.row -= evicted;
		}

		// Convert cursor from absolute to viewport-relative
		const newScrollbackLen = this.scrollbackLength;
		adjustedCursor.row -= newScrollbackLen;

		// Clamp cursor within viewport bounds
		adjustedCursor.row = Math.max(0, Math.min(adjustedCursor.row, rows - 1));
		adjustedCursor.col = Math.max(0, Math.min(adjustedCursor.col, cols - 1));

		// Invalidate scroll region
		this.scrollRegion = null;

		// Mark all lines as dirty
		for (let k = 0; k < this._rows; k++) {
			this.getLine(k).dirty = true;
		}

		return adjustedCursor;
	}

	/**
	 * Resize lines in-place without reflow (for alternate buffer).
	 * Lines are resized to new width, rows added/removed as needed.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resizeNoReflow(cols: number, rows: number): void {
		cols = Math.max(1, cols);
		rows = Math.max(1, rows);

		if (this.wasmGrid) {
			// WASM mode: delegate to resize_no_reflow (no reflow for alternate buffer)
			this.wasmGrid.resizeNoReflow(cols, rows);
			this._cols = cols;
			this._rows = rows;
			// Invalidate scroll region if needed
			if (this.scrollRegion && this.scrollRegion.bottom >= rows) {
				this.scrollRegion = null;
			}
			this.wasmGrid.markAllDirty();
			return;
		}

		// JS mode: original implementation
		// Collect current viewport lines
		const currentLines: Line[] = [];
		for (let i = 0; i < this._rows; i++) {
			currentLines.push(this.getLine(i) as Line);
		}

		// Resize existing lines to new width
		for (const line of currentLines) {
			line.resize(cols);
		}

		// Rebuild ring buffer with new capacity
		this.capacity = rows;
		this.ring = new Array(this.capacity).fill(null);
		this.head = 0;
		this._size = 0;
		this._cols = cols;
		this._rows = rows;

		// Re-add existing lines (up to new row count)
		const linesToKeep = Math.min(currentLines.length, rows);
		for (let i = 0; i < linesToKeep; i++) {
			this.ring[this._size] = currentLines[i]!;
			this._size++;
		}

		// Add blank lines if new rows > old rows
		for (let i = linesToKeep; i < rows; i++) {
			this.ring[this._size] = new Line(cols);
			this._size++;
		}

		// Invalidate scroll region if needed
		if (this.scrollRegion && this.scrollRegion.bottom >= rows) {
			this.scrollRegion = null;
		}

		// Mark all dirty
		for (let i = 0; i < this._rows; i++) {
			this.getLine(i).dirty = true;
		}
	}

	/**
	 * Adjust row count without reflow (when cols unchanged).
	 * Trims empty lines from bottom first on shrink.
	 */
	private adjustRowCount(
		rows: number,
		cursorRow: number,
		cursorCol: number,
	): { col: number; row: number } {
		const oldRows = this._rows;

		if (rows > this._rows) {
			// Add blank lines at bottom
			const toAdd = rows - this._rows;
			for (let i = 0; i < toAdd; i++) {
				this.push(new Line(this._cols));
			}
		} else if (rows < this._rows) {
			// Trim empty lines from bottom first
			let trimmed = 0;
			const toRemove = this._rows - rows;
			while (trimmed < toRemove) {
				const lastViewportRow = this._rows - 1 - trimmed;
				if (lastViewportRow <= cursorRow) break; // Don't trim at or above cursor
				const line = this.getLine(lastViewportRow);
				if (line.isEmpty()) {
					// Remove this empty line from end of ring
					this._size--;
					trimmed++;
				} else {
					break;
				}
			}
			// If still need to remove, let excess become scrollback
			// (no need to do anything - they're already in the ring)
		}

		this._rows = rows;

		// Recalculate capacity: preserve original scrollback limit
		const scrollbackCapacity = this.capacity - oldRows;
		const newCapacity = scrollbackCapacity + rows;

		if (newCapacity !== this.capacity) {
			// Rebuild ring buffer with new capacity
			// Read from old ring before switching capacity
			const oldRing = this.ring;
			const oldHead = this.head;
			const oldCapacity = this.capacity;
			const oldSize = this._size;

			const startIndex = Math.max(0, oldSize - newCapacity);
			const evicted = startIndex;

			const newRing: (Line | null)[] = new Array(newCapacity).fill(null);
			let newSize = 0;
			for (let i = startIndex; i < oldSize; i++) {
				newRing[newSize] = oldRing[(oldHead + i) % oldCapacity]!;
				newSize++;
			}

			this.ring = newRing;
			this.head = 0;
			this._size = newSize;
			this.capacity = newCapacity;

			if (evicted > 0 && this.onEvict) {
				this.onEvict(evicted);
			}
		}

		// Invalidate scroll region
		if (this.scrollRegion && this.scrollRegion.bottom >= rows) {
			this.scrollRegion = null;
		}

		// Mark all dirty
		for (let i = 0; i < this._rows; i++) {
			this.getLine(i).dirty = true;
		}

		// Clamp cursor
		const newRow = Math.max(0, Math.min(cursorRow, rows - 1));
		const newCol = Math.max(0, Math.min(cursorCol, this._cols - 1));
		return { col: newCol, row: newRow };
	}

	/**
	 * Resize with WASM grid: delegate entirely to WASM resize_reflow.
	 * WASM handles scrollback reflow, cursor tracking, and ring buffer management.
	 */
	private resizeWithWasm(
		cols: number,
		rows: number,
		cursorRow: number,
		cursorCol: number,
	): { col: number; row: number } {
		const grid = this.wasmGrid!;

		// Sync cursor to WASM before resize
		grid.core.set_cursor(cursorCol, cursorRow);

		// Delegate entirely to WASM resize (internally calls resize_reflow)
		grid.resize(cols, rows);

		// Read back adjusted cursor from WASM
		const newCol = grid.core.get_cursor_col();
		const newRow = grid.core.get_cursor_row();

		// Update TS state
		this._cols = cols;
		this._rows = rows;

		// Invalidate scroll region
		this.scrollRegion = null;

		grid.markAllDirty();

		return { col: newCol, row: newRow };
	}

	// ===== Buffer Utilities =====

	/**
	 * Clone this buffer (deep copy).
	 *
	 * @returns New independent buffer
	 */
	clone(): UnifiedBuffer {
		const newBuf = new UnifiedBuffer(this._cols, 0, 0);
		// Manually set internals for an exact copy
		newBuf.ring = new Array(this.capacity).fill(null);
		newBuf.capacity = this.capacity;
		newBuf._cols = this._cols;
		newBuf._rows = this._rows;
		newBuf.head = 0;
		newBuf._size = this._size;
		newBuf.allowScrollback = this.allowScrollback;
		newBuf.scrollRegion = this.scrollRegion ? { ...this.scrollRegion } : null;

		// Deep copy all lines
		for (let i = 0; i < this._size; i++) {
			newBuf.ring[i] = this.getAbsolute(i).clone();
		}

		return newBuf;
	}
}
