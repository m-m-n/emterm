/**
 * Tab Drag Handler
 *
 * Implements pointer-event-based drag and drop for tab reordering.
 * Uses pointerdown/pointermove/pointerup instead of HTML5 Drag and Drop API
 * to avoid Tauri's dragDropEnabled conflict on Windows.
 */

import type { TabManager } from "./tab-manager";
import type { TabBarUI } from "./tab-bar-ui";

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

/** Minimum drag distance (px) before drag is recognized */
const DRAG_THRESHOLD = 5;

/**
 * TabDragHandler - Manages pointer-based drag operations for tab reordering
 */
export class TabDragHandler {
  private tabManager: TabManager;
  private tabBarUI: TabBarUI;

  private draggedTabId: string | null = null;
  private dropIndicatorPosition: DropIndicatorPosition | null = null;
  private dropIndicatorElement: HTMLElement | null = null;
  private unsubscribers: (() => void)[] = [];
  private boundHandlers: Map<string, EventListener> = new Map();

  // Pointer drag state
  private pointerStartX = 0;
  private pointerStartY = 0;
  private isDragging = false;
  private pendingTabId: string | null = null;
  private ghostElement: HTMLElement | null = null;
  private ghostOffsetX = 0;
  private ghostOffsetY = 0;

  // Document-level handlers (bound once during drag)
  private onPointerMoveBound = this.onPointerMove.bind(this);
  private onPointerUpBound = this.onPointerUp.bind(this);
  private onSelectStartBound = (e: Event) => e.preventDefault();

  constructor(options: TabDragHandlerOptions) {
    this.tabManager = options.tabManager;
    this.tabBarUI = options.tabBarUI;
  }

  /**
   * Initializes drag handlers
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
   * Attaches pointer event listeners to existing tabs
   */
  private attachListeners(): void {
    const tabs = this.tabManager.getTabs();
    for (const tab of tabs) {
      this.attachTabListeners(tab.id);
    }
  }

  /**
   * Attaches pointer listeners to a specific tab element
   */
  private attachTabListeners(tabId: string): void {
    const element = this.tabBarUI.getTabElement(tabId);
    if (!element) return;

    const pointerDownHandler = (e: Event) =>
      this.onPointerDown(e as PointerEvent);

    element.addEventListener("pointerdown", pointerDownHandler);

    this.boundHandlers.set(`${tabId}-pointerdown`, pointerDownHandler);
  }

  /**
   * Removes pointer listeners from a tab element
   */
  private detachTabListeners(tabId: string): void {
    const element = this.tabBarUI.getTabElement(tabId);
    if (!element) return;

    const handler = this.boundHandlers.get(`${tabId}-pointerdown`);
    if (handler) {
      element.removeEventListener("pointerdown", handler);
      this.boundHandlers.delete(`${tabId}-pointerdown`);
    }
  }

