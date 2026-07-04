/**
 * Build a flat, display-ordered node list from a parsed front matter value.
 *
 * DOM-free pure logic (IMPLEMENTATION.md D2). Ported from the legacy WebView
 * data viewer (`legacy/webview` `src/data-viewer/tree-builder.ts`) and made
 * self-contained: it defines and exports its own {@link TreeNode} type rather
 * than importing from `types.ts` (IMPLEMENTATION.md D4), so this module has no
 * cross-task file dependency.
 *
 * @module markdown/frontmatter/tree-builder
 */

import { MAX_DEPTH, MAX_NODES } from "./limits.ts";

// Re-exported so existing consumers (view.ts, tests) keep a stable import site;
// the value itself lives in the neutral limits module (finding 31b07c8b).
export { MAX_NODES } from "./limits.ts";

/** A single row of the always-fully-expanded front matter tree. */
export interface TreeNode {
  /** Object key, or `[i]` for an array element. */
  key: string;
  /** Nesting depth: 0 at the top level, +1 per level. */
  depth: number;
  /**
   * Path from the root: dot-joined object keys with bracketed array indexes
   * (e.g. `server.host`, `items[0].name`).
   */
  path: string;
  /** The value at this node. */
  value: unknown;
  /** True for non-null objects/arrays (nodes that contribute children). */
  hasChildren: boolean;
}

/** The outcome of building a tree: the (possibly capped) rows plus whether
 * either cap stopped the walk before the whole value was emitted. */
export interface TreeBuildResult {
  /** Flat node array in display order (at most {@link MAX_NODES} entries). */
  nodes: TreeNode[];
  /**
   * True when either the node budget ({@link MAX_NODES}) or the depth cap
   * ({@link MAX_DEPTH}) dropped part of the value (the tree is partial), so the
   * view surfaces the same partial-tree notice for either cap (SPEC.md FR5).
   */
  truncated: boolean;
}

/**
 * Build a flat array of {@link TreeNode} from a parsed value, in display order.
 *
 * A container root (a non-null object or array) contributes its children
 * recursively, one node per key/element at every nesting level. A
 * non-container root (scalar, null, or undefined) yields an empty array — a
 * defined result the view renders as an empty tree.
 *
 * Emission stops at {@link MAX_NODES} total rows; when that happens `truncated`
 * is `true` so the view can surface a partial-tree notice.
 *
 * @param data - Parsed front matter value (object / array / scalar / null).
 * @returns The display-order nodes (capped at the budget) and a truncation flag.
 */
export function buildTree(data: unknown): TreeBuildResult {
  const nodes: TreeNode[] = [];
  const state = { truncated: false };
  if (data !== null && typeof data === "object") {
    addChildren(data, 0, "", nodes, state);
  }
  return { nodes, truncated: state.truncated };
}

/**
 * Append a node for every key/element of `data`, recursing into containers.
 *
 * Recursion stops once `depth` reaches {@link MAX_DEPTH}, so nodes are only
 * ever emitted at depths `0..MAX_DEPTH - 1`. Emission also stops once
 * {@link MAX_NODES} rows have been produced. Either cap records the truncation
 * in `state` so the caller can render the same partial-tree notice (SPEC.md
 * FR5) — a depth cap is no longer silent.
 */
function addChildren(
  data: unknown,
  depth: number,
  parentPath: string,
  nodes: TreeNode[],
  state: { truncated: boolean },
): void {
  if (depth >= MAX_DEPTH) {
    // The depth cap stops the walk here. If this container still holds entries,
    // its descendants are dropped — flag the tree partial so the notice fires,
    // exactly like the node budget. An empty container drops nothing, so it is
    // not flagged (no spurious notice on a complete-but-deeply-nested tree).
    const hasEntries = Array.isArray(data)
      ? data.length > 0
      : data !== null &&
        typeof data === "object" &&
        Object.keys(data as Record<string, unknown>).length > 0;
    if (hasEntries) state.truncated = true;
    return;
  }

  if (Array.isArray(data)) {
    for (let i = 0; i < data.length; i++) {
      if (nodes.length >= MAX_NODES) {
        state.truncated = true;
        return;
      }
      const key = `[${i}]`;
      const path = parentPath ? `${parentPath}${key}` : key;
      const value = data[i];
      const hasChildren = value !== null && typeof value === "object";
      nodes.push({ key, depth, path, value, hasChildren });
      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes, state);
      }
    }
  } else if (data !== null && typeof data === "object") {
    for (const [key, value] of Object.entries(
      data as Record<string, unknown>,
    )) {
      if (nodes.length >= MAX_NODES) {
        state.truncated = true;
        return;
      }
      const path = parentPath ? `${parentPath}.${key}` : key;
      const hasChildren = value !== null && typeof value === "object";
      nodes.push({ key, depth, path, value, hasChildren });
      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes, state);
      }
    }
  }
}
