/**
 * CSS class-based styling cache for terminal performance optimization.
 *
 * Instead of inline styles, we generate CSS classes dynamically
 * and cache them for reuse. This significantly reduces DOM style
 * recalculation overhead.
 */

import type { CellAttributes } from "./attributes.ts";
import { getEffectiveForeground, getEffectiveBackground, attributesEqual } from "./attributes.ts";
import type { Rgb } from "./colors.ts";

/**
 * Performance metrics for style cache.
 */
export interface StyleCacheMetrics {
  /** Number of cached classes. */
  cachedClasses: number;
  /** Cache hit count. */
  hits: number;
  /** Cache miss count. */
  misses: number;
  /** Hit rate percentage. */
  hitRate: number;
}

/**
 * Style cache for CSS class-based rendering.
 *
 * Generates unique CSS class names for attribute combinations
 * and caches them for efficient reuse.
 */
export class StyleCache {
  /** Map from attribute hash to CSS class name. */
  private classMap: Map<string, string> = new Map();

  /** Counter for generating unique class names. */
  private classCounter: number = 0;

  /** The style element containing generated CSS. */
  private styleElement: HTMLStyleElement | null = null;

  /** Pending CSS rules to be added. */
  private pendingRules: string[] = [];

  /** Whether we have pending rules to flush. */
  private hasPendingRules: boolean = false;

  /** Cache hit counter. */
  private hits: number = 0;

  /** Cache miss counter. */
  private misses: number = 0;

  /** Cached attribute objects for comparison. */
  private attrCache: Map<string, CellAttributes> = new Map();

  constructor() {
    this.initStyleElement();
  }

  /**
   * Initialize the style element in the document head.
   */
  private initStyleElement(): void {
    // Remove existing style element if present
    const existingStyle = document.getElementById("terminal-style-cache");
    if (existingStyle) {
      existingStyle.remove();
    }

    this.styleElement = document.createElement("style");
    this.styleElement.id = "terminal-style-cache";
    document.head.appendChild(this.styleElement);

    // Add base terminal styles
    this.addBaseStyles();
  }

  /**
   * Add base terminal styles.
   */
  private addBaseStyles(): void {
    const baseCSS = `
      .term-span {
        display: inline;
        white-space: pre;
      }
      .term-bold { font-weight: bold; }
      .term-dim { opacity: 0.5; }
      .term-italic { font-style: italic; }
      .term-underline { text-decoration: underline; }
      .term-strikethrough { text-decoration: line-through; }
      .term-underline-strikethrough { text-decoration: underline line-through; }
      .term-blink { animation: term-blink 1s step-end infinite; }
      .term-hidden { visibility: hidden; }
      @keyframes term-blink {
        0%, 50% { opacity: 1; }
        51%, 100% { opacity: 0; }
      }
    `;

    if (this.styleElement) {
      this.styleElement.textContent = baseCSS;
    }
  }

  /**
   * Generate a hash key for attributes.
   * This is used for fast lookup in the cache.
   */
  private hashAttributes(attrs: CellAttributes): string {
    const fg = getEffectiveForeground(attrs);
    const bg = getEffectiveBackground(attrs);

    // Create a compact string representation
    const parts: string[] = [];

    // Colors
    parts.push(`f${fg.r},${fg.g},${fg.b}`);
    if (bg !== null) {
      parts.push(`b${bg.r},${bg.g},${bg.b}`);
    }

    // Boolean flags as a bitmask
    let flags = 0;
    if (attrs.bold) flags |= 1;
    if (attrs.dim) flags |= 2;
    if (attrs.italic) flags |= 4;
    if (attrs.underline) flags |= 8;
    if (attrs.blink) flags |= 16;
    if (attrs.hidden) flags |= 32;
    if (attrs.strikethrough) flags |= 64;

    parts.push(`x${flags}`);

    return parts.join("|");
  }

