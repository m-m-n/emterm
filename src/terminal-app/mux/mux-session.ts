/**
 * Mux session management functions extracted from TerminalApp.
 * Handles entering and exiting mux mode: daemon connection, PTY routing,
 * grid management, and state lifecycle.
 */

import { MuxClient, MuxMessageType, decodeWelcomeMsg } from "../../terminal/mux/mux-client";
import { muxLog } from "../../terminal/mux/mux-logger";
import type { MuxSessionInfo } from "../../terminal/mux/mux-client";
import type { MuxAction } from "../../terminal/mux/prefix-key";
import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import type { KeyboardHandler } from "../handlers/keyboard";
import type { PtyHandlerHandle } from "../pty-handler";
import type { LayoutNode } from "../../terminal/mux/layout";
import { WasmGrid } from "../../terminal/wasm/terminal-core";
import type { MuxPaneGridState } from "../../terminal/state";
import { SettingsService } from "../../settings/settings-service";
import type { CopyModeManager, ViKeybinds, EmacsKeybinds } from "../../terminal/mux-copy-mode";
import { setMuxApcContext } from "../../terminal/handlers/apc_handlers";

/**
 * Subset of TerminalApp state needed by mux session management functions.
 * Provides access to state via getter/setter proxies, following the same
 * pattern used by other mux modules.
 */
export interface MuxSessionContext {
  // Core state accessors
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getPtyClient: () => PtyClient | null;
  getKeyboardHandler: () => KeyboardHandler | null;
  getPtyHandlerHandle: () => PtyHandlerHandle | null;

  // Mux mode state
  getInMuxMode: () => boolean;
  setInMuxMode: (value: boolean) => void;
  getMuxClient: () => MuxClient | null;
  setMuxClient: (client: MuxClient | null) => void;
  getMuxWindows: () => { id: number; name: string }[];
  setMuxWindows: (windows: { id: number; name: string }[]) => void;
  getActiveMuxWindowIndex: () => number;
  setActiveMuxWindowIndex: (index: number) => void;
  getMuxPaneIds: () => number[];
  setMuxPaneIds: (ids: number[]) => void;
  getMuxPendingWindowCount: () => number;
  setMuxPendingWindowCount: (count: number) => void;
  getMuxIsReattaching: () => boolean;
  setMuxIsReattaching: (value: boolean) => void;
  getMuxOriginalGrid: () => WasmGrid | null;
  setMuxOriginalGrid: (grid: WasmGrid | null) => void;
  getMuxPaneGrids: () => Map<number, MuxPaneGridState>;
  getMuxLayoutRoot: () => LayoutNode | null;
  getMuxPaneCanvases: () => Map<number, unknown>;
  getMuxPendingSplitCount: () => number;
  setMuxPendingSplitCount: (count: number) => void;
  getMuxLastActiveIndex: () => number;
  setMuxLastActiveIndex: (index: number) => void;

  // Copy mode state
  getCopyModeManager: () => CopyModeManager | null;
  setCopyModeManager: (manager: CopyModeManager | null) => void;
  getCopyModeKeybinds: () => ViKeybinds | EmacsKeybinds | null;
  setCopyModeKeybinds: (keybinds: ViKeybinds | EmacsKeybinds | null) => void;

  // Delegate methods that call into other mux modules via TerminalApp wrappers
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  handleMuxPaneCreated: (paneId: number) => void;
  handleMuxPaneExited: (paneId: number) => void;
  handleMuxAction: (action: MuxAction) => void;
  sendMuxControl: (msgType: number, paneId: number, payload?: Uint8Array) => void;
  renderMuxPaneOutput: (paneId: number, data: Uint8Array) => void;
  getActiveMuxPaneId: () => number | null;
  emitMuxStateChange: () => void;
  exitMultiPaneMode: (remainingPaneId: number | null) => void;
}

