/**
 * Terminal application main class
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  calculateTerminalSize,
  measureCharacterSize,
  observeContainerResize,
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
import { showPasteDialog, sendTextInChunks } from "../clipboard";
import { handleSemanticPrompt, handleFoldCommand } from "../terminal/handlers/osc_handlers";
import { showTerminalContextMenu } from "../context-menu";
import { FileDropHandler, formatPathsForPaste, extractRemotePath, type FileDropInfo } from "../sftp/file-drop-handler";
import { UploadManager } from "../sftp/upload-manager";
import { DownloadSessionManager } from "../download";
import { OscColorHandler } from "../terminal/osc-colors";
import { indexToRgb, DEFAULT_FOREGROUND, DEFAULT_BACKGROUND } from "../terminal/colors";
import { handleOsc52 } from "../terminal/osc-clipboard";
import { parseOsc9, sendNotification } from "../terminal/osc-notification";
import { parseOsc22, CursorShapeStack } from "../terminal/osc-cursor-shape";
import { parseIterm2Command } from "../terminal/osc-iterm2";

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
      this.imageHandler!.queueApc(data);
    });

    core.set_dcs_callback((data: Uint8Array) => {
      // Queue data - do NOT access core here (recursive borrow error)
      this.imageHandler!.queueDcs(data);
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
   *
   * The onData handler uses a while loop to support buffer switch interruption:
   * when process_pty_data encounters a mode 47/1047/1049 switch, it stops early
   * so the TS side can perform the buffer switch, then the remaining data is
   * routed to the correct (alternate or primary) core.
   */
  private async setupPtyHandlers(): Promise<void> {
    if (!this.ptyClient || !this.state) return;

    // Register callbacks on primary core
    this.registerCoreCallbacks(this.state.getWasmCore());

    // Track which core has callbacks registered
    let registeredCore = this.state.getWasmCore();

    // Buffer for incoming PTY data — processed in rAF with frame budgeting
    // "Video approach": process data within time budget, render at 60fps
    let pendingChunks: Uint8Array[] = [];
    let leftoverData: Uint8Array | null = null;
    let rafScheduled = false;
    let rafWatchdog: ReturnType<typeof setTimeout> | null = null;
    const FRAME_BUDGET_MS = 12; // Leave ~4ms for rendering within 16.67ms frame
    const RAF_WATCHDOG_MS = 500; // Fallback if rAF stops being delivered

    const processPendingData = () => {
      rafScheduled = false;
      if (rafWatchdog !== null) {
        clearTimeout(rafWatchdog);
        rafWatchdog = null;
      }
      if (!this.state || !this.renderer) return;

      try {
        // Take all pending chunks
        const chunks = pendingChunks;
        pendingChunks = [];

        // Include leftover from previous frame
        if (leftoverData) {
          chunks.unshift(leftoverData);
          leftoverData = null;
        }

        if (chunks.length === 0) return;

        // Merge chunks into a single buffer
        let merged: Uint8Array;
        if (chunks.length === 1) {
          merged = chunks[0]!;
        } else {
          let totalLen = 0;
          for (const c of chunks) totalLen += c.length;
          merged = new Uint8Array(totalLen);
          let offset = 0;
          for (const chunk of chunks) {
            merged.set(chunk, offset);
            offset += chunk.length;
          }
        }

        // Process data with frame budget — stop when time is up
        let remaining = merged;
        const deadline = performance.now() + FRAME_BUDGET_MS;
        let processed = false;

        while (remaining.length > 0) {
          const core = this.state.getActiveCore();

          if (core !== registeredCore) {
            this.registerCoreCallbacks(core);
            this.state.setCellSizePx(
              Math.round(this.charSize.width),
              Math.round(this.charSize.height),
            );
            registeredCore = core;
          }

          const consumed = core.process_pty_data(remaining);

          this.processPendingOscQueue();
          this.imageHandler!.processPendingApcQueue();
          this.imageHandler!.processPendingDcsQueue();

          // Debug: track cursor visibility changes
          const prevCursorVisible = this.state.cursorVisible;

          this.state.syncModesFromWasm();

          if (prevCursorVisible !== this.state.cursorVisible) {
            const processedChunk = remaining.subarray(0, consumed);
            // Show the data that caused the change (escape sequences as hex for non-printable)
            const chunkPreview = Array.from(processedChunk.slice(-64)).map(b =>
              b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : `\\x${b.toString(16).padStart(2, "0")}`,
            ).join("");
            console.warn(
              `[DEBUG][CURSOR] cursorVisible changed: ${prevCursorVisible} → ${this.state.cursorVisible}`,
              `| chunk size=${consumed}, tail="${chunkPreview}"`,
              `| isAlt=${this.state.isAlternateBuffer}`,
              `| cursor=(${this.state.cursorCol},${this.state.cursorRow})`,
            );
          }

          const modeActions = core.take_mode_actions();
          if (modeActions.length > 0) {
            const preActionCursorVisible = this.state.cursorVisible;
            let i = 0;
            while (i < modeActions.length) {
              const action = modeActions[i]!;
              if (action === 0xFF || action === 0xFE) {
                const mode = modeActions[i + 1]! | (modeActions[i + 2]! << 8);
                const isSet = action === 0xFF;
                this.state.setDecPrivateMode(mode, isSet);
                i += 3;
              } else {
                this.state.handleModeAction(action);
                i += 1;
              }
            }
            if (preActionCursorVisible !== this.state.cursorVisible) {
              console.warn(
                `[DEBUG][CURSOR] cursorVisible changed by modeAction: ${preActionCursorVisible} → ${this.state.cursorVisible}`,
                `| actions=[${Array.from(modeActions)}]`,
                `| isAlt=${this.state.isAlternateBuffer}`,
              );
            }
          }

          remaining = remaining.subarray(consumed);
          processed = true;

          if (consumed === 0) break;

          // Check frame budget — defer remaining data to next frame
          if (remaining.length > 0 && performance.now() >= deadline) {
            leftoverData = remaining;
            break;
          }
        }

        if (processed) {
          this.outputActivityCallback?.();
          // Render immediately in this frame (no extra rAF delay)
          this.renderer.renderImmediate(this.state);
          this.imeHandler?.updatePosition();
        }

        // If there's leftover data, schedule next frame to continue
        if (leftoverData && !rafScheduled) {
          scheduleProcessing();
        }
      } catch (error) {
        console.error("[ERROR][FRONTEND] processPendingData failed:", error);
        leftoverData = null;

        // Detect WASM crash: after "memory access out of bounds", the wasm-bindgen
        // borrow flag stays set, causing "recursive use of an object" on subsequent
        // calls. Recreate the WASM core to recover.
        const isWasmCrash = error instanceof WebAssembly.RuntimeError;
        const msg = error instanceof Error ? error.message : String(error);
        const isBorrowError = msg.includes("recursive use of an object");
        if (isWasmCrash || isBorrowError) {
          console.warn("[WARN][FRONTEND] WASM crash detected — attempting recovery");
          if (this.state?.recreateWasmCore()) {
            // Re-register callbacks on the new core
            const newCore = this.state.getWasmCore();
            this.registerCoreCallbacks(newCore);
            registeredCore = newCore;
            this.state.setCellSizePx(
              Math.round(this.charSize.width),
              Math.round(this.charSize.height),
            );
            // Force re-render to show recovered (blank) terminal
            this.renderer?.forceRender(this.state);
          }
        }
      }
    };

    const scheduleProcessing = () => {
      if (rafScheduled) return;
      rafScheduled = true;
      requestAnimationFrame(processPendingData);
      // Watchdog: fallback if rAF callback is not delivered (e.g. WebKitGTK bug)
      if (rafWatchdog !== null) clearTimeout(rafWatchdog);
      rafWatchdog = setTimeout(() => {
        if (rafScheduled) {
          console.warn(
            "[WARN][FRONTEND] rAF watchdog triggered — forcing data processing",
          );
          processPendingData();
        }
      }, RAF_WATCHDOG_MS);
    };

    // Register binary data handler — just buffer and schedule rAF
    this.ptyClient.onData((data: Uint8Array) => {
      pendingChunks.push(data);
      scheduleProcessing();
    });

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
   * Process all queued OSC events.
   * Safe to call after process_pty_data has returned (borrow released).
   */
  private processPendingOscQueue(): void {
    if (this.pendingOscQueue.length === 0) return;
    const events = this.pendingOscQueue;
    this.pendingOscQueue = [];
    for (const { actionType, data } of events) {
      this.handleOscCallback(actionType, data);
    }
  }

  /**
   * Handle OSC callback from WASM parser.
   * actionType maps to OSC number (0=SetTitleAndIcon, 2=SetTitle, etc.)
   */
  private handleOscCallback(actionType: number, data: string): void {
    if (!this.state) return;

    switch (actionType) {
      case 0: // SetTitleAndIcon
        this.state._title = data;
        this.state._iconName = data;
        this.updateWindowTitle(data);
        break;
      case 1: // SetIconName
        this.state._iconName = data;
        break;
      case 2: // SetTitle
        this.state._title = data;
        this.updateWindowTitle(data);
        break;
      case 4: { // SetColorPalette
        const writeFn = (resp: string) => {
          this.ptyClient?.write(new TextEncoder().encode(resp));
        };
        this.oscColorHandler.handleOsc4(data, writeFn, (index) => {
          return indexToRgb(index);
        });
        // Notify renderer of palette change
        this.renderer?.forceRender(this.state!);
        break;
      }
      case 7: // SetWorkingDirectory
        this.state._workingDirectory = data;
        break;
      case 8: { // Hyperlink
        // data format: "params;uri" (semicolon-separated)
        const sepIdx = data.indexOf(";");
        if (sepIdx >= 0) {
          const params = data.substring(0, sepIdx);
          const uri = data.substring(sepIdx + 1);
          if (uri) {
            this.state._activeHyperlink = { params, uri };
          } else {
            this.state._activeHyperlink = null;
          }
        }
        break;
      }
      case 10: // SetForegroundColor
      case 11: // SetBackgroundColor
      case 12: { // SetCursorColor
        const writeFn = (resp: string) => {
          this.ptyClient?.write(new TextEncoder().encode(resp));
        };
        const lookupThemeDefault = (oscNum: number) => {
          switch (oscNum) {
            case 10: return DEFAULT_FOREGROUND;
            case 11: return DEFAULT_BACKGROUND;
            case 12: return DEFAULT_FOREGROUND; // cursor defaults to foreground
            default: return null;
          }
        };
        this.oscColorHandler.handleOscDefaultColor(actionType, data, writeFn, lookupThemeDefault);
        this.renderer?.forceRender(this.state!);
        break;
      }
      case 104: // ResetColorPalette
        this.oscColorHandler.handleOsc104(data);
        this.renderer?.forceRender(this.state!);
        break;
      case 110: // ResetForegroundColor
        this.oscColorHandler.resetForeground();
        this.renderer?.forceRender(this.state!);
        break;
      case 111: // ResetBackgroundColor
        this.oscColorHandler.resetBackground();
        this.renderer?.forceRender(this.state!);
        break;
      case 112: // ResetCursorColor
        this.oscColorHandler.resetCursorColor();
        this.renderer?.forceRender(this.state!);
        break;
      case 9: { // Notification / Progress (OSC 9)
        const action = parseOsc9(data);
        if (!action) break;
        if (action.type === "notification") {
          sendNotification("eMterm", action.message);
        } else {
          // Progress: update state and notify tab bar
          this.state._progressState = action.state;
          this.state._progressPercentage = action.percentage;
          this.titleChangeCallback?.(this.state.title || "Terminal");
        }
        break;
      }
      case 22: { // Cursor Shape (OSC 22)
        const action = parseOsc22(data);
        if (!action) break;
        const terminalRoot = this.terminalRoot;
        if (action.type === "set") {
          this.cursorShapeStack.set(action.shape);
        } else if (action.type === "push") {
          this.cursorShapeStack.push(action.shape);
        } else {
          this.cursorShapeStack.pop();
        }
        if (terminalRoot) {
          terminalRoot.style.cursor = this.cursorShapeStack.current();
        }
        break;
      }
      case 52: { // Clipboard (OSC 52)
        const settings = SettingsService.getCached();
        const config = {
          readEnabled: settings?.clipboard_read_osc52 ?? true,
          maxSize: settings?.clipboard_max_size_osc52 ?? 10 * 1024 * 1024,
        };
        handleOsc52(
          data,
          config,
          async () => {
            const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
            return await readText();
          },
          async (text: string) => {
            const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
            await writeText(text);
          },
          (resp: string) => {
            this.ptyClient?.write(new TextEncoder().encode(resp));
          },
        );
        break;
      }
      case 100: { // EmtermExtension (OSC 777)
        // data format: "verb;param1;param2;..."
        const parts = data.split(";");
        const verb = parts[0] || "";
        const params = parts.slice(1);
        // Handle fold commands first
        if (verb === "emterm" && params.length > 0 && params[0] === "fold") {
          handleFoldCommand(this.state, params.slice(1));
        } else if (verb === "emterm" && params.length > 0 && params[0] === "download") {
          // Route to download manager
          this.downloadManager?.handleCommand(verb, params);
        } else {
          // Route to markdown manager
          this.state.getMarkdownManager().handleCommand(verb, params);
        }
        break;
      }
      case 133: { // SemanticPrompt
        // data format: "A" or "D;0" (zone_type[;exit_code])
        const parts = data.split(";");
        const zoneType = parts[0] || "";
        const exitCode = parts.length > 1 ? parseInt(parts[1]!, 10) : null;
        handleSemanticPrompt(this.state, zoneType, exitCode);
        break;
      }
      case 101: { // iTerm2 Protocol (OSC 1337)
        const cmd = parseIterm2Command(data);
        if (!cmd) break;
        if (cmd.type === "file") {
          if (cmd.inline && cmd.base64Data) {
            // Inline image display: decode via backend and show
            this.handleIterm2InlineImage(cmd.base64Data, cmd.name);
          } else {
            // Download mode: log for now (download infrastructure is backend-driven)
            console.log(`[LOG][FRONTEND] OSC 1337;File download: ${cmd.name || "unnamed"}`);
          }
        } else if (cmd.type === "set_user_var") {
          this.state._userVariables.set(cmd.key, cmd.value);
        }
        break;
      }
      // Unknown (255) - ignored
    }
  }

  /**
   * Update window title and notify callbacks.
   */
  /**
   * Handle iTerm2 inline image (OSC 1337;File with inline=1).
   * Decodes raw image data via Tauri backend and displays it.
   */
  private async handleIterm2InlineImage(base64Data: string, name: string): Promise<void> {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      // Send raw image data to backend for decoding into RGBA
      const result = await invoke<{ width: number; height: number; rgba_base64: string }>(
        "decode_iterm2_image",
        { base64Data },
      );
      if (result && this.imageHandler) {
        // Create a synthetic DecodedImage and display it
        const image = {
          id: Date.now(), // Use timestamp as unique ID
          width: result.width,
          height: result.height,
          rgba_base64: result.rgba_base64,
        };
        this.imageHandler.showImage(image);
      }
    } catch (error) {
      console.error(`[ERROR][FRONTEND] Failed to decode iTerm2 image "${name}":`, error);
    }
  }

  private updateWindowTitle(title: string): void {
    if (title === this.lastWindowTitle) return;
    this.lastWindowTitle = title;

    const displayTitle = title || "eMterm";
    getCurrentWebviewWindow().setTitle(displayTitle).catch((error) => {
      console.error("Failed to set window title:", error);
    });

    if (this.titleChangeCallback) {
      this.titleChangeCallback(title || "Terminal");
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
            // Update cell size for CSI 14t/16t XTWINOPS responses
            this.state.setCellSizePx(
              Math.round(this.charSize.width),
              Math.round(this.charSize.height),
            );
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
   * Handle middle-click paste from clipboard
   */
  private async handleMiddleClickPaste(): Promise<void> {
    if (!this.selectionController || !this.ptyClient) return;

    try {
      const text = await this.selectionController.paste();
      if (!text) return;

      // Auto-scroll to bottom when user pastes during scrollback
      this.exitScrollback();

      if (this.selectionController.isMultiLinePaste(text)) {
        const lineCount = this.selectionController.countPasteLines(text);
        const result = await showPasteDialog({ text, lineCount });
        if (result.confirmed) {
          await sendTextInChunks(text, (data: Uint8Array) =>
            this.ptyClient!.write(data),
          );
        }
      } else {
        const bytes = new TextEncoder().encode(text);
        await this.ptyClient.write(bytes);
      }
    } catch (error) {
      console.error("Failed to paste from clipboard (middle-click):", error);
    } finally {
      this.imeHandler?.focus();
    }
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
   * Toggle the search bar open/closed.
   */
  toggleSearch(): void {
    this.searchHandler?.toggleSearch();
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
   * Updates charSize, resizes state/renderer/selection/PTY, and reconnects ResizeObserver.
   */
  private handleCharSizeChange(): void {
    if (!this.renderer || !this.state) return;
    // Skip resize if container is hidden (e.g. inactive tab) - dimensions would be 0x0
    if (this.container.style.display === "none") return;

    const newWidth = this.renderer.getCharWidth();
    const newHeight = this.renderer.getCharHeight();

    // Skip if dimensions didn't actually change
    if (newWidth === this.charSize.width && newHeight === this.charSize.height) {
      return;
    }

    this.charSize = { width: newWidth, height: newHeight };

    // Recalculate terminal dimensions with new character size
    const { cols, rows } = calculateTerminalSize(
      this.container,
      newWidth,
      newHeight,
    );

    // Resize state, renderer, and selection
    this.state.resize(cols, rows);
    this.state.setCellSizePx(Math.round(newWidth), Math.round(newHeight));
    this.renderer.resize(cols, rows);
    this.renderer.forceRender(this.state);

    this.mouseHandler?.updateCharSize(newWidth, newHeight);
    this.imeHandler?.updateCharSize(newWidth, newHeight);
    this.selectionController?.resize(cols, rows, newWidth, newHeight);

    // Reconnect ResizeObserver with new character dimensions
    this.disconnectResizeObserver?.();
    this.setupResizeObserver();

    // Resize PTY
    this.ptyClient?.resize(cols, rows);
  }
}

// Re-export types
export * from "./types";
export * from "./config";
