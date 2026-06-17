/**
 * Search state management for in-terminal text search.
 *
 * Provides incremental search across scrollback and screen buffers
 * with support for plain text, regex, and case sensitivity options.
 */

export interface SearchMatch {
  lineIndex: number;
  startCol: number;
  endCol: number; // Exclusive
}

export interface SearchOptions {
  isRegex: boolean;
  caseSensitive: boolean;
}

/** Maximum search execution time in milliseconds. */
const SEARCH_TIMEOUT_MS = 200;

/** Lines between timeout checks. */
const TIMEOUT_CHECK_INTERVAL = 100;

/** Maximum line length to search (ReDoS protection for single-line backtracking). */
const MAX_SEARCH_LINE_LENGTH = 10000;

/**
 * Manages search state including query, options, matches, and navigation.
 */
export class SearchStateManager {
  query: string = "";
  options: SearchOptions = { isRegex: false, caseSensitive: false };
  matches: SearchMatch[] = [];
  currentMatchIndex: number = -1;
  error: string | null = null;

  /**
   * Set the search query.
   */
  setQuery(query: string): void {
    this.query = query;
  }

  /**
   * Update search options.
   */
  setOptions(options: Partial<SearchOptions>): void {
    if (options.isRegex !== undefined) this.options.isRegex = options.isRegex;
    if (options.caseSensitive !== undefined)
      this.options.caseSensitive = options.caseSensitive;
  }

  /**
   * Execute search across all lines.
   *
   * @param lines - Array of line text strings to search
   */
  executeSearch(lines: string[]): void {
    this.matches = [];
    this.currentMatchIndex = -1;
    this.error = null;

    if (!this.query) {
      return;
    }

    let pattern: RegExp;
    try {
      if (this.options.isRegex) {
        const flags = this.options.caseSensitive ? "g" : "gi";
        pattern = new RegExp(this.query, flags);
      } else {
        const escaped = this.query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        const flags = this.options.caseSensitive ? "g" : "gi";
        pattern = new RegExp(escaped, flags);
      }
    } catch {
      this.error = "Invalid regex pattern";
      return;
    }

    const startTime = performance.now();

    for (let i = 0; i < lines.length; i++) {
      // Timeout check
      if (
        i > 0 &&
        i % TIMEOUT_CHECK_INTERVAL === 0 &&
        performance.now() - startTime > SEARCH_TIMEOUT_MS
      ) {
        this.error = "Search timed out";
        this.matches = [];
        return;
      }

      let lineText = lines[i];
      if (!lineText) continue;

      // Truncate very long lines to prevent ReDoS within a single regex exec call
      if (lineText.length > MAX_SEARCH_LINE_LENGTH) {
        lineText = lineText.slice(0, MAX_SEARCH_LINE_LENGTH);
      }

      pattern.lastIndex = 0;
      let match: RegExpExecArray | null;

      while ((match = pattern.exec(lineText)) !== null) {
        if (match[0].length === 0) {
          // Prevent infinite loop on zero-length matches
          pattern.lastIndex++;
          continue;
        }

        this.matches.push({
          lineIndex: i,
          startCol: match.index,
          endCol: match.index + match[0].length,
        });
      }
    }

    if (this.matches.length > 0) {
      this.currentMatchIndex = 0;
    }
  }

  /**
   * Move to the next match (wraps around).
   */
  nextMatch(): SearchMatch | null {
    if (this.matches.length === 0) return null;

    this.currentMatchIndex =
      (this.currentMatchIndex + 1) % this.matches.length;
    return this.matches[this.currentMatchIndex] ?? null;
  }

  /**
   * Move to the previous match (wraps around).
   */
  prevMatch(): SearchMatch | null {
    if (this.matches.length === 0) return null;

    this.currentMatchIndex =
      (this.currentMatchIndex - 1 + this.matches.length) % this.matches.length;
    return this.matches[this.currentMatchIndex] ?? null;
  }

  /**
   * Get matches visible within a line range.
   *
   * @param startLine - Start line index (inclusive)
   * @param endLine - End line index (exclusive)
   */
  getVisibleMatches(startLine: number, endLine: number): SearchMatch[] {
    return this.matches.filter(
      (m) => m.lineIndex >= startLine && m.lineIndex < endLine,
    );
  }

  /**
   * Get the current match.
   */
  getCurrentMatch(): SearchMatch | null {
    if (
      this.currentMatchIndex < 0 ||
      this.currentMatchIndex >= this.matches.length
    ) {
      return null;
    }
    return this.matches[this.currentMatchIndex] ?? null;
  }

  /**
   * Invalidate search results (called on buffer changes).
   */
  invalidate(): void {
    this.matches = [];
    this.currentMatchIndex = -1;
  }

  /**
   * Clear all search state.
   */
  clear(): void {
    this.query = "";
    this.matches = [];
    this.currentMatchIndex = -1;
    this.error = null;
  }
}
