/**
 * Mux Window Manager functions extracted from TerminalApp.
 * Handles mux window/pane lifecycle: creation, switching, resizing, and cleanup.
 */

import { WasmGrid } from "../../terminal/wasm/terminal-core";
import { muxLog } from "../../terminal/mux/mux-logger";
import { MuxMessageType } from "../../terminal/mux/mux-client";
import type { MuxClient, MuxWindowInfo } from "../../terminal/mux/mux-client";
import type { TerminalState, MuxPaneGridState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import type { KeyboardHandler } from "../handlers/keyboard";
import type { LayoutNode } from "../../terminal/mux/layout";
import type { SplitDirection } from "../../terminal/mux/layout";
import { SettingsService } from "../../settings/settings-service";

/**
 * Context needed by mux window manager functions.
 * Provides access to the subset of TerminalApp state these functions need.
 */
export interface MuxWindowManagerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getMuxClient: () => MuxClient | null;
  getKeyboardHandler: () => KeyboardHandler | null;
  getInMuxMode: () => boolean;

  // Mux window/pane state accessors
  getMuxWindows: () => { id: number; name: string }[];
  getActiveMuxWindowIndex: () => number;
  setActiveMuxWindowIndex: (index: number) => void;
  getMuxPaneIds: () => number[];
  getMuxPaneGrids: () => Map<number, MuxPaneGridState>;
  getMuxDetachedGrids: () => Map<string, Uint8Array>;
  getMuxPendingWindowCount: () => number;
  setMuxPendingWindowCount: (count: number) => void;
  getMuxIsReattaching: () => boolean;
  setMuxIsReattaching: (value: boolean) => void;
  getMuxPendingSplitCount: () => number;
  setMuxPendingSplitCount: (count: number) => void;
  getMuxLastActiveIndex: () => number;
  getMuxReattachWindows: () => MuxWindowInfo[];
  getMuxPendingSplitDirection: () => SplitDirection;
  getMuxLayoutRoot: () => LayoutNode | null;
  getMuxPaneCanvases: () => Map<number, unknown>;

  // Callbacks
  onMuxStateChange: ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null;

  // PTY handler
  flushPtyPendingData: () => void;
  processPtyPendingDataNow: () => void;

  // Delegate methods that remain on TerminalApp
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  sendMuxControl: (msgType: number, paneId: number, payload?: Uint8Array) => void;
  handleMuxSplitPaneCreated: (paneId: number, direction: SplitDirection) => void;
  removeMuxPane: (paneId: number) => void;
  exitMuxMode: () => void;
  enterMuxMode: (socketPath: string, sessionId: number) => Promise<void>;
  /** Sync the TerminalApp's title dedup cache to match state._title. Called
   *  after we swap in a different pane's saved title so the next OSC event
   *  isn't suppressed (and the parent tab title immediately reflects the
   *  new active window). */
  syncWindowTitleFromState: () => void;
}

/** Clear the terminal screen for mux window switching. */
export function clearMuxScreen(ctx: MuxWindowManagerContext): void {
  const state = ctx.getState();
  if (state) {
    state.getWasmCore().reset();
    const renderer = ctx.getRenderer();
    if (renderer) {
      renderer.forceRender(state);
    }
  }
}

/** Create a fresh WASM grid for a new mux pane and swap it in. */
export function createFreshMuxGrid(ctx: MuxWindowManagerContext): void {
  const state = ctx.getState();
  if (!state) return;
  // Discard any buffered PTY data from the previous pane to prevent
  // stale output (e.g. TUI app frames) from bleeding into the new grid.
  ctx.flushPtyPendingData();
  const cols = state.getWasmCore().cols();
  const rows = state.getWasmCore().rows();
  const newGrid = new WasmGrid(cols, rows, 10000);
  state.swapPrimaryGrid(newGrid);
  ctx.registerCoreCallbacks(state.getActiveCore());
  const renderer = ctx.getRenderer();
  if (renderer) {
    renderer.forceRender(state);
  }
}

