/**
 * Sanitizer-boundary regression test for `MarkdownRenderer.render()`.
 *
 * Pins the heading-survival behavior at the sanitize call
 * (renderer.ts's `DOMPurify.sanitize(rawHtml, PURIFY_CONFIG)`), one layer
 * closer to the sanitizer than the viewer entry tests. A future dependency
 * bump that reintroduces the loss this test guards against fails here,
 * naming the sanitizer as the culprit instead of failing an entry-level
 * assertion (see doc/dompurify-h1-sanitization.md for the investigation
 * this test encodes).
 *
 * Covers two entry points into the renderer, per the reported failures:
 * - AC-5a: a document whose first block is a level-1 heading.
 * - AC-5b: a document whose first block was a front matter block, with the
 *   heading in the body that remains after front matter extraction.
 */

import { describe, expect, test } from "bun:test";

import { extractFrontMatter } from "./frontmatter/extractor.ts";
import { MarkdownRenderer } from "./renderer.ts";

function renderToContainer(markdown: string): HTMLDivElement {
  const renderer = new MarkdownRenderer();
  const html = renderer.render(markdown, "gfm");
  const container = document.createElement("div");
  container.innerHTML = html;
  return container;
}

describe("MarkdownRenderer sanitizer boundary — heading survival", () => {
  test("AC-5a: a document whose first block is a level-1 heading keeps that heading and its text", () => {
    const container = renderToContainer("# Title\n\nHello **world**.");

    const heading = container.querySelector("h1");
    expect(heading).not.toBeNull();
    expect(heading?.textContent).toContain("Title");
  });

  test("AC-5b: a document whose heading was preceded by front matter keeps that heading and its text, after the body is extracted", () => {
    const source = "---\ntitle: My Doc\n---\n# Title\n\nHello **world**.";
    const { found, body } = extractFrontMatter(source);
    expect(found).toBe(true);

    const container = renderToContainer(body);

    const heading = container.querySelector("h1");
    expect(heading).not.toBeNull();
    expect(heading?.textContent).toContain("Title");
  });
});
