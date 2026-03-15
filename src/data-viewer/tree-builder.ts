/**
 * Build tree nodes from parsed data for the outline view.
 *
 * @module data-viewer/tree-builder
 */

import type { TreeNode } from "./types.ts";

/** Maximum recursion depth to prevent stack overflow on adversarial input */
const MAX_DEPTH = 128;

/**
 * Build a flat array of tree nodes from parsed data.
 *
 * Recursively traverses the data structure, creating a node for each
 * key at every nesting level. Array elements use index-based keys.
 *
 * @param data - Parsed JSON/YAML data
 * @returns Flat array of TreeNode in display order
 */
export function buildTree(data: unknown): TreeNode[] {
  const nodes: TreeNode[] = [];

  // Root node representing the entire document
  const rootIsContainer =
    data !== null && typeof data === "object";

  if (rootIsContainer) {
    // Add children directly at root level
    addChildren(data, 0, "", nodes);
  }

  return nodes;
}

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
      const hasChildren =
        value !== null && typeof value === "object";

      nodes.push({ key, depth, path, value, hasChildren });

      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes);
      }
    }
  } else if (data !== null && typeof data === "object") {
    for (const [key, value] of Object.entries(data as Record<string, unknown>)) {
      const path = parentPath ? `${parentPath}.${key}` : key;
      const hasChildren =
        value !== null && typeof value === "object";

      nodes.push({ key, depth, path, value, hasChildren });

      if (hasChildren) {
        addChildren(value, depth + 1, path, nodes);
      }
    }
  }
}