/** Switch to the current activeMuxWindowIndex: swap WASM grids and update UI. */
export function switchMuxWindow(ctx: MuxWindowManagerContext, previousIndex?: number): void {
  const state = ctx.getState();
  if (!state) return;
  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();

  // Capture current terminal dimensions from the active (prev) pane before
  // save/restore. muxPaneGrids entries are only resized when their pane is
  // active, so a pane that was inactive during a terminal resize (e.g., mux
  // status bar appeared after reattach) holds a grid at stale dimensions.
  // We re-apply the current size after restoreMuxPaneState so forceRender
  // paints the correct area and sendMuxPaneResize sends the correct size to
  // the daemon.
  const targetCols = state.cols;
  const targetRows = state.rows;

  // Save current pane's full state (primary + alternate)
  if (previousIndex != null) {
    const prevPaneId = muxPaneIds[previousIndex];
    if (prevPaneId != null) {
      muxPaneGrids.set(prevPaneId, state.saveMuxPaneState());
      // Clear callbacks on saved grids to prevent OSC events from inactive panes
      // leaking into the shared pendingOscQueue and polluting the active window's title
      const saved = muxPaneGrids.get(prevPaneId)!;
      saved.primaryGrid.core.clear_callbacks();
      saved.alternateGrid?.core.clear_callbacks();
    }
  }

  // Discard any buffered PTY data from the previous pane
  ctx.flushPtyPendingData();

  // Restore the target pane's state
  const newPaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()];
  if (newPaneId != null) {
    const savedState = muxPaneGrids.get(newPaneId);
    if (savedState) {
      muxPaneGrids.delete(newPaneId);
      state.restoreMuxPaneState(savedState);
      ctx.registerCoreCallbacks(state.getActiveCore());
    } else {
      // No saved state (first visit, e.g. after reattach). Seed the title
      // from the daemon-provided window name so syncWindowTitleFromState
      // does not overwrite muxWindows[i].name with the "Terminal" fallback.
      state.getWasmCore().reset();
      const windows = ctx.getMuxWindows();
      state._title = windows[ctx.getActiveMuxWindowIndex()]?.name ?? "";
      state._iconName = "";
      ctx.registerCoreCallbacks(state.getActiveCore());
    }
  }

  // Reconcile restored grid dimensions with the current terminal size.
  reconcileActivePaneSize(state, targetCols, targetRows);

  // Sync the title dedup cache and parent tab title to the restored pane.
  ctx.syncWindowTitleFromState();

  // Notify daemon of active pane change for status bar cwd tracking
  const activePaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()];
  if (activePaneId != null) {
    ctx.sendMuxControl(MuxMessageType.SwitchWindow, activePaneId);
    // Reconcile daemon-side PTY dimensions with the current terminal size.
    // The newly activated pane may have stale dimensions (e.g., initialized
    // during reattach before the status bar was restored), which would cause
    // `stty size` to report the wrong row count.
    sendMuxPaneResize(ctx, activePaneId);
    // Request an on-demand screen snapshot so the displayed grid is
    // reconciled with the daemon shadow_parser authoritative state. Without
    // this, any bytes that arrived in the gap between the click and
    // flushPtyPendingData (or any ambiguity in the inactive-pane processing
    // path) can leave the target pane's saved grid stale.
    requestPaneSnapshot(ctx, activePaneId);
  }

  const renderer = ctx.getRenderer();
  if (renderer) {
    diagTraceMuxRender(state, renderer, "switchMuxWindow");
  }
  emitMuxStateChange(ctx);
}

/** Send RequestPaneSnapshot to the daemon for the given pane. The daemon
 *  replies with a PtyOutput frame containing a screen reset + shadow parser
 *  replay, which the normal PTY pipeline applies to the active WASM grid. */
function requestPaneSnapshot(ctx: MuxWindowManagerContext, paneId: number): void {
  const client = ctx.getMuxClient();
  if (!client) return;
  client.sendRequestPaneSnapshot(paneId).catch((err: unknown) => {
    muxLog.warn(
      `sendRequestPaneSnapshot failed for pane ${paneId}: ${
        err instanceof Error ? err.message : String(err)
      }`,
    );
  });
}

/** Resize the currently-active grid to match the given dimensions if they
 *  differ. Used after restoreMuxPaneState to compensate for saved grids that
 *  went stale while their pane was inactive (saved grids are not touched by
 *  ResizeObserver). */
function reconcileActivePaneSize(state: TerminalState, cols: number, rows: number): void {
  if (state.cols === cols && state.rows === rows) return;
  state.resize(cols, rows);
}

