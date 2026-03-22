/**
 * Terminal application main class
 */

import {
  calculateTerminalSize,
  measureCharacterSize,
  PtyClient,
} from "../pty";
import { TerminalState } from "../terminal/state";
import { WasmGrid } from "../terminal/wasm/terminal-core";
import { createRenderer, createRendererAsync, type ITerminalRenderer } from "../terminal";
import { SelectionController } from "../selection-v2";
import type { TerminalAppOptions, CharSize } from "./types";
import { KeyboardHandler, MouseHandler, ImeHandler, FoldHandler, SearchHandler, ImageHandler, LinkHandler } from "./handlers";
import type { KeyboardHandlerContext } from "./handlers/keyboard";
import type { RendererSettings } from "../settings/settings-applier";
import { SettingsService } from "../settings/settings-service";
import { buildFontFamilyChain } from "../settings/settings-applier";
import { showTerminalContextMenu } from "../context-menu";
import { FileDropHandler, formatPathsForPaste, extractRemotePath, type FileDropInfo } from "../sftp/file-drop-handler";
import { UploadManager } from "../sftp/upload-manager";
import { DownloadSessionManager } from "../download";
import { MuxClient, MuxMessageType } from "../terminal/mux/mux-client";
import type { MuxAction } from "../terminal/mux/prefix-key";
import {
  CopyModeManager,
  ViKeybinds,
  EmacsKeybinds,
  type CopyModeSelection,
} from "../terminal/mux-copy-mode";
import {
  calculateLayout,
  splitPane as splitLayoutPane,
  removePane as removeLayoutPane,
  getAllPaneIds,
  resizeSplitBetween,
  getSplitBounds,
  type LayoutNode,
  type SplitDirection,
} from "../terminal/mux/layout";
import { applyLayoutToContainer, detectBorderHit } from "../terminal/mux/pane-border";
import { OscColorHandler } from "../terminal/osc-colors";
import { CursorShapeStack } from "../terminal/osc-cursor-shape";
import { setupPtyHandlers, type PtyHandlerHandle } from "./pty-handler";
import { processPendingOscQueue, type OscHandlerContext } from "./osc-handler";
import { setupResizeObserver, handleCharSizeChange, type ResizeHandlerContext } from "./resize-handler";
import { handleBell, handleWheel, handleMiddleClickPaste } from "./ui-handler";

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
  private foldHandler: FoldHandler | null = null;
  private state: TerminalState | null = null;
  private renderer: ITerminalRenderer | null = null;
  private selectionController: SelectionController | null = null;
  private imageHandler: ImageHandler | null = null;
  private charSize: CharSize = { width: 8, height: 16 };
  private disconnectResizeObserver: (() => void) | null = null;
  private lastWindowTitle = "";
  private sessionExitCallback: ((sessionId: string) => void) | null = null;
  private titleChangeCallback: ((title: string) => void) | null = null;

  // Mux mode callbacks (set internally in init() to wire enterMuxMode/exitMuxMode)
  public muxAttachCallback: ((socketPath: string, sessionId: number) => void) | null = null;
  public muxDetachCallback: (() => void) | null = null;
  private muxClient: MuxClient | null = null;
  // Status bar will be implemented as an eMterm application-level feature
  private inMuxMode = false;
  private muxWindows: { id: number; name: string }[] = [];
  private activeMuxWindowIndex = 0;
  private muxPaneIds: number[] = []; // Actual pane IDs from daemon
  private muxPendingWindowCount = 0; // Windows waiting for PaneCreated response
  private muxPaneGrids: Map<number, WasmGrid> = new Map(); // WASM grids per pane
  private muxOriginalGrid: WasmGrid | null = null; // Original grid saved before mux mode
  private muxDetachedGrids: Map<string, Uint8Array> = new Map(); // Saved snapshots across detach/reattach (keyed by socket+session)
  private copyModeManager: CopyModeManager | null = null;
  private copyModeKeybinds: ViKeybinds | EmacsKeybinds | null = null;

  // Multi-pane state (within active window)
  private muxLayoutRoot: LayoutNode | null = null;
  private muxActivePaneId: number | null = null;
  private muxPaneCanvases: Map<number, {
    container: HTMLElement;
    canvas: HTMLCanvasElement;
    grid: WasmGrid;
    state: TerminalState;
    renderer: ITerminalRenderer;
  }> = new Map();
  private muxPaneContainer: HTMLElement | null = null;
  private muxPendingSplitCount = 0;
  private muxPendingSplitDirection: SplitDirection = "vertical";
  private muxDragState: {
    direction: "horizontal" | "vertical";
    paneA: number;
    paneB: number;
  } | null = null;
  private muxPreZoomLayout: LayoutNode | null = null;

  /** Callback to update tab UI when mux window state changes */
  public onMuxStateChange: ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null = null;
  private bellActivityCallback: (() => void) | null = null;
  private outputActivityCallback: (() => void) | null = null;
  private searchHandler: SearchHandler | null = null;
  private linkHandler: LinkHandler | null = null;
  private fileDropHandler: FileDropHandler | null = null;
  private _uploadManager: UploadManager | null = null;
  private downloadManager: DownloadSessionManager | null = null;
  private pendingOscQueue: { actionType: number; data: string }[] = [];
  private oscColorHandler: OscColorHandler = new OscColorHandler();
  private cursorShapeStack: CursorShapeStack = new CursorShapeStack();
  private ptyHandlerHandle: PtyHandlerHandle | null = null;

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
    this.terminalRoot.dataset.testid = "terminal";
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

    // Calculate initial terminal size (consistent with ResizeObserver)
    const { cols, rows } = calculateTerminalSize(
      this.container,
      this.charSize.width,
      this.charSize.height,
    );

    // Get font configuration from computed styles
    const computedStyle = window.getComputedStyle(this.container);
    const fontFamily = computedStyle.fontFamily || "monospace";
    const fontSize = parseFloat(computedStyle.fontSize) || 14;

    // Initialize terminal state and renderer
    this.state = new TerminalState(cols, rows);
    // Set cell size in pixels for CSI 14t/16t XTWINOPS responses
    this.state.setCellSizePx(
      Math.round(this.charSize.width),
      Math.round(this.charSize.height),
    );
    this.renderer = await createRendererAsync(terminalContainer, fontFamily, fontSize);

    // Apply diagnostic flags from environment variables
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const flags = await invoke<Record<string, boolean>>("get_diagnostic_flags");
      this.renderer.setDiagnosticFlags(flags);
    } catch {
      // Non-fatal: diagnostic flags are optional
    }

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
      if (cachedSettings.bold_brightens_ansi_colors !== undefined) {
        this.renderer.applySetting("boldBrightensAnsiColors", cachedSettings.bold_brightens_ansi_colors);
      }
      const fontChain = buildFontFamilyChain(
        cachedSettings.font_family_primary || "",
        cachedSettings.font_family_emoji || "",
        cachedSettings.font_family_secondary || "",
      );
      if (fontChain) {
        this.renderer.applySetting("fontFamily", fontChain);
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
      getScrollOffset: () => this.renderer?.getScrollOffset() ?? 0,
    });

    // Create PTY client
    this.ptyClient = new PtyClient();

    // Wire mux mode callbacks so OSC handler can trigger enterMuxMode/exitMuxMode
    this.muxAttachCallback = (socketPath, sessionId) => {
      this.enterMuxMode(socketPath, sessionId);
    };
    this.muxDetachCallback = () => {
      this.exitMuxMode();
    };

    // Listen for settings changes to update mux keybinds in real-time
    window.addEventListener("emterm-settings-changed", ((e: CustomEvent) => {
      if (e.detail?.key === "mux") {
        this.reloadMuxSettings();
      }
    }) as EventListener);

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
      onExitScrollback: () => this.exitScrollback(),
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
      onExitScrollback: () => this.exitScrollback(),
      onCopyModeKey: (event: KeyboardEvent) => this.handleCopyModeKey(event),
    };
    this.keyboardHandler = new KeyboardHandler(keyboardContext);
    // Attach to document but check if this tab's container is visible
    // This allows keyboard input to work even when focus is elsewhere in the window
    this.keyboardHandler.attach(document);

    // Initialize search handler
    this.searchHandler = new SearchHandler({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getImeHandler: () => this.imeHandler,
    });
    this.searchHandler.init(this.terminalRoot!);

    // Add middle-click paste handler (registered before MouseHandler so stopImmediatePropagation
    // prevents PTY mouse tracking from seeing middle button events when paste is enabled)
    terminalContainer.addEventListener('mousedown', (e) => {
      if (e.button === 1) {
        const settings = SettingsService.getCached();
        if (settings?.middle_click_paste !== false) {
          e.preventDefault();
          e.stopImmediatePropagation();
          // Suppress the matching mouseup to prevent an orphaned release event reaching the PTY.
          // Uses capture phase so it fires before MouseHandler's bubble-phase listener.
          const suppressMouseUp = (ev: MouseEvent) => {
            if (ev.button === 1) {
              ev.stopPropagation();
              ev.preventDefault();
              terminalContainer.removeEventListener('mouseup', suppressMouseUp, true);
            }
          };
          terminalContainer.addEventListener('mouseup', suppressMouseUp, true);
          this.handleMiddleClickPaste();
        }
      }
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

    // Add context menu handler for terminal right-click
    // Use this.container (.tab-content) instead of terminalContainer (.terminal-root)
    // so the handler also covers the padding area around the terminal
    this.container.addEventListener('contextmenu', (e) => {
      showTerminalContextMenu(e, { app: this });
    });

    // Attach selection controller
    this.selectionController.attach();

    // Add mouse wheel handler for scrollback
    terminalContainer.addEventListener('wheel', (e) => this.handleWheel(e));

    // Create fold handler
    this.foldHandler = new FoldHandler({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getTerminalRoot: () => this.terminalRoot,
      getCharSize: () => this.charSize,
    });

    // Create link handler for URL/file path detection and hover cursor
    this.linkHandler = new LinkHandler({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getTerminalRoot: () => this.terminalRoot,
      getCharSize: () => this.charSize,
    });
    this.linkHandler.attach(terminalContainer);

    // Add click handler for fold toggle (plain click) and URL opening (Ctrl+click)
    terminalContainer.addEventListener('click', (e) => {
      if (e.ctrlKey || e.metaKey) {
        this.linkHandler?.handleUrlClick(e);
      } else {
        this.foldHandler?.handleFoldClick(e);
      }
    });

    // Initialize image handler (ImageViewer + Kitty/SIXEL event listener)
    this.imageHandler = new ImageHandler({
      getPtyClient: () => this.ptyClient,
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getImeHandler: () => this.imeHandler,
      getOverlayRoot: () => this.overlayRoot,
    });
    await this.imageHandler.init();

    // Initialize SFTP file drop handler and upload manager
    this._uploadManager = new UploadManager();
    await this._uploadManager.init();

    this.fileDropHandler = new FileDropHandler({
      container: this.container,
      isActiveTab: () => this.container.style.display !== "none",
      getSshConnectionName: () => this.options.sshConnectionName || "",
      onSshDrop: (files: FileDropInfo[]) => {
        const destination = extractRemotePath(this.state?._workingDirectory || "");
        const sshConnectionName = this.options.sshConnectionName || "";
        this._uploadManager?.handleSshDrop(files, sshConnectionName, destination);
      },
      onLocalDrop: (paths: string[]) => {
        const text = formatPathsForPaste(paths);
        if (text && this.ptyClient) {
          const bytes = new TextEncoder().encode(text);
          this.ptyClient.write(bytes);
        }
      },
    });
    await this.fileDropHandler.attach();

    // Set markdown session manager's container for fullscreen view
    this.state.getMarkdownManager().setContainer(this.overlayRoot!);

    // Set data viewer session manager's container
    this.state.getDataViewerManager().setContainer(this.overlayRoot!);

    // Initialize download session manager
    this.downloadManager = new DownloadSessionManager();
    this.downloadManager.setContainer(this.overlayRoot!);

    // Wire up IME blur/focus for fullscreen markdown view (same pattern as ImageViewer)
    const fullscreenView = this.state.getMarkdownManager().getFullscreenView();
    fullscreenView.onShow(() => {
      this.imeHandler?.blur();
    });
    fullscreenView.onHide(() => {
      this.imeHandler?.focus();
    });

    // Wire up IME blur/focus for data viewer
    const dataViewerFullscreen = this.state.getDataViewerManager().getFullscreenView();
    dataViewerFullscreen.onShow(() => {
      this.imeHandler?.blur();
    });
    dataViewerFullscreen.onHide(() => {
      this.imeHandler?.focus();
    });

    // Make terminal focusable and set up resize observer before PTY spawn
    terminalContainer.tabIndex = 0;
    this.setupResizeObserver();

    // Initial render to show empty terminal immediately
    this.renderer.forceRender(this.state);

    // Focus terminal UI early for better UX
    this.imeHandler.focus();

    // Spawn PTY session (non-blocking UI)
    try {
      // Use profile-specific spawn overrides if provided, otherwise fall back to global settings
      const overrides = this.options.spawnOverrides;
      const shell = (overrides?.shell_path || cachedSettings?.shell_path) || undefined;
      const args = overrides?.shell_args?.length
        ? overrides.shell_args
        : cachedSettings?.shell_args?.length ? cachedSettings.shell_args : undefined;
      const env_vars = overrides?.env_vars;
      const working_directory = overrides?.working_directory || undefined;

      await this.ptyClient.spawn({ shell, args, cols, rows, env_vars, working_directory });

      // Force render after spawn completes (data may have arrived via onData)
      if (this.state && this.renderer) {
        this.renderer.forceRender(this.state);
      }
    } catch (error) {
      console.error("Failed to spawn PTY:", error);
      terminalContainer.textContent = `Failed to start terminal: ${error}`;
      return;
    }
  }

  /**
   * Register WASM callbacks on a TerminalCore instance.
   * Called once for primary core and again when alternate core becomes active.
   */
  private registerCoreCallbacks(core: ReturnType<TerminalState["getActiveCore"]>): void {
    core.set_osc_callback((actionType: number, data: string) => {
      // Queue data - do NOT access core here (recursive borrow error)
      // OSC 133 (SemanticPrompt) and OSC 777 (EmtermExtension) call
      // getScrollbackLength() which re-enters WASM during process_pty_data.
      this.pendingOscQueue.push({ actionType, data });
    });

    core.set_apc_callback((data: Uint8Array) => {
      // Queue data - do NOT access core here (recursive borrow error)
      this.imageHandler?.queueApc(data);
    });

    core.set_dcs_callback((data: Uint8Array) => {
      // Queue data - do NOT access core here (recursive borrow error)
      this.imageHandler?.queueDcs(data);
    });

    core.set_bell_callback(() => {
      this.state?.onBell?.();
    });

    core.set_device_response_callback((data: Uint8Array) => {
      // Skip Kitty Graphics Protocol APC responses (ESC _ G ...).
      // These are handled by the PTY reader thread's KittyScanner which
      // writes directly to the master fd for zero-latency delivery.
      if (data.length >= 3 && data[0] === 0x1b && data[1] === 0x5f && data[2] === 0x47) {
        return;
      }
      this.ptyClient?.write(data);
    });
  }

  /**
   * Sets up PTY output handlers using WASM parser + binary Channel IPC.
   * Delegates to pty-handler module.
   */
  private async setupPtyHandlers(): Promise<void> {
    this.ptyHandlerHandle = await setupPtyHandlers({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getPtyClient: () => this.ptyClient,
      getImeHandler: () => this.imeHandler,
      getImageHandler: () => this.imageHandler,
      getCharSize: () => this.charSize,
      registerCoreCallbacks: (core) => this.registerCoreCallbacks(core),
      processPendingOscQueue: () => this.processPendingOscQueue(),
      getOutputActivityCallback: () => this.outputActivityCallback,
      getSessionExitCallback: () => this.sessionExitCallback,
    });
  }

  /**
   * Build the OSC handler context from current state.
   */
  private getOscHandlerContext(): OscHandlerContext {
    return {
      state: this.state,
      renderer: this.renderer,
      ptyClient: this.ptyClient,
      oscColorHandler: this.oscColorHandler,
      cursorShapeStack: this.cursorShapeStack,
      imageHandler: this.imageHandler,
      downloadManager: this.downloadManager,
      terminalRoot: this.terminalRoot,
      titleChangeCallback: this.titleChangeCallback,
      lastWindowTitle: this.lastWindowTitle,
      setLastWindowTitle: (title: string) => { this.lastWindowTitle = title; },
      muxAttachCallback: this.muxAttachCallback,
      muxDetachCallback: this.muxDetachCallback,
    };
  }

  /**
   * Process all queued OSC events.
   * Safe to call after process_pty_data has returned (borrow released).
   */
  private processPendingOscQueue(): void {
    processPendingOscQueue(this.pendingOscQueue, this.getOscHandlerContext());
  }

  /**
   * Sets up resize observer for the container.
   * Delegates to resize-handler module.
   */
  private setupResizeObserver(): void {
    this.disconnectResizeObserver = setupResizeObserver(this.getResizeHandlerContext());
  }

  /**
   * Build the resize handler context from current state.
   */
  private getResizeHandlerContext(): ResizeHandlerContext {
    return {
      container: this.container,
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getPtyClient: () => this.ptyClient,
      getImeHandler: () => this.imeHandler,
      getMouseHandler: () => this.mouseHandler,
      getSelectionController: () => this.selectionController,
      getCharSize: () => this.charSize,
      getDisconnectResizeObserver: () => this.disconnectResizeObserver,
      setDisconnectResizeObserver: (fn) => { this.disconnectResizeObserver = fn; },
      setupResizeObserver: () => this.setupResizeObserver(),
      onMuxResize: (cols, rows) => {
        if (!this.inMuxMode || !this.muxClient) return;
        if (this.muxLayoutRoot) {
          // Multi-pane: recalculate layout and send all pane resizes
          this.applyMuxLayout();
          this.sendPaneResizes();
        } else {
          // Single-pane: send resize for the active pane
          const activePaneId = this.getActiveMuxPaneId();
          if (activePaneId != null) {
            this.sendMuxPaneResize(activePaneId);
          }
        }
      },
    };
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
   * Exit scrollback mode by resetting scroll offset to bottom.
   */
  exitScrollback(): void {
    if (this.renderer && this.renderer.getScrollOffset() > 0) {
      this.renderer.setScrollOffset(0);
      if (this.state) {
        this.renderer.forceRender(this.state);
      }
    }
  }

  /**
   * Handle mouse wheel events for scrollback.
   * Delegates to ui-handler module.
   */
  private handleWheel(e: WheelEvent): void {
    handleWheel(e, this.renderer, this.state, this.charSize);
  }

  /**
   * Handle middle-click paste from clipboard.
   * Delegates to ui-handler module.
   */
  private async handleMiddleClickPaste(): Promise<void> {
    await handleMiddleClickPaste(
      this.selectionController,
      this.ptyClient,
      this.imeHandler,
      () => this.exitScrollback(),
    );
  }

  /**
   * Handle BEL character based on bell_action setting.
   * Delegates to ui-handler module.
   */
  private handleBell(): void {
    handleBell(this.terminalRoot, this.bellActivityCallback);
  }

  /**
   * Toggle the search bar open/closed.
   */
  toggleSearch(): void {
    this.searchHandler?.toggleSearch();
  }

  /** Clear the terminal screen for mux window switching. */
  private clearMuxScreen(): void {
    if (this.state) {
      this.state.getWasmCore().reset();
      if (this.renderer) {
        this.renderer.forceRender(this.state);
      }
    }
  }

  /** Create a fresh WASM grid for a new mux pane and swap it in. */
  private createFreshMuxGrid(): void {
    if (!this.state) return;
    const cols = this.state.getWasmCore().cols();
    const rows = this.state.getWasmCore().rows();
    const newGrid = new WasmGrid(cols, rows, 10000);
    this.state.swapPrimaryGrid(newGrid);
    this.registerCoreCallbacks(this.state.getActiveCore());
    if (this.renderer) {
      this.renderer.forceRender(this.state);
    }
  }

  /** Switch to the current activeMuxWindowIndex: swap WASM grids and update UI. */
  private switchMuxWindow(previousIndex?: number): void {
    if (!this.state) return;

    // Save current pane's grid (swap out)
    if (previousIndex != null) {
      const prevPaneId = this.muxPaneIds[previousIndex];
      if (prevPaneId != null) {
        const currentGrid = this.state.getPrimaryGrid();
        if (currentGrid) {
          this.muxPaneGrids.set(prevPaneId, currentGrid);
        }
      }
    }

    // Restore the target pane's grid (swap in)
    const newPaneId = this.muxPaneIds[this.activeMuxWindowIndex];
    if (newPaneId != null) {
      const savedGrid = this.muxPaneGrids.get(newPaneId);
      if (savedGrid) {
        this.muxPaneGrids.delete(newPaneId);
        this.state.swapPrimaryGrid(savedGrid);
        this.registerCoreCallbacks(this.state.getActiveCore());
      } else {
        // No saved grid (first visit) — just clear
        this.state.getWasmCore().reset();
      }
    }

    if (this.renderer) {
      this.renderer.forceRender(this.state);
    }
    this.emitMuxStateChange();
  }

  /** Handle PaneCreated from daemon — register actual pane ID and update UI. */
  private handleMuxPaneCreated(paneId: number): void {
    // Check if this is a split pane response
    if (this.muxPendingSplitCount > 0) {
      this.muxPendingSplitCount--;
      this.handleMuxSplitPaneCreated(paneId, this.muxPendingSplitDirection);
      return;
    }

    if (this.muxPendingWindowCount <= 0) return;
    this.muxPendingWindowCount--;

    // Save current pane's grid before switching
    const previousIndex = this.activeMuxWindowIndex;
    const prevPaneId = this.muxPaneIds[previousIndex];
    if (prevPaneId != null && this.state) {
      const currentGrid = this.state.getPrimaryGrid();
      if (currentGrid) {
        this.muxPaneGrids.set(prevPaneId, currentGrid);
      }
    }

    const newIdx = this.muxWindows.length;
    this.muxWindows.push({ id: newIdx, name: `${newIdx}:shell` });
    this.muxPaneIds.push(paneId);
    this.activeMuxWindowIndex = newIdx;

    console.info(`[INFO][FRONTEND] Mux pane created: id=${paneId}, window=${newIdx}`);

    // Try to restore from detached snapshot, otherwise create fresh grid
    const detachedKey = `pane-${paneId}`;
    const detachedSnapshot = this.muxDetachedGrids.get(detachedKey);
    if (detachedSnapshot && this.state) {
      const restored = this.state.restoreFromSnapshot(detachedSnapshot);
      if (restored) {
        this.registerCoreCallbacks(this.state.getActiveCore());
        console.info(`[INFO][FRONTEND] Restored detached grid for window ${newIdx} (key=${detachedKey})`);
      } else {
        this.createFreshMuxGrid();
      }
      this.muxDetachedGrids.delete(detachedKey);
    } else {
      this.createFreshMuxGrid();
    }

    // Send initial resize so daemon PTY matches actual terminal dimensions
    this.sendMuxPaneResize(paneId);

    // Ensure canvas reflects restored/fresh grid (without this, canvas stays blank)
    if (this.renderer && this.state) {
      this.renderer.forceRender(this.state);
    }

    // After all pending windows are received during reattach, switch to first window
    if (this.muxPendingWindowCount === 0 && this.muxWindows.length > 1 && this.activeMuxWindowIndex !== 0) {
      const prev = this.activeMuxWindowIndex;
      this.activeMuxWindowIndex = 0;
      this.switchMuxWindow(prev);
    }

    this.emitMuxStateChange();
  }

  /** Send a Resize message to the daemon for a single pane using current terminal dimensions. */
  private sendMuxPaneResize(paneId: number): void {
    if (!this.state || !this.muxClient) return;
    const cols = this.state.getWasmCore().cols();
    const rows = this.state.getWasmCore().rows();
    const payload = new Uint8Array(4);
    payload[0] = cols & 0xFF;
    payload[1] = (cols >> 8) & 0xFF;
    payload[2] = rows & 0xFF;
    payload[3] = (rows >> 8) & 0xFF;
    this.sendMuxControl(MuxMessageType.Resize, paneId, payload);
  }

  /** Handle a mux pane exiting (shell closed). Remove the window and switch if needed. */
  private handleMuxPaneExited(paneId: number): void {
    // Notify daemon to clean up the exited pane (cascade: pane→window→session)
    this.sendMuxControl(MuxMessageType.DestroyPane, paneId);

    // Multi-pane mode: remove from layout
    if (this.muxLayoutRoot && this.muxPaneCanvases.has(paneId)) {
      this.removeMuxPane(paneId);
      return;
    }

    const windowIdx = this.muxPaneIds.indexOf(paneId);
    if (windowIdx === -1) return;

    console.info(`[INFO][FRONTEND] Mux pane ${paneId} exited (window ${windowIdx})`);

    // Clean up snapshot for the exited pane
    this.muxPaneGrids.delete(paneId);

    // If the exited pane is NOT the active one, save current pane's snapshot
    // before the index adjustment that follows
    const wasActive = windowIdx === this.activeMuxWindowIndex;

    // Remove the window
    this.muxWindows.splice(windowIdx, 1);
    this.muxPaneIds.splice(windowIdx, 1);

    // If no windows left, exit mux mode
    if (this.muxWindows.length === 0) {
      this.exitMuxMode();
      return;
    }

    // Adjust active window index
    if (this.activeMuxWindowIndex >= this.muxWindows.length) {
      this.activeMuxWindowIndex = this.muxWindows.length - 1;
    }

    // Renumber window names
    for (let i = 0; i < this.muxWindows.length; i++) {
      this.muxWindows[i]!.name = `${i}:shell`;
    }

    // Only switch if the active pane was the one that exited
    if (wasActive) {
      this.switchMuxWindow();
    } else {
      this.emitMuxStateChange();
    }
  }

  /** Re-apply mux keybind settings (call when settings change at runtime). */
  reloadMuxSettings(): void {
    if (!this.inMuxMode || !this.keyboardHandler) return;
    const muxSettings = SettingsService.getCached()?.mux;
    if (muxSettings) {
      this.keyboardHandler.updateMuxSettings(
        muxSettings.prefix ?? "Ctrl+B",
        muxSettings.keybinds ?? {},
      );
    }
  }

  /** Notify listeners of mux window state changes. */
  private emitMuxStateChange(): void {
    this.onMuxStateChange?.({
      windowCount: this.muxWindows.length,
      activeWindow: this.activeMuxWindowIndex,
      windowNames: this.muxWindows.map((w) => w.name),
    });
  }

  /** Enter mux mode -- connect to daemon, enable prefix key, show status bar. */
  async enterMuxMode(socketPath: string, sessionId: number): Promise<void> {
    if (this.inMuxMode) return;
    this.inMuxMode = true;

    console.info(`[INFO][FRONTEND] Entering mux mode: socket=${socketPath}, session=${sessionId}`);

    // Connect to daemon
    let muxSessions: import("../terminal/mux/mux-client").MuxSessionInfo[] = [];
    try {
      this.muxClient = new MuxClient();
      muxSessions = await this.muxClient.connect(socketPath);
      console.info(`[INFO][FRONTEND] Mux connected: ${muxSessions.length} session(s)`);
    } catch (e) {
      console.error("[ERROR][FRONTEND] Mux connect failed:", e);
      this.inMuxMode = false;
      this.muxClient = null;
      return;
    }

    // Set up PTY output handler -- route to correct pane
    this.muxClient.setOnPtyOutput((paneId: number, data: Uint8Array) => {
      // Multi-pane mode: route to specific pane's canvas/grid
      if (this.muxLayoutRoot && this.muxPaneCanvases.has(paneId)) {
        this.renderMuxPaneOutput(paneId, data);
        return;
      }

      // Single-pane mode: route to main renderer
      const activePaneId = this.muxPaneIds[this.activeMuxWindowIndex];
      if (activePaneId === undefined) {
        // PaneCreated hasn't arrived yet -- accept all output during init
        if (this.ptyHandlerHandle) {
          this.ptyHandlerHandle.injectData(data);
        }
        return;
      }
      if (paneId === activePaneId) {
        if (this.ptyHandlerHandle) {
          this.ptyHandlerHandle.injectData(data);
        }
      } else {
        // Route to inactive pane's saved grid (preserves ring buffer replay data)
        const savedGrid = this.muxPaneGrids.get(paneId);
        if (savedGrid) {
          savedGrid.core.process_pty_data(data);
        }
      }
    });

    // Set up PTY exit handler -- remove window when its pane exits
    this.muxClient.setOnPtyExited((paneId: number) => {
      this.handleMuxPaneExited(paneId);
    });

    // Set up pane created handler -- receive actual pane ID from daemon
    this.muxClient.setOnPaneCreated((paneId: number) => {
      this.handleMuxPaneCreated(paneId);
    });

    // Start output stream
    try {
      await this.muxClient.startOutputStream();
    } catch (e) {
      console.error("[ERROR][FRONTEND] Mux start output stream failed:", e);
    }

    // Route all PTY writes to mux daemon via proxy
    if (this.ptyClient) {
      this.ptyClient.setWriteProxy((data: Uint8Array) => {
        if (!this.muxClient) return Promise.resolve();
        const activePaneId = this.getActiveMuxPaneId() ?? this.muxPaneIds[this.activeMuxWindowIndex] ?? 1;
        return this.muxClient.sendInput(activePaneId, data);
      });
    }

    // Suppress original PTY output during mux mode
    if (this.ptyHandlerHandle) {
      this.ptyHandlerHandle.suppressOriginalPty = true;
    }

    // Save the original grid and create a fresh one for mux mode
    if (this.state) {
      this.muxOriginalGrid = this.state.getPrimaryGrid();
      const cols = this.state.getWasmCore().cols();
      const rows = this.state.getWasmCore().rows();
      const freshGrid = new WasmGrid(cols, rows, 10000);
      this.state.swapPrimaryGrid(freshGrid);
      this.registerCoreCallbacks(this.state.getActiveCore());
      if (this.renderer) {
        this.renderer.forceRender(this.state);
      }
    }

    // Initialize mux window tracking
    this.muxWindows = [];
    this.activeMuxWindowIndex = 0;
    this.muxPaneIds = [];
    this.muxPendingWindowCount = 0;

    // Check if daemon has existing panes (reattach case)
    const existingPanes = muxSessions.reduce((sum, s) => sum + s.pane_count, 0);

    if (existingPanes > 0) {
      // Reattach: daemon will send PaneCreated + buffered output for existing panes
      this.muxPendingWindowCount = existingPanes;
      console.info(`[INFO][FRONTEND] Reattaching to ${existingPanes} existing pane(s)`);
    } else {
      // Fresh start: create initial window
      try {
        this.muxPendingWindowCount++;
        await this.muxClient.sendControl(MuxMessageType.CreateWindow, 0);
      } catch (e) {
        console.error("[ERROR][FRONTEND] Mux create window failed:", e);
      }
    }

    // Enable prefix key handling
    const muxSettings = SettingsService.getCached()?.mux;
    if (this.keyboardHandler) {
      this.keyboardHandler.enableMuxMode(
        muxSettings?.prefix ?? "Ctrl+B",
        muxSettings?.keybinds ?? {},
        (action) => this.handleMuxAction(action),
      );
    }

    // TODO: Status bar will be an eMterm application-level feature
  }

  /** Exit mux mode -- disconnect, disable prefix key, hide status bar. */
  exitMuxMode(): void {
    if (!this.inMuxMode) return;
    this.inMuxMode = false;

    console.info("[INFO][FRONTEND] Exiting mux mode");

    // Exit copy mode if active
    if (this.copyModeManager) {
      this.copyModeManager.exit();
      this.copyModeManager = null;
      this.copyModeKeybinds = null;
    }

    // Re-enable original PTY output
    if (this.ptyHandlerHandle) {
      this.ptyHandlerHandle.suppressOriginalPty = false;
    }

    // Save ALL pane grid snapshots for potential reattach (keyed by pane-{id})
    if (this.state && this.muxPaneIds.length > 0) {
      // Save the active pane's grid (currently in state)
      const activePaneId = this.muxPaneIds[this.activeMuxWindowIndex];
      if (activePaneId != null) {
        try {
          const snapshot = this.state.getWasmCore().wasm_snapshot_to_bytes();
          this.muxDetachedGrids.set(`pane-${activePaneId}`, snapshot);
        } catch { /* ignore */ }
      }
      // Save inactive panes' grids (stored in muxPaneGrids)
      for (const [paneId, grid] of this.muxPaneGrids) {
        try {
          const s = grid.core.wasm_snapshot_to_bytes();
          this.muxDetachedGrids.set(`pane-${paneId}`, s);
        } catch { /* ignore */ }
      }
    }

    // Restore original grid
    if (this.muxOriginalGrid && this.state) {
      this.state.swapPrimaryGrid(this.muxOriginalGrid);
      this.registerCoreCallbacks(this.state.getActiveCore());
      if (this.renderer) {
        this.renderer.forceRender(this.state);
      }
      this.muxOriginalGrid = null;
    }

    // Clean up multi-pane state
    if (this.muxLayoutRoot) {
      this.exitMultiPaneMode(null);
    }

    // Reset mux window tracking
    this.muxWindows = [];
    this.activeMuxWindowIndex = 0;
    this.muxPaneIds = [];
    this.muxPendingWindowCount = 0;
    this.muxPendingSplitCount = 0;
    this.muxPaneGrids.clear();
    this.emitMuxStateChange();

    // Disable prefix key handling
    if (this.keyboardHandler) {
      this.keyboardHandler.disableMuxMode();
    }

    // Restore direct PTY writes
    if (this.ptyClient) {
      this.ptyClient.setWriteProxy(null);
    }

    // Disconnect
    if (this.muxClient) {
      this.muxClient.disconnect().catch(() => {});
      this.muxClient = null;
    }
  }

  /** Enter mux copy mode with vi or emacs keybindings. */
  private enterCopyMode(): void {
    if (!this.state || !this.inMuxMode) return;

    const core = this.state.getWasmCore();
    const cols = core.cols();
    const rows = core.rows();

    this.copyModeManager = new CopyModeManager();

    // Default to vi keybindings (no copy_mode setting exists yet)
    const muxSettings = SettingsService.getCached()?.mux;
    const mode = (muxSettings as unknown as Record<string, unknown> | undefined)?.copy_mode as string | undefined ?? "vi";

    if (mode === "emacs") {
      this.copyModeKeybinds = new EmacsKeybinds(this.copyModeManager, cols, rows);
    } else {
      this.copyModeKeybinds = new ViKeybinds(this.copyModeManager, cols, rows);
    }

    this.copyModeManager.setOnStateChange((state) => {
      if (state === "inactive") {
        this.exitCopyMode();
      }
    });

    this.copyModeManager.setOnSelectionChange(() => {
      if (this.renderer && this.state) {
        this.renderer.forceRender(this.state);
      }
    });

    this.copyModeManager.enter();
    console.info("[INFO][FRONTEND] Entered mux copy mode");
  }

  /** Exit mux copy mode. */
  private exitCopyMode(): void {
    this.copyModeManager = null;
    this.copyModeKeybinds = null;
    if (this.renderer && this.state) {
      this.renderer.forceRender(this.state);
    }
    console.info("[INFO][FRONTEND] Exited mux copy mode");
  }

  /** Handle keyboard input during copy mode. Returns true if the key was consumed. */
  private handleCopyModeKey(event: KeyboardEvent): boolean {
    if (!this.copyModeManager || !this.copyModeKeybinds) return false;
    if (!this.copyModeManager.isActive) return false;

    // Save selection before handling key (yank clears it and exits copy mode)
    const preYankSelection = this.copyModeManager.getSelection();

    let consumed: boolean;
    if (this.copyModeKeybinds instanceof EmacsKeybinds) {
      consumed = this.copyModeKeybinds.handleKeyEvent(event);
    } else {
      consumed = (this.copyModeKeybinds as ViKeybinds).handleKey(event.key);
    }

    if (!consumed) return false;

    // If copy mode just exited and we had a selection, it was a yank/copy action
    if (!this.copyModeManager.isActive && preYankSelection) {
      this.copySelectionToClipboard(preYankSelection);
    }

    return true;
  }

  /** Extract text from the terminal grid for the given selection and copy to clipboard. */
  private async copySelectionToClipboard(selection: CopyModeSelection): Promise<void> {
    if (!this.state) return;

    const text = this.state.extractText(
      selection.startCol,
      selection.startRow,
      selection.endCol,
      selection.endRow,
    );

    if (!text) return;

    try {
      await navigator.clipboard.writeText(text);
      console.info(`[INFO][FRONTEND] Copy mode: copied ${text.length} chars to clipboard`);
    } catch (e) {
      console.error("[ERROR][FRONTEND] Copy mode clipboard write failed:", e);
    }
  }

  /** Paste clipboard text into the active PTY (mux paste action). */
  private async pasteFromClipboard(): Promise<void> {
    try {
      const text = await navigator.clipboard.readText();
      if (text && this.ptyClient) {
        const data = new TextEncoder().encode(text);
        await this.ptyClient.write(data);
        console.info(`[INFO][FRONTEND] Mux paste: ${text.length} chars`);
      }
    } catch (e) {
      console.error("[ERROR][FRONTEND] Mux paste failed:", e);
    }
  }

  /** Handle mux action dispatched by PrefixKeyHandler. */
  private handleMuxAction(action: MuxAction): void {
    console.info(`[INFO][FRONTEND] Mux action: ${action.type}`);

    switch (action.type) {
      case "detach":
        this.sendMuxControl(MuxMessageType.Detach, 0);
        this.exitMuxMode();
        break;
      case "new-window": {
        // Actual pane ID will arrive via PaneCreated event
        this.muxPendingWindowCount++;
        this.sendMuxControl(MuxMessageType.CreateWindow, 0);
        break;
      }
      case "split-vertical": {
        const activePaneId = this.getActiveMuxPaneId();
        if (activePaneId != null) {
          this.muxPendingSplitCount++;
          this.muxPendingSplitDirection = "vertical";
          this.sendMuxControl(MuxMessageType.SplitPane, activePaneId, new Uint8Array([0x01]));
        }
        break;
      }
      case "split-horizontal": {
        const activePaneId = this.getActiveMuxPaneId();
        if (activePaneId != null) {
          this.muxPendingSplitCount++;
          this.muxPendingSplitDirection = "horizontal";
          this.sendMuxControl(MuxMessageType.SplitPane, activePaneId, new Uint8Array([0x00]));
        }
        break;
      }
      case "close-pane": {
        const activePaneId = this.getActiveMuxPaneId();
        if (activePaneId != null) {
          this.sendMuxControl(MuxMessageType.DestroyPane, activePaneId);
        }
        break;
      }
      case "next-window": {
        if (this.muxWindows.length > 1) {
          const prev = this.activeMuxWindowIndex;
          this.activeMuxWindowIndex = (this.activeMuxWindowIndex + 1) % this.muxWindows.length;
          this.switchMuxWindow(prev);
        }
        break;
      }
      case "prev-window": {
        if (this.muxWindows.length > 1) {
          const prev = this.activeMuxWindowIndex;
          this.activeMuxWindowIndex = (this.activeMuxWindowIndex - 1 + this.muxWindows.length) % this.muxWindows.length;
          this.switchMuxWindow(prev);
        }
        break;
      }
      case "rename-window": {
        const currentName = this.muxWindows[this.activeMuxWindowIndex]?.name ?? "";
        const newName = prompt("Rename window:", currentName);
        if (newName != null && newName !== "") {
          const win = this.muxWindows[this.activeMuxWindowIndex];
          if (win) {
            win.name = newName;
            this.emitMuxStateChange();
          }
          // Notify daemon: RenameWindowMsg { name: String }
          // bincode for String = u64 length (LE) + UTF-8 bytes
          const nameBytes = new TextEncoder().encode(newName);
          const payload = new Uint8Array(8 + nameBytes.length);
          const view = new DataView(payload.buffer);
          view.setBigUint64(0, BigInt(nameBytes.length), true);
          payload.set(nameBytes, 8);
          const windowId = win?.id ?? 0;
          this.sendMuxControl(MuxMessageType.RenameWindow, windowId, payload);
        }
        break;
      }
      case "prefix-passthrough":
        // Send the prefix key itself to PTY
        if (this.ptyClient) {
          const muxSettings = SettingsService.getCached()?.mux;
          const prefix = muxSettings?.prefix ?? "Ctrl+B";
          const byte = this.prefixKeyToByte(prefix);
          if (byte !== null) {
            this.ptyClient.write(new Uint8Array([byte])).catch(() => {});
          }
        }
        break;
      case "next-pane": {
        if (this.muxLayoutRoot) {
          const paneIds = getAllPaneIds(this.muxLayoutRoot);
          const currentIdx = paneIds.indexOf(this.muxActivePaneId!);
          const nextIdx = (currentIdx + 1) % paneIds.length;
          this.setActiveMuxPane(paneIds[nextIdx]!);
        }
        break;
      }
      case "prev-pane": {
        if (this.muxLayoutRoot) {
          const paneIds = getAllPaneIds(this.muxLayoutRoot);
          const currentIdx = paneIds.indexOf(this.muxActivePaneId!);
          const prevIdx = (currentIdx - 1 + paneIds.length) % paneIds.length;
          this.setActiveMuxPane(paneIds[prevIdx]!);
        }
        break;
      }
      case "zoom-toggle":
        this.toggleMuxZoom();
        break;
      case "copy-mode":
        this.enterCopyMode();
        break;
      case "paste":
        this.pasteFromClipboard();
        break;
    }
  }

  /** Send a control message to the mux daemon. */
  private sendMuxControl(msgType: number, paneId: number, payload?: Uint8Array): void {
    if (!this.muxClient) return;
    this.muxClient.sendControl(msgType, paneId, payload).catch((e) => {
      console.error(`[ERROR][FRONTEND] Mux control failed (type=0x${msgType.toString(16)}):`, e);
    });
  }

  /** Convert a prefix keybind string to a control byte (e.g., "Ctrl+B" → 0x02). */
  private prefixKeyToByte(prefix: string): number | null {
    const match = prefix.match(/^Ctrl\+([A-Z])$/i);
    if (match) {
      return match[1]!.toUpperCase().charCodeAt(0) - 0x40; // Ctrl+A=1, Ctrl+B=2, etc.
    }
    return null;
  }

  /** Get the active mux pane ID (multi-pane or single-pane mode). */
  private getActiveMuxPaneId(): number | null {
    if (this.muxActivePaneId != null) return this.muxActivePaneId;
    return this.muxPaneIds[this.activeMuxWindowIndex] ?? null;
  }

  /** Handle a split pane creation from the daemon. */
  private handleMuxSplitPaneCreated(newPaneId: number, direction: SplitDirection): void {
    if (!this.state || !this.terminalRoot) return;

    const activePaneId = this.getActiveMuxPaneId();
    if (activePaneId == null) return;

    // First split: transition from single-canvas to multi-canvas mode
    if (!this.muxLayoutRoot) {
      this.initMultiPaneMode(activePaneId);
    }

    // Split the active pane in the layout tree
    const containerWidth = this.terminalRoot.clientWidth;
    const containerHeight = this.terminalRoot.clientHeight;
    const newLayout = splitLayoutPane(
      this.muxLayoutRoot!, activePaneId, newPaneId, direction,
      containerWidth, containerHeight,
      this.charSize.width, this.charSize.height,
    );
    if (!newLayout) {
      console.warn("[WARN][FRONTEND] Split refused: pane too small");
      return;
    }
    this.muxLayoutRoot = newLayout;

    // Create canvas and renderer for the new pane
    this.createPaneCanvas(newPaneId);

    // Set new pane as active
    this.setActiveMuxPane(newPaneId);

    // Apply layout to all pane canvases
    this.applyMuxLayout();

    // Send resize messages for all panes based on new layout
    this.sendPaneResizes();

    console.info(`[INFO][FRONTEND] Split pane created: id=${newPaneId}, direction=${direction}`);
  }

  /** Initialize multi-pane mode from single-pane mode. */
  private initMultiPaneMode(existingPaneId: number): void {
    if (!this.terminalRoot || !this.state) return;

    // Create overlay container for pane canvases
    if (!this.muxPaneContainer) {
      this.muxPaneContainer = document.createElement("div");
      this.muxPaneContainer.className = "mux-pane-container";
      this.muxPaneContainer.style.position = "absolute";
      this.muxPaneContainer.style.inset = "0";
      this.terminalRoot.appendChild(this.muxPaneContainer);
    }
    this.muxPaneContainer.style.display = "block";

    // Initialize layout tree with existing pane as single leaf
    this.muxLayoutRoot = { type: "leaf", paneId: existingPaneId };

    // Get the current grid and renderer for the existing pane
    const existingGrid = this.state.getPrimaryGrid();
    if (!existingGrid) return;

    // Create a pane canvas for the existing pane
    this.createPaneCanvas(existingPaneId);

    // Move the existing grid into the pane canvas state
    const paneEntry = this.muxPaneCanvases.get(existingPaneId);
    if (paneEntry) {
      // Dispose the auto-created grid and replace with the existing one
      paneEntry.grid.dispose();
      paneEntry.grid = existingGrid;
      paneEntry.state.swapPrimaryGrid(existingGrid);
    }

    // Hide the main canvas (renderer manages it)
    const mainCanvas = this.terminalRoot.querySelector("canvas:not(.mux-pane-canvas)") as HTMLCanvasElement | null;
    if (mainCanvas) {
      mainCanvas.style.display = "none";
    }

    this.muxActivePaneId = existingPaneId;

    // Wire up drag-resize for pane borders
    this.initMuxDragResize();
  }

  /** Create a canvas element and renderer for a new pane. */
  private createPaneCanvas(paneId: number): void {
    if (!this.muxPaneContainer || !this.state) return;

    const container = document.createElement("div");
    container.className = "mux-pane";
    container.dataset.paneId = String(paneId);
    container.style.position = "absolute";
    container.style.overflow = "hidden";
    container.style.boxSizing = "border-box";

    this.muxPaneContainer.appendChild(container);

    // Create a WASM grid and TerminalState for this pane
    const cols = this.state.getWasmCore().cols();
    const rows = this.state.getWasmCore().rows();
    const grid = new WasmGrid(cols, rows, 10000);
    const paneState = new TerminalState(cols, rows);
    paneState.swapPrimaryGrid(grid);

    // Create a renderer inside this pane container
    const computedStyle = window.getComputedStyle(this.container);
    const fontFamily = computedStyle.fontFamily || "monospace";
    const fontSize = parseFloat(computedStyle.fontSize) || 14;
    const paneRenderer = createRenderer(container, fontFamily, fontSize);

    // Apply cached settings to the pane renderer
    const cachedSettings = SettingsService.getCached();
    if (cachedSettings?.terminal_color_scheme) {
      const userScheme = cachedSettings.custom_color_schemes?.find(
        (s) => s.name === cachedSettings.terminal_color_scheme,
      );
      if (userScheme) {
        paneRenderer.setUserColorScheme(userScheme);
      } else {
        paneRenderer.applySetting("colorScheme", cachedSettings.terminal_color_scheme);
      }
    }
    if (cachedSettings?.cursor_style) {
      paneRenderer.applySetting("cursorStyle", cachedSettings.cursor_style);
    }
    if (cachedSettings?.bold_brightens_ansi_colors !== undefined) {
      paneRenderer.applySetting("boldBrightensAnsiColors", cachedSettings.bold_brightens_ansi_colors);
    }

    const canvas = container.querySelector("canvas") as HTMLCanvasElement;
    this.muxPaneCanvases.set(paneId, { container, canvas, grid, state: paneState, renderer: paneRenderer });
  }

  /** Set the active pane and update visual indicators. */
  private setActiveMuxPane(paneId: number): void {
    this.muxActivePaneId = paneId;

    // Update active pane border styling
    if (this.muxPaneContainer && this.muxLayoutRoot) {
      const layoutResults = calculateLayout(
        this.muxLayoutRoot,
        this.muxPaneContainer.clientWidth || this.terminalRoot!.clientWidth,
        this.muxPaneContainer.clientHeight || this.terminalRoot!.clientHeight,
        this.charSize.width,
        this.charSize.height,
      );
      applyLayoutToContainer(this.muxPaneContainer, layoutResults, paneId);
    }
  }

  /** Apply the current layout tree to position all pane canvases. */
  private applyMuxLayout(): void {
    if (!this.muxLayoutRoot || !this.muxPaneContainer || !this.terminalRoot) return;

    const containerWidth = this.muxPaneContainer.clientWidth || this.terminalRoot.clientWidth;
    const containerHeight = this.muxPaneContainer.clientHeight || this.terminalRoot.clientHeight;

    const results = calculateLayout(
      this.muxLayoutRoot,
      containerWidth,
      containerHeight,
      this.charSize.width,
      this.charSize.height,
    );

    applyLayoutToContainer(this.muxPaneContainer, results, this.muxActivePaneId);

    // Update each pane's canvas dimensions, grid size, and state
    for (const result of results) {
      const paneEntry = this.muxPaneCanvases.get(result.paneId);
      if (!paneEntry) continue;

      // Resize the renderer canvas
      paneEntry.renderer.resize(result.cols, result.rows);

      // Resize the WASM grid and terminal state to match
      if (paneEntry.grid.cols !== result.cols || paneEntry.grid.rows !== result.rows) {
        paneEntry.state.resize(result.cols, result.rows);
      }
    }
  }

  /** Send resize messages to daemon for all panes in the current layout. */
  private sendPaneResizes(): void {
    if (!this.muxLayoutRoot || !this.muxPaneContainer || !this.terminalRoot) return;

    const containerWidth = this.muxPaneContainer.clientWidth || this.terminalRoot.clientWidth;
    const containerHeight = this.muxPaneContainer.clientHeight || this.terminalRoot.clientHeight;

    const results = calculateLayout(
      this.muxLayoutRoot,
      containerWidth,
      containerHeight,
      this.charSize.width,
      this.charSize.height,
    );

    for (const result of results) {
      // Encode cols/rows as bincode-compatible u16 LE pairs
      const payload = new Uint8Array(4);
      payload[0] = result.cols & 0xFF;
      payload[1] = (result.cols >> 8) & 0xFF;
      payload[2] = result.rows & 0xFF;
      payload[3] = (result.rows >> 8) & 0xFF;
      this.sendMuxControl(MuxMessageType.Resize, result.paneId, payload);
    }
  }

  /** Remove a pane from the multi-pane layout. */
  private removeMuxPane(paneId: number): void {
    // If zoomed and removing the zoomed pane, unzoom first
    if (this.muxPreZoomLayout && paneId === this.muxActivePaneId) {
      this.muxLayoutRoot = this.muxPreZoomLayout;
      this.muxPreZoomLayout = null;
      for (const [, p] of this.muxPaneCanvases) {
        p.container.style.display = "";
      }
    }

    // Remove from layout tree
    if (this.muxLayoutRoot) {
      const newRoot = removeLayoutPane(this.muxLayoutRoot, paneId);
      if (newRoot === null) {
        // Last pane removed -- this shouldn't happen here, handled by handleMuxPaneExited
        return;
      }
      this.muxLayoutRoot = newRoot;
    }

    // Clean up pane canvas and state (state.dispose() also frees the WASM grid)
    const paneEntry = this.muxPaneCanvases.get(paneId);
    if (paneEntry) {
      paneEntry.state.dispose();
      paneEntry.container.remove();
      this.muxPaneCanvases.delete(paneId);
    }

    // If only one pane left, exit multi-pane mode
    const remainingPanes = this.muxLayoutRoot ? getAllPaneIds(this.muxLayoutRoot) : [];
    if (remainingPanes.length <= 1) {
      this.exitMultiPaneMode(remainingPanes[0] ?? null);
      return;
    }

    // Select new active pane if needed
    if (this.muxActivePaneId === paneId) {
      this.setActiveMuxPane(remainingPanes[0]!);
    }

    this.applyMuxLayout();
    this.sendPaneResizes();
  }

  /** Exit multi-pane mode, returning to single-canvas rendering. */
  private exitMultiPaneMode(remainingPaneId: number | null): void {
    // Restore the remaining pane's grid as the main grid
    if (remainingPaneId != null) {
      const paneEntry = this.muxPaneCanvases.get(remainingPaneId);
      if (paneEntry && this.state) {
        this.state.swapPrimaryGrid(paneEntry.grid);
        this.registerCoreCallbacks(this.state.getActiveCore());
        // Swap a dummy grid into the pane state before disposing to avoid
        // double-freeing the grid we just moved into this.state
        const dummyGrid = new WasmGrid(1, 1, 0);
        paneEntry.state.swapPrimaryGrid(dummyGrid);
        paneEntry.state.dispose();
        paneEntry.container.remove();
        this.muxPaneCanvases.delete(remainingPaneId);
      }
    }

    // Clean up any remaining pane canvases (state.dispose() also frees the WASM grid)
    for (const [, paneEntry] of this.muxPaneCanvases) {
      paneEntry.state.dispose();
      paneEntry.container.remove();
    }
    this.muxPaneCanvases.clear();

    // Remove pane container
    if (this.muxPaneContainer) {
      this.muxPaneContainer.remove();
      this.muxPaneContainer = null;
    }

    // Show the main canvas again
    if (this.terminalRoot) {
      const mainCanvas = this.terminalRoot.querySelector("canvas:not(.mux-pane-canvas)") as HTMLCanvasElement | null;
      if (mainCanvas) {
        mainCanvas.style.display = "block";
      }
    }

    this.muxLayoutRoot = null;
    this.muxActivePaneId = null;
    this.muxPreZoomLayout = null;

    if (this.state && this.renderer) {
      this.renderer.forceRender(this.state);
    }
  }

  /** Initialize drag-resize listeners on the mux pane container. */
  private initMuxDragResize(): void {
    if (!this.muxPaneContainer) return;

    this.muxPaneContainer.addEventListener("mousedown", (e) => {
      if (!this.muxLayoutRoot || !this.muxPaneContainer) return;
      const rect = this.muxPaneContainer.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;

      const results = calculateLayout(
        this.muxLayoutRoot,
        this.muxPaneContainer.clientWidth,
        this.muxPaneContainer.clientHeight,
        this.charSize.width,
        this.charSize.height,
      );

      const hit = detectBorderHit(x, y, results);
      if (!hit) return;

      e.preventDefault();

      this.muxDragState = {
        direction: hit.direction,
        paneA: hit.paneA,
        paneB: hit.paneB,
      };

      document.addEventListener("mousemove", this.handleMuxDragMove);
      document.addEventListener("mouseup", this.handleMuxDragEnd);
    });

    // Cursor change on hover
    this.muxPaneContainer.addEventListener("mousemove", (e) => {
      if (this.muxDragState) return;
      if (!this.muxLayoutRoot || !this.muxPaneContainer) return;

      const rect = this.muxPaneContainer.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;

      const results = calculateLayout(
        this.muxLayoutRoot,
        this.muxPaneContainer.clientWidth,
        this.muxPaneContainer.clientHeight,
        this.charSize.width,
        this.charSize.height,
      );

      const hit = detectBorderHit(x, y, results);
      this.muxPaneContainer.style.cursor = hit
        ? (hit.direction === "vertical" ? "col-resize" : "row-resize")
        : "";
    });
  }

  private handleMuxDragMove = (e: MouseEvent): void => {
    if (!this.muxDragState || !this.muxLayoutRoot || !this.muxPaneContainer) return;

    const containerRect = this.muxPaneContainer.getBoundingClientRect();
    const containerWidth = this.muxPaneContainer.clientWidth;
    const containerHeight = this.muxPaneContainer.clientHeight;

    // Find the bounds of the parent split that contains both panes
    const splitBounds = getSplitBounds(
      this.muxLayoutRoot,
      this.muxDragState.paneA,
      this.muxDragState.paneB,
      0, 0,
      containerWidth,
      containerHeight,
      this.charSize.width,
      this.charSize.height,
    );
    if (!splitBounds) return;

    // Calculate ratio relative to the parent split's bounds
    const mousePos = this.muxDragState.direction === "vertical"
      ? e.clientX - containerRect.left
      : e.clientY - containerRect.top;

    const splitStart = this.muxDragState.direction === "vertical"
      ? splitBounds.x
      : splitBounds.y;

    const splitSize = this.muxDragState.direction === "vertical"
      ? splitBounds.width
      : splitBounds.height;

    const newRatio = Math.max(0.1, Math.min(0.9, (mousePos - splitStart) / splitSize));

    this.muxLayoutRoot = resizeSplitBetween(
      this.muxLayoutRoot,
      this.muxDragState.paneA,
      this.muxDragState.paneB,
      newRatio,
    );
    this.applyMuxLayout();
  };

  private handleMuxDragEnd = (_e: MouseEvent): void => {
    document.removeEventListener("mousemove", this.handleMuxDragMove);
    document.removeEventListener("mouseup", this.handleMuxDragEnd);
    this.muxDragState = null;

    // Send resize messages for all panes after drag completes
    this.sendPaneResizes();
  };

  /** Toggle zoom on the active pane. */
  private toggleMuxZoom(): void {
    if (!this.muxLayoutRoot || !this.muxPaneContainer || !this.muxActivePaneId) return;

    if (this.muxPreZoomLayout) {
      // Unzoom: restore saved layout
      this.muxLayoutRoot = this.muxPreZoomLayout;
      this.muxPreZoomLayout = null;

      // Show all pane canvases
      for (const [, pane] of this.muxPaneCanvases) {
        pane.container.style.display = "";
      }
    } else {
      // Zoom: save current layout, show only active pane
      this.muxPreZoomLayout = this.muxLayoutRoot;
      this.muxLayoutRoot = { type: "leaf", paneId: this.muxActivePaneId };

      // Hide non-active pane canvases
      for (const [paneId, pane] of this.muxPaneCanvases) {
        pane.container.style.display = paneId === this.muxActivePaneId ? "" : "none";
      }
    }

    this.applyMuxLayout();
    this.sendPaneResizes();
    console.info(`[INFO][FRONTEND] Mux zoom: ${this.muxPreZoomLayout ? "zoomed" : "restored"}`);
  }

  /** Render PTY output for a specific pane in multi-pane mode. */
  private renderMuxPaneOutput(paneId: number, data: Uint8Array): void {
    const pane = this.muxPaneCanvases.get(paneId);
    if (!pane) return;

    // Process data through the pane's WASM grid
    pane.grid.core.process_pty_data(data);

    // Render using the pane's own TerminalState and renderer
    pane.renderer.forceRender(pane.state);
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

    // Clean up mux mode
    this.exitMuxMode();

    // Clean up handlers
    this.linkHandler?.dispose();
    this.linkHandler = null;
    this.keyboardHandler?.detach();
    this.mouseHandler?.detach();
    this.imeHandler?.dispose();
    this.selectionController?.dispose();
    this.searchHandler?.dispose();
    this.searchHandler = null;

    // Clean up download handler
    this.downloadManager?.dispose();
    this.downloadManager = null;

    // Clean up SFTP handlers
    this.fileDropHandler?.detach();
    this.fileDropHandler = null;
    this._uploadManager?.dispose();
    this._uploadManager = null;

    // Clean up image handler (ImageViewer, event listener, queues)
    this.imageHandler?.dispose();
    this.imageHandler = null;

    // Clean up PTY
    if (this.ptyClient) {
      this.ptyClient.dispose();
      this.ptyClient.kill().catch(console.error);
      this.ptyClient = null;
    }

    // Dispose WASM resources and callbacks
    this.state?.dispose();

    // Clear references
    this.state = null;
    this.renderer = null;
    this.selectionController = null;
    this.sessionExitCallback = null;
    this.titleChangeCallback = null;
    this.bellActivityCallback = null;
    this.outputActivityCallback = null;
    this.onMuxStateChange = null;

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
   * Gets the selection controller
   */
  get selection(): SelectionController | null {
    return this.selectionController;
  }

  /**
   * Gets the terminal root element
   */
  get root(): HTMLElement | null {
    return this.terminalRoot;
  }

  /**
   * Gets the character cell dimensions
   */
  get cellSize(): CharSize {
    return this.charSize;
  }

  /**
   * Gets the upload manager (for tab close guard checks)
   */
  get uploadManager(): UploadManager | null {
    return this._uploadManager;
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

    // Font changes affect character dimensions - recalculate terminal size
    if (
      (setting === "fontSize" || setting === "fontFamily") &&
      this.renderer &&
      this.state
    ) {
      this.handleCharSizeChange();
    }
  }

  /**
   * Recalculate terminal size after character dimensions change (e.g. font change).
   * Delegates to resize-handler module.
   */
  private handleCharSizeChange(): void {
    const newCharSize = handleCharSizeChange(this.getResizeHandlerContext());
    if (newCharSize) {
      this.charSize = newCharSize;
      this.setupResizeObserver();
    }
  }
}

// Re-export types
export * from "./types";
export * from "./config";
