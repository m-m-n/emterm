/**
 * JSON/YAML parser with error handling.
 *
 * @module data-viewer/parser
 */

import YAML from "yaml";
import type { DataFormat, ParseResult } from "./types.ts";

/**
 * Parse raw text as JSON or YAML.
 *
 * @param rawText - Raw text content
 * @param format - Data format to parse as
 * @returns ParseResult with parsed data or error message
 */
export function parseData(rawText: string, format: DataFormat): ParseResult {
  try {
    if (format === "json") {
      const data = JSON.parse(rawText);
      return { ok: true, data, rawText };
    } else {
      const data = YAML.parse(rawText);
      return { ok: true, data, rawText };
    }
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    return { ok: false, error, rawText };
  }
}

/**
 * Pretty-print JSON data.
 *
 * @param data - Parsed JSON data
 * @returns Formatted JSON string with 2-space indentation
 */
export function prettyPrintJson(data: unknown): string {
  return JSON.stringify(data, null, 2);
}

/**
 * Serialize a value back to JSON or YAML string.
 *
 * @param data - Data to serialize
 * @param format - Output format
 * @returns Formatted string
 */
export function serializeData(data: unknown, format: DataFormat): string {
  if (format === "json") {
    return JSON.stringify(data, null, 2);
  } else {
    return YAML.stringify(data, { indent: 2 });
  }
}
