/**
 * Front matter content parsing + format dispatch (SPEC.md FR3/FR6).
 *
 * DOM-free pure logic (IMPLEMENTATION.md D2). Given the raw content of a front
 * matter block and its format (both from the extractor's contract in
 * `types.ts`, task0001), produce either a plain JS value tree or a captured
 * parse failure.
 *
 * The function NEVER throws: any exception from the underlying parser library
 * is caught and returned as a failure carrying the parser's message
 * (IMPLEMENTATION.md D5). Library-specific value types (e.g. TOML date/time
 * values) are normalized to plain strings/numbers so the tree builder and view
 * only ever see plain JS values.
 *
 * Library selection (task0002 decision):
 * - YAML: `yaml` (eemeli) — already a project dependency, so no added bundle
 *   weight, well maintained, informative error messages.
 * - TOML: `smol-toml` — the smallest adequate TOML 1.0 parser, pure ESM,
 *   zero-dependency, bun-bundler friendly (over the larger, older
 *   `@iarna/toml`).
 *
 * @module markdown/frontmatter/parser
 */

import { parse as parseToml } from "smol-toml";
import { parse as parseYaml } from "yaml";

import type {
  FrontMatterFormat,
  FrontMatterParseResult,
  FrontMatterValue,
} from "./types.ts";

/**
 * Parse raw front matter `content` according to `format`.
 *
 * Empty or whitespace-only content is a successful empty (`null`) tree so the
 * empty-block case (FR4) displays as an empty tree rather than an error.
 *
 * @param content - Raw block content (delimiters already excluded by the extractor).
 * @param format - The format decided by the extractor.
 * @returns A success carrying a plain JS value tree, or a failure carrying the
 *   parser's error message. Never throws.
 */
export function parseFrontMatter(
  content: string,
  format: FrontMatterFormat,
): FrontMatterParseResult {
  // An empty or whitespace-only block is a valid empty tree, not an error.
  // (The extractor can yield empty raw content, e.g. a `---`/`---` YAML block.)
  if (content.trim() === "") {
    return { ok: true, value: null };
  }

  try {
    const raw = parseByFormat(content, format);
    return { ok: true, value: normalizeValue(raw) };
  } catch (err) {
    return { ok: false, error: toErrorMessage(err) };
  }
}

/** Dispatch to the concrete parser for `format`. May throw; the caller catches. */
function parseByFormat(content: string, format: FrontMatterFormat): unknown {
  switch (format) {
    case "yaml":
      return parseYaml(content);
    case "toml":
      return parseToml(content);
    case "json":
      return JSON.parse(content);
  }
}

/** Placeholder substituted for a value that points back to one of its ancestors. */
const CIRCULAR_PLACEHOLDER = "[Circular]";

/**
 * Recursively convert a parsed value into a plain JS value tree.
 *
 * `undefined` (e.g. a YAML document that parses to nothing) becomes `null`.
 * `Date` instances — including `smol-toml`'s `TomlDate` (which extends `Date`)
 * — become their ISO string. `bigint` values become numbers. Plain objects and
 * arrays are rebuilt element-by-element so no library-specific prototype leaks
 * into the tree.
 *
 * The YAML parser resolves anchors/aliases and can hand back a genuinely cyclic
 * object graph (`v.a.b === v.a`). `ancestors` tracks the objects/arrays on the
 * current path so a reference back to an ancestor is replaced by
 * {@link CIRCULAR_PLACEHOLDER} instead of being chased into stack exhaustion.
 * Nodes are removed from the set on the way out, so a value that is merely
 * *shared* between siblings (an acyclic alias) is still fully expanded.
 */
function normalizeValue(
  value: unknown,
  ancestors: WeakSet<object> = new WeakSet(),
): FrontMatterValue {
  if (value === null || value === undefined) {
    return null;
  }

  const t = typeof value;
  if (t === "string" || t === "boolean") {
    return value as string | boolean;
  }
  if (t === "number") {
    return value as number;
  }
  if (t === "bigint") {
    return Number(value);
  }
  if (value instanceof Date) {
    // smol-toml's TomlDate.toISOString() yields the correct plain-string form
    // for date-only / time-only / date-time TOML values.
    return value.toISOString();
  }

  if (Array.isArray(value) || t === "object") {
    const container = value as object;
    // A reference back to an ancestor closes a cycle — stop before recursing.
    if (ancestors.has(container)) {
      return CIRCULAR_PLACEHOLDER;
    }
    ancestors.add(container);
    try {
      if (Array.isArray(value)) {
        return value.map((v) => normalizeValue(v, ancestors));
      }
      const out: { [key: string]: FrontMatterValue } = {};
      for (const [key, v] of Object.entries(value as Record<string, unknown>)) {
        out[key] = normalizeValue(v, ancestors);
      }
      return out;
    } finally {
      // Leaving this node: a sibling may legitimately reference the same value
      // without it being a cycle, so it must not stay "seen".
      ancestors.delete(container);
    }
  }

  // Functions, symbols, and any other exotic value: coerce to a string so the
  // tree never carries a non-plain value.
  return String(value);
}

/** Extract a non-empty error message from an unknown thrown value. */
function toErrorMessage(err: unknown): string {
  const message =
    err instanceof Error
      ? err.message
      : typeof err === "string"
        ? err
        : String(err);
  return message.length > 0 ? message : "Unknown parse error";
}
