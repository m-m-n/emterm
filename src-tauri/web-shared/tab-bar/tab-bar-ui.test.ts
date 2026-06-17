/**
 * TabBarUI Unit Tests
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Mock Tauri API before importing modules that depend on it
mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async (cmd: string) => {
    if (cmd === "load_settings") {
      return {
        font_size: 14,
        font_family: "monospace",
        ui_theme: "dark",
        ui_theme_preset: "purple",
        terminal_color_scheme: "default",
        padding: 8,
        scrollback_lines: 10000,
        show_scrollbar: "auto",
        shell_path: "/bin/bash",
        shell_args: [],
        cursor_style: "block",
        cursor_blink: true,
        scroll_speed: 3,
        bell_action: "none",
        url_detection: true,
        copy_on_select: false,
        keybinds: {
          copy: "Ctrl+Shift+C",
          paste: "Ctrl+Shift+V",
          select_all: "Ctrl+Shift+A",
          search: "Ctrl+Shift+F",
          new_tab: "Ctrl+Shift+T",
          new_tab_global: "Ctrl+Shift+G",
          close_tab: "Ctrl+Shift+W",
          next_tab: "Ctrl+PageDown",
          prev_tab: "Ctrl+PageUp",
          zoom_in: "Ctrl+Plus",
          zoom_out: "Ctrl+Minus",
          zoom_reset: "Ctrl+0",
          toggle_fullscreen: "F11",
          open_settings: "Ctrl+,",
        },
        language: "auto",
        custom_color_schemes: [],
      };
    }
    return null;
  }),
}));

import { TabBarUI } from "./tab-bar-ui";
import { TabManager } from "./tab-manager";
import type { Tab } from "./types";

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

describe("TabBarUI", () => {
  let tabBarContainer: HTMLElement;
  let contentContainer: HTMLElement;
  let tabManager: TabManager;
  let tabBarUI: TabBarUI;

  beforeEach(() => {
    // Create DOM elements
    tabBarContainer = document.createElement("div");
    tabBarContainer.id = "tab-bar";
    document.body.appendChild(tabBarContainer);

    contentContainer = document.createElement("div");
    contentContainer.id = "tab-content-container";
    document.body.appendChild(contentContainer);

    // Create TabManager
    tabManager = new TabManager({
      container: contentContainer,
      createTerminalApp: async () => mockTerminalApp() as any,
    });

    // Create TabBarUI
    tabBarUI = new TabBarUI({
      container: tabBarContainer,
      tabManager,
    });
  });

  afterEach(() => {
    tabBarUI.dispose();
    tabBarContainer.remove();
    contentContainer.remove();
  });

  describe("initialization", () => {
    test("creates tab bar structure", () => {
      tabBarUI.init();

      expect(tabBarContainer.querySelector(".tab-scroll-area")).not.toBeNull();
      expect(tabBarContainer.querySelector(".tab-fixed-area")).not.toBeNull();
    });

    test("creates new tab button", () => {
      tabBarUI.init();

      const newTabButton = tabBarContainer.querySelector(".tab-button-new");
      expect(newTabButton).not.toBeNull();
    });

    test("creates settings button", () => {
      tabBarUI.init();

      const settingsButton = tabBarContainer.querySelector(
        ".tab-button-settings",
      );
      expect(settingsButton).not.toBeNull();
    });
  });

  describe("tab rendering", () => {
    test("renders tab element when tab is created", async () => {
      tabBarUI.init();

      await tabManager.createTab();

      const tabElements = tabBarContainer.querySelectorAll(".tab");
      expect(tabElements.length).toBe(1);
    });

    test("renders tab title", async () => {
      tabBarUI.init();

      await tabManager.createTab({ title: "Test Tab" });

      const tabTitle = tabBarContainer.querySelector(".tab-title");
      expect(tabTitle?.textContent).toBe("Test Tab");
    });

    test("marks active tab with class", async () => {
      tabBarUI.init();

      const tab = await tabManager.createTab();

      const tabElement = tabBarContainer.querySelector(
        `[data-tab-id="${tab!.id}"]`,
      );
      expect(tabElement?.classList.contains("active")).toBe(true);
    });

    test("removes tab element when tab is closed", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      await tabManager.closeTab(tab1!.id);

      const tabElements = tabBarContainer.querySelectorAll(".tab");
      expect(tabElements.length).toBe(1);
    });

    test("updates active class when switching tabs", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);

      const tab1Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab1!.id}"]`,
      );
      const tab2Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab2!.id}"]`,
      );

      expect(tab1Element?.classList.contains("active")).toBe(true);
      expect(tab2Element?.classList.contains("active")).toBe(false);
    });
  });

  describe("click handling", () => {
    test("clicking tab activates it", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      const tab1Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab1!.id}"]`,
      ) as HTMLElement;
      tab1Element?.click();

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });

    test("clicking new tab button creates tab", async () => {
      tabBarUI.init();

      const newTabButton = tabBarContainer.querySelector(
        ".tab-button-new",
      ) as HTMLButtonElement;
      newTabButton?.click();

      // Wait for async operation
      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(tabManager.getTabs().length).toBe(1);
    });
  });

  describe("tab title update", () => {
    test("updates tab title when titleChanged event fires", async () => {
      tabBarUI.init();

      const tab = await tabManager.createTab({ title: "Initial Title" });

      // Simulate title change event (would normally come from terminal OSC)
      tabBarUI.updateTabTitle(tab!.id, "New Title");

      const tabTitle = tabBarContainer.querySelector(
        `[data-tab-id="${tab!.id}"] .tab-title`,
      );
      expect(tabTitle?.textContent).toBe("New Title");
    });
  });

  describe("dispose", () => {
    test("removes all tab elements", async () => {
      tabBarUI.init();

      await tabManager.createTab();
      await tabManager.createTab();

      tabBarUI.dispose();

      expect(tabBarContainer.innerHTML).toBe("");
    });
  });

  describe("settings tab singleton", () => {
    test("openOrFocusSettingsTab creates settings tab if none exists", async () => {
      tabBarUI.init();

      tabBarUI.openOrFocusSettingsTab();

      // Wait for async operation
      await new Promise((resolve) => setTimeout(resolve, 10));

      const tabs = tabManager.getTabs();
      const settingsTabs = tabs.filter((t) => t.type === "settings");
      expect(settingsTabs.length).toBe(1);
    });

    test("openOrFocusSettingsTab focuses existing settings tab", async () => {
      tabBarUI.init();

      // Create terminal tab first
      await tabManager.createTab();

      // Create settings tab
      const settingsTab = await tabManager.createTab({ type: "settings" });

      // Create another terminal tab
      await tabManager.createTab();

      // Focus settings tab again
      tabBarUI.openOrFocusSettingsTab();

      // Should not create a new settings tab
      const tabs = tabManager.getTabs();
      const settingsTabs = tabs.filter((t) => t.type === "settings");
      expect(settingsTabs.length).toBe(1);

      // Should have switched to settings tab
      expect(tabManager.getActiveTab()?.id).toBe(settingsTab!.id);
    });

    test("settings button triggers openOrFocusSettingsTab", async () => {
      tabBarUI.init();

      const settingsButton = tabBarContainer.querySelector(
        ".tab-button-settings",
      ) as HTMLButtonElement;
      settingsButton?.click();

      // Wait for async operation
      await new Promise((resolve) => setTimeout(resolve, 10));

      const tabs = tabManager.getTabs();
      const settingsTabs = tabs.filter((t) => t.type === "settings");
      expect(settingsTabs.length).toBe(1);
    });
  });

  describe("tab reordering", () => {
    test("reorders DOM elements when tab:reordered event fires", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // Initial order in DOM
      const scrollArea = tabBarContainer.querySelector(".tab-scroll-area");
      let tabElements = scrollArea?.querySelectorAll(".tab");
      expect(tabElements?.[0]?.getAttribute("data-tab-id")).toBe(tab1!.id);
      expect(tabElements?.[1]?.getAttribute("data-tab-id")).toBe(tab2!.id);
      expect(tabElements?.[2]?.getAttribute("data-tab-id")).toBe(tab3!.id);

      // Reorder: move tab3 to first position
      tabManager.reorderTabs(tab3!.id, tab1!.id, "before");

      // Check new order in DOM
      tabElements = scrollArea?.querySelectorAll(".tab");
      expect(tabElements?.[0]?.getAttribute("data-tab-id")).toBe(tab3!.id);
      expect(tabElements?.[1]?.getAttribute("data-tab-id")).toBe(tab1!.id);
      expect(tabElements?.[2]?.getAttribute("data-tab-id")).toBe(tab2!.id);
    });
  });

  describe("mux sub-tabs (renderMuxSubTabs)", () => {
    test("renders [1] badge even for a single mux window", async () => {
      tabBarUI.init();
      const tab = await tabManager.createTab();
      tabBarUI.renderMuxSubTabs(tab!.id, [{ name: "shell", active: true }]);

      const group = tabBarContainer.querySelector(".mux-tab-group");
      expect(group).not.toBeNull();
      const windowTabs = group!.querySelectorAll(".mux-window-tab");
      expect(windowTabs.length).toBe(1);
      const numberEl = windowTabs[0]!.querySelector(".mux-window-number");
      expect(numberEl?.textContent).toBe("[1]");
      const titleEl = windowTabs[0]!.querySelector(".tab-title");
      expect(titleEl?.textContent).toBe("shell");
    });

    test("renders sequential [1] [2] [3] badges for multiple windows", async () => {
      tabBarUI.init();
      const tab = await tabManager.createTab();
      tabBarUI.renderMuxSubTabs(tab!.id, [
        { name: "a", active: false },
        { name: "b", active: true },
        { name: "c", active: false },
      ]);

      const windowTabs = tabBarContainer.querySelectorAll(".mux-window-tab");
      expect(windowTabs.length).toBe(3);
      expect(
        windowTabs[0]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[1]");
      expect(
        windowTabs[1]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[2]");
      expect(
        windowTabs[2]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[3]");
    });

    test("updates number badges when the window list is reordered", async () => {
      tabBarUI.init();
      const tab = await tabManager.createTab();
      tabBarUI.renderMuxSubTabs(tab!.id, [
        { name: "a", active: true },
        { name: "b", active: false },
        { name: "c", active: false },
      ]);
      // Reorder to [c, a, b]
      tabBarUI.renderMuxSubTabs(tab!.id, [
        { name: "c", active: false },
        { name: "a", active: true },
        { name: "b", active: false },
      ]);

      const windowTabs = tabBarContainer.querySelectorAll(".mux-window-tab");
      expect(windowTabs[0]!.querySelector(".tab-title")?.textContent).toBe("c");
      expect(
        windowTabs[0]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[1]");
      expect(windowTabs[1]!.querySelector(".tab-title")?.textContent).toBe("a");
      expect(
        windowTabs[1]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[2]");
      expect(windowTabs[2]!.querySelector(".tab-title")?.textContent).toBe("b");
      expect(
        windowTabs[2]!.querySelector(".mux-window-number")?.textContent,
      ).toBe("[3]");
    });
  });

  describe("accessibility", () => {
    test("tab bar has role=tablist", () => {
      tabBarUI.init();

      expect(tabBarContainer.getAttribute("role")).toBe("tablist");
    });

    test("tab bar has aria-label", () => {
      tabBarUI.init();

      expect(tabBarContainer.getAttribute("aria-label")).toBe("Terminal tabs");
    });

    test("tabs have role=tab", async () => {
      tabBarUI.init();

      await tabManager.createTab();

      const tabElement = tabBarContainer.querySelector(".tab");
      expect(tabElement?.getAttribute("role")).toBe("tab");
    });

    test("tabs have tabindex for keyboard navigation", async () => {
      tabBarUI.init();

      await tabManager.createTab();

      const tabElement = tabBarContainer.querySelector(".tab");
      expect(tabElement?.getAttribute("tabindex")).toBe("0");
    });

    test("active tab has aria-selected=true", async () => {
      tabBarUI.init();

      await tabManager.createTab();

      const tabElement = tabBarContainer.querySelector(".tab");
      expect(tabElement?.getAttribute("aria-selected")).toBe("true");
    });

    test("inactive tab has aria-selected=false", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      const tab1Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab1!.id}"]`,
      );
      expect(tab1Element?.getAttribute("aria-selected")).toBe("false");
    });

    test("new tab button has aria-label", () => {
      tabBarUI.init();

      const newTabButton = tabBarContainer.querySelector(".tab-button-new");
      expect(newTabButton?.getAttribute("aria-label")).toBe("Create new tab");
    });

    test("settings button has aria-label", () => {
      tabBarUI.init();

      const settingsButton = tabBarContainer.querySelector(
        ".tab-button-settings",
      );
      expect(settingsButton?.getAttribute("aria-label")).toBe("Open settings");
    });

    test("tab can be activated with Enter key", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      const tab1Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab1!.id}"]`,
      ) as HTMLElement;

      // Simulate Enter keypress
      const event = new KeyboardEvent("keydown", { key: "Enter" });
      tab1Element.dispatchEvent(event);

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });

    test("tab can be activated with Space key", async () => {
      tabBarUI.init();

      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      const tab1Element = tabBarContainer.querySelector(
        `[data-tab-id="${tab1!.id}"]`,
      ) as HTMLElement;

      // Simulate Space keypress
      const event = new KeyboardEvent("keydown", { key: " " });
      tab1Element.dispatchEvent(event);

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });
  });
});
