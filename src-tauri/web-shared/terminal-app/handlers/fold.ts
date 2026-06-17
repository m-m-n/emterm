/**
 * Fold click handler for terminal fold regions.
 *
 * Handles click events to toggle fold/unfold of collapsible regions.
 */

import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import { SettingsService } from "../../settings/settings-service";

export interface FoldHandlerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getTerminalRoot: () => HTMLElement | null;
  getCharSize: () => { width: number; height: number };
}

export class FoldHandler {
  constructor(private context: FoldHandlerContext) {}

  /**
   * Handle click on fold region to toggle fold/unfold.
   * Only triggers on plain left-click (no modifiers, no text selection).
   */
  handleFoldClick(e: MouseEvent): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    if (!state || !renderer) return;

    const cachedSettings = SettingsService.getCached();
    if (cachedSettings && !cachedSettings.fold_enabled) return;

    const foldManager = state.getFoldManager();
    if (!foldManager.isEnabled()) return;
    if (foldManager.getCollapsedRegions().length === 0 && !this.hasFoldableRegions()) return;

    // Don't toggle if user is selecting text
    const selection = window.getSelection();
    if (selection && selection.toString().length > 0) return;

    // Calculate display row from click coordinates
    const rect = this.context.getTerminalRoot()?.getBoundingClientRect();
    if (!rect) return;

    const charSize = this.context.getCharSize();
    const displayRow = Math.floor((e.clientY - rect.top) / charSize.height);
    if (displayRow < 0 || displayRow >= state.rows) return;

    // Calculate actual display line index
    const scrollbackLength = state.getScrollbackLength();
    const totalActualLines = scrollbackLength + state.rows;
    const totalDisplayLines = foldManager.getTotalDisplayLines(totalActualLines);
    const displayStart = Math.max(0, totalDisplayLines - state.rows - renderer.getScrollOffset());
    const displayLine = displayStart + displayRow;

    // Check if clicking on a summary line (expand)
    const summaryRegion = foldManager.getSummaryRegion(displayLine);
    if (summaryRegion) {
      foldManager.expandRegionContaining(summaryRegion.startLine);
      renderer.forceRender(state);
      return;
    }

    // Check if clicking on a foldable region (collapse)
    const actualLine = foldManager.displayLineToActual(displayLine);
    const region = foldManager.getRegionAtLine(actualLine);
    if (region && !region.collapsed) {
      // Calculate scroll adjustment: if fold is above or at viewport top, adjust scroll
      const regionDisplayLine = foldManager.actualLineToDisplay(region.startLine);
      foldManager.toggleFold(actualLine);
      // Adjust scroll if the fold causes viewport shift
      if (regionDisplayLine < displayStart) {
        const delta = region.lineCount - 1;
        renderer.setScrollOffset(Math.max(0, renderer.getScrollOffset() - delta));
      }
      renderer.forceRender(state);
    }
  }

  /**
   * Check if there are any foldable regions (even if not collapsed).
   */
  hasFoldableRegions(): boolean {
    const state = this.context.getState();
    if (!state) return false;
    const foldManager = state.getFoldManager();
    // Quick check: if there are any regions registered
    return foldManager.getRegionAtLine(0) !== null ||
      foldManager.getCollapsedRegions().length > 0;
  }
}
