/**
 * Tab Drag Handler
 *
 * Implements HTML5 drag and drop for tab reordering.
 */

import type { TabManager } from "./tab-manager";
import type { TabBarUI } from "./tab-bar-ui";
import type { Tab } from "./types";

/**
 * Options for creating TabDragHandler
 */
export interface TabDragHandlerOptions {
  /** TabManager instance */
  tabManager: TabManager;
  /** TabBarUI instance */
  tabBarUI: TabBarUI;
}

/**
 * Drop indicator position
 */
interface DropIndicatorPosition {
  targetTabId: string;
  position: "before" | "after";
  x: number;
}

/**
 * TabDragHandler - Manages drag and drop operations for tabs
 */
export class TabDragHandler {
  private tabManager: TabManager;
  private tabBarUI: TabBarUI;

  private draggedTabId: string | null = null;
  private dropIndicatorPosition: DropIndicatorPosition | null = null;
  private dropIndicatorElement: HTMLElement | null = null;
  private unsubscribers: (() => void)[] = [];
  private boundHandlers: Map<string, EventListener> = new Map();

  constructor(options: TabDragHandlerOptions) {
    this.tabManager = options.tabManager;
    this.tabBarUI = options.tabBarUI;
  }

  /**
   * Initializes drag and drop handlers
   */
  init(): void {
    this.createDropIndicator();
    this.attachListeners();
    this.subscribeToTabEvents();
  }

  /**
   * Creates the drop indicator element
   */
  private createDropIndicator(): void {
    this.dropIndicatorElement = document.createElement("div");
    this.dropIndicatorElement.className = "tab-drop-indicator";
    this.dropIndicatorElement.style.display = "none";
  }

  /**
   * Attaches drag event listeners to existing tabs
   */
  private attachListeners(): void {
    const tabs = this.tabManager.getTabs();
    for (const tab of tabs) {
      this.attachTabListeners(tab.id);
    }
  }

  /**
   * Attaches drag listeners to a specific tab element
   */
  private attachTabListeners(tabId: string): void {
    const element = this.tabBarUI.getTabElement(tabId);
    if (!element) return;

    const dragStartHandler = (e: Event) =>
      this.handleDragStart(e as DragEvent);
    const dragEndHandler = (e: Event) => this.handleDragEnd(e as DragEvent);
    const dragOverHandler = (e: Event) => this.handleDragOver(e as DragEvent);
    const dragLeaveHandler = (e: Event) =>
      this.handleDragLeave(e as DragEvent);
    const dropHandler = (e: Event) => this.handleDrop(e as DragEvent);

    element.addEventListener("dragstart", dragStartHandler);
    element.addEventListener("dragend", dragEndHandler);
    element.addEventListener("dragover", dragOverHandler);
    element.addEventListener("dragleave", dragLeaveHandler);
    element.addEventListener("drop", dropHandler);

    // Store handlers for cleanup
    this.boundHandlers.set(`${tabId}-dragstart`, dragStartHandler);
    this.boundHandlers.set(`${tabId}-dragend`, dragEndHandler);
    this.boundHandlers.set(`${tabId}-dragover`, dragOverHandler);
    this.boundHandlers.set(`${tabId}-dragleave`, dragLeaveHandler);
    this.boundHandlers.set(`${tabId}-drop`, dropHandler);
  }

  /**
   * Removes drag listeners from a tab element
   */
  private detachTabListeners(tabId: string): void {
    const element = this.tabBarUI.getTabElement(tabId);
    if (!element) return;

    const events = ["dragstart", "dragend", "dragover", "dragleave", "drop"];
    for (const eventName of events) {
      const handler = this.boundHandlers.get(`${tabId}-${eventName}`);
      if (handler) {
        element.removeEventListener(eventName, handler);
        this.boundHandlers.delete(`${tabId}-${eventName}`);
      }
    }
  }

  /**
   * Subscribes to tab creation/close events
   */
  private subscribeToTabEvents(): void {
    const unsubCreated = this.tabManager.on("tab:created", ({ tab }) => {
      // Attach listeners after a short delay to ensure DOM is ready
      requestAnimationFrame(() => {
        this.attachTabListeners(tab.id);
      });
    });
    this.unsubscribers.push(unsubCreated);

    const unsubClosed = this.tabManager.on("tab:closed", ({ tabId }) => {
      this.detachTabListeners(tabId);
    });
    this.unsubscribers.push(unsubClosed);
  }

  /**
   * Handles dragstart event
   */
  handleDragStart(event: DragEvent): void {
    const target = event.target as HTMLElement;
    const tabId = target.dataset?.tabId;

    if (!tabId) {
      event.preventDefault();
      return;
    }

    // Check if this is a settings tab (not draggable)
    const tab = this.tabManager.getTab(tabId);
    if (!tab || tab.type === "settings") {
      event.preventDefault();
      return;
    }

    this.draggedTabId = tabId;

    // Set drag data
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", tabId);
    }

