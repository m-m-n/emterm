/**
 * Mux copy mode — text selection with vi/emacs keybindings.
 *
 * Entered via prefix+[, exited via q/Escape/Ctrl+C.
 * Supports scrollback navigation, text selection, and clipboard copy.
 */

export { ViKeybinds } from "./vi-keybinds";
export { EmacsKeybinds } from "./emacs-keybinds";

/** Copy mode state. */
export type CopyModeState = "inactive" | "navigating" | "selecting";

/** Virtual cursor position in copy mode. */
export interface CopyModeCursor {
  col: number;
  row: number; // viewport-relative
  scrollOffset: number; // lines scrolled back from bottom
}

/** Selection range. */
export interface CopyModeSelection {
  startCol: number;
  startRow: number;
  endCol: number;
  endRow: number;
}

/**
 * Copy mode manager.
 */
export class CopyModeManager {
  private _state: CopyModeState = "inactive";
  private cursor: CopyModeCursor = { col: 0, row: 0, scrollOffset: 0 };
  private selection: CopyModeSelection | null = null;
  private onStateChange: ((state: CopyModeState) => void) | null = null;
  private onSelectionChange: ((selection: CopyModeSelection | null) => void) | null = null;

  get state(): CopyModeState {
    return this._state;
  }

  get isActive(): boolean {
    return this._state !== "inactive";
  }

  setOnStateChange(callback: (state: CopyModeState) => void): void {
    this.onStateChange = callback;
  }

  setOnSelectionChange(callback: (selection: CopyModeSelection | null) => void): void {
    this.onSelectionChange = callback;
  }

  /** Enter copy mode. */
  enter(): void {
    if (this._state !== "inactive") return;
    this._state = "navigating";
    this.cursor = { col: 0, row: 0, scrollOffset: 0 };
    this.selection = null;
    this.onStateChange?.("navigating");
  }

  /** Exit copy mode. */
  exit(): void {
    if (this._state === "inactive") return;
    this._state = "inactive";
    this.selection = null;
    this.onStateChange?.("inactive");
    this.onSelectionChange?.(null);
  }

  /** Start visual selection at current cursor position. */
  startSelection(): void {
    if (this._state !== "navigating") return;
    this._state = "selecting";
    this.selection = {
      startCol: this.cursor.col,
      startRow: this.cursor.row - this.cursor.scrollOffset,
      endCol: this.cursor.col,
      endRow: this.cursor.row - this.cursor.scrollOffset,
    };
    this.onStateChange?.("selecting");
    this.onSelectionChange?.(this.selection);
  }

  /** Move cursor and update selection if in selecting mode. */
  moveCursor(deltaCol: number, deltaRow: number, maxCols: number, maxRows: number): void {
    if (this._state === "inactive") return;

    this.cursor.col = Math.max(0, Math.min(maxCols - 1, this.cursor.col + deltaCol));
    this.cursor.row = Math.max(0, Math.min(maxRows - 1, this.cursor.row + deltaRow));

    if (this._state === "selecting" && this.selection) {
      this.selection.endCol = this.cursor.col;
      this.selection.endRow = this.cursor.row - this.cursor.scrollOffset;
      this.onSelectionChange?.(this.selection);
    }
  }

  /** Get current cursor position. */
  getCursor(): CopyModeCursor {
    return { ...this.cursor };
  }

  /** Get current selection. */
  getSelection(): CopyModeSelection | null {
    return this.selection ? { ...this.selection } : null;
  }

  /** Yank (copy) selection text. Returns the selection for clipboard. */
  yank(): CopyModeSelection | null {
    if (this._state !== "selecting" || !this.selection) return null;
    const sel = { ...this.selection };
    this.exit();
    return sel;
  }
}
