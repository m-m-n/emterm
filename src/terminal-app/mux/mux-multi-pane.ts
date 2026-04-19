/**
 * Mux multi-pane layout functions extracted from TerminalApp.
 * Handles split pane creation, layout management, pane resizing,
 * and multi-pane mode lifecycle.
 */

import { TerminalState } from "../../terminal/state";
import { WasmGrid } from "../../terminal/wasm/terminal-core";
import { createRenderer, type ITerminalRenderer } from "../../terminal";
import {
  calculateLayout,
  splitPane as splitLayoutPane,
  removePane as removeLayoutPane,
  getAllPaneIds,
  type LayoutNode,
  type SplitDirection,
} from "../../terminal/mux/layout";
import { applyLayoutToContainer } from "../../terminal/mux/pane-border";
import { MuxMessageType } from "../../terminal/mux/mux-client";
import { muxLog } from "../../terminal/mux/mux-logger";
import { SettingsService } from "../../settings/settings-service";
import type { CharSize } from "../types";

/** Entry for a pane canvas with its associated state. */
export interface MuxPaneEntry {
  container: HTMLElement;
  canvas: HTMLCanvasElement;
  grid: WasmGrid;
  state: TerminalState;
  renderer: ITerminalRenderer;
}

/** Subset of TerminalApp state needed by multi-pane functions. */
export interface MuxMultiPaneContext {
  terminalRoot: HTMLElement | null;
  container: HTMLElement;
  state: TerminalState | null;
  renderer: ITerminalRenderer | null;
  charSize: CharSize;
  muxLayoutRoot: LayoutNode | null;
  muxActivePaneId: number | null;
  muxPaneCanvases: Map<number, MuxPaneEntry>;
  muxPaneContainer: HTMLElement | null;
  muxPreZoomLayout: LayoutNode | null;
  getActiveMuxPaneId(): number | null;
  sendMuxControl(msgType: number, paneId: number, payload?: Uint8Array): void;
  registerCoreCallbacks(core: ReturnType<TerminalState["getActiveCore"]>): void;
  initMuxDragResize(): void;
}

/** Handle a split pane creation from the daemon. */
export function handleMuxSplitPaneCreated(
  ctx: MuxMultiPaneContext,
  newPaneId: number,
  direction: SplitDirection,
): void {
  if (!ctx.state || !ctx.terminalRoot) return;

  const activePaneId = ctx.getActiveMuxPaneId();
  if (activePaneId == null) return;

  // First split: transition from single-canvas to multi-canvas mode
  if (!ctx.muxLayoutRoot) {
    initMultiPaneMode(ctx, activePaneId);
  }

  // Split the active pane in the layout tree
  const containerWidth = ctx.terminalRoot.clientWidth;
  const containerHeight = ctx.terminalRoot.clientHeight;
  const newLayout = splitLayoutPane(
    ctx.muxLayoutRoot!, activePaneId, newPaneId, direction,
    containerWidth, containerHeight,
    ctx.charSize.width, ctx.charSize.height,
  );
  if (!newLayout) {
    muxLog.warn("Split refused: pane too small");
    return;
  }
  ctx.muxLayoutRoot = newLayout;

  // Create canvas and renderer for the new pane
  createPaneCanvas(ctx, newPaneId);

  // Set new pane as active
  setActiveMuxPane(ctx, newPaneId);

  // Apply layout to all pane canvases
  applyMuxLayout(ctx);

  // Send resize messages for all panes based on new layout
  sendPaneResizes(ctx);

  muxLog.info(`Split pane created: id=${newPaneId}, direction=${direction}`);
}

