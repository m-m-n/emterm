/**
 * Link handler for URL/file path detection and opening.
 *
 * Handles Ctrl+click to open URLs and file paths, and hover cursor
 * feedback for clickable links and fold regions.
 */

import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import type { CharSize } from "../types";
import { SettingsService } from "../../settings/settings-service";
import {
  findUrlAtPosition,
  findFilePathAtPosition,
  getLogicalLine,
  physicalToLogicalCol,
} from "../../terminal/url-detector";

/**
 * Context interface for LinkHandler to access terminal state and UI.
 */
export interface LinkHandlerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getTerminalRoot: () => HTMLElement | null;
  getCharSize: () => CharSize;
}

/**
 * Handles URL/file path link detection, hover cursor feedback, and Ctrl+click opening.
 */
export class LinkHandler {
  private lastMouseEvent: MouseEvent | null = null;
  private ctrlKeyHandler: ((e: KeyboardEvent) => void) | null = null;

  constructor(private context: LinkHandlerContext) {}

  /**
   * Attach event listeners. Call after terminalRoot is ready.
   */
  attach(terminalRoot: HTMLElement): void {
    terminalRoot.addEventListener('mousemove', (e) => this.handleHover(e));

    terminalRoot.addEventListener('mouseleave', () => {
      this.context.getRenderer()?.setHoverPosition(-1, -1);
    });

    this.ctrlKeyHandler = (e: KeyboardEvent) => {
      if (e.key === 'Control' || e.key === 'Meta') {
        this.updateHoverCursor();
      }
    };
    window.addEventListener('keydown', this.ctrlKeyHandler);
    window.addEventListener('keyup', this.ctrlKeyHandler);
  }

  /**
   * Handle Ctrl+click to open URLs or file paths.
   */
  handleUrlClick(e: MouseEvent): void {
    const state = this.context.getState();
    if (!state) return;

    const cachedSettings = SettingsService.getCached();
    const terminalRoot = this.context.getTerminalRoot();
    const charSize = this.context.getCharSize();

    // Calculate grid position from click coordinates
    const rect = terminalRoot?.getBoundingClientRect();
    if (!rect) return;

    const col = Math.floor((e.clientX - rect.left) / charSize.width);
    const row = Math.floor((e.clientY - rect.top) / charSize.height);

    // Get the text content by joining soft-wrapped lines into a logical line
    const buffer = state.getActiveBuffer();
    if (row < 0 || row >= state.rows) return;

    const logical = getLogicalLine((r) => buffer.getLine(r), row, state.rows);
    const logicalCol = physicalToLogicalCol(row, col, logical);

    // Check URL first (existing behavior)
    if (!cachedSettings || cachedSettings.url_detection) {
      const url = findUrlAtPosition(logical.text, logicalCol);
      if (url) {
        e.preventDefault();
        import("@tauri-apps/plugin-shell").then(({ open }) => {
          open(url).catch(console.error);
        }).catch(console.error);
        return;
      }
    }

    // Check file path (new behavior)
    if (!cachedSettings || cachedSettings.file_path_detection) {
      const match = findFilePathAtPosition(logical.text, logicalCol);
      if (match) {
        e.preventDefault();
        this.openFileInEditor(match.path, match.line, match.col);
      }
    }
  }

