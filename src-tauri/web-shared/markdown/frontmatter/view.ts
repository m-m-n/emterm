/**
 * Front matter block view (DOM builder).
 *
 * Builds a single detached, collapsed-by-default DOM element for a detected
 * front matter block: a header (label + format badge, plus a failure indicator
 * in the error state) and a content area that is an always-fully-expanded tree
 * on success, or the localized parse-error notice + raw text on failure. The
 * caller (task0005) mounts the returned element above the rendered body
 * (IMPLEMENTATION.md D3).
 *
 * View layer only — this module owns all DOM construction and consumes pure
 * logic outputs (the extraction/parse contracts and the tree builder). Every
 * front-matter-derived string enters the DOM via text nodes, built
 * element-by-element with no HTML-string assembly (NFR1). Colors live entirely
 * in `frontmatter.css`, drawn from the viewer's theme-aware `--markdown-*`
 * variables so the block follows the effective light/dark theme (NFR2).
 *
 * @module markdown/frontmatter/view
 */

import { t } from "../../i18n/index.ts";
import type { FrontMatterExtracted, FrontMatterParseResult } from "./types.ts";
import { MAX_ERROR_RAW_DISPLAY_CHARS } from "./limits.ts";
import { MAX_NODES, buildTree, type TreeNode } from "./tree-builder.ts";

/**
 * Marker appended to the error-state raw text when it is clamped for display.
 * A fixed literal (never front-matter-derived), so it carries no escaping
 * concern and its presence unambiguously signals truncation.
 */
export const ERROR_RAW_TRUNCATION_MARKER = "\n…[truncated]";

/** Monotonic id source so each block's content gets a unique aria-controls id. */
let contentIdCounter = 0;

/**
 * Build the front matter block element for a detected block.
 *
 * @param extraction - The "found" extraction result (supplies format + raw).
 * @param parse - The parse result (success tree, or failure message).
 * @returns A detached, collapsed-by-default block element the caller mounts.
 */
export function buildFrontMatterBlock(
  extraction: FrontMatterExtracted,
  parse: FrontMatterParseResult,
): HTMLElement {
  const contentId = `fm-content-${++contentIdCounter}`;

  const section = document.createElement("section");
  section.className = "fm-block";
  section.dataset.format = extraction.format;
  if (!parse.ok) section.classList.add("fm-block-error");

  const header = buildHeader(extraction, parse, contentId);
  section.appendChild(header);

  const content = document.createElement("div");
  content.className = "fm-content";
  content.id = contentId;
  content.setAttribute("hidden", "");
  if (parse.ok) {
    const { nodes, truncated } = buildTree(parse.value);
    content.appendChild(buildTreeView(nodes));
    // Hostile front matter can exceed the node budget at either stage: the
    // parser bounds normalization (parse.truncated) and the tree builder bounds
    // row emission (truncated). When either partial-copies the data, surface the
    // same notice so the omission is visible rather than silent.
    if (truncated || parse.truncated) {
      content.appendChild(buildTruncatedNotice());
    }
  } else {
    content.appendChild(buildErrorView(parse.error, extraction.raw));
  }
  section.appendChild(content);

  const setOpen = (open: boolean): void => {
    header.setAttribute("aria-expanded", open ? "true" : "false");
    section.classList.toggle("is-open", open);
    if (open) content.removeAttribute("hidden");
    else content.setAttribute("hidden", "");
  };

  header.addEventListener("click", () => {
    setOpen(header.getAttribute("aria-expanded") !== "true");
  });

  return section;
}

/** Build the clickable header row. */
function buildHeader(
  extraction: FrontMatterExtracted,
  parse: FrontMatterParseResult,
  contentId: string,
): HTMLButtonElement {
  const header = document.createElement("button");
  header.type = "button";
  header.className = "fm-header";
  header.setAttribute("aria-expanded", "false");
  header.setAttribute("aria-controls", contentId);

  const icon = document.createElement("span");
  icon.className = "fm-toggle-icon";
  icon.setAttribute("aria-hidden", "true");
  header.appendChild(icon);

  const label = document.createElement("span");
  label.className = "fm-label";
  // "Front Matter" is a proper noun for this feature — intentionally untranslated.
  label.textContent = "Front Matter";
  header.appendChild(label);

  const badge = document.createElement("span");
  badge.className = "fm-badge";
  // Format names (YAML / TOML / JSON) are not translated.
  badge.textContent = extraction.format.toUpperCase();
  header.appendChild(badge);

  if (!parse.ok) {
    const indicator = document.createElement("span");
    indicator.className = "fm-error-indicator";
    indicator.textContent = t("markdown.frontMatter.parseError");
    header.appendChild(indicator);
  }

  return header;
}

