/**
 * Mermaid diagram renderer for fullscreen Markdown view.
 *
 * Lazy-loads mermaid.js and renders mermaid code blocks as SVG diagrams
 * with a Code/Chart toggle UI. Source code is preserved for copy functionality.
 *
 * @module markdown/mermaid-renderer
 */

import { t } from "../i18n/index.ts";
import { openMermaidPopup } from "./mermaid-popup.ts";

/** Mermaid API interface for dynamic import */
interface MermaidAPI {
  initialize: (config: Record<string, unknown>) => void;
  render: (
    id: string,
    source: string,
    container?: HTMLElement,
  ) => Promise<{ svg: string }>;
}

/** Chart icon SVG (flowchart nodes) */
const CHART_ICON = `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="1" width="6" height="3.5" rx="1"/><rect x="4" y="9.5" width="6" height="3.5" rx="1"/><line x1="7" y1="4.5" x2="7" y2="9.5"/><polyline points="4.5,7.5 7,9.5 9.5,7.5"/></svg>`;

/** Code icon SVG (angle brackets) */
const CODE_ICON = `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="5,3 1.5,7 5,11"/><polyline points="9,3 12.5,7 9,11"/></svg>`;

/** Spread icon SVG (diagonal expansion arrows) */
const SPREAD_ICON = `<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="1.5,5 1.5,1.5 5,1.5"/><polyline points="12.5,5 12.5,1.5 9,1.5"/><polyline points="1.5,9 1.5,12.5 5,12.5"/><polyline points="12.5,9 12.5,12.5 9,12.5"/><line x1="1.5" y1="1.5" x2="5.5" y2="5.5"/><line x1="12.5" y1="1.5" x2="8.5" y2="5.5"/><line x1="1.5" y1="12.5" x2="5.5" y2="8.5"/><line x1="12.5" y1="12.5" x2="8.5" y2="8.5"/></svg>`;

/** Feedback timeout for the copy button's success / error label revert. */
const COPY_FEEDBACK_MS = 1500;

/**
 * Attaches a `click` handler that writes the mermaid `source` to the
 * clipboard and updates the button UI with success / error feedback that
 * reverts to the original label after {@link COPY_FEEDBACK_MS}.
 */
function attachMermaidCopyHandler(
  copyBtn: HTMLButtonElement,
  copyIcon: HTMLElement,
  source: string,
): void {
  const originalLabel = t("markdown.copyCode");

  const setFeedback = (
    cls: "copy-success" | "copy-error",
    labelKey: string,
  ): void => {
    copyBtn.classList.remove("copy-success", "copy-error");
    copyBtn.classList.add(cls);
    const label = t(labelKey);
    copyBtn.setAttribute("aria-label", label);
    copyIcon.textContent = label;
  };
  const restore = (): void => {
    copyBtn.classList.remove("copy-success", "copy-error");
    copyBtn.setAttribute("aria-label", originalLabel);
    copyIcon.textContent = originalLabel;
  };

  copyBtn.addEventListener("click", () => {
    // Guard against environments where the Clipboard API is unavailable
    // (older WebViews) — treat it as a failure so the error UI still shows.
    const clipboard = navigator.clipboard;
    const write =
      clipboard && typeof clipboard.writeText === "function"
        ? clipboard.writeText(source)
        : Promise.reject(new Error("navigator.clipboard is unavailable"));

    write
      .then(() => {
        setFeedback("copy-success", "markdown.copySuccess");
        setTimeout(restore, COPY_FEEDBACK_MS);
      })
      .catch((err: unknown) => {
        console.warn("[WARN][FRONTEND] MermaidRenderer: copy failed", err);
        setFeedback("copy-error", "markdown.copyFailed");
        setTimeout(restore, COPY_FEEDBACK_MS);
      });
  });
}

/**
 * Renders mermaid code blocks as SVG diagrams with toggle support.
 *
 * Uses lazy loading to avoid loading mermaid.js when no mermaid blocks exist.
 * Provides Code/Chart toggle to switch between source code and rendered diagram.
 */
export class MermaidRenderer {
  /** Cached mermaid instance */
  private mermaid: MermaidAPI | null = null;

  /** Counter for generating unique render IDs */
  private renderCounter = 0;