    // Add dragging class
    target.classList.add("dragging");
  }

  /**
   * Handles dragend event
   */
  handleDragEnd(event: DragEvent): void {
    const target = event.target as HTMLElement;
    target.classList.remove("dragging");

    this.clearDragState();
    this.restoreFocusToTerminal();
  }

  /**
   * Handles dragover event
   */
  handleDragOver(event: DragEvent): void {
    event.preventDefault();

    if (!this.draggedTabId) return;

    const target = this.findTabElement(event.target as HTMLElement);
    if (!target) return;

    const tabId = target.dataset?.tabId;
    if (!tabId || tabId === this.draggedTabId) return;

    // Calculate position (before or after)
    const rect = target.getBoundingClientRect();
    const midpoint = rect.left + rect.width / 2;
    const position: "before" | "after" =
      event.clientX < midpoint ? "before" : "after";

    // Update drop indicator
    this.dropIndicatorPosition = {
      targetTabId: tabId,
      position,
      x: position === "before" ? rect.left : rect.right,
    };

    this.showDropIndicator();
  }

  /**
   * Handles dragleave event
   */
  handleDragLeave(event: DragEvent): void {
    // Only hide if leaving the tab bar entirely
    const relatedTarget = event.relatedTarget as HTMLElement | null;
    if (
      !relatedTarget ||
      !this.findTabElement(relatedTarget) ||
      relatedTarget.closest(".tab-scroll-area") === null
    ) {
      this.hideDropIndicator();
    }
  }

  /**
   * Handles drop event
   */
  handleDrop(event: DragEvent): void {
    event.preventDefault();

    if (!this.draggedTabId || !this.dropIndicatorPosition) {
      this.clearDragState();
      return;
    }

    const { targetTabId, position } = this.dropIndicatorPosition;

    // Perform the reorder
    this.tabManager.reorderTabs(this.draggedTabId, targetTabId, position);

    this.clearDragState();
  }

  /**
   * Finds the tab element from an event target
   */
  private findTabElement(element: HTMLElement | null): HTMLElement | null {
    if (!element) return null;
    if (element.classList.contains("tab")) return element;
    return element.closest(".tab") as HTMLElement | null;
  }

  /**
   * Shows the drop indicator
   */
  private showDropIndicator(): void {
    if (!this.dropIndicatorElement || !this.dropIndicatorPosition) return;

    const scrollArea = document.querySelector(".tab-scroll-area");
    if (!scrollArea) return;

    // Ensure indicator is in DOM
    if (!this.dropIndicatorElement.parentElement) {
      scrollArea.appendChild(this.dropIndicatorElement);
    }

    // Position the indicator
    const scrollRect = scrollArea.getBoundingClientRect();
    this.dropIndicatorElement.style.display = "block";
    this.dropIndicatorElement.style.left = `${this.dropIndicatorPosition.x - scrollRect.left}px`;
    this.dropIndicatorElement.style.top = "0";
  }

  /**
   * Hides the drop indicator
   */
  private hideDropIndicator(): void {
    if (this.dropIndicatorElement) {
      this.dropIndicatorElement.style.display = "none";
    }
    this.dropIndicatorPosition = null;
  }

  /**
   * Clears all drag state
   */
  private clearDragState(): void {
    // Remove dragging class from dragged element
    if (this.draggedTabId) {
      const element = this.tabBarUI.getTabElement(this.draggedTabId);
      if (element) {
        element.classList.remove("dragging");
      }
    }

    this.draggedTabId = null;
    this.hideDropIndicator();
  }

  /**
   * Restores focus to the active terminal after drag operations
   */
  private restoreFocusToTerminal(): void {
    const activeTab = this.tabManager.getActiveTab();
    if (activeTab) {
      const app = this.tabManager.getTerminalApp(activeTab.id);
      app?.focus();
    }
  }

  /**
   * Gets the current drop indicator position (for testing)
   */
  getDropIndicatorPosition(): DropIndicatorPosition | null {
    return this.dropIndicatorPosition;
  }

  /**
   * Disposes the drag handler
   */
  dispose(): void {
    // Unsubscribe from events
    for (const unsubscribe of this.unsubscribers) {
      unsubscribe();
    }
    this.unsubscribers = [];

    // Remove all tab listeners
    const tabs = this.tabManager.getTabs();
    for (const tab of tabs) {
      this.detachTabListeners(tab.id);
    }

    // Remove drop indicator
    if (this.dropIndicatorElement?.parentElement) {
      this.dropIndicatorElement.remove();
    }
    this.dropIndicatorElement = null;

    this.clearDragState();
  }
}