  /**
   * Subscribes to tab creation/close events
   */
  private subscribeToTabEvents(): void {
    const unsubCreated = this.tabManager.on("tab:created", ({ tab }) => {
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
   * Handles pointerdown on a tab element
   */
  private onPointerDown(event: PointerEvent): void {
    // Only respond to primary button (left click)
    if (event.button !== 0) return;

    const target = this.findTabElement(event.target as HTMLElement);
    if (!target) return;

    const tabId = target.dataset?.tabId;
    if (!tabId) return;

    const tab = this.tabManager.getTab(tabId);
    if (!tab) return;

    // Store start position for threshold check
    this.pendingTabId = tabId;
    this.pointerStartX = event.clientX;
    this.pointerStartY = event.clientY;
    this.isDragging = false;

    // Listen for move/up on document to handle drag outside tab
    document.addEventListener("pointermove", this.onPointerMoveBound);
    document.addEventListener("pointerup", this.onPointerUpBound);
  }

  /**
   * Handles pointermove on document during drag
   */
  private onPointerMove(event: PointerEvent): void {
    if (!this.pendingTabId) return;

    const dx = event.clientX - this.pointerStartX;
    const dy = event.clientY - this.pointerStartY;

    if (!this.isDragging) {
      // Check threshold
      if (Math.abs(dx) < DRAG_THRESHOLD && Math.abs(dy) < DRAG_THRESHOLD) {
        return;
      }
      // Start dragging
      this.isDragging = true;
      this.draggedTabId = this.pendingTabId;

      const element = this.tabBarUI.getTabElement(this.draggedTabId);
      if (element) {
        element.classList.add("dragging");
        this.createGhostElement(element, event.clientX, event.clientY);
      }
      // Prevent text selection during drag
      document.addEventListener("selectstart", this.onSelectStartBound);
      window.getSelection()?.removeAllRanges();
    }

    // Update ghost position and drop indicator
    this.updateGhostPosition(event.clientX, event.clientY);
    this.updateDropPosition(event.clientX);
  }

  /**
   * Handles pointerup on document
   */
  private onPointerUp(_event: PointerEvent): void {
    document.removeEventListener("pointermove", this.onPointerMoveBound);
    document.removeEventListener("pointerup", this.onPointerUpBound);

    if (this.isDragging && this.draggedTabId && this.dropIndicatorPosition) {
      const { targetTabId, position } = this.dropIndicatorPosition;
      this.tabManager.reorderTabs(this.draggedTabId, targetTabId, position);
    }

    this.clearDragState();
    this.restoreFocusToTerminal();

    this.pendingTabId = null;
    this.isDragging = false;
  }

  /**
   * Updates drop indicator position based on current pointer X coordinate
   */
  private updateDropPosition(clientX: number): void {
    if (!this.draggedTabId) return;

    const tabs = this.tabManager.getTabs();
    let bestTarget: DropIndicatorPosition | null = null;

    for (const tab of tabs) {
      if (tab.id === this.draggedTabId) continue;

      const element = this.tabBarUI.getTabElement(tab.id);
      if (!element) continue;

      const rect = element.getBoundingClientRect();
      const midpoint = rect.left + rect.width / 2;
      const position: "before" | "after" =
        clientX < midpoint ? "before" : "after";

      // Check if pointer is within the tab's horizontal bounds (with some tolerance)
      if (clientX >= rect.left - 10 && clientX <= rect.right + 10) {
        bestTarget = {
          targetTabId: tab.id,
          position,
          x: position === "before" ? rect.left : rect.right,
        };
        break;
      }
    }

    if (bestTarget) {
      this.dropIndicatorPosition = bestTarget;
      this.showDropIndicator();
    } else {
      this.hideDropIndicator();
    }
  }

  /**
   * Creates a ghost element that follows the cursor during drag
   */
  private createGhostElement(
    sourceElement: HTMLElement,
    clientX: number,
    clientY: number,
  ): void {
    const ghost = sourceElement.cloneNode(true) as HTMLElement;
    const rect = sourceElement.getBoundingClientRect();

    ghost.className = "tab tab-drag-ghost";
    ghost.style.position = "fixed";
    ghost.style.zIndex = "10000";
    ghost.style.pointerEvents = "none";
    ghost.style.opacity = "0.7";
    ghost.style.width = `${rect.width}px`;
    ghost.style.height = `${rect.height}px`;
    ghost.style.margin = "0";

    this.ghostOffsetX = clientX - rect.left;
    this.ghostOffsetY = clientY - rect.top;
    ghost.style.left = `${clientX - this.ghostOffsetX}px`;
    ghost.style.top = `${clientY - this.ghostOffsetY}px`;

    document.body.appendChild(ghost);
    this.ghostElement = ghost;
  }

  /**
   * Updates ghost element position to follow cursor
   */
  private updateGhostPosition(clientX: number, clientY: number): void {
    if (!this.ghostElement) return;
    this.ghostElement.style.left = `${clientX - this.ghostOffsetX}px`;
    this.ghostElement.style.top = `${clientY - this.ghostOffsetY}px`;
  }

  /**
   * Removes the ghost element
   */
  private removeGhostElement(): void {
    if (this.ghostElement) {
      this.ghostElement.remove();
      this.ghostElement = null;
    }
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

    if (!this.dropIndicatorElement.parentElement) {
      scrollArea.appendChild(this.dropIndicatorElement);
    }

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
    if (this.draggedTabId) {
      const element = this.tabBarUI.getTabElement(this.draggedTabId);
      if (element) {
        element.classList.remove("dragging");
      }
    }

    this.draggedTabId = null;
    this.removeGhostElement();
    this.hideDropIndicator();
    document.removeEventListener("selectstart", this.onSelectStartBound);
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
    // Remove document-level listeners if still active
    document.removeEventListener("pointermove", this.onPointerMoveBound);
    document.removeEventListener("pointerup", this.onPointerUpBound);
    document.removeEventListener("selectstart", this.onSelectStartBound);

    for (const unsubscribe of this.unsubscribers) {
      unsubscribe();
    }
    this.unsubscribers = [];

    const tabs = this.tabManager.getTabs();
    for (const tab of tabs) {
      this.detachTabListeners(tab.id);
    }

    if (this.dropIndicatorElement?.parentElement) {
      this.dropIndicatorElement.remove();
    }
    this.dropIndicatorElement = null;

    this.clearDragState();
    this.pendingTabId = null;
    this.isDragging = false;
  }
}
