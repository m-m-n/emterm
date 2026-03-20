/**
 * Terminal application main class
 */

import {
  calculateTerminalSize,
  measureCharacterSize,
  PtyClient,
} from "../pty";
import { TerminalState } from "../terminal/state";
import { createRendererAsync, type ITerminalRenderer } from "../terminal";
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
import { OscColorHandler } from "../terminal/osc-colors";
import { CursorShapeStack } from "../terminal/osc-cursor-shape";
import { setupPtyHandlers } from "./pty-handler";
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
    await setupPtyHandlers({
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

  /** Enter mux mode -- connect to daemon, enable prefix key, show status bar. */
  async enterMuxMode(socketPath: string, sessionId: number): Promise<void> {
    if (this.inMuxMode) return;
    this.inMuxMode = true;

    console.info(`[INFO][FRONTEND] Entering mux mode: socket=${socketPath}, session=${sessionId}`);

    // Connect to daemon
    try {
      this.muxClient = new MuxClient();
      const sessions = await this.muxClient.connect(socketPath);
      console.info(`[INFO][FRONTEND] Mux connected: ${sessions.length} session(s)`);
    } catch (e) {
      console.error("[ERROR][FRONTEND] Mux connect failed:", e);
      this.inMuxMode = false;
      this.muxClient = null;
      return;
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

    // Disable prefix key handling
    if (this.keyboardHandler) {
      this.keyboardHandler.disableMuxMode();
    }

    // Disconnect
    if (this.muxClient) {
      this.muxClient.disconnect().catch(() => {});
      this.muxClient = null;
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
      case "new-window":
        this.sendMuxControl(MuxMessageType.CreateWindow, 0);
        break;
      case "split-vertical":
        this.sendMuxControl(MuxMessageType.SplitPane, 0, new Uint8Array([0x01])); // 0x01 = vertical
        break;
      case "split-horizontal":
        this.sendMuxControl(MuxMessageType.SplitPane, 0, new Uint8Array([0x00])); // 0x00 = horizontal
        break;
      case "close-pane":
        this.sendMuxControl(MuxMessageType.DestroyPane, 0);
        break;
      case "next-window":
        this.sendMuxControl(MuxMessageType.SwitchWindow, 0, new Uint8Array([0x01])); // next
        break;
      case "prev-window":
        this.sendMuxControl(MuxMessageType.SwitchWindow, 0, new Uint8Array([0x00])); // prev
        break;
      case "rename-window":
        // TODO: prompt for new name
        console.info("[INFO][FRONTEND] Rename window: prompt not yet implemented");
        break;
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
      case "zoom-toggle":
      case "next-pane":
      case "prev-pane":
      case "copy-mode":
      case "paste":
        console.info(`[INFO][FRONTEND] Mux action not yet routed: ${action.type}`);
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