  /**
   * Render all mermaid code blocks in the given container.
   *
   * Scans for `pre > code.language-mermaid` elements, lazy-loads mermaid.js
   * if any are found, and replaces them with SVG diagrams with toggle UI.
   *
   * @param container - DOM element containing rendered Markdown
   */
  async renderAll(container: HTMLElement): Promise<void> {
    const codeBlocks = this.findMermaidBlocks(container);
    if (codeBlocks.length === 0) return;

    await this.ensureInitialized();

    for (const codeElement of codeBlocks) {
      await this.renderBlock(codeElement);
    }
  }

  /**
   * Find all mermaid code blocks in the container.
   */
  private findMermaidBlocks(container: HTMLElement): HTMLElement[] {
    const selector =
      "pre > code.language-mermaid, pre > code.hljs.language-mermaid";
    return Array.from(container.querySelectorAll<HTMLElement>(selector));
  }

  /**
   * Lazy-load and initialize mermaid.js with dark theme.
   */
  private async ensureInitialized(): Promise<void> {
    if (this.mermaid) return;

    const mermaidModule = await import("mermaid");
    this.mermaid = mermaidModule.default;

    this.mermaid.initialize({
      startOnLoad: false,
      suppressErrorRendering: true,
      theme: "dark",
      securityLevel: "strict",
      fontFamily: "sans-serif",
      themeVariables: {
        darkMode: true,
        background: "#2d2d2d",
        primaryColor: "#3b3b5c",
        primaryTextColor: "#d4d4d4",
        primaryBorderColor: "#6c6c9c",
        lineColor: "#808080",
        secondaryColor: "#2d2d4d",
        tertiaryColor: "#1e1e3e",
        noteBkgColor: "#2d2d2d",
        noteTextColor: "#d4d4d4",
        noteBorderColor: "#505050",
        actorBkg: "#2d2d4d",
        actorTextColor: "#d4d4d4",
        actorBorder: "#6c6c9c",
        signalColor: "#d4d4d4",
        signalTextColor: "#d4d4d4",
      },
      sequence: {
        mirrorActors: false,
        useMaxWidth: true,
      },
    });
  }

