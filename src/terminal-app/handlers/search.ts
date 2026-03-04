/**
 * Search handler for terminal text search.
 *
 * Manages the search bar UI and search state, coordinating between
 * the search state manager and the terminal renderer for highlighting.
 */

import { SearchStateManager } from "../../terminal/search/search-state";
import { SearchBar } from "../../terminal/search/search-bar";
import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";

export interface SearchHandlerContext {
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getImeHandler: () => { focus(): void } | null;
}

export class SearchHandler {
  private searchStateManager: SearchStateManager = new SearchStateManager();
  private searchBar: SearchBar | null = null;

  constructor(private context: SearchHandlerContext) {}

  init(terminalRoot: HTMLElement): void {
    this.searchBar = new SearchBar(terminalRoot, {
      onSearch: (query, options) => this.handleSearch(query, options),
      onNextMatch: () => this.handleSearchNext(),
      onPrevMatch: () => this.handleSearchPrev(),
      onClose: () => this.handleSearchClose(),
    });
  }

  toggleSearch(): void {
    if (!this.searchBar) return;

    if (this.searchBar.isVisible()) {
      this.handleSearchClose();
    } else {
      this.searchBar.show();
    }
  }

  /**
   * Handle search query/options change from search bar.
   */
  private handleSearch(query: string, options: { isRegex: boolean; caseSensitive: boolean }): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    if (!state || !renderer) return;

    this.searchStateManager.setQuery(query);
    this.searchStateManager.setOptions(options);

    // Collect all line texts (scrollback + screen)
    const lines = this.getAllLineTexts();
    this.searchStateManager.executeSearch(lines);

    // Update search bar UI
    this.searchBar?.updateCount(
      this.searchStateManager.currentMatchIndex,
      this.searchStateManager.matches.length,
    );
    this.searchBar?.setError(this.searchStateManager.error !== null);

    // Update highlight rendering
    renderer.setSearchHighlights(
      this.searchStateManager.matches,
      this.searchStateManager.currentMatchIndex,
    );
    renderer.forceRender(state);

    // Scroll to first match if found
    if (this.searchStateManager.matches.length > 0) {
      this.scrollToCurrentMatch();
    }
  }

  /**
   * Handle next match navigation.
   */
  private handleSearchNext(): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    if (!state || !renderer) return;

    const match = this.searchStateManager.nextMatch();
    if (match) {
      renderer.setSearchHighlights(
        this.searchStateManager.matches,
        this.searchStateManager.currentMatchIndex,
      );
      this.searchBar?.updateCount(
        this.searchStateManager.currentMatchIndex,
        this.searchStateManager.matches.length,
      );
      this.scrollToCurrentMatch();
      renderer.forceRender(state);
    }
  }

  /**
   * Handle previous match navigation.
   */
  private handleSearchPrev(): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    if (!state || !renderer) return;

    const match = this.searchStateManager.prevMatch();
    if (match) {
      renderer.setSearchHighlights(
        this.searchStateManager.matches,
        this.searchStateManager.currentMatchIndex,
      );
      this.searchBar?.updateCount(
        this.searchStateManager.currentMatchIndex,
        this.searchStateManager.matches.length,
      );
      this.scrollToCurrentMatch();
      renderer.forceRender(state);
    }
  }

  /**
   * Handle search bar close.
   */
  private handleSearchClose(): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    this.searchBar?.hide();
    this.searchStateManager.clear();
    renderer?.clearSearchHighlights();
    if (state && renderer) {
      renderer.forceRender(state);
    }
    // Return focus to terminal
    this.context.getImeHandler()?.focus();
  }

  /**
   * Scroll to make the current search match visible.
   */
  private scrollToCurrentMatch(): void {
    const state = this.context.getState();
    const renderer = this.context.getRenderer();
    if (!state || !renderer) return;

    const match = this.searchStateManager.getCurrentMatch();
    if (!match) return;

    // Auto-expand fold region if match is inside a collapsed region
    const foldManager = state.getFoldManager();
    foldManager.expandRegionContaining(match.lineIndex);

    const scrollbackLength = state.getScrollbackLength();
    const currentScrollOffset = renderer.getScrollOffset();
    const visibleStartLine = scrollbackLength - currentScrollOffset;
    const visibleEndLine = visibleStartLine + state.rows;

    // Check if match is visible
    if (match.lineIndex >= visibleStartLine && match.lineIndex < visibleEndLine) {
      return; // Already visible
    }

    // Scroll so the match is roughly centered in view
    const targetOffset = Math.max(0, scrollbackLength - match.lineIndex + Math.floor(state.rows / 2));
    renderer.setScrollOffset(targetOffset);
  }

  /**
   * Get all line texts (scrollback + screen buffer) for search.
   */
  private getAllLineTexts(): string[] {
    const state = this.context.getState();
    if (!state) return [];

    const lines: string[] = [];
    const scrollback = state.getScrollbackBuffer();
    const buffer = state.getActiveBuffer();

    // Scrollback lines
    for (const line of scrollback) {
      const chars: string[] = [];
      for (let c = 0; c < line.length; c++) {
        chars.push(line.getCell(c).char || " ");
      }
      lines.push(chars.join(""));
    }

    // Screen buffer lines
    for (let row = 0; row < state.rows; row++) {
      const line = buffer.getLine(row);
      const chars: string[] = [];
      for (let c = 0; c < line.length; c++) {
        chars.push(line.getCell(c).char || " ");
      }
      lines.push(chars.join(""));
    }

    return lines;
  }

  dispose(): void {
    this.searchBar?.dispose();
    this.searchBar = null;
  }
}
