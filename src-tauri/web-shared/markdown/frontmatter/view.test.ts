/**
 * DOM tests for the front matter block view (task0004).
 *
 * Exercises the collapsed-by-default block builder under happy-dom
 * (`test-setup.ts` initializes i18n to "en"). Each test names the acceptance
 * criterion it proves (AC-1 .. AC-7 in tasks/task0004.md), covering SPEC.md
 * TS-8 / TS-9 / TS-10 (view half).
 */

import { afterEach, describe, expect, test } from "bun:test";

import { setLocale } from "../../i18n/index.ts";
import type {
  FrontMatterExtracted,
  FrontMatterParseResult,
  FrontMatterValue,
} from "./types.ts";
import { buildFrontMatterBlock } from "./view.ts";

/** Build a "found" extraction result for the given format + raw text. */
function extracted(
  format: FrontMatterExtracted["format"],
  raw: string,
): FrontMatterExtracted {
  return { found: true, format, raw, body: "" };
}

/** Build a success parse result. */
function ok(value: FrontMatterValue): FrontMatterParseResult {
  return { ok: true, value };
}

/** Build a failure parse result. */
function fail(error: string): FrontMatterParseResult {
  return { ok: false, error };
}

/** The header <button> of a built block. */
function header(block: HTMLElement): HTMLButtonElement {
  const el = block.querySelector<HTMLButtonElement>(".fm-header");
  if (!el) throw new Error(".fm-header not found");
  return el;
}

/** The content container of a built block. */
function content(block: HTMLElement): HTMLElement {
  const el = block.querySelector<HTMLElement>(".fm-content");
  if (!el) throw new Error(".fm-content not found");
  return el;
}

/** Find the row whose key span reads exactly `key`. */
function rowByKey(block: HTMLElement, key: string): HTMLElement {
  const rows = Array.from(block.querySelectorAll<HTMLElement>(".fm-row"));
  const match = rows.find(
    (r) => r.querySelector(".fm-key")?.textContent === key,
  );
  if (!match) throw new Error(`no row with key ${key}`);
  return match;
}

afterEach(() => {
  // Restore the default locale so a ja-locale test cannot bleed into others.
  setLocale("en");
});

describe("front matter block — collapsed header (AC-1)", () => {
  const cases: Array<[FrontMatterExtracted["format"], string]> = [
    ["yaml", "YAML"],
    ["toml", "TOML"],
    ["json", "JSON"],
  ];

  for (const [format, badge] of cases) {
    test(`AC-1: ${format} success block is collapsed with label + ${badge} badge`, () => {
      const block = buildFrontMatterBlock(
        extracted(format, "k: v"),
        ok({ k: "v" }),
      );

      expect(block.querySelector(".fm-label")?.textContent).toBe(
        "Front Matter",
      );
      expect(block.querySelector(".fm-badge")?.textContent).toBe(badge);
      expect(block.dataset.format).toBe(format);

      // Collapsed by default: content hidden, header not expanded, no error.
      expect(content(block).hasAttribute("hidden")).toBe(true);
      expect(header(block).getAttribute("aria-expanded")).toBe("false");
      expect(block.querySelector(".fm-error-indicator")).toBeNull();
    });
  }
});

