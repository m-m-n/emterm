/**
 * Tab Bar UI
 *
 * DOM rendering and user interactions for the tab bar.
 */

import type { TabManager } from "./tab-manager";
import type { Tab } from "./types";
import { t } from "../i18n/index.ts";

/**
 * Options for creating TabBarUI
 */
export interface TabBarUIOptions {
  /** Container element for the tab bar */
  container: HTMLElement;
  /** TabManager instance */
  tabManager: TabManager;
  /** Callback when settings button is clicked */
  onSettingsClick?: () => void;
}

/**
 * SVG icons
 */
const ICONS = {
  plus: `<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
    <path d="M8 2a.5.5 0 0 1 .5.5v5h5a.5.5 0 0 1 0 1h-5v5a.5.5 0 0 1-1 0v-5h-5a.5.5 0 0 1 0-1h5v-5A.5.5 0 0 1 8 2z"/>
  </svg>`,
  settings: `<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
    <path d="M8 4.754a3.246 3.246 0 1 0 0 6.492 3.246 3.246 0 0 0 0-6.492zM5.754 8a2.246 2.246 0 1 1 4.492 0 2.246 2.246 0 0 1-4.492 0z"/>
    <path d="M9.796 1.343c-.527-1.79-3.065-1.79-3.592 0l-.094.319a.873.873 0 0 1-1.255.52l-.292-.16c-1.64-.892-3.433.902-2.54 2.541l.159.292a.873.873 0 0 1-.52 1.255l-.319.094c-1.79.527-1.79 3.065 0 3.592l.319.094a.873.873 0 0 1 .52 1.255l-.16.292c-.892 1.64.901 3.434 2.541 2.54l.292-.159a.873.873 0 0 1 1.255.52l.094.319c.527 1.79 3.065 1.79 3.592 0l.094-.319a.873.873 0 0 1 1.255-.52l.292.16c1.64.893 3.434-.902 2.54-2.541l-.159-.292a.873.873 0 0 1 .52-1.255l.319-.094c1.79-.527 1.79-3.065 0-3.592l-.319-.094a.873.873 0 0 1-.52-1.255l.16-.292c.893-1.64-.902-3.433-2.541-2.54l-.292.159a.873.873 0 0 1-1.255-.52l-.094-.319zm-2.633.283c.246-.835 1.428-.835 1.674 0l.094.319a1.873 1.873 0 0 0 2.693 1.115l.291-.16c.764-.415 1.6.42 1.184 1.185l-.159.292a1.873 1.873 0 0 0 1.116 2.692l.318.094c.835.246.835 1.428 0 1.674l-.319.094a1.873 1.873 0 0 0-1.115 2.693l.16.291c.415.764-.42 1.6-1.185 1.184l-.291-.159a1.873 1.873 0 0 0-2.693 1.116l-.094.318c-.246.835-1.428.835-1.674 0l-.094-.319a1.873 1.873 0 0 0-2.692-1.115l-.292.16c-.764.415-1.6-.42-1.184-1.185l.159-.291A1.873 1.873 0 0 0 1.945 8.93l-.319-.094c-.835-.246-.835-1.428 0-1.674l.319-.094A1.873 1.873 0 0 0 3.06 4.377l-.16-.292c-.415-.764.42-1.6 1.185-1.184l.292.159a1.873 1.873 0 0 0 2.692-1.115l.094-.319z"/>
  </svg>`,
};

/**
 * TabBarUI - Renders and manages the tab bar DOM
 */
export class TabBarUI {
  private container: HTMLElement;
  private tabManager: TabManager;
  private onSettingsClick?: () => void;

  private scrollArea: HTMLElement | null = null;
  private fixedArea: HTMLElement | null = null;
  private tabElements: Map<string, HTMLElement> = new Map();
  private unsubscribers: (() => void)[] = [];

  constructor(options: TabBarUIOptions) {
    this.container = options.container;
    this.tabManager = options.tabManager;
    this.onSettingsClick = options.onSettingsClick;
  }

  /**
   * Initializes the tab bar UI
   */
  init(): void {
    this.createStructure();
    this.subscribeToEvents();
    this.renderExistingTabs();
  }

