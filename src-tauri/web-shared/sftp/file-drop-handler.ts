/**
 * File Drop Handler
 *
 * Handles drag & drop events on the terminal area using Tauri's
 * onDragDropEvent API. Shows a drop overlay during drag, routes
 * drops to either SFTP upload (SSH tabs) or path paste (non-SSH tabs).
 *
 * Note: Tauri's built-in drag-drop handler (dragDropEnabled: true by default)
 * intercepts OS-level file drops before they reach the WebView, so HTML5
 * Drag and Drop API cannot be used. Tauri's DragDropEvent provides absolute
 * file paths directly via event.payload.paths.
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { t } from "../i18n/index.ts";

/**
 * Context required by the FileDropHandler.
 */
export interface FileDropHandlerContext {
  /** The terminal container element to display the overlay on */
  container: HTMLElement;
  /** Returns true if this tab is the currently active (visible) tab */
  isActiveTab: () => boolean;
  /** Returns the SSH connection name for the active tab (empty string = non-SSH) */
  getSshConnectionName: () => string;
  /** Called when files are dropped on an SSH tab */
  onSshDrop: (files: FileDropInfo[]) => void;
  /** Called when files are dropped on a non-SSH tab */
  onLocalDrop: (paths: string[]) => void;
}

/**
 * Information about a dropped file or directory.
 */
export interface FileDropInfo {
  /** Absolute path of the dropped file/directory */
  path: string;
  /** Display name (filename without directory) */
  name: string;
  /** Whether this is a directory (not reliably detectable from Tauri events; checked via backend if needed) */
  isDirectory: boolean;
}

/**
 * Manages file drag & drop events on the terminal area via Tauri's DragDropEvent API.
 */
export class FileDropHandler {
  private ctx: FileDropHandlerContext;
  private overlay: HTMLElement | null = null;
  private unlisten: UnlistenFn | null = null;

  constructor(ctx: FileDropHandlerContext) {
    this.ctx = ctx;
  }

  /**
   * Attach Tauri drag & drop event listener.
   * Must be awaited as onDragDropEvent returns a Promise.
   */
  async attach(): Promise<void> {
    const appWindow = getCurrentWebviewWindow();
    this.unlisten = await appWindow.onDragDropEvent((event) => {
      // Only handle events for the active tab (Tauri fires window-level events)
      if (!this.ctx.isActiveTab()) return;

      switch (event.payload.type) {
        case "enter":
          this.showOverlay();
          break;
        case "leave":
          this.hideOverlay();
          break;
        case "drop":
          this.hideOverlay();
          this.handleDrop(event.payload.paths);
          break;
      }
    });
  }

  /**
   * Detach event listener and clean up.
   */
  detach(): void {
    this.unlisten?.();
    this.unlisten = null;
    this.hideOverlay();
  }

  /**
   * Handle file drop with absolute paths from Tauri.
   */
  private handleDrop(paths: string[]): void {
    if (paths.length === 0) return;

    const files: FileDropInfo[] = paths.map((p) => ({
      path: p,
      name: p.split(/[/\\]/).pop() || p,
      // Tauri's DragDropEvent does not distinguish files from directories;
      // the backend checks via std::fs::metadata if needed
      isDirectory: false,
    }));

    const sshConnectionName = this.ctx.getSshConnectionName();
    if (sshConnectionName) {
      // SSH tab: trigger upload workflow
      this.ctx.onSshDrop(files);
    } else {
      // Non-SSH tab: paste file paths
      this.ctx.onLocalDrop(paths);
    }
  }

  /**
   * Show the drop overlay on the terminal.
   */
  private showOverlay(): void {
    if (this.overlay) return;

    this.overlay = document.createElement("div");
    this.overlay.className = "sftp-drop-overlay";

    const sshConnectionName = this.ctx.getSshConnectionName();
    const message = document.createElement("div");
    message.className = "sftp-drop-overlay-message";
    message.textContent = sshConnectionName
      ? t("sftp.dropOverlay.upload")
      : t("sftp.dropOverlay.paste");

    this.overlay.appendChild(message);
    this.ctx.container.appendChild(this.overlay);
  }

  /**
   * Hide and remove the drop overlay.
   */
  private hideOverlay(): void {
    if (this.overlay) {
      this.overlay.remove();
      this.overlay = null;
    }
  }
}

/**
 * Extract the path component from an OSC 7 working directory value.
 *
 * OSC 7 data can be:
 * - A file:// URL: "file://hostname/path/to/dir" → "/path/to/dir"
 * - A plain path: "/path/to/dir" → "/path/to/dir"
 * - Empty: "" → "" (caller should use sftp default = home directory)
 */
export function extractRemotePath(oscWorkingDirectory: string): string {
  if (!oscWorkingDirectory) return "";

  // Handle file:// URL format
  if (oscWorkingDirectory.startsWith("file://")) {
    try {
      const url = new URL(oscWorkingDirectory);
      return decodeURIComponent(url.pathname) || "";
    } catch {
      // Malformed URL, try to extract path manually
      const match = oscWorkingDirectory.match(/^file:\/\/[^/]*(\/.*)/);
      return match?.[1] ?? "";
    }
  }

  return oscWorkingDirectory;
}

/**
 * Format file paths for pasting into the terminal.
 * Paths with spaces are quoted. Multiple paths are space-separated.
 */
export function formatPathsForPaste(paths: string[]): string {
  return paths
    .map((p) => (p.includes(" ") ? `"${p}"` : p))
    .join(" ");
}
