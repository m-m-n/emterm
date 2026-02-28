/**
 * Tab Manager
 *
 * Manages tab lifecycle, state, and all associated resources.
 * Uses centralized ownership pattern for TerminalApp instances and event cleanup.
 */

import type { UnlistenFn } from "@tauri-apps/api/event";
import type { TerminalApp } from "../terminal-app";
import { SettingsPanel } from "../settings";
import type { RendererSettings } from "../settings/settings-applier";
import type {
  Tab,
  TerminalTab,
  SettingsTab,
  TabOperationState,
  CreateTabOptions,
  ProfileSpawnOptions,
  TabEventType,
  TabEventPayloads,
  TabEventHandler,
  UnsubscribeFn,
  TabEventEmitter,
} from "./types";
import { isTerminalTab } from "./types";
import { t } from "../i18n/index.ts";

/**
 * Options for creating TabManager
 */
export interface TabManagerOptions {
  /** Container element for tab content */
  container: HTMLElement;
  /** Factory function to create TerminalApp instances */
  createTerminalApp: (container: HTMLElement, spawnOptions?: ProfileSpawnOptions) => Promise<TerminalApp>;
}

/**
 * Generates a unique tab ID
 */
function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

/**
 * Simple typed event emitter implementation
 */
class TypedEventEmitter implements TabEventEmitter {
  private handlers: Map<TabEventType, Set<TabEventHandler<any>>> = new Map();

  on<T extends TabEventType>(
    event: T,
    handler: TabEventHandler<T>,
  ): UnsubscribeFn {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set());
    }
    this.handlers.get(event)!.add(handler);

    return () => this.off(event, handler);
  }

  off<T extends TabEventType>(event: T, handler: TabEventHandler<T>): void {
    const eventHandlers = this.handlers.get(event);
    if (eventHandlers) {
      eventHandlers.delete(handler);
    }
  }

  emit<T extends TabEventType>(event: T, payload: TabEventPayloads[T]): void {
    const eventHandlers = this.handlers.get(event);
    if (eventHandlers) {
      for (const handler of eventHandlers) {
        try {
          handler(payload);
        } catch (error) {
          console.error(
            `[ERROR][FRONTEND] Error in event handler for ${event}:`,
            error,
          );
        }
      }
    }
  }
}

/**
 * TabManager - Manages multiple terminal tabs
 */
export class TabManager {
  private tabs: Tab[] = [];
  private activeTabId: string | null = null;
  private operationState: TabOperationState = { status: "idle" };
  private terminalApps: Map<string, TerminalApp> = new Map();
  private settingsPanels: Map<string, SettingsPanel> = new Map();
  private tabContainers: Map<string, HTMLElement> = new Map();
  private eventUnlistens: Map<string, UnlistenFn> = new Map();
  private eventEmitter: TypedEventEmitter = new TypedEventEmitter();
  private lastTabClosedCallback: (() => void) | null = null;

  private container: HTMLElement;
  private createTerminalApp: (container: HTMLElement, spawnOptions?: ProfileSpawnOptions) => Promise<TerminalApp>;

  constructor(options: TabManagerOptions) {
    this.container = options.container;
    this.createTerminalApp = options.createTerminalApp;
  }

  /**
   * Creates a new tab
   * @returns The created tab or null if blocked by state machine
   */
  async createTab(options: CreateTabOptions = {}): Promise<Tab | null> {
    // Check state machine - block if not idle
    if (this.operationState.status !== "idle") {
      console.warn(
        `Tab creation blocked: operation in progress (${this.operationState.status})`,
      );
      return null;
    }

    // Set state to creating
    this.operationState = { status: "creating" };

    try {
      const tabType = options.type ?? "terminal";
      const tabId = generateTabId();

      if (tabType === "settings") {
        return await this.createSettingsTab(tabId, options.title);
      }

      return await this.createTerminalTabInternal(tabId, options.title, options.profileSpawn);
    } finally {
      // Reset state to idle
      this.operationState = { status: "idle" };
    }
  }

