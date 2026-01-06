/**
 * eMterm - Terminal Emulator
 * Main entry point
 */

import {
  PtyClient,
  keyEventToBytes,
  shouldHandleKey,
  measureCharacterSize,
  observeContainerResize,
} from "./pty";
import {
  TerminalState,
  TerminalRenderer,
  encodeMouseEvent,
  domEventToMouseEvent,
  isMouseTrackingEnabled,
} from "./terminal";
import type { TerminalActionsPayload } from "./types/terminal.ts";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// Feature flag for new terminal rendering
const USE_NEW_TERMINAL = true;

// Debug flag for IME logging (only in development)
const IME_DEBUG = import.meta.env?.DEV ?? false;

// Type definitions for EditContext API (experimental Chromium feature)
interface EditContextInit {
  text?: string;
  selectionStart?: number;
  selectionEnd?: number;
}

interface EditContext extends EventTarget {
  text: string;
  selectionStart: number;
  selectionEnd: number;
  updateText(start: number, end: number, text: string): void;
  updateSelection(start: number, end: number): void;
  updateControlBounds(bounds: DOMRect): void;
  updateSelectionBounds(bounds: DOMRect): void;
  updateCharacterBounds(start: number, bounds: DOMRect[]): void;
  addEventListener(type: string, listener: (event: any) => void): void;
  removeEventListener(type: string, listener: (event: any) => void): void;
}

interface EditContextConstructor {
  new (init?: EditContextInit): EditContext;
}

// Global state
let ptyClient: PtyClient | null = null;
let disconnectResizeObserver: (() => void) | null = null;
let terminalState: TerminalState | null = null;
let terminalRenderer: TerminalRenderer | null = null;
let charSize: { width: number; height: number } = { width: 8, height: 16 };
let mouseEventListeners: (() => void)[] = [];
let imeInput: HTMLTextAreaElement | null = null;
let compositionView: HTMLDivElement | null = null;
let editContext: EditContext | null = null; // EditContext API (Chromium only)
let editContextCleanup: (() => void) | null = null; // Cleanup for EditContext event listeners
let terminalClickHandler: ((e: MouseEvent) => void) | null = null; // Terminal click handler for textarea focus

/**
 * Initialize the terminal
 */
