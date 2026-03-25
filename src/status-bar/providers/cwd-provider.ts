/**
 * CWD Provider
 *
 * Provides {cwd} variable showing the basename of the current working directory.
 * Receives CWD updates from OSC 7 events via TerminalState.
 */

import type { VariableProvider } from "./types";

/**
 * Extract basename from a path, supporting Unix, Windows, and file:// URIs.
 */
export function extractBasename(path: string): string {
  if (!path) return "";

  let normalized = path;

  // Handle file:// URI (OSC 7 format)
  if (normalized.startsWith("file://")) {
    // file:///path or file://hostname/path
    normalized = normalized.replace(/^file:\/\/[^/]*/, "");
    // Decode percent-encoded characters
    try {
      normalized = decodeURIComponent(normalized);
    } catch {
      // Ignore decode errors
    }
  }

  // Remove trailing slashes (but preserve root)
  if (normalized.length > 1) {
    normalized = normalized.replace(/[/\\]+$/, "");
  }

  // Handle root paths
  if (normalized === "/") return "/";
  // Windows drive root: "C:" after trailing slash removal
  if (/^[A-Za-z]:$/.test(normalized)) return normalized + "\\";
  if (/^[A-Za-z]:\\$/.test(normalized)) return normalized;

  // Extract basename from last separator
  const lastSlash = Math.max(normalized.lastIndexOf("/"), normalized.lastIndexOf("\\"));
  if (lastSlash >= 0) {
    return normalized.substring(lastSlash + 1);
  }

  return normalized;
}

/**
 * CwdProvider implements VariableProvider for the {cwd} variable.
 * It receives CWD updates from a callback (typically from TerminalState).
 */
export class CwdProvider implements VariableProvider {
  private cwd = "";

  getValue(): string {
    return this.cwd;
  }

  /**
   * Update the CWD value (called from OSC 7 handler or polling).
   */
  setCwd(fullPath: string): void {
    this.cwd = extractBasename(fullPath);
  }

  dispose(): void {
    // No resources to clean up
  }
}
