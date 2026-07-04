/**
 * Front matter detection + extraction (SPEC.md FR1/FR2).
 *
 * DOM-free pure logic. Given a Markdown source string, decide whether it starts
 * with a YAML (`---`) / TOML (`+++`) / JSON (`{...}`) front matter block and,
 * if so, return the raw block content, its format, and the body with the block
 * (plus its immediately following newline) stripped.
 *
 * This module checks delimiter/brace integrity only — it never parses the block
 * content (IMPLEMENTATION.md D5). A well-delimited block whose content is
 * garbage is still extracted; the parser (task0002) decides validity.
 *
 * @module markdown/frontmatter/extractor
 */

import type { FrontMatterExtraction, FrontMatterFormat } from "./types.ts";

/** UTF-8 byte order mark, tolerated before the leading delimiter. */
const BOM = "﻿";

const YAML_DELIMITER = "---";
const TOML_DELIMITER = "+++";

/**
 * Detect and extract a single front matter block at the very start of `source`.
 *
 * Returns a `found: false` result (with `body` reference-identical to `source`)
 * when there is no front matter, an unterminated delimited block, or an
 * unbalanced JSON brace (SPEC.md FR1 rules 5–6).
 */
export function extractFrontMatter(source: string): FrontMatterExtraction {
  // Tolerate a leading UTF-8 BOM before the delimiter.
  const start = source.startsWith(BOM) ? BOM.length : 0;
  const first = source[start];

  if (first === "{") {
    return extractJson(source, start) ?? noFrontMatter(source);
  }

  return (
    extractDelimited(source, start, YAML_DELIMITER, "yaml") ??
    extractDelimited(source, start, TOML_DELIMITER, "toml") ??
    noFrontMatter(source)
  );
}

/** The "no front matter" result: the original source, unmodified. */
function noFrontMatter(source: string): FrontMatterExtraction {
  return { found: false, body: source };
}

/**
 * True when `line` (already stripped of its terminating newline) is exactly the
 * delimiter, allowing trailing whitespace and a trailing carriage return
 * (CRLF). Leading whitespace is NOT allowed — the delimiter must be the first
 * character of the line.
 */
function isDelimiterLine(line: string, delimiter: string): boolean {
  return line.trimEnd() === delimiter;
}

/**
 * Extract a `---` / `+++` delimited block. Returns null when the first line is
 * not the opening delimiter or no matching closing delimiter line is found.
 */
function extractDelimited(
  source: string,
  start: number,
  delimiter: string,
  format: FrontMatterFormat,
): FrontMatterExtraction | null {
  const firstNewline = source.indexOf("\n", start);
  // An opening delimiter with no following newline can have no closing line.
  if (firstNewline === -1) {
    return null;
  }
  const firstLine = source.slice(start, firstNewline);
  if (!isDelimiterLine(firstLine, delimiter)) {
    return null;
  }

  // Content begins on the line after the opening delimiter.
  const contentStart = firstNewline + 1;
  let lineStart = contentStart;
  for (;;) {
    const newline = source.indexOf("\n", lineStart);
    const lineEnd = newline === -1 ? source.length : newline;
    const line = source.slice(lineStart, lineEnd);

    if (isDelimiterLine(line, delimiter)) {
      // Raw content is the exact text between the two delimiter lines.
      const raw = source.slice(contentStart, lineStart);
      // Body starts after the closing delimiter line's newline (if any).
      const bodyStart = newline === -1 ? source.length : newline + 1;
      return { found: true, format, raw, body: source.slice(bodyStart) };
    }

    if (newline === -1) {
      // Reached end of input without a closing delimiter.
      return null;
    }
    lineStart = newline + 1;
  }
}

/**
 * Extract a bare JSON object block (`source[start]` is `{`). Balances braces
 * while skipping over JSON string literals (double-quoted, backslash escapes
 * honored). Returns null when the object never closes (unbalanced brace).
 */
function extractJson(
  source: string,
  start: number,
): FrontMatterExtraction | null {
  let depth = 0;
  let inString = false;
  let escaped = false;
  let end = -1;

  for (let i = start; i < source.length; i++) {
    const ch = source[i];

    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (ch === "\\") {
        escaped = true;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      inString = true;
    } else if (ch === "{") {
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }

  if (end === -1) {
    // Unbalanced brace — treat the whole source as body (FR1 rule 6).
    return null;
  }

  // Raw content includes both braces.
  const raw = source.slice(start, end + 1);

  // Strip the block plus its immediately following newline (CRLF tolerated).
  let bodyStart = end + 1;
  if (source[bodyStart] === "\r") {
    bodyStart++;
  }
  if (source[bodyStart] === "\n") {
    bodyStart++;
  }

  return { found: true, format: "json", raw, body: source.slice(bodyStart) };
}
