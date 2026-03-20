/**
 * Binary tree layout engine for mux pane splitting.
 *
 * Each node is either a leaf (pane) or a split (direction + ratio + two children).
 * The tree calculates pixel bounds for each pane from the container dimensions.
 */

/** Split direction. */
export type SplitDirection = "horizontal" | "vertical";

/** Pixel rectangle for a pane. */
export interface PaneRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Minimum pane dimensions in cells. */
export const MIN_PANE_COLS = 10;
export const MIN_PANE_ROWS = 2;

/** Border width in pixels between panes. */
export const PANE_BORDER_WIDTH = 1;

/** Layout tree node. */
export type LayoutNode =
  | { type: "leaf"; paneId: number }
  | {
      type: "split";
      direction: SplitDirection;
      ratio: number; // 0.0-1.0, first child gets this fraction
      first: LayoutNode;
      second: LayoutNode;
    };

/** Result of layout calculation: pane bounds. */
export interface LayoutResult {
  paneId: number;
  rect: PaneRect;
  cols: number;
  rows: number;
}

/**
 * Calculate layout for all panes in the tree.
 */
export function calculateLayout(
  root: LayoutNode,
  containerWidth: number,
  containerHeight: number,
  cellWidth: number,
  cellHeight: number,
): LayoutResult[] {
  const results: LayoutResult[] = [];
  layoutNode(root, 0, 0, containerWidth, containerHeight, cellWidth, cellHeight, results);
  return results;
}

function layoutNode(
  node: LayoutNode,
  x: number,
  y: number,
  width: number,
  height: number,
  cellWidth: number,
  cellHeight: number,
  results: LayoutResult[],
): void {
  if (node.type === "leaf") {
    const cols = Math.max(MIN_PANE_COLS, Math.floor(width / cellWidth));
    const rows = Math.max(MIN_PANE_ROWS, Math.floor(height / cellHeight));
    results.push({ paneId: node.paneId, rect: { x, y, width, height }, cols, rows });
    return;
  }

  const { direction, ratio, first, second } = node;
  if (direction === "vertical") {
    // Split left/right
    const firstWidth = Math.floor((width - PANE_BORDER_WIDTH) * ratio);
    const secondWidth = width - firstWidth - PANE_BORDER_WIDTH;
    layoutNode(first, x, y, firstWidth, height, cellWidth, cellHeight, results);
    layoutNode(second, x + firstWidth + PANE_BORDER_WIDTH, y, secondWidth, height, cellWidth, cellHeight, results);
  } else {
    // Split top/bottom
    const firstHeight = Math.floor((height - PANE_BORDER_WIDTH) * ratio);
    const secondHeight = height - firstHeight - PANE_BORDER_WIDTH;
    layoutNode(first, x, y, width, firstHeight, cellWidth, cellHeight, results);
    layoutNode(second, x, y + firstHeight + PANE_BORDER_WIDTH, width, secondHeight, cellWidth, cellHeight, results);
  }
}

/**
 * Split a leaf node into two panes.
 * Returns the new tree with the split, or null if the pane is too small.
 */
export function splitPane(
  root: LayoutNode,
  targetPaneId: number,
  newPaneId: number,
  direction: SplitDirection,
  containerWidth: number,
  containerHeight: number,
  cellWidth: number,
  cellHeight: number,
): LayoutNode | null {
  // Check minimum size before splitting
  const currentLayout = calculateLayout(root, containerWidth, containerHeight, cellWidth, cellHeight);
  const target = currentLayout.find((l) => l.paneId === targetPaneId);
  if (!target) return null;

  if (direction === "vertical") {
    const halfWidth = (target.rect.width - PANE_BORDER_WIDTH) / 2;
    if (Math.floor(halfWidth / cellWidth) < MIN_PANE_COLS) return null;
  } else {
    const halfHeight = (target.rect.height - PANE_BORDER_WIDTH) / 2;
    if (Math.floor(halfHeight / cellHeight) < MIN_PANE_ROWS) return null;
  }

  return replaceNode(root, targetPaneId, {
    type: "split",
    direction,
    ratio: 0.5,
    first: { type: "leaf", paneId: targetPaneId },
    second: { type: "leaf", paneId: newPaneId },
  });
}

function replaceNode(node: LayoutNode, targetPaneId: number, replacement: LayoutNode): LayoutNode | null {
  if (node.type === "leaf") {
    return node.paneId === targetPaneId ? replacement : null;
  }

  const firstResult = replaceNode(node.first, targetPaneId, replacement);
  if (firstResult) {
    return { ...node, first: firstResult };
  }

  const secondResult = replaceNode(node.second, targetPaneId, replacement);
  if (secondResult) {
    return { ...node, second: secondResult };
  }

  return null;
}

/**
 * Remove a pane from the tree. The sibling takes over the parent's space.
 * Returns the new tree, or null if pane not found.
 */
