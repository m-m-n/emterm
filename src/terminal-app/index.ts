/**
 * Terminal application main class
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { measureCharacterSize, observeContainerResize, PtyClient } from "../pty";
import { TerminalState } from "../terminal/state";
import { TerminalRenderer } from "../terminal/renderer";
import { SelectionController } from "../selection-v2";
import type { TerminalAppOptions, CharSize } from "./types";
import { KeyboardHandler, MouseHandler, ImeHandler } from "./handlers";
import type { KeyboardHandlerContext } from "./handlers/keyboard";
import type { TerminalActionsPayload } from "../types/terminal";

/**
 * Main terminal application class that orchestrates the terminal UI and event handling
 */
export class TerminalApp {
  private container: HTMLElement;
  private options: TerminalAppOptions;
  private ptyClient: PtyClient | null = null;
  private keyboardHandler: KeyboardHandler | null = null;
  private mouseHandler: MouseHandler | null = null;
  private imeHandler: ImeHandler | null = null;
  private state: TerminalState | null = null;
  private renderer: TerminalRenderer | null = null;
  private selectionController: SelectionController | null = null;
  private charSize: CharSize = { width: 8, height: 16 };
  private disconnectResizeObserver: (() => void) | null = null;
  private lastWindowTitle = "";

  /**
   * Creates a new TerminalApp instance
   * @param container - HTML element to render the terminal into
   * @param options - Optional terminal configuration
   */
  constructor(container: HTMLElement, options: TerminalAppOptions = {}) {
    this.container = container;
    this.options = {
      useNewTerminal: true,
      ...options,
    };
  }

  /**
   * Initializes the terminal application
   */
  async init(): Promise<void> {
    // Measure character size from container's computed styles
    this.charSize = measureCharacterSize(this.container);

    // Calculate initial terminal size
    const cols = Math.max(
      1,
      Math.floor(this.container.clientWidth / this.charSize.width)
    );
    const rows = Math.max(
      1,
      Math.floor(this.container.clientHeight / this.charSize.height)
    );

    // Get font configuration from computed styles
    const computedStyle = window.getComputedStyle(this.container);
    const fontFamily = computedStyle.fontFamily || "monospace";
    const fontSize = parseFloat(computedStyle.fontSize) || 14;

    // Initialize terminal state and renderer
    this.state = new TerminalState(cols, rows);
    this.renderer = new TerminalRenderer(this.container, fontFamily, fontSize);

    // Initialize selection controller (new v2 system)
    this.selectionController = new SelectionController({
      container: this.container,
      charWidth: this.charSize.width,
      charHeight: this.charSize.height,
      cols,
      rows,
      getTerminalState: () => this.state!,
    });

    // Create PTY client
    this.ptyClient = new PtyClient();

    // Set up PTY output handler
    await this.setupPtyHandlers();

    // Initialize IME handler
    this.imeHandler = new ImeHandler({
      container: this.container,
      ptyClient: this.ptyClient,
      getState: () => this.state!,
      charSize: this.charSize,
    });
    this.imeHandler.init();

    // Initialize keyboard handler
    const keyboardContext: KeyboardHandlerContext = {
      ptyClient: this.ptyClient,
      getState: () => this.state!,
      getRenderer: () => this.renderer,
      selectionController: this.selectionController,
      isEditContextActive: () => this.imeHandler?.isEditContextActive() ?? false,
      isImeInputFocused: () => this.imeHandler?.isImeInputFocused() ?? false,
    };
    this.keyboardHandler = new KeyboardHandler(keyboardContext);
    this.keyboardHandler.attach(document);

    // Initialize mouse handler (for PTY mouse tracking only - selection handled by SelectionController)
    this.mouseHandler = new MouseHandler(
      this.container,
      this.ptyClient,
      () => this.state!,
      this.charSize,
      {
        // No selection callbacks - SelectionController handles selection
      }
    );
    this.mouseHandler.attach();

    // Attach selection controller
    this.selectionController.attach();

    // Spawn PTY session
    try {
      await this.ptyClient.spawn({ cols, rows });

      // Flush any terminal actions that arrived before spawn returned
      if (this.state && this.renderer) {
        this.ptyClient.flushPendingTerminalActions();
        this.renderer.forceRender(this.state);
      }
    } catch (error) {
      console.error("Failed to spawn PTY:", error);
      this.container.textContent = `Failed to start terminal: ${error}`;
      return;
    }

    // Set up resize observer
    this.setupResizeObserver();

    // Make terminal focusable
    this.container.tabIndex = 0;

    // Initial focus
    this.imeHandler.focus();
  }

