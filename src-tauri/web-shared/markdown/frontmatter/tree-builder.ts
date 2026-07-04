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

/** Maximum recursion depth; bounds adversarially deep input. */
const MAX_DEPTH = 128;

/**
 * Build a flat array of {@link TreeNode} from a parsed value, in display order.
 *
 * A container root (a non-null object or array) contributes its children
 * recursively, one node per key/element at every nesting level. A
 * non-container root (scalar, null, or undefined) yields an empty array — a
 * defined result the view renders as an empty tree.
 *
 * @param data - Parsed front matter value (object / array / scalar / null).
 * @returns Flat array of nodes in display order (empty for a non-container root).
 */
export function buildTree(data: unknown): TreeNode[] {
  const nodes: TreeNode[] = [];
  if (data !== null && typeof data === "object") {
    addChildren(data, 0, "", nodes);
  }
  return nodes;
}

/**
 * Append a node for every key/element of `data`, recursing into containers.
 *
 * Recursion stops once `depth` reaches {@link MAX_DEPTH}, so nodes are only
 * ever emitted at depths `0..MAX_DEPTH - 1`.
 */
function addChildren(
  data: unknown,
  depth: number,
  parentPath: string,
  nodes: TreeNode[],
): void {
  if (depth >= MAX_DEPTH) return;

  if (Array.isArray(data)) {
    for (let i = 0; i < data.length; i++) {
      const key = `[${i}]`;
      const path = parentPath ? `${parentPath}${key}` : key;
      const value = data[i];
      const hasChildren = value !== null && typeof value === "object";
      nodes.push({ key, depth, path, value, hasChildren });
      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes);
      }
    }
  } else if (data !== null && typeof data === "object") {
    for (const [key, value] of Object.entries(
      data as Record<string, unknown>,
    )) {
      const path = parentPath ? `${parentPath}.${key}` : key;
      const hasChildren = value !== null && typeof value === "object";
      nodes.push({ key, depth, path, value, hasChildren });
      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes);
      }
    }
  }
}
