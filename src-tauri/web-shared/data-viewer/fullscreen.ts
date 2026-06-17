/**
 * Fullscreen overlay controller for data viewer.
 *
 * Manages the fullscreen overlay lifecycle, view mode toggling,
 * and keyboard dispatch.
 *
 * @module data-viewer/fullscreen
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { OutlineView } from "./outline.ts";
import { prettyPrintJson } from "./parser.ts";
import { createRawView, updateRawViewContent } from "./raw-view.ts";
import { isAncestorHidden } from "../shared/dom-utils.ts";
import type { DataFormat, TreeNode } from "./types.ts";

/** Display mode */
type ViewMode = "outline" | "raw";

/** Parameters for showing the viewer */
export interface DataViewerShowParams {
  format: DataFormat;
  rawText: string;
  parsedData: unknown;
  tree: TreeNode[];
  error: string | null;
  container: HTMLElement;
}

/**
 * Fullscreen data viewer overlay.
 */
export class DataViewerFullscreen {
  private overlay: HTMLElement | null = null;
  private contentArea: HTMLElement | null = null;
  private statusBar: HTMLElement | null = null;
  private container: HTMLElement | null = null;
  private outlineView: OutlineView | null = null;
  private rawViewElement: HTMLElement | null = null;
  private errorBanner: HTMLElement | null = null;

  private mode: ViewMode = "outline";
  private format: DataFormat = "json";
  private rawText = "";
  private parsedData: unknown = null;
  private prettyPrinted = false;
  private hasParseError = false;

  private isActiveState = false;
  private previouslyFocusedElement: HTMLElement | null = null;

  private boundHandleKeydown: (e: KeyboardEvent) => void;
  private onShowCallback: (() => void) | null = null;
  private onHideCallback: (() => void) | null = null;

  constructor() {
    this.boundHandleKeydown = this.handleKeydown.bind(this);
  }

