/**
 * Fold manager for command output folding.
 *
 * Manages foldable regions (from OSC 133 C→D zones or custom OSC 777;emterm;fold)
 * and provides line mapping between display lines and actual buffer lines.
 */

export interface FoldRegion {
  /** Unique identifier (based on startLine) */
  id: string;
  /** Absolute line index of fold start */
  startLine: number;
  /** Absolute line index of fold end (exclusive) */
  endLine: number;
  /** Whether currently collapsed */
  collapsed: boolean;
  /** Source of this fold region */
  source: "osc133" | "custom";
  /** Command text (for osc133 source) */
  commandText?: string;
  /** Fold label (for custom source) */
  label?: string;
  /** Exit code (for osc133 source) */
  exitCode?: number;
  /** Number of lines in the fold region */
  lineCount: number;
}

export class FoldManager {
  private regions: Map<string, FoldRegion> = new Map();
  private enabled: boolean = true;
  /** Cache of collapsed regions sorted by startLine, invalidated on changes */
  private collapsedCache: FoldRegion[] | null = null;

  /**
   * Register a foldable region from OSC 133 C→D pair.
   */
  registerOsc133Region(
    startLine: number,
    endLine: number,
    commandText: string,
    exitCode?: number,
  ): void {
    const lineCount = endLine - startLine;
    if (lineCount <= 0) return;

    const id = `osc133:${startLine}`;
    if (this.hasOverlap(startLine, endLine)) return;

    const region: FoldRegion = {
      id,
      startLine,
      endLine,
      collapsed: false,
      source: "osc133",
      commandText,
      exitCode,
      lineCount,
    };
    this.regions.set(id, region);
    this.invalidateCache();
  }

  /**
   * Register a foldable region from custom OSC fold.
   */
  registerCustomRegion(
    startLine: number,
    endLine: number,
    label: string,
  ): void {
    const lineCount = endLine - startLine;
    if (lineCount <= 0) return;

    const id = `custom:${startLine}`;
    if (this.hasOverlap(startLine, endLine)) return;

    const region: FoldRegion = {
      id,
      startLine,
      endLine,
      collapsed: false,
      source: "custom",
      label: label || "...",
      lineCount,
    };
    this.regions.set(id, region);
    this.invalidateCache();
  }

  /**
   * Toggle fold state for a region containing the given line.
   * Returns true if a region was toggled, false otherwise.
   */
  toggleFold(lineIndex: number): boolean {
    if (!this.enabled) return false;

    const region = this.findRegionContaining(lineIndex);
    if (!region) return false;

    region.collapsed = !region.collapsed;
    this.invalidateCache();
    return true;
  }

  /**
   * Get fold region at a specific actual line (if any).
   */
  getRegionAtLine(lineIndex: number): FoldRegion | null {
    return this.findRegionContaining(lineIndex);
  }

  /**
   * Get all collapsed regions sorted by startLine.
   */
  getCollapsedRegions(): FoldRegion[] {
    if (this.collapsedCache !== null) return this.collapsedCache;

    const collapsed: FoldRegion[] = [];
    for (const region of this.regions.values()) {
      if (region.collapsed) {
        collapsed.push(region);
      }
    }
    collapsed.sort((a, b) => a.startLine - b.startLine);
    this.collapsedCache = collapsed;
    return collapsed;
  }

  /**
   * Map a display line index to an actual line index.
   *
   * When regions are collapsed, display lines skip the hidden lines.
   * The summary line at the start of a collapsed region occupies 1 display row.
   */
  displayLineToActual(displayLine: number): number {
    const collapsed = this.getCollapsedRegions();
    if (collapsed.length === 0) return displayLine;

    let actual = displayLine;
    for (const region of collapsed) {
      if (actual < region.startLine) break;
      if (actual === region.startLine) {
        // This display line IS the summary line
        return region.startLine;
      }
      // Display line is after this collapsed region's summary
      // Add back the hidden lines (lineCount - 1, since summary occupies 1 line)
      actual += region.lineCount - 1;
    }
    return actual;
  }