  /**
   * Resolve a file path and open it in the configured editor.
   */
  private async openFileInEditor(filePath: string, line: number, col: number): Promise<void> {
    const state = this.context.getState();
    const cachedSettings = SettingsService.getCached();
    const editorCommand = cachedSettings?.editor_command ?? "";
    if (!editorCommand.trim()) return;

    // Resolve relative paths using shell's CWD (from OSC 7)
    let resolvedPath = filePath;
    if (!filePath.startsWith("/")) {
      const cwd = state?.workingDirectory ?? "";
      if (cwd) {
        // Parse file:// URL properly to handle hostname and percent-encoding
        let cleanCwd: string;
        if (cwd.startsWith("file://")) {
          try {
            cleanCwd = decodeURIComponent(new URL(cwd).pathname);
          } catch {
            cleanCwd = cwd.replace(/^file:\/\//, "");
          }
        } else {
          cleanCwd = cwd;
        }
        resolvedPath = `${cleanCwd}/${filePath}`;
      }
    }

    try {
      const { invoke } = await import("@tauri-apps/api/core");

      // Check file existence
      const exists = await invoke<boolean>("check_file_exists", { path: resolvedPath });
      if (!exists) {
        const { sendNotification, isPermissionGranted } = await import("@tauri-apps/plugin-notification");
        const permitted = await isPermissionGranted();
        if (permitted) {
          sendNotification({ title: "eMterm", body: `File not found: ${resolvedPath}` });
        } else {
          console.warn(`File not found: ${resolvedPath}`);
        }
        return;
      }

      // Split template into tokens BEFORE expanding placeholders,
      // so that spaces in file paths don't break argument boundary.
      const tokens = editorCommand.split(/\s+/).filter(Boolean);
      if (tokens.length === 0) return;

      const program = tokens[0]!;
      const args = tokens.slice(1).map(token =>
        token
          .replace(/\{file\}/g, resolvedPath)
          .replace(/\{line\}/g, String(line))
          .replace(/\{col\}/g, String(col)),
      );

      await invoke("open_file_in_editor", { program, args });
    } catch (error) {
      console.error("Failed to open file in editor:", error);
    }
  }

  /**
   * Handle mousemove for hover cursor feedback (folds, URLs, file paths).
   */
  private handleHover(e: MouseEvent): void {
    this.lastMouseEvent = e;
    this.updateHoverCursor();

    // Pass hover position to renderer for link underline drawing
    const renderer = this.context.getRenderer();
    const terminalRoot = this.context.getTerminalRoot();
    if (renderer && terminalRoot) {
      const rect = terminalRoot.getBoundingClientRect();
      const charSize = this.context.getCharSize();
      const row = Math.floor((e.clientY - rect.top) / charSize.height);
      const col = Math.floor((e.clientX - rect.left) / charSize.width);
      renderer.setHoverPosition(row, col);
    }
  }

  /**
   * Update hover cursor based on current mouse position and modifier keys.
   */
  private updateHoverCursor(): void {
    const e = this.lastMouseEvent;
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    const terminalRoot = this.context.getTerminalRoot();
    if (!e || !state || !renderer || !terminalRoot) return;

    const charSize = this.context.getCharSize();
    const rect = terminalRoot.getBoundingClientRect();
    const displayRow = Math.floor((e.clientY - rect.top) / charSize.height);
    if (displayRow < 0 || displayRow >= state.rows) {
      terminalRoot.style.cursor = "";
      return;
    }

    // Fold hover detection
    const cachedSettings = SettingsService.getCached();
    const foldEnabled = !cachedSettings || cachedSettings.fold_enabled;
    if (foldEnabled) {
      const foldManager = state.getFoldManager();
      if (foldManager.isEnabled()) {
        const scrollbackLength = state.getScrollbackLength();
        const totalActualLines = scrollbackLength + state.rows;
        const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
        const displayStart = Math.max(0, totalDisplayLines - state.rows - renderer.getScrollOffset());
        const displayLine = displayStart + displayRow;

        const summaryRegion = foldManager.getSummaryRegion(displayLine);
        if (summaryRegion) {
          terminalRoot.style.cursor = "pointer";
          return;
        }

        const actualLine = foldManager.displayLineToActual(displayLine);
        const region = foldManager.getRegionAtLine(actualLine);
        if (region && !region.collapsed) {
          terminalRoot.style.cursor = "pointer";
          return;
        }
      }
    }

    // URL/file path hover detection (Ctrl or Meta held)
    if (e.ctrlKey || e.metaKey) {
      const col = Math.floor((e.clientX - rect.left) / charSize.width);
      const buffer = state.getActiveBuffer();
      if (displayRow >= 0 && displayRow < state.rows) {
        const logical = getLogicalLine((r) => buffer.getLine(r), displayRow, state.rows);
        const logicalCol = physicalToLogicalCol(displayRow, col, logical);

        if ((!cachedSettings || cachedSettings.url_detection) && findUrlAtPosition(logical.text, logicalCol)) {
          terminalRoot.style.cursor = "pointer";
          return;
        }
        if ((!cachedSettings || cachedSettings.file_path_detection) && findFilePathAtPosition(logical.text, logicalCol)) {
          terminalRoot.style.cursor = "pointer";
          return;
        }
      }
    }

    terminalRoot.style.cursor = "";
  }

  /**
   * Clean up event listeners and state.
   */
  dispose(): void {
    if (this.ctrlKeyHandler) {
      window.removeEventListener('keydown', this.ctrlKeyHandler);
      window.removeEventListener('keyup', this.ctrlKeyHandler);
      this.ctrlKeyHandler = null;
    }
    this.lastMouseEvent = null;
  }
}
