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
import { WasmGrid } from "../../terminal/wasm/terminal-core";
import type { MuxPaneGridState } from "../../terminal/state";
import { DECPrivateMode } from "../../terminal/modes";
import { SettingsService } from "../../settings/settings-service";
import type { MuxApcContext } from "../../terminal/handlers/apc_handlers";

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
  getMuxLastActiveIndex: () => number;
  setMuxLastActiveIndex: (index: number) => void;
  setMuxReattachWindows: (windows: import("../../terminal/mux/mux-client").MuxWindowInfo[]) => void;

  // Set the per-tab mux APC context on the ImageHandler
  setMuxApcContext: (ctx: MuxApcContext | null) => void;

  // Called after exitMuxMode to restore early APC context for next manual bridge launch
  onMuxModeExited?: () => void;

  // Status update callback: routes StatusUpdate to OSC layer
  onStatusUpdate?: (msg: { left: string; right: string }) => void;

  /**
   * Returns the deadline (Date.now() ms) until which post-recovery mux
   * traffic should be counted for observability. Returns 0 when the watch
   * window is closed. Used by the PtyOutput callback to gate counter
   * updates after a WASM viaReinit recovery.
   */
  getPostRecoveryWatchUntil?: () => number;

  /**
   * Increment the post-recovery PtyOutput counters by `bytes`. Called from
   * the PtyOutput callback only when the watch window is open. Aggregated
   * counts are emitted as a single summary log at window close to avoid
   * per-chunk logging on the hot path.
   */
  countPostRecoveryPtyOutput?: (bytes: number) => void;

  /**
   * Pane id we're awaiting a `RequestPaneSnapshot` reply for after a WASM
   * recovery. The PtyOutput callback uses this to log the first chunk that
   * matches and clear the wait. `null` means no snapshot is pending.
   */
  getSnapshotWaitPaneId?: () => number | null;
  /** Set/clear the snapshot-wait pane id. */
  setSnapshotWaitPaneId?: (paneId: number | null) => void;
  /** When the wait was set (performance.now()), used for elapsed-time log. */
  getSnapshotWaitSetAt?: () => number;

  // Delegate methods that call into other mux modules via TerminalApp wrappers
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  handleMuxPaneCreated: (paneId: number) => void;
  handleMuxPaneExited: (paneId: number) => void;
  handleRemoteSwitchWindow: (paneId: number) => void;
  handleMuxAction: (action: MuxAction) => void;
  sendMuxControl: (msgType: number, paneId: number, payload?: Uint8Array) => void;
  getActiveMuxPaneId: () => number | null;
  emitMuxStateChange: () => void;
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
  ctx.setMuxApcContext({
    getMuxClient: () => ctx.getMuxClient(),
  });

  // Set up PTY output handler -- route to correct pane
  client.setOnPtyOutput((paneId: number, data: Uint8Array) => {
    // Observability: during the post-recovery watch window, accumulate
    // chunk/byte counters so a single summary log is emitted when the
    // window closes. Avoids per-chunk logging on the hot path under
    // high mux throughput (案2 観測強化).
    const watchUntil = ctx.getPostRecoveryWatchUntil?.() ?? 0;
    if (watchUntil > 0 && Date.now() < watchUntil) {
      ctx.countPostRecoveryPtyOutput?.(data.length);
    }
    // TEMPORARY DIAGNOSTIC (remove once snapshot path is verified):
    // If a RequestPaneSnapshot is in flight, log the first PtyOutput chunk
    // that targets the matching pane so we can confirm the daemon's reply
    // is reaching the frontend at all. This intentionally cannot tell the
    // snapshot reply apart from any unrelated PTY output for that pane —
    // the log message says "first PtyOutput after snapshot request" rather
    // than "snapshot reply received" to avoid implying causation. Hot-path
    // overhead is two getter calls per chunk; remove this block when the
    // snapshot path is no longer under investigation.
    const waitPaneId = ctx.getSnapshotWaitPaneId?.();
    if (waitPaneId != null && waitPaneId === paneId) {
      const setAt = ctx.getSnapshotWaitSetAt?.() ?? 0;
      const elapsed = setAt > 0 ? performance.now() - setAt : -1;
      console.warn(
        `[WARN][FRONTEND] [DIAG-RECOVERY] first PtyOutput after snapshot request` +
        ` | paneId=${paneId} bytes=${data.length} elapsedMs=${elapsed.toFixed(0)}`,
      );
      ctx.setSnapshotWaitPaneId?.(null);
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
      // Route to inactive pane's saved state — process in a loop to handle
      // buffer switches (alternate screen toggle) and cursor_just_shown interrupts.
      const savedState = ctx.getMuxPaneGrids().get(paneId);
      if (savedState) {
        let remaining = data;
        let iteration = 0;
        while (remaining.length > 0) {
          const useAlt = savedState.useAlternate && !!savedState.alternateGrid;
          const core = useAlt ? savedState.alternateGrid!.core : savedState.primaryGrid.core;

          const consumed = core.process_pty_data(remaining);

          // Handle mode actions (buffer switches)
          const modeActions = core.take_mode_actions();
          if (modeActions.length > 0) {
            let i = 0;
            while (i < modeActions.length) {
              const action = modeActions[i]!;
              if (action === 0xFF || action === 0xFE) {
                // TS_FALLBACK: update savedState.tsModes for TS-only modes
                const mode = modeActions[i + 1]! | (modeActions[i + 2]! << 8);
                const isSet = action === 0xFF;
                applyTsFallbackToSavedState(savedState, mode, isSet);
                i += 3;
              } else if (action === 1 || action === 2) {
                // SWITCH_TO_ALT / SAVE_AND_SWITCH_TO_ALT
                console.warn(`[DIAG-MODE] inactive pane=${paneId} buffer-switch: → ALT (action=${action})`);
                savedState.useAlternate = true;
                if (!savedState.alternateGrid) {
                  const cols = savedState.primaryGrid.core.cols();
                  const rows = savedState.primaryGrid.core.rows();
                  savedState.alternateGrid = new WasmGrid(cols, rows);
                  console.warn(`[DIAG-MODE] inactive pane=${paneId} created alternateGrid ${cols}x${rows}`);
                }
                i += 1;
              } else if (action === 3) {
                // SWITCH_TO_MAIN
                console.warn(`[DIAG-MODE] inactive pane=${paneId} buffer-switch: → MAIN`);
                savedState.useAlternate = false;
                i += 1;
              } else {
                i += 1; // cursor save/restore — skip
              }
            }
          }

          remaining = remaining.subarray(consumed);
          iteration++;
          if (consumed === 0) {
            console.warn(`[DIAG-MODE] inactive pane=${paneId} consumed=0, breaking (remaining=${remaining.length})`);
            break;
          }
        }
        if (iteration > 1) {
          console.warn(`[DIAG-MODE] inactive pane=${paneId} multi-pass: ${iteration} iterations for ${data.length} bytes`);
        }
      } else {
        console.warn(`[DIAG-MODE] inactive pane=${paneId} NO savedState — data dropped (${data.length} bytes)`);
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

  // Set up status update handler -- push to OSC layer via callback
  client.setOnStatusUpdate((msg) => {
    ctx.onStatusUpdate?.(msg);
  });

  // Set up remote switch-window handler (e.g., CLI `emterm mux switch-window`)
  client.setOnSwitchWindow((paneId: number) => {
    ctx.handleRemoteSwitchWindow(paneId);
  });

  // Set up daemon-initiated window rename handler (OSC title detected by daemon)
  client.setOnWindowRenamed((windowId: number, name: string) => {
    const muxWindows = ctx.getMuxWindows();
    const idx = muxWindows.findIndex((w) => w.id === windowId);
    if (idx >= 0 && muxWindows[idx]!.name !== name) {
      console.warn(
        `[DIAG-MUX-RENAME] daemon rename: windowId=${windowId} idx=${idx} old="${muxWindows[idx]!.name}" new="${name}"`,
      );
      muxWindows[idx]!.name = name;
      ctx.emitMuxStateChange();
    }
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
      ctx.setMuxApcContext(null);
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
      ctx.setMuxApcContext(null);
      ctx.setInMuxMode(false);
      ctx.setMuxClient(null);
      return;
    }
  }

  // Route all PTY writes to mux daemon via APC proxy
  ptyClient.setWriteProxy((data: Uint8Array) => {
    const c = ctx.getMuxClient();
    if (!c) { console.warn(`[DIAG-MUX] writeProxy: no muxClient`); return Promise.resolve(); }
    const activePaneId = ctx.getActiveMuxPaneId() ?? ctx.getMuxPaneIds()[ctx.getActiveMuxWindowIndex()] ?? 1;
    return c.sendInput(activePaneId, data);
  });

  // Save the original grid and create a fresh one for mux mode
  const state = ctx.getState();
  if (state) {
    ctx.setMuxOriginalGrid(state.getPrimaryGrid());
    const cols = state.getWasmCore().cols();
    const rows = state.getWasmCore().rows();
    const freshGrid = new WasmGrid(cols, rows, 10000);
    state.swapPrimaryGrid(freshGrid);
    ctx.registerCoreCallbacks(state.getActiveCore());
    const renderer = ctx.getRenderer();
    if (renderer) {
      renderer.forceRender(state);
    }
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
    const activeIdx = muxSessions[0]?.active_window_index ?? 0;
    ctx.setMuxLastActiveIndex(activeIdx);
    ctx.setMuxReattachWindows(muxSessions[0]?.windows ?? []);
    muxLog.info(`Reattaching to ${existingPanes} existing pane(s), active_window_index=${activeIdx}`);
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
  ctx.setMuxApcContext(null);

  // Clear OSC layer (status bar content from daemon)
  ctx.onStatusUpdate?.({ left: "", right: "" });

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

  // Save active window index for reattach
  ctx.setMuxLastActiveIndex(ctx.getActiveMuxWindowIndex());

  // Reset mux window tracking
  ctx.setMuxWindows([]);
  ctx.setActiveMuxWindowIndex(0);
  ctx.setMuxPaneIds([]);
  ctx.setMuxPendingWindowCount(0);
  ctx.setMuxIsReattaching(false);
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
  // Delay: bridge process must exit first so the host shell becomes
  // the foreground process group leader and receives SIGWINCH.
  if (ptyClient && state) {
    const cols = state.getWasmCore().cols();
    const rows = state.getWasmCore().rows();
    const p = ptyClient;
    setTimeout(() => {
      muxLog.info(`SIGWINCH kick: resizing ${cols}x${rows} → ${cols - 1}x${rows} → ${cols}x${rows}`);
      p.resize(cols - 1, rows);
      p.resize(cols, rows);
      muxLog.info("SIGWINCH kick sent");
    }, 500);
  } else {
    muxLog.warn(`SIGWINCH kick skipped: ptyClient=${!!ptyClient}, state=${!!state}`);
  }

  // Restore early APC context so next manual `emterm mux` is detected
  ctx.onMuxModeExited?.();
  muxLog.info("exitMuxMode complete");
}

/**
 * Apply a TS_FALLBACK mode action to an inactive pane's saved state.
 * Updates tsModes for mouse tracking, mouse encoding, and cursor keys.
 */
function applyTsFallbackToSavedState(
  savedState: MuxPaneGridState,
  mode: number,
  isSet: boolean,
): void {
  switch (mode) {
    case DECPrivateMode.DECCKM:
      savedState.tsModes.cursorKeys = isSet ? "application" : "normal";
      break;
    case DECPrivateMode.X10_MOUSE:
      if (isSet) {
        savedState.tsModes.mouseTracking = "x10";
      } else if (savedState.tsModes.mouseTracking === "x10") {
        savedState.tsModes.mouseTracking = "none";
      }
      break;
    case DECPrivateMode.BTN_EVENT_MOUSE:
      if (isSet) {
        savedState.tsModes.mouseTracking = "button";
      } else if (savedState.tsModes.mouseTracking === "button") {
        savedState.tsModes.mouseTracking = "none";
      }
      break;
    case DECPrivateMode.ANY_EVENT_MOUSE:
      if (isSet) {
        savedState.tsModes.mouseTracking = "any";
      } else if (savedState.tsModes.mouseTracking === "any") {
        savedState.tsModes.mouseTracking = "none";
      }
      break;
    case DECPrivateMode.UTF8_MOUSE:
      if (isSet) {
        savedState.tsModes.mouseEncoding = "utf8";
      } else if (savedState.tsModes.mouseEncoding === "utf8") {
        savedState.tsModes.mouseEncoding = "default";
      }
      break;
    case DECPrivateMode.SGR_MOUSE:
      if (isSet) {
        savedState.tsModes.mouseEncoding = "sgr";
      } else if (savedState.tsModes.mouseEncoding === "sgr") {
        savedState.tsModes.mouseEncoding = "default";
      }
      break;
    default:
      console.warn(`[DIAG-MODE] inactive pane: unhandled TS_FALLBACK mode=${mode} isSet=${isSet}`);
      break;
  }
}