describe("front matter block — toggle (AC-2)", () => {
  test("AC-2: activating the header expands then collapses the content", () => {
    const block = buildFrontMatterBlock(
      extracted("yaml", "a: 1"),
      ok({ a: 1 }),
    );
    const h = header(block);
    const c = content(block);

    expect(c.hasAttribute("hidden")).toBe(true);

    h.click();
    expect(c.hasAttribute("hidden")).toBe(false);
    expect(h.getAttribute("aria-expanded")).toBe("true");
    expect(block.querySelector(".fm-tree")).not.toBeNull();

    h.click();
    expect(c.hasAttribute("hidden")).toBe(true);
    expect(h.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("front matter block — tree rows (AC-3)", () => {
  test("AC-3: one row per node with depth indentation and scalar values", () => {
    const value = {
      server: { host: "localhost", port: 8080 },
      tags: ["a", "b"],
    };
    const block = buildFrontMatterBlock(extracted("yaml", "..."), ok(value));
    header(block).click();

    // buildTree order: server, host, port, tags, [0], [1] => 6 nodes.
    const rows = block.querySelectorAll(".fm-row");
    expect(rows.length).toBe(6);

    const keys = Array.from(block.querySelectorAll(".fm-key")).map(
      (k) => k.textContent,
    );
    expect(keys).toContain("server");
    expect(keys).toContain("host");
    expect(keys).toContain("[0]");

    // Scalar values are rendered and visible.
    const values = Array.from(block.querySelectorAll(".fm-value")).map(
      (v) => v.textContent,
    );
    expect(values).toContain("localhost");
    expect(values).toContain("8080");
    expect(values).toContain("a");

    // Depth drives indentation metadata.
    expect(rowByKey(block, "server").dataset.depth).toBe("0");
    expect(rowByKey(block, "host").dataset.depth).toBe("1");
    expect(rowByKey(block, "server").style.getPropertyValue("--fm-depth")).toBe(
      "0",
    );
    expect(rowByKey(block, "host").style.getPropertyValue("--fm-depth")).toBe(
      "1",
    );

    // Container rows carry no scalar value span; leaf rows do.
    expect(rowByKey(block, "server").querySelector(".fm-value")).toBeNull();
    expect(rowByKey(block, "host").querySelector(".fm-value")).not.toBeNull();
  });
});

describe("front matter block — parse failure (AC-4)", () => {
  test("AC-4: failure shows header indicator, notice, message, and raw text", () => {
    const raw = "title: : broken";
    const block = buildFrontMatterBlock(
      extracted("yaml", raw),
      fail("bad indentation at line 1"),
    );

    // Header signals the failure (en locale via test-setup).
    const indicator = block.querySelector(".fm-error-indicator");
    expect(indicator).not.toBeNull();
    expect(indicator?.textContent).toBe("Parse error");

    header(block).click();

    // Localized notice + parser message + raw block.
    expect(block.querySelector(".fm-error-notice")?.textContent).toContain(
      "Failed to parse front matter",
    );
    expect(block.textContent).toContain("bad indentation at line 1");
    expect(block.querySelector(".fm-raw")?.textContent).toBe(raw);

    // A failure never renders a tree.
    expect(block.querySelector(".fm-tree")).toBeNull();
  });

  test("AC-4: parse-error strings are localized in ja", () => {
    setLocale("ja");
    const block = buildFrontMatterBlock(
      extracted("yaml", "x"),
      fail("some error"),
    );
    expect(block.querySelector(".fm-error-indicator")?.textContent).toBe(
      "解析エラー",
    );
    header(block).click();
    expect(block.querySelector(".fm-error-notice")?.textContent).toContain(
      "フロントマターの解析に失敗しました",
    );
  });
});

describe("front matter block — XSS safety (AC-5)", () => {
  test("AC-5: HTML in keys/values is literal text, no elements created", () => {
    const value: FrontMatterValue = {
      "<script>alert(1)</script>": "<img src=x onerror=alert(2)>",
    };
    const block = buildFrontMatterBlock(
      extracted("yaml", "raw: value"),
      ok(value),
    );
    header(block).click();

    // No element is created from front-matter-derived strings.
    expect(block.querySelector("script")).toBeNull();
    expect(block.querySelector("img")).toBeNull();

    // The markup appears verbatim as text.
    expect(block.querySelector(".fm-key")?.textContent).toBe(
      "<script>alert(1)</script>",
    );
    expect(block.querySelector(".fm-value")?.textContent).toBe(
      "<img src=x onerror=alert(2)>",
    );
  });

  test("AC-5: HTML in raw error text is literal", () => {
    const raw = "<script>evil()</script>\n<img onerror=boom>";
    const block = buildFrontMatterBlock(
      extracted("json", raw),
      fail("<b>unterminated</b>"),
    );
    header(block).click();

    expect(block.querySelector("script")).toBeNull();
    expect(block.querySelector("img")).toBeNull();
    expect(block.querySelector("b")).toBeNull();
    expect(block.querySelector(".fm-raw")?.textContent).toBe(raw);
    expect(block.textContent).toContain("<b>unterminated</b>");
  });
});

describe("front matter block — empty tree (AC-6)", () => {
  test("AC-6: empty object renders expanded with no rows and no error", () => {
    const block = buildFrontMatterBlock(extracted("json", "{}"), ok({}));
    header(block).click();

    expect(content(block).hasAttribute("hidden")).toBe(false);
    expect(block.querySelector(".fm-tree")).not.toBeNull();
    expect(block.querySelectorAll(".fm-row").length).toBe(0);
    expect(block.querySelector(".fm-error")).toBeNull();
    expect(block.querySelector(".fm-error-indicator")).toBeNull();
  });

  test("AC-6: scalar root renders an empty tree without error", () => {
    const block = buildFrontMatterBlock(
      extracted("yaml", "just a string"),
      ok("just a string"),
    );
    header(block).click();

    expect(block.querySelector(".fm-tree")).not.toBeNull();
    expect(block.querySelectorAll(".fm-row").length).toBe(0);
    expect(block.querySelector(".fm-error")).toBeNull();
  });
});

describe("front matter stylesheet — MD3 tokens only (AC-7)", () => {
  test("AC-7: frontmatter.css references only --md-sys-* custom properties for colors", async () => {
    const cssUrl = new URL("./frontmatter.css", import.meta.url);
    const raw = await Bun.file(cssUrl).text();
    // Drop comments so example text inside them cannot trip the scan.
    const css = raw.replace(/\/\*[\s\S]*?\*\//g, "");

    // No hard-coded hex colors.
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    // No rgb()/rgba()/hsl()/hsla() function colors.
    expect(css).not.toMatch(/\b(?:rgb|rgba|hsl|hsla)\s*\(/i);
    // Sanity: colors are supplied via MD3 system tokens.
    expect(css).toMatch(/var\(--md-sys-color-/);
  });
});