export function removePane(root: LayoutNode, paneId: number): LayoutNode | null {
  if (root.type === "leaf") {
    return root.paneId === paneId ? null : root; // Can't remove the last pane
  }

  return removePaneFromSplit(root, paneId);
}

function removePaneFromSplit(node: LayoutNode, paneId: number): LayoutNode | null {
  if (node.type === "leaf") return null;

  // Check if either direct child is the target leaf
  if (node.first.type === "leaf" && node.first.paneId === paneId) {
    return node.second; // Sibling takes over
  }
  if (node.second.type === "leaf" && node.second.paneId === paneId) {
    return node.first; // Sibling takes over
  }

  // Recurse into children
  const firstResult = removePaneFromSplit(node.first, paneId);
  if (firstResult) {
    return { ...node, first: firstResult };
  }

  const secondResult = removePaneFromSplit(node.second, paneId);
  if (secondResult) {
    return { ...node, second: secondResult };
  }

  return null;
}

/**
 * Resize a split by adjusting its ratio.
 * `splitPath` identifies which split to adjust (path from root).
 */
export function resizeSplit(root: LayoutNode, paneId: number, newRatio: number): LayoutNode {
  if (root.type === "leaf") return root;

  // Find the split containing the pane and adjust ratio
  if (containsPane(root.first, paneId)) {
    if (root.first.type === "leaf" && root.first.paneId === paneId) {
      return { ...root, ratio: Math.max(0.1, Math.min(0.9, newRatio)) };
    }
    return { ...root, first: resizeSplit(root.first, paneId, newRatio) };
  }
  if (containsPane(root.second, paneId)) {
    if (root.second.type === "leaf" && root.second.paneId === paneId) {
      return { ...root, ratio: Math.max(0.1, Math.min(0.9, 1 - newRatio)) };
    }
    return { ...root, second: resizeSplit(root.second, paneId, newRatio) };
  }

  return root;
}

function containsPane(node: LayoutNode, paneId: number): boolean {
  if (node.type === "leaf") return node.paneId === paneId;
  return containsPane(node.first, paneId) || containsPane(node.second, paneId);
}

/**
 * Get all pane IDs in the tree.
 */
export function getAllPaneIds(root: LayoutNode): number[] {
  if (root.type === "leaf") return [root.paneId];
  return [...getAllPaneIds(root.first), ...getAllPaneIds(root.second)];
}

/**
 * Generate preset layouts.
 */
export function presetLayout(
  paneIds: number[],
  preset: "even-horizontal" | "even-vertical" | "main-horizontal" | "main-vertical" | "tiled",
): LayoutNode | null {
  if (paneIds.length === 0) return null;
  if (paneIds.length === 1) return { type: "leaf", paneId: paneIds[0]! };

  switch (preset) {
    case "even-horizontal":
      return evenSplit(paneIds, "horizontal");
    case "even-vertical":
      return evenSplit(paneIds, "vertical");
    case "main-horizontal":
      return mainSplit(paneIds, "horizontal");
    case "main-vertical":
      return mainSplit(paneIds, "vertical");
    case "tiled":
      return tiledLayout(paneIds);
    default:
      return null;
  }
}

function evenSplit(paneIds: number[], direction: SplitDirection): LayoutNode {
  if (paneIds.length === 1) return { type: "leaf", paneId: paneIds[0]! };
  if (paneIds.length === 2) {
    return {
      type: "split",
      direction,
      ratio: 0.5,
      first: { type: "leaf", paneId: paneIds[0]! },
      second: { type: "leaf", paneId: paneIds[1]! },
    };
  }
  const mid = Math.ceil(paneIds.length / 2);
  return {
    type: "split",
    direction,
    ratio: mid / paneIds.length,
    first: evenSplit(paneIds.slice(0, mid), direction),
    second: evenSplit(paneIds.slice(mid), direction),
  };
}

function mainSplit(paneIds: number[], direction: SplitDirection): LayoutNode {
  if (paneIds.length <= 1) return { type: "leaf", paneId: paneIds[0]! };
  const otherDir: SplitDirection = direction === "horizontal" ? "vertical" : "horizontal";
  return {
    type: "split",
    direction,
    ratio: 0.5,
    first: { type: "leaf", paneId: paneIds[0]! },
    second: evenSplit(paneIds.slice(1), otherDir),
  };
}

function tiledLayout(paneIds: number[]): LayoutNode {
  if (paneIds.length <= 2) return evenSplit(paneIds, "vertical");
  if (paneIds.length <= 4) {
    const mid = Math.ceil(paneIds.length / 2);
    return {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: evenSplit(paneIds.slice(0, mid), "vertical"),
      second: evenSplit(paneIds.slice(mid), "vertical"),
    };
  }
  // Recursive tiling for > 4 panes
  const mid = Math.ceil(paneIds.length / 2);
  return {
    type: "split",
    direction: "horizontal",
    ratio: 0.5,
    first: tiledLayout(paneIds.slice(0, mid)),
    second: tiledLayout(paneIds.slice(mid)),
  };
}
