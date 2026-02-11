/**
 * TabManager Unit Tests
 */

import { describe, test, expect, beforeEach, mock } from "bun:test";

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

import { TabManager } from "./tab-manager";
import type { Tab, TabEventPayloads } from "./types";

// Mock TerminalApp
const mockTerminalApp = () => ({
  init: mock(() => Promise.resolve()),
  dispose: mock(() => {}),
  onTitleChange: mock(() => {}),
  pty: {
    getSessionId: () => `session-${Math.random().toString(36).slice(2)}`,
    kill: mock(() => Promise.resolve()),
  },
});

// Mock container element
const mockContainer = () => {
  const div = document.createElement("div");
  div.id = "terminal-container";
  return div;
};

describe("TabManager", () => {
  let tabManager: TabManager;
  let container: HTMLElement;

  beforeEach(() => {
    container = mockContainer();
    tabManager = new TabManager({
      container,
      createTerminalApp: async (tabContainer) => {
        const app = mockTerminalApp();
        return app as any;
      },
    });
  });

  describe("createTab", () => {
    test("creates tab with unique ID", async () => {
      const tab = await tabManager.createTab();

      expect(tab).not.toBeNull();
      expect(tab!.id).toBeDefined();
      expect(tab!.id.length).toBeGreaterThan(0);
    });

    test("creates terminal tab by default", async () => {
      const tab = await tabManager.createTab();

      expect(tab).not.toBeNull();
      expect(tab!.type).toBe("terminal");
    });

    test("creates tab with specified title", async () => {
      const tab = await tabManager.createTab({ title: "My Terminal" });

      expect(tab).not.toBeNull();
      expect(tab!.title).toBe("My Terminal");
    });

    test("sets new tab as active", async () => {
      const tab = await tabManager.createTab();

      expect(tabManager.getActiveTab()).toBe(tab);
    });

    test("creates multiple tabs with unique IDs", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      expect(tab1!.id).not.toBe(tab2!.id);
      expect(tab2!.id).not.toBe(tab3!.id);
      expect(tab1!.id).not.toBe(tab3!.id);
    });

    test("emits tab:created event", async () => {
      let emittedPayload: TabEventPayloads["tab:created"] | null = null;
      tabManager.on("tab:created", (payload) => {
        emittedPayload = payload;
      });

      const tab = await tabManager.createTab();

      expect(emittedPayload).not.toBeNull();
      expect(emittedPayload!.tab).toBe(tab);
    });

    test("emits tab:activated event", async () => {
      let emittedPayload: TabEventPayloads["tab:activated"] | null = null;
      tabManager.on("tab:activated", (payload) => {
        emittedPayload = payload;
      });

      const tab = await tabManager.createTab();

      expect(emittedPayload).not.toBeNull();
      expect(emittedPayload!.tab).toBe(tab);
      expect(emittedPayload!.previousTabId).toBeNull();
    });

    test("blocks concurrent creation (state machine)", async () => {
      // Start first creation
      const promise1 = tabManager.createTab();

      // Second creation should be blocked while first is in progress
      const promise2 = tabManager.createTab();

      const [tab1, tab2] = await Promise.all([promise1, promise2]);

      // First should succeed, second should be null (blocked)
      expect(tab1).not.toBeNull();
      expect(tab2).toBeNull();
    });
  });

  describe("closeTab", () => {
    test("removes tab from tabs array", async () => {
      const tab = await tabManager.createTab();
      expect(tabManager.getTabs().length).toBe(1);

      await tabManager.closeTab(tab!.id);
      expect(tabManager.getTabs().length).toBe(0);
    });

    test("activates adjacent tab after closing", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // Close the middle tab (tab2 is not active, tab3 is)
      await tabManager.switchTab(tab2!.id);
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);

      await tabManager.closeTab(tab2!.id);

      // Should activate tab3 (next) or tab1 (previous)
      const activeTab = tabManager.getActiveTab();
      expect(activeTab).not.toBeNull();
      expect([tab1!.id, tab3!.id]).toContain(activeTab!.id);
    });

    test("emits tab:closed event", async () => {
      let emittedPayload: TabEventPayloads["tab:closed"] | null = null;
      tabManager.on("tab:closed", (payload) => {
        emittedPayload = payload;
      });

      const tab = await tabManager.createTab();
      await tabManager.closeTab(tab!.id);

      expect(emittedPayload).not.toBeNull();
      expect(emittedPayload!.tabId).toBe(tab!.id);
      expect(emittedPayload!.wasActive).toBe(true);
    });

    test("signals application exit when last tab closes", async () => {
      let exitSignaled = false;
      tabManager.onLastTabClosed(() => {
        exitSignaled = true;
      });

      const tab = await tabManager.createTab();
      await tabManager.closeTab(tab!.id);

      expect(exitSignaled).toBe(true);
    });

    test("does nothing for non-existent tab", async () => {
      await tabManager.createTab();
      const result = await tabManager.closeTab("non-existent-id");

      expect(result).toBe(false);
      expect(tabManager.getTabs().length).toBe(1);
    });
  });

  describe("switchTab", () => {
    test("updates activeTabId correctly", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);

      expect(tabManager.getActiveTab()).toBe(tab1);
    });

    test("emits tab:activated event", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      let emittedPayload: TabEventPayloads["tab:activated"] | null = null;
      tabManager.on("tab:activated", (payload) => {
        emittedPayload = payload;
      });

      tabManager.switchTab(tab1!.id);

      expect(emittedPayload).not.toBeNull();
      expect(emittedPayload!.tab).toBe(tab1);
      expect(emittedPayload!.previousTabId).toBe(tab2!.id);
    });

    test("emits tab:deactivated event for previous tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      let emittedPayload: TabEventPayloads["tab:deactivated"] | null = null;
      tabManager.on("tab:deactivated", (payload) => {
        emittedPayload = payload;
      });

      tabManager.switchTab(tab1!.id);

      expect(emittedPayload).not.toBeNull();
      expect(emittedPayload!.tab).toBe(tab2);
    });

    test("does nothing when switching to already active tab", async () => {
      const tab = await tabManager.createTab();

      let activatedCount = 0;
      tabManager.on("tab:activated", () => {
        activatedCount++;
      });

      // Reset count after creation
      activatedCount = 0;

      tabManager.switchTab(tab!.id);

      expect(activatedCount).toBe(0);
    });

    test("does nothing for non-existent tab", async () => {
      const tab = await tabManager.createTab();

      tabManager.switchTab("non-existent-id");

      expect(tabManager.getActiveTab()).toBe(tab);
    });
  });

  describe("activateNextTab", () => {
    test("activates next tab in order", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);
      tabManager.activateNextTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("wraps to first tab from last", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // tab3 is already active (last created)
      tabManager.activateNextTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });
  });

  describe("activatePreviousTab", () => {
    test("activates previous tab in order", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // tab3 is active
      tabManager.activatePreviousTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("wraps to last tab from first", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);
      tabManager.activatePreviousTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab3!.id);
    });
  });

  describe("activateTabByIndex", () => {
    test("activates tab by index", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.activateTabByIndex(1);

      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("does nothing for out of bounds index", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);
      tabManager.activateTabByIndex(10);

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });

    test("handles negative index gracefully", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.activateTabByIndex(-1);

      // Should not change
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });
  });

  describe("activateLastTab", () => {
    test("activates the last tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);
      tabManager.activateLastTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab3!.id);
    });
  });

  describe("closeActiveTab", () => {
    test("closes the currently active tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      await tabManager.closeActiveTab();

      expect(tabManager.getTabs().length).toBe(1);
      expect(tabManager.getTabs()[0].id).toBe(tab1!.id);
    });

    test("does nothing when no active tab", async () => {
      const result = await tabManager.closeActiveTab();

      expect(result).toBe(false);
    });
  });

  describe("getTerminalApp", () => {
    test("returns TerminalApp for terminal tab", async () => {
      const tab = await tabManager.createTab();

      const app = tabManager.getTerminalApp(tab!.id);

      expect(app).not.toBeNull();
    });

    test("returns null for non-existent tab", () => {
      const app = tabManager.getTerminalApp("non-existent-id");

      expect(app).toBeNull();
    });
  });

  describe("event subscription", () => {
    test("on() returns unsubscribe function", async () => {
      let count = 0;
      const unsubscribe = tabManager.on("tab:created", () => {
        count++;
      });

      await tabManager.createTab();
      expect(count).toBe(1);

      unsubscribe();

      await tabManager.createTab();
      expect(count).toBe(1);
    });

    test("off() removes handler", async () => {
      let count = 0;
      const handler = () => {
        count++;
      };

      tabManager.on("tab:created", handler);
      await tabManager.createTab();
      expect(count).toBe(1);

      tabManager.off("tab:created", handler);
      await tabManager.createTab();
      expect(count).toBe(1);
    });
  });

  describe("isOperationInProgress", () => {
    test("returns false when idle", () => {
      expect(tabManager.isOperationInProgress()).toBe(false);
    });
  });

  describe("handleSessionExit", () => {
    test("closes tab by session ID", async () => {
      const tab = await tabManager.createTab();

      // Get the session ID from the mock
      if (tab?.type === "terminal") {
        await tabManager.handleSessionExit(tab.sessionId);
        expect(tabManager.getTabs().length).toBe(0);
      }
    });

    test("does nothing for unknown session ID", async () => {
      await tabManager.createTab();

      await tabManager.handleSessionExit("unknown-session-id");

      expect(tabManager.getTabs().length).toBe(1);
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

      tabManager.reorderTabs("non-existent", tab1!.id, "before");

      const tabs = tabManager.getTabs();
      expect(tabs.length).toBe(1);
    });

    test("does nothing if target tab not found", async () => {
      const tab1 = await tabManager.createTab();

      tabManager.reorderTabs(tab1!.id, "non-existent", "before");

      const tabs = tabManager.getTabs();
      expect(tabs.length).toBe(1);
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

  describe("settings tab", () => {
    test("creates settings tab without PTY", async () => {
      const tab = await tabManager.createTab({ type: "settings" });

      expect(tab).not.toBeNull();
      expect(tab!.type).toBe("settings");
    });

    test("settings tab has correct title", async () => {
      const tab = await tabManager.createTab({ type: "settings" });

      expect(tab!.title).toBe("Settings");
    });

    test("settings tab can be closed", async () => {
      const tab = await tabManager.createTab({ type: "settings" });
      expect(tabManager.getTabs().length).toBe(1);

      await tabManager.closeTab(tab!.id);
      expect(tabManager.getTabs().length).toBe(0);
    });
  });

  describe("edge cases", () => {
    test("handles rapid tab creation (second is blocked)", async () => {
      const promise1 = tabManager.createTab();
      const promise2 = tabManager.createTab();

      const [tab1, tab2] = await Promise.all([promise1, promise2]);

      expect(tab1).not.toBeNull();
      expect(tab2).toBeNull();
      expect(tabManager.getTabs().length).toBe(1);
    });

    test("handles closing non-active tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // tab3 is active, close tab1
      await tabManager.closeTab(tab1!.id);

      expect(tabManager.getTabs().length).toBe(2);
      expect(tabManager.getActiveTab()?.id).toBe(tab3!.id);
    });

    test("handles many tabs (10 tabs)", async () => {
      const tabs: Tab[] = [];
      for (let i = 0; i < 10; i++) {
        const tab = await tabManager.createTab();
        if (tab) tabs.push(tab);
      }

      expect(tabs.length).toBe(10);
      expect(tabManager.getTabs().length).toBe(10);
    });

    test("activateTabByIndex with first index", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.activateTabByIndex(0);

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });

    test("activateNextTab with single tab does nothing", async () => {
      const tab = await tabManager.createTab();

      tabManager.activateNextTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab!.id);
    });

    test("activatePreviousTab with single tab does nothing", async () => {
      const tab = await tabManager.createTab();

      tabManager.activatePreviousTab();

      expect(tabManager.getActiveTab()?.id).toBe(tab!.id);
    });

    test("getTab returns null for non-existent tab", () => {
      const tab = tabManager.getTab("non-existent");
      expect(tab).toBeNull();
    });

    test("getTabContainer returns null for non-existent tab", () => {
      const container = tabManager.getTabContainer("non-existent");
      expect(container).toBeNull();
    });
  });

  describe("dispose", () => {
    test("closes all tabs and cleans up", async () => {
      await tabManager.createTab();
      await tabManager.createTab();
      await tabManager.createTab();

      expect(tabManager.getTabs().length).toBe(3);

      await tabManager.dispose();

      expect(tabManager.getTabs().length).toBe(0);
    });
  });
});
