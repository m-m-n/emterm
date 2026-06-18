/**
 * Tests for the standalone Markdown viewer entry.
 *
 * Exercises the pure render path (`renderPayload` / `applyAppearance`)
 * against happy-dom. Mermaid rendering is fire-and-forget and lazy-loaded,
 * so we assert on the synchronous DOM structure only.
 */

import { describe, expect, test } from "bun:test";

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
      emojiFontFamily: "",
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
      emojiFontFamily: "",
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
    expect(parsed.appearance.emojiFontFamily).toBe("Noto Color Emoji");
    expect(parsed.appearance.fontSize).toBe(14);

    // The fixture must be renderable by the real consumer too.
    const root = document.createElement("div");
    const content = renderPayload(root, parsed);
    expect(content.querySelector("h1")?.textContent).toContain("Hi");
  });
});