  /**
   * Map an actual line index to a display line index.
   *
   * Lines inside a collapsed region map to the summary line position.
   */
  actualLineToDisplay(actualLine: number): number {
    const collapsed = this.getCollapsedRegions();
    if (collapsed.length === 0) return actualLine;

    let offset = 0;
    for (const region of collapsed) {
      if (actualLine < region.startLine) break;
      if (actualLine >= region.startLine && actualLine < region.endLine) {
        // Inside collapsed region: maps to summary line
        return region.startLine - offset;
      }
      // After this collapsed region: accumulate hidden lines
      offset += region.lineCount - 1;
    }
    return actualLine - offset;
  }

  /**
   * Check if a display line is a summary line for a collapsed region.
   */
  isSummaryLine(displayLine: number): boolean {
    return this.getSummaryRegion(displayLine) !== null;
  }

  /**
   * Get the fold region for a summary display line.
   */
  getSummaryRegion(displayLine: number): FoldRegion | null {
    const collapsed = this.getCollapsedRegions();
    if (collapsed.length === 0) return null;

    let offset = 0;
    for (const region of collapsed) {
      const summaryDisplay = region.startLine - offset;
      if (displayLine === summaryDisplay) return region;
      if (displayLine < summaryDisplay) break;
      offset += region.lineCount - 1;
    }
    return null;
  }

  /**
   * Calculate total display lines given total actual lines.
   */
  getTotalDisplayLines(totalActualLines: number): number {
    const collapsed = this.getCollapsedRegions();
    let hidden = 0;
    for (const region of collapsed) {
      hidden += region.lineCount - 1;
    }
    return totalActualLines - hidden;
  }

  /**
   * Expand the collapsed region containing the given actual line.
   * Returns true if a region was expanded.
   */
  expandRegionContaining(actualLine: number): boolean {
    const region = this.findRegionContaining(actualLine);
    if (!region || !region.collapsed) return false;

    region.collapsed = false;
    this.invalidateCache();
    return true;
  }

  /**
   * Unfold all regions.
   */
  unfoldAll(): void {
    for (const region of this.regions.values()) {
      region.collapsed = false;
    }
    this.invalidateCache();
  }

  /**
   * Prune regions for discarded scrollback lines.
   * Removes regions entirely before boundary and regions spanning the boundary.
   * Adjusts remaining region indices.
   */
  pruneBeforeLine(lineIndex: number): void {
    const newRegions = new Map<string, FoldRegion>();
    for (const region of this.regions.values()) {
      if (region.endLine <= lineIndex) {
        // Entirely before boundary: remove
        continue;
      }
      if (region.startLine < lineIndex) {
        // Spans boundary: remove (partial overlap)
        continue;
      }
      // After boundary: adjust indices
      const newStartLine = region.startLine - lineIndex;
      const newEndLine = region.endLine - lineIndex;
      const newId =
        region.source === "osc133"
          ? `osc133:${newStartLine}`
          : `custom:${newStartLine}`;
      newRegions.set(newId, {
        ...region,
        id: newId,
        startLine: newStartLine,
        endLine: newEndLine,
      });
    }
    this.regions = newRegions;
    this.invalidateCache();
  }

  /**
   * Enable/disable folding. Disabling unfolds all regions.
   */
  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    if (!enabled) {
      this.unfoldAll();
    }
  }

  /**
   * Check if folding is enabled.
   */
  isEnabled(): boolean {
    return this.enabled;
  }

  // --- Private helpers ---

  private findRegionContaining(lineIndex: number): FoldRegion | null {
    for (const region of this.regions.values()) {
      if (lineIndex >= region.startLine && lineIndex < region.endLine) {
        return region;
      }
    }
    return null;
  }

  private hasOverlap(startLine: number, endLine: number): boolean {
    for (const region of this.regions.values()) {
      if (startLine < region.endLine && endLine > region.startLine) {
        return true;
      }
    }
    return false;
  }

  private invalidateCache(): void {
    this.collapsedCache = null;
  }
}
