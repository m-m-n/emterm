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
import type { RendererSettings } from "../settings/settings-applier";
import { SettingsService } from "../settings/settings-service";
import { findUrlAtPosition, findFilePathAtPosition } from "../terminal/url-detector";
import type { DecodedImage, ImageEvent } from "../image/types";
import { SearchStateManager } from "../terminal/search/search-state";
import { SearchBar } from "../terminal/search/search-bar";

/**
 * Main terminal application class that orchestrates the terminal UI and event handling
 */
export class TerminalApp {
  private container: HTMLElement;
  private terminalRoot: HTMLElement | null = null;
  private overlayRoot: HTMLElement | null = null;
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
  private titleChangeCallback: ((title: string) => void) | null = null;
  private bellActivityCallback: (() => void) | null = null;
  private outputActivityCallback: (() => void) | null = null;
  private searchStateManager: SearchStateManager = new SearchStateManager();
  private searchBar: SearchBar | null = null;

  /**
   * Creates a new TerminalApp instance
   * @param container - HTML element to render the terminal into (tab-content)
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
   * Creates the container structure for terminal and overlay separation.
   * This allows viewers (ImageViewer, FullscreenMarkdownView) to render
   * within the tab content area without affecting terminal content.
   *
   * Structure:
   * - container (tab-content)
   *   - terminal-root: Terminal renderer target (canvas, etc.)
   *   - overlay-root: Overlay container (ImageViewer, MarkdownView, dialogs)
   */
  private createContainerStructure(): void {
    // Create terminal-root for terminal renderer
    this.terminalRoot = document.createElement("div");
    this.terminalRoot.className = "terminal-root";
    this.container.appendChild(this.terminalRoot);

    // Create overlay-root for viewers and dialogs
    this.overlayRoot = document.createElement("div");
    this.overlayRoot.className = "overlay-root";
    this.container.appendChild(this.overlayRoot);
  }

