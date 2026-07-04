/**
 * Shared bounds for front matter processing.
 *
 * Neutral home for the node/depth caps so both the parse/normalization layer
 * (`parser.ts`) and the view-tree layer (`tree-builder.ts`) depend on a common
 * policy module instead of one importing the other. This keeps the parse layer
 * from depending on the display tree-builder (finding 31b07c8b): the budget is
 * a shared policy, not something the view layer owns and the parser inherits.
 *
 * The values match SPEC.md and are unchanged from where they previously lived
 * in `tree-builder.ts`.
 *
 * This module imports nothing of its own.
 *
 * @module markdown/frontmatter/limits
 */

/**
 * Maximum recursion depth; bounds adversarially deep input so a pathologically
 * nested document cannot exhaust the stack or force unbounded work.
 */
export const MAX_DEPTH = 128;

/**
 * Maximum total number of nodes/rows emitted for one document. The depth cap
 * alone cannot bound a shallow-but-very-wide document (millions of top-level
 * keys), so the total count is capped as well to keep terminal-controlled front
 * matter from forcing unbounded eager work (security hardening). Chosen well
 * above any realistic hand-written front matter so legitimate documents are
 * never truncated in practice.
 */
export const MAX_NODES = 2000;

/**
 * Maximum raw front matter size, in UTF-8 bytes, that may be handed to a parser
 * library. Raw content larger than this is rejected as a parse failure BEFORE
 * any parser is invoked, so terminal-controlled front matter cannot force the
 * YAML/TOML/JSON parser into a long synchronous parse that blocks the WebView
 * (SPEC.md Security Considerations). The whole OSC session is already bounded on
 * the Rust side (100 MiB); this is the pre-parse bound a single front matter
 * block may reach the parser with.
 *
 * Chosen well above any realistic hand-written front matter (1 MiB) so
 * legitimate documents are never rejected, yet far below anything that stalls a
 * parse. Measured in bytes rather than UTF-16 code units so the bound does not
 * depend on the script the content is written in (a CJK document reaches the
 * same byte ceiling as an ASCII one).
 */
export const MAX_RAW_BYTES = 1024 * 1024;

/**
 * Maximum length, in UTF-16 code units, of raw front matter text rendered into
 * the error-state `<pre>`.
 *
 * This is a UI/display bound, distinct from the pre-parse bound `MAX_RAW_BYTES`:
 * `MAX_RAW_BYTES` keeps a parser from being handed an oversized block, whereas
 * this bound keeps the DOM from being handed one. task0009 routes raw larger
 * than `MAX_RAW_BYTES` to the FR6 error path, where the view would otherwise
 * assign the entire raw string (up to the Rust-side 100 MiB session cap) to a
 * text node — a very large `<pre>` can freeze or OOM the WebView through layout
 * even though no parser ran, so the DoS vector would merely have moved from
 * parsing to DOM layout (SPEC.md Security Considerations). The view clamps the
 * raw to this length with a truncation marker before it enters the DOM, so
 * terminal-controlled front matter can never stream unbounded text into layout.
 *
 * Measured in code units (what `String.slice`/`String.length` and the DOM text
 * node operate on) rather than bytes; the bound only needs to keep the `<pre>`
 * small enough that its layout is cheap. Chosen well above any raw a human would
 * read to diagnose a parse error (64 KiB) so real errors still show full
 * context, yet far below anything that stalls layout.
 */
export const MAX_ERROR_RAW_DISPLAY_CHARS = 64 * 1024;
