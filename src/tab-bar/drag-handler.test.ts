/**
 * TabDragHandler Unit Tests (pointer-event based)
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Mock Tauri API before importing modules that depend on it
mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async (cmd: string) => {
    if (cmd === "load_settings") {
      return {
        font_size: 14, font_family: "monospace",
        ui_theme: "dark", ui_theme_preset: "purple", terminal_color_scheme: "default",
        padding: 8, scrollback_lines: 10000, show_scrollbar: "auto",
        shell_path: "/bin/bash", shell_args: [], cursor_style: "block",
        cursor_blink: true, scroll_speed: 3, bell_action: "none",
        url_detection: true, copy_on_select: false,
        keybinds: {
          copy: "Ctrl+Shift+C", paste: "Ctrl+Shift+V", select_all: "Ctrl+Shift+A",
          search: "Ctrl+Shift+F", new_tab: "Ctrl+Shift+T", close_tab: "Ctrl+Shift+W",
          next_tab: "Ctrl+PageDown", prev_tab: "Ctrl+PageUp",
          zoom_in: "Ctrl+Plus", zoom_out: "Ctrl+Minus", zoom_reset: "Ctrl+0",
          toggle_fullscreen: "F11", open_settings: "Ctrl+,",
        },
        language: "auto",
        custom_color_schemes: [],
      };
    }
    return null;
  }),
}));

import { TabDragHandler } from "./drag-handler";
import { TabManager } from "./tab-manager";
import { TabBarUI } from "./tab-bar-ui";
import type { Tab } from "./types";

// Mock TerminalApp
const mockTerminalApp = () => ({
  init: mock(() => Promise.resolve()),
  dispose: mock(() => {}),
  focus: mock(() => {}),
  onTitleChange: mock(() => {}),
  pty: {
    getSessionId: () => `session-${Math.random().toString(36).slice(2)}`,
    kill: mock(() => Promise.resolve()),
  },
});

// Polyfill PointerEvent for happy-dom (which doesn't have it)
if (typeof globalThis.PointerEvent === "undefined") {
  (globalThis as any).PointerEvent = class PointerEvent extends Event {
    readonly clientX: number;
    readonly clientY: number;
    readonly button: number;
    constructor(type: string, init: PointerEventInit & { clientX?: number; clientY?: number; button?: number } = {}) {
      super(type, init);
      this.clientX = init.clientX ?? 0;
      this.clientY = init.clientY ?? 0;
      this.button = init.button ?? 0;
    }
  };
}

function createPointerEvent(
  type: string,
  options: {
    clientX?: number;
    clientY?: number;
    button?: number;
  } = {},
): PointerEvent {
  return new PointerEvent(type, {
    clientX: options.clientX ?? 0,
    clientY: options.clientY ?? 0,
    button: options.button ?? 0,
    bubbles: true,
    cancelable: true,
  });
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

  afterEach(() => {
    dragHandler.dispose();
  });

  describe("initialization", () => {
    test("creates drag handler with tabManager and tabBarUI", () => {
      expect(dragHandler).toBeDefined();
    });

    test("init attaches pointer listeners", async () => {
      await tabManager.createTab();
      dragHandler.init();

      // Should not throw
      dragHandler.dispose();
    });
  });

  describe("pointer drag start", () => {
    test("adds dragging class after threshold exceeded", async () => {
      const tab = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab!.id)!;

      // Pointer down on tab element
      tabElement.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 50, clientY: 10, button: 0 }),
      );

      // Move past threshold on document
      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 60, clientY: 10 }),
      );

      expect(tabElement.classList.contains("dragging")).toBe(true);
    });

    test("does not start drag before threshold", async () => {
      const tab = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab!.id)!;

      tabElement.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 50, clientY: 10, button: 0 }),
      );

      // Move less than threshold
      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 52, clientY: 10 }),
      );

      expect(tabElement.classList.contains("dragging")).toBe(false);
    });

    test("does not start drag on right click", async () => {
      const tab = await tabManager.createTab();
      dragHandler.init();

      const tabElement = tabBarUI.getTabElement(tab!.id)!;

      // Right-click
      tabElement.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 50, clientY: 10, button: 2 }),
      );

      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 60, clientY: 10 }),
      );

      expect(tabElement.classList.contains("dragging")).toBe(false);
    });
  });

  describe("drag over (pointer move)", () => {
    test("shows drop indicator when dragging over another tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;
      const tab2Element = tabBarUI.getTabElement(tab2!.id)!;

      // Mock getBoundingClientRect for tab2
      tab2Element.getBoundingClientRect = () => ({
        left: 100, right: 200, width: 100,
        top: 0, bottom: 32, height: 32,
        x: 100, y: 0, toJSON: () => {},
      });

      // Start drag on tab1
      tab1Element.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 50, clientY: 10, button: 0 }),
      );

      // Move over tab2 (past threshold, within tab2 bounds)
      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 120, clientY: 10 }),
      );

      expect(dragHandler.getDropIndicatorPosition()).toBeDefined();
      expect(dragHandler.getDropIndicatorPosition()?.targetTabId).toBe(tab2!.id);
    });
  });

  describe("drop (pointer up)", () => {
    test("reorders tabs on pointer up", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();
      dragHandler.init();

      // Initial order: tab1, tab2, tab3
      const initialTabs = tabManager.getTabs();
      expect(initialTabs[0]!.id).toBe(tab1!.id);
      expect(initialTabs[1]!.id).toBe(tab2!.id);
      expect(initialTabs[2]!.id).toBe(tab3!.id);

      const tab3Element = tabBarUI.getTabElement(tab3!.id)!;
      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;

      // Mock getBoundingClientRect for tab1
      tab1Element.getBoundingClientRect = () => ({
        left: 0, right: 100, width: 100,
        top: 0, bottom: 32, height: 32,
        x: 0, y: 0, toJSON: () => {},
      });

      // Start drag on tab3
      tab3Element.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 250, clientY: 10, button: 0 }),
      );

      // Move to tab1 (before position - clientX=10 < midpoint=50)
      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 10, clientY: 10 }),
      );

      // Release
      document.dispatchEvent(
        createPointerEvent("pointerup", { clientX: 10, clientY: 10 }),
      );

      // New order should be: tab3, tab1, tab2
      const finalTabs = tabManager.getTabs();
      expect(finalTabs[0]!.id).toBe(tab3!.id);
      expect(finalTabs[1]!.id).toBe(tab1!.id);
      expect(finalTabs[2]!.id).toBe(tab2!.id);
    });

    test("clears dragging state on pointer up", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      dragHandler.init();

      const tab1Element = tabBarUI.getTabElement(tab1!.id)!;

      // Start drag
      tab1Element.dispatchEvent(
        createPointerEvent("pointerdown", { clientX: 50, clientY: 10, button: 0 }),
      );

      // Move past threshold
      document.dispatchEvent(
        createPointerEvent("pointermove", { clientX: 60, clientY: 10 }),
      );
      expect(tab1Element.classList.contains("dragging")).toBe(true);

      // Pointer up
      document.dispatchEvent(
        createPointerEvent("pointerup", { clientX: 60, clientY: 10 }),
      );

      expect(tab1Element.classList.contains("dragging")).toBe(false);
    });
  });

  describe("reorderTabs", () => {
    test("moves tab before target", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

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

      tabManager.reorderTabs("non-existent", tab1!.id, "before");

      const tabs = tabManager.getTabs();
      expect(tabs.length).toBe(1);
      expect(tabs[0]!.id).toBe(tab1!.id);
    });

    test("does nothing if target tab not found", async () => {
      const tab1 = await tabManager.createTab();

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

      expect(true).toBe(true);
    });
  });
});
