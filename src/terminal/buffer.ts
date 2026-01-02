/**
 * Screen buffer for terminal display.
 */
import { type Cell, Line, createEmptyCell } from "./grid.ts";

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
 * Screen buffer containing the terminal grid.
 */
export class ScreenBuffer {
  /** Number of columns. */
  private _cols: number;

  /** Number of rows. */
  private _rows: number;

  /** Lines in the buffer. */
  private lines: Line[];

  /** Scroll region (null means full screen). */
  private scrollRegion: ScrollRegion | null = null;

  /**
   * Create a new screen buffer.
   *
   * @param cols - Number of columns
   * @param rows - Number of rows
   */
  constructor(cols: number, rows: number) {
    this._cols = cols;
    this._rows = rows;
    this.lines = [];
    for (let i = 0; i < rows; i++) {
      this.lines.push(new Line(cols));
    }
  }

  /** Get number of columns. */
  get cols(): number {
    return this._cols;
  }

  /** Get number of rows. */
  get rows(): number {
    return this._rows;
  }

  /**
   * Get a line by row index.
   *
   * @param row - Row index
   * @returns Line at that row
   * @throws Error if row is out of bounds
   */
  getLine(row: number): Line {
    if (row < 0 || row >= this._rows) {
      throw new Error(`Row ${row} out of bounds (0-${this._rows - 1})`);
    }
    return this.lines[row]!;
  }

  /**
   * Get a cell at the specified position.
   *
   * @param col - Column index
   * @param row - Row index
   * @returns Cell at that position
   */
  getCell(col: number, row: number): Cell {
    return this.getLine(row).getCell(col);
  }

  /**
   * Set a cell at the specified position.
   *
   * @param col - Column index
   * @param row - Row index
   * @param cell - Cell to set
   */
  setCell(col: number, row: number, cell: Cell): void {
    this.getLine(row).setCell(col, cell);
  }

  /**
   * Set the scroll region.
   *
   * @param top - Top margin (0-indexed, inclusive)
   * @param bottom - Bottom margin (0-indexed, inclusive)
   */
  setScrollRegion(top: number, bottom: number): void {
    // Validate region
    if (top < 0) top = 0;
    if (bottom >= this._rows) bottom = this._rows - 1;

    // If it covers the full screen, clear the region
    if (top === 0 && bottom === this._rows - 1) {
      this.scrollRegion = null;
    } else if (top < bottom) {
      this.scrollRegion = { top, bottom };
    }
    // Invalid region (top >= bottom) is ignored
  }