  /**
   * Render a single mermaid code block with toolbar (Chart/Code/Copy).
   *
   * Creates a structure within the existing code-block-wrapper:
   * - mermaid-block: contains diagram view and source view
   * - mermaid-toolbar: Chart icon, Code icon, Copy text button (top-right on hover)
   *
   * On failure, leaves the original code block unchanged.
   */
  private async renderBlock(codeElement: HTMLElement): Promise<void> {
    if (!this.mermaid) return;

    const pre = codeElement.parentElement;
    if (!pre) return;

    const source = codeElement.textContent || "";
    const id = `mermaid-${++this.renderCounter}`;

    // Use a hidden container to prevent mermaid from polluting document.body
    const hiddenContainer = document.createElement("div");
    hiddenContainer.style.cssText =
      "position:absolute;left:-9999px;top:-9999px;width:800px;height:600px;overflow:hidden;visibility:hidden";
    document.body.appendChild(hiddenContainer);

    try {
      const { svg } = await this.mermaid.render(id, source, hiddenContainer);

      // Find the code-block-wrapper created by addCopyButtons()
      const existingWrapper = pre.parentElement?.classList.contains(
        "code-block-wrapper",
      )
        ? pre.parentElement
        : null;

      // Build mermaid block structure
      const block = document.createElement("div");
      block.className = "mermaid-block";
      block.dataset.view = "diagram";
      block.setAttribute("data-mermaid-source", source);

      // Diagram container
      const diagramContainer = document.createElement("div");
      diagramContainer.className = "mermaid-diagram";
      diagramContainer.innerHTML = svg;

      // Source code container (clone original pre)
      const sourceContainer = document.createElement("div");
      sourceContainer.className = "mermaid-source";
      sourceContainer.style.display = "none";
      sourceContainer.appendChild(pre.cloneNode(true));

      block.appendChild(diagramContainer);
      block.appendChild(sourceContainer);

      // Build toolbar (Chart icon | Code icon | Copy text)
      const toolbar = document.createElement("div");
      toolbar.className = "mermaid-toolbar";

      const chartBtn = document.createElement("button");
      chartBtn.className = "mermaid-view-btn active";
      chartBtn.dataset.mode = "diagram";
      chartBtn.type = "button";
      chartBtn.setAttribute("aria-label", t("markdown.mermaidChart"));
      chartBtn.innerHTML = CHART_ICON;

      const codeBtn = document.createElement("button");
      codeBtn.className = "mermaid-view-btn";
      codeBtn.dataset.mode = "code";
      codeBtn.type = "button";
      codeBtn.setAttribute("aria-label", t("markdown.mermaidCode"));
      codeBtn.innerHTML = CODE_ICON;

      const spreadBtn = document.createElement("button");
      spreadBtn.className = "mermaid-spread-btn";
      spreadBtn.type = "button";
      spreadBtn.setAttribute("aria-label", t("markdown.mermaidSpread"));
      spreadBtn.innerHTML = SPREAD_ICON;
      spreadBtn.addEventListener("click", () => {
        const svgEl = diagramContainer.querySelector("svg");
        if (!svgEl) return;
        openMermaidPopup({
          svg: svgEl as SVGElement,
          triggerButton: spreadBtn,
        });
      });

      const copyBtn = document.createElement("button");
      copyBtn.className = "copy-code-button mermaid-copy-btn";
      copyBtn.type = "button";
      copyBtn.setAttribute("aria-label", t("markdown.copyCode"));
      const copyIcon = document.createElement("span");
      copyIcon.className = "copy-icon";
      copyIcon.textContent = t("markdown.copyCode");
      copyBtn.appendChild(copyIcon);

      // Wire the copy button so clicking writes the mermaid source to the
      // clipboard with visible success / error feedback (~1.5s revert).
      attachMermaidCopyHandler(copyBtn, copyIcon, source);

      toolbar.appendChild(chartBtn);
      toolbar.appendChild(codeBtn);
      toolbar.appendChild(spreadBtn);
      toolbar.appendChild(copyBtn);

      // Replace in DOM
      if (existingWrapper) {
        // Remove the old standalone copy button
        const oldCopyBtn = existingWrapper.querySelector(
          ":scope > .copy-code-button",
        );
        oldCopyBtn?.remove();
        existingWrapper.replaceChild(block, pre);
        existingWrapper.appendChild(toolbar);
        existingWrapper.classList.add("mermaid-block-wrapper");
      } else {
        // Create wrapper if none exists
        const wrapper = document.createElement("div");
        wrapper.className = "code-block-wrapper mermaid-block-wrapper";
        pre.parentNode?.insertBefore(wrapper, pre);
        wrapper.appendChild(block);
        wrapper.appendChild(toolbar);
        pre.remove();
      }

      // Toggle event handler
      toolbar.addEventListener("click", (e) => {
        const target = (e.target as HTMLElement).closest(".mermaid-view-btn");
        if (!target) return;
        const mode = (target as HTMLElement).dataset.mode;
        if (!mode) return;

        block.dataset.view = mode;

        toolbar
          .querySelectorAll(".mermaid-view-btn")
          .forEach((btn) => btn.classList.toggle("active", btn === target));

        diagramContainer.style.display = mode === "diagram" ? "" : "none";
        sourceContainer.style.display = mode === "code" ? "" : "none";
      });
    } catch (err) {
      console.warn(
        "[WARN][FRONTEND] MermaidRenderer: failed to render block",
        err,
      );

      // Clean up any mermaid-injected elements that leaked into document.body
      // Mermaid creates a temporary div with id "d" + renderId in document.body
      const leakedEl = document.getElementById(`d${id}`);
      if (leakedEl) {
        leakedEl.remove();
      }

      // Show error banner above the preserved code block
      const wrapper = pre.parentElement?.classList.contains(
        "code-block-wrapper",
      )
        ? pre.parentElement
        : pre.parentNode;
      if (wrapper) {
        const banner = document.createElement("div");
        banner.className = "mermaid-error-banner";
        banner.textContent = t("markdown.mermaidSyntaxError");
        wrapper.insertBefore(banner, pre);
      }
    } finally {
      hiddenContainer.remove();
    }
  }
}