/** Diagnostic trace for forceRender on mux switch paths. Logs canvas state,
 *  grid dimensions, parent visibility, and samples again on next rAF to see
 *  whether anything overwrites the canvas after forceRender. */
function diagTraceMuxRender(state: TerminalState, renderer: ITerminalRenderer, callsite: string): void {
  try {
    const activeCore = state.getActiveCore();
    const rows = activeCore.rows();
    const cols = activeCore.cols();
    const dirtyBefore = state.getDirtyRows().length;
    const rend = renderer as unknown as { canvas?: HTMLCanvasElement; cols?: number; rows?: number; scrollOffset?: number; dpr?: number };
    const cw = rend.canvas?.width ?? -1;
    const ch = rend.canvas?.height ?? -1;
    const cssW = rend.canvas?.clientWidth ?? -1;
    const cssH = rend.canvas?.clientHeight ?? -1;
    const parent = rend.canvas?.parentElement;
    const parentDisplay = parent ? getComputedStyle(parent).display : "n/a";
    const parentVis = parent ? getComputedStyle(parent).visibility : "n/a";
    const isReady = state.isReady?.() ?? "n/a";
    console.warn(
      `[DIAG-MUX-RENDER][${callsite}] pre-forceRender` +
      ` | gridCols=${cols} gridRows=${rows}` +
      ` | rendCols=${rend.cols} rendRows=${rend.rows}` +
      ` | scrollOffset=${rend.scrollOffset}` +
      ` | canvasPx=${cw}x${ch} cssPx=${cssW}x${cssH} dpr=${rend.dpr}` +
      ` | parentDisplay=${parentDisplay} parentVis=${parentVis}` +
      ` | dirtyRows=${dirtyBefore}` +
      ` | isReady=${isReady}`,
    );
    const t0 = performance.now();
    renderer.forceRender(state);
    const t1 = performance.now();
    const dirtyAfter = state.getDirtyRows().length;
    console.warn(
      `[DIAG-MUX-RENDER][${callsite}] post-forceRender` +
      ` | elapsed=${(t1 - t0).toFixed(2)}ms` +
      ` | dirtyAfter=${dirtyAfter}` +
      ` | canvasPx=${rend.canvas?.width}x${rend.canvas?.height}`,
    );
    requestAnimationFrame(() => {
      const cw2 = rend.canvas?.width ?? -1;
      const ch2 = rend.canvas?.height ?? -1;
      const parentDisplay2 = parent ? getComputedStyle(parent).display : "n/a";
      const parentVis2 = parent ? getComputedStyle(parent).visibility : "n/a";
      console.warn(
        `[DIAG-MUX-RENDER][${callsite}] next-rAF` +
        ` | canvasPx=${cw2}x${ch2}` +
        ` | parentDisplay=${parentDisplay2} parentVis=${parentVis2}` +
        ` | canvasSizeChanged=${cw !== cw2 || ch !== ch2}`,
      );
    });
  } catch (err) {
    console.warn(`[DIAG-MUX-RENDER][${callsite}] diag error: ${err instanceof Error ? err.message : String(err)}`);
    renderer.forceRender(state);
  }
}

