/**
 * Terminal application main class
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  calculateTerminalSize,
  measureCharacterSize,
  PtyClient,
  type VisibilityController,
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
import { applyInitialCachedSettings } from "./initial-settings";
import type { FileDropHandler } from "../sftp/file-drop-handler";
import type { UploadManager } from "../sftp/upload-manager";
import type { DownloadSessionManager } from "../download";
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
  getMuxSwitchCountWithin,
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
import {
  setupPtyHandlers,
  type PtyHandlerHandle,
} from "./pty-handler";
import { processPendingOscQueue, type OscHandlerContext } from "./osc-handler";
import { ScrollbackBudgetEnforcer, type ScrollbackPane } from "./mux-scrollback-budget";
import { setupResizeObserver, handleCharSizeChange, type ResizeHandlerContext } from "./resize-handler";
import { handleBell, handleWheel, handleMiddleClickPaste } from "./ui-handler";
import { wireInputEvents } from "./input-wiring";
import { setupOverlayBindings } from "./overlay-setup";
import { setupSftpFileDrop } from "./sftp-setup";
import { buildVisibilityController } from "./visibility-setup";
import { registerBackgroundNotificationListener } from "../terminal/background-notification-listener";
import { DiagnosticsController } from "./diagnostics";
import { registerCoreCallbacks as registerCoreCallbacksImpl } from "./core-callbacks";
import {
  onWasmRecovered as onWasmRecoveredImpl,
  type RecoveryHookContext,
} from "./recovery-hook";
import {
  buildMuxSessionContext,
  buildMuxWindowManagerContext,
  buildMuxActionContext,
  type MuxStateAccess,
} from "./mux-state";

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
  private visibilityController: VisibilityController | null = null;
  /** Unlisten for the backend `osc_notification` (hidden-window) event. */
  private backgroundNotificationUnlisten: (() => void) | null = null;
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
   * Cross-pane global scrollback budget enforcer (FR4). Gated on a coarse
   * growth cadence so the PTY hot path is untouched (NFR2). Tracks the
   * previous aggregate scrollback length to feed the growth counter.
   */
  private scrollbackBudgetEnforcer = new ScrollbackBudgetEnforcer();
  private lastAggregateScrollbackLength = 0;
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

  /** Owns the diagnostic heartbeat (`[DIAG-PTY-HEALTH]`) and event-loop
   *  watchdog (`[DIAG-EVENTLOOP] hang ...`) timers. See `diagnostics.ts`
   *  for the rationale and the exact signals each timer surfaces. */
  private diagnostics: DiagnosticsController | null = null;

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
    applyInitialCachedSettings(this.state, this.renderer);

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

    // Visibility-aware streaming controller (FR1, FR2, FR5, NFR5).
    // Watches document.visibilityState + Tauri focus, debounces hide,
    // forwards confirmed transitions to backend / mux daemon.
    this.visibilityController = buildVisibilityController({
      getPtyClient: () => this.ptyClient,
      getMuxClient: () => this.muxClient,
    });

    // Background OSC 9 notification listener (FR1): the backend reader emits
    // `osc_notification` when an OSC 9 desktop notification is recognized
    // while the window is hidden. Fire the OS notification via the shared
    // permission-gated sink. Fire-and-forget; not part of resume replay.
    // Scope to this tab's PTY session: app.emit broadcasts to every tab's
    // listener, so without this predicate one OSC 9 would fire one OS
    // notification per open tab. `undefined` keeps the default sink.
    registerBackgroundNotificationListener(
      listen,
      undefined,
      (sessionId) => sessionId === this.ptyClient?.getSessionId(),
    )
      .then((unlisten) => {
        this.backgroundNotificationUnlisten = unlisten;
      })
      .catch((err) => {
        console.warn(
          "[WARN][FRONTEND] registerBackgroundNotificationListener failed:",
          err,
        );
      });

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

    // Wire pointer events (middle-click paste, contextmenu, wheel, click).
    // Registered BEFORE `mouseHandler.attach()` so the middle-click
    // suppressor's stopImmediatePropagation runs before PTY mouse tracking.
    wireInputEvents({
      container: this.container,
      terminalContainer,
      getSelectionController: () => this.selectionController,
      getLinkHandler: () => this.linkHandler,
      getFoldHandler: () => this.foldHandler,
      app: this,
      onMiddleClickPaste: () => { this.handleMiddleClickPaste(); },
      onWheel: (e) => this.handleWheel(e),
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

    // Create fold handler (referenced by the click listener above via getter).
    this.foldHandler = new FoldHandler({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getTerminalRoot: () => this.terminalRoot,
      getCharSize: () => this.charSize,
    });

    // Create link handler for URL/file path detection and hover cursor
    // (also referenced by the click listener above via getter).
    this.linkHandler = new LinkHandler({
      getState: () => this.state,
      getRenderer: () => this.renderer,
      getTerminalRoot: () => this.terminalRoot,
      getCharSize: () => this.charSize,
    });
    this.linkHandler.attach(terminalContainer);

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
    const sftpSetup = await setupSftpFileDrop({
      container: this.container,
      isActiveTab: () => this.isThisTabActive(),
      getSshConnectionName: () => this.options.sshConnectionName || "",
      getState: () => this.state,
      getPtyClient: () => this.ptyClient,
    });
    this._uploadManager = sftpSetup.uploadManager;
    this.fileDropHandler = sftpSetup.fileDropHandler;

    // Wire markdown / data viewer / download managers into the overlay
    // root, including the IME blur/focus pairing for their fullscreen views.
    const overlaySetup = setupOverlayBindings({
      state: this.state,
      overlayRoot: this.overlayRoot!,
      getPtyClient: () => this.ptyClient,
      getImeHandler: () => this.imeHandler,
    });
    this.downloadManager = overlaySetup.downloadManager;

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
      const cachedSettings = SettingsService.getCached();
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

    this.diagnostics = new DiagnosticsController({
      getRenderer: () => this.renderer,
      getPtyClient: () => this.ptyClient,
      getPtyHandlerHandle: () => this.ptyHandlerHandle,
      getInMuxMode: () => this.inMuxMode,
      getMuxWindowsLength: () => this.muxWindows.length,
      getActiveMuxWindowIndex: () => this.activeMuxWindowIndex,
      getMuxPaneIds: () => this.muxPaneIds,
    });
    this.diagnostics.start();
  }

  /**
   * Register WASM callbacks on a TerminalCore instance.
   * Called once for primary core and again when alternate core becomes active.
   */
  private registerCoreCallbacks(core: ReturnType<TerminalState["getActiveCore"]>): void {
    registerCoreCallbacksImpl(core, {
      getState: () => this.state,
      getPtyClient: () => this.ptyClient,
      getImageHandler: () => this.imageHandler,
      enqueueOsc: (actionType, data) => this.pendingOscQueue.push({ actionType, data }),
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
      enforceScrollbackBudget: () => this.enforceScrollbackBudget(),
    });

    // Route renderer-side WASM crashes (render, renderImmediate, cursor blink)
    // into the shared recovery entry point so they do not get silently swallowed.
    this.renderer?.setWasmRecoveryCallback((error) =>
      this.ptyHandlerHandle?.tryRecoverFromWasmCrash(error) ?? false,
    );
  }

  /**
   * Collect the live, scrollback-bearing WASM grids across all panes. Only the
   * primary grid of each pane retains scrollback (the alternate buffer never
   * does). The active pane's primary grid plus every mux pane's primary grid
   * are gathered and de-duplicated (the active pane is also present in
   * muxPaneGrids while in mux mode). Detached panes are frozen serialized
   * snapshots that do not grow, so they are excluded.
   */
  private collectLiveScrollbackPanes(): ScrollbackPane[] {
    const seen = new Set<WasmGrid>();
    const panes: ScrollbackPane[] = [];
    const add = (grid: WasmGrid | null | undefined) => {
      if (grid && !seen.has(grid)) {
        seen.add(grid);
        panes.push(grid);
      }
    };
    add(this.state?.getPrimaryWasmGrid() ?? null);
    for (const paneState of this.muxPaneGrids.values()) {
      add(paneState.primaryGrid);
    }
    return panes;
  }

  /**
   * Coarse-cadence cross-pane scrollback budget enforcement (FR4 / NFR2).
   * Invoked once per processPendingData drain cycle — NOT per PTY byte. Feeds
   * the aggregate scrollback growth into the enforcer's growth counter and only
   * runs the full scan + eviction when enough new scrollback has accumulated.
   */
  private enforceScrollbackBudget(): void {
    const panes = this.collectLiveScrollbackPanes();
    let aggregate = 0;
    for (const pane of panes) aggregate += pane.getScrollbackLength();

    const growth = aggregate - this.lastAggregateScrollbackLength;
    this.lastAggregateScrollbackLength = aggregate;

    // Only feed positive growth into the coarse cadence; shrinkage (eviction,
    // clear) does not warrant a check.
    const ready =
      growth > 0
        ? this.scrollbackBudgetEnforcer.noteScrollbackGrowth(growth)
        : this.scrollbackBudgetEnforcer.shouldEnforce();
    if (!ready) return;

    const evicted = this.scrollbackBudgetEnforcer.enforce(panes);
    if (evicted > 0) {
      // Re-baseline so the freed lines are not re-counted as growth next cycle.
      this.lastAggregateScrollbackLength = Math.max(0, aggregate - evicted);
      console.warn(
        `[WARN][FRONTEND] scrollback budget enforced — evicted ${evicted} line(s) across ${panes.length} pane(s)`,
      );
    }
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
      // Defense: if the container is hidden but we're being told this tab is
      // active, the tab manager's display orchestration got out of sync (seen
      // after multi-hour-background → visibility-recover paths where the
      // foreground tab came up with `_tabActive=true` AND `display=none`,
      // leaving the canvas invisible until manual restart). Force display back
      // to "" so the canvas can render. Logged so the underlying ordering bug
      // remains observable rather than silently masked.
      if (this.container.style.display === "none") {
        console.warn(
          `[WARN][FRONTEND] setTabActive(true) recovered hidden container: ` +
            `containerId=${this.container.id || "(unset)"} (forced display="")`,
        );
        this.container.style.display = "";
      }
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

  /** Build the shared mux-state access used by all three mux context builders. */
  private getMuxStateAccess(): MuxStateAccess {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getState: () => self.state,
      getRenderer: () => self.renderer,
      getPtyClient: () => self.ptyClient,
      getKeyboardHandler: () => self.keyboardHandler,
      getImageHandler: () => self.imageHandler,
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
      getMuxDetachedGrids: () => self.muxDetachedGrids,
      getMuxLastActiveIndex: () => self.muxLastActiveIndex,
      setMuxLastActiveIndex: (index) => { self.muxLastActiveIndex = index; },
      setMuxReattachWindows: (windows) => { self.muxReattachWindows = windows; },
      getMuxReattachWindows: () => self.muxReattachWindows,

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

      onStatusUpdate: (msg) => self.muxStatusUpdateCallback?.(msg),

      registerCoreCallbacks: (core) => self.registerCoreCallbacks(core),
      handleMuxPaneCreated: (paneId) => self.handleMuxPaneCreated(paneId),
      handleMuxPaneExited: (paneId) => self.handleMuxPaneExited(paneId),
      handleRemoteSwitchWindow: (paneId) => self.handleRemoteSwitchWindow(paneId),
      handleMuxAction: (action) => self.handleMuxAction(action),
      sendMuxControl: (msgType, paneId, payload) => self.sendMuxControl(msgType, paneId, payload),
      getActiveMuxPaneId: () => self.getActiveMuxPaneId(),
      emitMuxStateChange: () => self.emitMuxStateChange(),
      switchMuxWindow: (previousIndex?) => self.switchMuxWindow(previousIndex),
      exitMuxMode: () => self.exitMuxMode(),
      enterMuxMode: (socketPath, sessionId, options) =>
        self.enterMuxMode(socketPath, sessionId, options),
      onMuxModeExited: () => self.registerEarlyApcContext(),
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
      getOnMuxStateChange: () => self.onMuxStateChange,
    };
  }

  private getMuxSessionContext(): MuxSessionContext {
    return buildMuxSessionContext(this.getMuxStateAccess());
  }

  private getMuxWindowManagerContext(): MuxWindowManagerContext {
    return buildMuxWindowManagerContext(this.getMuxStateAccess());
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
   * Post-recovery hook: restore visible content + probe mux IPC liveness.
   * Implementation lives in `recovery-hook.ts`; this method just wires
   * the host's mutable state into a context object.
   */
  private onWasmRecovered(viaReinit: boolean): void {
    onWasmRecoveredImpl(this.getRecoveryHookContext(), viaReinit);
  }

  /** Build the context object for recovery-hook functions. */
  private getRecoveryHookContext(): RecoveryHookContext {
    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const self = this;
    return {
      getState: () => self.state,
      getInMuxMode: () => self.inMuxMode,
      getMuxClient: () => self.muxClient,
      getActiveMuxPaneId: () => self.getActiveMuxPaneId(),
      getActiveMuxWindowIndex: () => self.activeMuxWindowIndex,
      getMuxPaneIds: () => self.muxPaneIds,
      getMuxPaneGrids: () => self.muxPaneGrids,
      getMuxDetachedGrids: () => self.muxDetachedGrids,
      getSnapshotWaitPaneId: () => self.snapshotWaitPaneId,
      setSnapshotWaitPaneId: (paneId) => { self.snapshotWaitPaneId = paneId; },
      getSnapshotWaitSetAt: () => self.snapshotWaitSetAt,
      setSnapshotWaitSetAt: (perfNow) => { self.snapshotWaitSetAt = perfNow; },
      getPostRecoveryWatchUntil: () => self.postRecoveryWatchUntil,
      setPostRecoveryWatchUntil: (deadlineMs) => { self.postRecoveryWatchUntil = deadlineMs; },
      resetPostRecoveryCounters: () => {
        self.postRecoveryPtyOutputChunks = 0;
        self.postRecoveryPtyOutputBytes = 0;
      },
      getPostRecoveryPtyOutputChunks: () => self.postRecoveryPtyOutputChunks,
      getPostRecoveryPtyOutputBytes: () => self.postRecoveryPtyOutputBytes,
      getMuxStatusUpdateCallback: () => self.muxStatusUpdateCallback,
      setMuxStatusUpdateCallback: (cb) => { self.muxStatusUpdateCallback = cb; },
      exitMuxMode: () => self.exitMuxMode(),
    };
  }

  private getMuxActionContext(): MuxActionContext {
    return buildMuxActionContext(this.getMuxStateAccess());
  }

  /**
   * Cleans up resources and event listeners
   */
  dispose(): void {
    this.diagnostics?.stop();
    this.diagnostics = null;

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

    // Stop visibility controller before PTY teardown so its final
    // setVisibility callbacks don't race with `dispose`.
    this.visibilityController?.stop();
    this.visibilityController = null;

    // Remove the backend `osc_notification` listener.
    try {
      this.backgroundNotificationUnlisten?.();
    } catch {
      /* ignore */
    }
    this.backgroundNotificationUnlisten = null;

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