  /**
   * Creates the tab bar DOM structure
   */
  private createStructure(): void {
    // Clear container
    this.container.innerHTML = "";
    this.container.className = "tab-bar";
    this.container.setAttribute("role", "tablist");
    this.container.setAttribute("aria-label", t("tabBar.terminalTabs"));

    // Create scroll area for tabs
    this.scrollArea = document.createElement("div");
    this.scrollArea.className = "tab-scroll-area";
    this.container.appendChild(this.scrollArea);

    // Create fixed area for buttons
    this.fixedArea = document.createElement("div");
    this.fixedArea.className = "tab-fixed-area";
    this.container.appendChild(this.fixedArea);

    // Create new tab button
    const newTabButton = document.createElement("button");
    newTabButton.className = "tab-button tab-button-new";
    newTabButton.innerHTML = ICONS.plus;
    newTabButton.title = t("tabBar.newTabShortcut");
    newTabButton.setAttribute("aria-label", t("tabBar.createNewTab"));
    newTabButton.addEventListener("click", () => this.handleNewTabClick());
    this.fixedArea.appendChild(newTabButton);

    // Create settings button
    const settingsButton = document.createElement("button");
    settingsButton.className = "tab-button tab-button-settings";
    settingsButton.innerHTML = ICONS.settings;
    settingsButton.title = t("tabBar.settings");
    settingsButton.setAttribute("aria-label", t("tabBar.openSettings"));
    settingsButton.addEventListener("click", () => this.handleSettingsClick());
    this.fixedArea.appendChild(settingsButton);
  }

  /**
   * Subscribes to TabManager events
   */
  private subscribeToEvents(): void {
    // Tab created
    const unsubCreated = this.tabManager.on("tab:created", ({ tab }) => {
      this.addTabElement(tab);
    });
    this.unsubscribers.push(unsubCreated);

    // Tab closed
    const unsubClosed = this.tabManager.on("tab:closed", ({ tabId }) => {
      this.removeTabElement(tabId);
    });
    this.unsubscribers.push(unsubClosed);

    // Tab activated
    const unsubActivated = this.tabManager.on(
      "tab:activated",
      ({ tab, previousTabId }) => {
        this.updateActiveState(tab.id, previousTabId);
        this.scrollToTab(tab.id);
      },
    );
    this.unsubscribers.push(unsubActivated);

    // Tab title changed
    const unsubTitleChanged = this.tabManager.on(
      "tab:titleChanged",
      ({ tabId, title }) => {
        this.updateTabTitle(tabId, title);
      },
    );
    this.unsubscribers.push(unsubTitleChanged);

    // Tab reordered
    const unsubReordered = this.tabManager.on("tab:reordered", ({ tabs }) => {
      this.reorderTabElements(tabs);
    });
    this.unsubscribers.push(unsubReordered);
  }

  /**
   * Reorders tab elements in the DOM to match the tab array order
   */
  private reorderTabElements(tabs: Tab[]): void {
    if (!this.scrollArea) return;

    // Reorder elements in DOM according to new tab order
    for (const tab of tabs) {
      const element = this.tabElements.get(tab.id);
      if (element) {
        // Moving to end of scrollArea puts it in correct order
        this.scrollArea.appendChild(element);
      }
    }
  }

  /**
   * Renders existing tabs (for late initialization)
   */
  private renderExistingTabs(): void {
    const tabs = this.tabManager.getTabs();
    const activeTab = this.tabManager.getActiveTab();

    for (const tab of tabs) {
      this.addTabElement(tab);
    }

    if (activeTab) {
      this.updateActiveState(activeTab.id, null);
    }
  }