/** Handle PaneCreated from daemon — register actual pane ID and update UI. */
export function handleMuxPaneCreated(ctx: MuxWindowManagerContext, paneId: number): void {
  // Check if this is a split pane response
  if (ctx.getMuxPendingSplitCount() > 0) {
    ctx.setMuxPendingSplitCount(ctx.getMuxPendingSplitCount() - 1);
    ctx.handleMuxSplitPaneCreated(paneId, ctx.getMuxPendingSplitDirection());
    return;
  }

  if (ctx.getMuxPendingWindowCount() <= 0) return;
  ctx.setMuxPendingWindowCount(ctx.getMuxPendingWindowCount() - 1);

  const state = ctx.getState();
  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();
  const muxWindows = ctx.getMuxWindows();
  const muxDetachedGrids = ctx.getMuxDetachedGrids();

  // Save current pane's full state (primary + alternate) before switching.
  // During reattach, process pending PTY data synchronously first so the
  // alternate screen state from the daemon's replay is captured in the save.
  const previousIndex = ctx.getActiveMuxWindowIndex();
  const prevPaneId = muxPaneIds[previousIndex];
  const hadPrevPane = prevPaneId != null && state != null;
  if (hadPrevPane) {
    if (ctx.getMuxIsReattaching()) {
      ctx.processPtyPendingDataNow();
    }
    muxPaneGrids.set(prevPaneId, state!.saveMuxPaneState());
    // Clear callbacks on saved grids to prevent OSC leaking from inactive panes
    const saved = muxPaneGrids.get(prevPaneId)!;
    saved.primaryGrid.core.clear_callbacks();
    saved.alternateGrid?.core.clear_callbacks();
    // Reset shared title state so the NEW window doesn't inherit the
    // previous pane's title via initialName / dedup. The previous pane's
    // title is preserved inside its saved MuxPaneGridState.
    state!._title = "";
    state!._iconName = "";
  }

  const newIdx = muxWindows.length;
  // Determine initial window name and daemon window ID:
  // During reattach, use daemon-provided window info (matched by index position,
  // since PaneCreated messages arrive in the same order as the windows array).
  let initialName = hadPrevPane ? "Terminal" : (state?.title || "Terminal");
  let daemonWindowId = newIdx; // fallback: use frontend index
  if (ctx.getMuxIsReattaching()) {
    const reattachWindows = ctx.getMuxReattachWindows();
    const winInfo = reattachWindows[newIdx];
    if (winInfo) {
      daemonWindowId = winInfo.id;
      if (winInfo.name) {
        initialName = winInfo.name;
      }
    }
  }
  muxWindows.push({ id: daemonWindowId, name: initialName });
  muxPaneIds.push(paneId);
  ctx.setActiveMuxWindowIndex(newIdx);

  console.warn(
    `[DIAG-MUX-ATTACH] push window newIdx=${newIdx} name="${initialName}" daemonId=${daemonWindowId} paneId=${paneId} reattach=${ctx.getMuxIsReattaching()} reattachWindows=${JSON.stringify(ctx.getMuxReattachWindows().map((w) => ({ id: w.id, name: w.name })))}`,
  );

  muxLog.info(`Mux pane created: id=${paneId}, window=${newIdx}`);

  // Try to restore from detached snapshot, otherwise create fresh grid
  const detachedKey = `pane-${paneId}`;
  const detachedSnapshot = muxDetachedGrids.get(detachedKey);
  // Shadow parser on the daemon side now handles screen restoration via PtyOutput.
  // Skip slow frontend snapshot restore (WASM deserialization takes ~3s in debug).
  // Just create a fresh grid — the daemon's screen data will populate it.
  createFreshMuxGrid(ctx);
  muxDetachedGrids.delete(detachedKey);

  // Sync title dedup / parent tab to the fresh pane (empty title) so that
  // the previous pane's title doesn't linger on the parent tab until the
  // new pane emits its own OSC.
  // Skip during reattach — initialName was just set from daemon-provided
  // window name, and syncWindowTitleFromState would overwrite it with the
  // empty state._title (which would also clobber daemon-side via RenameWindow).
  if (hadPrevPane && !ctx.getMuxIsReattaching()) {
    ctx.syncWindowTitleFromState();
  }

  // Send initial resize so daemon PTY matches actual terminal dimensions
  sendMuxPaneResize(ctx, paneId);

  // Ensure canvas reflects restored/fresh grid (without this, canvas stays blank)
  const renderer = ctx.getRenderer();
  if (renderer && state) {
    renderer.forceRender(state);
  }

  // After all pending windows are received during reattach, switch to first window
  if (ctx.getMuxIsReattaching() && ctx.getMuxPendingWindowCount() === 0) {
    // Process any pending output for the last pane before switching,
    // so its screen data and OSC title are captured in the saved state.
    ctx.processPtyPendingDataNow();

    // Restore the active window from before detach (clamped to valid range)
    const targetIndex = Math.min(ctx.getMuxLastActiveIndex(), muxWindows.length - 1);
    if (targetIndex !== ctx.getActiveMuxWindowIndex()) {
      const prev = ctx.getActiveMuxWindowIndex();
      ctx.setActiveMuxWindowIndex(targetIndex);
      switchMuxWindow(ctx, prev);
    }
    ctx.setMuxIsReattaching(false);

    // Request status bar content from daemon after reattach completes
    ctx.getMuxClient()?.sendRequestStatusUpdate().catch(() => {});
  }

  emitMuxStateChange(ctx);
}