  /**
   * Show the data viewer in fullscreen overlay.
   */
  show(params: DataViewerShowParams): void {
    if (this.isActiveState) this.close();

    this.container = params.container;
    this.format = params.format;
    this.rawText = params.rawText;
    this.parsedData = params.parsedData;
    this.hasParseError = params.error !== null;
    this.prettyPrinted = false;
    this.previouslyFocusedElement = document.activeElement as HTMLElement | null;

    // Create overlay
    this.overlay = document.createElement("div");
    this.overlay.className = "dv-fullscreen-overlay";
    this.overlay.setAttribute("role", "dialog");
    this.overlay.setAttribute("aria-modal", "true");

    // Error banner (if parse error)
    if (params.error) {
      this.errorBanner = document.createElement("div");
      this.errorBanner.className = "dv-error-banner";
      this.errorBanner.textContent = `Parse error: ${params.error}`;
      this.overlay.appendChild(this.errorBanner);
    }

    // Content area
    this.contentArea = document.createElement("div");
    this.contentArea.className = "dv-content-area";
    this.overlay.appendChild(this.contentArea);

    // Status bar
    this.statusBar = document.createElement("div");
    this.statusBar.className = "dv-status-bar";
    this.overlay.appendChild(this.statusBar);

    // Build views
    if (!this.hasParseError) {
      this.outlineView = new OutlineView(
        params.tree,
        params.format,
        params.parsedData,
      );
    }
    this.rawViewElement = createRawView(params.rawText, params.format, this.hasParseError);

    // Default mode
    if (this.hasParseError) {
      this.mode = "raw";
    } else {
      this.mode = "outline";
    }
    this.renderCurrentMode();

    // Insert into container
    this.container.appendChild(this.overlay);

    // Set up keyboard listener
    document.addEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });

    this.isActiveState = true;
    this.onShowCallback?.();

    this.contentArea.setAttribute("tabindex", "-1");
    this.contentArea.focus();
  }

  /**
   * Close the viewer.
   */
  close(): void {
    if (!this.isActiveState) return;

    document.removeEventListener("keydown", this.boundHandleKeydown, {
      capture: true,
    });

    this.outlineView?.dispose();
    this.outlineView = null;

    if (this.overlay) {
      this.overlay.remove();
      this.overlay = null;
      this.contentArea = null;
      this.statusBar = null;
      this.rawViewElement = null;
      this.errorBanner = null;
    }

    this.container = null;

    if (this.previouslyFocusedElement?.focus) {
      this.previouslyFocusedElement.focus();
    }
    this.previouslyFocusedElement = null;

    this.onHideCallback?.();
    this.isActiveState = false;
  }

  isActive(): boolean {
    return this.isActiveState;
  }

  onShow(callback: () => void): void {
    this.onShowCallback = callback;
  }

  onHide(callback: () => void): void {
    this.onHideCallback = callback;
  }

  dispose(): void {
    this.close();
  }

  private renderCurrentMode(): void {
    if (!this.contentArea || !this.statusBar) return;

    // Clear content area
    this.contentArea.innerHTML = "";

    if (this.mode === "outline" && this.outlineView) {
      this.contentArea.appendChild(this.outlineView.getElement());
    } else if (this.rawViewElement) {
      this.contentArea.appendChild(this.rawViewElement);
    }

    this.updateStatusBar();
  }

  private updateStatusBar(): void {
    if (!this.statusBar) return;

    const modeLabel = this.mode === "outline" ? "Outline" : "RAW";
    const formatLabel = this.format.toUpperCase();
    let shortcuts = "[r] Toggle  [Esc] Close";

    if (this.mode === "raw" && this.format === "json" && !this.hasParseError) {
      const ppState = this.prettyPrinted ? "on" : "off";
      shortcuts = `[r] Toggle  [p] Pretty (${ppState})  [Esc] Close`;
    }

    if (this.hasParseError) {
      shortcuts = "[Esc] Close";
    }

    this.statusBar.textContent = `${formatLabel} [${modeLabel}]  ${shortcuts}`;
  }

  private toggleMode(): void {
    if (this.hasParseError) return; // Can't switch to outline on parse error

    if (this.mode === "outline") {
      this.mode = "raw";
    } else {
      this.mode = "outline";
    }
    this.renderCurrentMode();
  }

  private togglePrettyPrint(): void {
    if (this.mode !== "raw" || this.format !== "json" || this.hasParseError)
      return;
    if (!this.rawViewElement) return;

    this.prettyPrinted = !this.prettyPrinted;

    const displayText = this.prettyPrinted
      ? prettyPrintJson(this.parsedData)
      : this.rawText;

    updateRawViewContent(this.rawViewElement, displayText, this.format);
    this.updateStatusBar();
  }

  private handleKeydown(e: KeyboardEvent): void {
    if (!this.isActiveState) return;
    if (this.overlay && isAncestorHidden(this.overlay)) return;

    // Handle Ctrl+C (copy) and Ctrl+A (select all) explicitly
    if ((e.ctrlKey || e.metaKey) && e.key === "c") {
      e.preventDefault();
      const sel = window.getSelection();
      const text = sel?.toString();
      if (text) {
        writeText(text).catch(() => {
          navigator.clipboard.writeText(text).catch(() => {});
        });
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key === "a") {
      e.preventDefault();
      const target = this.mode === "outline" && this.outlineView
        ? this.outlineView.getElement()
        : this.rawViewElement;
      if (target) {
        const sel = window.getSelection();
        const range = document.createRange();
        range.selectNodeContents(target);
        sel?.removeAllRanges();
        sel?.addRange(range);
      }
      return;
    }

    e.preventDefault();

    switch (e.key) {
      case "Escape":
        this.close();
        break;
      case "r":
        this.toggleMode();
        break;
      case "p":
        this.togglePrettyPrint();
        break;
      case "ArrowUp":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateUp();
        } else {
          this.scrollBy(-40);
        }
        break;
      case "ArrowDown":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateDown();
        } else {
          this.scrollBy(40);
        }
        break;
      case "PageUp":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateBy(-10);
        } else {
          this.scrollBy(-(this.contentArea?.clientHeight || 400));
        }
        break;
      case "PageDown":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateBy(10);
        } else {
          this.scrollBy(this.contentArea?.clientHeight || 400);
        }
        break;
      case "Home":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateHome();
        } else {
          this.scrollTo("top");
        }
        break;
      case "End":
        if (this.mode === "outline" && this.outlineView) {
          this.outlineView.navigateEnd();
        } else {
          this.scrollTo("bottom");
        }
        break;
      case " ":
        if (this.mode === "raw") {
          if (e.shiftKey) {
            this.scrollBy(-(this.contentArea?.clientHeight || 400) * 0.85);
          } else {
            this.scrollBy((this.contentArea?.clientHeight || 400) * 0.85);
          }
        }
        break;
    }
  }

  private scrollBy(amount: number): void {
    if (this.mode === "raw" && this.rawViewElement) {
      const pre = this.rawViewElement.querySelector(".dv-raw-content");
      if (pre) {
        pre.scrollBy({ top: amount, behavior: "auto" });
      }
    }
  }

  private scrollTo(position: "top" | "bottom"): void {
    if (this.mode === "raw" && this.rawViewElement) {
      const pre = this.rawViewElement.querySelector(".dv-raw-content");
      if (pre) {
        pre.scrollTo({
          top: position === "top" ? 0 : pre.scrollHeight,
          behavior: "auto",
        });
      }
    }
  }
}
