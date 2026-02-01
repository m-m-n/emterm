/**
 * URL detection for terminal output.
 *
 * Detects URLs in text lines and returns their positions.
 */

/** A detected URL match with position information. */
export interface UrlMatch {
  /** Start column (0-based, inclusive). */
  startCol: number;
  /** End column (0-based, exclusive). */
  endCol: number;
  /** The matched URL string. */
  url: string;
}

/** URL pattern matching common protocols. */
const URL_REGEX = /(?:https?|ftp|file):\/\/[^\s<>"'`)\]},;]+/g;

/**
 * Detect URLs in a text line.
 *
 * @param text - The text line to scan
 * @returns Array of URL matches with positions
 */
export function detectUrls(text: string): UrlMatch[] {
  const matches: UrlMatch[] = [];
  URL_REGEX.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = URL_REGEX.exec(text)) !== null) {
    // Trim trailing punctuation that's likely not part of the URL
    let url = match[0];
    while (url.length > 0 && /[.,;:!?)}\]>]$/.test(url)) {
      url = url.slice(0, -1);
    }

    matches.push({
      startCol: match.index,
      endCol: match.index + url.length,
      url,
    });
  }

  return matches;
}

/**
 * Find a URL at a specific column position in a text line.
 *
 * @param text - The text line to scan
 * @param col - The column to check (0-based)
 * @returns The URL at the position, or null if none
 */
export function findUrlAtPosition(text: string, col: number): string | null {
  const matches = detectUrls(text);
  for (const m of matches) {
    if (col >= m.startCol && col < m.endCol) {
      return m.url;
    }
  }
  return null;
}
