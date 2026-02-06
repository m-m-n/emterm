/**
 * Tab Keyboard Handler
 *
 * Handles keyboard shortcuts for tab operations.
 */

import type { TabManager } from "./tab-manager";
import { SettingsService } from "../settings/settings-service";
import { matchKeybindStr } from "../keybind/matcher";

/**
 * TabKeyboardHandler - Manages tab-related keyboard shortcuts
 *
 * Supported shortcuts:
 * - Ctrl+T: New tab
 * - Ctrl+W: Close active tab
 * - Ctrl+PageDown: Next tab
 * - Ctrl+PageUp: Previous tab
 * - Ctrl+1-8: Jump to tab by index
 * - Ctrl+9: Jump to last tab
 */
/**
 * Options for creating TabKeyboardHandler
 */
export interface TabKeyboardHandlerOptions {
  /** Callback when tab bar toggle keybind is pressed */
  onToggleTabBar?: () => void;
  /** Callback when open settings keybind is pressed */
  onOpenSettings?: () => void;
}

export class TabKeyboardHandler {
  private tabManager: TabManager;
  private target: EventTarget | null = null;
  private boundHandler: ((event: KeyboardEvent) => void) | null = null;
  private onToggleTabBar?: () => void;
  private onOpenSettings?: () => void;

  constructor(tabManager: TabManager, options?: TabKeyboardHandlerOptions) {
    this.tabManager = tabManager;
    this.onToggleTabBar = options?.onToggleTabBar;
    this.onOpenSettings = options?.onOpenSettings;
  }

  /**
   * Handles a keydown event
   * @returns true if the event was handled (should stop propagation)
   */
  handleKeyDown(event: KeyboardEvent): boolean {
    const keybinds = SettingsService.getCached()?.keybinds;

    // Toggle tab bar
    if (matchKeybindStr(event, keybinds?.toggle_tab_bar ?? "Ctrl+Shift+B")) {
      event.preventDefault();
      this.onToggleTabBar?.();
      return true;
    }

    // Open settings
    if (matchKeybindStr(event, keybinds?.open_settings ?? "Ctrl+,")) {
      event.preventDefault();
      this.onOpenSettings?.();
      return true;
    }

    // New tab
    if (matchKeybindStr(event, keybinds?.new_tab ?? "Ctrl+Shift+T")) {
      event.preventDefault();
      this.tabManager.createTab();
      return true;
    }

    // Close active tab
    if (matchKeybindStr(event, keybinds?.close_tab ?? "Ctrl+Shift+W")) {
      event.preventDefault();
      this.tabManager.closeActiveTab();
      return true;
    }

    // Next tab
    if (matchKeybindStr(event, keybinds?.next_tab ?? "Ctrl+PageDown")) {
      event.preventDefault();
      this.tabManager.activateNextTab();
      return true;
    }

    // Previous tab
    if (matchKeybindStr(event, keybinds?.prev_tab ?? "Ctrl+PageUp")) {
      event.preventDefault();
      this.tabManager.activatePreviousTab();
      return true;
    }

    // Ctrl+1-8: Jump to tab by index (not configurable)
    if (event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && /^[1-8]$/.test(event.key)) {
      event.preventDefault();
      const index = parseInt(event.key, 10) - 1;
      this.tabManager.activateTabByIndex(index);
      return true;
    }

    // Ctrl+9: Jump to last tab (not configurable)
    if (event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey && event.key === "9") {
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
