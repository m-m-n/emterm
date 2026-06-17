/**
 * Cursor state management for the terminal.
 */
import {
	type CellAttributes,
	cloneAttributes,
	createDefaultAttributes,
} from "./attributes.ts";

/**
 * Cursor style.
 */
export type CursorStyle = "block" | "underline" | "bar";

/**
 * WASM-backed cursor position storage interface.
 * Structurally compatible with TerminalCore from wasm_bindgen.
 */
export interface WasmCursorCore {
	get_cursor_col(): number;
	get_cursor_row(): number;
	set_cursor_col(col: number): void;
	set_cursor_row(row: number): void;
}

/**
 * Saved cursor state for ESC 7 / ESC 8.
 */
interface SavedCursor {
	col: number;
	row: number;
	attrs: CellAttributes;
}

/**
 * Cursor state and operations.
 *
 * When a WasmCursorCore is provided, col/row are delegated to WASM
 * linear memory for efficient cross-boundary access.
 */
export class CursorState {
	/** Backing field for column (used when no WASM core). */
	private _col: number = 0;

	/** Backing field for row (used when no WASM core). */
	private _row: number = 0;

	/** Optional WASM core for cursor position delegation. */
	private _core: WasmCursorCore | null;

	/** Terminal columns. */
	cols: number;

	/** Terminal rows. */
	rows: number;

	/** Current text attributes. */
	attrs: CellAttributes;

	/** Whether cursor is visible (managed by TerminalModes now). */
	visible: boolean = true;

	/** Cursor style. */
	style: CursorStyle = "block";

	/** Whether cursor should blink (managed by TerminalModes now). */
	blink: boolean = true;

	/** Tab stop interval. */
	private readonly tabWidth: number = 8;

	/** Saved cursor state. */
	private saved: SavedCursor | null = null;

	/** Current column position (0-indexed). Delegates to WASM when available. */
	get col(): number {
		return this._core ? this._core.get_cursor_col() : this._col;
	}

	set col(v: number) {
		if (this._core) {
			this._core.set_cursor_col(v);
		} else {
			this._col = v;
		}
	}

	/** Current row position (0-indexed). Delegates to WASM when available. */
	get row(): number {
		return this._core ? this._core.get_cursor_row() : this._row;
	}

	set row(v: number) {
		if (this._core) {
			this._core.set_cursor_row(v);
		} else {
			this._row = v;
		}
	}

	/**
	 * Create a new cursor state.
	 *
	 * @param cols - Number of columns
	 * @param rows - Number of rows
	 * @param core - Optional WASM core for position delegation
	 */
	constructor(cols: number, rows: number, core?: WasmCursorCore) {
		this._core = core ?? null;
		this.cols = cols;
		this.rows = rows;
		this.attrs = createDefaultAttributes();
	}

	/**
	 * Move cursor right.
	 *
	 * @param count - Number of columns to move (default: 1)
	 */
	moveRight(count: number = 1): void {
		this.col = Math.min(this.cols - 1, this.col + count);
	}

	/**
	 * Move cursor left.
	 *
	 * @param count - Number of columns to move (default: 1)
	 */
	moveLeft(count: number = 1): void {
		this.col = Math.max(0, this.col - count);
	}

	/**
	 * Move cursor down.
	 *
	 * @param count - Number of rows to move (default: 1)
	 */
	moveDown(count: number = 1): void {
		this.row = Math.min(this.rows - 1, this.row + count);
	}

	/**
	 * Move cursor up.
	 *
	 * @param count - Number of rows to move (default: 1)
	 */
	moveUp(count: number = 1): void {
		this.row = Math.max(0, this.row - count);
	}

	/**
	 * Move cursor to absolute position.
	 *
	 * @param col - Target column (0-indexed)
	 * @param row - Target row (0-indexed)
	 */
	moveTo(col: number, row: number): void {
		this.col = Math.max(0, Math.min(this.cols - 1, col));
		this.row = Math.max(0, Math.min(this.rows - 1, row));
	}

	/**
	 * Set cursor to absolute column position.
	 *
	 * @param col - Target column (0-indexed)
	 */
	setColumn(col: number): void {
		this.col = Math.max(0, Math.min(this.cols - 1, col));
	}

	/**
	 * Set cursor to absolute row position.
	 *
	 * @param row - Target row (0-indexed)
	 */
	setRow(row: number): void {
		this.row = Math.max(0, Math.min(this.rows - 1, row));
	}

	/**
	 * Carriage return - move to column 0.
	 */
	carriageReturn(): void {
		this.col = 0;
	}

