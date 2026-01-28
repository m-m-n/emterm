/**
 * Settings Panel
 *
 * UI component for application settings with category navigation.
 */

import { SettingsService } from "./settings-service";
import { applySettingsToCSS } from "./settings-applier";
import type { AppSettings } from "./types";
import { MIN_FONT_SIZE, MAX_FONT_SIZE } from "./types";

/**
 * Options for creating SettingsPanel
 */
export interface SettingsPanelOptions {
  /** Container element for the settings panel */
  container: HTMLElement;
}

/**
 * Category definition
 */
interface Category {
  id: string;
  label: string;
  enabled: boolean;
}

/**
 * SettingsPanel - Displays and manages application settings
 */
export class SettingsPanel {
  private container: HTMLElement;
  private navElement: HTMLElement | null = null;
  private contentElement: HTMLElement | null = null;
  private fontSizeInput: HTMLInputElement | null = null;
  private activeCategory = "appearance";
  private currentSettings: AppSettings | null = null;
  private lastSavedValue: number = 0;
  private eventListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
  }> = [];

  private readonly categories: Category[] = [
    { id: "appearance", label: "Appearance", enabled: true },
    { id: "terminal", label: "Terminal", enabled: false },
    { id: "keybinds", label: "Keybinds", enabled: false },
  ];

  constructor(options: SettingsPanelOptions) {
    this.container = options.container;
  }

  /**
   * Initializes the settings panel
   */
  async init(): Promise<void> {
    // Load current settings
    this.currentSettings = await SettingsService.load();
    this.lastSavedValue = this.currentSettings.font_size;

    this.render();
    this.attachEventListeners();
  }

  /**
   * Renders the settings panel UI
   */
  private render(): void {
    this.container.innerHTML = "";
    this.container.className = "settings-panel";

    // Create navigation with ARIA tablist
    this.navElement = document.createElement("nav");
    this.navElement.className = "settings-nav";
    this.navElement.setAttribute("role", "tablist");
    this.navElement.setAttribute("aria-orientation", "vertical");
    this.navElement.setAttribute("aria-label", "Settings categories");
    this.renderNavigation();
    this.container.appendChild(this.navElement);

    // Create content area
    this.contentElement = document.createElement("main");
    this.contentElement.className = "settings-content";
    this.renderContent();
    this.container.appendChild(this.contentElement);
  }

  /**
   * Renders the category navigation with ARIA tab pattern
   */
  private renderNavigation(): void {
    if (!this.navElement) return;

    this.navElement.innerHTML = "";

    for (const category of this.categories) {
      const button = document.createElement("button");
      button.className = "settings-nav-item";
      button.textContent = category.label;
      button.dataset.categoryId = category.id;

      // ARIA tab attributes
      button.setAttribute("role", "tab");
      button.id = `tab-${category.id}`;
      button.setAttribute("aria-controls", `panel-${category.id}`);

      const isActive = category.id === this.activeCategory;
      button.setAttribute("aria-selected", String(isActive));
      button.setAttribute("tabindex", isActive ? "0" : "-1");

      if (isActive) {
        button.classList.add("active");
      }

      if (!category.enabled) {
        button.classList.add("disabled");
        button.disabled = true;
        button.setAttribute("aria-disabled", "true");
      }

      this.navElement.appendChild(button);
    }
  }

  /**
   * Renders the content area for the active category with ARIA tabpanel
   */
  private renderContent(): void {
    if (!this.contentElement) return;

    this.contentElement.innerHTML = "";

    // Create tabpanel wrapper
    const panel = document.createElement("section");
    panel.className = "settings-content-panel";
    panel.setAttribute("role", "tabpanel");
    panel.id = `panel-${this.activeCategory}`;
    panel.setAttribute("aria-labelledby", `tab-${this.activeCategory}`);
    panel.setAttribute("tabindex", "0");

    this.contentElement.appendChild(panel);

    switch (this.activeCategory) {
      case "appearance":
        this.renderAppearanceSection(panel);
        break;
      default:
        // Future categories
        break;
    }
  }

  /**
   * Renders the Appearance settings section
   */
  private renderAppearanceSection(panel: HTMLElement): void {
    if (!this.currentSettings) return;

    // Section header
    const header = document.createElement("h2");
    header.className = "settings-section-header";
    header.textContent = "Appearance";
    panel.appendChild(header);

    // Font size setting - vertical layout: Label → Input + pt → Hint
    const row = document.createElement("div");
    row.className = "settings-row";

    // Label
    const label = document.createElement("label");
    label.className = "settings-label";
    label.htmlFor = "settings-font-size";
    label.textContent = "Font Size";
    row.appendChild(label);

    // Input group (input + pt unit)
    const inputGroup = document.createElement("div");
    inputGroup.className = "settings-input-group";

    this.fontSizeInput = document.createElement("input");
    this.fontSizeInput.type = "number";
    this.fontSizeInput.id = "settings-font-size";
    this.fontSizeInput.className = "settings-number-input";
    this.fontSizeInput.min = String(MIN_FONT_SIZE);
    this.fontSizeInput.max = String(MAX_FONT_SIZE);
    this.fontSizeInput.value = String(this.currentSettings.font_size);
    inputGroup.appendChild(this.fontSizeInput);

    const unit = document.createElement("span");
    unit.className = "settings-unit";
    unit.textContent = "pt";
    inputGroup.appendChild(unit);

    row.appendChild(inputGroup);

    // Hint text
    const hint = document.createElement("span");
    hint.className = "settings-hint";
    hint.textContent = `Range: ${MIN_FONT_SIZE}-${MAX_FONT_SIZE}pt`;
    row.appendChild(hint);

    panel.appendChild(row);
  }

  /**
   * Attaches event listeners
   */
  private attachEventListeners(): void {
    // Navigation click handler
    if (this.navElement) {
      const navClickHandler = (e: Event) => {
        const target = e.target as HTMLElement;
        if (
          target.classList.contains("settings-nav-item") &&
          !target.classList.contains("disabled")
        ) {
          const categoryId = target.dataset.categoryId;
          if (categoryId && categoryId !== this.activeCategory) {
            this.switchCategory(categoryId);
          }
        }
      };
      this.navElement.addEventListener("click", navClickHandler);
      this.eventListeners.push({
        element: this.navElement,
        type: "click",
        handler: navClickHandler,
      });

      // Keyboard navigation handler for ARIA tab pattern
      const navKeydownHandler = (e: Event) => {
        this.handleNavKeydown(e as KeyboardEvent);
      };
      this.navElement.addEventListener("keydown", navKeydownHandler);
      this.eventListeners.push({
        element: this.navElement,
        type: "keydown",
        handler: navKeydownHandler,
      });
    }

    // Font size input handlers
    if (this.fontSizeInput) {
      // Real-time preview on input
      const inputHandler = () => {
        this.handleFontSizeInput();
      };
      this.fontSizeInput.addEventListener("input", inputHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "input",
        handler: inputHandler,
      });

      // Save on blur
      const blurHandler = () => {
        this.handleFontSizeSave();
      };
      this.fontSizeInput.addEventListener("blur", blurHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "blur",
        handler: blurHandler,
      });

      // Save on Enter key
      const keydownHandler = (e: Event) => {
        if ((e as KeyboardEvent).key === "Enter") {
          this.handleFontSizeSave();
        }
      };
      this.fontSizeInput.addEventListener("keydown", keydownHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "keydown",
        handler: keydownHandler,
      });
    }
  }

  /**
   * Handles keyboard navigation for tabs (Arrow keys, Home, End, Enter, Space)
   */
  private handleNavKeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement;
    if (!target.classList.contains("settings-nav-item")) return;

    const enabledCategories = this.categories.filter((c) => c.enabled);
    const currentIndex = enabledCategories.findIndex(
      (c) => c.id === target.dataset.categoryId,
    );

    if (currentIndex === -1) return;

    let newIndex = currentIndex;

    switch (e.key) {
      case "ArrowDown":
      case "ArrowRight":
        e.preventDefault();
        newIndex = (currentIndex + 1) % enabledCategories.length;
        break;
      case "ArrowUp":
      case "ArrowLeft":
        e.preventDefault();
        newIndex =
          (currentIndex - 1 + enabledCategories.length) %
          enabledCategories.length;
        break;
      case "Home":
        e.preventDefault();
        newIndex = 0;
        break;
      case "End":
        e.preventDefault();
        newIndex = enabledCategories.length - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (
          target.dataset.categoryId &&
          target.dataset.categoryId !== this.activeCategory
        ) {
          this.switchCategory(target.dataset.categoryId);
        }
        return;
      default:
        return;
    }

    // Focus the new tab
    const newCategory = enabledCategories[newIndex];
    if (!newCategory) return;

    const newTab = this.navElement?.querySelector(
      `[data-category-id="${newCategory.id}"]`,
    ) as HTMLElement | null;
    if (newTab) {
      // Update tabindex for roving focus
      target.setAttribute("tabindex", "-1");
      newTab.setAttribute("tabindex", "0");
      newTab.focus();
    }
  }

  /**
   * Handles font size input change (real-time preview)
   */
  private handleFontSizeInput(): void {
    if (!this.fontSizeInput) return;

    const value = Number(this.fontSizeInput.value);

    // Validate range
    if (value >= MIN_FONT_SIZE && value <= MAX_FONT_SIZE) {
      // Apply immediately for preview
      applySettingsToCSS({ font_size: value });
    }
  }

  /**
   * Handles font size save (on blur or Enter)
   */
  private async handleFontSizeSave(): Promise<void> {
    if (!this.fontSizeInput || !this.currentSettings) return;

    let value = Number(this.fontSizeInput.value);

    // Clamp to valid range
    value = Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, value));

    // Update input if clamped
    this.fontSizeInput.value = String(value);

    // Skip if unchanged
    if (value === this.lastSavedValue) {
      return;
    }

    // Update current settings
    this.currentSettings.font_size = value;

    // Apply to CSS
    applySettingsToCSS(this.currentSettings);

    // Save to backend
    try {
      await SettingsService.save(this.currentSettings);
      this.lastSavedValue = value;
    } catch (error) {
      console.error("Failed to save settings:", error);
    }
  }

  /**
   * Switches to a different category
   */
  private switchCategory(categoryId: string): void {
    // Detach old content event listeners before re-rendering
    this.detachContentEventListeners();

    this.activeCategory = categoryId;
    this.renderNavigation();
    this.renderContent();

    // Attach new content event listeners after re-render
    this.attachContentEventListeners();
  }

  /**
   * Attaches event listeners for content area only
   */
  private attachContentEventListeners(): void {
    if (this.fontSizeInput) {
      const inputHandler = () => this.handleFontSizeInput();
      this.fontSizeInput.addEventListener("input", inputHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "input",
        handler: inputHandler,
      });

      const blurHandler = () => this.handleFontSizeSave();
      this.fontSizeInput.addEventListener("blur", blurHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "blur",
        handler: blurHandler,
      });

      const keydownHandler = (e: Event) => {
        if ((e as KeyboardEvent).key === "Enter") {
          this.handleFontSizeSave();
        }
      };
      this.fontSizeInput.addEventListener("keydown", keydownHandler);
      this.eventListeners.push({
        element: this.fontSizeInput,
        type: "keydown",
        handler: keydownHandler,
      });
    }
  }

  /**
   * Detaches content-related event listeners (font size input)
   */
  private detachContentEventListeners(): void {
    // We need to filter out listeners that are attached to the current fontSizeInput
    // and remove them from the DOM before the element is replaced
    const currentFontSizeInput = this.fontSizeInput;
    if (!currentFontSizeInput) return;

    this.eventListeners = this.eventListeners.filter((listener) => {
      if (listener.element === currentFontSizeInput) {
        listener.element.removeEventListener(listener.type, listener.handler);
        return false;
      }
      return true;
    });
  }

  /**
   * Gets the panel element (for testing)
   */
  getPanelElement(): HTMLElement {
    return this.container;
  }

  /**
   * Disposes the settings panel
   */
  dispose(): void {
    // Remove all event listeners
    for (const listener of this.eventListeners) {
      listener.element.removeEventListener(listener.type, listener.handler);
    }
    this.eventListeners = [];

    // Clear DOM
    this.container.innerHTML = "";
    this.navElement = null;
    this.contentElement = null;
    this.fontSizeInput = null;
  }
}
