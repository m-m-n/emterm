/**
 * Tab Keyboard Handler
 *
 * Handles keyboard shortcuts for tab operations.
 */

import type { TabManager } from "./tab-manager";

/**
 * TabKeyboardHandler - Manages tab-related keyboard shortcuts
 *
 * Supported shortcuts:
 * - Ctrl+T: New tab
 * - Ctrl+W: Close active tab
 * - Ctrl+Tab: Next tab
 * - Ctrl+Shift+Tab: Previous tab
 * - Ctrl+1-8: Jump to tab by index
 * - Ctrl+9: Jump to last tab
 */
export class TabKeyboardHandler {
  private tabManager: TabManager;
  private target: EventTarget | null = null;
  private boundHandler: ((event: KeyboardEvent) => void) | null = null;

  constructor(tabManager: TabManager) {
    this.tabManager = tabManager;
  }

  /**
   * Handles a keydown event
   * @returns true if the event was handled (should stop propagation)
   */
  handleKeyDown(event: KeyboardEvent): boolean {
    // Only handle Ctrl+key combinations (not Ctrl+Alt or Ctrl+Meta)
    if (!event.ctrlKey || event.altKey || event.metaKey) {
      return false;
    }

    const key = event.key.toLowerCase();

    // Ctrl+T: New tab
    if (key === "t" && !event.shiftKey) {
      event.preventDefault();
      this.tabManager.createTab();
      return true;
    }

    // Ctrl+W: Close active tab
    if (key === "w" && !event.shiftKey) {
      event.preventDefault();
      this.tabManager.closeActiveTab();
      return true;
    }

    // Ctrl+Tab or Ctrl+Shift+Tab: Navigate tabs
    if (event.key === "Tab") {
      event.preventDefault();
      if (event.shiftKey) {
        this.tabManager.activatePreviousTab();
      } else {
        this.tabManager.activateNextTab();
      }
      return true;
    }

    // Ctrl+1-8: Jump to tab by index
    if (!event.shiftKey && /^[1-8]$/.test(event.key)) {
      event.preventDefault();
      const index = parseInt(event.key, 10) - 1;
      this.tabManager.activateTabByIndex(index);
      return true;
    }

    // Ctrl+9: Jump to last tab
    if (event.key === "9" && !event.shiftKey) {
      event.preventDefault();
      this.tabManager.activateLastTab();
      return true;
    }

    return false;
  }

  /**
   * Attaches the keyboard handler to an element
   */
  attach(target: EventTarget): void {
    this.detach(); // Remove any existing listener

    this.target = target;
    this.boundHandler = (event: KeyboardEvent) => {
      if (this.handleKeyDown(event)) {
        event.stopPropagation();
      }
    };

    target.addEventListener("keydown", this.boundHandler as EventListener);
  }

  /**
   * Detaches the keyboard handler
   */
  detach(): void {
    if (this.target && this.boundHandler) {
      this.target.removeEventListener(
        "keydown",
        this.boundHandler as EventListener,
      );
    }
    this.target = null;
    this.boundHandler = null;
  }
}
