/**
 * Grid structures for terminal cells.
 */
import {
	type CellAttributes,
	cloneAttributes,
	createDefaultAttributes,
} from "./attributes.ts";
import { charWidth } from "./unicode.ts";

/**
 * A single cell in the terminal grid.
 */
export interface Cell {
	/** Character displayed in this cell. */
	char: string;
	/** Display width (0, 1, or 2). */
	width: number;
	/** Cell styling attributes. */
	attrs: CellAttributes;
	/** Whether the cell needs re-rendering. */
	dirty: boolean;
}

/**
 * Create an empty cell (space character with default attributes).
 */
export function createEmptyCell(): Cell {
	return {
		char: " ",
		width: 1,
		attrs: createDefaultAttributes(),
		dirty: true,
	};
}

/**
 * Create a cell with the specified character and optional attributes.
 *
 * @param char - Character to display
 * @param attrs - Optional styling attributes
 * @returns New cell
 */
export function createCell(char: string, attrs?: CellAttributes): Cell {
	return {
		char,
		width: charWidth(char),
		attrs: attrs ? cloneAttributes(attrs) : createDefaultAttributes(),
		dirty: true,
	};
}

/**
 * Create a cell for an ASCII character (fast path).
 * Skips charWidth calculation since ASCII is always width 1.
 *
 * @param char - ASCII character (0x20-0x7E)
 * @param attrs - Styling attributes
 * @returns New cell with width 1
 */
export function createAsciiCell(char: string, attrs: CellAttributes): Cell {
	return {
		char,
		width: 1,
		attrs: cloneAttributesFast(attrs),
		dirty: true,
	};
}

/**
 * Fast attribute cloning for common cases.
 * Optimized for attributes with null fg/bg (most common case).
 */
function cloneAttributesFast(attrs: CellAttributes): CellAttributes {
	// Fast path: when fg and bg are null, use object spread
	// This is safe because all fields are primitives or null
	if (attrs.fg === null && attrs.bg === null) {
		return { ...attrs };
	}
	// Fall back to full clone for colored attributes
	return cloneAttributes(attrs);
}

/**
 * Clone a cell.
 *
 * @param cell - Cell to clone
 * @returns New independent cell
 */
export function cloneCell(cell: Cell): Cell {
	return {
		char: cell.char,
		width: cell.width,
		attrs: cloneAttributes(cell.attrs),
		dirty: cell.dirty,
	};
}

/**
 * A line (row) in the terminal grid.
 */
export class Line {
	/** Cells in this line. */
	private cells: Cell[];

	/** Whether this line needs re-rendering. */
	dirty: boolean = true;

	/** Whether this line is a continuation of the previous line (soft wrap). */
	wrapped: boolean = false;

	/**
	 * Create a new line with the specified width.
	 *
	 * @param cols - Number of columns
	 */
	constructor(cols: number) {
		this.cells = [];
		for (let i = 0; i < cols; i++) {
			this.cells.push(createEmptyCell());
		}
	}

	/**
	 * Get the number of cells in this line.
	 */
	get length(): number {
		return this.cells.length;
	}

	/**
	 * Get the cell at the specified index.
	 *
	 * @param index - Column index
	 * @returns Cell at that position
	 * @throws Error if index is out of bounds
	 */
	getCell(index: number): Cell {
		if (index < 0 || index >= this.cells.length) {
			throw new Error(
				`Cell index ${index} out of bounds (0-${this.cells.length - 1})`,
			);
		}
		return this.cells[index]!;
	}

	/**
	 * Set the cell at the specified index.
	 *
	 * @param index - Column index
	 * @param cell - Cell to set
	 * @throws Error if index is out of bounds
	 */
	setCell(index: number, cell: Cell): void {
		if (index < 0 || index >= this.cells.length) {
			throw new Error(
				`Cell index ${index} out of bounds (0-${this.cells.length - 1})`,
			);
		}
		this.cells[index] = cell;
		this.dirty = true;
	}

	/**
	 * Clear all cells to empty.
	 */
	clear(): void {
		for (let i = 0; i < this.cells.length; i++) {
			this.cells[i] = createEmptyCell();
		}
		this.dirty = true;
	}

	/**
	 * Clear cells in the specified range.
	 *
	 * @param start - Start index (inclusive)
	 * @param end - End index (exclusive)
	 */
	clearRange(start: number, end: number): void {
		const clampedStart = Math.max(0, start);
		const clampedEnd = Math.min(this.cells.length, end);

		for (let i = clampedStart; i < clampedEnd; i++) {
			this.cells[i] = createEmptyCell();
		}
		this.dirty = true;
	}

	/**
	 * Resize the line to the specified width.
	 *
	 * @param cols - New number of columns
	 */
	resize(cols: number): void {
		if (cols > this.cells.length) {
			// Expand with empty cells
			while (this.cells.length < cols) {
				this.cells.push(createEmptyCell());
			}
		} else if (cols < this.cells.length) {
			// Shrink
			this.cells.length = cols;
		}
		this.dirty = true;
	}

	/**
	 * Get the text content of this line.
	 *
	 * @returns Line text
	 */
	getText(): string {
		let text = "";
		for (const cell of this.cells) {
			if (cell.width > 0) {
				text += cell.char;
			}
		}
		return text;
	}

	/**
	 * Clone this line.
	 *
	 * @returns New independent line
	 */
	clone(): Line {
		const newLine = new Line(0);
		newLine.cells = this.cells.map(cloneCell);
		newLine.dirty = this.dirty;
		newLine.wrapped = this.wrapped;
		return newLine;
	}

	/**
	 * Get the cells array (for reflow operations).
	 *
	 * @returns Cells in this line
	 */
	getCells(): Cell[] {
		return this.cells;
	}

	/**
	 * Set the cells array (for reflow operations).
	 *
	 * @param cells - New cells array
	 */
	setCells(cells: Cell[]): void {
		this.cells = cells;
		this.dirty = true;
	}

	/**
	 * Mark all cells as dirty.
	 */
	markDirty(): void {
		this.dirty = true;
		for (const cell of this.cells) {
			cell.dirty = true;
		}
	}

	/**
	 * Clear the dirty flag.
	 */
	clearDirty(): void {
		this.dirty = false;
		for (const cell of this.cells) {
			cell.dirty = false;
		}
	}
}
