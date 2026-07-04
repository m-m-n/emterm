/**
 * Shared type contracts for the Markdown front matter feature.
 *
 * This file is the single source of truth for the data exchanged between the
 * extractor (task0001), the parser (task0002), the block view (task0004), and
 * the renderer integration (task0005). Later-wave tasks import these types and
 * MUST NOT edit this file (IMPLEMENTATION.md D1).
 *
 * @module markdown/frontmatter/types
 */

/**
 * The three supported front matter formats, decided by the leading delimiter
 * (SPEC.md FR1). Exactly these three values — no others.
 */
export type FrontMatterFormat = "yaml" | "toml" | "json";

/**
 * A detected and extracted front matter block.
 */
export interface FrontMatterExtracted {
  /** Discriminant: a block was found. */
  found: true;
  /** The format decided by the leading delimiter. */
  format: FrontMatterFormat;
  /**
   * The raw block content. For YAML/TOML this is the exact text between the
   * opening and closing delimiter lines (delimiters excluded; the trailing
   * line break of the last content line is included). For JSON it is the
   * balanced object text including both braces. The extractor does not parse
   * this content (IMPLEMENTATION.md D5).
   */
  raw: string;
  /**
   * The source with the front matter block and its immediately following
   * newline removed (SPEC.md FR2). This is what reaches marked.
   */
  body: string;
}

/**
 * The outcome when no front matter block is present.
 */
export interface NoFrontMatter {
  /** Discriminant: no block was found. */
  found: false;
  /**
   * The original source, unmodified (reference-identical to the input where
   * possible — the FR7 / NFR4 passthrough fast path).
   */
  body: string;
}

/**
 * Result of front matter detection + extraction (SPEC.md FR1/FR2).
 * Produced by the extractor (task0001).
 */
export type FrontMatterExtraction = FrontMatterExtracted | NoFrontMatter;

/**
 * A plain JS value tree produced by parsing front matter content: object,
 * array, string, number, boolean, or null (SPEC.md FR3).
 */
export type FrontMatterValue =
  | string
  | number
  | boolean
  | null
  | FrontMatterValue[]
  | { [key: string]: FrontMatterValue };

/**
 * A successfully parsed front matter block.
 */
export interface FrontMatterParseSuccess {
  /** Discriminant: parsing succeeded. */
  ok: true;
  /** The parsed plain JS value tree. */
  value: FrontMatterValue;
  /**
   * True when normalization stopped early because the shared node budget
   * (`MAX_NODES`) was exhausted, so `value` is a bounded partial copy of the
   * parsed input (SPEC.md FR5). The view surfaces this as the same partial-tree
   * notice the tree builder uses. Absent/`false` means the value is complete.
   */
  truncated?: boolean;
}

/**
 * A front matter block that failed to parse (SPEC.md FR6).
 */
export interface FrontMatterParseFailure {
  /** Discriminant: parsing failed. */
  ok: false;
  /** The parser's error message. */
  error: string;
}

/**
 * Result of parsing front matter content (SPEC.md FR3/FR6).
 * Produced by the parser (task0002); defined here as a shared contract.
 */
export type FrontMatterParseResult =
  | FrontMatterParseSuccess
  | FrontMatterParseFailure;
