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
import { effectiveMiddleClickPaste } from "../settings/effective-settings";
import { buildFontFamilyChain } from "../settings/settings-applier";
import { showTerminalContextMenu } from "../context-menu";
import { FileDropHandler, formatPathsForPaste, extractRemotePath, type FileDropInfo } from "../sftp/file-drop-handler";
import { UploadManager } from "../sftp/upload-manager";
import { DownloadSessionManager } from "../download";
import type { MuxClient } from "../terminal/mux/mux-client";
import type { MuxAction } from "../terminal/mux/prefix-key";
import {
  switchMuxWindow as switchMuxWindowImpl,
  handleMuxPaneCreated as handleMuxPaneCreatedImpl,
  sendMuxPaneResize as sendMuxPaneResizeImpl,
  handleMuxPaneExited as handleMuxPaneExitedImpl,
  emitMuxStateChange as emitMuxStateChangeImpl,
  reloadMuxSettings as reloadMuxSettingsImpl,
  startMuxDirect as startMuxDirectImpl,
  handleRemoteSwitchWindow as handleRemoteSwitchWindowImpl,
  getLastMuxSwitchAt,
  type MuxWindowManagerContext,
} from "./mux/mux-window-manager";
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
import { OscColorHandler } from "../terminal/osc-colors";
import { CursorShapeStack } from "../terminal/osc-cursor-shape";
import { setupPtyHandlers, type PtyHandlerHandle } from "./pty-handler";
import { processPendingOscQueue, type OscHandlerContext } from "./osc-handler";
import { setupResizeObserver, handleCharSizeChange, type ResizeHandlerContext } from "./resize-handler";
import { handleBell, handleWheel, handleMiddleClickPaste } from "./ui-handler";
import { getWasmMemoryBytes } from "../terminal/wasm/loader";


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
  private muxReattachWindows: import("../terminal/mux/mux-client").MuxWindowInfo[] = []; // Window info from Welcome for reattach
  private muxPaneGrids: Map<number, import("../terminal/state").MuxPaneGridState> = new Map(); // Full pane state per pane
  private muxOriginalGrid: WasmGrid | null = null; // Original grid saved before mux mode
  private muxDetachedGrids: Map<string, Uint8Array> = new Map(); // Saved snapshots across detach/reattach (keyed by socket+session)
  /**
   * After a viaReinit WASM recovery completes, this is set to a deadline
   * (Date.now() + N) during which incoming mux traffic is counted for
   * observability. 0 means the watch window is closed; treated as a
   * single-flight gate by `runPostRecoveryIpcHealthCheck`.
   */
  private postRecoveryWatchUntil = 0;
  /** PtyOutput chunks seen during the post-recovery watch window. */
  private postRecoveryPtyOutputChunks = 0;
  /** PtyOutput bytes seen during the post-recovery watch window. */
  private postRecoveryPtyOutputBytes = 0;
  /**
   * Pane id we're awaiting a `RequestPaneSnapshot` reply for after a WASM
   * recovery in mux mode. The PtyOutput callback in mux-session.ts logs the
   * arriving chunk once and clears this so we can confirm the snapshot
   * reply path is alive (or missing).
   */
  private snapshotWaitPaneId: number | null = null;
  /** When `snapshotWaitPaneId` was set (performance.now()), for elapsed-time logging. */
  private snapshotWaitSetAt = 0;
  private muxLastActiveIndex = 0;

  /** Diagnostic heartbeat timer, started at the end of init() and stopped in
   *  dispose(). Logs a single [DIAG-MUX-HEARTBEAT] warn line every 5s with
   *  pane count, active pane id, last switch elapsed, main-thread loop lag
   *  (delay between heartbeat firings minus the 5000 ms expected), max rAF
   *  gap reported by the renderer, and WASM heap size. The lag and rAF gap
   *  values are the most direct signals for "main thread or compositor was
   *  blocked between heartbeats" — the freeze fingerprint. */
  private _heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private _heartbeatLastFiredAt = 0;

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
  /** Cooldown timestamp (Date.now() ms) for `tab-active mismatch` warning. */
  private _tabActiveLogAt = 0;
  /**
   * Strict tab-active check for input handlers. Returns true only when both
   * the explicit `_tabActive` flag (set by the tab:activated event) AND the
   * DOM `display` style agree the tab is visible. Logs when they disagree so
   * the wrong-tab-receives-input regression observed in tmp/crash-recovery.md
   * can be diagnosed if it recurs.
   */
  private isThisTabActive(): boolean {
    const visible = this.container.style.display !== "none";
    const flag = this._tabActive;
    if (visible !== flag) {
      // Sampled log: emit at most every ~5s per tab to avoid log spam from
      // input-handler call sites firing per keystroke.
      const now = Date.now();
      if (now - this._tabActiveLogAt > 5_000) {
        this._tabActiveLogAt = now;
        console.warn(
          `[WARN][FRONTEND] tab-active mismatch: _tabActive=${flag} display=${visible ? "visible" : "none"} containerId=${this.container.id || "(unset)"}`,
        );
      }
    }
    return flag && visible;
  }

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
      isActiveTab: () => this.isThisTabActive(),
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
      isActiveTab: () => this.isThisTabActive(),
      onToggleSearch: () => this.toggleSearch(),
      onRestoreFocus: () => this.imeHandler?.focus(),
      onExitScrollback: () => this.exitScrollback(),
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
        if (effectiveMiddleClickPaste(settings)) {
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
      isActiveTab: () => this.isThisTabActive(),
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

    this.startDiagnosticHeartbeat();
  }

  /** Start the 5s diagnostic heartbeat (see _heartbeatTimer field doc). */
  private startDiagnosticHeartbeat(): void {
    if (this._heartbeatTimer) return;
    this._heartbeatLastFiredAt = performance.now();
    this._heartbeatTimer = setInterval(() => this.fireHeartbeat(), 5000);
  }

  /** Emit one heartbeat warn line. Hot path is intentionally minimal — no
   *  Tauri IPC, no per-pane WASM calls (we only touch the active core).
   *  Lag is the difference between the actual interval and the expected
   *  5000 ms; large positive values mean the timer was held back by a
   *  blocked main thread. */
  private fireHeartbeat(): void {
    try {
      const now = performance.now();
      const lag = Math.round(now - this._heartbeatLastFiredAt - 5000);
      this._heartbeatLastFiredAt = now;

      const panes = this.muxWindows.length;
      const activeIdx = this.activeMuxWindowIndex;
      const activePaneId = this.muxPaneIds[activeIdx] ?? -1;

      const lastSwitchAt = getLastMuxSwitchAt();
      const lastSwitchAgoMs = lastSwitchAt > 0 ? Math.round(now - lastSwitchAt) : -1;

      const rafGap = (this.renderer as unknown as {
        getAndResetMaxRafGap?: () => number;
      })?.getAndResetMaxRafGap?.() ?? -1;

      let wasmHeapMB = -1;
      try {
        const bytes = getWasmMemoryBytes();
        if (bytes >= 0) wasmHeapMB = Math.round(bytes / (1024 * 1024));
      } catch { /* loader not initialized */ }

      // IPC layer observability: chunkRecv* counters tell us whether the
      // backend → frontend Channel listener is firing at all (== distinguish
      // "IPC stuck" from "scheduling stuck"). pending* counters reveal
      // whether listener-delivered chunks are piling up because
      // processPendingData isn't running. lastChunkAgoMs == -1 means no
      // chunk has been received since spawn.
      const recv = this.ptyClient?.getRecvStats();
      const recvCount = recv?.count ?? -1;
      const recvBytes = recv?.bytes ?? -1;
      const lastChunkAgoMs = recv && recv.lastRecvAt > 0
        ? Math.round(now - recv.lastRecvAt)
        : -1;
      const pending = this.ptyHandlerHandle?.getPendingStats();
      const pendingChunks = pending?.chunks ?? -1;
      const pendingBytes = pending?.bytes ?? -1;
      const pendingLeftover = pending?.hasLeftover ?? false;

      console.warn(
        `[DIAG-MUX-HEARTBEAT]` +
        ` mux=${this.inMuxMode}` +
        ` panes=${panes}` +
        ` activeIdx=${activeIdx} activePaneId=${activePaneId}` +
        ` lastSwitchAgoMs=${lastSwitchAgoMs}` +
        ` loopLag=${lag}ms` +
        ` rafMaxGap=${Math.round(rafGap)}ms` +
        ` wasmHeapMB=${wasmHeapMB}` +
        ` chunkRecv=${recvCount}/${recvBytes}b` +
        ` lastChunkAgoMs=${lastChunkAgoMs}` +
        ` pending=${pendingChunks}c/${pendingBytes}b leftover=${pendingLeftover}`,
      );
    } catch (err) {
      console.warn(`[DIAG-MUX-HEARTBEAT] heartbeat threw: ${err instanceof Error ? err.message : String(err)}`);
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
      onRecovered: (viaReinit) => this.onWasmRecovered(viaReinit),
    });

    // Route renderer-side WASM crashes (render, renderImmediate, cursor blink)
    // into the shared recovery entry point so they do not get silently swallowed.
    this.renderer?.setWasmRecoveryCallback((error) =>
      this.ptyHandlerHandle?.tryRecoverFromWasmCrash(error) ?? false,
    );
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
      tryRecoverFromWasmCrash: (error) =>
        this.ptyHandlerHandle?.tryRecoverFromWasmCrash(error) ?? false,
      onMuxResize: (_cols, _rows) => {
        if (!this.inMuxMode || !this.muxClient) return;
        // Broadcast resize to all panes. Sending to only the active pane
        // leaves inactive windows' daemon-side PTYs stale (e.g., if
        // dimensions were initialized before the status bar was restored
        // during reattach), so switching to them reports the wrong
        // `stty size`.
        for (const paneId of this.muxPaneIds) {
          if (paneId != null) this.sendMuxPaneResize(paneId);
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
   * Route an error through the shared WASM crash recovery entry point.
   *
   * Used by callers outside the PTY handler (e.g., the `tab:activated`
   * handler in main.ts) that may catch a `WebAssembly.RuntimeError` from
   * WASM-touching operations such as `forceRender` invoked via
   * `setTabActive(true)`.
   *
   * @returns `true` if the error was a WASM crash and recovery was attempted;
   *   `false` if the error was unrelated or no PTY handler is active.
   */
  tryRecoverFromWasmCrash(error: unknown): boolean {
    return this.ptyHandlerHandle?.tryRecoverFromWasmCrash(error) ?? false;
  }

  /**
   * User-initiated WASM reinitialization. Bypasses the automatic recovery
   * guards so it can rescue a terminal that has already been marked
   * unrecoverable. `onComplete` fires with the final success flag.
   */
  forceReinitWasm(onComplete?: (success: boolean) => void): void {
    const handle = this.ptyHandlerHandle;
    if (!handle) {
      onComplete?.(false);
      return;
    }
    handle.forceReinitWasm(onComplete);
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
      getMuxLastActiveIndex: () => self.muxLastActiveIndex,
      setMuxLastActiveIndex: (index) => { self.muxLastActiveIndex = index; },
      setMuxReattachWindows: (windows) => { self.muxReattachWindows = windows; },
      setMuxApcContext: (ctx) => self.imageHandler?.setMuxApcContext(ctx),
      registerCoreCallbacks: (core) => self.registerCoreCallbacks(core),
      handleMuxPaneCreated: (paneId) => self.handleMuxPaneCreated(paneId),
      handleMuxPaneExited: (paneId) => self.handleMuxPaneExited(paneId),
      handleRemoteSwitchWindow: (paneId) => self.handleRemoteSwitchWindow(paneId),
      handleMuxAction: (action) => self.handleMuxAction(action),
      sendMuxControl: (msgType, paneId, payload) => self.sendMuxControl(msgType, paneId, payload),
      getActiveMuxPaneId: () => self.getActiveMuxPaneId(),
      emitMuxStateChange: () => self.emitMuxStateChange(),
      onMuxModeExited: () => self.registerEarlyApcContext(),
      onStatusUpdate: (msg) => self.muxStatusUpdateCallback?.(msg),
      getPostRecoveryWatchUntil: () => self.postRecoveryWatchUntil,
      countPostRecoveryPtyOutput: (bytes: number) => {
        self.postRecoveryPtyOutputChunks++;
        self.postRecoveryPtyOutputBytes += bytes;
      },
      getSnapshotWaitPaneId: () => self.snapshotWaitPaneId,
      setSnapshotWaitPaneId: (paneId: number | null) => {
        self.snapshotWaitPaneId = paneId;
        self.snapshotWaitSetAt = paneId == null ? 0 : performance.now();
      },
      getSnapshotWaitSetAt: () => self.snapshotWaitSetAt,
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
      getMuxLastActiveIndex: () => self.muxLastActiveIndex,
      getMuxReattachWindows: () => self.muxReattachWindows,
      get onMuxStateChange() { return self.onMuxStateChange; },
      flushPtyPendingData: () => { self.ptyHandlerHandle?.flushPendingData(); },
      processPtyPendingDataNow: () => { self.ptyHandlerHandle?.processNow(); },
      registerCoreCallbacks: (core) => self.registerCoreCallbacks(core),
      sendMuxControl: (msgType, paneId, payload) => self.sendMuxControl(msgType, paneId, payload),
      exitMuxMode: () => self.exitMuxMode(),
      enterMuxMode: (socketPath, sessionId) => self.enterMuxMode(socketPath, sessionId),
      syncWindowTitleFromState: () => {
        // After swapping in a different pane's saved title (or resetting
        // for a new window), force-push the title through the normal
        // update path. Bypasses updateWindowTitle's dedup (since the
        // effective title may equal the previous cached value even though
        // the active pane just changed) and updates the browser window
        // title, parent tab title, and mux sub-tab name.
        const current = self.state?._title ?? "";
        self.lastWindowTitle = current;
        self.titleChangeCallback?.(current || "Terminal");
      },
    };
  }

  /** Switch to a specific mux window by index (called from tab bar UI). */
  public switchToMuxWindow(windowIndex: number): void {
    if (!this.inMuxMode) { console.warn(`[DIAG-MUX] switchToMuxWindow blocked: not in mux mode (idx=${windowIndex})`); return; }
    if (windowIndex < 0 || windowIndex >= this.muxWindows.length) {
      console.warn(`[DIAG-MUX] switchToMuxWindow blocked: idx=${windowIndex} out of range (len=${this.muxWindows.length})`);
      return;
    }
    if (windowIndex === this.activeMuxWindowIndex) return;

    const previousIndex = this.activeMuxWindowIndex;
    this.activeMuxWindowIndex = windowIndex;
    console.warn(`[DIAG-MUX] switchToMuxWindow ${previousIndex}→${windowIndex}`);
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

  /**
   * Post-recovery hook: restore visible content that a fresh empty WASM grid
   * cannot reproduce on its own.
   *
   * - `viaReinit=true`: the WASM module was replaced, so every saved
   *   `MuxPaneGridState` / detached snapshot references dead memory. Drop
   *   them so subsequent `switchMuxWindow` takes the "no saved state"
   *   branch (fresh grid + daemon snapshot) instead of crashing during
   *   `restoreMuxPaneState`.
   * - In any mux mode, ask the daemon to resend the active pane's shadow
   *   screen so the user sees real content instead of a blank buffer.
   * - Non-mux has no backend buffer — the shell redraws on the next keypress.
   */
  private onWasmRecovered(viaReinit: boolean): void {
    const activePaneIdPre = this.inMuxMode ? this.getActiveMuxPaneId() : null;
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered entry | viaReinit=${viaReinit} inMuxMode=${this.inMuxMode} muxClient=${!!this.muxClient} activePaneId=${activePaneIdPre} activeMuxWindowIndex=${this.activeMuxWindowIndex} muxPaneGrids=${this.muxPaneGrids.size} muxDetached=${this.muxDetachedGrids.size}`,
    );
    if (viaReinit) {
      this.muxPaneGrids.clear();
      this.muxDetachedGrids.clear();
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered cleared stale refs | muxPaneGrids=0 muxDetached=0`,
      );
    }
    if (!this.inMuxMode || !this.muxClient) {
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered skip snapshot — inMuxMode=${this.inMuxMode} muxClient=${!!this.muxClient}`,
      );
      return;
    }
    const paneId = this.getActiveMuxPaneId();
    if (paneId == null) {
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered skip snapshot — activePaneId=null activeMuxWindowIndex=${this.activeMuxWindowIndex} muxPaneIds=[${this.muxPaneIds.join(",")}]`,
      );
      return;
    }
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] onWasmRecovered sending RequestPaneSnapshot | paneId=${paneId}`,
    );
    // Arm the snapshot-reply observer so the next PtyOutput chunk for this
    // pane gets logged once. Cleared in mux-session.ts on first match. Use
    // the setter (rather than direct field write) so any future invariants
    // added to setSnapshotWaitPaneId apply here too.
    if (this.snapshotWaitPaneId != null) {
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] previous snapshot wait abandoned | prevPaneId=${this.snapshotWaitPaneId} newPaneId=${paneId} elapsedMs=${(performance.now() - this.snapshotWaitSetAt).toFixed(0)}`,
      );
    }
    this.snapshotWaitPaneId = paneId;
    this.snapshotWaitSetAt = performance.now();
    this.muxClient.sendRequestPaneSnapshot(paneId).then(() => {
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] RequestPaneSnapshot sent | paneId=${paneId}`,
      );
      // After a WASM module reinit, the daemon snapshot reply is the only
      // way to repaint. Run a lightweight IPC health check to detect a dead
      // bridge socket (e.g. from a PC suspend that left the Unix socket in a
      // half-open state) and surface it instead of leaving a blank screen.
      if (viaReinit) {
        this.runPostRecoveryIpcHealthCheck().catch((healthErr: unknown) => {
          console.error(
            `[ERROR][FRONTEND] runPostRecoveryIpcHealthCheck threw: ${
              healthErr instanceof Error ? healthErr.message : String(healthErr)
            }`,
          );
        });
      }
    }).catch((err: unknown) => {
      console.warn(
        `[WARN][FRONTEND] sendRequestPaneSnapshot after WASM recovery failed: ${
          err instanceof Error ? err.message : String(err)
        }`,
      );
      // Disarm the wait observer on send failure so a stray PtyOutput chunk
      // for this pane doesn't get falsely attributed to a snapshot reply
      // that will never arrive.
      if (this.snapshotWaitPaneId === paneId) {
        this.snapshotWaitPaneId = null;
        this.snapshotWaitSetAt = 0;
      }
    });
  }

  /**
   * Post-recovery mux IPC health probe.
   *
   * Runs only after a viaReinit WASM recovery in mux mode. Sends a
   * `RequestStatusUpdate` and waits up to `HEALTH_CHECK_TIMEOUT_MS` for the
   * matching `StatusUpdate` reply. If nothing arrives, the bridge↔daemon
   * socket is presumably dead (typical after a long PC suspend on Linux),
   * so we exit mux mode to expose the host shell prompt — the user can
   * type `emterm mux` to relaunch the bridge and rebuild the connection.
   *
   * Implementation notes:
   * - Uses `Promise.race` between StatusUpdate arrival and timeout so
   *   in-flight replies short-circuit the wait instead of being judged
   *   against a 2 ms-precision setTimeout boundary (this caused a
   *   false-positive exit in production: alive reply arrived 2 ms after
   *   the 3 s timer fired).
   * - After timeout, applies a small grace window for late arrivals that
   *   raced the timer fire — recovers the session without exiting mux.
   * - Also opens an observability window during which incoming mux APC
   *   traffic is logged at warn level.
   */
  private async runPostRecoveryIpcHealthCheck(): Promise<void> {
    if (!this.muxClient || !this.inMuxMode) return;
    // Single-flight: if a check is already in flight, skip. Avoids
    // wrapper-chain corruption from overlapping recoveries.
    if (this.postRecoveryWatchUntil > 0) return;

    const client = this.muxClient;
    const sessionMuxClient = client;
    // 10 s tolerates slow daemon replies after heavy WASM reinit work
    // (snapshot replay, large grid resize) which previously squeezed
    // the alive reply past the old 3 s threshold.
    const HEALTH_CHECK_TIMEOUT_MS = 10_000;
    // Grace window for replies that lost the race against the timeout
    // fire by a handful of ms. Without this, a 2 ms latecomer trips a
    // false exit identical to the original bug.
    const LATE_ARRIVAL_GRACE_MS = 200;
    // Watch window equals the maximum wait — `finally` clears
    // `postRecoveryWatchUntil` immediately when the await chain ends,
    // so any tail beyond timeout+grace is unobservable.
    const WATCH_WINDOW_MS = HEALTH_CHECK_TIMEOUT_MS + LATE_ARRIVAL_GRACE_MS;

    const watchOpenedAt = Date.now();
    this.postRecoveryWatchUntil = watchOpenedAt + WATCH_WINDOW_MS;
    this.postRecoveryPtyOutputChunks = 0;
    this.postRecoveryPtyOutputBytes = 0;
    console.warn(
      `[WARN][FRONTEND] [DIAG-RECOVERY] post-recovery watch opened | windowMs=${WATCH_WINDOW_MS} timeoutMs=${HEALTH_CHECK_TIMEOUT_MS} graceMs=${LATE_ARRIVAL_GRACE_MS}`,
    );

    let statusReceived = false;
    let statusResolve: (() => void) | null = null;
    const statusPromise = new Promise<void>((resolve) => {
      statusResolve = resolve;
    });
    const originalCallback = this.muxStatusUpdateCallback;
    const wrapper = (msg: { left: string; right: string }) => {
      // If a session swap happened mid-flight (exit + re-enter), the new
      // session's StatusUpdate must NOT mark this probe as alive — that
      // would let a half-dead original session escape detection.
      if (this.muxClient !== sessionMuxClient) {
        originalCallback?.(msg);
        return;
      }
      if (!statusReceived) {
        statusReceived = true;
        const elapsedMs = Date.now() - watchOpenedAt;
        console.warn(
          `[WARN][FRONTEND] [DIAG-RECOVERY] mux IPC alive — StatusUpdate received post-recovery | elapsedMs=${elapsedMs}`,
        );
        statusResolve?.();
      }
      originalCallback?.(msg);
    };
    this.muxStatusUpdateCallback = wrapper;

    // Restore the callback only if our wrapper is still the current one —
    // otherwise some other code (concurrent run, exit, external rewire)
    // has taken over the slot and we must not stomp on it.
    const restoreCallback = () => {
      if (this.muxStatusUpdateCallback === wrapper) {
        this.muxStatusUpdateCallback = originalCallback;
      }
    };

    try {
      try {
        await client.sendRequestStatusUpdate();
      } catch (err) {
        console.error(
          `[ERROR][FRONTEND] [DIAG-RECOVERY] post-recovery sendRequestStatusUpdate failed: ${
            err instanceof Error ? err.message : String(err)
          }`,
        );
        return;
      }

      // Race: alive reply short-circuits the timeout. This avoids the
      // 2 ms false-positive class entirely.
      await Promise.race([
        statusPromise,
        new Promise<void>((resolve) =>
          setTimeout(resolve, HEALTH_CHECK_TIMEOUT_MS),
        ),
      ]);

      // Bail out if mux mode was torn down or replaced during the wait —
      // a late StatusUpdate from a stale session must not trigger exit on
      // a freshly-attached session.
      if (!this.inMuxMode || this.muxClient !== sessionMuxClient) return;

      if (statusReceived) return;

      // Timeout fired without a reply. Wait briefly for late arrivals
      // that raced the timer — exiting mux mode is destructive (forces
      // the user to manually `emterm mux`), so a small grace is well
      // worth a 200 ms delay.
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] no StatusUpdate within ${HEALTH_CHECK_TIMEOUT_MS}ms — entering ${LATE_ARRIVAL_GRACE_MS}ms grace window before exiting mux mode`,
      );

      await Promise.race([
        statusPromise,
        new Promise<void>((resolve) =>
          setTimeout(resolve, LATE_ARRIVAL_GRACE_MS),
        ),
      ]);

      if (!this.inMuxMode || this.muxClient !== sessionMuxClient) return;

      if (statusReceived) {
        console.warn(
          `[WARN][FRONTEND] [DIAG-RECOVERY] mux IPC alive (late arrival in grace window) — keeping mux mode`,
        );
        return;
      }

      console.error(
        `[ERROR][FRONTEND] [DIAG-RECOVERY] mux IPC dead — no StatusUpdate within ${HEALTH_CHECK_TIMEOUT_MS}ms + ${LATE_ARRIVAL_GRACE_MS}ms grace after WASM recovery. Exiting mux mode so the user can relaunch the bridge.`,
      );
      this.exitMuxMode();
    } finally {
      restoreCallback();
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] post-recovery watch closed | chunks=${this.postRecoveryPtyOutputChunks} bytes=${this.postRecoveryPtyOutputBytes} statusReceived=${statusReceived}`,
      );
      this.postRecoveryWatchUntil = 0;
    }
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
      switchMuxWindow: (previousIndex?) => self.switchMuxWindow(previousIndex),
      emitMuxStateChange: () => self.emitMuxStateChange(),
      exitMuxMode: () => self.exitMuxMode(),
    };
  }

  /**
   * Cleans up resources and event listeners
   */
  dispose(): void {
    if (this._heartbeatTimer) {
      clearInterval(this._heartbeatTimer);
      this._heartbeatTimer = null;
    }

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

    // Remove PTY-handler-owned document/window listeners (visibilitychange,
    // Tauri focus) before tearing down the PTY client itself.
    this.ptyHandlerHandle?.destroy();
    this.ptyHandlerHandle = null;

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
    this.titleChangeCallback = (title: string) => {
      // In mux mode, local title updates are now handled by daemon-side
      // OSC detection → WindowRenamed notification (no frontend→daemon rename).
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
    const state = this.state;
    const renderer = this.renderer;
    if (!state || !renderer) return;
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

    const currentCols = state.cols;
    const currentRows = state.rows;
    if (cols === currentCols && rows === currentRows) return;

    // Replays the resize pipeline. Reused after WASM recovery so state and
    // renderer dimensions stay in sync if the first attempt failed mid-way.
    const applyResize = (): void => {
      state.resize(cols, rows);
      state.setCellSizePx(
        Math.round(this.charSize.width),
        Math.round(this.charSize.height),
      );
      renderer.resize(cols, rows);
      renderer.forceRender(state);
    };

    try {
      applyResize();
    } catch (error) {
      console.error("Failed to resize terminal in recheckSize:", error);
      // Route into shared WASM recovery so the terminal can self-heal after
      // system suspend/resume instead of leaving the UI permanently stuck.
      const handled = this.ptyHandlerHandle?.tryRecoverFromWasmCrash(error, (success) => {
        if (!success) return;
        // After recovery the fresh state/renderer may not be the same objects
        // we captured above, so re-read via this.* for the retry attempt.
        const recoveredState = this.state;
        const recoveredRenderer = this.renderer;
        if (!recoveredState || !recoveredRenderer) return;
        try {
          recoveredState.resize(cols, rows);
          recoveredState.setCellSizePx(
            Math.round(this.charSize.width),
            Math.round(this.charSize.height),
          );
          recoveredRenderer.resize(cols, rows);
          recoveredRenderer.forceRender(recoveredState);
        } catch (retryError) {
          console.error("[ERROR][FRONTEND] recheckSize retry after WASM recovery failed:", retryError);
        }
      }) ?? false;
      if (!handled) {
        try {
          renderer.forceRender(state);
        } catch {
          // Recovery failed — nothing more we can do
        }
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
      // Broadcast to all panes — see comment in onMuxResize for rationale.
      for (const paneId of this.muxPaneIds) {
        if (paneId != null) this.sendMuxPaneResize(paneId);
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
