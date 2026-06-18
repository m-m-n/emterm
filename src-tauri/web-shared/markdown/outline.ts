/**
 * Outline panel for fullscreen Markdown view.
 *
 * Extracts h1-h3 headings from rendered content and builds a
 * navigable outline panel with scroll-synced active tracking.
 *
 * @module markdown/outline
 */

import { t } from "../i18n/index.ts";

/** Heading info extracted from the content. */
interface HeadingInfo {
  /** Heading level (1-3) */
  level: number;
  /** Heading text content */
  text: string;
  /** Reference to the heading element */
  element: HTMLElement;
}

/**
 * Builds and manages a heading navigation outline panel.
 *
 * Scans rendered Markdown content for h1-h3 headings and creates
 * a clickable outline panel with IntersectionObserver-based active tracking.
 */
export class OutlinePanel {
  /** The panel DOM element */
  private panel: HTMLElement | null = null;

  /** IntersectionObserver for scroll tracking */
  private observer: IntersectionObserver | null = null;

  /** Map of heading element to outline item element */
  private headingToItem: Map<HTMLElement, HTMLElement> = new Map();

  /** Currently active outline item */
  private activeItem: HTMLElement | null = null;

  /** Content element reference for scroll navigation */
  private contentEl: HTMLElement | null = null;

  /**
   * Build the outline panel from content headings.
   *
   * @param contentElement - DOM element containing rendered Markdown
   * @returns Panel element, or null if no h1-h3 headings found
   */
  build(contentElement: HTMLElement): HTMLElement | null {
    this.dispose();
    this.contentEl = contentElement;

    const headings = this.extractHeadings(contentElement);
    if (headings.length === 0) return null;

    this.assignHeadingIds(headings);
    this.panel = this.buildDOM(headings);
    this.setupScrollTracking(headings);

    return this.panel;
  }

  /**
   * Dispose panel and release resources.
   */
  dispose(): void {
    if (this.observer) {
      this.observer.disconnect();
      this.observer = null;
    }
    this.headingToItem.clear();
    this.activeItem = null;
    this.panel = null;
    this.contentEl = null;
  }

  /**
   * Extract h1-h3 headings from the content element.
   */
  private extractHeadings(contentElement: HTMLElement): HeadingInfo[] {
    const headings: HeadingInfo[] = [];
    const elements = contentElement.querySelectorAll<HTMLElement>("h1, h2, h3");

    for (const el of elements) {
      const level = Number.parseInt(el.tagName.charAt(1), 10);
      headings.push({
        level,
        text: el.textContent?.trim() || "",
        element: el,
      });
    }

    return headings;
  }

  /**
   * Assign IDs to headings that don't have one.
   */
  private assignHeadingIds(headings: HeadingInfo[]): void {
    let counter = 0;
    for (const heading of headings) {
      if (!heading.element.id) {
        heading.element.id = `heading-${++counter}`;
      }
    }
  }

  /**
   * Build the outline panel DOM.
   */
  private buildDOM(headings: HeadingInfo[]): HTMLElement {
    const panel = document.createElement("nav");
    panel.className = "markdown-outline-panel";
    panel.setAttribute("role", "navigation");
    panel.setAttribute("aria-label", t("markdown.outline"));

    const list = document.createElement("div");
    list.className = "outline-list";

    for (const heading of headings) {
      const item = document.createElement("div");
      item.className = "outline-item";
      item.setAttribute("data-level", String(heading.level));
      item.textContent = heading.text;
      item.addEventListener("click", () => {
        heading.element.scrollIntoView({ behavior: "smooth" });
      });

      this.headingToItem.set(heading.element, item);
      list.appendChild(item);
    }

    panel.appendChild(list);
    return panel;
  }

  /**
   * Set up IntersectionObserver to track the active heading.
   */
  private setupScrollTracking(headings: HeadingInfo[]): void {
    if (headings.length === 0) return;

    const root = this.contentEl;

    this.observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const item = this.headingToItem.get(entry.target as HTMLElement);
          if (!item) continue;

          if (entry.isIntersecting) {
            this.activeItem?.classList.remove("outline-item-active");
            item.classList.add("outline-item-active");
            this.activeItem = item;
          }
        }
      },
      {
        root,
        rootMargin: "0px 0px -80% 0px",
        threshold: 0,
      },
    );

    for (const heading of headings) {
      this.observer.observe(heading.element);
    }
  }
}
