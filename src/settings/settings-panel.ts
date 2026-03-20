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
  renderNotificationSection,
  renderMarkdownViewerSection,
  renderProfilesSection,
  renderSshSection,
  renderLogSection,
  renderMuxSection,
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

/** SVG icons for each settings category (24px viewBox, currentColor fill) */
const CATEGORY_ICONS: Record<string, string> = {
  ui: '<svg viewBox="0 0 24 24"><path d="M12 22C6.49 22 2 17.51 2 12S6.49 2 12 2s10 4.04 10 9c0 3.31-2.69 6-6 6h-1.77c-.28 0-.5.22-.5.5 0 .12.05.23.13.33.41.47.64 1.06.64 1.67A2.5 2.5 0 0 1 12 22zm0-18c-4.41 0-8 3.59-8 8s3.59 8 8 8c.28 0 .5-.22.5-.5a.54.54 0 0 0-.14-.35c-.41-.46-.63-1.05-.63-1.65a2.5 2.5 0 0 1 2.5-2.5H16c2.21 0 4-1.79 4-4 0-3.86-3.59-7-8-7z"/><circle cx="6.5" cy="11.5" r="1.5"/><circle cx="9.5" cy="7.5" r="1.5"/><circle cx="14.5" cy="7.5" r="1.5"/><circle cx="17.5" cy="11.5" r="1.5"/></svg>',
  keybinds: '<svg viewBox="0 0 24 24"><path d="M20 5H4c-1.1 0-1.99.9-1.99 2L2 17c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm-9 3h2v2h-2V8zm0 3h2v2h-2v-2zM8 8h2v2H8V8zm0 3h2v2H8v-2zM5 11h2v2H5v-2zm0-3h2v2H5V8zm3 7H5v-2h3v2zm8 0H9v-2h7v2zm2 0h-2v-2h2v2zm0-3h-2v-2h2v2zm0-3h-2V8h2v2zm2 3h-2v-2h2v2z"/></svg>',
  mux: '<svg viewBox="0 0 24 24"><path d="M3 3h8v8H3V3zm0 10h8v8H3v-8zm10-10h8v8h-8V3zm0 10h8v8h-8v-8z"/></svg>',
  "terminal-appearance": '<svg viewBox="0 0 24 24"><path d="M2 17h2v.5H3v1h1v.5H2v1h3v-4H2v1zm1-9h1V4H2v1h1v3zm-1 3h1.8L2 13.1v.9h3v-1H3.2L5 10.9V10H2v1zm5-6v2h14V5H7zm0 14h14v-2H7v2zm0-6h14v-2H7v2z"/></svg>',
  "terminal-behavior": '<svg viewBox="0 0 24 24"><path d="M20 4H4c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 14H4V8h16v12zM6 10l4 4-4 4 1.4 1.4L12.8 14l-5.4-5.4L6 10z"/></svg>',
  notification: '<svg viewBox="0 0 24 24"><path d="M12 22c1.1 0 2-.9 2-2h-4a2 2 0 0 0 2 2zm6-6v-5c0-3.07-1.64-5.64-4.5-6.32V4c0-.83-.67-1.5-1.5-1.5s-1.5.67-1.5 1.5v.68C7.63 5.36 6 7.92 6 11v5l-2 2v1h16v-1l-2-2z"/></svg>',
  "markdown-viewer": '<svg viewBox="0 0 24 24"><path d="M19 3H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm-5 14H7v-2h7v2zm3-4H7v-2h10v2zm0-4H7V7h10v2z"/></svg>',
  profiles: '<svg viewBox="0 0 24 24"><path d="M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z"/></svg>',
  ssh: '<svg viewBox="0 0 24 24"><path d="M21 2H3c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h7l-2 3v1h8v-1l-2-3h7c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm0 14H3V4h18v12z"/><path d="M7 8l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  log: '<svg viewBox="0 0 24 24"><path d="M14 2H6c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8l-6-6zm2 16H8v-2h8v2zm0-4H8v-2h8v2zm-3-5V3.5L18.5 9H13z"/></svg>',
};

