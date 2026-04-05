/**
 * Emacs keybindings for mux copy mode.
 *
 * Movement: C-f/C-b (forward/back char), C-n/C-p (next/prev line), C-a/C-e (start/end line)
 * Selection: C-Space (toggle)
 * Action: M-w (copy/yank)
 * Exit: C-g, Escape
 */

import type { CopyModeManager } from "./index";

/** Emacs keybinding handler for copy mode. */
export class EmacsKeybinds {
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
  handleKeyEvent(event: KeyboardEvent): boolean {
    if (!this.manager.isActive) return false;

    if (event.ctrlKey) {
      switch (event.key) {
        case "f":
          this.manager.moveCursor(1, 0, this.maxCols, this.maxRows);
          return true;
        case "b":
          this.manager.moveCursor(-1, 0, this.maxCols, this.maxRows);
          return true;
        case "n":
          this.manager.moveCursor(0, 1, this.maxCols, this.maxRows);
          return true;
        case "p":
          this.manager.moveCursor(0, -1, this.maxCols, this.maxRows);
          return true;
        case "a":
          this.manager.moveCursor(-this.manager.getCursor().col, 0, this.maxCols, this.maxRows);
          return true;
        case "e":
          this.manager.moveCursor(
            this.maxCols - 1 - this.manager.getCursor().col,
            0,
            this.maxCols,
            this.maxRows,
          );
          return true;
        case " ": // C-Space: toggle selection
          if (this.manager.state === "navigating") {
            this.manager.startSelection();
          }
          return true;
        case "c": // C-c: exit (tmux-compatible)
          this.manager.exit();
          return true;
        case "g": // C-g: cancel/exit
          this.manager.exit();
          return true;
      }
    }

    if (event.altKey && event.key === "w") {
      // M-w: copy selection
      this.manager.yank();
      return true;
    }

    if (event.key === "Escape") {
      this.manager.exit();
      return true;
    }

    return true; // Consume all keys in copy mode
  }
}