/** Send a Resize message to the daemon for a single pane using current terminal dimensions.
 *  Forces SIGWINCH by first sending a slightly smaller size, then the actual size.
 *  This ensures the shell redraws even if the PTY was already at the same dimensions. */
export function sendMuxPaneResize(ctx: MuxWindowManagerContext, paneId: number): void {
  const state = ctx.getState();
  const muxClient = ctx.getMuxClient();
  if (!state || !muxClient) return;
  const cols = state.getWasmCore().cols();
  const rows = state.getWasmCore().rows();

  // Send a slightly different size first to guarantee SIGWINCH
  const kickCols = Math.max(1, cols - 1);
  const kickPayload = new Uint8Array(4);
  kickPayload[0] = kickCols & 0xFF;
  kickPayload[1] = (kickCols >> 8) & 0xFF;
  kickPayload[2] = rows & 0xFF;
  kickPayload[3] = (rows >> 8) & 0xFF;
  ctx.sendMuxControl(MuxMessageType.Resize, paneId, kickPayload);

  // Then send the actual size to restore correct dimensions
  const payload = new Uint8Array(4);
  payload[0] = cols & 0xFF;
  payload[1] = (cols >> 8) & 0xFF;
  payload[2] = rows & 0xFF;
  payload[3] = (rows >> 8) & 0xFF;
  ctx.sendMuxControl(MuxMessageType.Resize, paneId, payload);
}

/** Handle a mux pane exiting (shell closed). Remove the window and switch if needed. */
export function handleMuxPaneExited(ctx: MuxWindowManagerContext, paneId: number): void {
  // Notify daemon to clean up the exited pane (cascade: pane->window->session)
  ctx.sendMuxControl(MuxMessageType.DestroyPane, paneId);

  // Multi-pane mode: remove from layout
  if (ctx.getMuxLayoutRoot() && ctx.getMuxPaneCanvases().has(paneId)) {
    ctx.removeMuxPane(paneId);
    return;
  }

  const muxPaneIds = ctx.getMuxPaneIds();
  const muxWindows = ctx.getMuxWindows();
  const muxPaneGrids = ctx.getMuxPaneGrids();

  const windowIdx = muxPaneIds.indexOf(paneId);
  if (windowIdx === -1) return;

  muxLog.info(`Mux pane ${paneId} exited (window ${windowIdx})`);

  // Clean up snapshot for the exited pane — dispose WASM grids to free memory
  const savedState = muxPaneGrids.get(paneId);
  if (savedState) {
    savedState.primaryGrid.dispose();
    savedState.alternateGrid?.dispose();
    muxPaneGrids.delete(paneId);
  }

  // If the exited pane is NOT the active one, save current pane's snapshot
  // before the index adjustment that follows
  const wasActive = windowIdx === ctx.getActiveMuxWindowIndex();

  // Remove the window
  muxWindows.splice(windowIdx, 1);
  muxPaneIds.splice(windowIdx, 1);

  // If no windows left, exit mux mode
  if (muxWindows.length === 0) {
    ctx.exitMuxMode();
    return;
  }

  // Adjust active window index
  if (ctx.getActiveMuxWindowIndex() >= muxWindows.length) {
    ctx.setActiveMuxWindowIndex(muxWindows.length - 1);
  }

  // Only switch if the active pane was the one that exited
  if (wasActive) {
    switchMuxWindow(ctx);
  } else {
    emitMuxStateChange(ctx);
  }
}

/** Notify listeners of mux window state changes. */
export function emitMuxStateChange(ctx: MuxWindowManagerContext): void {
  const muxWindows = ctx.getMuxWindows();
  console.warn(
    `[DIAG-MUX-STATE] emit windowCount=${muxWindows.length} active=${ctx.getActiveMuxWindowIndex()} names=${JSON.stringify(muxWindows.map((w) => w.name))} ids=${JSON.stringify(muxWindows.map((w) => w.id))}`,
  );
  ctx.onMuxStateChange?.({
    windowCount: muxWindows.length,
    activeWindow: ctx.getActiveMuxWindowIndex(),
    windowNames: muxWindows.map((w) => w.name),
  });
}