	/**
	 * Line feed - move down one row.
	 * Respects scroll region bottom margin when provided.
	 *
	 * @param scrollBottom - Bottom margin of scroll region (0-indexed, inclusive).
	 *                       If omitted, uses the absolute screen bottom.
	 * @returns true if scrolling is needed (cursor at bottom margin)
	 */
	lineFeed(scrollBottom?: number): boolean {
		const bottom = scrollBottom ?? (this.rows - 1);
		if (this.row === bottom) {
			return true;
		}
		if (this.row < this.rows - 1) {
			this.row++;
		}
		return false;
	}

	/**
	 * Tab - move to next tab stop.
	 */
	tab(): void {
		const nextTab = Math.floor(this.col / this.tabWidth + 1) * this.tabWidth;
		this.col = Math.min(this.cols - 1, nextTab);
	}

	/**
	 * Backspace - move left one column.
	 */
	backspace(): void {
		this.col = Math.max(0, this.col - 1);
	}

	/**
	 * Resize terminal dimensions.
	 *
	 * @param cols - New number of columns
	 * @param rows - New number of rows
	 */
	resize(cols: number, rows: number): void {
		this.cols = cols;
		this.rows = rows;
		// Clamp cursor to new dimensions
		this.col = Math.min(this.col, cols - 1);
		this.row = Math.min(this.row, rows - 1);
	}

	/**
	 * Save cursor state (ESC 7).
	 */
	save(): void {
		this.saved = {
			col: this.col,
			row: this.row,
			attrs: cloneAttributes(this.attrs),
		};
	}

	/**
	 * Restore cursor state (ESC 8).
	 */
	restore(): void {
		if (this.saved) {
			this.col = this.saved.col;
			this.row = this.saved.row;
			this.attrs = cloneAttributes(this.saved.attrs);
		} else {
			// Reset to defaults if no saved state
			this.col = 0;
			this.row = 0;
			this.attrs = createDefaultAttributes();
		}
	}

	/**
	 * Reset cursor to initial state.
	 */
	reset(): void {
		this.col = 0;
		this.row = 0;
		this.attrs = createDefaultAttributes();
		this.visible = true;
		this.style = "block";
		this.blink = true;
		this.saved = null;
	}

	/**
	 * Clone this cursor state.
	 *
	 * @returns A new CursorState with the same values
	 */
	clone(): CursorState {
		const cloned = new CursorState(this.cols, this.rows);
		cloned.col = this.col;
		cloned.row = this.row;
		cloned.attrs = cloneAttributes(this.attrs);
		cloned.visible = this.visible;
		cloned.style = this.style;
		cloned.blink = this.blink;
		if (this.saved) {
			cloned.saved = {
				col: this.saved.col,
				row: this.saved.row,
				attrs: cloneAttributes(this.saved.attrs),
			};
		}
		return cloned;
	}

	/**
	 * Restore state from another cursor.
	 *
	 * @param other - The cursor state to restore from
	 */
	restoreFrom(other: CursorState): void {
		this.col = Math.min(other.col, this.cols - 1);
		this.row = Math.min(other.row, this.rows - 1);
		this.attrs = cloneAttributes(other.attrs);
		this.visible = other.visible;
		this.style = other.style;
		this.blink = other.blink;
		if (other.saved) {
			this.saved = {
				col: other.saved.col,
				row: other.saved.row,
				attrs: cloneAttributes(other.saved.attrs),
			};
		} else {
			this.saved = null;
		}
	}

	/**
	 * Set cursor style.
	 *
	 * @param style - The cursor style to set
	 */
	setStyle(style: CursorStyle): void {
		this.style = style;
	}

	/**
	 * Set cursor style from DECSCUSR parameter.
	 *
	 * CSI Ps SP q - Set cursor style (DECSCUSR)
	 *   Ps = 0  -> blinking block (default)
	 *   Ps = 1  -> blinking block
	 *   Ps = 2  -> steady block
	 *   Ps = 3  -> blinking underline
	 *   Ps = 4  -> steady underline
	 *   Ps = 5  -> blinking bar
	 *   Ps = 6  -> steady bar
	 *
	 * @param param - DECSCUSR parameter value
	 */
	setStyleFromDECSCUSR(param: number): void {
		switch (param) {
			case 0:
			case 1:
				this.style = "block";
				this.blink = true;
				break;
			case 2:
				this.style = "block";
				this.blink = false;
				break;
			case 3:
				this.style = "underline";
				this.blink = true;
				break;
			case 4:
				this.style = "underline";
				this.blink = false;
				break;
			case 5:
				this.style = "bar";
				this.blink = true;
				break;
			case 6:
				this.style = "bar";
				this.blink = false;
				break;
		}
	}
}