  /**
   * Initializes the terminal application
   */
  async init(): Promise<void> {
    // Create container structure for terminal/overlay separation
    this.createContainerStructure();

    // Use terminalRoot for terminal-related operations
    const terminalContainer = this.terminalRoot!;

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
    this.renderer = await createRendererAsync(terminalContainer, fontFamily, fontSize);

    // Apply cached settings to the newly created renderer
    // (applySettings runs before tabManager exists, so renderer notifications are dropped)
    const cachedSettings = SettingsService.getCached();
    if (cachedSettings) {
      if (cachedSettings.terminal_color_scheme) {
        // Check if it's a user-defined color scheme
        const userScheme = cachedSettings.custom_color_schemes?.find(
          (s) => s.name === cachedSettings.terminal_color_scheme
        );
        if (userScheme) {
          // Apply user-defined color scheme directly
          this.renderer.setUserColorScheme(userScheme);
        } else {
          // Apply preset color scheme
          this.renderer.applySetting("colorScheme", cachedSettings.terminal_color_scheme);
        }
      }
      if (cachedSettings.cursor_style) {
        this.renderer.applySetting("cursorStyle", cachedSettings.cursor_style);
      }
      if (cachedSettings.cursor_blink !== undefined) {
        this.renderer.applySetting("cursorBlink", cachedSettings.cursor_blink);
      }
      if (cachedSettings.fold_enabled !== undefined) {
        this.state.getFoldManager().setEnabled(cachedSettings.fold_enabled);
      }
    }

    // Register bell callback
    this.state.onBell = () => this.handleBell();

    // Initialize selection controller (new v2 system)
    this.selectionController = new SelectionController({
      container: terminalContainer,
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
      container: terminalContainer,
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
      onToggleSearch: () => this.toggleSearch(),
      onRestoreFocus: () => this.imeHandler?.focus(),
    };
    this.keyboardHandler = new KeyboardHandler(keyboardContext);
    // Attach to document but check if this tab's container is visible
    // This allows keyboard input to work even when focus is elsewhere in the window
    this.keyboardHandler.attach(document);

    // Initialize search bar
    this.searchBar = new SearchBar(this.terminalRoot!, {
      onSearch: (query, options) => this.handleSearch(query, options),
      onNextMatch: () => this.handleSearchNext(),
      onPrevMatch: () => this.handleSearchPrev(),
      onClose: () => this.handleSearchClose(),
    });

    // Initialize mouse handler (for PTY mouse tracking only - selection handled by SelectionController)
    this.mouseHandler = new MouseHandler(
      terminalContainer,
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

    // Add mouse wheel handler for scrollback
    terminalContainer.addEventListener('wheel', (e) => this.handleWheel(e));

    // Add click handler for fold toggle (plain click) and URL opening (Ctrl+click)
    terminalContainer.addEventListener('click', (e) => {
      if (e.ctrlKey || e.metaKey) {
        this.handleUrlClick(e);
      } else {
        this.handleFoldClick(e);
      }
    });

    // Add mousemove handler for fold cursor feedback
    terminalContainer.addEventListener('mousemove', (e) => this.handleFoldHover(e));

    // Initialize ImageViewer with overlay-root container
    this.imageViewer = new ImageViewer(this.overlayRoot!);
    this.imageViewer.onHide(() => {
      // Force re-render after image viewer closes (e.g. via Escape key)
      if (this.state && this.renderer) {
        this.renderer.forceRender(this.state);
      }
    });

    // Set markdown session manager's container for fullscreen view
    this.state.getMarkdownManager().setContainer(this.overlayRoot!);

    // Set up image event listener
    await this.setupImageEventListener();

    // Make terminal focusable and set up resize observer before PTY spawn
    terminalContainer.tabIndex = 0;
    this.setupResizeObserver();

    // Initial render to show empty terminal immediately
    this.renderer.forceRender(this.state);

    // Focus terminal UI early for better UX
    this.imeHandler.focus();

    // Spawn PTY session (non-blocking UI)
    try {
      // Read shell settings from cached settings (reuse from above)
      const shell = cachedSettings?.shell_path || undefined;
      const args = cachedSettings?.shell_args?.length ? cachedSettings.shell_args : undefined;

      await this.ptyClient.spawn({ shell, args, cols, rows });

      // Flush any terminal actions that arrived before spawn returned
      if (this.state && this.renderer) {
        this.ptyClient.flushPendingTerminalActions();
        this.renderer.forceRender(this.state);
      }
    } catch (error) {
      console.error("Failed to spawn PTY:", error);
      terminalContainer.textContent = `Failed to start terminal: ${error}`;
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
          // Notify tab title change
          if (this.titleChangeCallback) {
            this.titleChangeCallback(newTitle || "Terminal");
          }
        }

        // Notify activity tracker of output
        this.outputActivityCallback?.();

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
          // Force re-render after closing image viewer to show correct state
          if (this.state && this.renderer) {
            this.renderer.forceRender(this.state);
          }
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
          try {
            this.state.resize(newCols, newRows);
            this.renderer.resize(newCols, newRows);
            this.renderer.forceRender(this.state);
          } catch (error) {
            console.error("Failed to resize terminal:", error);
            // Attempt recovery: force re-render with current state
            try {
              this.renderer.forceRender(this.state);
            } catch {
              // Rendering failed too - nothing we can do
            }
          }
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
   * Handle mouse wheel events for scrollback
   */
  private handleWheel(e: WheelEvent): void {
    e.preventDefault();

    if (!this.renderer || !this.state) return;

    // Get scroll speed multiplier from settings (default: 3)
    const cachedSettings = SettingsService.getCached();
    const scrollSpeed = cachedSettings?.scroll_speed ?? 3;

    // Calculate number of lines to scroll based on wheel delta and speed
    const lines = Math.ceil(Math.abs(e.deltaY) / this.charSize.height * scrollSpeed);

    if (e.deltaY < 0) {
      // Scroll up (toward past)
      this.renderer.scrollUp(lines);
    } else {
      // Scroll down (toward present)
      this.renderer.scrollDown(lines);
    }

    // Force re-render with new scroll offset
    this.renderer.forceRender(this.state);
  }

  /**
   * Handle BEL character based on bell_action setting
   */
  private handleBell(): void {
    const cachedSettings = SettingsService.getCached();
    const bellAction = cachedSettings?.bell_action ?? "visual";

    switch (bellAction) {
      case "visual": {
        const container = this.terminalRoot;
        if (container) {
          container.classList.add("terminal-bell-flash");
          setTimeout(() => container.classList.remove("terminal-bell-flash"), 150);
        }
        break;
      }
      case "sound": {
        try {
          const ctx = new AudioContext();
          const oscillator = ctx.createOscillator();
          const gain = ctx.createGain();
          oscillator.connect(gain);
          gain.connect(ctx.destination);
          oscillator.frequency.value = 800;
          gain.gain.value = 0.1;
          oscillator.start();
          oscillator.stop(ctx.currentTime + 0.1);
        } catch {
          // Audio not available
        }
        break;
      }
      case "none":
        break;
    }

    // Notify activity tracker
    this.bellActivityCallback?.();
  }

  /**
   * Handle Ctrl+click to open URLs or file paths
   */
  private handleUrlClick(e: MouseEvent): void {
    if (!this.state) return;

    const cachedSettings = SettingsService.getCached();

    // Calculate grid position from click coordinates
    const rect = this.terminalRoot?.getBoundingClientRect();
    if (!rect) return;

    const col = Math.floor((e.clientX - rect.left) / this.charSize.width);
    const row = Math.floor((e.clientY - rect.top) / this.charSize.height);

    // Get the text content of the clicked row
    const buffer = this.state.getActiveBuffer();
    if (row < 0 || row >= this.state.rows) return;

    const line = buffer.getLine(row);
    if (!line) return;

    // Build text string from line cells
    let text = "";
    for (let c = 0; c < line.length; c++) {
      text += line.getCell(c).char || " ";
    }

    // Check URL first (existing behavior)
    if (!cachedSettings || cachedSettings.url_detection) {
      const url = findUrlAtPosition(text, col);
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
      const match = findFilePathAtPosition(text, col);
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
    const cachedSettings = SettingsService.getCached();
    const editorCommand = cachedSettings?.editor_command ?? "";
    if (!editorCommand.trim()) return;

    // Resolve relative paths using shell's CWD (from OSC 7)
    let resolvedPath = filePath;
    if (!filePath.startsWith("/")) {
      const cwd = this.state?.workingDirectory ?? "";
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
      // so that spaces in file paths don't break argument boundaries.
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
   * Handle click on fold region to toggle fold/unfold.
   * Only triggers on plain left-click (no modifiers, no text selection).
   */
  private handleFoldClick(e: MouseEvent): void {
    if (!this.state || !this.renderer) return;

    const cachedSettings = SettingsService.getCached();
    if (cachedSettings && !cachedSettings.fold_enabled) return;

    const foldManager = this.state.getFoldManager();
    if (!foldManager.isEnabled()) return;
    if (foldManager.getCollapsedRegions().length === 0 && !this.hasFoldableRegions()) return;

    // Don't toggle if user is selecting text
    const selection = window.getSelection();
    if (selection && selection.toString().length > 0) return;

    // Calculate display row from click coordinates
    const rect = this.terminalRoot?.getBoundingClientRect();
    if (!rect) return;

    const displayRow = Math.floor((e.clientY - rect.top) / this.charSize.height);
    if (displayRow < 0 || displayRow >= this.state.rows) return;

    // Calculate actual display line index
    const scrollbackLength = this.state.getScrollbackLength();
    const totalActualLines = scrollbackLength + this.state.rows;
    const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
    const displayStart = Math.max(0, totalDisplayLines - this.state.rows - this.renderer.getScrollOffset());
    const displayLine = displayStart + displayRow;

    // Check if clicking on a summary line (expand)
    const summaryRegion = foldManager.getSummaryRegion(displayLine);
    if (summaryRegion) {
      foldManager.expandRegionContaining(summaryRegion.startLine);
      this.renderer.forceRender(this.state);
      return;
    }

    // Check if clicking on a foldable region (collapse)
    const actualLine = foldManager.displayLineToActual(displayLine);
    const region = foldManager.getRegionAtLine(actualLine);
    if (region && !region.collapsed) {
      // Calculate scroll adjustment: if fold is above or at viewport top, adjust scroll
      const regionDisplayLine = foldManager.actualLineToDisplay(region.startLine);
      foldManager.toggleFold(actualLine);
      // Adjust scroll if the fold causes viewport shift
      if (regionDisplayLine < displayStart) {
        const delta = region.lineCount - 1;
        this.renderer.setScrollOffset(Math.max(0, this.renderer.getScrollOffset() - delta));
      }
      this.renderer.forceRender(this.state);
    }
  }

  /**
   * Check if there are any foldable regions (even if not collapsed).
   */
  private hasFoldableRegions(): boolean {
    if (!this.state) return false;
    const foldManager = this.state.getFoldManager();
    // Quick check: if there are any regions registered
    return foldManager.getRegionAtLine(0) !== null ||
      foldManager.getCollapsedRegions().length > 0;
  }

  /**
   * Handle mousemove for fold cursor feedback.
   */
  private handleFoldHover(e: MouseEvent): void {
    if (!this.state || !this.renderer || !this.terminalRoot) return;

    const cachedSettings = SettingsService.getCached();
    if (cachedSettings && !cachedSettings.fold_enabled) return;

    const foldManager = this.state.getFoldManager();
    if (!foldManager.isEnabled()) return;

    const rect = this.terminalRoot.getBoundingClientRect();
    const displayRow = Math.floor((e.clientY - rect.top) / this.charSize.height);
    if (displayRow < 0 || displayRow >= this.state.rows) {
      this.terminalRoot.style.cursor = "";
      return;
    }

    const scrollbackLength = this.state.getScrollbackLength();
    const totalActualLines = scrollbackLength + this.state.rows;
    const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
    const displayStart = Math.max(0, totalDisplayLines - this.state.rows - this.renderer.getScrollOffset());
    const displayLine = displayStart + displayRow;

    // Check if hovering over a summary line or foldable region
    const summaryRegion = foldManager.getSummaryRegion(displayLine);
    if (summaryRegion) {
      this.terminalRoot.style.cursor = "pointer";
      return;
    }

    const actualLine = foldManager.displayLineToActual(displayLine);
    const region = foldManager.getRegionAtLine(actualLine);
    if (region && !region.collapsed) {
      this.terminalRoot.style.cursor = "pointer";
      return;
    }

    this.terminalRoot.style.cursor = "";
  }

  /**
   * Toggle the search bar open/closed.
   */
  toggleSearch(): void {
    if (!this.searchBar) return;

    if (this.searchBar.isVisible()) {
      this.handleSearchClose();
    } else {
      this.searchBar.show();
    }
  }

  /**
   * Handle search query/options change from search bar.
   */
  private handleSearch(query: string, options: { isRegex: boolean; caseSensitive: boolean }): void {
    if (!this.state || !this.renderer) return;

    this.searchStateManager.setQuery(query);
    this.searchStateManager.setOptions(options);

    // Collect all line texts (scrollback + screen)
    const lines = this.getAllLineTexts();
    this.searchStateManager.executeSearch(lines);

    // Update search bar UI
    this.searchBar?.updateCount(
      this.searchStateManager.currentMatchIndex,
      this.searchStateManager.matches.length,
    );
    this.searchBar?.setError(this.searchStateManager.error !== null);

    // Update highlight rendering
    this.renderer.setSearchHighlights(
      this.searchStateManager.matches,
      this.searchStateManager.currentMatchIndex,
    );
    this.renderer.forceRender(this.state);

    // Scroll to first match if found
    if (this.searchStateManager.matches.length > 0) {
      this.scrollToCurrentMatch();
    }
  }

  /**
   * Handle next match navigation.
   */
  private handleSearchNext(): void {
    if (!this.state || !this.renderer) return;

    const match = this.searchStateManager.nextMatch();
    if (match) {
      this.renderer.setSearchHighlights(
        this.searchStateManager.matches,
        this.searchStateManager.currentMatchIndex,
      );
      this.searchBar?.updateCount(
        this.searchStateManager.currentMatchIndex,
        this.searchStateManager.matches.length,
      );
      this.scrollToCurrentMatch();
      this.renderer.forceRender(this.state);
    }
  }

  /**
   * Handle previous match navigation.
   */
  private handleSearchPrev(): void {
    if (!this.state || !this.renderer) return;

    const match = this.searchStateManager.prevMatch();
    if (match) {
      this.renderer.setSearchHighlights(
        this.searchStateManager.matches,
        this.searchStateManager.currentMatchIndex,
      );
      this.searchBar?.updateCount(
        this.searchStateManager.currentMatchIndex,
        this.searchStateManager.matches.length,
      );
      this.scrollToCurrentMatch();
      this.renderer.forceRender(this.state);
    }
  }

  /**
   * Handle search bar close.
   */
  private handleSearchClose(): void {
    this.searchBar?.hide();
    this.searchStateManager.clear();
    this.renderer?.clearSearchHighlights();
    if (this.state && this.renderer) {
      this.renderer.forceRender(this.state);
    }
    // Return focus to terminal
    this.imeHandler?.focus();
  }

  /**
   * Scroll to make the current search match visible.
   */
  private scrollToCurrentMatch(): void {
    if (!this.state || !this.renderer) return;

    const match = this.searchStateManager.getCurrentMatch();
    if (!match) return;

    // Auto-expand fold region if match is inside a collapsed region
    const foldManager = this.state.getFoldManager();
    foldManager.expandRegionContaining(match.lineIndex);

    const scrollbackLength = this.state.getScrollbackLength();
    const currentScrollOffset = this.renderer.getScrollOffset();
    const visibleStartLine = scrollbackLength - currentScrollOffset;
    const visibleEndLine = visibleStartLine + this.state.rows;

    // Check if match is visible
    if (match.lineIndex >= visibleStartLine && match.lineIndex < visibleEndLine) {
      return; // Already visible
    }

    // Scroll so the match is roughly centered in view
    const targetOffset = Math.max(0, scrollbackLength - match.lineIndex + Math.floor(this.state.rows / 2));
    this.renderer.setScrollOffset(targetOffset);
  }

  /**
   * Get all line texts (scrollback + screen buffer) for search.
   */
  private getAllLineTexts(): string[] {
    if (!this.state) return [];

    const lines: string[] = [];
    const scrollback = this.state.getScrollbackBuffer();
    const buffer = this.state.getActiveBuffer();

    // Scrollback lines
    for (const line of scrollback) {
      const chars: string[] = [];
      for (let c = 0; c < line.length; c++) {
        chars.push(line.getCell(c).char || " ");
      }
      lines.push(chars.join(""));
    }

    // Screen buffer lines
    for (let row = 0; row < this.state.rows; row++) {
      const line = buffer.getLine(row);
      const chars: string[] = [];
      for (let c = 0; c < line.length; c++) {
        chars.push(line.getCell(c).char || " ");
      }
      lines.push(chars.join(""));
    }

    return lines;
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
    this.searchBar?.dispose();
    this.searchBar = null;

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
    this.sessionExitCallback = null;
    this.titleChangeCallback = null;
    this.bellActivityCallback = null;
    this.outputActivityCallback = null;

    // Remove container structure elements
    if (this.terminalRoot) {
      this.terminalRoot.remove();
      this.terminalRoot = null;
    }
    if (this.overlayRoot) {
      this.overlayRoot.remove();
      this.overlayRoot = null;
    }
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

  /**
   * Sets callback for when terminal title changes via OSC sequences
   * Used by TabManager to update the tab title
   */
  onTitleChange(callback: (title: string) => void): void {
    this.titleChangeCallback = callback;
  }

  /**
   * Sets callback for when BEL character is received.
   * Used by TabActivityTracker for activity monitoring.
   */
  onBellActivity(callback: () => void): void {
    this.bellActivityCallback = callback;
  }

  /**
   * Sets callback for when terminal output is received.
   * Used by TabActivityTracker for activity monitoring.
   */
  onOutputActivity(callback: () => void): void {
    this.outputActivityCallback = callback;
  }

  /**
   * Update the font size of the terminal.
   * @param fontSize - New font size in points
   */
  setFontSize(fontSize: number): void {
    this.renderer?.setFontSize(fontSize);
  }

  /**
   * Apply a setting change to the terminal.
   * @param setting - The setting key
   * @param value - The new value
   */
  applySetting<K extends keyof RendererSettings>(
    setting: K,
    value: RendererSettings[K],
  ): void {
    if (setting === "foldEnabled") {
      this.state?.getFoldManager().setEnabled(value as boolean);
      if (this.state && this.renderer) {
        this.renderer.forceRender(this.state);
      }
      return;
    }
    this.renderer?.applySetting(setting, value);
  }
}

// Re-export types
export * from "./types";
export * from "./config";