/** Re-apply mux keybind settings (call when settings change at runtime). */
export function reloadMuxSettings(ctx: MuxWindowManagerContext): void {
  if (!ctx.getInMuxMode() || !ctx.getKeyboardHandler()) return;
  const muxSettings = SettingsService.getCached()?.mux;
  if (muxSettings) {
    ctx.getKeyboardHandler()!.updateMuxSettings(
      muxSettings.prefix ?? "Ctrl+B",
      muxSettings.keybinds ?? {},
    );
  }
}

/** Handle a remote SwitchWindow notification (e.g., from CLI `emterm mux switch-window`).
 *  Finds the window containing the given paneId and switches to it.
 *  Does NOT send SwitchWindow back to daemon (unlike switchMuxWindow) to avoid feedback loops. */
export function handleRemoteSwitchWindow(ctx: MuxWindowManagerContext, paneId: number): void {
  const state = ctx.getState();
  if (!state) return;

  const muxPaneIds = ctx.getMuxPaneIds();
  const muxPaneGrids = ctx.getMuxPaneGrids();
  const targetIndex = muxPaneIds.indexOf(paneId);
  if (targetIndex === -1) {
    muxLog.warn(`Remote SwitchWindow: pane ${paneId} not found in local windows`);
    return;
  }
  if (targetIndex === ctx.getActiveMuxWindowIndex()) {
    muxLog.debug(`Remote SwitchWindow: already on window ${targetIndex}`);
    return;
  }

  muxLog.info(`Remote SwitchWindow: switching to window ${targetIndex} (pane ${paneId})`);

  // Capture current terminal dimensions before save/restore — see
  // switchMuxWindow for rationale.
  const targetCols = state.cols;
  const targetRows = state.rows;

  // Save current pane's full state
  const previousIndex = ctx.getActiveMuxWindowIndex();
  const prevPaneId = muxPaneIds[previousIndex];
  if (prevPaneId != null) {
    muxPaneGrids.set(prevPaneId, state.saveMuxPaneState());
    // Clear callbacks on saved grids to prevent OSC leaking from inactive panes
    const saved = muxPaneGrids.get(prevPaneId)!;
    saved.primaryGrid.core.clear_callbacks();
    saved.alternateGrid?.core.clear_callbacks();
  }

  // Discard any buffered PTY data from the previous pane
  ctx.flushPtyPendingData();

  ctx.setActiveMuxWindowIndex(targetIndex);

  // Restore the target pane's state
  const savedState = muxPaneGrids.get(paneId);
  if (savedState) {
    muxPaneGrids.delete(paneId);
    state.restoreMuxPaneState(savedState);
    ctx.registerCoreCallbacks(state.getActiveCore());
  } else {
    state.getWasmCore().reset();
    const windows = ctx.getMuxWindows();
    state._title = windows[ctx.getActiveMuxWindowIndex()]?.name ?? "";
    state._iconName = "";
    ctx.registerCoreCallbacks(state.getActiveCore());
  }

  // Reconcile restored grid dimensions with the current terminal size.
  reconcileActivePaneSize(state, targetCols, targetRows);

  // Sync the title dedup cache and parent tab title to the restored pane.
  ctx.syncWindowTitleFromState();

  // Skip sendMuxControl(SwitchWindow) — the daemon already knows

  // Reconcile daemon-side PTY dimensions with the current terminal size.
  // Same rationale as switchMuxWindow: the target pane may have stale
  // dimensions if reattach initialized it before the status bar was restored.
  sendMuxPaneResize(ctx, paneId);

  // Request on-demand screen snapshot — same rationale as switchMuxWindow.
  requestPaneSnapshot(ctx, paneId);

  const renderer = ctx.getRenderer();
  if (renderer) {
    renderer.forceRender(state);
  }
  emitMuxStateChange(ctx);
}

/** Start or attach to mux session via inband protocol.
 *  Launches bridge process in the PTY and communicates via APC. */
export async function startMuxDirect(ctx: MuxWindowManagerContext): Promise<void> {
  if (ctx.getInMuxMode()) return;
  // enterMuxMode launches the bridge process and waits for Welcome APC
  await ctx.enterMuxMode("", 0);
}
