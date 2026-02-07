/**
 * TabKeyboardHandler Unit Tests
 */

import { describe, test, expect, beforeEach, mock } from "bun:test";
import { TabKeyboardHandler } from "./keyboard-handler";
import { TabManager } from "./tab-manager";

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