  /**
   * Creates a terminal tab
   */
  private async createTerminalTabInternal(
    tabId: string,
    title?: string,
    profileSpawn?: ProfileSpawnOptions,
  ): Promise<TerminalTab | null> {
    try {
      // Create container for this tab
      const tabContainer = document.createElement("div");
      tabContainer.id = `tab-content-${tabId}`;
      tabContainer.className = "tab-content";
      tabContainer.style.display = "none";
      this.container.appendChild(tabContainer);
      this.tabContainers.set(tabId, tabContainer);

      // Create TerminalApp (pass profile spawn options if provided)
      const terminalApp = await this.createTerminalApp(tabContainer, profileSpawn);
      this.terminalApps.set(tabId, terminalApp);

      // Connect title change callback
      const currentTabId = tabId;
      terminalApp.onTitleChange((newTitle: string) => {
        this.updateTabTitle(currentTabId, newTitle);
      });

      // Get session ID from PTY
      const sessionId = terminalApp.pty?.getSessionId() ?? `session-${tabId}`;

      // Create tab data
      const tab: TerminalTab = {
        id: tabId,
        type: "terminal",
        sessionId,
        title: title ?? "Terminal",
      };

      // Update previous active tab
      const previousTabId = this.activeTabId;
      const previousTab = this.getActiveTab();

      // Add to tabs array and set as active
      this.tabs.push(tab);
      this.activeTabId = tabId;

      // Show this tab's container, hide previous
      if (previousTabId) {
        const prevContainer = this.tabContainers.get(previousTabId);
        if (prevContainer) {
          prevContainer.style.display = "none";
        }
      }
      tabContainer.style.display = "";

      // Emit events
      this.eventEmitter.emit("tab:created", { tab });

      if (previousTab) {
        this.eventEmitter.emit("tab:deactivated", { tab: previousTab });
      }

      this.eventEmitter.emit("tab:activated", { tab, previousTabId });

      return tab;
    } catch (error) {
      console.error("[ERROR][FRONTEND] Failed to create terminal tab:", error);

      // Cleanup on failure
      const tabContainer = this.tabContainers.get(tabId);
      if (tabContainer) {
        tabContainer.remove();
        this.tabContainers.delete(tabId);
      }

      return null;
    }
  }

  /**
   * Creates a settings tab
   */
  private async createSettingsTab(
    tabId: string,
    title?: string,
  ): Promise<SettingsTab> {
    // Create container for settings
    const tabContainer = document.createElement("div");
    tabContainer.id = `tab-content-${tabId}`;
    tabContainer.className = "tab-content settings-tab-content";
    tabContainer.style.display = "none";
    this.container.appendChild(tabContainer);
    this.tabContainers.set(tabId, tabContainer);

    // Create and initialize SettingsPanel
    const settingsPanel = new SettingsPanel({ container: tabContainer });
    await settingsPanel.init();
    this.settingsPanels.set(tabId, settingsPanel);

    // Create tab data
    const tab: SettingsTab = {
      id: tabId,
      type: "settings",
      title: title ?? t("tabBar.settings"),
    };

    // Update previous active tab
    const previousTabId = this.activeTabId;
    const previousTab = this.getActiveTab();

    // Add to tabs array and set as active
    this.tabs.push(tab);
    this.activeTabId = tabId;

    // Show this tab's container, hide previous
    if (previousTabId) {
      const prevContainer = this.tabContainers.get(previousTabId);
      if (prevContainer) {
        prevContainer.style.display = "none";
      }
    }
    tabContainer.style.display = "";

    // Emit events
    this.eventEmitter.emit("tab:created", { tab });

    if (previousTab) {
      this.eventEmitter.emit("tab:deactivated", { tab: previousTab });
    }

    this.eventEmitter.emit("tab:activated", { tab, previousTabId });

    return tab;
  }

  /**
   * Closes a tab by ID
   * @returns true if tab was closed, false otherwise
   */
  async closeTab(tabId: string): Promise<boolean> {
    const tabIndex = this.tabs.findIndex((t) => t.id === tabId);
    if (tabIndex === -1) {
      return false;
    }

    // Check state machine
    if (this.operationState.status !== "idle") {
      console.warn(
        `[WARN][FRONTEND] Tab close blocked: operation in progress (${this.operationState.status})`,
      );
      return false;
    }

    // Set state to closing
    this.operationState = { status: "closing", tabId };

    try {
      const tab = this.tabs[tabIndex];
      const wasActive = this.activeTabId === tabId;

      // Cleanup resources
      await this.cleanupTabResources(tabId);

      // Remove from tabs array
      this.tabs.splice(tabIndex, 1);

      // Handle active tab change
      if (wasActive) {
        if (this.tabs.length > 0) {
          // Activate adjacent tab (prefer next, then previous)
          const newIndex = Math.min(tabIndex, this.tabs.length - 1);
          const newActiveTab = this.tabs[newIndex]!;
          this.activeTabId = newActiveTab.id;

          // Show new active tab container
          const newContainer = this.tabContainers.get(newActiveTab.id);
          if (newContainer) {
            newContainer.style.display = "";
          }

          this.eventEmitter.emit("tab:activated", {
            tab: newActiveTab,
            previousTabId: tabId,
          });
        } else {
          this.activeTabId = null;
        }
      }

      // Emit closed event
      this.eventEmitter.emit("tab:closed", { tabId, wasActive });

      // Signal last tab closed
      if (this.tabs.length === 0) {
        this.lastTabClosedCallback?.();
      }

      return true;
    } finally {
      // Reset state to idle
      this.operationState = { status: "idle" };
    }
  }