  /**
   * Adds a tab element to the DOM
   */
  private addTabElement(tab: Tab): void {
    if (!this.scrollArea) return;

    const tabElement = document.createElement("div");
    tabElement.className = "tab";
    tabElement.dataset.tabId = tab.id;
    tabElement.draggable = tab.type === "terminal"; // Settings tab not draggable

    // Accessibility attributes
    tabElement.setAttribute("role", "tab");
    tabElement.setAttribute("tabindex", "0");
    tabElement.setAttribute("aria-label", tab.title);
    tabElement.setAttribute("aria-selected", "false");

    // Tab icon (different for terminal vs settings)
    const icon = document.createElement("span");
    icon.className = "tab-icon";
    icon.setAttribute("aria-hidden", "true");
    icon.innerHTML = tab.type === "settings" ? ICONS.settings : "";
    if (tab.type !== "settings") {
      icon.style.display = "none";
    }
    tabElement.appendChild(icon);

    // Tab title
    const title = document.createElement("span");
    title.className = "tab-title";
    title.textContent = tab.title;
    tabElement.appendChild(title);

    // Click handler
    tabElement.addEventListener("click", () => {
      this.tabManager.switchTab(tab.id);
    });

    // Keyboard handler for accessibility
    tabElement.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        this.tabManager.switchTab(tab.id);
      }
    });

    // Store reference and add to DOM
    this.tabElements.set(tab.id, tabElement);
    this.scrollArea.appendChild(tabElement);
  }

  /**
   * Removes a tab element from the DOM
   */
  private removeTabElement(tabId: string): void {
    const element = this.tabElements.get(tabId);
    if (element) {
      element.remove();
      this.tabElements.delete(tabId);
    }
  }

  /**
   * Updates active state classes and ARIA attributes
   */
  private updateActiveState(
    activeTabId: string,
    previousTabId: string | null,
  ): void {
    // Remove active class and ARIA from previous
    if (previousTabId) {
      const prevElement = this.tabElements.get(previousTabId);
      if (prevElement) {
        prevElement.classList.remove("active");
        prevElement.setAttribute("aria-selected", "false");
      }
    }

    // Add active class and ARIA to new
    const activeElement = this.tabElements.get(activeTabId);
    if (activeElement) {
      activeElement.classList.add("active");
      activeElement.setAttribute("aria-selected", "true");
    }
  }

  /**
   * Scrolls to make a tab visible
   */
  private scrollToTab(tabId: string): void {
    const element = this.tabElements.get(tabId);
    if (element && this.scrollArea) {
      element.scrollIntoView({
        behavior: "smooth",
        block: "nearest",
        inline: "nearest",
      });
    }
  }

  /**
   * Updates a tab's title
   */
  updateTabTitle(tabId: string, title: string): void {
    const element = this.tabElements.get(tabId);
    if (element) {
      const titleElement = element.querySelector(".tab-title");
      if (titleElement) {
        titleElement.textContent = title;
      }
    }
  }

  /**
   * Handles new tab button click
   */
  private handleNewTabClick(): void {
    this.tabManager.createTab();
  }

  /**
   * Handles settings button click
   */
  private handleSettingsClick(): void {
    if (this.onSettingsClick) {
      this.onSettingsClick();
    } else {
      // Default: singleton pattern for settings tab
      this.openOrFocusSettingsTab();
    }
  }

  /**
   * Opens settings tab or focuses existing one (singleton pattern)
   */
  openOrFocusSettingsTab(): void {
    // Check if settings tab already exists
    const tabs = this.tabManager.getTabs();
    const existingSettingsTab = tabs.find((t) => t.type === "settings");

    if (existingSettingsTab) {
      // Switch to existing settings tab
      this.tabManager.switchTab(existingSettingsTab.id);
    } else {
      // Create new settings tab
      this.tabManager.createTab({ type: "settings" });
    }
  }

  /**
   * Gets the tab element for a tab ID
   */
  getTabElement(tabId: string): HTMLElement | null {
    return this.tabElements.get(tabId) ?? null;
  }

  /**
   * Sets the visibility of the tab bar
   * @param visible - true to show, false to hide
   */
  setVisible(visible: boolean): void {
    if (visible) {
      this.container.classList.remove("hidden");
    } else {
      this.container.classList.add("hidden");
    }
  }

  /**
   * Returns whether the tab bar is currently visible
   */
  isVisible(): boolean {
    return !this.container.classList.contains("hidden");
  }

  /**
   * Disposes the UI
   */
  dispose(): void {
    // Unsubscribe from events
    for (const unsubscribe of this.unsubscribers) {
      unsubscribe();
    }
    this.unsubscribers = [];

    // Clear DOM
    this.container.innerHTML = "";
    this.tabElements.clear();
    this.scrollArea = null;
    this.fixedArea = null;
  }
}
