/**
 * Tests for MermaidRenderer toolbar behavior.
 *
 * These tests inject a stub mermaid API into a MermaidRenderer instance so
 * we can exercise the DOM building code inside `renderBlock` without loading
 * the real mermaid runtime. That keeps unit tests hermetic and fast.
 *
 * Covers:
 * - TS-1 toolbar order after renderBlock
 * - TS-2 Spread button attributes
 * - TS-3 Copy click success path
 * - TS-4 Copy click failure path
 */

import { afterEach, describe, expect, test } from "bun:test";

import { MermaidRenderer } from "./mermaid-renderer.ts";

/** Minimal SVG returned by the fake mermaid.render(). */
const FAKE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><rect x="0" y="0" width="100" height="100"/></svg>';

interface FakeMermaid {
  initialize: (config: Record<string, unknown>) => void;
  render: (
    id: string,
    source: string,
    container?: HTMLElement,
  ) => Promise<{ svg: string }>;
}

function makeFakeMermaid(): FakeMermaid {
  return {
    initialize: () => {},
    render: async () => ({ svg: FAKE_SVG }),
  };
}

/**
 * Build a fresh `<pre><code class="language-mermaid">…</code></pre>` in the
 * document body and run the private renderBlock() against it. Returns the
 * mermaid block wrapper that the renderer created.
 */
async function renderInto(source: string): Promise<HTMLElement> {
  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.className = "language-mermaid";
  code.textContent = source;
  pre.appendChild(code);
  document.body.appendChild(pre);

  const renderer = new MermaidRenderer();
  // Inject stub mermaid to avoid loading the real dependency.
  (renderer as unknown as { mermaid: FakeMermaid }).mermaid = makeFakeMermaid();
  // renderBlock is private in the type but exists on the instance.
  await (
    renderer as unknown as { renderBlock: (el: HTMLElement) => Promise<void> }
  ).renderBlock(code);

  const wrapper = document.querySelector<HTMLElement>(".mermaid-block-wrapper");
  if (!wrapper) throw new Error("mermaid-block-wrapper not created");
  return wrapper;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("MermaidRenderer toolbar order and Spread attributes (TS-1 / TS-2)", () => {
  test("TS-1: toolbar has exactly [chartBtn, codeBtn, spreadBtn, copyBtn] in that DOM order", async () => {
    const wrapper = await renderInto("graph TD; A-->B");
    const toolbar = wrapper.querySelector<HTMLElement>(".mermaid-toolbar");
    expect(toolbar).not.toBeNull();
    const buttons = Array.from(
      toolbar?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    );
    expect(buttons.length).toBe(4);
    expect(buttons[0]?.classList.contains("mermaid-view-btn")).toBe(true);
    expect(buttons[0]?.dataset.mode).toBe("diagram");
    expect(buttons[1]?.classList.contains("mermaid-view-btn")).toBe(true);
    expect(buttons[1]?.dataset.mode).toBe("code");
    expect(buttons[2]?.classList.contains("mermaid-spread-btn")).toBe(true);
    expect(buttons[3]?.classList.contains("mermaid-copy-btn")).toBe(true);
  });

  test("TS-2: Spread button has type=button, correct aria-label, and mermaid-spread-btn class", async () => {
    const wrapper = await renderInto("graph TD; A-->B");
    const spread = wrapper.querySelector<HTMLButtonElement>(
      ".mermaid-spread-btn",
    );
    expect(spread).not.toBeNull();
    expect(spread?.type).toBe("button");
    expect(spread?.getAttribute("aria-label")).toBe("Enlarge diagram");
    expect(spread?.classList.contains("mermaid-spread-btn")).toBe(true);
  });
});

describe("MermaidRenderer copy button (TS-3 / TS-4)", () => {
  test("TS-3: click writes the exact source to the clipboard and shows .copy-success then reverts", async () => {
    const source = "graph TD; A-->B";
    const wrapper = await renderInto(source);

    const writes: string[] = [];
    const originalClipboard = (navigator as unknown as { clipboard?: unknown })
      .clipboard;
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: (s: string) => {
          writes.push(s);
          return Promise.resolve();
        },
      },
      configurable: true,
    });

    try {
      const copyBtn =
        wrapper.querySelector<HTMLButtonElement>(".mermaid-copy-btn");
      expect(copyBtn).not.toBeNull();
      if (!copyBtn) throw new Error();

      copyBtn.click();

      // Flush the microtask chain around the clipboard write.
      await Promise.resolve();
      await Promise.resolve();

      expect(writes).toEqual([source]);
      expect(copyBtn.classList.contains("copy-success")).toBe(true);
      const icon = copyBtn.querySelector<HTMLElement>(".copy-icon");
      expect(icon?.textContent).toBe("Copied!");

      // Should revert after ~1500ms.
      await new Promise((r) => setTimeout(r, 1600));
      expect(copyBtn.classList.contains("copy-success")).toBe(false);
      expect(copyBtn.classList.contains("copy-error")).toBe(false);
      expect(icon?.textContent).toBe("Copy code");
    } finally {
      if (originalClipboard === undefined) {
        delete (navigator as unknown as { clipboard?: unknown }).clipboard;
      } else {
        Object.defineProperty(navigator, "clipboard", {
          value: originalClipboard,
          configurable: true,
        });
      }
    }
  });

  test("TS-4: rejection triggers .copy-error, logs console.warn, and reverts", async () => {
    const source = "sequenceDiagram; Alice->>Bob: hi";
    const wrapper = await renderInto(source);

    const originalClipboard = (navigator as unknown as { clipboard?: unknown })
      .clipboard;
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: () => Promise.reject(new Error("nope")),
      },
      configurable: true,
    });
    const originalWarn = console.warn;
    const warnCalls: unknown[][] = [];
    console.warn = (...args: unknown[]) => {
      warnCalls.push(args);
    };

    try {
      const copyBtn =
        wrapper.querySelector<HTMLButtonElement>(".mermaid-copy-btn");
      if (!copyBtn) throw new Error();

      copyBtn.click();
      // Let the rejected promise settle.
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();

      expect(copyBtn.classList.contains("copy-error")).toBe(true);
      expect(copyBtn.classList.contains("copy-success")).toBe(false);
      const icon = copyBtn.querySelector<HTMLElement>(".copy-icon");
      expect(icon?.textContent).toBe("Failed");
      // At least one console.warn should have fired.
      expect(warnCalls.length).toBeGreaterThan(0);

      // Should revert after ~1500ms.
      await new Promise((r) => setTimeout(r, 1600));
      expect(copyBtn.classList.contains("copy-error")).toBe(false);
      expect(icon?.textContent).toBe("Copy code");
    } finally {
      console.warn = originalWarn;
      if (originalClipboard === undefined) {
        delete (navigator as unknown as { clipboard?: unknown }).clipboard;
      } else {
        Object.defineProperty(navigator, "clipboard", {
          value: originalClipboard,
          configurable: true,
        });
      }
    }
  });
});
