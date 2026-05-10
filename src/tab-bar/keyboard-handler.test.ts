/**
 * TabKeyboardHandler Unit Tests
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";
import { TabKeyboardHandler } from "./keyboard-handler";
import { TabManager } from "./tab-manager";
import { SettingsService } from "../settings/settings-service";

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

// Create mock keyboard event
function createKeyboardEvent(
  key: string,
  options: Partial<{
    ctrlKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
    metaKey: boolean;
  }> = {},
): KeyboardEvent {
  return {
    key,
    ctrlKey: options.ctrlKey ?? false,
    shiftKey: options.shiftKey ?? false,
    altKey: options.altKey ?? false,
    metaKey: options.metaKey ?? false,
    preventDefault: mock(() => {}),
  } as unknown as KeyboardEvent;
}

describe("TabKeyboardHandler", () => {
  let container: HTMLElement;
  let tabManager: TabManager;
  let keyboardHandler: TabKeyboardHandler;

  beforeEach(() => {
    container = document.createElement("div");

    tabManager = new TabManager({
      container,
      createTerminalApp: async () => mockTerminalApp() as any,
    });

    keyboardHandler = new TabKeyboardHandler(tabManager);
  });

  describe("Ctrl+Shift+T (new tab)", () => {
    test("creates new tab", async () => {
      const event = createKeyboardEvent("t", { ctrlKey: true, shiftKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();

      // Wait for async create
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(tabManager.getTabs().length).toBe(1);
    });

    test("handles uppercase T", async () => {
      const event = createKeyboardEvent("T", { ctrlKey: true, shiftKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
    });
  });

  describe("Ctrl+Shift+G (new tab with global settings)", () => {
    const originalCached = SettingsService.getCached();

    afterEach(() => {
      // Restore cached settings after each test in this block
      (SettingsService as any).cachedSettings = originalCached;
    });

    function makeKeybinds(overrides: Record<string, string> = {}) {
      return {
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
        toggle_tab_bar: "Ctrl+Shift+B",
        jump_to_prev_prompt: "Ctrl+Shift+ArrowUp",
        jump_to_next_prompt: "Ctrl+Shift+ArrowDown",
        profile_selector: "Ctrl+Shift+P",
        ...overrides,
      };
    }

    test("creates new tab via tabManager.createTab when no profiles exist", async () => {
      // No SettingsService cache → keybinds is undefined → fallback to default
      const createTabSpy = mock(tabManager.createTab.bind(tabManager));
      tabManager.createTab = createTabSpy as any;

      const event = createKeyboardEvent("g", { ctrlKey: true, shiftKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(createTabSpy).toHaveBeenCalled();
      // No profile argument
      expect(createTabSpy.mock.calls[0]?.length ?? 0).toBe(0);
    });

    test("does not call tabBarUI.createTabWithProfile even when default profile exists", async () => {
      // Set cached settings with a default profile
      (SettingsService as any).cachedSettings = {
        profiles: [
          {
            name: "Default Profile",
            shell_path: "/bin/zsh",
            shell_args: [],
            env_vars: "",
            working_directory: "",
            is_default: true,
            ssh_connection_name: "",
            wsl_distro_name: "",
          },
        ],
        keybinds: makeKeybinds(),
      };

      const tabBarUIMock = {
        createTabWithProfile: mock(() => {}),
        showProfileSelector: mock(() => {}),
      };
      const handler = new TabKeyboardHandler(tabManager, {
        tabBarUI: tabBarUIMock as any,
      });

      const createTabSpy = mock(tabManager.createTab.bind(tabManager));
      tabManager.createTab = createTabSpy as any;

      const event = createKeyboardEvent("g", { ctrlKey: true, shiftKey: true });
      const handled = handler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(tabBarUIMock.createTabWithProfile).not.toHaveBeenCalled();
      expect(createTabSpy).toHaveBeenCalled();
      expect(createTabSpy.mock.calls[0]?.length ?? 0).toBe(0);
    });

    test("Ctrl+Shift+T regression: still uses profile-aware path", async () => {
      const defaultProfile = {
        name: "Default Profile",
        shell_path: "/bin/zsh",
        shell_args: [],
        env_vars: "",
        working_directory: "",
        is_default: true,
        ssh_connection_name: "",
        wsl_distro_name: "",
      };
      (SettingsService as any).cachedSettings = {
        profiles: [defaultProfile],
        keybinds: makeKeybinds(),
      };

      const tabBarUIMock = {
        createTabWithProfile: mock(() => {}),
        showProfileSelector: mock(() => {}),
      };
      const handler = new TabKeyboardHandler(tabManager, {
        tabBarUI: tabBarUIMock as any,
      });

      const event = createKeyboardEvent("t", { ctrlKey: true, shiftKey: true });
      const handled = handler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      // Profile-aware path must still be used for new_tab
      expect(tabBarUIMock.createTabWithProfile).toHaveBeenCalledWith(
        defaultProfile,
      );
    });

    test("custom keybind override (Ctrl+Alt+N) triggers global new tab", async () => {
      (SettingsService as any).cachedSettings = {
        profiles: [],
        keybinds: makeKeybinds({ new_tab_global: "Ctrl+Alt+N" }),
      };

      const createTabSpy = mock(tabManager.createTab.bind(tabManager));
      tabManager.createTab = createTabSpy as any;

      // Default Ctrl+Shift+G should NOT trigger now
      const eventDefault = createKeyboardEvent("g", {
        ctrlKey: true,
        shiftKey: true,
      });
      const handledDefault = keyboardHandler.handleKeyDown(eventDefault);
      expect(handledDefault).toBe(false);
      expect(createTabSpy).not.toHaveBeenCalled();

      // Custom Ctrl+Alt+N must trigger
      const eventCustom = createKeyboardEvent("n", {
        ctrlKey: true,
        altKey: true,
      });
      const handledCustom = keyboardHandler.handleKeyDown(eventCustom);
      expect(handledCustom).toBe(true);
      expect(eventCustom.preventDefault).toHaveBeenCalled();
      expect(createTabSpy).toHaveBeenCalled();
      expect(createTabSpy.mock.calls[0]?.length ?? 0).toBe(0);
    });
  });

  describe("Ctrl+Shift+W (close tab)", () => {
    test("closes active tab", async () => {
      // Create a tab first
      await tabManager.createTab();
      expect(tabManager.getTabs().length).toBe(1);

      const event = createKeyboardEvent("w", { ctrlKey: true, shiftKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();

      // Wait for async close
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(tabManager.getTabs().length).toBe(0);
    });

    test("handles uppercase W", async () => {
      await tabManager.createTab();

      const event = createKeyboardEvent("W", { ctrlKey: true, shiftKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
    });
  });

  describe("Ctrl+PageDown (next tab)", () => {
    test("activates next tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);

      const event = createKeyboardEvent("PageDown", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("wraps to first tab from last", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      // tab2 is active (last created)
      const event = createKeyboardEvent("PageDown", { ctrlKey: true });
      keyboardHandler.handleKeyDown(event);

      expect(tabManager.getActiveTab()?.id).toBe(tab1!.id);
    });
  });

  describe("Ctrl+PageUp (previous tab)", () => {
    test("activates previous tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // tab3 is active
      const event = createKeyboardEvent("PageUp", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("wraps to last tab from first", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);

      const event = createKeyboardEvent("PageUp", { ctrlKey: true });
      keyboardHandler.handleKeyDown(event);

      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });
  });

  describe("Ctrl+1-8 (tab by index)", () => {
    test("activates tab by index 1-8", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      // Activate tab 2 (index 1)
      const event = createKeyboardEvent("2", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id);
    });

    test("does nothing for index out of bounds", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();

      const event = createKeyboardEvent("5", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true); // Still handled to prevent propagation
      expect(tabManager.getActiveTab()?.id).toBe(tab2!.id); // Unchanged
    });
  });

  describe("Ctrl+9 (last tab)", () => {
    test("activates last tab", async () => {
      const tab1 = await tabManager.createTab();
      const tab2 = await tabManager.createTab();
      const tab3 = await tabManager.createTab();

      tabManager.switchTab(tab1!.id);

      const event = createKeyboardEvent("9", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(tabManager.getActiveTab()?.id).toBe(tab3!.id);
    });
  });

  describe("Ctrl+, (open settings)", () => {
    test("calls onOpenSettings callback", () => {
      const onOpenSettings = mock(() => {});
      const handler = new TabKeyboardHandler(tabManager, { onOpenSettings });

      const event = createKeyboardEvent(",", { ctrlKey: true });
      const handled = handler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
      expect(onOpenSettings).toHaveBeenCalled();
    });

    test("does nothing without callback", () => {
      const event = createKeyboardEvent(",", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(true);
      expect(event.preventDefault).toHaveBeenCalled();
    });
  });

  describe("non-tab shortcuts", () => {
    test("does not handle non-Ctrl keys", () => {
      const event = createKeyboardEvent("t", { ctrlKey: false });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(false);
      expect(event.preventDefault).not.toHaveBeenCalled();
    });

    test("does not handle unrelated Ctrl combinations", () => {
      const event = createKeyboardEvent("c", { ctrlKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(false);
      expect(event.preventDefault).not.toHaveBeenCalled();
    });

    test("does not handle Alt combinations", () => {
      const event = createKeyboardEvent("t", { ctrlKey: true, altKey: true });
      const handled = keyboardHandler.handleKeyDown(event);

      expect(handled).toBe(false);
    });
  });

  describe("attach/detach", () => {
    test("attaches keydown listener to target", async () => {
      const target = document.createElement("div");
      keyboardHandler.attach(target);

      // Simulate keydown event
      const event = createKeyboardEvent("t", { ctrlKey: true, shiftKey: true });
      target.dispatchEvent(
        new (
          globalThis.KeyboardEvent ??
          class extends Event {
            ctrlKey = true;
            shiftKey = true;
            key = "t";
            preventDefault = mock(() => {});
          }
        )("keydown", { ctrlKey: true, shiftKey: true, key: "t" }),
      );

      keyboardHandler.detach();
    });

    test("detach removes listener", () => {
      const target = document.createElement("div");
      keyboardHandler.attach(target);
      keyboardHandler.detach();

      // Verify no error when trying to detach again
      keyboardHandler.detach();
    });
  });
});
