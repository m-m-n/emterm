/**
 * Mux drag-resize functions extracted from TerminalApp.
 * Handles pane border drag resizing and zoom toggle.
 */

import {
  calculateLayout,
  resizeSplitBetween,
  getSplitBounds,
  type LayoutNode,
} from "../../terminal/mux/layout";
import { detectBorderHit } from "../../terminal/mux/pane-border";
import type { MuxPaneEntry } from "./mux-multi-pane";
import type { CharSize } from "../types";

/** Drag state for an in-progress border drag. */
export interface MuxDragState {
  direction: "horizontal" | "vertical";
  paneA: number;
  paneB: number;
}

/** Subset of TerminalApp state needed by drag-resize functions. */
export interface MuxDragResizeContext {
  getMuxPaneContainer: () => HTMLElement | null;
  getMuxLayoutRoot: () => LayoutNode | null;
  setMuxLayoutRoot: (layout: LayoutNode | null) => void;
  getCharSize: () => CharSize;
  getMuxDragState: () => MuxDragState | null;
  setMuxDragState: (state: MuxDragState | null) => void;
  getMuxActivePaneId: () => number | null;
  getMuxPaneCanvases: () => Map<number, MuxPaneEntry>;
  getMuxPreZoomLayout: () => LayoutNode | null;
  setMuxPreZoomLayout: (layout: LayoutNode | null) => void;
  applyMuxLayout: () => void;
  sendPaneResizes: () => void;
}

/** Initialize drag-resize listeners on the mux pane container. */
export function initMuxDragResize(ctx: MuxDragResizeContext): void {
  const paneContainer = ctx.getMuxPaneContainer();
  if (!paneContainer) return;

  // Wrap handler references so document listeners can be removed
  const handleDragMove = (e: MouseEvent): void => {
    handleMuxDragMove(ctx, e);
  };
  const handleDragEnd = (_e: MouseEvent): void => {
    handleMuxDragEnd(ctx, handleDragMove, handleDragEnd);
  };

  paneContainer.addEventListener("mousedown", (e) => {
    const layoutRoot = ctx.getMuxLayoutRoot();
    const container = ctx.getMuxPaneContainer();
    if (!layoutRoot || !container) return;
    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const charSize = ctx.getCharSize();
    const results = calculateLayout(
      layoutRoot,
      container.clientWidth,
      container.clientHeight,
      charSize.width,
      charSize.height,
    );

    const hit = detectBorderHit(x, y, results);
    if (!hit) return;

    e.preventDefault();

    ctx.setMuxDragState({
      direction: hit.direction,
      paneA: hit.paneA,
      paneB: hit.paneB,
    });

    document.addEventListener("mousemove", handleDragMove);
    document.addEventListener("mouseup", handleDragEnd);
  });

  // Cursor change on hover
  paneContainer.addEventListener("mousemove", (e) => {
    if (ctx.getMuxDragState()) return;
    const layoutRoot = ctx.getMuxLayoutRoot();
    const container = ctx.getMuxPaneContainer();
    if (!layoutRoot || !container) return;

    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    const charSize = ctx.getCharSize();
    const results = calculateLayout(
      layoutRoot,
      container.clientWidth,
      container.clientHeight,
      charSize.width,
      charSize.height,
    );

    const hit = detectBorderHit(x, y, results);
    container.style.cursor = hit
      ? (hit.direction === "vertical" ? "col-resize" : "row-resize")
      : "";
  });
}

/** Calculate split ratio during drag. */
function handleMuxDragMove(ctx: MuxDragResizeContext, e: MouseEvent): void {
  const dragState = ctx.getMuxDragState();
  const layoutRoot = ctx.getMuxLayoutRoot();
  const paneContainer = ctx.getMuxPaneContainer();
  if (!dragState || !layoutRoot || !paneContainer) return;

  const containerRect = paneContainer.getBoundingClientRect();
  const containerWidth = paneContainer.clientWidth;
  const containerHeight = paneContainer.clientHeight;

  const charSize = ctx.getCharSize();

  // Find the bounds of the parent split that contains both panes
  const splitBounds = getSplitBounds(
    layoutRoot,
    dragState.paneA,
    dragState.paneB,
    0, 0,
    containerWidth,
    containerHeight,
    charSize.width,
    charSize.height,
  );
  if (!splitBounds) return;

  // Calculate ratio relative to the parent split's bounds
  const mousePos = dragState.direction === "vertical"
    ? e.clientX - containerRect.left
    : e.clientY - containerRect.top;

  const splitStart = dragState.direction === "vertical"
    ? splitBounds.x
    : splitBounds.y;

  const splitSize = dragState.direction === "vertical"
    ? splitBounds.width
    : splitBounds.height;

  const newRatio = Math.max(0.1, Math.min(0.9, (mousePos - splitStart) / splitSize));

  ctx.setMuxLayoutRoot(resizeSplitBetween(
    layoutRoot,
    dragState.paneA,
    dragState.paneB,
    newRatio,
  ));
  ctx.applyMuxLayout();
}

/** Cleanup drag state, send final resize. */
function handleMuxDragEnd(
  ctx: MuxDragResizeContext,
  dragMoveHandler: (e: MouseEvent) => void,
  dragEndHandler: (e: MouseEvent) => void,
): void {
  document.removeEventListener("mousemove", dragMoveHandler);
  document.removeEventListener("mouseup", dragEndHandler);
  ctx.setMuxDragState(null);

  // Send resize messages for all panes after drag completes
  ctx.sendPaneResizes();
}

/** Toggle zoom on the active pane. */
export function toggleMuxZoom(ctx: MuxDragResizeContext): void {
  const layoutRoot = ctx.getMuxLayoutRoot();
  const paneContainer = ctx.getMuxPaneContainer();
  const activePaneId = ctx.getMuxActivePaneId();
  if (!layoutRoot || !paneContainer || !activePaneId) return;

  const preZoomLayout = ctx.getMuxPreZoomLayout();

  if (preZoomLayout) {
    // Unzoom: restore saved layout
    ctx.setMuxLayoutRoot(preZoomLayout);
    ctx.setMuxPreZoomLayout(null);

    // Show all pane canvases
    for (const [, pane] of ctx.getMuxPaneCanvases()) {
      pane.container.style.display = "";
    }
  } else {
    // Zoom: save current layout, show only active pane
    ctx.setMuxPreZoomLayout(layoutRoot);
    ctx.setMuxLayoutRoot({ type: "leaf", paneId: activePaneId });

    // Hide non-active pane canvases
    for (const [paneId, pane] of ctx.getMuxPaneCanvases()) {
      pane.container.style.display = paneId === activePaneId ? "" : "none";
    }
  }

  ctx.applyMuxLayout();
  ctx.sendPaneResizes();
  console.info(`[INFO][FRONTEND] Mux zoom: ${ctx.getMuxPreZoomLayout() ? "zoomed" : "restored"}`);
}