/** Initialize multi-pane mode from single-pane mode. */
export function initMultiPaneMode(ctx: MuxMultiPaneContext, existingPaneId: number): void {
  if (!ctx.terminalRoot || !ctx.state) return;

  // Create overlay container for pane canvases
  if (!ctx.muxPaneContainer) {
    ctx.muxPaneContainer = document.createElement("div");
    ctx.muxPaneContainer.className = "mux-pane-container";
    ctx.muxPaneContainer.style.position = "absolute";
    ctx.muxPaneContainer.style.inset = "0";
    ctx.terminalRoot.appendChild(ctx.muxPaneContainer);
  }
  ctx.muxPaneContainer.style.display = "block";

  // Initialize layout tree with existing pane as single leaf
  ctx.muxLayoutRoot = { type: "leaf", paneId: existingPaneId };

  // Get the current grid and renderer for the existing pane
  const existingGrid = ctx.state.getPrimaryGrid();
  if (!existingGrid) return;

  // Create a pane canvas for the existing pane
  createPaneCanvas(ctx, existingPaneId);

  // Move the existing grid into the pane canvas state
  const paneEntry = ctx.muxPaneCanvases.get(existingPaneId);
  if (paneEntry) {
    // Dispose the auto-created grid and replace with the existing one
    paneEntry.grid.dispose();
    paneEntry.grid = existingGrid;
    paneEntry.state.swapPrimaryGrid(existingGrid);
  }

  // Hide the main canvas (renderer manages it)
  const mainCanvas = ctx.terminalRoot.querySelector("canvas:not(.mux-pane-canvas)") as HTMLCanvasElement | null;
  if (mainCanvas) {
    mainCanvas.style.display = "none";
  }

  ctx.muxActivePaneId = existingPaneId;

  // Wire up drag-resize for pane borders
  ctx.initMuxDragResize();
}

/** Create a canvas element and renderer for a new pane. */
export function createPaneCanvas(ctx: MuxMultiPaneContext, paneId: number): void {
  if (!ctx.muxPaneContainer || !ctx.state) return;

  const container = document.createElement("div");
  container.className = "mux-pane";
  container.dataset.paneId = String(paneId);
  container.style.position = "absolute";
  container.style.overflow = "hidden";
  container.style.boxSizing = "border-box";

  ctx.muxPaneContainer.appendChild(container);

  // Create a WASM grid and TerminalState for this pane
  const cols = ctx.state.getWasmCore().cols();
  const rows = ctx.state.getWasmCore().rows();
  // Pass `grid` as existingGrid so TerminalState adopts it directly instead of
  // allocating a throw-away internal WasmGrid (~33 MB at cols=206).
  const grid = new WasmGrid(cols, rows, 10000);
  const paneState = new TerminalState(cols, rows, 10000, true, grid);

  // Create a renderer inside this pane container
  const computedStyle = window.getComputedStyle(ctx.container);
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
  ctx.muxPaneCanvases.set(paneId, { container, canvas, grid, state: paneState, renderer: paneRenderer });
}

/** Set the active pane and update visual indicators. */
export function setActiveMuxPane(ctx: MuxMultiPaneContext, paneId: number): void {
  ctx.muxActivePaneId = paneId;

  // Update active pane border styling
  if (ctx.muxPaneContainer && ctx.muxLayoutRoot) {
    const layoutResults = calculateLayout(
      ctx.muxLayoutRoot,
      ctx.muxPaneContainer.clientWidth || ctx.terminalRoot!.clientWidth,
      ctx.muxPaneContainer.clientHeight || ctx.terminalRoot!.clientHeight,
      ctx.charSize.width,
      ctx.charSize.height,
    );
    applyLayoutToContainer(ctx.muxPaneContainer, layoutResults, paneId);
  }
}

