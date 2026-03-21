/**
 * Keyboard event handler for terminal input
 */

import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import type { KeyboardHandlerOptions } from "../types";
import { keyEventToBytes, shouldHandleKey } from "../../pty/keyboard";
import type { SelectionController } from "../../selection-v2";
import { showPasteDialog, sendTextInChunks } from "../../clipboard";
import { SettingsService } from "../../settings/settings-service";
import { matchKeybindStr } from "../../keybind/matcher";
import { isModalOverlayVisible } from "../../shared/dom-utils";
import {
  PrefixKeyHandler,
  type MuxAction,
} from "../../terminal/mux/prefix-key";

/**
 * Extended options for keyboard handler including IME integration
 */
export interface KeyboardHandlerContext {
  /** PTY client for sending input */
  ptyClient: PtyClient;
  /** Function to get current terminal state */
  getState: () => TerminalState;
  /** Function to get terminal renderer */
  getRenderer: () => ITerminalRenderer | null;
  /** Optional keyboard handler configuration */
  options?: KeyboardHandlerOptions;
  /** Selection controller instance */
  selectionController?: SelectionController | null;
  /** Function to check if EditContext API is active */
  isEditContextActive?: () => boolean;
  /** Function to check if IME input has focus */
  isImeInputFocused?: () => boolean;
  /** Function to check if this tab is active (visible) - for multi-tab support */
  isActiveTab?: () => boolean;
  /** Callback to toggle the search bar */
  onToggleSearch?: () => void;
  /** Callback to restore focus (e.g., to IME input) after paste */
  onRestoreFocus?: () => void;
  /** Callback to exit scrollback mode (scroll to bottom) */
  onExitScrollback?: () => void;
  /** Whether mux mode is active */
  muxMode?: boolean;
  /** Callback when a mux action is triggered via prefix key */
  onMuxAction?: (action: MuxAction) => void;
}

/**
 * Handles keyboard input and special key combinations for the terminal
 */
export class KeyboardHandler {
  private ptyClient: PtyClient;
  private getState: () => TerminalState;
  private getRenderer: () => ITerminalRenderer | null;
  private options: KeyboardHandlerOptions;
  private selectionController: SelectionController | null = null;
  private isEditContextActive: () => boolean;
  private isImeInputFocused: () => boolean;
  private isActiveTab: () => boolean;
  private onToggleSearch: (() => void) | null;
  private onRestoreFocus: (() => void) | null;
  private onExitScrollback: (() => void) | null;
  private prefixKeyHandler: PrefixKeyHandler | null = null;
  private onMuxAction: ((action: MuxAction) => void) | null;
  private muxInputCallback: ((data: Uint8Array) => void) | null = null;
  private target: EventTarget | null = null;
  private boundHandleKeyDown: ((e: KeyboardEvent) => void) | null = null;
  private boundHandleClipboardShortcut: ((e: KeyboardEvent) => void) | null =
    null;

  /**
   * Creates a new KeyboardHandler instance
   * @param context - Keyboard handler context with all dependencies
   */
  constructor(context: KeyboardHandlerContext) {
    this.ptyClient = context.ptyClient;
    this.getState = context.getState;
    this.getRenderer = context.getRenderer;
    this.options = context.options || {};
    this.selectionController = context.selectionController || null;
    this.isEditContextActive = context.isEditContextActive || (() => false);
    this.isImeInputFocused = context.isImeInputFocused || (() => false);
    this.isActiveTab = context.isActiveTab || (() => true);
    this.onToggleSearch = context.onToggleSearch || null;
    this.onRestoreFocus = context.onRestoreFocus || null;
    this.onExitScrollback = context.onExitScrollback || null;
    this.onMuxAction = context.onMuxAction ?? null;

    if (context.muxMode) {
      const muxSettings = SettingsService.getCached()?.mux;
      this.prefixKeyHandler = new PrefixKeyHandler(
        muxSettings?.prefix ?? "Ctrl+B",
        muxSettings?.keybinds ?? {},
      );
      if (this.onMuxAction) {
        this.prefixKeyHandler.setOnAction(this.onMuxAction);
      }
    }
  }

  /**
   * Updates selection controller reference
   */
  setSelectionController(controller: SelectionController | null): void {
    this.selectionController = controller;
  }