  /**
   * Get or create a CSS class for the given attributes.
   *
   * @param attrs - Cell attributes
   * @returns CSS class name to apply
   */
  getClass(attrs: CellAttributes): string {
    const hash = this.hashAttributes(attrs);

    // Check cache
    let className = this.classMap.get(hash);
    if (className !== undefined) {
      this.hits++;
      return className;
    }

    this.misses++;

    // Generate new class
    className = `ts${this.classCounter++}`;
    this.classMap.set(hash, className);
    this.attrCache.set(hash, { ...attrs });

    // Generate CSS rule
    const rule = this.generateCSSRule(className, attrs);
    this.pendingRules.push(rule);
    this.hasPendingRules = true;

    return className;
  }

  /**
   * Generate a CSS rule for the given class and attributes.
   */
  private generateCSSRule(className: string, attrs: CellAttributes): string {
    const styles: string[] = [];

    // Foreground color
    const fg = getEffectiveForeground(attrs);
    styles.push(`color: rgb(${fg.r}, ${fg.g}, ${fg.b})`);

    // Background color
    const bg = getEffectiveBackground(attrs);
    if (bg !== null) {
      styles.push(`background-color: rgb(${bg.r}, ${bg.g}, ${bg.b})`);
    }

    return `.${className} { ${styles.join("; ")}; }`;
  }

  /**
   * Get additional CSS classes for text decoration attributes.
   * These are reusable base classes.
   */
  getDecorationClasses(attrs: CellAttributes): string {
    const classes: string[] = [];

    if (attrs.bold) classes.push("term-bold");
    if (attrs.dim) classes.push("term-dim");
    if (attrs.italic) classes.push("term-italic");
    if (attrs.blink) classes.push("term-blink");
    if (attrs.hidden) classes.push("term-hidden");

    // Handle underline and strikethrough combination
    if (attrs.underline && attrs.strikethrough) {
      classes.push("term-underline-strikethrough");
    } else if (attrs.underline) {
      classes.push("term-underline");
    } else if (attrs.strikethrough) {
      classes.push("term-strikethrough");
    }

    return classes.join(" ");
  }

  /**
   * Flush pending CSS rules to the stylesheet.
   * Should be called after batch processing.
   */
  flush(): void {
    if (!this.hasPendingRules || !this.styleElement) return;

    // Append new rules to the stylesheet
    const newCSS = this.pendingRules.join("\n");
    this.styleElement.textContent += "\n" + newCSS;

    this.pendingRules = [];
    this.hasPendingRules = false;
  }

  /**
   * Get cache metrics for debugging/monitoring.
   */
  getMetrics(): StyleCacheMetrics {
    const total = this.hits + this.misses;
    return {
      cachedClasses: this.classMap.size,
      hits: this.hits,
      misses: this.misses,
      hitRate: total > 0 ? (this.hits / total) * 100 : 0,
    };
  }

  /**
   * Reset the cache (useful after terminal reset or major changes).
   */
  reset(): void {
    this.classMap.clear();
    this.attrCache.clear();
    this.classCounter = 0;
    this.pendingRules = [];
    this.hasPendingRules = false;
    this.hits = 0;
    this.misses = 0;

    if (this.styleElement) {
      this.addBaseStyles();
    }
  }

  /**
   * Destroy the cache and clean up DOM elements.
   */
  destroy(): void {
    if (this.styleElement) {
      this.styleElement.remove();
      this.styleElement = null;
    }
    this.classMap.clear();
    this.attrCache.clear();
  }
}

/**
 * Global singleton instance of the style cache.
 */
let globalStyleCache: StyleCache | null = null;

/**
 * Get the global style cache instance.
 */
export function getStyleCache(): StyleCache {
  if (globalStyleCache === null) {
    globalStyleCache = new StyleCache();
  }
  return globalStyleCache;
}

/**
 * Reset the global style cache.
 */
export function resetStyleCache(): void {
  if (globalStyleCache !== null) {
    globalStyleCache.reset();
  }
}
