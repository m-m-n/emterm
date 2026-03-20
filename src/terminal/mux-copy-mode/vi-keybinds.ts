/**
 * Vi keybindings for mux copy mode.
 *
 * Movement: h/j/k/l, w/b, 0/$, g/G
 * Selection: v (toggle), V (line)
 * Action: y (yank/copy), / (search)
 * Exit: q, Escape
 */

import type { CopyModeManager } from "./index";

/** Vi keybinding handler for copy mode. */
export class ViKeybinds {
  private manager: CopyModeManager;
  private maxCols: number;
  private maxRows: number;

  constructor(manager: CopyModeManager, maxCols: number, maxRows: number) {
    this.manager = manager;
    this.maxCols = maxCols;
    this.maxRows = maxRows;
  }

  /** Update dimensions. */
  setDimensions(cols: number, rows: number): void {
    this.maxCols = cols;
    this.maxRows = rows;
  }

  /**
   * Handle a key event. Returns true if consumed.
   */
  handleKey(key: string): boolean {
    if (!this.manager.isActive) return false;

    switch (key) {
      // Movement
      case "h":
        this.manager.moveCursor(-1, 0, this.maxCols, this.maxRows);
        return true;
      case "j":
        this.manager.moveCursor(0, 1, this.maxCols, this.maxRows);
        return true;
      case "k":
        this.manager.moveCursor(0, -1, this.maxCols, this.maxRows);
        return true;
      case "l":
        this.manager.moveCursor(1, 0, this.maxCols, this.maxRows);
        return true;
      case "0":
        // Move to beginning of line
        this.manager.moveCursor(-this.manager.getCursor().col, 0, this.maxCols, this.maxRows);
        return true;
      case "$":
        // Move to end of line
        this.manager.moveCursor(this.maxCols - 1 - this.manager.getCursor().col, 0, this.maxCols, this.maxRows);
        return true;
      // Selection
      case "v":
        if (this.manager.state === "navigating") {
          this.manager.startSelection();
        }
        return true;
      // Yank
      case "y":
        this.manager.yank();
        return true;
      // Exit
      case "q":
      case "Escape":
        this.manager.exit();
        return true;
      default:
        return true; // Consume all keys in copy mode
    }
  }
}