  /** Enable mux mode at runtime (e.g., when mux attach OSC is received). */
  enableMuxMode(
    prefix: string,
    keybinds: Record<string, string>,
    onAction: (action: MuxAction) => void,
    onInput?: (data: Uint8Array) => void,
  ): void {
    this.prefixKeyHandler = new PrefixKeyHandler(prefix, keybinds);
    this.onMuxAction = onAction;
    this.prefixKeyHandler.setOnAction(onAction);
    this.muxInputCallback = onInput ?? null;
  }

  /** Disable mux mode at runtime (e.g., on detach). */
  disableMuxMode(): void {
    if (this.prefixKeyHandler) {
      this.prefixKeyHandler.reset();
      this.prefixKeyHandler = null;
    }
    this.onMuxAction = null;
    this.muxInputCallback = null;
  }

  /** Update mux prefix key handler with new settings. */
  updateMuxSettings(prefix: string, keybinds: Record<string, string>): void {
    if (this.prefixKeyHandler) {
      this.prefixKeyHandler = new PrefixKeyHandler(prefix, keybinds);
      if (this.onMuxAction) {
        this.prefixKeyHandler.setOnAction(this.onMuxAction);
      }
    }
  }

  /**
   * Attaches keyboard event listeners to the target element
   * @param target - Event target to attach listeners to
   */
  attach(target: EventTarget): void {
    // Auto-detach if already attached to prevent duplicate listeners
    if (this.boundHandleClipboardShortcut || this.boundHandleKeyDown) {
      this.detach();
    }

    this.target = target;

    // Capture phase listener for clipboard shortcuts (Ctrl+Shift+C/V)
    // This runs before IME can consume the event
    this.boundHandleClipboardShortcut = (e: KeyboardEvent) => {
      this.handleClipboardShortcut(e);
    };
    target.addEventListener(
      "keydown",
      this.boundHandleClipboardShortcut as EventListener,
      { capture: true },
    );

    // Bubble phase listener for regular key handling
    this.boundHandleKeyDown = (e: KeyboardEvent) => {
      this.handleKeyDown(e);
    };
    target.addEventListener(
      "keydown",
      this.boundHandleKeyDown as EventListener,
    );
  }

  /**
   * Detaches keyboard event listeners
   */
  detach(): void {
    if (this.target) {
      // Remove capture phase listener
      if (this.boundHandleClipboardShortcut) {
        this.target.removeEventListener(
          "keydown",
          this.boundHandleClipboardShortcut as EventListener,
          { capture: true },
        );
      }

      // Remove bubble phase listener
      if (this.boundHandleKeyDown) {
        this.target.removeEventListener(
          "keydown",
          this.boundHandleKeyDown as EventListener,
        );
      }
    }
    this.boundHandleClipboardShortcut = null;
    this.boundHandleKeyDown = null;
    this.target = null;
  }