async function initTerminal(): Promise<void> {
  const terminal = document.getElementById("terminal");
  if (!terminal) {
    console.error("Terminal element not found");
    return;
  }

  // Measure character size from container's computed styles
  charSize = measureCharacterSize(terminal);

  // Create composition view to show IME input inline (for SKK etc.)
  compositionView = document.createElement("div");
  compositionView.id = "ime-composition-view";
  compositionView.style.cssText = `
    position: fixed;
    z-index: 99999;
    background: #1e1e1e;
    color: #d4d4d4;
    font-family: inherit;
    font-size: inherit;
    white-space: pre;
    pointer-events: none;
    display: none;
    padding: 2px 4px;
    border: 1px solid #555;
    border-radius: 2px;
    min-width: 20px;
    min-height: 1em;
  `;
  // Append to body to avoid any container overflow issues
  document.body.appendChild(compositionView);

  // Try to use EditContext API (Chromium/WebView2 only)
  if ("EditContext" in window) {
    if (IME_DEBUG) console.log("[IME] EditContext API available, using it");
    setupEditContextIME(terminal, compositionView);
  } else {
    if (IME_DEBUG) console.log("[IME] EditContext API not available, using textarea fallback");
    // Create hidden textarea for IME (fallback for WebKit)
    imeInput = document.createElement("textarea");
    imeInput.autocomplete = "off";
    imeInput.setAttribute("autocapitalize", "off");
    imeInput.setAttribute("spellcheck", "false");
    imeInput.tabIndex = 0;
    // Style: position off-screen but still focusable
    imeInput.style.cssText = `
      position: fixed;
      left: -9999px;
      top: 0;
      width: 10px;
      height: 10px;
      opacity: 0;
      border: none;
      padding: 0;
      margin: 0;
      outline: none;
      overflow: hidden;
      resize: none;
    `;
    document.body.appendChild(imeInput);

    // Set up IME handlers
    setupIMEHandlers(imeInput, compositionView);
  }

  // Calculate initial terminal size
  const initialSize = {
    cols: Math.floor(terminal.clientWidth / charSize.width),
    rows: Math.floor(terminal.clientHeight / charSize.height),
  };

  const cols = Math.max(1, initialSize.cols);
  const rows = Math.max(1, initialSize.rows);

  // Debug logging for size calculation
  if (import.meta.env?.DEV) {
    console.log("[Size Debug] Container dimensions:", {
      clientWidth: terminal.clientWidth,
      clientHeight: terminal.clientHeight,
      offsetWidth: terminal.offsetWidth,
      offsetHeight: terminal.offsetHeight,
    });
    console.log("[Size Debug] Character size:", charSize);
    console.log("[Size Debug] Calculated terminal size:", { cols, rows });
    console.log("[Size Debug] Expected pixel usage:", {
      usedWidth: cols * charSize.width,
      usedHeight: rows * charSize.height,
      remainderWidth: terminal.clientWidth - cols * charSize.width,
      remainderHeight: terminal.clientHeight - rows * charSize.height,
    });
  }

  if (USE_NEW_TERMINAL) {
    // New terminal rendering system
    initNewTerminal(terminal, cols, rows);
  } else {
    // Legacy terminal rendering (simple text append)
    initLegacyTerminal(terminal);
  }

  // Create PTY client
  ptyClient = new PtyClient();

  // Set up event handlers based on mode
  if (USE_NEW_TERMINAL) {
    await setupNewTerminalHandlers();
  } else {
    await setupLegacyHandlers(terminal);
  }

  // Spawn PTY session
  try {
    await ptyClient.spawn({ cols, rows });

    // Flush any terminal actions that arrived before spawn returned
    // This fixes the race condition where the shell prompt arrives
    // before sessionId is set
    if (USE_NEW_TERMINAL && terminalState && terminalRenderer) {
      ptyClient.flushPendingTerminalActions();
      // Force re-render after flush to ensure flushed content is displayed
      terminalRenderer.forceRender(terminalState);
    }
  } catch (error) {
    console.error("Failed to spawn PTY:", error);
    terminal.textContent = `Failed to start terminal: ${error}`;
    return;
  }

  // Set up keyboard input handler
  document.addEventListener("keydown", handleKeyDown);

  // Set up mouse event handlers (for new terminal only)
  if (USE_NEW_TERMINAL) {
    setupMouseHandlers(terminal);
  }

  // Set up resize observer
  disconnectResizeObserver = observeContainerResize(
    terminal,
    charSize.width,
    charSize.height,
    async (newCols, newRows) => {
      if (import.meta.env?.DEV) {
        console.log("[Size Debug] ResizeObserver callback:", {
          newCols,
          newRows,
          containerClientWidth: terminal.clientWidth,
          containerClientHeight: terminal.clientHeight,
          charWidth: charSize.width,
          charHeight: charSize.height,
        });
      }

      if (ptyClient) {
        try {
          await ptyClient.resize(newCols, newRows);

          // Resize terminal state if using new system
          if (terminalState && terminalRenderer) {
            terminalState.resize(newCols, newRows);
            terminalRenderer.resize(newCols, newRows);
            // Force re-render after resize to recreate line elements
            terminalRenderer.forceRender(terminalState);
            // Update IME position after resize
            updateIMEPosition();
          }
        } catch (error) {
          console.error("Failed to resize PTY:", error);
        }
      }
    },
  );

  // Focus handling - make terminal focusable and focus hidden textarea for IME
  terminal.tabIndex = 0;
  // Only register click handler for textarea fallback mode
  if (imeInput) {
    terminalClickHandler = (e) => {
      // Prevent default to avoid terminal DIV getting focus
      // But allow the textarea to receive focus
      if (e.target !== imeInput) {
        e.preventDefault();
      }
      if (IME_DEBUG) console.log("[IME Debug] terminal mousedown, focusing textarea");
      if (imeInput) {
        imeInput.focus();
        // Use setTimeout to ensure focus happens after event processing
        setTimeout(() => {
          if (IME_DEBUG) console.log("[IME Debug] activeElement after focus:", document.activeElement?.tagName);
        }, 0);
      }
    };
    terminal.addEventListener("mousedown", terminalClickHandler);
  }

  // Initial focus on hidden textarea (with delay to ensure DOM is ready)
  if (imeInput) {
    setTimeout(() => {
      if (imeInput) {
        imeInput.focus();
        if (IME_DEBUG) console.log("[IME Debug] initial focus, activeElement:", document.activeElement?.tagName);
      }
    }, 100);
  }

  // Expose for E2E testing (must be after initialization)
  window.terminalState = terminalState;
  window.ptyClient = ptyClient;
  window.terminalRenderer = terminalRenderer;
}