/**
 * SettingsPanel - Displays and manages application settings
 */
export class SettingsPanel {
  private container: HTMLElement;
  private navElement: HTMLElement | null = null;
  private contentElement: HTMLElement | null = null;
  private activeCategory = "ui";
  private navCollapsed = false;
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
      { id: "mux", label: t("settings.categories.mux"), enabled: true },
      { id: "terminal-appearance", label: t("settings.categories.terminalAppearance"), enabled: true },
      { id: "terminal-behavior", label: t("settings.categories.terminalBehavior"), enabled: true },
      { id: "notification", label: t("settings.categories.notification"), enabled: true },
      { id: "markdown-viewer", label: t("settings.categories.markdownViewer"), enabled: true },
      { id: "profiles", label: t("settings.categories.profiles"), enabled: true },
      { id: "ssh", label: t("settings.categories.ssh"), enabled: true },
      { id: "log", label: t("settings.categories.log"), enabled: true },
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

    // Toggle row (right-aligned when expanded, centered when collapsed)
    const toggleRow = document.createElement("div");
    toggleRow.className = "settings-nav-toggle-row";
    const collapseBtn = document.createElement("button");
    collapseBtn.className = "settings-nav-collapse-btn";
    collapseBtn.setAttribute(
      "aria-label",
      this.navCollapsed ? t("settings.nav.expand") : t("settings.nav.collapse"),
    );
    collapseBtn.textContent = "\u2261"; // ≡ hamburger
    collapseBtn.addEventListener("click", () => this.toggleNavCollapsed());
    toggleRow.appendChild(collapseBtn);
    this.navElement.appendChild(toggleRow);

    for (const category of this.categories) {
      const button = document.createElement("button");
      button.className = "settings-nav-item";
      button.dataset.categoryId = category.id;

      // Icon
      const iconSpan = document.createElement("span");
      iconSpan.className = "settings-nav-icon";
      iconSpan.innerHTML = CATEGORY_ICONS[category.id] || "";
      button.appendChild(iconSpan);

      // Label
      const labelSpan = document.createElement("span");
      labelSpan.className = "settings-nav-label";
      labelSpan.textContent = category.label;
      button.appendChild(labelSpan);

      // Tooltip in collapsed state
      if (this.navCollapsed) {
        button.title = category.label;
      }

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
      case "mux":
        renderMuxSection(panel, ctx);
        break;
      case "terminal-appearance":
        renderTerminalAppearanceSection(panel, ctx);
        break;
      case "terminal-behavior":
        renderTerminalBehaviorSection(panel, ctx);
        break;
      case "notification":
        renderNotificationSection(panel, ctx);
        break;
      case "markdown-viewer":
        renderMarkdownViewerSection(panel, ctx);
        break;
      case "profiles":
        renderProfilesSection(panel, ctx);
        break;
      case "ssh":
        renderSshSection(panel, ctx);
        break;
      case "log":
        renderLogSection(panel, ctx);
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
        const target = (e.target as HTMLElement).closest(
          ".settings-nav-item",
        ) as HTMLElement | null;
        if (target && !target.classList.contains("disabled")) {
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
  // Nav Collapse Toggle
  // ============================================================

  private toggleNavCollapsed(): void {
    this.navCollapsed = !this.navCollapsed;
    this.container.classList.toggle("nav-collapsed", this.navCollapsed);

    // Update aria-label on toggle button
    const collapseBtn = this.navElement?.querySelector(".settings-nav-collapse-btn");
    if (collapseBtn) {
      collapseBtn.setAttribute(
        "aria-label",
        this.navCollapsed ? t("settings.nav.expand") : t("settings.nav.collapse"),
      );
    }

    // Update title tooltips on nav items
    const navItems = this.navElement?.querySelectorAll(".settings-nav-item");
    if (navItems) {
      for (const item of navItems) {
        const el = item as HTMLElement;
        if (this.navCollapsed) {
          const label = el.querySelector(".settings-nav-label");
          el.title = label?.textContent || "";
        } else {
          el.removeAttribute("title");
        }
      }
    }
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