  /**
   * Handles keydown events
   * @param event - Keyboard event to handle
   */
  handleKeyDown(event: KeyboardEvent): void {
    // Skip if this tab is not active (for multi-tab support)
    // This allows multiple KeyboardHandlers to be attached to document
    // but only the active tab processes input
    if (!this.isActiveTab()) {
      return;
    }

    // Skip if event was already handled by another component (e.g., fullscreen markdown view)
    // This is a cooperative pattern - components call preventDefault() when they handle an event
    if (event.defaultPrevented) {
      return;
    }

    // Skip if a modal overlay (image viewer, markdown fullscreen) is visible.
    // Defense-in-depth: the overlay's own capture-phase handler should intercept
    // events first, but we also check here to handle edge cases in event propagation.
    if (isModalOverlayVisible()) {
      return;
    }

    // Handle Escape key - clear selection
    if (event.key === "Escape" && this.selectionController) {
      if (this.selectionController.hasSelection()) {
        this.selectionController.clearSelection();
        event.preventDefault();
        return;
      }
    }

    // Mux prefix key handling
    if (this.prefixKeyHandler) {
      if (this.prefixKeyHandler.handleKeyEvent(event)) {
        event.preventDefault();
        return;
      }
    }

    // Handle copy shortcut (fallback for non-IME scenarios)
    const keybinds = SettingsService.getCached()?.keybinds;
    if (matchKeybindStr(event, keybinds?.copy ?? "Ctrl+Shift+C")) {
      this.handleCopy(event);
      return;
    }

    // Handle paste shortcut (fallback for non-IME scenarios)
    if (matchKeybindStr(event, keybinds?.paste ?? "Ctrl+Shift+V")) {
      this.handlePaste(event);
      return;
    }

    // Handle prompt jump shortcuts
    if (
      matchKeybindStr(
        event,
        keybinds?.jump_to_prev_prompt ?? "Ctrl+Shift+ArrowUp",
      )
    ) {
      this.handlePromptJump("prev");
      event.preventDefault();
      return;
    }
    if (
      matchKeybindStr(
        event,
        keybinds?.jump_to_next_prompt ?? "Ctrl+Shift+ArrowDown",
      )
    ) {
      this.handlePromptJump("next");
      event.preventDefault();
      return;
    }

    // Handle search shortcut
    if (matchKeybindStr(event, keybinds?.search ?? "Ctrl+Shift+F")) {
      if (this.onToggleSearch) {
        this.onToggleSearch();
      }
      event.preventDefault();
      return;
    }

    if (!shouldHandleKey(event)) {
      return;
    }

    // Skip if IME composition is in progress
    // Note: event.isComposing works for browser IME, but not for Emacs-style IMEs like SKK
    if (event.isComposing) {
      return;
    }

    // Skip Ctrl+J when SKK mode is enabled (default: true)
    // Ctrl+J is commonly used by Emacs-style IMEs (SKK) for mode switching
    // Without this, Ctrl+J would send LF (0x0A) which causes unwanted newlines
    const cachedSettings = SettingsService.getCached();
    if (
      event.ctrlKey &&
      !event.altKey &&
      !event.shiftKey &&
      !event.metaKey &&
      event.key.toLowerCase() === "j" &&
      cachedSettings?.skk_mode !== false
    ) {
      return;
    }

    // In mux mode, bypass EditContext/IME — all input goes through muxInputCallback
    if (!this.prefixKeyHandler) {
      // If using EditContext API, let it handle most input
      if (this.isEditContextActive()) {
        // Only process special keys that EditContext doesn't handle
        // Note: Enter key is NOT handled by EditContext's textupdate event
        // (see: https://developer.mozilla.org/en-US/docs/Web/API/EditContext_API/Guide)
        // So we must process Enter here explicitly
        if (!this.isSpecialKey(event) && event.key !== "Enter") {
          return; // Let EditContext handle regular input
        }
        // Special keys (Ctrl+C, arrows, Enter, etc.) fall through to be processed
      }

      // Skip if hidden textarea has focus (IME is active) - fallback mode
      if (this.isImeInputFocused()) {
        // Only allow special keys to pass through
        // Note: event.isComposing is already checked above (line 173),
        // so if we reach here, IME composition is not in progress.
        // Enter key must be processed here since IME textarea doesn't handle it.
        if (!this.isSpecialKey(event) && event.key !== "Enter") {
          return; // Let IME handler process regular keys
        }
        // Navigation, function keys, and Enter fall through
      }
    }

    // Get cursor keys mode from terminal state for DECCKM support
    const state = this.getState();
    const bytes = keyEventToBytes(event, {
      cursorKeysMode: state.getModes().cursorKeys,
      shiftEnterAsAltEnter: cachedSettings?.shift_enter_as_alt_enter !== false,
    });
    if (bytes) {
      event.preventDefault();

      // Auto-scroll to bottom when user types during scrollback
      this.onExitScrollback?.();

      if (this.muxInputCallback) {
        // In mux mode: send to daemon instead of local PTY
        this.muxInputCallback(bytes);
      } else {
        // Fire-and-forget: don't await to avoid blocking key repeat
        this.ptyClient.write(bytes).catch((error) => {
          console.error("Failed to write to PTY:", error);
        });
      }
    }
  }

  /**
   * Handles clipboard shortcuts in capture phase (Ctrl+Shift+C/V)
   * This runs before IME can consume the event
   * @param event - Keyboard event to handle
   */
  private handleClipboardShortcut(event: KeyboardEvent): void {
    // Skip if this tab is not active (for multi-tab support)
    if (!this.isActiveTab()) {
      return;
    }

    // Skip if a modal overlay is visible - let the overlay handle keys
    if (isModalOverlayVisible()) {
      return;
    }

    const keybinds = SettingsService.getCached()?.keybinds;

    if (matchKeybindStr(event, keybinds?.copy ?? "Ctrl+Shift+C")) {
      // CRITICAL: preventDefault/stopPropagation must be called synchronously
      // before any async operation to prevent IME from consuming the event
      event.preventDefault();
      event.stopPropagation();
      this.handleCopy(event);
      return;
    }

    if (matchKeybindStr(event, keybinds?.paste ?? "Ctrl+Shift+V")) {
      // CRITICAL: preventDefault/stopPropagation must be called synchronously
      // before any async operation to prevent IME from consuming the event
      event.preventDefault();
      event.stopPropagation();
      this.handlePaste(event);
      return;
    }

    if (matchKeybindStr(event, keybinds?.search ?? "Ctrl+Shift+F")) {
      event.preventDefault();
      event.stopPropagation();
      if (this.onToggleSearch) {
        this.onToggleSearch();
      }
      return;
    }
  }

