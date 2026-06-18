/**
 * Standalone Markdown viewer entry (native-poc child window).
 *
 * Reuses the WebView Markdown renderer (`src/markdown`) and the appearance
 * appliers (`src/settings`) to render a single injected document inside the
 * Wry/WebKitGTK child window. `src/` is imported but never modified
 * (branch policy / NFR3).
 *
 * The host (the Rust child viewer, Phase 4) injects the render payload as a
 * global `window.__EMTERM_VIEWER_PAYLOAD__` before this module runs, or
 * sets it afterwards and dispatches `emterm-viewer-payload`. Both paths are
 * supported so the bundle does not depend on script ordering.
 *
 * @module native-poc/viewer/entry
 */

import { MarkdownRenderer } from "../../web-shared/markdown/renderer.ts";
import { OutlinePanel } from "../../web-shared/markdown/outline.ts";
import { MermaidRenderer } from "../../web-shared/markdown/mermaid-renderer.ts";
import {
  applyMarkdownColorTheme,
  applyMarkdownSettings,
} from "../../web-shared/settings/settings-applier.ts";

import type { MarkdownFormat } from "../../web-shared/markdown/types.ts";
import type {
  UiTheme,
  UiThemePreset,
} from "../../web-shared/settings/types.ts";

// Pull the shared markdown styling in so the rendered content matches the
// WebView fullscreen overlay. Import-only — the source CSS is unchanged.
import "../../web-shared/markdown/fullscreen.css";
import "../../web-shared/markdown/outline.css";

/**
 * Resolved appearance carried from native-poc settings (Phase 1 resolver).
 * `follow_ui` is already applied on the Rust side, so `theme`/`preset` are
 * the *effective* values and the page applies them directly.
 */
export interface ViewerAppearance {
  theme: UiTheme;
  preset: UiThemePreset;
  bodyFontFamily: string;
  codeFontFamily: string;
  emojiFontFamily: string;
  fontSize: number;
}

/** The full render payload injected by the native host. */
export interface ViewerPayload {
  markdown: string;
  format: MarkdownFormat;
  basedir?: string;
  appearance: ViewerAppearance;
}

declare global {
  interface Window {
    __EMTERM_VIEWER_PAYLOAD__?: ViewerPayload;
  }
}

/**
 * Parse a JSON string injected by the Rust host into a {@link ViewerPayload}.
 *
 * The wire shape is owned by the Rust `ViewerPayload`/`PayloadAppearance`
 * structs (`native-poc/src/viewer/launch.rs`); this is the TS half of the
 * H5 contract test, which validates the same committed fixture both sides
 * share. The cast is intentional — the host is trusted to emit the agreed
 * field names; the contract test guards against drift.
 */
export function parsePayload(json: string): ViewerPayload {
  return JSON.parse(json) as ViewerPayload;
}

/**
 * Apply the resolved appearance to the document root.
 *
 * The native resolver already honored `markdown_theme_follow_ui`, so we
 * pass `followUi: false` with the effective theme/preset and let the
 * shared applier write the `--markdown-*` CSS variables.
 */
export function applyAppearance(appearance: ViewerAppearance): void {
  applyMarkdownSettings(
    appearance.bodyFontFamily,
    appearance.codeFontFamily,
    appearance.emojiFontFamily,
    appearance.fontSize,
  );
  applyMarkdownColorTheme({
    followUi: false,
    mdTheme: appearance.theme,
    mdPreset: appearance.preset,
    // uiTheme/uiPreset are unused when followUi is false, but the
    // applier requires the shape; mirror the effective values.
    uiTheme: appearance.theme,
    uiPreset: appearance.preset,
  });
}

/**
 * Render one payload into `root`, building the markdown content, the
 * outline/TOC panel, and (asynchronously) any mermaid diagrams.
 *
 * Returns the content element so callers/tests can inspect the DOM.
 */
export function renderPayload(
  root: HTMLElement,
  payload: ViewerPayload,
): HTMLElement {
  applyAppearance(payload.appearance);

  const renderer = new MarkdownRenderer();
  const html = renderer.render(payload.markdown, payload.format);

  // Mirror the WebView fullscreen overlay structure so the reused
  // fullscreen.css selectors (`.markdown-fullscreen-content …`) match.
  root.replaceChildren();
  const overlay = document.createElement("div");
  overlay.className = "markdown-fullscreen-overlay";
  const content = document.createElement("div");
  content.className = "markdown-fullscreen-content";
  content.tabIndex = 0;
  content.innerHTML = html;
  overlay.appendChild(content);
  root.appendChild(overlay);

  // Rewrite local-image placeholders inserted by MarkdownRenderer.markLocalImages()
  // to the custom scheme served by the Rust child viewer. The Rust side resolves
  // relative paths against basedir and enforces confinement.
  for (const img of content.querySelectorAll<HTMLImageElement>(
    "img[data-local-src]",
  )) {
    const localPath =
      img.dataset.localSrc ?? img.getAttribute("data-local-src") ?? "";
    if (
      !localPath ||
      localPath.startsWith("data:") ||
      /^[a-zA-Z][a-zA-Z0-9+\-.]*:\/\//.test(localPath)
    ) {
      continue;
    }
    img.src = "emterm-viewer://localhost/__img/" + encodeURI(localPath);
  }

  // Outline/TOC panel (h1–h3). Prepend when present.
  try {
    const outline = new OutlinePanel();
    const panel = outline.build(content);
    if (panel) {
      overlay.classList.add("has-outline");
      overlay.insertBefore(panel, content);
    }
  } catch (e) {
    console.warn("viewer: outline build failed", e);
  }

  // Mermaid diagrams render asynchronously (lazy-loaded).
  void (async () => {
    try {
      const mermaid = new MermaidRenderer();
      await mermaid.renderAll(content);
    } catch (e) {
      console.warn("viewer: mermaid render failed", e);
    }
  })();

  // Focus the content so keyboard scrolling works immediately.
  content.focus();
  return content;
}

/** Resolve the viewer root element, creating one if the host omitted it. */
function viewerRoot(): HTMLElement {
  let root = document.getElementById("viewer-root");
  if (!root) {
    root = document.createElement("div");
    root.id = "viewer-root";
    document.body.appendChild(root);
  }
  return root;
}

/** Boot: render an already-injected payload, or wait for one. */
function boot(): void {
  const root = viewerRoot();
  if (window.__EMTERM_VIEWER_PAYLOAD__) {
    renderPayload(root, window.__EMTERM_VIEWER_PAYLOAD__);
    return;
  }
  // Payload not yet present — render when the host announces it.
  window.addEventListener(
    "emterm-viewer-payload",
    () => {
      if (window.__EMTERM_VIEWER_PAYLOAD__) {
        renderPayload(root, window.__EMTERM_VIEWER_PAYLOAD__);
      }
    },
    { once: true },
  );
}

// Only auto-boot in a real document (skipped under unit tests that import
// the pure functions above).
if (typeof document !== "undefined" && document.getElementById("viewer-root")) {
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot, { once: true });
  } else {
    boot();
  }
}
