/**
 * Keyboard event handler for terminal input
 */

import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { TerminalRenderer } from "../../terminal/renderer";
import type { KeyboardHandlerOptions } from "../types";
import { IME_DEBUG } from "../config";
import { keyEventToBytes, shouldHandleKey } from "../../pty/keyboard";
import type { SelectionController } from "../../selection-v2";
import { showPasteDialog, sendTextInChunks } from "../../clipboard";

/**
 * Extended options for keyboard handler including IME integration
 */
export interface KeyboardHandlerContext {
  /** PTY client for sending input */
  ptyClient: PtyClient;
  /** Function to get current terminal state */
  getState: () => TerminalState;
  /** Function to get terminal renderer */
  getRenderer: () => TerminalRenderer | null;
  /** Optional keyboard handler configuration */
  options?: KeyboardHandlerOptions;
  /** Selection controller instance */
  selectionController?: SelectionController | null;
  /** Function to check if EditContext API is active */
  isEditContextActive?: () => boolean;
  /** Function to check if IME input has focus */
  isImeInputFocused?: () => boolean;
}

/**
 * Handles keyboard input and special key combinations for the terminal
 */
export class KeyboardHandler {
  private ptyClient: PtyClient;
  private getState: () => TerminalState;
  private getRenderer: () => TerminalRenderer | null;
  private options: KeyboardHandlerOptions;
  private selectionController: SelectionController | null = null;
  private isEditContextActive: () => boolean;
  private isImeInputFocused: () => boolean;
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
  }

  /**
   * Updates selection controller reference
   */
  setSelectionController(controller: SelectionController | null): void {
    this.selectionController = controller;
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
  async handleKeyDown(event: KeyboardEvent): Promise<void> {
    // Handle Escape key - clear selection
    if (event.key === "Escape" && this.selectionController) {
      if (this.selectionController.hasSelection()) {
        this.selectionController.clearSelection();
        event.preventDefault();
        return;
      }
    }

    // Handle Shift+Ctrl+C - copy selection
    // Note: This is a fallback for non-IME scenarios. When IME is active,
    // handleClipboardShortcut (capture phase) handles this before reaching here.
    if (event.key.toLowerCase() === "c" && event.shiftKey && event.ctrlKey) {
      await this.handleCopy(event);
      return;
    }

    // Handle Shift+Ctrl+V - paste from clipboard
    // Note: This is a fallback for non-IME scenarios. When IME is active,
    // handleClipboardShortcut (capture phase) handles this before reaching here.
    if (event.key.toLowerCase() === "v" && event.shiftKey && event.ctrlKey) {
      await this.handlePaste(event);
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

    // Skip Ctrl+J - commonly used by Emacs-style IMEs (SKK) for mode switching
    // Without this, Ctrl+J would send LF (0x0A) which causes unwanted newlines
    if (event.ctrlKey && event.key.toLowerCase() === "j") {
      return;
    }

    // If using EditContext API, let it handle most input
    if (this.isEditContextActive()) {
      // Only process special keys that EditContext doesn't handle
      if (!this.isSpecialKey(event)) {
        return; // Let EditContext handle regular input
      }
      // Enter should be handled by EditContext for IME confirmation
      if (event.key === "Enter" && !event.ctrlKey && !event.altKey) {
        return;
      }
      // Special keys (Ctrl+C, arrows, etc.) fall through to be processed
    }

    // Skip if hidden textarea has focus (IME is active) - fallback mode
    if (this.isImeInputFocused()) {
      // Only allow certain special keys to pass through
      // Enter should be handled by IME for confirmation
      if (event.key === "Enter") {
        return; // Let IME handler process Enter
      }
      if (!this.isSpecialKey(event)) {
        return; // Let IME handler process regular keys
      }
      // Navigation and function keys fall through
    }

    const bytes = keyEventToBytes(event);
    if (bytes) {
      event.preventDefault();
      try {
        await this.ptyClient.write(bytes);
      } catch (error) {
        console.error("Failed to write to PTY:", error);
      }
    }
  }

  /**
   * Handles clipboard shortcuts in capture phase (Ctrl+Shift+C/V)
   * This runs before IME can consume the event
   * @param event - Keyboard event to handle
   */
  private handleClipboardShortcut(event: KeyboardEvent): void {
    // Only handle Ctrl+Shift combinations
    if (!event.ctrlKey || !event.shiftKey) {
      return;
    }

    const key = event.key.toLowerCase();

    if (key === "c") {
      // CRITICAL: preventDefault/stopPropagation must be called synchronously
      // before any async operation to prevent IME from consuming the event
      event.preventDefault();
      event.stopPropagation();
      this.handleCopy(event);
      return;
    }

    if (key === "v") {
      // CRITICAL: preventDefault/stopPropagation must be called synchronously
      // before any async operation to prevent IME from consuming the event
      event.preventDefault();
      event.stopPropagation();
      this.handlePaste(event);
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
    }
    event.preventDefault();
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
