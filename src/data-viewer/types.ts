/**
 * Data viewer type definitions.
 *
 * @module data-viewer/types
 */

/** Supported data formats */
export type DataFormat = "json" | "yaml";

/** A session for streaming data viewer content */
export interface DataViewerSession {
  id: string;
  format: DataFormat;
  version: number;
  chunks: Map<number, string>;
  lastChunkAt: number;
}

/** Result of parsing data */
export type ParseResult =
  | { ok: true; data: unknown; rawText: string }
  | { ok: false; error: string; rawText: string };

/** A node in the outline tree */
export interface TreeNode {
  /** Display key name */
  key: string;
  /** Nesting depth (0 = root) */
  depth: number;
  /** JSON path from root (e.g., "server.host") */
  path: string;
  /** The value at this node */
  value: unknown;
  /** Whether this node has children (is object or array) */
  hasChildren: boolean;
}
