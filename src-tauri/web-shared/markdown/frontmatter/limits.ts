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