  /**
   * Cleans up resources for a tab
   */
  private async cleanupTabResources(tabId: string): Promise<void> {
    // Dispose TerminalApp
    const terminalApp = this.terminalApps.get(tabId);
    if (terminalApp) {
      terminalApp.dispose();
      try {
        await terminalApp.pty?.kill();
      } catch (error) {
        console.error("[ERROR][FRONTEND] Failed to kill PTY:", error);
      }
      this.terminalApps.delete(tabId);
    }

    // Dispose SettingsPanel
    const settingsPanel = this.settingsPanels.get(tabId);
    if (settingsPanel) {
      settingsPanel.dispose();
      this.settingsPanels.delete(tabId);
    }

    // Call event unlisten
    const unlisten = this.eventUnlistens.get(tabId);
    if (unlisten) {
      unlisten();
      this.eventUnlistens.delete(tabId);
    }

    // Remove container
    const container = this.tabContainers.get(tabId);
    if (container) {
      container.remove();
      this.tabContainers.delete(tabId);
    }
  }

  /**
   * Closes the currently active tab
   * @returns true if tab was closed, false otherwise
   */
  async closeActiveTab(): Promise<boolean> {
    if (!this.activeTabId) {
      return false;
    }
    return this.closeTab(this.activeTabId);
  }

  /**
   * Switches to a tab by ID
   */
  switchTab(tabId: string): void {
    // Don't switch if same tab or tab doesn't exist
    if (tabId === this.activeTabId) {
      return;
    }

    const tab = this.tabs.find((t) => t.id === tabId);
    if (!tab) {
      return;
    }

    const previousTabId = this.activeTabId;
    const previousTab = this.getActiveTab();

    // Hide previous container
    if (previousTabId) {
      const prevContainer = this.tabContainers.get(previousTabId);
      if (prevContainer) {
        prevContainer.style.display = "none";
      }
    }

    // Show new container
    const newContainer = this.tabContainers.get(tabId);
    if (newContainer) {
      newContainer.style.display = "";
    }

    // Update active tab
    this.activeTabId = tabId;

    // Emit events
    if (previousTab) {
      this.eventEmitter.emit("tab:deactivated", { tab: previousTab });
    }

    this.eventEmitter.emit("tab:activated", { tab, previousTabId });
  }

  /**
   * Activates the next tab (wraps to first)
   */
  activateNextTab(): void {
    if (this.tabs.length <= 1 || !this.activeTabId) {
      return;
    }

    const currentIndex = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const nextIndex = (currentIndex + 1) % this.tabs.length;
    const nextTab = this.tabs[nextIndex];
    if (nextTab) {
      this.switchTab(nextTab.id);
    }
  }

  /**
   * Activates the previous tab (wraps to last)
   */
  activatePreviousTab(): void {
    if (this.tabs.length <= 1 || !this.activeTabId) {
      return;
    }

    const currentIndex = this.tabs.findIndex((t) => t.id === this.activeTabId);
    const prevIndex = (currentIndex - 1 + this.tabs.length) % this.tabs.length;
    const prevTab = this.tabs[prevIndex];
    if (prevTab) {
      this.switchTab(prevTab.id);
    }
  }

  /**
   * Activates a tab by index (0-based)
   */
  activateTabByIndex(index: number): void {
    if (index < 0 || index >= this.tabs.length) {
      return;
    }
    const tab = this.tabs[index];
    if (tab) {
      this.switchTab(tab.id);
    }
  }

  /**
   * Activates the last tab
   */
  activateLastTab(): void {
    if (this.tabs.length === 0) {
      return;
    }
    const lastTab = this.tabs[this.tabs.length - 1];
    if (lastTab) {
      this.switchTab(lastTab.id);
    }
  }

