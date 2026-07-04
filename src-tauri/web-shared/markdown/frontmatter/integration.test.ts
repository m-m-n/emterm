/**
 * Integration tests for the front matter render pipeline (task0005).
 *
 * Exercises the real extractor (task0001) -> parser (task0002) -> block view
 * (task0004) chain together with the MarkdownRenderer front-matter hook — no
 * mocking. Covers SPEC.md TS-11 (front matter: clean body + block) and TS-12
 * (no front matter: byte-identical regression), plus the front-matter-only
 * edge case. Each test names the acceptance criterion it proves
 * (tasks/task0005.md AC-1 .. AC-5).
 */

import { describe, expect, test } from "bun:test";

import type { MarkdownFormat } from "../types.ts";
import { MarkdownRenderer } from "../renderer.ts";
import { extractFrontMatter } from "./extractor.ts";
import { parseFrontMatter } from "./parser.ts";
import { buildFrontMatterBlock } from "./view.ts";

/**
 * Run the same front matter pipeline the viewer entry performs: extract from
 * the original source, render it (the renderer strips the block internally),
 * and — when a block was found — parse it and build the mounted block element.
 */
function pipeline(
  source: string,
  format: MarkdownFormat = "gfm",
): { html: string; block: HTMLElement | null } {
  const extraction = extractFrontMatter(source);
  const html = new MarkdownRenderer().render(source, format);
  const block = extraction.found
    ? buildFrontMatterBlock(
        extraction,
        parseFrontMatter(extraction.raw, extraction.format),
      )
    : null;
  return { html, block };
}

/** Expand a block so its content (tree or error view) is queryable. */
function expand(block: HTMLElement): void {
  block.querySelector<HTMLButtonElement>(".fm-header")?.click();
}

describe("front matter pipeline — YAML (AC-1 / TS-11)", () => {
  test("AC-1: YAML body has no delimiter artifacts; YAML block quarantines the data", () => {
    const src =
      "---\ntitle: My Post\ntag: draft\n---\n\n# Heading\n\nBody paragraph.";
    const { html, block } = pipeline(src);

    // The delimiters and their content never reach marked: no leaked front
    // matter text, no hr/heading artifact from the `---` lines.
    expect(html).not.toContain("My Post");
    expect(html).not.toContain("title:");
    expect(html).not.toContain("<hr");
    // The real body survives.
    expect(html).toContain("Body paragraph.");

    // A collapsed YAML block is produced; expanding shows the parsed data.
    expect(block).not.toBeNull();
    expect(block?.querySelector(".fm-badge")?.textContent).toBe("YAML");
    expect(block?.classList.contains("fm-block")).toBe(true);
    expand(block as HTMLElement);
    expect(block?.querySelector(".fm-tree")).not.toBeNull();
    expect(block?.textContent).toContain("My Post");
  });
});

describe("front matter pipeline — TOML / JSON (AC-2 / TS-11)", () => {
  test("AC-2: TOML body clean; TOML block quarantines the data", () => {
    const src = '+++\ntitle = "My Post"\n+++\n\nBody paragraph.';
    const { html, block } = pipeline(src);

    expect(html).not.toContain("My Post");
    expect(html).not.toContain("+++");
    expect(html).toContain("Body paragraph.");

    expect(block?.querySelector(".fm-badge")?.textContent).toBe("TOML");
    expand(block as HTMLElement);
    expect(block?.textContent).toContain("My Post");
  });

  test("AC-2: JSON body clean; JSON block quarantines the data", () => {
    const src = '{\n  "title": "My Post"\n}\n\nBody paragraph.';
    const { html, block } = pipeline(src);

    expect(html).not.toContain("My Post");
    expect(html).toContain("Body paragraph.");

    expect(block?.querySelector(".fm-badge")?.textContent).toBe("JSON");
    expand(block as HTMLElement);
    expect(block?.textContent).toContain("My Post");
  });
});

describe("front matter pipeline — parse failure (AC-3)", () => {
  test("AC-3: broken front matter — body clean, block in error state", () => {
    // Braces balance (so the extractor recognizes a JSON block) but the content
    // is invalid JSON (single quotes / trailing comma), so parsing fails.
    const src = "{ \"title\": 'oops', }\n\nBody paragraph.";
    const { html, block } = pipeline(src);

    // The block content does not leak into the rendered body.
    expect(html).not.toContain("oops");
    expect(html).toContain("Body paragraph.");

    // The mounted block is in the error state.
    expect(block).not.toBeNull();
    expect(block?.classList.contains("fm-block-error")).toBe(true);
    expect(block?.querySelector(".fm-error-indicator")).not.toBeNull();
    // A failure never renders a data tree.
    expect(block?.querySelector(".fm-tree")).toBeNull();
  });
});

describe("front matter pipeline — passthrough (AC-4 / TS-12)", () => {
  test("AC-4: no front matter — fast-path body is reference-identical (byte-identical to marked), no block", () => {
    const src =
      "# Just a doc\n\nPlain body.\n\n---\n\nAfter a real thematic break.";
    const extraction = extractFrontMatter(src);

    expect(extraction.found).toBe(false);
    // The body handed to marked is the SAME string object as the input, so the
    // rendered output is byte-identical to the pre-change pipeline by
    // construction (FR7 / NFR4 fast path).
    expect(extraction.body).toBe(src);

    const { html, block } = pipeline(src);
    expect(block).toBeNull();
    // A mid-body `---` is still a normal thematic break — not front matter.
    expect(html).toContain("<hr");
    expect(html).toContain("Plain body.");
    expect(html).toContain("After a real thematic break.");
  });
});

describe("front matter pipeline — front-matter-only (AC-5)", () => {
  test("AC-5: document that is only front matter — empty body plus the block", () => {
    const src = "---\ntitle: Only\n---\n";
    const extraction = extractFrontMatter(src);
    expect(extraction.found).toBe(true);
    // Everything after the closing delimiter's newline is empty.
    expect(extraction.found && extraction.body).toBe("");

    const { html, block } = pipeline(src);
    // The rendered body carries none of the front matter text.
    expect(html).not.toContain("Only");
    expect(html).not.toContain("title:");

    // The block is present and holds the parsed data when expanded.
    expect(block).not.toBeNull();
    expect(block?.querySelector(".fm-badge")?.textContent).toBe("YAML");
    expand(block as HTMLElement);
    expect(block?.textContent).toContain("Only");
  });
});