/** Build the always-fully-expanded tree from a flat node list. */
function buildTreeView(nodes: TreeNode[]): HTMLElement {
  const tree = document.createElement("div");
  tree.className = "fm-tree";
  tree.setAttribute("role", "tree");
  for (const node of nodes) {
    tree.appendChild(buildRow(node));
  }
  return tree;
}

/** Build the localized partial-tree notice shown when the node budget was hit. */
function buildTruncatedNotice(): HTMLElement {
  const notice = document.createElement("p");
  notice.className = "fm-truncated";
  notice.setAttribute("role", "status");
  notice.textContent = t("markdown.frontMatter.truncatedNotice", {
    count: MAX_NODES,
  });
  return notice;
}

/** Build one tree row: key (+ scalar value for leaf nodes). */
function buildRow(node: TreeNode): HTMLElement {
  const row = document.createElement("div");
  row.className = "fm-row";
  row.setAttribute("role", "treeitem");
  row.setAttribute("aria-level", String(node.depth + 1));
  row.dataset.depth = String(node.depth);
  row.style.setProperty("--fm-depth", String(node.depth));

  const key = document.createElement("span");
  key.className = "fm-key";
  key.textContent = node.key;
  row.appendChild(key);

  if (!node.hasChildren) {
    const sep = document.createElement("span");
    sep.className = "fm-sep";
    sep.setAttribute("aria-hidden", "true");
    sep.textContent = ":";
    row.appendChild(sep);

    const value = document.createElement("span");
    value.className = "fm-value";
    value.dataset.type = scalarType(node.value);
    value.textContent = formatScalar(node.value);
    row.appendChild(value);
  }

  return row;
}

/** Build the parse-failure content: notice + parser message + raw text. */
function buildErrorView(error: string, raw: string): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "fm-error";

  const notice = document.createElement("p");
  notice.className = "fm-error-notice";
  notice.setAttribute("role", "alert");
  notice.textContent = t("markdown.frontMatter.parseErrorNotice");
  wrap.appendChild(notice);

  if (error) {
    const message = document.createElement("p");
    message.className = "fm-error-message";
    message.textContent = error;
    wrap.appendChild(message);
  }

  const pre = document.createElement("pre");
  pre.className = "fm-raw";
  // Never assign the unbounded raw to the DOM: oversized front matter reaches
  // this error path (task0009 routes raw over MAX_RAW_BYTES here) and can be as
  // large as the Rust-side 100 MiB session cap. Clamp to the display bound with
  // a marker so the <pre> layout cost stays bounded (SPEC.md Security
  // Considerations).
  pre.textContent = clampErrorRaw(raw);
  wrap.appendChild(pre);

  return wrap;
}

/**
 * Clamp the error-state raw text to the display bound, appending a truncation
 * marker when it was cut. Raw at or below the bound is returned verbatim.
 */
function clampErrorRaw(raw: string): string {
  if (raw.length <= MAX_ERROR_RAW_DISPLAY_CHARS) return raw;
  return (
    raw.slice(0, MAX_ERROR_RAW_DISPLAY_CHARS) + ERROR_RAW_TRUNCATION_MARKER
  );
}

/** Display string for a scalar leaf value. */
function formatScalar(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "string") return value;
  if (typeof value === "number") return String(value);
  return String(value);
}

/** Type discriminator used to style scalar values (data-type attribute). */
function scalarType(value: unknown): string {
  if (value === null || value === undefined) return "null";
  const kind = typeof value;
  if (kind === "string" || kind === "number" || kind === "boolean") return kind;
  return "other";
}