/**
 * Enter mux mode -- launch bridge process, wait for Welcome APC,
 * enable prefix key, show status bar.
 *
 * The bridge process (`emterm mux`) is launched as a shell command in the PTY.
 * It connects to the daemon, performs handshake, and writes the Welcome APC
 * to stdout (PTY output). The GUI receives it via the WASM APC callback.
 */
/**
 * Options for enterMuxMode.
 * When welcomeData is provided, the bridge is already running and we skip launching it.
 */
export interface EnterMuxOptions {
  /** Pre-received Welcome APC data (bridge already running, user typed `emterm mux` manually). */
  welcomeData?: { msgType: number; paneId: number; data: Uint8Array };
}

export async function enterMuxMode(ctx: MuxSessionContext, _socketPath: string, _sessionId: number, options?: EnterMuxOptions): Promise<void> {
  if (ctx.getInMuxMode()) {
    muxLog.warn("enterMuxMode called but already in mux mode");
    return;
  }
  ctx.setInMuxMode(true);

  muxLog.info(`Entering mux mode via inband protocol (welcomeData=${!!options?.welcomeData})`);

  const ptyClient = ctx.getPtyClient();
  if (!ptyClient) {
    muxLog.error("No PTY client available for mux mode");
    ctx.setInMuxMode(false);
    return;
  }

  // Create MuxClient and register it for APC callbacks
  const client = new MuxClient();
  client.setPtyClient(ptyClient);
  ctx.setMuxClient(client);

  // Register mux APC context so incoming APCs are routed to MuxClient
  setMuxApcContext({
    getMuxClient: () => ctx.getMuxClient(),
  });

  // Set up PTY output handler -- route to correct pane
  client.setOnPtyOutput((paneId: number, data: Uint8Array) => {
    // Multi-pane mode: route to specific pane's canvas/grid
    if (ctx.getMuxLayoutRoot() && (ctx.getMuxPaneCanvases() as Map<number, unknown>).has(paneId)) {
      ctx.renderMuxPaneOutput(paneId, data);
      return;
    }

    // Single-pane mode: route to main renderer
    const activePaneId = ctx.getMuxPaneIds()[ctx.getActiveMuxWindowIndex()];
    if (activePaneId === undefined) {
      // PaneCreated hasn't arrived yet -- accept all output during init
      const handle = ctx.getPtyHandlerHandle();
      if (handle) {
        handle.injectData(data);
      }
      return;
    }
    if (paneId === activePaneId) {
      const handle = ctx.getPtyHandlerHandle();
      if (handle) {
        handle.injectData(data);
      }
    } else {
      // Route to inactive pane's saved state (preserves ring buffer replay data)
      const savedState = ctx.getMuxPaneGrids().get(paneId);
      if (savedState) {
        // Process on the active core (alternate if alternate screen was active)
        const core = savedState.useAlternate && savedState.alternateGrid
          ? savedState.alternateGrid.core
          : savedState.primaryGrid.core;
        core.process_pty_data(data);
      } else {
        // No saved state for this pane -- data dropped
      }
    }
  });

  // Set up PTY exit handler -- remove window when its pane exits
  client.setOnPtyExited((paneId: number) => {
    ctx.handleMuxPaneExited(paneId);
  });

  // Set up pane created handler -- receive actual pane ID from daemon
  client.setOnPaneCreated((paneId: number) => {
    ctx.handleMuxPaneCreated(paneId);
  });

  // Set up detached handler
  client.setOnDetached(() => {
    exitMuxMode(ctx);
  });

  // Suppress original PTY output during mux mode
  const ptyHandlerHandle = ctx.getPtyHandlerHandle();
  if (ptyHandlerHandle) {
    ptyHandlerHandle.suppressOriginalPty = true;
  }

  let muxSessions: MuxSessionInfo[] = [];

  if (options?.welcomeData) {
    // Bridge is already running (user typed `emterm mux` manually).
    // Decode the Welcome data directly without launching bridge.
    const sessions = decodeWelcomeMsg(options.welcomeData.data);
    if (sessions) {
      client.handleWelcome(sessions);
      muxSessions = sessions;
      muxLog.info(`Mux bridge already running: ${muxSessions.length} session(s)`);
    } else {
      muxLog.error("Failed to decode pre-received Welcome");
      if (ptyHandlerHandle) {
        ptyHandlerHandle.suppressOriginalPty = false;
      }
      setMuxApcContext(null);
      ctx.setInMuxMode(false);
      ctx.setMuxClient(null);
      return;
    }
  } else {
    // Start listening for Welcome before launching bridge
    const welcomePromise = client.waitForWelcome();

    // Launch the bridge process by writing the command to the PTY
    const muxCommand = "emterm mux\n";
    await ptyClient.write(new TextEncoder().encode(muxCommand));

    // Wait for Welcome APC from bridge
    try {
      muxSessions = await welcomePromise;
      muxLog.info(`Mux bridge connected: ${muxSessions.length} session(s)`);
    } catch (e) {
      muxLog.error(`Mux bridge handshake failed: ${e}`);
      if (ptyHandlerHandle) {
        ptyHandlerHandle.suppressOriginalPty = false;
      }
      setMuxApcContext(null);
      ctx.setInMuxMode(false);
      ctx.setMuxClient(null);
      return;
    }
  }

  muxLog.info("Setting up PTY write proxy");
  // Route all PTY writes to mux daemon via APC proxy
  ptyClient.setWriteProxy((data: Uint8Array) => {
    const c = ctx.getMuxClient();
    if (!c) return Promise.resolve();
    const activePaneId = ctx.getActiveMuxPaneId() ?? ctx.getMuxPaneIds()[ctx.getActiveMuxWindowIndex()] ?? 1;
    return c.sendInput(activePaneId, data);
  });

  muxLog.info("Saving original grid and creating fresh grid for mux");
  // Save the original grid and create a fresh one for mux mode
  const state = ctx.getState();
  if (state) {
    muxLog.debug("Saving original grid reference");
    ctx.setMuxOriginalGrid(state.getPrimaryGrid());
    muxLog.debug("Getting cols/rows from WASM core");
    const cols = state.getWasmCore().cols();
    const rows = state.getWasmCore().rows();
    muxLog.debug(`Creating fresh WasmGrid (${cols}x${rows})`);
    const freshGrid = new WasmGrid(cols, rows, 10000);
    muxLog.debug("Swapping primary grid");
    state.swapPrimaryGrid(freshGrid);
    muxLog.debug("Registering core callbacks");
    ctx.registerCoreCallbacks(state.getActiveCore());
    const renderer = ctx.getRenderer();
    if (renderer) {
      muxLog.debug("Calling forceRender");
      renderer.forceRender(state);
    }
    muxLog.debug("Grid setup complete");
  } else {
    muxLog.warn("No state available for grid setup");
  }

  // Initialize mux window tracking
  ctx.setMuxWindows([]);
  ctx.setActiveMuxWindowIndex(0);
  ctx.setMuxPaneIds([]);
  ctx.setMuxPendingWindowCount(0);
  ctx.setMuxIsReattaching(false);

  // Check if daemon has existing panes (reattach case)
  const existingPanes = muxSessions.reduce((sum, s) => sum + s.pane_count, 0);
  muxLog.info(`existingPanes=${existingPanes}, sessions=${JSON.stringify(muxSessions)}`);

  if (existingPanes > 0) {
    // Reattach: send Attach message to daemon AFTER APC communication is established.
    // Daemon will respond with PaneCreated + buffered output for existing panes.
    ctx.setMuxPendingWindowCount(existingPanes);
    ctx.setMuxIsReattaching(true);
    muxLog.info(`Reattaching to ${existingPanes} existing pane(s)`);
    const attachSessionId = muxSessions[0]?.id ?? 1;
    // AttachMsg payload: session_id as u32 LE (bincode serializes u32 as 4 bytes LE)
    const attachPayload = new Uint8Array(4);
    const view = new DataView(attachPayload.buffer);
    view.setUint32(0, attachSessionId, true);
    ctx.sendMuxControl(MuxMessageType.Attach, 0, attachPayload);
  } else {
    // Fresh start: create initial window
    try {
      ctx.setMuxPendingWindowCount(ctx.getMuxPendingWindowCount() + 1);
      await client.sendControl(MuxMessageType.CreateWindow, 0);
    } catch (e) {
      muxLog.error(`Mux create window failed: ${e}`);
    }
  }

  muxLog.info("enterMuxMode completed, enabling prefix key");
  // Enable prefix key handling
  const muxSettings = SettingsService.getCached()?.mux;
  const keyboardHandler = ctx.getKeyboardHandler();
  if (keyboardHandler) {
    keyboardHandler.enableMuxMode(
      muxSettings?.prefix ?? "Ctrl+B",
      muxSettings?.keybinds ?? {},
      (action) => ctx.handleMuxAction(action),
    );
  }
}

