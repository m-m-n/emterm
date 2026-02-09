/**
 * URL and file path detection for terminal output.
 *
 * Detects URLs and file paths with line numbers in text lines
 * and returns their positions.
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

/** A detected file path match with position and location information. */
export interface FilePathMatch {
  /** Start column (0-based, inclusive). */
  startCol: number;
  /** End column (0-based, exclusive). */
  endCol: number;
  /** The file path (without line/col suffix). */
  path: string;
  /** Line number (1-based). */
  line: number;
  /** Column number (1-based, defaults to 1 if not specified). */
  col: number;
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

/**
 * File path pattern matching paths with line numbers.
 *
 * Matches patterns like:
 *   src/foo.ts:42
 *   src/foo.ts:42:10
 *   /home/user/file.rs:10
 *   ./src/foo.ts:42
 *   ../lib/bar.py:5:3
 *
 * Does NOT match:
 *   http://example.com:8080 (URL)
 *   12:30:45 (time pattern)
 *   foo.ts (no line number)
 */
const FILE_PATH_REGEX =
  /(?:\.?\.\/)?\/?(?:[a-zA-Z0-9_@.-]+\/)*[a-zA-Z0-9_@.-]+\.[a-zA-Z0-9]+:\d+(?::\d+)?/g;

/** URL protocol pattern to exclude matches that are part of URLs. */
const URL_PROTOCOL_PREFIX = /(?:https?|ftp|file):\/\//;

/**
 * Detect file paths with line numbers in a text line.
 *
 * @param text - The text line to scan
 * @returns Array of file path matches with positions
 */
export function detectFilePaths(text: string): FilePathMatch[] {
  const matches: FilePathMatch[] = [];
  FILE_PATH_REGEX.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = FILE_PATH_REGEX.exec(text)) !== null) {
    let raw = match[0];
    const startCol = match.index;

    // Check if this match is part of a URL (look back for protocol prefix)
    const textBefore = text.substring(0, startCol);
    if (URL_PROTOCOL_PREFIX.test(textBefore + raw.charAt(0))) {
      // Check more precisely: is there a protocol immediately before this match?
      const protocolMatch = text
        .substring(0, startCol + raw.length)
        .match(/(?:https?|ftp|file):\/\/\S*$/);
      if (protocolMatch) {
        const protocolStart = text.lastIndexOf(
          protocolMatch[0],
          startCol + raw.length,
        );
        if (protocolStart >= 0 && protocolStart < startCol) {
          continue;
        }
      }
    }

    // Trim trailing punctuation (common in error messages like "src/foo.ts:42.")
    while (raw.length > 0 && /[.,;:!?)}\]>]$/.test(raw)) {
      raw = raw.slice(0, -1);
    }

    // Must still contain ':' after trimming (the line number separator)
    const colonIdx = raw.indexOf(":");
    if (colonIdx < 0) continue;

    // Split into path and line:col parts
    const path = raw.substring(0, colonIdx);
    const rest = raw.substring(colonIdx + 1);

    // Must have a path component with at least one '/' or start with './' or '../' or '/'
    // (to distinguish from bare filenames or time patterns)
    if (
      !path.includes("/") &&
      !path.startsWith("./") &&
      !path.startsWith("../") &&
      !path.startsWith("/")
    ) {
      continue;
    }

    // Parse line and optional column
    const parts = rest.split(":");
    const linePart = parts[0] ?? "";
    const line = parseInt(linePart, 10);
    if (isNaN(line) || line <= 0) continue;

    let col = 1;
    if (parts.length >= 2) {
      const colPart = parts[1] ?? "";
      const parsedCol = parseInt(colPart, 10);
      if (!isNaN(parsedCol) && parsedCol > 0) {
        col = parsedCol;
      }
    }

    matches.push({
      startCol,
      endCol: startCol + raw.length,
      path,
      line,
      col,
    });
  }

  return matches;
}

/**
 * Find a file path at a specific column position in a text line.
 *
 * @param text - The text line to scan
 * @param col - The column to check (0-based)
 * @returns The file path match at the position, or null if none
 */
export function findFilePathAtPosition(
  text: string,
  col: number,
): FilePathMatch | null {
  const matches = detectFilePaths(text);
  for (const m of matches) {
    if (col >= m.startCol && col < m.endCol) {
      return m;
    }
  }
  return null;
}
