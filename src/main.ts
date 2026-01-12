/**
 * eMterm - Terminal Emulator
 * Entry point
 */

import { TerminalApp } from "./terminal-app";
import { initConsoleBridge } from "./utils/console-bridge";

let app: TerminalApp | null = null;

/**
 * Initialize the terminal application
 */
async function main(): Promise<void> {
  // Initialize console bridge to forward logs to stdout/stderr
  initConsoleBridge();

  const container = document.getElementById("terminal");
  if (!container) {
    console.error("Terminal element not found");
    return;
  }

  app = new TerminalApp(container);
  await app.init();

  // Expose for E2E testing
  window.terminalApp = app;
  window.terminalState = app.terminalState;
  window.terminalRenderer = app.terminalRenderer;
}

/**
 * Cleanup resources before unload
 */
function cleanup(): void {
  if (app) {
    app.dispose();
    app = null;
  }
}

// Initialize when DOM is ready
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", main);
} else {
  main();
}

// Cleanup on page unload
window.addEventListener("beforeunload", cleanup);

// Type declarations for E2E testing globals
declare global {
  interface Window {
    terminalApp: TerminalApp | null;
    terminalState: import("./terminal/state").TerminalState | null;
    terminalRenderer: import("./terminal/renderer").TerminalRenderer | null;
    ptyClient: import("./pty/client").PtyClient | null;
  }
}