/** Exit mux mode -- disconnect, disable prefix key, hide status bar. */
export function exitMuxMode(ctx: MuxSessionContext): void {
  if (!ctx.getInMuxMode()) return;
  ctx.setInMuxMode(false);

  muxLog.info("Exiting mux mode");

  // Clear mux APC context
  setMuxApcContext(null);

  // Exit copy mode if active
  const copyModeManager = ctx.getCopyModeManager();
  if (copyModeManager) {
    copyModeManager.exit();
    ctx.setCopyModeManager(null);
    ctx.setCopyModeKeybinds(null);
  }

  // Re-enable original PTY output
  const ptyHandlerHandle = ctx.getPtyHandlerHandle();
  if (ptyHandlerHandle) {
    ptyHandlerHandle.suppressOriginalPty = false;
  }

  // Screen restoration is now handled by the daemon's shadow VT100 parser.
  // No need to save frontend snapshots (WASM serialization/deserialization is slow).

  // Restore original grid
  const muxOriginalGrid = ctx.getMuxOriginalGrid();
  const state = ctx.getState();
  if (muxOriginalGrid && state) {
    state.swapPrimaryGrid(muxOriginalGrid);
    ctx.registerCoreCallbacks(state.getActiveCore());
    const renderer = ctx.getRenderer();
    if (renderer) {
      renderer.forceRender(state);
    }
    ctx.setMuxOriginalGrid(null);
  }

  // Clean up multi-pane state
  if (ctx.getMuxLayoutRoot()) {
    ctx.exitMultiPaneMode(null);
  }

  // Save active window index for reattach
  ctx.setMuxLastActiveIndex(ctx.getActiveMuxWindowIndex());

  // Reset mux window tracking
  ctx.setMuxWindows([]);
  ctx.setActiveMuxWindowIndex(0);
  ctx.setMuxPaneIds([]);
  ctx.setMuxPendingWindowCount(0);
  ctx.setMuxIsReattaching(false);
  ctx.setMuxPendingSplitCount(0);
  ctx.getMuxPaneGrids().clear();
  ctx.emitMuxStateChange();

  // Disable prefix key handling
  const keyboardHandler = ctx.getKeyboardHandler();
  if (keyboardHandler) {
    keyboardHandler.disableMuxMode();
  }

  // Restore direct PTY writes
  const ptyClient = ctx.getPtyClient();
  if (ptyClient) {
    ptyClient.setWriteProxy(null);
  }

  // Disconnect
  const muxClient = ctx.getMuxClient();
  if (muxClient) {
    muxClient.disconnect().catch(() => {});
    ctx.setMuxClient(null);
  }

  // Trigger host shell prompt redraw via SIGWINCH.
  // During mux mode, the host shell's prompt output was suppressed.
  // A resize kick forces the shell to redraw its prompt line.
  if (ptyClient && state) {
    const cols = state.getWasmCore().cols();
    const rows = state.getWasmCore().rows();
    ptyClient.resize(cols - 1, rows);
    ptyClient.resize(cols, rows);
  }
}