/** Apply the current layout tree to position all pane canvases. */
export function applyMuxLayout(ctx: MuxMultiPaneContext): void {
  if (!ctx.muxLayoutRoot || !ctx.muxPaneContainer || !ctx.terminalRoot) return;

  const containerWidth = ctx.muxPaneContainer.clientWidth || ctx.terminalRoot.clientWidth;
  const containerHeight = ctx.muxPaneContainer.clientHeight || ctx.terminalRoot.clientHeight;

  const results = calculateLayout(
    ctx.muxLayoutRoot,
    containerWidth,
    containerHeight,
    ctx.charSize.width,
    ctx.charSize.height,
  );

  applyLayoutToContainer(ctx.muxPaneContainer, results, ctx.muxActivePaneId);

  // Update each pane's canvas dimensions, grid size, and state
  for (const result of results) {
    const paneEntry = ctx.muxPaneCanvases.get(result.paneId);
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
export function sendPaneResizes(ctx: MuxMultiPaneContext): void {
  if (!ctx.muxLayoutRoot || !ctx.muxPaneContainer || !ctx.terminalRoot) return;

  const containerWidth = ctx.muxPaneContainer.clientWidth || ctx.terminalRoot.clientWidth;
  const containerHeight = ctx.muxPaneContainer.clientHeight || ctx.terminalRoot.clientHeight;

  const results = calculateLayout(
    ctx.muxLayoutRoot,
    containerWidth,
    containerHeight,
    ctx.charSize.width,
    ctx.charSize.height,
  );

  for (const result of results) {
    // Encode cols/rows as bincode-compatible u16 LE pairs
    const payload = new Uint8Array(4);
    payload[0] = result.cols & 0xFF;
    payload[1] = (result.cols >> 8) & 0xFF;
    payload[2] = result.rows & 0xFF;
    payload[3] = (result.rows >> 8) & 0xFF;
    ctx.sendMuxControl(MuxMessageType.Resize, result.paneId, payload);
  }
}

/** Remove a pane from the multi-pane layout. */
export function removeMuxPane(ctx: MuxMultiPaneContext, paneId: number): void {
  // If zoomed and removing the zoomed pane, unzoom first
  if (ctx.muxPreZoomLayout && paneId === ctx.muxActivePaneId) {
    ctx.muxLayoutRoot = ctx.muxPreZoomLayout;
    ctx.muxPreZoomLayout = null;
    for (const [, p] of ctx.muxPaneCanvases) {
      p.container.style.display = "";
    }
  }

  // Remove from layout tree
  if (ctx.muxLayoutRoot) {
    const newRoot = removeLayoutPane(ctx.muxLayoutRoot, paneId);
    if (newRoot === null) {
      // Last pane removed -- this shouldn't happen here, handled by handleMuxPaneExited
      return;
    }
    ctx.muxLayoutRoot = newRoot;
  }

  // Clean up pane canvas and state (state.dispose() also frees the WASM grid)
  const paneEntry = ctx.muxPaneCanvases.get(paneId);
  if (paneEntry) {
    paneEntry.state.dispose();
    paneEntry.container.remove();
    ctx.muxPaneCanvases.delete(paneId);
  }

  // If only one pane left, exit multi-pane mode
  const remainingPanes = ctx.muxLayoutRoot ? getAllPaneIds(ctx.muxLayoutRoot) : [];
  if (remainingPanes.length <= 1) {
    exitMultiPaneMode(ctx, remainingPanes[0] ?? null);
    return;
  }

  // Select new active pane if needed
  if (ctx.muxActivePaneId === paneId) {
    setActiveMuxPane(ctx, remainingPanes[0]!);
  }

  applyMuxLayout(ctx);
  sendPaneResizes(ctx);
}

/** Exit multi-pane mode, returning to single-canvas rendering. */
export function exitMultiPaneMode(ctx: MuxMultiPaneContext, remainingPaneId: number | null): void {
  // Restore the remaining pane's grid as the main grid
  if (remainingPaneId != null) {
    const paneEntry = ctx.muxPaneCanvases.get(remainingPaneId);
    if (paneEntry && ctx.state) {
      ctx.state.swapPrimaryGrid(paneEntry.grid);
      ctx.registerCoreCallbacks(ctx.state.getActiveCore());
      // Swap a dummy grid into the pane state before disposing to avoid
      // double-freeing the grid we just moved into ctx.state
      const dummyGrid = new WasmGrid(1, 1, 0);
      paneEntry.state.swapPrimaryGrid(dummyGrid);
      paneEntry.state.dispose();
      paneEntry.container.remove();
      ctx.muxPaneCanvases.delete(remainingPaneId);
    }
  }

  // Clean up any remaining pane canvases (state.dispose() also frees the WASM grid)
  for (const [, paneEntry] of ctx.muxPaneCanvases) {
    paneEntry.state.dispose();
    paneEntry.container.remove();
  }
  ctx.muxPaneCanvases.clear();

  // Remove pane container
  if (ctx.muxPaneContainer) {
    ctx.muxPaneContainer.remove();
    ctx.muxPaneContainer = null;
  }

  // Show the main canvas again
  if (ctx.terminalRoot) {
    const mainCanvas = ctx.terminalRoot.querySelector("canvas:not(.mux-pane-canvas)") as HTMLCanvasElement | null;
    if (mainCanvas) {
      mainCanvas.style.display = "block";
    }
  }

  ctx.muxLayoutRoot = null;
  ctx.muxActivePaneId = null;
  ctx.muxPreZoomLayout = null;

  if (ctx.state && ctx.renderer) {
    ctx.renderer.forceRender(ctx.state);
  }
}