  /**
   * Sets up PTY output handlers
   */
  private async setupPtyHandlers(): Promise<void> {
    if (!this.ptyClient) return;

    // Listen for terminal_actions events (from Phase 1 ANSI parser)
    await this.ptyClient.onTerminalActions(
      async (payload: TerminalActionsPayload) => {
        if (!this.state || !this.renderer || !this.ptyClient) return;

        // Process each action
        for (const action of payload.actions) {
          this.state.processAction(action);
        }

        // Handle DSR responses - write back to PTY
        const response = this.state.takePendingResponse();
        if (response) {
          try {
            await this.ptyClient.write(response);
          } catch (error) {
            console.error("Failed to write DSR response:", error);
          }
        }

        // Handle window title changes
        const newTitle = this.state.title;
        if (newTitle !== this.lastWindowTitle) {
          this.lastWindowTitle = newTitle;
          try {
            const appWindow = getCurrentWebviewWindow();
            await appWindow.setTitle(newTitle || "eMterm");
          } catch (error) {
            console.error("Failed to set window title:", error);
          }
        }

        // Schedule render
        this.renderer.scheduleRender(this.state);

        // Update IME position after terminal state changes
        this.imeHandler?.updatePosition();
      }
    );

    // Handle exit event
    await this.ptyClient.onExit(async (code, remainingSessions) => {
      if (remainingSessions === 0) {
        try {
          const appWindow = getCurrentWebviewWindow();
          await appWindow.close();
        } catch (error) {
          console.error("Failed to close window:", error);
        }
      }
    });
  }

  /**
   * Sets up resize observer for the container
   */
  private setupResizeObserver(): void {
    this.disconnectResizeObserver = observeContainerResize(
      this.container,
      this.charSize.width,
      this.charSize.height,
      async (newCols, newRows) => {
        if (this.ptyClient) {
          try {
            await this.ptyClient.resize(newCols, newRows);

            if (this.state && this.renderer) {
              this.state.resize(newCols, newRows);
              this.renderer.resize(newCols, newRows);
              this.renderer.forceRender(this.state);
              this.imeHandler?.updatePosition();
              this.mouseHandler?.updateCharSize(
                this.charSize.width,
                this.charSize.height
              );

              // Update selection controller dimensions (clears selection)
              this.selectionController?.resize(
                newCols,
                newRows,
                this.charSize.width,
                this.charSize.height
              );
            }
          } catch (error) {
            console.error("Failed to resize PTY:", error);
          }
        }
      }
    );
  }

  /**
   * Resizes the terminal to the specified dimensions
   * @param cols - Number of columns
   * @param rows - Number of rows
   */
  resize(cols: number, rows: number): void {
    if (this.state && this.renderer) {
      this.state.resize(cols, rows);
      this.renderer.resize(cols, rows);
      this.renderer.forceRender(this.state);
    }
  }

  /**
   * Cleans up resources and event listeners
   */
  dispose(): void {
    // Disconnect resize observer
    if (this.disconnectResizeObserver) {
      this.disconnectResizeObserver();
      this.disconnectResizeObserver = null;
    }

    // Clean up handlers
    this.keyboardHandler?.detach();
    this.mouseHandler?.detach();
    this.imeHandler?.dispose();
    this.selectionController?.dispose();

    // Clean up PTY
    if (this.ptyClient) {
      this.ptyClient.dispose();
      this.ptyClient.kill().catch(console.error);
      this.ptyClient = null;
    }

    // Clear references
    this.state = null;
    this.renderer = null;
    this.selectionController = null;
  }

  /**
   * Gets the PTY client
   * @returns PTY client instance or null if not initialized
   */
  get pty(): PtyClient | null {
    return this.ptyClient;
  }

  /**
   * Gets the current terminal state
   * @returns Current terminal state
   */
  get terminalState(): TerminalState {
    if (!this.state) {
      throw new Error("Terminal not initialized");
    }
    return this.state;
  }

  /**
   * Gets the terminal renderer
   * @returns Terminal renderer instance
   */
  get terminalRenderer(): TerminalRenderer {
    if (!this.renderer) {
      throw new Error("Terminal not initialized");
    }
    return this.renderer;
  }
}

// Re-export types
export * from "./types";
export * from "./config";
