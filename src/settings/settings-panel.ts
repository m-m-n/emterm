/**
 * Settings Panel
 *
 * Orchestrator for application settings UI with category navigation.
 * Delegates section rendering, UI components, font picker, and keybind
 * editing to dedicated modules.
 */

import { SettingsService } from "./settings-service";
import type { AppSettings } from "./types";
import { t } from "../i18n/index.ts";
import { showFontPicker } from "./font-picker";
import type { FontCategory } from "./types";
import {
  createKeybindCaptureState,
  exitKeybindCapture,
} from "./keybind-editor";
import type { KeybindCaptureState } from "./keybind-editor";
import {
  renderUiSection,
  renderKeybindsSection,
  renderTerminalAppearanceSection,
  renderTerminalBehaviorSection,
  renderMarkdownViewerSection,
} from "./settings-sections";
import { filterFontList } from "./font-picker";

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
  private activeCategory = "ui";
  private currentSettings: AppSettings | null = null;
  private eventListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
  }> = [];

  /** Keybind capture state (shared with keybind-editor module) */
  private keybindState: KeybindCaptureState = createKeybindCaptureState();

  private get categories(): Category[] {
    return [
      { id: "ui", label: t("settings.categories.ui"), enabled: true },
      { id: "keybinds", label: t("settings.categories.keybinds"), enabled: true },
      { id: "terminal-appearance", label: t("settings.categories.terminalAppearance"), enabled: true },
      { id: "terminal-behavior", label: t("settings.categories.terminalBehavior"), enabled: true },
      { id: "markdown-viewer", label: t("settings.categories.markdownViewer"), enabled: true },
    ];
  }

  constructor(options: SettingsPanelOptions) {
    this.container = options.container;
  }

  /**
   * Initializes the settings panel
   */
  async init(): Promise<void> {
    this.currentSettings = await SettingsService.load();
    this.render();
    this.attachEventListeners();
  }

  /**
   * Renders the settings panel UI
   */
  private render(): void {
    this.container.innerHTML = "";
    this.container.className = "settings-panel";

    // Navigation
    this.navElement = document.createElement("nav");
    this.navElement.className = "settings-nav";
    this.navElement.setAttribute("role", "tablist");
    this.navElement.setAttribute("aria-orientation", "vertical");
    this.navElement.setAttribute("aria-label", "Settings categories");
    this.renderNavigation();
    this.container.appendChild(this.navElement);

    // Content
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
   * Renders the content area for the active category
   */
  private renderContent(): void {
    if (!this.contentElement) return;
    this.contentElement.innerHTML = "";

    const panel = document.createElement("section");
    panel.className = "settings-content-panel";
    panel.setAttribute("role", "tabpanel");
    panel.id = `panel-${this.activeCategory}`;
    panel.setAttribute("aria-labelledby", `tab-${this.activeCategory}`);
    panel.setAttribute("tabindex", "0");

    this.contentElement.appendChild(panel);

    if (!this.currentSettings) return;

    const ctx = this.buildSectionContext();

    switch (this.activeCategory) {
      case "ui":
        renderUiSection(panel, ctx);
        break;
      case "keybinds":
        renderKeybindsSection(panel, ctx);
        break;
      case "terminal-appearance":
        renderTerminalAppearanceSection(panel, ctx);
        break;
      case "terminal-behavior":
        renderTerminalBehaviorSection(panel, ctx);
        break;
      case "markdown-viewer":
        renderMarkdownViewerSection(panel, ctx);
        break;
    }
  }

  // ============================================================
  // Section Context Builder
  // ============================================================

  private buildSectionContext() {
    return {
      currentSettings: this.currentSettings!,
      addContentListener: this.addContentListener.bind(this),
      saveSetting: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
        this.saveSetting(key, value);
      },
      showFontPicker: (category: FontCategory, currentValue: string, onSelect: (value: string) => void) => {
        showFontPicker(category, currentValue, onSelect, {
          contentElement: this.contentElement!,
          navElement: this.navElement,
          addContentListener: this.addContentListener.bind(this),
          detachContentListeners: this.detachContentListeners.bind(this),
          renderContent: this.renderContent.bind(this),
        });
      },
      keybindCtx: {
        state: this.keybindState,
        eventListeners: this.eventListeners,
        currentSettings: this.currentSettings,
      },
      reRender: () => {
        this.detachContentListeners();
        this.render();
        this.attachEventListeners();
      },
    };
  }

  // ============================================================
  // Settings Save Helper
  // ============================================================

  private async saveSetting<K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K],
  ): Promise<void> {
    if (!this.currentSettings) return;
    this.currentSettings[key] = value;
    try {
      await SettingsService.save(this.currentSettings);
    } catch (error) {
      console.error("Failed to save setting:", error);
    }
  }

  // ============================================================
  // Event Listener Management
  // ============================================================

  private contentListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
    capture?: boolean;
  }> = [];

  private addContentListener(
    element: EventTarget,
    type: string,
    handler: EventListener,
    capture?: boolean,
  ): void {
    element.addEventListener(type, handler, capture);
    this.contentListeners.push({ element, type, handler, capture });
  }

  private detachContentListeners(): void {
    for (const l of this.contentListeners) {
      l.element.removeEventListener(l.type, l.handler, l.capture);
    }
    this.contentListeners = [];
  }

  private attachEventListeners(): void {
    // Navigation click
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

      // Keyboard navigation
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
  }

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

    const newCategory = enabledCategories[newIndex];
    if (!newCategory) return;

    const newTab = this.navElement?.querySelector(
      `[data-category-id="${newCategory.id}"]`,
    ) as HTMLElement | null;
    if (newTab) {
      target.setAttribute("tabindex", "-1");
      newTab.setAttribute("tabindex", "0");
      newTab.focus();
    }
  }

  private switchCategory(categoryId: string): void {
    // Cancel any keybind capture
    if (this.keybindState.capturingKeybindButton) {
      exitKeybindCapture(true, {
        state: this.keybindState,
        eventListeners: this.eventListeners,
        currentSettings: this.currentSettings,
      });
    }

    this.detachContentListeners();
    this.activeCategory = categoryId;
    this.renderNavigation();
    this.renderContent();
  }

  // ============================================================
  // Public API
  // ============================================================

  /** Delegate to font-picker filterFontList for backward compatibility */
  filterFontList(searchText: string, fonts: string[]): string[] {
    return filterFontList(searchText, fonts);
  }

  getPanelElement(): HTMLElement {
    return this.container;
  }

  dispose(): void {
    if (this.keybindState.capturingKeybindButton) {
      exitKeybindCapture(true, {
        state: this.keybindState,
        eventListeners: this.eventListeners,
        currentSettings: this.currentSettings,
      });
    }

    this.detachContentListeners();

    for (const listener of this.eventListeners) {
      listener.element.removeEventListener(listener.type, listener.handler);
    }
    this.eventListeners = [];

    this.container.innerHTML = "";
    this.navElement = null;
    this.contentElement = null;
  }
}
