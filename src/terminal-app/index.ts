/**
 * Terminal application main class
 */

import { invoke } from "@tauri-apps/api/core";
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
import type { MuxClient } from "../terminal/mux/mux-client";
import type { MuxAction } from "../terminal/mux/prefix-key";
import {
  CopyModeManager,
  ViKeybinds,
  EmacsKeybinds,
  type CopyModeSelection,
} from "../terminal/mux-copy-mode";
import {
  enterCopyMode as enterCopyModeImpl,
  exitCopyMode as exitCopyModeImpl,
  handleCopyModeKey as handleCopyModeKeyImpl,
  copySelectionToClipboard as copySelectionToClipboardImpl,
  pasteFromClipboard as pasteFromClipboardImpl,
  type MuxCopyModeContext,
} from "./mux/mux-copy-mode";
import {
  handleMuxSplitPaneCreated as handleMuxSplitPaneCreatedImpl,
  initMultiPaneMode as initMultiPaneModeImpl,
  createPaneCanvas as createPaneCanvasImpl,
  setActiveMuxPane as setActiveMuxPaneImpl,
  applyMuxLayout as applyMuxLayoutImpl,
  sendPaneResizes as sendPaneResizesImpl,
  removeMuxPane as removeMuxPaneImpl,
  exitMultiPaneMode as exitMultiPaneModeImpl,
  type MuxMultiPaneContext,
} from "./mux/mux-multi-pane";
import {
  clearMuxScreen as clearMuxScreenImpl,
  createFreshMuxGrid as createFreshMuxGridImpl,
  switchMuxWindow as switchMuxWindowImpl,
  handleMuxPaneCreated as handleMuxPaneCreatedImpl,
  sendMuxPaneResize as sendMuxPaneResizeImpl,
  handleMuxPaneExited as handleMuxPaneExitedImpl,
  emitMuxStateChange as emitMuxStateChangeImpl,
  reloadMuxSettings as reloadMuxSettingsImpl,
  startMuxDirect as startMuxDirectImpl,
  handleRemoteSwitchWindow as handleRemoteSwitchWindowImpl,
  type MuxWindowManagerContext,
} from "./mux/mux-window-manager";
import {
  initMuxDragResize as initMuxDragResizeImpl,
  toggleMuxZoom as toggleMuxZoomImpl,
  type MuxDragResizeContext,
  type MuxDragState,
} from "./mux/mux-drag-resize";
import {
  handleMuxAction as handleMuxActionImpl,
  sendMuxControl as sendMuxControlImpl,
  getActiveMuxPaneId as getActiveMuxPaneIdImpl,
  type MuxActionContext,
} from "./mux/mux-action-handler";
import {
  enterMuxMode as enterMuxModeImpl,
  exitMuxMode as exitMuxModeImpl,
  type MuxSessionContext,
  type EnterMuxOptions,
} from "./mux/mux-session";
import {
  type LayoutNode,
  type SplitDirection,
} from "../terminal/mux/layout";
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
  /** Callback for status bar OSC commands (set from main.ts) */
  public statusBarOscCallback: ((command: string, param1?: string, param2?: string) => void) | null = null;
  /** Callback for mux status updates (set from main.ts, routes to OSC layer) */
  public muxStatusUpdateCallback: ((msg: { left: string; right: string }) => void) | null = null;
  private muxClient: MuxClient | null = null;
  private inMuxMode = false;
  private muxWindows: { id: number; name: string }[] = [];
  private activeMuxWindowIndex = 0;
  private muxPaneIds: number[] = []; // Actual pane IDs from daemon
  private muxPendingWindowCount = 0; // Windows waiting for PaneCreated response
  private muxIsReattaching = false; // True during reattach (receiving existing panes)
  private muxPaneGrids: Map<number, import("../terminal/state").MuxPaneGridState> = new Map(); // Full pane state per pane
  private muxOriginalGrid: WasmGrid | null = null; // Original grid saved before mux mode
  private muxDetachedGrids: Map<string, Uint8Array> = new Map(); // Saved snapshots across detach/reattach (keyed by socket+session)
  private copyModeManager: CopyModeManager | null = null;
  private copyModeKeybinds: ViKeybinds | EmacsKeybinds | null = null;
  private copyModeIndicator: HTMLElement | null = null;

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
  private muxLastActiveIndex = 0;
  private muxDragState: MuxDragState | null = null;
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
  private _tabActive = true;

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

    // NOTE: registerEarlyApcContext() is called after imageHandler.init() below,
    // because it needs imageHandler to store the per-tab mux context.

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
      debugId: this.container.id || `tab-${Date.now()}`,
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
        // Clear selection on middle click
        this.selectionController?.clearSelection();

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

    // Register early APC context so Welcome from a manually-launched bridge
    // (`emterm mux` typed by user) triggers auto-enter mux mode.
    // Must be after imageHandler creation since context is stored there.
    this.registerEarlyApcContext();

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

    // Wire PTY write callback for markdown navigation (navigate/image/quit commands)
    this.state.getMarkdownManager().setPtyWriteCallback((data: string) => {
      this.ptyClient?.write(new TextEncoder().encode(data));
    });

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
      getMuxApcContext: () => this.imageHandler?.getMuxApcContext() ?? null,
      isTabActive: () => this._tabActive,
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
      statusBarOscCallback: this.statusBarOscCallback,
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
          // Single-pane: broadcast resize to all panes. Sending to only the
          // active pane leaves inactive windows' daemon-side PTYs stale
          // (e.g., if dimensions were initialized before the status bar was
          // restored during reattach), so switching to them reports the wrong
          // `stty size`.
          for (const paneId of this.muxPaneIds) {
            if (paneId != null) this.sendMuxPaneResize(paneId);
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

  /** Mark this tab as active — resumes canvas rendering and repaints. */
  setTabActive(active: boolean): void {
    this._tabActive = active;
    if (active) {
      this.ptyHandlerHandle?.notifyTabActivated();
    }
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
    // Update selection overlay position after scroll
    this.selectionController?.notifyScroll();
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

  /** Build the context object for mux session management functions. */
  private getMuxSessionContext(): MuxSessionContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getState: () => self.state,
      getRenderer: () => self.renderer,
      getPtyClient: () => self.ptyClient,
      getKeyboardHandler: () => self.keyboardHandler,
      getPtyHandlerHandle: () => self.ptyHandlerHandle,
      getInMuxMode: () => self.inMuxMode,
      setInMuxMode: (value) => { self.inMuxMode = value; },
      getMuxClient: () => self.muxClient,
      setMuxClient: (client) => { self.muxClient = client; },
      getMuxWindows: () => self.muxWindows,
      setMuxWindows: (windows) => { self.muxWindows = windows; },
      getActiveMuxWindowIndex: () => self.activeMuxWindowIndex,
      setActiveMuxWindowIndex: (index) => { self.activeMuxWindowIndex = index; },
      getMuxPaneIds: () => self.muxPaneIds,
      setMuxPaneIds: (ids) => { self.muxPaneIds = ids; },
      getMuxPendingWindowCount: () => self.muxPendingWindowCount,
      setMuxPendingWindowCount: (count) => { self.muxPendingWindowCount = count; },
      getMuxIsReattaching: () => self.muxIsReattaching,
      setMuxIsReattaching: (value) => { self.muxIsReattaching = value; },
      getMuxOriginalGrid: () => self.muxOriginalGrid,
      setMuxOriginalGrid: (grid) => { self.muxOriginalGrid = grid; },
      getMuxPaneGrids: () => self.muxPaneGrids,
      getMuxLayoutRoot: () => self.muxLayoutRoot,
      getMuxPaneCanvases: () => self.muxPaneCanvases,
      getMuxPendingSplitCount: () => self.muxPendingSplitCount,
      setMuxPendingSplitCount: (count) => { self.muxPendingSplitCount = count; },
      getMuxLastActiveIndex: () => self.muxLastActiveIndex,
      setMuxLastActiveIndex: (index) => { self.muxLastActiveIndex = index; },
      getCopyModeManager: () => self.copyModeManager,
      setCopyModeManager: (manager) => { self.copyModeManager = manager; },
      getCopyModeKeybinds: () => self.copyModeKeybinds,
      setCopyModeKeybinds: (keybinds) => { self.copyModeKeybinds = keybinds; },
      setMuxApcContext: (ctx) => self.imageHandler?.setMuxApcContext(ctx),
      registerCoreCallbacks: (core) => self.registerCoreCallbacks(core),
      handleMuxPaneCreated: (paneId) => self.handleMuxPaneCreated(paneId),
      handleMuxPaneExited: (paneId) => self.handleMuxPaneExited(paneId),
      handleRemoteSwitchWindow: (paneId) => self.handleRemoteSwitchWindow(paneId),
      handleMuxAction: (action) => self.handleMuxAction(action),
      sendMuxControl: (msgType, paneId, payload) => self.sendMuxControl(msgType, paneId, payload),
      renderMuxPaneOutput: (paneId, data) => self.renderMuxPaneOutput(paneId, data),
      getActiveMuxPaneId: () => self.getActiveMuxPaneId(),
      emitMuxStateChange: () => self.emitMuxStateChange(),
      exitMultiPaneMode: (remainingPaneId) => self.exitMultiPaneMode(remainingPaneId),
      onMuxModeExited: () => self.registerEarlyApcContext(),
      onStatusUpdate: (msg) => self.muxStatusUpdateCallback?.(msg),
    };
  }

  /** Build the context object for mux window manager functions. */
  private getMuxWindowManagerContext(): MuxWindowManagerContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getState: () => self.state,
      getRenderer: () => self.renderer,
      getMuxClient: () => self.muxClient,
      getKeyboardHandler: () => self.keyboardHandler,
      getInMuxMode: () => self.inMuxMode,
      getMuxWindows: () => self.muxWindows,
      getActiveMuxWindowIndex: () => self.activeMuxWindowIndex,
      setActiveMuxWindowIndex: (index) => { self.activeMuxWindowIndex = index; },
      getMuxPaneIds: () => self.muxPaneIds,
      getMuxPaneGrids: () => self.muxPaneGrids,
      getMuxDetachedGrids: () => self.muxDetachedGrids,
      getMuxPendingWindowCount: () => self.muxPendingWindowCount,
      setMuxPendingWindowCount: (count) => { self.muxPendingWindowCount = count; },
      getMuxIsReattaching: () => self.muxIsReattaching,
      setMuxIsReattaching: (value) => { self.muxIsReattaching = value; },
      getMuxPendingSplitCount: () => self.muxPendingSplitCount,
      setMuxPendingSplitCount: (count) => { self.muxPendingSplitCount = count; },
      getMuxPendingSplitDirection: () => self.muxPendingSplitDirection,
      getMuxLastActiveIndex: () => self.muxLastActiveIndex,
      getMuxLayoutRoot: () => self.muxLayoutRoot,
      getMuxPaneCanvases: () => self.muxPaneCanvases,
      get onMuxStateChange() { return self.onMuxStateChange; },
      flushPtyPendingData: () => { self.ptyHandlerHandle?.flushPendingData(); },
      processPtyPendingDataNow: () => { self.ptyHandlerHandle?.processNow(); },
      registerCoreCallbacks: (core) => self.registerCoreCallbacks(core),
      sendMuxControl: (msgType, paneId, payload) => self.sendMuxControl(msgType, paneId, payload),
      handleMuxSplitPaneCreated: (paneId, direction) => self.handleMuxSplitPaneCreated(paneId, direction),
      removeMuxPane: (paneId) => self.removeMuxPane(paneId),
      exitMuxMode: () => self.exitMuxMode(),
      enterMuxMode: (socketPath, sessionId) => self.enterMuxMode(socketPath, sessionId),
    };
  }

  /** Clear the terminal screen for mux window switching. */
  private clearMuxScreen(): void {
    clearMuxScreenImpl(this.getMuxWindowManagerContext());
  }

  /** Create a fresh WASM grid for a new mux pane and swap it in. */
  private createFreshMuxGrid(): void {
    createFreshMuxGridImpl(this.getMuxWindowManagerContext());
  }

  /** Switch to a specific mux window by index (called from tab bar UI). */
  public switchToMuxWindow(windowIndex: number): void {
    console.warn(`[DIAG-MUX] switchToMuxWindow called: windowIndex=${windowIndex} inMuxMode=${this.inMuxMode} muxWindows.length=${this.muxWindows.length} activeMuxWindowIndex=${this.activeMuxWindowIndex}`);
    if (!this.inMuxMode) { console.warn(`[DIAG-MUX] switchToMuxWindow: BLOCKED — not in mux mode`); return; }
    if (windowIndex < 0 || windowIndex >= this.muxWindows.length) { console.warn(`[DIAG-MUX] switchToMuxWindow: BLOCKED — index out of range`); return; }
    if (windowIndex === this.activeMuxWindowIndex) { console.warn(`[DIAG-MUX] switchToMuxWindow: BLOCKED — already active`); return; }

    const previousIndex = this.activeMuxWindowIndex;
    this.activeMuxWindowIndex = windowIndex;
    console.warn(`[DIAG-MUX] switchToMuxWindow: switching ${previousIndex} → ${windowIndex}, paneIds=[${this.muxPaneIds}]`);
    this.switchMuxWindow(previousIndex);

    // Restore focus after mux window switch (mouse clicks on mux tabs
    // don't trigger tab:activated when the tab is already active).
    // Use rAF to ensure focus is restored after browser's default
    // mousedown focus handling completes.
    this.focus();
    requestAnimationFrame(() => this.focus());
  }

  /** Switch to the current activeMuxWindowIndex: swap WASM grids and update UI. */
  private switchMuxWindow(previousIndex?: number): void {
    switchMuxWindowImpl(this.getMuxWindowManagerContext(), previousIndex);
  }

  /** Handle PaneCreated from daemon — register actual pane ID and update UI. */
  private handleMuxPaneCreated(paneId: number): void {
    handleMuxPaneCreatedImpl(this.getMuxWindowManagerContext(), paneId);
  }

  /** Handle remote SwitchWindow notification from CLI. */
  private handleRemoteSwitchWindow(paneId: number): void {
    handleRemoteSwitchWindowImpl(this.getMuxWindowManagerContext(), paneId);
  }

  /** Send a Resize message to the daemon for a single pane using current terminal dimensions. */
  private sendMuxPaneResize(paneId: number): void {
    sendMuxPaneResizeImpl(this.getMuxWindowManagerContext(), paneId);
  }

  /** Handle a mux pane exiting (shell closed). Remove the window and switch if needed. */
  private handleMuxPaneExited(paneId: number): void {
    handleMuxPaneExitedImpl(this.getMuxWindowManagerContext(), paneId);
  }

  /** Re-apply mux keybind settings (call when settings change at runtime). */
  reloadMuxSettings(): void {
    reloadMuxSettingsImpl(this.getMuxWindowManagerContext());
  }

  /** Notify listeners of mux window state changes. */
  private emitMuxStateChange(): void {
    emitMuxStateChangeImpl(this.getMuxWindowManagerContext());
  }

  /** Start or attach to mux session directly via Tauri command.
   *  Bypasses the CLI → OSC → PTY parser roundtrip for instant response. */
  async startMuxDirect(): Promise<void> {
    await startMuxDirectImpl(this.getMuxWindowManagerContext());
  }

  /** Register early APC context for detecting manually-launched bridge processes. */
  private registerEarlyApcContext(): void {
    this.imageHandler?.setMuxApcContext({
      getMuxClient: () => this.muxClient,
      onWelcomeWithoutClient: (msgType, paneId, data) => {
        this.enterMuxMode("", 0, { welcomeData: { msgType, paneId, data } });
      },
    });
  }

  /** Whether this terminal app is currently in mux mode. */
  get isInMuxMode(): boolean {
    return this.inMuxMode;
  }

  /** Send RequestStatusUpdate to the mux daemon (for tab switch). */
  sendMuxRequestStatusUpdate(): void {
    this.muxClient?.sendRequestStatusUpdate().catch((e) => {
      console.warn("sendMuxRequestStatusUpdate failed:", e);
    });
  }

  /** Enter mux mode -- connect to daemon, enable prefix key, show status bar. */
  async enterMuxMode(socketPath: string, sessionId: number, options?: EnterMuxOptions): Promise<void> {
    await enterMuxModeImpl(this.getMuxSessionContext(), socketPath, sessionId, options);
  }

  /** Exit mux mode -- disconnect, disable prefix key, hide status bar. */
  exitMuxMode(): void {
    exitMuxModeImpl(this.getMuxSessionContext());
    // Restore early APC context so next manual `emterm mux` is detected
    this.registerEarlyApcContext();
  }

  /** Build the context object for mux copy mode functions. */
  private getMuxCopyModeContext(): MuxCopyModeContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      get state() { return self.state; },
      get renderer() { return self.renderer; },
      get ptyClient() { return self.ptyClient; },
      get inMuxMode() { return self.inMuxMode; },
      get copyModeManager() { return self.copyModeManager; },
      set copyModeManager(v) { self.copyModeManager = v; },
      get copyModeKeybinds() { return self.copyModeKeybinds; },
      set copyModeKeybinds(v) { self.copyModeKeybinds = v; },
      onCopyModeIndicatorChange: (active: boolean) => self.handleCopyModeIndicatorChange(active),
    };
  }

  /** Enter mux copy mode with vi or emacs keybindings. */
  private enterCopyMode(): void {
    enterCopyModeImpl(this.getMuxCopyModeContext());
  }

  /** Exit mux copy mode. */
  private exitCopyMode(): void {
    exitCopyModeImpl(this.getMuxCopyModeContext());
  }

  /** Handle keyboard input during copy mode. Returns true if the key was consumed. */
  private handleCopyModeKey(event: KeyboardEvent): boolean {
    return handleCopyModeKeyImpl(this.getMuxCopyModeContext(), event);
  }

  /** Extract text from the terminal grid for the given selection and copy to clipboard. */
  private async copySelectionToClipboard(selection: CopyModeSelection): Promise<void> {
    await copySelectionToClipboardImpl(this.getMuxCopyModeContext(), selection);
  }

  /** Paste clipboard text into the active PTY (mux paste action). */
  private async pasteFromClipboard(): Promise<void> {
    await pasteFromClipboardImpl(this.getMuxCopyModeContext());
  }

  /** Show or hide the copy mode indicator overlay. */
  private handleCopyModeIndicatorChange(active: boolean): void {
    if (active) {
      if (!this.copyModeIndicator) {
        this.copyModeIndicator = document.createElement("div");
        this.copyModeIndicator.className = "copy-mode-indicator";
        this.copyModeIndicator.textContent = "-- COPY --";
        this.terminalRoot?.appendChild(this.copyModeIndicator);
      }
      this.copyModeIndicator.style.display = "";
    } else {
      if (this.copyModeIndicator) {
        this.copyModeIndicator.style.display = "none";
      }
    }
  }

  /** Handle mux action dispatched by PrefixKeyHandler. */
  private handleMuxAction(action: MuxAction): void {
    handleMuxActionImpl(this.getMuxActionContext(), action);
  }

  /** Send a control message to the mux daemon. */
  private sendMuxControl(msgType: number, paneId: number, payload?: Uint8Array): void {
    sendMuxControlImpl(this.getMuxActionContext(), msgType, paneId, payload);
  }

  /** Get the active mux pane ID (multi-pane or single-pane mode). */
  private getActiveMuxPaneId(): number | null {
    return getActiveMuxPaneIdImpl(this.getMuxActionContext());
  }

  /** Build the context object for mux action handler functions. */
  private getMuxActionContext(): MuxActionContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getMuxClient: () => self.muxClient,
      getPtyClient: () => self.ptyClient,
      getMuxWindows: () => self.muxWindows,
      getActiveMuxWindowIndex: () => self.activeMuxWindowIndex,
      setActiveMuxWindowIndex: (index) => { self.activeMuxWindowIndex = index; },
      getMuxPaneIds: () => self.muxPaneIds,
      getMuxPendingWindowCount: () => self.muxPendingWindowCount,
      setMuxPendingWindowCount: (count) => { self.muxPendingWindowCount = count; },
      getMuxPendingSplitCount: () => self.muxPendingSplitCount,
      setMuxPendingSplitCount: (count) => { self.muxPendingSplitCount = count; },
      setMuxPendingSplitDirection: (direction) => { self.muxPendingSplitDirection = direction; },
      getMuxActivePaneId: () => self.muxActivePaneId,
      getMuxLayoutRoot: () => self.muxLayoutRoot,
      switchMuxWindow: (previousIndex?) => self.switchMuxWindow(previousIndex),
      emitMuxStateChange: () => self.emitMuxStateChange(),
      setActiveMuxPane: (paneId) => self.setActiveMuxPane(paneId),
      toggleMuxZoom: () => self.toggleMuxZoom(),
      enterCopyMode: () => self.enterCopyMode(),
      pasteFromClipboard: () => self.pasteFromClipboard(),
      exitMuxMode: () => self.exitMuxMode(),
    };
  }

  /** Build the context object for multi-pane functions. */
  private getMuxMultiPaneContext(): MuxMultiPaneContext {
    return {
      terminalRoot: this.terminalRoot,
      container: this.container,
      state: this.state,
      renderer: this.renderer,
      charSize: this.charSize,
      muxLayoutRoot: this.muxLayoutRoot,
      muxActivePaneId: this.muxActivePaneId,
      muxPaneCanvases: this.muxPaneCanvases,
      muxPaneContainer: this.muxPaneContainer,
      muxPreZoomLayout: this.muxPreZoomLayout,
      getActiveMuxPaneId: () => this.getActiveMuxPaneId(),
      sendMuxControl: (msgType, paneId, payload) => this.sendMuxControl(msgType, paneId, payload),
      registerCoreCallbacks: (core) => this.registerCoreCallbacks(core),
      initMuxDragResize: () => this.initMuxDragResize(),
    };
  }

  /** Sync mutable context fields back after a multi-pane function call. */
  private syncMuxMultiPaneContext(ctx: MuxMultiPaneContext): void {
    this.muxLayoutRoot = ctx.muxLayoutRoot;
    this.muxActivePaneId = ctx.muxActivePaneId;
    this.muxPaneContainer = ctx.muxPaneContainer;
    this.muxPreZoomLayout = ctx.muxPreZoomLayout;
  }

  /** Handle a split pane creation from the daemon. */
  private handleMuxSplitPaneCreated(newPaneId: number, direction: SplitDirection): void {
    const ctx = this.getMuxMultiPaneContext();
    handleMuxSplitPaneCreatedImpl(ctx, newPaneId, direction);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Initialize multi-pane mode from single-pane mode. */
  private initMultiPaneMode(existingPaneId: number): void {
    const ctx = this.getMuxMultiPaneContext();
    initMultiPaneModeImpl(ctx, existingPaneId);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Create a canvas element and renderer for a new pane. */
  private createPaneCanvas(paneId: number): void {
    const ctx = this.getMuxMultiPaneContext();
    createPaneCanvasImpl(ctx, paneId);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Set the active pane and update visual indicators. */
  private setActiveMuxPane(paneId: number): void {
    const ctx = this.getMuxMultiPaneContext();
    setActiveMuxPaneImpl(ctx, paneId);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Apply the current layout tree to position all pane canvases. */
  private applyMuxLayout(): void {
    const ctx = this.getMuxMultiPaneContext();
    applyMuxLayoutImpl(ctx);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Send resize messages to daemon for all panes in the current layout. */
  private sendPaneResizes(): void {
    const ctx = this.getMuxMultiPaneContext();
    sendPaneResizesImpl(ctx);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Remove a pane from the multi-pane layout. */
  private removeMuxPane(paneId: number): void {
    const ctx = this.getMuxMultiPaneContext();
    removeMuxPaneImpl(ctx, paneId);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Exit multi-pane mode, returning to single-canvas rendering. */
  private exitMultiPaneMode(remainingPaneId: number | null): void {
    const ctx = this.getMuxMultiPaneContext();
    exitMultiPaneModeImpl(ctx, remainingPaneId);
    this.syncMuxMultiPaneContext(ctx);
  }

  /** Build the context object for drag-resize functions. */
  private getMuxDragResizeContext(): MuxDragResizeContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getMuxPaneContainer: () => self.muxPaneContainer,
      getMuxLayoutRoot: () => self.muxLayoutRoot,
      setMuxLayoutRoot: (layout) => { self.muxLayoutRoot = layout; },
      getCharSize: () => self.charSize,
      getMuxDragState: () => self.muxDragState,
      setMuxDragState: (state) => { self.muxDragState = state; },
      getMuxActivePaneId: () => self.muxActivePaneId,
      getMuxPaneCanvases: () => self.muxPaneCanvases,
      getMuxPreZoomLayout: () => self.muxPreZoomLayout,
      setMuxPreZoomLayout: (layout) => { self.muxPreZoomLayout = layout; },
      applyMuxLayout: () => self.applyMuxLayout(),
      sendPaneResizes: () => self.sendPaneResizes(),
    };
  }

  /** Initialize drag-resize listeners on the mux pane container. */
  private initMuxDragResize(): void {
    initMuxDragResizeImpl(this.getMuxDragResizeContext());
  }

  /** Toggle zoom on the active pane. */
  private toggleMuxZoom(): void {
    toggleMuxZoomImpl(this.getMuxDragResizeContext());
  }

  /** Render PTY output for a specific pane in multi-pane mode. */
  private renderMuxPaneOutput(paneId: number, data: Uint8Array): void {
    const pane = this.muxPaneCanvases.get(paneId);
    if (!pane) return;

    // Process data through the pane's WASM grid
    const consumed = pane.grid.core.process_pty_data(data);
    if (consumed < data.length) {
      console.warn(`[DIAG-MODE] muxPane=${paneId} consumed=${consumed}/${data.length} (partial - buffer switch or cursor_shown)`);
    }

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
    this.copyModeIndicator?.remove();
    this.copyModeIndicator = null;
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
    this.titleChangeCallback = (title: string) => {
      // In mux mode, also update the active window's name
      if (this.inMuxMode && this.muxWindows.length > 0) {
        const activeWin = this.muxWindows[this.activeMuxWindowIndex];
        if (activeWin && activeWin.name !== title) {
          activeWin.name = title;
          this.emitMuxStateChange();
        }
      }
      callback(title);
    };
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
   * Recheck container size and resize terminal if dimensions changed.
   * Used when external layout changes (e.g. status bar visibility) may have
   * altered the available area without triggering ResizeObserver reliably.
   */
  recheckSize(): void {
    if (!this.state || !this.renderer) return;
    // Skip if container is hidden (inactive tab)
    if (this.container.style.display === "none" ||
        this.container.clientWidth === 0 || this.container.clientHeight === 0) {
      return;
    }

    const { cols, rows } = calculateTerminalSize(
      this.container,
      this.charSize.width,
      this.charSize.height,
    );

    const currentCols = this.state.cols;
    const currentRows = this.state.rows;
    if (cols === currentCols && rows === currentRows) return;

    try {
      this.state.resize(cols, rows);
      this.state.setCellSizePx(
        Math.round(this.charSize.width),
        Math.round(this.charSize.height),
      );
      this.renderer.resize(cols, rows);
      this.renderer.forceRender(this.state);
    } catch (error) {
      console.error("Failed to resize terminal in recheckSize:", error);
      try {
        this.renderer.forceRender(this.state);
      } catch {
        // Recovery failed — nothing more we can do
      }
      return;
    }

    this.imeHandler?.updatePosition();
    this.mouseHandler?.updateCharSize(this.charSize.width, this.charSize.height);
    this.selectionController?.resize(cols, rows, this.charSize.width, this.charSize.height);

    // Resize PTY
    this.ptyClient?.resize(cols, rows);

    // Propagate to mux daemon if in mux mode
    if (this.inMuxMode && this.muxClient) {
      if (this.muxLayoutRoot) {
        this.applyMuxLayout();
        this.sendPaneResizes();
      } else {
        // Broadcast to all panes — see comment in onMuxResize for rationale.
        for (const paneId of this.muxPaneIds) {
          if (paneId != null) this.sendMuxPaneResize(paneId);
        }
      }
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
