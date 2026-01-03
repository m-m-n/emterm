/**
 * eMterm - Terminal Emulator
 * Main entry point
 */

import { PtyClient, keyEventToBytes, shouldHandleKey, measureCharacterSize, observeContainerResize } from "./pty";
import { TerminalState, TerminalRenderer, encodeMouseEvent, domEventToMouseEvent, isMouseTrackingEnabled } from "./terminal";
import type { TerminalActionsPayload } from "./types/terminal.ts";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

// Terminal configuration
const FONT_FAMILY = "monospace";
const FONT_SIZE = 14;

// Feature flag for new terminal rendering
const USE_NEW_TERMINAL = true;

// Global state
let ptyClient: PtyClient | null = null;
let disconnectResizeObserver: (() => void) | null = null;
let terminalState: TerminalState | null = null;
let terminalRenderer: TerminalRenderer | null = null;
let charSize: { width: number; height: number } = { width: 8, height: 16 };
let mouseEventListeners: (() => void)[] = [];

/**
 * Initialize the terminal
 */
async function initTerminal(): Promise<void> {
  const terminal = document.getElementById("terminal");
  if (!terminal) {
    console.error("Terminal element not found");
    return;
  }

  // Measure character size
  charSize = measureCharacterSize(FONT_FAMILY, FONT_SIZE);

  // Calculate initial terminal size
  const initialSize = {
    cols: Math.floor((terminal.clientWidth - 16) / charSize.width),
    rows: Math.floor((terminal.clientHeight - 16) / charSize.height),
  };

  const cols = Math.max(1, initialSize.cols);
  const rows = Math.max(1, initialSize.rows);

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
      if (ptyClient) {
        try {
          await ptyClient.resize(newCols, newRows);

          // Resize terminal state if using new system
          if (terminalState && terminalRenderer) {
            terminalState.resize(newCols, newRows);
            terminalRenderer.resize(newCols, newRows);
            // Force re-render after resize to recreate line elements
            terminalRenderer.forceRender(terminalState);
          }
        } catch (error) {
          console.error("Failed to resize PTY:", error);
        }
      }
    }
  );

  // Focus handling - make terminal focusable
  terminal.tabIndex = 0;
  terminal.focus();

  // Expose for E2E testing (must be after initialization)
  window.terminalState = terminalState;
  window.ptyClient = ptyClient;
  window.terminalRenderer = terminalRenderer;
}

/**
 * Initialize new terminal rendering system.
 */
function initNewTerminal(container: HTMLElement, cols: number, rows: number): void {
  // Apply terminal styles
  container.style.backgroundColor = "#1e1e1e";
  container.style.color = "#d4d4d4";
  container.style.height = "100%";
  container.style.boxSizing = "border-box";
  container.style.padding = "8px";

  // Create terminal state and renderer
  terminalState = new TerminalState(cols, rows);
  terminalRenderer = new TerminalRenderer(container, FONT_FAMILY, FONT_SIZE);

  // Initial render
  terminalRenderer.forceRender(terminalState);
}

/**
 * Initialize legacy terminal rendering.
 */
function initLegacyTerminal(container: HTMLElement): void {
  container.style.fontFamily = FONT_FAMILY;
  container.style.fontSize = `${FONT_SIZE}px`;
  container.style.whiteSpace = "pre-wrap";
  container.style.overflow = "auto";
  container.style.padding = "8px";
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
  });

  // Handle exit and error events
  await ptyClient.onExit(async (code) => {
    // Close the window when shell exits
    try {
      const appWindow = getCurrentWebviewWindow();
      await appWindow.close();
    } catch (error) {
      console.error("Failed to close window:", error);
    }
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
 * Handle keyboard input
 */
async function handleKeyDown(event: KeyboardEvent): Promise<void> {
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
 * Set up mouse event handlers.
 */
function setupMouseHandlers(container: HTMLElement): void {
  const handleMouseEvent = async (
    event: MouseEvent | WheelEvent,
    type: "down" | "up" | "move" | "wheel"
  ) => {
    if (!terminalState || !ptyClient) return;

    const modes = terminalState.getModes();
    if (!isMouseTrackingEnabled(modes.mouseTracking)) return;

    const rect = container.getBoundingClientRect();
    const mouseEvent = domEventToMouseEvent(event, charSize.width, charSize.height, rect, type);

    if (!mouseEvent) return;

    const encoded = encodeMouseEvent(mouseEvent, modes.mouseTracking, modes.mouseEncoding);
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
    if (terminalState && isMouseTrackingEnabled(terminalState.getModes().mouseTracking)) {
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
    () => container.removeEventListener("contextmenu", onContextMenu)
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