/**
 * Initialize new terminal rendering system.
 */
function initNewTerminal(
  container: HTMLElement,
  cols: number,
  rows: number,
): void {
  // Apply terminal styles
  container.style.backgroundColor = "#1e1e1e";
  container.style.color = "#d4d4d4";
  container.style.height = "100%";
  container.style.boxSizing = "border-box";

  // Get font family and size from computed styles
  const computedStyle = getComputedStyle(container);
  const fontFamily = computedStyle.fontFamily || "monospace";
  const fontSize = parseFloat(computedStyle.fontSize) || 14;

  // Create terminal state and renderer
  terminalState = new TerminalState(cols, rows);
  terminalRenderer = new TerminalRenderer(container, fontFamily, fontSize);

  // Initial render
  terminalRenderer.forceRender(terminalState);
}

/**
 * Initialize legacy terminal rendering.
 */
function initLegacyTerminal(container: HTMLElement): void {
  // Get font family and size from computed styles
  const computedStyle = getComputedStyle(container);
  const fontFamily = computedStyle.fontFamily || "monospace";
  const fontSize = computedStyle.fontSize || "14px";

  container.style.fontFamily = fontFamily;
  container.style.fontSize = fontSize;
  container.style.whiteSpace = "pre-wrap";
  container.style.overflow = "auto";
  container.style.backgroundColor = "#1e1e1e";
  container.style.color = "#d4d4d4";
  container.style.height = "100%";
  container.style.boxSizing = "border-box";
}

// Track the last known title to avoid unnecessary updates
let lastWindowTitle = "";

/**
 * Set up handlers for new terminal system.
 * Will listen to terminal_actions events from the ANSI parser.
 */
async function setupNewTerminalHandlers(): Promise<void> {
  if (!ptyClient) return;

  // Listen for terminal_actions events (from Phase 1 ANSI parser)
  await ptyClient.onTerminalActions(async (payload: TerminalActionsPayload) => {
    if (!terminalState || !terminalRenderer || !ptyClient) return;

    // Process each action
    for (const action of payload.actions) {
      terminalState.processAction(action);
    }

    // Handle DSR responses - write back to PTY
    const response = terminalState.takePendingResponse();
    if (response) {
      try {
        await ptyClient.write(response);
      } catch (error) {
        console.error("Failed to write DSR response:", error);
      }
    }

    // Handle window title changes
    const newTitle = terminalState.title;
    if (newTitle !== lastWindowTitle) {
      lastWindowTitle = newTitle;
      try {
        const appWindow = getCurrentWebviewWindow();
        await appWindow.setTitle(newTitle || "eMterm");
      } catch (error) {
        console.error("Failed to set window title:", error);
      }
    }

    // Schedule render
    terminalRenderer.scheduleRender(terminalState);

    // Update IME position after terminal state changes
    updateIMEPosition();
  });

  // Handle exit and error events
  await ptyClient.onExit(async (code, remainingSessions) => {
    if (import.meta.env?.DEV) {
      console.log(
        `[Main] onExit callback: code=${code}, remainingSessions=${remainingSessions}`,
      );
    }

    // Use remaining_sessions from the event (already removed from backend)
    // This ensures accurate count as the session is removed before event emission
    if (remainingSessions === 0) {
      if (import.meta.env?.DEV) {
        console.log("[Main] Last session exited, closing window...");
      }

      // Only close window if no other sessions exist
      try {
        const appWindow = getCurrentWebviewWindow();
        await appWindow.close();

        if (import.meta.env?.DEV) {
          console.log("[Main] Window closed successfully");
        }
      } catch (error) {
        if (import.meta.env?.DEV) {
          console.error("[Main] Failed to close window:", error);
        } else {
          console.error("Failed to close window:", error);
        }
      }
    } else {
      if (import.meta.env?.DEV) {
        console.log(
          `[Main] ${remainingSessions} session(s) remaining, keeping window open`,
        );
      }
    }
    // If remainingSessions > 0, other sessions exist (future multi-tab support)
  });

  await ptyClient.onError((message) => {
    console.error("PTY error:", message);
  });
}