  /**
   * Clear the scroll region (reset to full screen).
   */
  clearScrollRegion(): void {
    this.scrollRegion = null;
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

  /**
   * Scroll the buffer up by the specified number of lines.
   * Respects the scroll region if set.
   * New empty lines are added at the bottom of the region.
   *
   * @param count - Number of lines to scroll (default: 1)
   */
  scrollUp(count: number = 1): void {
    if (count <= 0) return;

    const { top, bottom } = this.getEffectiveScrollRegion();
    const regionHeight = bottom - top + 1;
    const actualCount = Math.min(count, regionHeight);

    // Remove lines from top of region
    this.lines.splice(top, actualCount);

    // Add new empty lines at bottom of region
    for (let i = 0; i < actualCount; i++) {
      this.lines.splice(bottom - actualCount + 1 + i, 0, new Line(this._cols));
    }

    // Mark affected lines as dirty
    for (let i = top; i <= bottom; i++) {
      this.lines[i]!.dirty = true;
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

    // Remove lines from bottom of region
    this.lines.splice(bottom - actualCount + 1, actualCount);

    // Add new empty lines at top of region
    for (let i = 0; i < actualCount; i++) {
      this.lines.splice(top, 0, new Line(this._cols));
    }

    // Mark affected lines as dirty
    for (let i = top; i <= bottom; i++) {
      this.lines[i]!.dirty = true;
    }
  }

  /**
   * Clear all lines in the buffer.
   */
  clearAll(): void {
    for (const line of this.lines) {
      line.clear();
    }
  }

  /**
   * Clear a single line.
   *
   * @param row - Row index
   */
  clearLine(row: number): void {
    if (row >= 0 && row < this._rows) {
      this.lines[row]!.clear();
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
      this.lines[row]!.clearRange(col, this._cols);
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
      this.lines[row]!.clearRange(0, col + 1);
    }
  }

  /**
   * Clear from cursor to end of screen (ED 0).
   *
   * @param col - Cursor column
   * @param row - Cursor row
   */
  clearBelow(col: number, row: number): void {
    // Clear from cursor to end of current line
    this.clearLineFromCursor(row, col);

    // Clear all lines below
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
    // Clear all lines above
    for (let r = 0; r < row; r++) {
      this.clearLine(r);
    }

    // Clear from start of current line to cursor (inclusive)
    this.clearLineToCursor(row, col);
  }

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

    // Only operate if row is within scroll region
    if (row < top || row > bottom) return;

    const actualCount = Math.min(count, bottom - row + 1);

    // Remove lines from bottom of region
    this.lines.splice(bottom - actualCount + 1, actualCount);

    // Insert blank lines at cursor row
    for (let i = 0; i < actualCount; i++) {
      this.lines.splice(row, 0, new Line(this._cols));
    }

    // Mark affected lines as dirty
    for (let i = row; i <= bottom; i++) {
      this.lines[i]!.dirty = true;
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

    // Only operate if row is within scroll region
    if (row < top || row > bottom) return;

    const actualCount = Math.min(count, bottom - row + 1);

    // Remove lines at cursor row
    this.lines.splice(row, actualCount);

    // Add blank lines at bottom of region
    for (let i = 0; i < actualCount; i++) {
      this.lines.splice(bottom - actualCount + 1 + i, 0, new Line(this._cols));
    }

    // Mark affected lines as dirty
    for (let i = row; i <= bottom; i++) {
      this.lines[i]!.dirty = true;
    }
  }

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

    const line = this.lines[row]!;
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

    const line = this.lines[row]!;
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

    const line = this.lines[row]!;
    const endCol = Math.min(col + count, this._cols);

    for (let i = col; i < endCol; i++) {
      line.setCell(i, createEmptyCell());
    }

    line.dirty = true;
  }

  /**
   * Get indices of dirty rows.
   *
   * @returns Array of row indices that are dirty
   */
  getDirtyRows(): number[] {
    const dirty: number[] = [];
    for (let i = 0; i < this._rows; i++) {
      if (this.lines[i]!.dirty) {
        dirty.push(i);
      }
    }
    return dirty;
  }

  /**
   * Clear the dirty flag on all lines.
   */
  clearAllDirty(): void {
    for (const line of this.lines) {
      line.clearDirty();
    }
  }

  /**
   * Resize the buffer to new dimensions.
   *
   * @param cols - New number of columns
   * @param rows - New number of rows
   */
  resize(cols: number, rows: number): void {
    // Resize existing lines
    for (const line of this.lines) {
      line.resize(cols);
    }

    // Add or remove rows
    if (rows > this._rows) {
      for (let i = this._rows; i < rows; i++) {
        this.lines.push(new Line(cols));
      }
    } else if (rows < this._rows) {
      this.lines.length = rows;
    }

    this._cols = cols;
    this._rows = rows;

    // Mark all as dirty after resize
    for (const line of this.lines) {
      line.dirty = true;
    }
  }

  /**
   * Clone this buffer.
   *
   * @returns New independent buffer
   */
  clone(): ScreenBuffer {
    const newBuffer = new ScreenBuffer(this._cols, this._rows);
    newBuffer.lines = this.lines.map((line) => line.clone());
    newBuffer.scrollRegion = this.scrollRegion
      ? { ...this.scrollRegion }
      : null;
    return newBuffer;
  }
}
