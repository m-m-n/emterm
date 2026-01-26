/**
 * Terminal application main class
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  measureCharacterSize,
  observeContainerResize,
  PtyClient,
} from "../pty";
import { TerminalState } from "../terminal/state";
import { createRendererAsync, type ITerminalRenderer } from "../terminal";
import { SelectionController } from "../selection-v2";
import { ImageViewer } from "../image-viewer";
import type { TerminalAppOptions, CharSize } from "./types";
import { KeyboardHandler, MouseHandler, ImeHandler } from "./handlers";
import type { KeyboardHandlerContext } from "./handlers/keyboard";
import type {
  TerminalActionsPayload,
  ImageEventPayload,
} from "../types/terminal";
import type { DecodedImage, ImageEvent } from "../image/types";

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
  private renderer: ITerminalRenderer | null = null;
  private selectionController: SelectionController | null = null;
  private imageViewer: ImageViewer | null = null;
  private imageEventUnlisten: UnlistenFn | null = null;
  private pendingImages: Map<number, DecodedImage> = new Map();
  private charSize: CharSize = { width: 8, height: 16 };
  private disconnectResizeObserver: (() => void) | null = null;
  private lastWindowTitle = "";
  private sessionExitCallback: ((sessionId: string) => void) | null = null;

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
      Math.floor(this.container.clientWidth / this.charSize.width),
    );
    const rows = Math.max(
      1,
      Math.floor(this.container.clientHeight / this.charSize.height),
    );

    // Get font configuration from computed styles
    const computedStyle = window.getComputedStyle(this.container);
    const fontFamily = computedStyle.fontFamily || "monospace";
    const fontSize = parseFloat(computedStyle.fontSize) || 14;

    // Initialize terminal state and renderer
    this.state = new TerminalState(cols, rows);
    this.renderer = await createRendererAsync(this.container, fontFamily, fontSize);

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
    // Use container id or generate unique id for debugging
    const imeDebugId = this.container.id || `ime-${Date.now()}`;
    this.imeHandler = new ImeHandler({
      container: this.container,
      ptyClient: this.ptyClient,
      getState: () => this.state!,
      charSize: this.charSize,
      // Check if this tab's container is visible (for multi-tab support)
      isActiveTab: () => this.container.style.display !== "none",
      debugId: imeDebugId,
    });
    this.imeHandler.init();

    // Initialize keyboard handler
    const keyboardContext: KeyboardHandlerContext = {
      ptyClient: this.ptyClient,
      getState: () => this.state!,
      getRenderer: () => this.renderer,
      selectionController: this.selectionController,
      isEditContextActive: () =>
        this.imeHandler?.isEditContextActive() ?? false,
      isImeInputFocused: () => this.imeHandler?.isImeInputFocused() ?? false,
      // Check if this tab's container is visible (for multi-tab support)
      isActiveTab: () => this.container.style.display !== "none",
    };
    this.keyboardHandler = new KeyboardHandler(keyboardContext);
    // Attach to document but check if this tab's container is visible
    // This allows keyboard input to work even when focus is elsewhere in the window
    this.keyboardHandler.attach(document);

    // Initialize mouse handler (for PTY mouse tracking only - selection handled by SelectionController)
    this.mouseHandler = new MouseHandler(
      this.container,
      this.ptyClient,
      () => this.state!,
      this.charSize,
      {
        // No selection callbacks - SelectionController handles selection
      },
    );
    this.mouseHandler.attach();

    // Attach selection controller
    this.selectionController.attach();

    // Initialize ImageViewer
    this.imageViewer = new ImageViewer(this.container);

    // Set up image event listener
    await this.setupImageEventListener();

    // Make terminal focusable and set up resize observer before PTY spawn
    this.container.tabIndex = 0;
    this.setupResizeObserver();

    // Initial render to show empty terminal immediately
    this.renderer.forceRender(this.state);

    // Focus terminal UI early for better UX
    this.imeHandler.focus();

    // Spawn PTY session (non-blocking UI)
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
      },
    );

    // Handle exit event
    await this.ptyClient.onExit(async (_code, _remainingSessions) => {
      // Notify session exit callback (for TabManager integration)
      const sessionId = this.ptyClient?.getSessionId();
      if (sessionId && this.sessionExitCallback) {
        this.sessionExitCallback(sessionId);
      }
      // Note: Window close is now handled by TabManager.onLastTabClosed()
    });
  }

  /**
   * Sets up image event listener for Kitty Graphics and SIXEL support
   */
  private async setupImageEventListener(): Promise<void> {
    this.imageEventUnlisten = await listen<ImageEventPayload>(
      "image_event",
      (event: { payload: ImageEventPayload }) => {
        // Only process events for the current session
        if (
          this.ptyClient &&
          event.payload.session_id === this.ptyClient.getSessionId()
        ) {
          this.handleImageEvent(event.payload);
        }
      },
    );
  }

  /**
   * Handles image events from the backend
   */
  private handleImageEvent(payload: ImageEventPayload): void {
    const eventType = payload.type;

    switch (eventType) {
      case "ImageReady": {
        // Store the decoded image for later display
        const image = payload.image as DecodedImage;
        this.pendingImages.set(image.id, image);
        console.info(
          `[INFO][FRONTEND] Image ready: id=${image.id}, ${image.width}x${image.height}`,
        );
        break;
      }

      case "Place": {
        // Display the image at the specified position
        const placement = payload.placement;
        if (!placement) {
          console.warn("[WARN][FRONTEND] Place event without placement data");
          break;
        }
        const image = this.pendingImages.get(placement.image_id);
        if (image && this.imageViewer) {
          // For fullscreen display mode
          this.imageViewer.show(image);
          console.info(
            `[INFO][FRONTEND] Image placed: id=${placement.image_id}`,
          );
        } else {
          console.warn(
            `[WARN][FRONTEND] Image not found for placement: ${placement.image_id}`,
          );
        }
        break;
      }

      case "Delete": {
        // Handle image deletion
        const target = payload.target;
        if (!target) {
          console.warn("[WARN][FRONTEND] Delete event without target data");
          break;
        }
        if (target.type === "All" || target.type === "AllIncludingHidden") {
          this.pendingImages.clear();
          this.imageViewer?.hide();
        } else if (target.type === "ById" && target.id !== undefined) {
          this.pendingImages.delete(target.id);
        }
        console.info(
          `[INFO][FRONTEND] Image deleted: ${JSON.stringify(target)}`,
        );
        break;
      }

      case "QueryResponse": {
        // Handle query response (graphics protocol supported)
        console.info(
          `[INFO][FRONTEND] Graphics supported: ${payload.supported}`,
        );
        break;
      }

      case "Animation": {
        // Handle animation events
        if (this.imageViewer && payload.data) {
          this.imageViewer.handleAnimationEvent(
            payload.data as import("../image/types").AnimationEvent,
          );
        }
        break;
      }

      case "Response": {
        // Handle protocol response - send back to PTY
        // This is used by Kitty protocol for OK/ERROR responses
        const responseData = payload.data as string | undefined;
        if (responseData && this.ptyClient) {
          // Write response back to PTY
          this.ptyClient.write(responseData);
        }
        break;
      }

      default:
        console.debug(
          `[DEBUG][FRONTEND] Unknown image event type: ${eventType}`,
        );
    }
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
        // Skip resize if container is hidden (tab not active)
        // This prevents buffer content from being lost when a tab becomes hidden
        // and ResizeObserver reports 0x0 dimensions (leading to 1x1 resize)
        if (this.container.style.display === "none") {
          return;
        }

        // Always update local terminal state/renderer (even if PTY not ready)
        if (this.state && this.renderer) {
          this.state.resize(newCols, newRows);
          this.renderer.resize(newCols, newRows);
          this.renderer.forceRender(this.state);
          this.imeHandler?.updatePosition();
          this.mouseHandler?.updateCharSize(
            this.charSize.width,
            this.charSize.height,
          );

          // Update selection controller dimensions (clears selection)
          this.selectionController?.resize(
            newCols,
            newRows,
            this.charSize.width,
            this.charSize.height,
          );
        }

        // Resize PTY if session is active (returns false if not ready)
        if (this.ptyClient) {
          const resized = await this.ptyClient.resize(newCols, newRows);
          if (!resized && import.meta.env?.DEV) {
            console.debug("PTY resize skipped - session not yet started");
          }
        }
      },
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
   * Focus the terminal for keyboard input
   */
  focus(): void {
    this.imeHandler?.focus();
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

    // Clean up image viewer and listener
    if (this.imageEventUnlisten) {
      this.imageEventUnlisten();
      this.imageEventUnlisten = null;
    }
    this.imageViewer?.dispose();
    this.imageViewer = null;
    this.pendingImages.clear();

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
  get terminalRenderer(): ITerminalRenderer {
    if (!this.renderer) {
      throw new Error("Terminal not initialized");
    }
    return this.renderer;
  }

  /**
   * Sets callback for when PTY session exits
   * Used by TabManager to close the tab when shell exits
   */
  onSessionExit(callback: (sessionId: string) => void): void {
    this.sessionExitCallback = callback;
  }
}

// Re-export types
export * from "./types";
export * from "./config";
