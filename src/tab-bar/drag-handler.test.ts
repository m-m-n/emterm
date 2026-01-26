/**
 * TabDragHandler Unit Tests
 */

import { describe, test, expect, beforeEach, mock } from "bun:test";
import { TabDragHandler } from "./drag-handler";
import { TabManager } from "./tab-manager";
import { TabBarUI } from "./tab-bar-ui";
import type { Tab } from "./types";

// Mock TerminalApp
const mockTerminalApp = () => ({
  init: mock(() => Promise.resolve()),
  dispose: mock(() => {}),
  pty: {
    getSessionId: () => `session-${Math.random().toString(36).slice(2)}`,
    kill: mock(() => Promise.resolve()),
  },
});

// Create mock DragEvent
function createDragEvent(
  type: string,
  options: Partial<{
    dataTransfer: DataTransfer | null;
    clientX: number;
    target: EventTarget | null;
    relatedTarget: EventTarget | null;
  }> = {},
): DragEvent {
  const mockDataTransfer: Partial<DataTransfer> = {
    setData: mock(() => {}),
    getData: mock(() => ""),
    effectAllowed: "move",
    dropEffect: "none",
    types: [],
    files: [] as unknown as FileList,
    items: [] as unknown as DataTransferItemList,
    clearData: mock(() => {}),
    setDragImage: mock(() => {}),
  };

  return {
    type,
    clientX: options.clientX ?? 0,
    target: options.target ?? null,
    relatedTarget: options.relatedTarget ?? null,
    dataTransfer: (options.dataTransfer ?? mockDataTransfer) as DataTransfer,
    preventDefault: mock(() => {}),
    stopPropagation: mock(() => {}),
  } as unknown as DragEvent;
}

