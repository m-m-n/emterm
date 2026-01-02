/**
 * eMterm - Terminal Emulator
 * Main entry point
 */

import { PtyClient, keyEventToBytes, shouldHandleKey, measureCharacterSize, observeContainerResize } from "./pty";

// Terminal configuration
const FONT_FAMILY = "monospace";
const FONT_SIZE = 14;

// Global state
let ptyClient: PtyClient | null = null;
let disconnectResizeObserver: (() => void) | null = null;

/**
 * Initialize the terminal
 */
async function initTerminal(): Promise<void> {
  const terminal = document.getElementById("terminal");
  if (!terminal) {
    console.error("Terminal element not found");
    return;
  }

  // Apply terminal styles
  terminal.style.fontFamily = FONT_FAMILY;
  terminal.style.fontSize = `${FONT_SIZE}px`;
  terminal.style.whiteSpace = "pre-wrap";
  terminal.style.overflow = "auto";
  terminal.style.padding = "8px";
  terminal.style.backgroundColor = "#1e1e1e";
  terminal.style.color = "#d4d4d4";
  terminal.style.height = "100%";
  terminal.style.boxSizing = "border-box";

  // Measure character size
  const charSize = measureCharacterSize(FONT_FAMILY, FONT_SIZE);

  // Create PTY client
  ptyClient = new PtyClient();

  // Set up event handlers
  await ptyClient.onOutput((data) => {
    const text = new TextDecoder().decode(data);
    terminal.textContent += text;
    // Auto-scroll to bottom
    terminal.scrollTop = terminal.scrollHeight;
  });

  await ptyClient.onExit((code) => {
    terminal.textContent += `\n[Process exited with code ${code}]\n`;
    console.log("PTY exited with code:", code);
  });

  await ptyClient.onError((message) => {
    console.error("PTY error:", message);
    terminal.textContent += `\n[Error: ${message}]\n`;
  });

  // Calculate initial terminal size
  const initialSize = {
    cols: Math.floor((terminal.clientWidth - 16) / charSize.width),
    rows: Math.floor((terminal.clientHeight - 16) / charSize.height),
  };

  // Spawn PTY session
  try {
    const sessionId = await ptyClient.spawn({
      cols: Math.max(1, initialSize.cols),
      rows: Math.max(1, initialSize.rows),
    });
    console.log("PTY session started:", sessionId);
  } catch (error) {
    console.error("Failed to spawn PTY:", error);
    terminal.textContent = `Failed to start terminal: ${error}`;
    return;
  }

  // Set up keyboard input handler
  document.addEventListener("keydown", handleKeyDown);

  // Set up resize observer
  disconnectResizeObserver = observeContainerResize(
    terminal,
    charSize.width,
    charSize.height,
    async (cols, rows) => {
      if (ptyClient) {
        try {
          await ptyClient.resize(cols, rows);
          console.log(`Resized to ${cols}x${rows}`);
        } catch (error) {
          console.error("Failed to resize PTY:", error);
        }
      }
    }
  );

  // Focus handling - make terminal focusable
  terminal.tabIndex = 0;
  terminal.focus();
}

/**
 * Handle keyboard input
 */
async function handleKeyDown(event: KeyboardEvent): Promise<void> {
  if (!ptyClient || !shouldHandleKey(event)) {
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
 * Cleanup function
 */
function cleanup(): void {
  if (disconnectResizeObserver) {
    disconnectResizeObserver();
    disconnectResizeObserver = null;
  }

  if (ptyClient) {
    ptyClient.dispose();
    ptyClient.kill().catch(console.error);
    ptyClient = null;
  }

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

console.log("eMterm initialized");
