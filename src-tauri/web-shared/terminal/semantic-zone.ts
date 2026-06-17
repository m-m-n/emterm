/**
 * Semantic zone tracking for OSC 133 (Semantic Prompts).
 *
 * Tracks prompt/command/output zone markers emitted by shells
 * (bash/zsh/fish) to enable prompt-to-prompt navigation.
 */

export interface SemanticMarker {
  type: "A" | "B" | "C" | "D";
  lineIndex: number;
  exitCode?: number;
}

/**
 * Tracks semantic zone markers from OSC 133 sequences.
 *
 * Markers are stored in chronological order (by lineIndex).
 * Provides binary search for efficient prompt navigation.
 */
export class SemanticZoneTracker {
  private markers: SemanticMarker[] = [];

  /**
   * Add a semantic marker.
   *
   * @param type - Zone type (A/B/C/D)
   * @param lineIndex - Absolute line index
   * @param exitCode - Exit code (only for type D)
   */
  addMarker(type: string, lineIndex: number, exitCode?: number): void {
    if (type !== "A" && type !== "B" && type !== "C" && type !== "D") {
      return; // Ignore unknown zone types
    }
    const marker: SemanticMarker = {
      type,
      lineIndex,
    };
    if (exitCode !== undefined) {
      marker.exitCode = exitCode;
    }
    this.markers.push(marker);
  }

  /**
   * Get all markers.
   */
  getMarkers(): readonly SemanticMarker[] {
    return this.markers;
  }

  /**
   * Get only prompt start (type "A") markers.
   */
  getPromptMarkers(): SemanticMarker[] {
    return this.markers.filter((m) => m.type === "A");
  }

  /**
   * Find the nearest prompt marker above the given line.
   *
   * @param currentLine - Current line index
   * @returns The nearest "A" marker with lineIndex < currentLine, or null
   */
  findPrevPrompt(currentLine: number): SemanticMarker | null {
    const prompts = this.getPromptMarkers();
    // Find the last prompt with lineIndex < currentLine
    for (let i = prompts.length - 1; i >= 0; i--) {
      const prompt = prompts[i];
      if (prompt && prompt.lineIndex < currentLine) {
        return prompt;
      }
    }
    return null;
  }

  /**
   * Find the nearest prompt marker below the given line.
   *
   * @param currentLine - Current line index
   * @returns The nearest "A" marker with lineIndex > currentLine, or null
   */
  findNextPrompt(currentLine: number): SemanticMarker | null {
    const prompts = this.getPromptMarkers();
    // Find the first prompt with lineIndex > currentLine
    for (const prompt of prompts) {
      if (prompt.lineIndex > currentLine) {
        return prompt;
      }
    }
    return null;
  }

  /**
   * Remove markers before the given line and adjust remaining indices.
   *
   * Called when scrollback lines are discarded.
   *
   * @param lineIndex - Lines before this index are removed
   */
  pruneBeforeLine(lineIndex: number): void {
    this.markers = this.markers
      .filter((m) => m.lineIndex >= lineIndex)
      .map((m) => ({ ...m, lineIndex: m.lineIndex - lineIndex }));
  }

  /**
   * Clear all markers.
   */
  clear(): void {
    this.markers = [];
  }
}