  /**
   * Reorders tabs by moving draggedTabId relative to targetTabId
   * @param draggedTabId The tab being moved
   * @param targetTabId The tab to position relative to
   * @param position 'before' or 'after' the target
   */
  reorderTabs(
    draggedTabId: string,
    targetTabId: string,
    position: "before" | "after",
  ): void {
    // Don't reorder if same tab
    if (draggedTabId === targetTabId) {
      return;
    }

    // Find both tabs
    const draggedIndex = this.tabs.findIndex((t) => t.id === draggedTabId);
    const targetIndex = this.tabs.findIndex((t) => t.id === targetTabId);

    // Bail if either not found
    if (draggedIndex === -1 || targetIndex === -1) {
      return;
    }

    // Remove the dragged tab
    const [draggedTab] = this.tabs.splice(draggedIndex, 1);

    // Calculate new index (adjusted after removal)
    let newIndex = targetIndex;
    if (draggedIndex < targetIndex) {
      // Dragged was before target, so target shifted down by 1
      newIndex = targetIndex - 1;
    }
    if (position === "after") {
      newIndex = newIndex + 1;
    }

    // Insert at new position
    this.tabs.splice(newIndex, 0, draggedTab!);

    // Emit reorder event
    this.eventEmitter.emit("tab:reordered", { tabs: [...this.tabs] });
  }

  /**
   * Gets the currently active tab
   */
  getActiveTab(): Tab | null {
    if (!this.activeTabId) {
      return null;
    }
    return this.tabs.find((t) => t.id === this.activeTabId) ?? null;
  }

  /**
   * Gets all tabs
   */
  getTabs(): Tab[] {
    return [...this.tabs];
  }

  /**
   * Gets a tab by ID
   */
  getTab(tabId: string): Tab | null {
    return this.tabs.find((t) => t.id === tabId) ?? null;
  }

  /**
   * Gets TerminalApp for a tab
   */
  getTerminalApp(tabId: string): TerminalApp | null {
    return this.terminalApps.get(tabId) ?? null;
  }

  /**
   * Gets the tab container element
   */
  getTabContainer(tabId: string): HTMLElement | null {
    return this.tabContainers.get(tabId) ?? null;
  }

  /**
   * Checks if an operation is in progress
   */
  isOperationInProgress(): boolean {
    return this.operationState.status !== "idle";
  }

  /**
   * Handles PTY session exit
   */
  async handleSessionExit(sessionId: string): Promise<void> {
    // Find tab by session ID using type guard
    const tab = this.tabs.find(
      (t) => isTerminalTab(t) && t.sessionId === sessionId,
    );

    if (tab) {
      await this.closeTab(tab.id);
    }
  }

  /**
   * Updates a tab's title and emits titleChanged event
   */
  updateTabTitle(tabId: string, title: string): void {
    const tab = this.tabs.find((t) => t.id === tabId);
    if (tab && tab.title !== title) {
      tab.title = title;
      this.eventEmitter.emit("tab:titleChanged", { tabId, title });
    }
  }

  /**
   * Sets callback for when last tab is closed
   */
  onLastTabClosed(callback: () => void): void {
    this.lastTabClosedCallback = callback;
  }

  /**
   * Subscribes to tab events
   */
  on<T extends TabEventType>(
    event: T,
    handler: TabEventHandler<T>,
  ): UnsubscribeFn {
    return this.eventEmitter.on(event, handler);
  }

  /**
   * Unsubscribes from tab events
   */
  off<T extends TabEventType>(event: T, handler: TabEventHandler<T>): void {
    this.eventEmitter.off(event, handler);
  }

  /**
   * Disposes all resources
   */
  async dispose(): Promise<void> {
    // Close all tabs
    for (const tab of [...this.tabs]) {
      await this.closeTab(tab.id);
    }
  }

  /**
   * Update a setting for all terminal tabs.
   * @param setting - The setting key
   * @param value - The new value
   */
  updateAllTerminalsSetting<K extends keyof RendererSettings>(
    setting: K,
    value: RendererSettings[K],
  ): void {
    for (const [_, terminal] of this.terminalApps) {
      terminal.applySetting(setting, value);
    }
  }

  /**
   * Update font size for all terminal tabs.
   * @param fontSize - New font size in points
   * @deprecated Use updateAllTerminalsSetting("fontSize", fontSize) instead
   */
  updateAllTerminalsFontSize(fontSize: number): void {
    this.updateAllTerminalsSetting("fontSize", fontSize);
  }
}