/**
 * Set up legacy event handlers (simple text append).
 */
async function setupLegacyHandlers(terminal: HTMLElement): Promise<void> {
  if (!ptyClient) return;

  await ptyClient.onOutput((data) => {
    const text = new TextDecoder().decode(data);
    terminal.textContent += text;
    // Auto-scroll to bottom
    terminal.scrollTop = terminal.scrollHeight;
  });

  await ptyClient.onExit((code) => {
    terminal.textContent += `\n[Process exited with code ${code}]\n`;
  });

  await ptyClient.onError((message) => {
    console.error("PTY error:", message);
    terminal.textContent += `\n[Error: ${message}]\n`;
  });
}

/**
 * Check if a key event represents a special key that should bypass IME.
 */
function isSpecialKey(event: KeyboardEvent): boolean {
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
  if (event.key === "Escape" || event.key === "Tab" || event.key === "Insert") {
    return true;
  }

  return false;
}

/**
 * Handle keyboard input
 */
async function handleKeyDown(event: KeyboardEvent): Promise<void> {
  if (IME_DEBUG) {
    console.log("[Debug] handleKeyDown:", {
      key: event.key,
      target: (event.target as HTMLElement)?.tagName,
      activeElement: document.activeElement?.tagName,
    });
  }
  if (!ptyClient || !shouldHandleKey(event)) {
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
  if (editContext) {
    // Only process special keys that EditContext doesn't handle
    if (!isSpecialKey(event)) {
      return; // Let EditContext handle regular input
    }
    // Enter should be handled by EditContext for IME confirmation
    if (event.key === "Enter" && !event.ctrlKey && !event.altKey) {
      return;
    }
    // Special keys (Ctrl+C, arrows, etc.) fall through to be processed
  }

  // Skip if hidden textarea has focus (IME is active) - fallback mode
  if (imeInput && document.activeElement === imeInput) {
    // Only allow certain special keys to pass through
    // Enter should be handled by IME for confirmation
    if (event.key === "Enter") {
      return; // Let IME handler process Enter
    }
    if (!isSpecialKey(event)) {
      return; // Let IME handler process regular keys
    }
    // Navigation and function keys fall through
  }

  const bytes = keyEventToBytes(event);
  if (bytes) {
    event.preventDefault();
    try {
      await ptyClient.write(bytes);
    } catch (error) {
      console.error("Failed to write to PTY:", error);
    }
  }
}

/**
 * Update IME input position to match terminal cursor.
 */
function updateIMEPosition(): void {
  if (!imeInput || !terminalState) {
    return;
  }

  const cursorCol = terminalState.cursorCol;
  const cursorRow = terminalState.cursorRow;
  const rows = terminalState.rows;

  // Get terminal container
  const terminal = document.getElementById("terminal");
  if (!terminal) return;

  const rect = terminal.getBoundingClientRect();

  // Get computed styles for accurate padding
  const styles = getComputedStyle(terminal);
  const paddingLeft = parseFloat(styles.paddingLeft) || 0;
  const paddingTop = parseFloat(styles.paddingTop) || 0;

  // Get scroll offset if available
  const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;

  // Calculate pixel position
  const x = cursorCol * charSize.width + paddingLeft;
  const y = cursorRow * charSize.height + paddingTop - scrollOffset;

  // Determine vertical position (handle bottom row)
  let top: number;
  if (cursorRow === rows - 1) {
    // Bottom row - position above cursor
    top = rect.top + y - charSize.height;
  } else {
    // Other rows - position below cursor
    top = rect.top + y + charSize.height;
  }

  // Apply position
  imeInput.style.left = `${rect.left + x}px`;
  imeInput.style.top = `${top}px`;
}

/**
 * Set up IME using EditContext API (Chromium/WebView2 only).
 */
function setupEditContextIME(terminal: HTMLElement, view: HTMLDivElement): void {
  const EditContextClass = (window as any).EditContext as EditContextConstructor;
  editContext = new EditContextClass();

  // Make terminal editable with EditContext
  (terminal as any).editContext = editContext;
  terminal.tabIndex = 0;

  let compositionText = "";
  let isComposing = false;

  // Handle text updates (both direct input and composition)
  const onTextUpdate = (event: any) => {
    if (IME_DEBUG) {
      console.log("[EditContext] textupdate:", {
        text: event.text,
        selectionStart: event.selectionStart,
        selectionEnd: event.selectionEnd,
        compositionStart: event.compositionStart,
        compositionEnd: event.compositionEnd,
      });
    }

    const text = event.text;

    if (isComposing) {
      // Update composition view
      compositionText = text;
      updateCompositionView(view, text);
    } else {
      // Direct input - send to PTY
      if (text) {
        if (ptyClient) {
          const bytes = new TextEncoder().encode(text);
          ptyClient.write(bytes).catch((error) => {
            console.error("Failed to write to PTY:", error);
          });
        } else if (IME_DEBUG) {
          console.warn("[EditContext] PTY client not ready, input dropped:", text);
        }
      }
    }

    // Update EditContext's text bounds for IME positioning
    if (editContext) {
      updateEditContextBounds(terminal, editContext);
    }
  };

  // Handle composition start
  const onCompositionStart = (event: any) => {
    if (IME_DEBUG) console.log("[EditContext] compositionstart");
    isComposing = true;
    compositionText = "";
  };

  // Handle composition end
  const onCompositionEnd = (event: any) => {
    if (IME_DEBUG) console.log("[EditContext] compositionend");
    isComposing = false;

    // Send the final composition text to PTY
    if (compositionText) {
      if (ptyClient) {
        const bytes = new TextEncoder().encode(compositionText);
        ptyClient.write(bytes).catch((error) => {
          console.error("Failed to write to PTY:", error);
        });
      } else if (IME_DEBUG) {
        console.warn("[EditContext] PTY client not ready, composition dropped:", compositionText);
      }
    }

    // Clear composition view
    compositionText = "";
    updateCompositionView(view, "");

    // Reset EditContext text
    if (editContext) {
      editContext.updateText(0, editContext.text.length, "");
      editContext.updateSelection(0, 0);
    }
  };

  // Handle character bounds request (for IME candidate window positioning)
  const onCharacterBoundsUpdate = (event: any) => {
    if (IME_DEBUG) console.log("[EditContext] characterboundsupdate:", event);
    if (editContext) {
      updateEditContextBounds(terminal, editContext);
    }
  };

  // Focus terminal to activate EditContext
  const onTerminalClick = () => {
    terminal.focus();
  };

  // Add event listeners
  editContext.addEventListener("textupdate", onTextUpdate);
  editContext.addEventListener("compositionstart", onCompositionStart);
  editContext.addEventListener("compositionend", onCompositionEnd);
  editContext.addEventListener("characterboundsupdate", onCharacterBoundsUpdate);
  terminal.addEventListener("click", onTerminalClick);

  // Store cleanup function
  editContextCleanup = () => {
    if (editContext) {
      editContext.removeEventListener("textupdate", onTextUpdate);
      editContext.removeEventListener("compositionstart", onCompositionStart);
      editContext.removeEventListener("compositionend", onCompositionEnd);
      editContext.removeEventListener("characterboundsupdate", onCharacterBoundsUpdate);
    }
    terminal.removeEventListener("click", onTerminalClick);
  };

  // Initial focus
  terminal.focus();
}

/**
 * Update EditContext bounds for IME positioning.
 */
function updateEditContextBounds(terminal: HTMLElement, ctx: EditContext): void {
  if (!terminalState) return;

  const cursorCol = terminalState.cursorCol;
  const cursorRow = terminalState.cursorRow;

  const rect = terminal.getBoundingClientRect();

  // Get computed styles for accurate padding
  const styles = getComputedStyle(terminal);
  const paddingLeft = parseFloat(styles.paddingLeft) || 0;
  const paddingTop = parseFloat(styles.paddingTop) || 0;

  // Get scroll offset if available
  const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;

  // Calculate pixel position (including padding and scroll offset)
  const x = rect.left + cursorCol * charSize.width + paddingLeft;
  const y = rect.top + cursorRow * charSize.height + paddingTop - scrollOffset;

  // Set control bounds (the editable area)
  ctx.updateControlBounds(
    new DOMRect(rect.left, rect.top, rect.width, rect.height)
  );

  // Set selection bounds (cursor position)
  ctx.updateSelectionBounds(
    new DOMRect(x, y, charSize.width, charSize.height)
  );

  // Set character bounds for composition text
  const textLength = ctx.text?.length || 0;
  if (textLength > 0) {
    const bounds: DOMRect[] = [];
    for (let i = 0; i < textLength; i++) {
      bounds.push(
        new DOMRect(
          x + i * charSize.width,
          y,
          charSize.width,
          charSize.height
        )
      );
    }
    ctx.updateCharacterBounds(0, bounds);
  }
}

/**
 * Check if the input contains SKK conversion markers.
 * SKK uses special markers that indicate conversion is in progress:
 * - ▽ (U+25BD): Waiting for conversion (hiragana input)
 * - ▼ (U+25BC): Converting (candidate selection)
 * - 【】: Annotation (dictionary registration)
 */
function hasSKKMarker(text: string): boolean {
  return text.includes("▽") || text.includes("▼") || /【.*】/.test(text);
}

/**
 * Update composition view position and content.
 */
function updateCompositionView(view: HTMLDivElement, text: string): void {
  if (IME_DEBUG) {
    console.log("[IME Debug] updateCompositionView:", {
      text,
      hasTerminalState: !!terminalState,
      viewId: view.id,
    });
  }

  if (!terminalState) return;

  if (!text) {
    view.style.display = "none";
    view.textContent = "";
    return;
  }

  const terminal = document.getElementById("terminal");
  if (!terminal) return;

  const rect = terminal.getBoundingClientRect();
  const cursorCol = terminalState.cursorCol;
  const cursorRow = terminalState.cursorRow;

  // Get computed styles for accurate padding
  const styles = getComputedStyle(terminal);
  const paddingLeft = parseFloat(styles.paddingLeft) || 0;
  const paddingTop = parseFloat(styles.paddingTop) || 0;

  // Get scroll offset if available
  const scrollOffset = (terminalState as any).getScrollOffset?.() ?? 0;

  // Position at cursor (use fixed positioning relative to viewport, including padding and scroll offset)
  const x = rect.left + cursorCol * charSize.width + paddingLeft;
  const y = rect.top + cursorRow * charSize.height + paddingTop - scrollOffset;

  if (IME_DEBUG) {
    console.log("[IME Debug] positioning compositionView at:", { x, y, cursorCol, cursorRow, rectLeft: rect.left, rectTop: rect.top, paddingLeft, paddingTop, scrollOffset });
  }

  view.style.left = `${x}px`;
  view.style.top = `${y}px`;
  view.style.display = "block";
  view.textContent = text;
}

/**
 * Set up IME event handlers.
 */
function setupIMEHandlers(input: HTMLTextAreaElement, view: HTMLDivElement): void {
  // Duplicate detection
  let lastSentValue = "";
  let lastSentTimestamp = 0;
  // Track if we're in composition mode
  let isComposing = false;

  // Focus debugging
  input.addEventListener("focus", () => {
    if (IME_DEBUG) console.log("[IME Debug] textarea focus gained");
  });
  input.addEventListener("blur", () => {
    if (IME_DEBUG) console.log("[IME Debug] textarea focus lost, activeElement:", document.activeElement?.tagName);
  });

  // Handle compositionstart to reset flags
  input.addEventListener("compositionstart", (event) => {
    if (IME_DEBUG) {
      console.log("[IME Debug] compositionstart:", {
        data: (event as CompositionEvent).data,
        inputValue: input.value,
      });
    }
    isComposing = true;
  });

  // Handle compositionupdate - show current composition
  input.addEventListener("compositionupdate", (event) => {
    const ce = event as CompositionEvent;
    if (IME_DEBUG) {
      console.log("[IME Debug] compositionupdate:", {
        data: ce.data,
        inputValue: input.value,
        viewDisplay: view.style.display,
      });
    }
    // Show composition text in view (use event.data if input.value is empty)
    const displayText = input.value || ce.data || "";
    if (displayText) {
      updateCompositionView(view, displayText);
    }
  });

  // Handle any input changes (for SKK which may not use composition events properly)
  input.addEventListener("beforeinput", (event) => {
    if (IME_DEBUG) {
      console.log("[IME Debug] beforeinput:", {
        inputType: event.inputType,
        data: event.data,
        isComposing: event.isComposing,
      });
    }
  });

  // Handle compositioncancel to cleanup
  input.addEventListener("compositioncancel", () => {
    if (IME_DEBUG) console.log("[IME Debug] compositioncancel");
    isComposing = false;
    input.value = "";
    updateCompositionView(view, "");
  });

  // Handle input event (primary handler)
  input.addEventListener("input", async (event: Event) => {
    const inputEvent = event as InputEvent;
    const value = input.value;
    if (IME_DEBUG) {
      console.log("[IME Debug] input event:", {
        value,
        isComposing: inputEvent.isComposing,
        localIsComposing: isComposing,
        inputType: inputEvent.inputType,
        data: inputEvent.data,
        hasSKKMarker: hasSKKMarker(value),
      });
    }

    // If composing (standard IME or SKK with markers), show in composition view
    if (inputEvent.isComposing || isComposing || hasSKKMarker(value)) {
      if (IME_DEBUG) console.log("[IME Debug] input: composing, updating view");
      updateCompositionView(view, value);
      return;
    }

    // Not composing - this is final input, send to PTY
    if (!value) {
      return;
    }

    // Duplicate detection - skip if same value sent within 100ms
    const now = Date.now();
    if (value === lastSentValue && now - lastSentTimestamp < 100) {
      if (IME_DEBUG) console.log("[IME Debug] input: duplicate, skipping");
      input.value = "";
      updateCompositionView(view, "");
      return;
    }

    try {
      if (IME_DEBUG) console.log("[IME Debug] input: sending value:", value);
      // Encode as UTF-8 and send to PTY
      const bytes = new TextEncoder().encode(value);
      if (ptyClient) {
        await ptyClient.write(bytes);
        // Set duplicate tracking AFTER successful write
        lastSentValue = value;
        lastSentTimestamp = now;
        if (IME_DEBUG) console.log("[IME Debug] input: sent successfully");
      } else if (IME_DEBUG) {
        console.warn("[IME Debug] PTY client not ready, input dropped:", value);
      }
    } catch (error) {
      console.error("Failed to write IME input to PTY:", error);
      // Don't set lastSent on failure - allow retry
    } finally {
      // Clear input, view, and reset flag
      input.value = "";
      updateCompositionView(view, "");
    }
  });

  // Handle compositionend (fallback for standard IME)
  input.addEventListener("compositionend", async (event) => {
    if (IME_DEBUG) {
      console.log("[IME Debug] compositionend:", {
        data: (event as CompositionEvent).data,
        inputValue: input.value,
      });
    }

    // Mark composition as ended
    isComposing = false;

    const value = input.value;
    if (!value) {
      if (IME_DEBUG) console.log("[IME Debug] compositionend: no value, returning");
      updateCompositionView(view, "");
      return;
    }

    // Skip if SKK markers still present (SKK uses compositionend differently)
    if (hasSKKMarker(value)) {
      if (IME_DEBUG) console.log("[IME Debug] compositionend: SKK marker found, keeping view");
      updateCompositionView(view, value);
      return;
    }

    // Duplicate detection - skip if same value sent within 100ms
    const now = Date.now();
    if (value === lastSentValue && now - lastSentTimestamp < 100) {
      if (IME_DEBUG) console.log("[IME Debug] compositionend: duplicate detected, skipping");
      input.value = "";
      updateCompositionView(view, "");
      return;
    }

    try {
      if (IME_DEBUG) console.log("[IME Debug] compositionend: sending value:", value);
      const bytes = new TextEncoder().encode(value);
      if (ptyClient) {
        await ptyClient.write(bytes);
        // Set duplicate tracking AFTER successful write
        lastSentValue = value;
        lastSentTimestamp = now;
        if (IME_DEBUG) console.log("[IME Debug] compositionend: sent successfully");
      } else if (IME_DEBUG) {
        console.warn("[IME Debug] PTY client not ready, composition dropped:", value);
      }
    } catch (error) {
      console.error("Failed to write IME composition to PTY:", error);
      // Don't set lastSent on failure - allow retry
    } finally {
      input.value = "";
      updateCompositionView(view, "");
    }
  });
}

/**
 * Set up mouse event handlers.
 */
function setupMouseHandlers(container: HTMLElement): void {
  const handleMouseEvent = async (
    event: MouseEvent | WheelEvent,
    type: "down" | "up" | "move" | "wheel",
  ) => {
    if (!terminalState || !ptyClient) return;

    const modes = terminalState.getModes();
    if (!isMouseTrackingEnabled(modes.mouseTracking)) return;

    const rect = container.getBoundingClientRect();
    const mouseEvent = domEventToMouseEvent(
      event,
      charSize.width,
      charSize.height,
      rect,
      type,
    );

    if (!mouseEvent) return;

    const encoded = encodeMouseEvent(
      mouseEvent,
      modes.mouseTracking,
      modes.mouseEncoding,
    );
    if (encoded) {
      event.preventDefault();
      try {
        await ptyClient.write(encoded);
      } catch (error) {
        console.error("Failed to send mouse event:", error);
      }
    }
  };

  const onMouseDown = (e: MouseEvent) => handleMouseEvent(e, "down");
  const onMouseUp = (e: MouseEvent) => handleMouseEvent(e, "up");
  const onMouseMove = (e: MouseEvent) => {
    // Only track motion if a button is pressed or any-event mode
    if (terminalState) {
      const modes = terminalState.getModes();
      if (modes.mouseTracking === "any" || e.buttons !== 0) {
        handleMouseEvent(e, "move");
      }
    }
  };
  const onWheel = (e: WheelEvent) => handleMouseEvent(e, "wheel");

  // Prevent context menu when mouse tracking is enabled
  const onContextMenu = (e: MouseEvent) => {
    if (
      terminalState &&
      isMouseTrackingEnabled(terminalState.getModes().mouseTracking)
    ) {
      e.preventDefault();
    }
  };

  container.addEventListener("mousedown", onMouseDown);
  container.addEventListener("mouseup", onMouseUp);
  container.addEventListener("mousemove", onMouseMove);
  container.addEventListener("wheel", onWheel, { passive: false });
  container.addEventListener("contextmenu", onContextMenu);

  // Store cleanup functions
  mouseEventListeners.push(
    () => container.removeEventListener("mousedown", onMouseDown),
    () => container.removeEventListener("mouseup", onMouseUp),
    () => container.removeEventListener("mousemove", onMouseMove),
    () => container.removeEventListener("wheel", onWheel),
    () => container.removeEventListener("contextmenu", onContextMenu),
  );
}

/**
 * Cleanup function
 */
function cleanup(): void {
  if (disconnectResizeObserver) {
    disconnectResizeObserver();
    disconnectResizeObserver = null;
  }

  // Remove mouse event listeners
  for (const removeListener of mouseEventListeners) {
    removeListener();
  }
  mouseEventListeners = [];

  // Remove IME input element
  if (imeInput && imeInput.parentNode) {
    imeInput.parentNode.removeChild(imeInput);
    imeInput = null;
  }

  // Remove composition view
  if (compositionView && compositionView.parentNode) {
    compositionView.parentNode.removeChild(compositionView);
    compositionView = null;
  }

  // Clean up EditContext event listeners
  if (editContextCleanup) {
    editContextCleanup();
    editContextCleanup = null;
  }
  if (editContext) {
    editContext = null;
  }

  // Clean up terminal click handler
  if (terminalClickHandler) {
    const terminal = document.getElementById("terminal");
    if (terminal) {
      terminal.removeEventListener("mousedown", terminalClickHandler);
    }
    terminalClickHandler = null;
  }

  if (ptyClient) {
    ptyClient.dispose();
    ptyClient.kill().catch(console.error);
    ptyClient = null;
  }

  terminalState = null;
  terminalRenderer = null;

  document.removeEventListener("keydown", handleKeyDown);
}

// Initialize when DOM is ready
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", initTerminal);
} else {
  initTerminal();
}

// Cleanup on page unload
window.addEventListener("beforeunload", cleanup);

// Type declaration for E2E testing globals
declare global {
  interface Window {
    terminalState: typeof terminalState;
    ptyClient: typeof ptyClient;
    terminalRenderer: typeof terminalRenderer;
  }
}
