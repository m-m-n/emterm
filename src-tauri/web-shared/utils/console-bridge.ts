/**
 * Console bridge module
 * Forwards all console methods (log, warn, error, info, debug) to Rust backend
 * while preserving original console functionality for DevTools.
 */

import { invoke } from "@tauri-apps/api/core";

// Store original console methods
const originalConsole = {
  log: console.log.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  info: console.info.bind(console),
  debug: console.debug.bind(console),
};

/**
 * Format console arguments to string
 */
function formatArgs(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "string") {
        return arg;
      }
      if (arg instanceof Error) {
        return `${arg.name}: ${arg.message}\n${arg.stack || ""}`;
      }
      try {
        return JSON.stringify(arg, null, 2);
      } catch {
        return String(arg);
      }
    })
    .join(" ");
}

/**
 * Initialize console bridging to Rust backend
 */
export function initConsoleBridge(): void {
  // Override console.log -> stdout
  console.log = (...args: unknown[]) => {
    originalConsole.log(...args);
    const message = formatArgs(args);
    invoke("console_log", { message }).catch((err) => {
      originalConsole.error("Failed to forward console.log:", err);
    });
  };

  // Override console.warn -> stderr
  console.warn = (...args: unknown[]) => {
    originalConsole.warn(...args);
    const message = formatArgs(args);
    invoke("console_warn", { message }).catch((err) => {
      originalConsole.error("Failed to forward console.warn:", err);
    });
  };

  // Override console.error -> stderr
  console.error = (...args: unknown[]) => {
    originalConsole.error(...args);
    const message = formatArgs(args);
    invoke("console_error", { message }).catch((err) => {
      originalConsole.error("Failed to forward console.error:", err);
    });
  };

  // Override console.info -> stdout
  console.info = (...args: unknown[]) => {
    originalConsole.info(...args);
    const message = formatArgs(args);
    invoke("console_info", { message }).catch((err) => {
      originalConsole.error("Failed to forward console.info:", err);
    });
  };

  // Override console.debug -> stdout
  console.debug = (...args: unknown[]) => {
    originalConsole.debug(...args);
    const message = formatArgs(args);
    invoke("console_debug", { message }).catch((err) => {
      originalConsole.error("Failed to forward console.debug:", err);
    });
  };
}

/**
 * Restore original console methods
 */
export function restoreConsole(): void {
  console.log = originalConsole.log;
  console.warn = originalConsole.warn;
  console.error = originalConsole.error;
  console.info = originalConsole.info;
  console.debug = originalConsole.debug;
}
