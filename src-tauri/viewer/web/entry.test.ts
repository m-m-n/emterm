/**
 * Tests for the standalone Markdown viewer entry.
 *
 * Exercises the pure render path (`renderPayload` / `applyAppearance`)
 * against happy-dom. Mermaid rendering is fire-and-forget and lazy-loaded,
 * so we assert on the synchronous DOM structure only.
 */

import { describe, expect, spyOn, test } from "bun:test";

// Polyfill IntersectionObserver for happy-dom (the production WebKitGTK
// runtime provides it; mirrors src/markdown/outline.test.ts).
if (typeof globalThis.IntersectionObserver === "undefined") {
  globalThis.IntersectionObserver = class IntersectionObserver {
    readonly root: Element | null = null;
    readonly rootMargin: string = "";
    readonly thresholds: ReadonlyArray<number> = [];
    constructor(
      _callback: IntersectionObserverCallback,
      _options?: IntersectionObserverInit,
    ) {}
    observe(_target: Element): void {}
    unobserve(_target: Element): void {}
    disconnect(): void {}
    takeRecords(): IntersectionObserverEntry[] {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
}

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import * as extractorMod from "../../web-shared/markdown/frontmatter/extractor.ts";
import { applyAppearance, parsePayload, renderPayload } from "./entry.ts";
import type { ViewerPayload } from "./entry.ts";

function payload(overrides: Partial<ViewerPayload> = {}): ViewerPayload {
  return {
    markdown: "# Title\n\nHello **world**.",
    format: "gfm",
    appearance: {
      theme: "dark",
      preset: "purple",
      bodyFontFamily: "",
      codeFontFamily: "",
      fontSize: 14,
      ...overrides.appearance,
    },
    ...overrides,
  };
}

describe("viewer entry", () => {
  test("renders an injected sample into the fullscreen content structure", () => {
    const root = document.createElement("div");
    const content = renderPayload(root, payload());

    // Mirrors the WebView fullscreen overlay structure (for CSS parity).
    const overlay = root.querySelector(".markdown-fullscreen-overlay");
    expect(overlay).not.toBeNull();
    expect(content.classList.contains("markdown-fullscreen-content")).toBe(
      true,
    );

    // Rendered markdown is present (heading + bold).
    expect(content.querySelector("h1")?.textContent).toContain("Title");
    expect(content.querySelector("strong")?.textContent).toContain("world");
  });

  test("builds an outline panel when headings exist", () => {
    const root = document.createElement("div");
    renderPayload(root, payload({ markdown: "# A\n\n## B\n\n### C" }));
    const overlay = root.querySelector(".markdown-fullscreen-overlay");
    // OutlinePanel marks the overlay and inserts a panel before content.
    expect(overlay?.classList.contains("has-outline")).toBe(true);
  });

  test("applyAppearance writes the markdown font-size CSS variable", () => {
    applyAppearance({
      theme: "light",
      preset: "green",
      bodyFontFamily: "Noto Sans",
      codeFontFamily: "Fira Code",
      fontSize: 20,
    });
    const root = document.documentElement;
    expect(root.style.getPropertyValue("--markdown-body-font-size")).toBe(
      "20pt",
    );
    expect(root.style.getPropertyValue("--markdown-body-font-family")).toBe(
      "Noto Sans",
    );
  });

  test("empty markdown renders without throwing", () => {
    const root = document.createElement("div");
    const content = renderPayload(root, payload({ markdown: "" }));
    expect(content.classList.contains("markdown-fullscreen-content")).toBe(
      true,
    );
  });

  // H5: the wire payload is defined twice (Rust ViewerPayload/PayloadAppearance
  // in native-poc/src/viewer/launch.rs, and the TS ViewerPayload above). Both
  // sides validate against the SAME committed fixture, so a field-name
  // rename/removal on either side fails its half of the contract test.
  test("parses the shared Rust/TS payload fixture with all fields", () => {
    const fixtureUrl = new URL(
      "./__fixtures__/payload.fixture.json",
      import.meta.url,
    );
    const raw = readFileSync(fileURLToPath(fixtureUrl), "utf8");
    const parsed: ViewerPayload = parsePayload(raw);

    expect(parsed.markdown).toBe("# Hi\n\n本文 🎉");
    expect(parsed.format).toBe("gfm");
    expect(parsed.basedir).toBe("/home/me/docs");
    expect(parsed.appearance.theme).toBe("dark");
    expect(parsed.appearance.preset).toBe("purple");
    expect(parsed.appearance.bodyFontFamily).toBe("Noto Sans");
    expect(parsed.appearance.codeFontFamily).toBe("Fira Code");
    expect(parsed.appearance.fontSize).toBe(14);

    // The fixture must be renderable by the real consumer too.
    const root = document.createElement("div");
    const content = renderPayload(root, parsed);
    expect(content.querySelector("h1")?.textContent).toContain("Hi");
  });
});

describe("viewer entry — front matter (task0005)", () => {
  test("AC-1: mounts the front matter block above the rendered body", () => {
    const root = document.createElement("div");
    const content = renderPayload(
      root,
      payload({ markdown: "---\ntitle: My Post\n---\n\nBody paragraph." }),
    );

    const block = content.querySelector(".fm-block");
    expect(block).not.toBeNull();
    expect(block?.querySelector(".fm-badge")?.textContent).toBe("YAML");

    // The block is the first child of the scroll container, directly above the
    // rendered markdown body.
    expect(content.firstElementChild).toBe(block);
    expect(
      block?.nextElementSibling?.classList.contains("markdown-content"),
    ).toBe(true);

    // Front matter text does not leak into the body; the real body survives.
    expect(content.querySelector(".markdown-content")?.innerHTML).not.toContain(
      "My Post",
    );
    expect(content.innerHTML).toContain("Body paragraph.");
  });

  test("AC-4: no front matter — no block mounted, body rendered normally", () => {
    const root = document.createElement("div");
    const content = renderPayload(
      root,
      payload({
        markdown: "# Doc\n\nPlain body.\n\n---\n\nAfter a real break.",
      }),
    );

    expect(content.querySelector(".fm-block")).toBeNull();
    const body = content.querySelector(".markdown-content");
    expect(body).not.toBeNull();
    // The mid-body thematic break stays a normal <hr> (not treated as front matter).
    expect(body?.innerHTML).toContain("<hr");
    expect(content.innerHTML).toContain("Plain body.");
    expect(content.innerHTML).toContain("After a real break.");
  });

  test("AC-3: broken front matter — error-state block mounted, body clean", () => {
    const root = document.createElement("div");
    const content = renderPayload(
      root,
      payload({ markdown: "{ \"title\": 'oops', }\n\nBody paragraph." }),
    );

    const block = content.querySelector(".fm-block");
    expect(block).not.toBeNull();
    expect(block?.classList.contains("fm-block-error")).toBe(true);
    expect(content.querySelector(".markdown-content")?.innerHTML).not.toContain(
      "oops",
    );
    expect(content.innerHTML).toContain("Body paragraph.");
  });

  test("AC-5: document that is only front matter — block mounted, empty body", () => {
    const root = document.createElement("div");
    const content = renderPayload(
      root,
      payload({ markdown: "---\ntitle: Only\n---\n" }),
    );

    const block = content.querySelector(".fm-block");
    expect(block).not.toBeNull();
    expect(block?.querySelector(".fm-badge")?.textContent).toBe("YAML");
    const body = content.querySelector(".markdown-content");
    expect(body).not.toBeNull();
    // Empty body: no leaked front matter text.
    expect(body?.textContent?.trim()).toBe("");
  });
});

/** Parse `--name: value;` custom-property declarations into a name→value map. */
function parseCssVars(css: string): Record<string, string> {
  const out: Record<string, string> = {};
  const re = /(--[\w-]+)\s*:\s*([^;]+);/g;
  let m: RegExpExecArray | null;
  // biome-ignore lint/suspicious/noAssignInExpressions: idiomatic regex loop
  while ((m = re.exec(css)) !== null) {
    // Normalize internal whitespace so `cubic-bezier(0.2, 0, 0, 1)` compares
    // equal regardless of spacing between the two files.
    out[m[1]!] = m[2]!.trim().replace(/\s+/g, " ");
  }
  return out;
}

describe("viewer entry — front matter theme-following (task0006 AC-1)", () => {
  test("AC-1: the block's active color tokens differ between light and dark themes", () => {
    const root = document.documentElement;

    applyAppearance({
      theme: "light",
      preset: "purple",
      bodyFontFamily: "",
      codeFontFamily: "",
      fontSize: 14,
    });
    // The resolved theme is exposed for theme-scoped styles.
    expect(root.getAttribute("data-theme")).toBe("light");
    // The variables the front matter block consumes (--markdown-pre-bg for its
    // surface, --markdown-border for its outline) resolve to light values.
    const lightBg = root.style.getPropertyValue("--markdown-pre-bg");
    const lightBorder = root.style.getPropertyValue("--markdown-border");

    applyAppearance({
      theme: "dark",
      preset: "purple",
      bodyFontFamily: "",
      codeFontFamily: "",
      fontSize: 14,
    });
    expect(root.getAttribute("data-theme")).toBe("dark");
    const darkBg = root.style.getPropertyValue("--markdown-pre-bg");
    const darkBorder = root.style.getPropertyValue("--markdown-border");

    // Asserted on the resolved/active token values (not a static grep): the
    // block genuinely follows the theme rather than staying dark.
    expect(lightBg).not.toBe(darkBg);
    expect(lightBorder).not.toBe(darkBorder);
    // Dark values are unchanged from the purple preset (no dark regression);
    // light values are the light palette.
    expect(darkBg.toUpperCase()).toBe("#1D1B20");
    expect(lightBg.toUpperCase()).toBe("#F3EDF7");
  });
});

describe("viewer entry — MD3 token duplication removed (task0006 AC-2)", () => {
  test("AC-2: index.html drops the dark-only --md-sys-color copy; remaining MD3 tokens are pinned to styles.css", async () => {
    const html = await Bun.file(
      new URL("./index.html", import.meta.url),
    ).text();

    // The hand-maintained dark-only color token copy is gone (its drift risk
    // and theme lock-in were the round-1 findings).
    expect(html).not.toContain("--md-sys-color-");

    // Whatever MD3 tokens index.html still provides (theme-independent shape /
    // motion, needed by fullscreen.css) must match the SSOT mirror value-for-
    // value, so the remaining copy cannot silently drift.
    const styles = await Bun.file(
      new URL("../../web-shared/styles.css", import.meta.url),
    ).text();
    const htmlVars = parseCssVars(html);
    const styleVars = parseCssVars(styles);

    const remainingMdTokens = Object.keys(htmlVars).filter((k) =>
      k.startsWith("--md-"),
    );
    // The shape/motion tokens fullscreen.css depends on are still provided.
    expect(remainingMdTokens.length).toBeGreaterThan(0);
    for (const token of remainingMdTokens) {
      expect(styleVars[token]).toBe(htmlVars[token]!);
    }
  });
});

describe("viewer entry — single extraction (task0006 AC-3)", () => {
  test("AC-3: extractFrontMatter runs exactly once per rendered document", () => {
    const spy = spyOn(extractorMod, "extractFrontMatter");
    try {
      const root = document.createElement("div");
      renderPayload(
        root,
        payload({ markdown: "---\ntitle: Solo\n---\n\nBody paragraph." }),
      );
      // The renderer no longer strips internally, so the boundary is the single
      // extraction site: one call per rendered document (no double-extraction).
      expect(spy.mock.calls.length).toBe(1);
    } finally {
      spy.mockRestore();
    }
  });
});
