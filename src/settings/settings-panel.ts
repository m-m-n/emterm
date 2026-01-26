/**
 * Settings Panel
 *
 * Placeholder UI for settings display.
 */

/**
 * Options for creating SettingsPanel
 */
export interface SettingsPanelOptions {
  /** Container element for the settings panel */
  container: HTMLElement;
}

/**
 * SettingsPanel - Displays settings UI (placeholder)
 */
export class SettingsPanel {
  private container: HTMLElement;
  private panelElement: HTMLElement | null = null;

  constructor(options: SettingsPanelOptions) {
    this.container = options.container;
  }

  /**
   * Initializes the settings panel
   */
  init(): void {
    this.render();
  }

  /**
   * Renders the settings panel UI
   */
  private render(): void {
    this.container.innerHTML = "";
    this.container.className = "settings-panel";

    this.panelElement = document.createElement("div");
    this.panelElement.className = "settings-content";

    // Header
    const header = document.createElement("h1");
    header.className = "settings-header";
    header.textContent = "Settings";
    this.panelElement.appendChild(header);

    // Placeholder message
    const placeholder = document.createElement("p");
    placeholder.className = "settings-placeholder-text";
    placeholder.textContent =
      "Settings panel is under development. Configuration options will be available here.";
    this.panelElement.appendChild(placeholder);

    this.container.appendChild(this.panelElement);
  }

  /**
   * Gets the panel element (for testing)
   */
  getPanelElement(): HTMLElement | null {
    return this.panelElement;
  }

  /**
   * Disposes the settings panel
   */
  dispose(): void {
    this.container.innerHTML = "";
    this.panelElement = null;
  }
}
