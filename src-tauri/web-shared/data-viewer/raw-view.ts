/**
 * RAW view component for data viewer.
 *
 * Displays file content with syntax highlighting and copy button.
 *
 * @module data-viewer/raw-view
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { highlightData } from "./highlighter.ts";
import type { DataFormat } from "./types.ts";

/**
 * Create a RAW view element.
 *
 * @param text - Raw text content to display
 * @param format - Data format for syntax highlighting
 * @returns Container element with highlighted content and copy button
 */
export function createRawView(text: string, format: DataFormat, plainText = false): HTMLElement {
  const container = document.createElement("div");
  container.className = "dv-raw-view";

  const pre = document.createElement("pre");
  pre.className = "dv-raw-content";
  if (plainText) {
    pre.textContent = text;
  } else {
    pre.innerHTML = highlightData(text, format);
  }

  const copyBtn = document.createElement("button");
  copyBtn.className = "dv-copy-btn";
  copyBtn.textContent = "Copy";
  copyBtn.addEventListener("click", async () => {
    try {
      await writeText(text);
      copyBtn.textContent = "Copied!";
      setTimeout(() => {
        copyBtn.textContent = "Copy";
      }, 2000);
    } catch {
      try {
        await navigator.clipboard.writeText(text);
        copyBtn.textContent = "Copied!";
        setTimeout(() => {
          copyBtn.textContent = "Copy";
        }, 2000);
      } catch {
        // Ignore clipboard errors
      }
    }
  });

  container.appendChild(pre);
  container.appendChild(copyBtn);

  return container;
}

/**
 * Update RAW view content with new text (for pretty-print toggle).
 */
export function updateRawViewContent(
  container: HTMLElement,
  text: string,
  format: DataFormat,
): void {
  const pre = container.querySelector(".dv-raw-content");
  if (pre) {
    pre.innerHTML = highlightData(text, format);
  }
}
