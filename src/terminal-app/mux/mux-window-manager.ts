/**
 * Mux Window Manager functions extracted from TerminalApp.
 * Handles mux window/pane lifecycle: creation, switching, resizing, and cleanup.
 */

import { invoke } from "@tauri-apps/api/core";
import { WasmGrid } from "../../terminal/wasm/terminal-core";
import { MuxMessageType } from "../../terminal/mux/mux-client";
import type { MuxClient } from "../../terminal/mux/mux-client";
import type { TerminalState } from "../../terminal/state";
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
  getMuxPaneGrids: () => Map<number, WasmGrid>;
  getMuxDetachedGrids: () => Map<string, Uint8Array>;
  getMuxPendingWindowCount: () => number;
  setMuxPendingWindowCount: (count: number) => void;
  getMuxIsReattaching: () => boolean;
  setMuxIsReattaching: (value: boolean) => void;
  getMuxPendingSplitCount: () => number;
  setMuxPendingSplitCount: (count: number) => void;
  getMuxPendingSplitDirection: () => SplitDirection;
  getMuxLayoutRoot: () => LayoutNode | null;
  getMuxPaneCanvases: () => Map<number, unknown>;

  // Callbacks
  onMuxStateChange: ((info: {
    windowCount: number;
    activeWindow: number;
    windowNames: string[];
  }) => void) | null;

  // Delegate methods that remain on TerminalApp
  registerCoreCallbacks: (core: ReturnType<TerminalState["getActiveCore"]>) => void;
  sendMuxControl: (msgType: number, paneId: number, payload?: Uint8Array) => void;
  handleMuxSplitPaneCreated: (paneId: number, direction: SplitDirection) => void;
  removeMuxPane: (paneId: number) => void;
  exitMuxMode: () => void;
  enterMuxMode: (socketPath: string, sessionId: number) => Promise<void>;
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

  // Save current pane's grid (swap out)
  if (previousIndex != null) {
    const prevPaneId = muxPaneIds[previousIndex];
    if (prevPaneId != null) {
      const currentGrid = state.getPrimaryGrid();
      if (currentGrid) {
        muxPaneGrids.set(prevPaneId, currentGrid);
      }
    }
  }

  // Restore the target pane's grid (swap in)
  const newPaneId = muxPaneIds[ctx.getActiveMuxWindowIndex()];
  if (newPaneId != null) {
    const savedGrid = muxPaneGrids.get(newPaneId);
    if (savedGrid) {
      muxPaneGrids.delete(newPaneId);
      state.swapPrimaryGrid(savedGrid);
      ctx.registerCoreCallbacks(state.getActiveCore());
    } else {
      // No saved grid (first visit) — just clear
      state.getWasmCore().reset();
    }
  }

  const renderer = ctx.getRenderer();
  if (renderer) {
    renderer.forceRender(state);
  }
  emitMuxStateChange(ctx);
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

  // Save current pane's grid before switching
  const previousIndex = ctx.getActiveMuxWindowIndex();
  const prevPaneId = muxPaneIds[previousIndex];
  if (prevPaneId != null && state) {
    const currentGrid = state.getPrimaryGrid();
    if (currentGrid) {
      muxPaneGrids.set(prevPaneId, currentGrid);
    }
  }

  const newIdx = muxWindows.length;
  muxWindows.push({ id: newIdx, name: `${newIdx}:shell` });
  muxPaneIds.push(paneId);
  ctx.setActiveMuxWindowIndex(newIdx);

  console.info(`[INFO][FRONTEND] Mux pane created: id=${paneId}, window=${newIdx}`);

  // Try to restore from detached snapshot, otherwise create fresh grid
  const detachedKey = `pane-${paneId}`;
  const detachedSnapshot = muxDetachedGrids.get(detachedKey);
  // Shadow parser on the daemon side now handles screen restoration via PtyOutput.
  // Skip slow frontend snapshot restore (WASM deserialization takes ~3s in debug).
  // Just create a fresh grid — the daemon's screen data will populate it.
  createFreshMuxGrid(ctx);
  muxDetachedGrids.delete(detachedKey);

  // Send initial resize so daemon PTY matches actual terminal dimensions
  sendMuxPaneResize(ctx, paneId);

  // Ensure canvas reflects restored/fresh grid (without this, canvas stays blank)
  const renderer = ctx.getRenderer();
  if (renderer && state) {
    renderer.forceRender(state);
  }

  // After all pending windows are received during reattach, switch to first window
  if (ctx.getMuxIsReattaching() && ctx.getMuxPendingWindowCount() === 0 && muxWindows.length > 1 && ctx.getActiveMuxWindowIndex() !== 0) {
    const prev = ctx.getActiveMuxWindowIndex();
    ctx.setActiveMuxWindowIndex(0);
    switchMuxWindow(ctx, prev);
    ctx.setMuxIsReattaching(false);
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

  console.info(`[INFO][FRONTEND] Mux pane ${paneId} exited (window ${windowIdx})`);

  // Clean up snapshot for the exited pane
  muxPaneGrids.delete(paneId);

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

  // Renumber window names
  for (let i = 0; i < muxWindows.length; i++) {
    muxWindows[i]!.name = `${i}:shell`;
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

/** Start or attach to mux session directly via Tauri command.
 *  Bypasses the CLI -> OSC -> PTY parser roundtrip for instant response. */
export async function startMuxDirect(ctx: MuxWindowManagerContext): Promise<void> {
  if (ctx.getInMuxMode()) return;
  try {
    const socketPath = await invoke<string>("mux_start_daemon");
    await ctx.enterMuxMode(socketPath, 0);
  } catch (e) {
    console.error("[ERROR][FRONTEND] Direct mux start failed:", e);
  }
}
