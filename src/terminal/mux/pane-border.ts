/**
 * Pane border rendering and drag-resize handling.
 *
 * Draws 1px borders between panes with accent color on active pane.
 * Supports drag-resize by intercepting mouse events on border areas.
 */

import { PANE_BORDER_WIDTH, type LayoutResult } from "./layout";

/** Border hit zone width for drag detection (wider than visual border). */
const DRAG_HIT_ZONE = 5;

/** CSS class names for pane borders. */
export const BORDER_CLASS = "mux-pane-border";
export const BORDER_ACTIVE_CLASS = "mux-pane-border-active";

/**
 * Apply pane layout as CSS Grid positions on the container.
 */
export function applyLayoutToContainer(
  container: HTMLElement,
  layoutResults: LayoutResult[],
  activePaneId: number | null,
): void {
  for (const result of layoutResults) {
    const el = container.querySelector(`[data-pane-id="${result.paneId}"]`) as HTMLElement | null;
    if (!el) continue;

    el.style.position = "absolute";
    el.style.left = `${result.rect.x}px`;
    el.style.top = `${result.rect.y}px`;
    el.style.width = `${result.rect.width}px`;
    el.style.height = `${result.rect.height}px`;

    // Active pane indicator
    if (result.paneId === activePaneId) {
      el.style.borderColor = "var(--md-sys-color-primary, #6750A4)";
      el.classList.add(BORDER_ACTIVE_CLASS);
    } else {
      el.style.borderColor = "var(--md-sys-color-outline-variant, #49454F)";
      el.classList.remove(BORDER_ACTIVE_CLASS);
    }
    el.style.borderWidth = `${PANE_BORDER_WIDTH}px`;
    el.style.borderStyle = "solid";
    el.style.boxSizing = "border-box";
  }
}

/**
 * Detect if a mouse position is on a border between panes.
 * Returns the pane IDs on either side of the border and the direction.
 */
export function detectBorderHit(
  x: number,
  y: number,
  layoutResults: LayoutResult[],
): { direction: "horizontal" | "vertical"; paneA: number; paneB: number } | null {
  for (let i = 0; i < layoutResults.length; i++) {
    for (let j = i + 1; j < layoutResults.length; j++) {
      const a = layoutResults[i]!;
      const b = layoutResults[j]!;

      // Check vertical border (between left and right panes)
      const rightEdgeA = a.rect.x + a.rect.width;
      if (
        Math.abs(rightEdgeA - b.rect.x) <= PANE_BORDER_WIDTH + 1 &&
        y >= Math.max(a.rect.y, b.rect.y) &&
        y <= Math.min(a.rect.y + a.rect.height, b.rect.y + b.rect.height) &&
        Math.abs(x - rightEdgeA) <= DRAG_HIT_ZONE
      ) {
        return { direction: "vertical", paneA: a.paneId, paneB: b.paneId };
      }

      // Check horizontal border (between top and bottom panes)
      const bottomEdgeA = a.rect.y + a.rect.height;
      if (
        Math.abs(bottomEdgeA - b.rect.y) <= PANE_BORDER_WIDTH + 1 &&
        x >= Math.max(a.rect.x, b.rect.x) &&
        x <= Math.min(a.rect.x + a.rect.width, b.rect.x + b.rect.width) &&
        Math.abs(y - bottomEdgeA) <= DRAG_HIT_ZONE
      ) {
        return { direction: "horizontal", paneA: a.paneId, paneB: b.paneId };
      }
    }
  }
  return null;
}
