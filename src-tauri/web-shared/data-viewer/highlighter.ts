/**
 * Syntax highlighter for JSON/YAML.
 *
 * Token-based highlighting that produces HTML with CSS classes.
 *
 * @module data-viewer/highlighter
 */

import DOMPurify from "dompurify";
import type { DataFormat } from "./types.ts";

/**
 * Highlight JSON text and return sanitized HTML.
 */
export function highlightJson(text: string): string {
  // Use regex-based tokenization for JSON
  const html = text.replace(
    /("(?:[^"\\]|\\.)*")\s*(:)|("(?:[^"\\]|\\.)*")|((?:-?\d+\.?\d*(?:[eE][+-]?\d+)?)(?=[,\s\]\}]|$))|(true|false)|(null)|([{}\[\],:])/g,
    (
      _match: string,
      keyStr: string | undefined,
      colon: string | undefined,
      str: string | undefined,
      num: string | undefined,
      bool: string | undefined,
      nullVal: string | undefined,
      punct: string | undefined,
    ) => {
      if (keyStr && colon) {
        return `<span class="dv-key">${escapeHtml(keyStr)}</span><span class="dv-punct">:</span>`;
      }
      if (str) return `<span class="dv-string">${escapeHtml(str)}</span>`;
      if (num) return `<span class="dv-number">${escapeHtml(num)}</span>`;
      if (bool) return `<span class="dv-boolean">${escapeHtml(bool)}</span>`;
      if (nullVal) return `<span class="dv-null">${escapeHtml(nullVal)}</span>`;
      if (punct) return `<span class="dv-punct">${escapeHtml(punct)}</span>`;
      return escapeHtml(_match);
    },
  );

  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: ["span"],
    ALLOWED_ATTR: ["class"],
  });
}

/**
 * Highlight YAML text and return sanitized HTML.
 */
export function highlightYaml(text: string): string {
  const lines = text.split("\n");
  const htmlLines = lines.map((line) => {
    // Comment lines
    if (/^\s*#/.test(line)) {
      return `<span class="dv-comment">${escapeHtml(line)}</span>`;
    }

    // Key: value pattern
    const kvMatch = line.match(/^(\s*)([\w.-]+(?:\s[\w.-]+)*)(\s*:\s*)(.*)?$/);
    if (kvMatch) {
      const [, indent, key, colon, rawValue] = kvMatch;
      let valuePart = "";
      if (rawValue !== undefined && rawValue !== "") {
        valuePart = highlightYamlValue(rawValue);
      }
      return `${escapeHtml(indent!)}<span class="dv-key">${escapeHtml(key!)}</span><span class="dv-punct">${escapeHtml(colon!)}</span>${valuePart}`;
    }

    // List items: - value
    const listMatch = line.match(/^(\s*-\s+)(.*)?$/);
    if (listMatch) {
      const [, prefix, rawValue] = listMatch;
      let valuePart = "";
      if (rawValue !== undefined && rawValue !== "") {
        valuePart = highlightYamlValue(rawValue);
      }
      return `<span class="dv-punct">${escapeHtml(prefix!)}</span>${valuePart}`;
    }

    // Document separators
    if (/^---\s*$/.test(line) || /^\.\.\.\s*$/.test(line)) {
      return `<span class="dv-punct">${escapeHtml(line)}</span>`;
    }

    return escapeHtml(line);
  });

  const html = htmlLines.join("\n");
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS: ["span"],
    ALLOWED_ATTR: ["class"],
  });
}

function highlightYamlValue(value: string): string {
  const trimmed = value.trim();

  // Quoted strings
  if (/^".*"$/.test(trimmed) || /^'.*'$/.test(trimmed)) {
    return `<span class="dv-string">${escapeHtml(value)}</span>`;
  }
  // Boolean
  if (/^(true|false|yes|no|on|off)$/i.test(trimmed)) {
    return `<span class="dv-boolean">${escapeHtml(value)}</span>`;
  }
  // Null
  if (/^(null|~)$/i.test(trimmed)) {
    return `<span class="dv-null">${escapeHtml(value)}</span>`;
  }
  // Number
  if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(trimmed)) {
    return `<span class="dv-number">${escapeHtml(value)}</span>`;
  }
  // Inline comment after value
  const commentIdx = value.indexOf(" #");
  if (commentIdx > 0) {
    const val = value.substring(0, commentIdx);
    const comment = value.substring(commentIdx);
    return `<span class="dv-string">${escapeHtml(val)}</span><span class="dv-comment">${escapeHtml(comment)}</span>`;
  }

  // Plain string
  return `<span class="dv-string">${escapeHtml(value)}</span>`;
}

/**
 * Highlight text based on format.
 */
export function highlightData(text: string, format: DataFormat): string {
  if (format === "json") {
    return highlightJson(text);
  }
  return highlightYaml(text);
}

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
