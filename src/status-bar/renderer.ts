/**
 * Status Bar Renderer
 *
 * DOM rendering, layer creation, and visibility management for the status bar.
 * Creates a 3-layer structure: OSC, App Line 1, App Line 2.
 * Each layer has left and right sections.
 */

import type { StatusBarLayer, StatusBarSection, StatusBarConfig } from "./types";

/**
 * StatusBarRenderer manages the DOM structure and visibility of the status bar.
 */
export class StatusBarRenderer {
  private container: HTMLElement;
  private layers: Map<StatusBarLayer, HTMLElement> = new Map();
  private sections: Map<string, HTMLElement> = new Map();

  constructor(container: HTMLElement) {
    this.container = container;
    this.buildDOM();
  }

  /**
   * Build the DOM structure for all layers and sections.
   */
  private buildDOM(): void {
    this.container.innerHTML = "";
    this.container.className = "status-bar";

    const layerIds: StatusBarLayer[] = ["osc", "app-line1", "app-line2"];
    for (const layerId of layerIds) {
      const layer = document.createElement("div");
      layer.className = `status-bar-layer status-bar-layer--${layerId}`;
      layer.dataset.layer = layerId;

      const left = document.createElement("span");
      left.className = "status-bar-section status-bar-section--left";
      left.dataset.section = "left";

      const right = document.createElement("span");
      right.className = "status-bar-section status-bar-section--right";
      right.dataset.section = "right";

      layer.appendChild(left);
      layer.appendChild(right);
      this.container.appendChild(layer);

      this.layers.set(layerId, layer);
      this.sections.set(`${layerId}:left`, left);
      this.sections.set(`${layerId}:right`, right);
    }

    // OSC layer and app-line2 are hidden by default (empty)
    this.updateLayerVisibility("osc");
    this.updateLayerVisibility("app-line2");
  }

  /**
   * Set content for a specific section.
   * Updates layer visibility based on whether content exists.
   */
  setContent(layer: StatusBarLayer, section: StatusBarSection, content: string): void {
    const el = this.sections.get(`${layer}:${section}`);
    if (!el) return;

    // Only update if content actually changed (differential rendering)
    if (el.innerHTML === content) return;
    el.innerHTML = content;

    this.updateLayerVisibility(layer);
  }

  /**
   * Get content for a specific section.
   */
  getContent(layer: StatusBarLayer, section: StatusBarSection): string {
    const el = this.sections.get(`${layer}:${section}`);
    return el?.innerHTML ?? "";
  }

  /**
   * Clear content for a specific section or all sections in a layer.
   */
  clearContent(layer: StatusBarLayer, section?: StatusBarSection): void {
    if (section) {
      this.setContent(layer, section, "");
    } else {
      this.setContent(layer, "left", "");
      this.setContent(layer, "right", "");
    }
  }

  /**
   * Update layer visibility based on content.
   * Layers with no content in either section are hidden.
   * App Line 1 is always visible when status bar is shown.
   */
  private updateLayerVisibility(layerId: StatusBarLayer): void {
    // App Line 1 is always visible
    if (layerId === "app-line1") return;

    const layer = this.layers.get(layerId);
    if (!layer) return;

    const leftContent = this.sections.get(`${layerId}:left`)?.textContent ?? "";
    const rightContent = this.sections.get(`${layerId}:right`)?.textContent ?? "";
    const hasContent = leftContent.length > 0 || rightContent.length > 0;

    layer.classList.toggle("hidden", !hasContent);
  }

  /**
   * Show or hide a specific layer explicitly.
   */
  setLayerVisible(layerId: StatusBarLayer, visible: boolean): void {
    const layer = this.layers.get(layerId);
    if (!layer) return;
    layer.classList.toggle("hidden", !visible);
  }

  /**
   * Apply appearance settings (colors, font size, opacity).
   */
  applyConfig(config: StatusBarConfig): void {
    const style = this.container.style;

    if (config.bgColor) {
      style.setProperty("--status-bar-bg", config.bgColor);
    } else {
      style.removeProperty("--status-bar-bg");
    }

    if (config.fgColor) {
      style.setProperty("--status-bar-fg", config.fgColor);
    } else {
      style.removeProperty("--status-bar-fg");
    }

    if (config.fontSize != null) {
      style.setProperty("--status-bar-font-size", `${config.fontSize}pt`);
    } else {
      style.removeProperty("--status-bar-font-size");
    }

    style.setProperty("--status-bar-opacity", String(config.opacity));
  }

  /**
   * Get the layer element for a given layer id.
   */
  getLayer(layerId: StatusBarLayer): HTMLElement | undefined {
    return this.layers.get(layerId);
  }

  /**
   * Get the section element for a given layer and section.
   */
  getSection(layer: StatusBarLayer, section: StatusBarSection): HTMLElement | undefined {
    return this.sections.get(`${layer}:${section}`);
  }

  /**
   * Dispose of the renderer, clearing DOM.
   */
  dispose(): void {
    this.container.innerHTML = "";
    this.layers.clear();
    this.sections.clear();
  }
}