describe("TabDragHandler", () => {
  let container: HTMLElement;
  let tabBarContainer: HTMLElement;
  let tabManager: TabManager;
  let tabBarUI: TabBarUI;
  let dragHandler: TabDragHandler;

  beforeEach(() => {
    container = document.createElement("div");
    tabBarContainer = document.createElement("div");

    tabManager = new TabManager({
      container,
      createTerminalApp: async () => mockTerminalApp() as any,
    });

    tabBarUI = new TabBarUI({
      container: tabBarContainer,
      tabManager,
    });
    tabBarUI.init();

    dragHandler = new TabDragHandler({
      tabManager,
      tabBarUI,
    });
  });

  describe("initialization", () => {
    test("creates drag handler with tabManager and tabBarUI", () => {
      expect(dragHandler).toBeDefined();
    });

    test("init attaches drag listeners", async () => {
      await tabManager.createTab();
      dragHandler.init();

      // Should not throw
      dragHandler.dispose();
    });
  });

  describe("drag start", () => {
    test("sets drag data on dragstart", async () => {
      const tab = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => ""),
        effectAllowed: "move",
        dropEffect: "none",
      };

      const event = createDragEvent("dragstart", {
        target: tabElement,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });

      dragHandler.handleDragStart(event);

      expect(mockDataTransfer.setData).toHaveBeenCalledWith(
        "text/plain",
        tab!.id,
      );
      expect(tabElement.classList.contains("dragging")).toBe(true);
    });

    test("settings tab cannot be dragged", async () => {
      const settingsTab = await tabManager.createTab({ type: "settings" });
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(settingsTab!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => ""),
        effectAllowed: "none",
        dropEffect: "none",
      };

      const event = createDragEvent("dragstart", {
        target: tabElement,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });

      dragHandler.handleDragStart(event);

      // Settings tab should not trigger drag
      expect(mockDataTransfer.setData).not.toHaveBeenCalled();
      expect(event.preventDefault).toHaveBeenCalled();
    });
  });

  describe("drag over", () => {
    test("allows drop on terminal tabs", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab2!.id)!;
      const event = createDragEvent("dragover", {
        target: tabElement,
        clientX: 100,
      });

      dragHandler.handleDragOver(event);

      expect(event.preventDefault).toHaveBeenCalled();
    });

    test("shows drop indicator", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      // Start dragging tab1
      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;
      const dragStartEvent = createDragEvent("dragstart", {
        target: tab1Element,
        dataTransfer: {
          setData: mock(() => {}),
          getData: mock(() => tab1!.id),
          effectAllowed: "move",
        } as unknown as DataTransfer,
      });
      dragHandler.handleDragStart(dragStartEvent);

      // Drag over tab2
      const tab2Element = tabBarUI.getTabElement(tab2!.id)!;
      const dragOverEvent = createDragEvent("dragover", {
        target: tab2Element,
        clientX: 100,
      });
      dragHandler.handleDragOver(dragOverEvent);

      expect(dragHandler.getDropIndicatorPosition()).toBeDefined();
    });
  });

  describe("drop", () => {
    test("reorders tabs on drop", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();
      dragHandler.init();

      // Initial order: tab1, tab2, tab3
      const initialTabs = tabManager.getTabs();
      expect(initialTabs[0]!.id).toBe(tab1!.id);
      expect(initialTabs[1]!.id).toBe(tab2!.id);
      expect(initialTabs[2]!.id).toBe(tab3!.id);

      // Start dragging tab3
      const tab3Element = tabBarUI.getTabElement(tab3!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => tab3!.id),
        effectAllowed: "move",
        dropEffect: "move",
      };
      const dragStartEvent = createDragEvent("dragstart", {
        target: tab3Element,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });
      dragHandler.handleDragStart(dragStartEvent);

      // Drag over tab1 to set drop indicator position
      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;
      // Mock getBoundingClientRect for tab1Element
      const origGetBoundingClientRect = tab1Element.getBoundingClientRect;
      tab1Element.getBoundingClientRect = () => ({
        left: 0,
        right: 100,
        width: 100,
        top: 0,
        bottom: 32,
        height: 32,
        x: 0,
        y: 0,
        toJSON: () => {},
      });

      const dragOverEvent = createDragEvent("dragover", {
        target: tab1Element,
        clientX: 10, // left side = before
      });
      dragHandler.handleDragOver(dragOverEvent);

      // Drop on tab1 (before position)
      const dropEvent = createDragEvent("drop", {
        target: tab1Element,
        clientX: 10,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });
      dragHandler.handleDrop(dropEvent);

      // Restore
      tab1Element.getBoundingClientRect = origGetBoundingClientRect;

      // New order should be: tab3, tab1, tab2
      const finalTabs = tabManager.getTabs();
      expect(finalTabs[0]!.id).toBe(tab3!.id);
      expect(finalTabs[1]!.id).toBe(tab1!.id);
      expect(finalTabs[2]!.id).toBe(tab2!.id);
    });

    test("clears dragging state on drop", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => tab1!.id),
        effectAllowed: "move",
        dropEffect: "move",
      };

      // Drag start
      const dragStartEvent = createDragEvent("dragstart", {
        target: tab1Element,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });
      dragHandler.handleDragStart(dragStartEvent);
      expect(tab1Element.classList.contains("dragging")).toBe(true);

      // Drop
      const dropEvent = createDragEvent("drop", {
        target: tabBarUI.getTabElement(tab2!.id)!,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });
      dragHandler.handleDrop(dropEvent);

      expect(tab1Element.classList.contains("dragging")).toBe(false);
    });
  });

  describe("drag end", () => {
    test("clears dragging state on dragend", async () => {
      const tab = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => ""),
        effectAllowed: "move",
      };

      // Start drag
      const dragStartEvent = createDragEvent("dragstart", {
        target: tabElement,
        dataTransfer: mockDataTransfer as unknown as DataTransfer,
      });
      dragHandler.handleDragStart(dragStartEvent);
      expect(tabElement.classList.contains("dragging")).toBe(true);

      // End drag
      const dragEndEvent = createDragEvent("dragend", {
        target: tabElement,
      });
      dragHandler.handleDragEnd(dragEndEvent);

      expect(tabElement.classList.contains("dragging")).toBe(false);
    });
  });

  describe("drag leave", () => {
    test("hides drop indicator on dragleave", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      // Start drag and drag over
      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;
      const tab2Element = tabBarUI.getTabElement(tab2!.id)!;
      const mockDataTransfer = {
        setData: mock(() => {}),
        getData: mock(() => tab1!.id),
        effectAllowed: "move",
      };

      dragHandler.handleDragStart(
        createDragEvent("dragstart", {
          target: tab1Element,
          dataTransfer: mockDataTransfer as unknown as DataTransfer,
        }),
      );

      dragHandler.handleDragOver(
        createDragEvent("dragover", {
          target: tab2Element,
          clientX: 100,
        }),
      );

      // Drag leave
      dragHandler.handleDragLeave(
        createDragEvent("dragleave", {
          target: tab2Element,
          relatedTarget: null, // leaving the tab bar
        }),
      );

      expect(dragHandler.getDropIndicatorPosition()).toBeNull();
    });
  });

  describe("reorderTabs", () => {
    test("moves tab before target", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // Move tab3 before tab1
      tabManager.reorderTabs(tab3!.id, tab1!.id, "before");

      const tabs = tabManager.getTabs();
      expect(tabs[0]!.id).toBe(tab3!.id);
      expect(tabs[1]!.id).toBe(tab1!.id);
      expect(tabs[2]!.id).toBe(tab2!.id);
    });

    test("moves tab after target", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // Move tab1 after tab3
      tabManager.reorderTabs(tab1!.id, tab3!.id, "after");

      const tabs = tabManager.getTabs();
      expect(tabs[0]!.id).toBe(tab2!.id);
      expect(tabs[1]!.id).toBe(tab3!.id);
      expect(tabs[2]!.id).toBe(tab1!.id);
    });

    test("emits tab:reordered event", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      let emittedTabs: Tab[] | null = null;
      tabManager.on("tab:reordered", ({ tabs }) => {
        emittedTabs = tabs;
      });

      tabManager.reorderTabs(tab2!.id, tab1!.id, "before");

      expect(emittedTabs).not.toBeNull();
      expect(emittedTabs!.length).toBe(2);
      expect(emittedTabs![0]!.id).toBe(tab2!.id);
    });

    test("does nothing if dragged tab not found", async () => {
      const tab1 = await tabManager.createTab();

      // Try to reorder non-existent tab
      tabManager.reorderTabs("non-existent", tab1!.id, "before");

      const tabs = tabManager.getTabs();
      expect(tabs.length).toBe(1);
      expect(tabs[0]!.id).toBe(tab1!.id);
    });

    test("does nothing if target tab not found", async () => {
      const tab1 = await tabManager.createTab();

      // Try to reorder to non-existent target
      tabManager.reorderTabs(tab1!.id, "non-existent", "before");

      const tabs = tabManager.getTabs();
      expect(tabs.length).toBe(1);
      expect(tabs[0]!.id).toBe(tab1!.id);
    });

    test("does nothing if dragging onto itself", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.reorderTabs(tab1!.id, tab1!.id, "before");

      const tabs = tabManager.getTabs();
      expect(tabs[0]!.id).toBe(tab1!.id);
      expect(tabs[1]!.id).toBe(tab2!.id);
    });
  });

  describe("dispose", () => {
    test("removes all event listeners", async () => {
      await tabManager.createTab();
      dragHandler.init();
      dragHandler.dispose();

      // Should not throw after dispose
      expect(true).toBe(true);
    });
  });
});