  /**
   * Handles copy operation (Shift+Ctrl+C)
   */
  private async handleCopy(event: KeyboardEvent): Promise<void> {
    if (this.selectionController && this.selectionController.hasSelection()) {
      const success = await this.selectionController.copy();
      if (success) {
        this.selectionController.clearSelection();
      }
    }
    event.preventDefault();
  }

  /**
   * Handles paste operation (Shift+Ctrl+V)
   */
  private async handlePaste(event: KeyboardEvent): Promise<void> {
    if (!this.selectionController) {
      event.preventDefault();
      return;
    }

    try {
      const text = await this.selectionController.paste();
      if (!text) {
        event.preventDefault();
        return;
      }

      // Auto-scroll to bottom when user pastes during scrollback
      this.onExitScrollback?.();

      // Check if multi-line
      if (this.selectionController.isMultiLinePaste(text)) {
        const lineCount = this.selectionController.countPasteLines(text);
        // Show confirmation dialog for multi-line paste
        const result = await showPasteDialog({ text, lineCount });
        if (result.confirmed) {
          // Send text in chunks
          await sendTextInChunks(text, (data: Uint8Array) =>
            this.ptyClient.write(data),
          );
        }
      } else {
        // Single line - paste directly
        const bytes = new TextEncoder().encode(text);
        await this.ptyClient.write(bytes);
      }
    } catch (error) {
      console.error("Failed to paste from clipboard:", error);
    } finally {
      // Restore focus to IME input after paste completes
      this.onRestoreFocus?.();
    }
    event.preventDefault();
  }

  /**
   * Handles prompt jump navigation.
   * Finds the nearest prompt marker and scrolls to it.
   */
  private handlePromptJump(direction: "prev" | "next"): void {
    const state = this.getState();
    const renderer = this.getRenderer();
    if (!renderer) return;

    const tracker = state.getSemanticZoneTracker();
    const scrollbackLength = state.getScrollbackLength();
    const scrollOffset = renderer.getScrollOffset();

    // Calculate the absolute line of the current view top
    const currentTopLine = scrollbackLength - scrollOffset;

    const marker =
      direction === "prev"
        ? tracker.findPrevPrompt(currentTopLine)
        : tracker.findNextPrompt(currentTopLine);

    if (marker) {
      // Auto-expand fold region if jumping into a collapsed region
      const foldManager = state.getFoldManager();
      foldManager.expandRegionContaining(marker.lineIndex);

      // Scroll so the marker line is at the top of the view
      const newScrollOffset = scrollbackLength - marker.lineIndex;
      renderer.setScrollOffset(newScrollOffset);
      renderer.forceRender(state);
    } else if (direction === "prev") {
      // No previous prompt: scroll to top
      renderer.setScrollOffset(scrollbackLength);
      renderer.forceRender(state);
    } else {
      // No next prompt: scroll to bottom
      renderer.setScrollOffset(0);
      renderer.forceRender(state);
    }
  }

  /**
   * Checks if a key event represents a special key that should bypass IME
   * @param event - Keyboard event to check
   * @returns True if this is a special key combination
   */
  isSpecialKey(event: KeyboardEvent): boolean {
    // Ctrl/Alt/Meta combinations (always special)
    if (event.ctrlKey || event.altKey || event.metaKey) {
      return true;
    }

    // Navigation keys
    if (
      event.key.startsWith("Arrow") ||
      event.key === "Home" ||
      event.key === "End" ||
      event.key === "PageUp" ||
      event.key === "PageDown"
    ) {
      return true;
    }

    // Editing keys
    if (event.key === "Backspace" || event.key === "Delete") {
      return true;
    }

    // Function keys
    if (event.key.startsWith("F") && /^F\d+$/.test(event.key)) {
      return true;
    }

    // Other special keys
    if (
      event.key === "Escape" ||
      event.key === "Tab" ||
      event.key === "Insert"
    ) {
      return true;
    }

    return false;
  }

}
