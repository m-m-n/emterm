/**
 * Tests for KeyboardHandler class - capture phase clipboard shortcut handling
 */

import { describe, expect, it, mock, spyOn } from "bun:test";

// Mock external dependencies that keyboard.ts transitively imports.
// These mocks must be registered before importing the module under test.
mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async () => null),
  Resource: class Resource {
    readonly rid: number;
    constructor(rid: number) { this.rid = rid; }
    close() { return Promise.resolve(); }
  },
  Channel: class Channel {},
  transformCallback: () => 0,
}));

mock.module("../../settings/settings-service", () => ({
  SettingsService: {
    load: () => Promise.resolve(null),
    save: () => Promise.resolve(),
    getCached: () => null,
  },
}));

mock.module("../../clipboard", () => ({
  ClipboardManager: {},
  showPasteDialog: mock(async () => ({ confirmed: false })),
  sendTextInChunks: mock(async () => {}),
}));

import { KeyboardHandler, type KeyboardHandlerContext } from "./keyboard";
import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { SelectionController } from "../../selection-v2";

/**
 * Helper to create a mock KeyboardEvent with spies
 */
function createKeyEvent(
  key: string,
  options: {
    ctrlKey?: boolean;
    altKey?: boolean;
    shiftKey?: boolean;
    metaKey?: boolean;
    code?: string;
  } = {},
): KeyboardEvent & {
  preventDefault: ReturnType<typeof mock>;
  stopPropagation: ReturnType<typeof mock>;
} {
  const event = {
    key,
    code: options.code ?? "",
    ctrlKey: options.ctrlKey ?? false,
    altKey: options.altKey ?? false,
    shiftKey: options.shiftKey ?? false,
    metaKey: options.metaKey ?? false,
    isComposing: false,
    preventDefault: mock(() => {}),
    stopPropagation: mock(() => {}),
  } as unknown as KeyboardEvent & {
    preventDefault: ReturnType<typeof mock>;
    stopPropagation: ReturnType<typeof mock>;
  };
  return event;
}

/**
 * Creates a mock PtyClient
 */
function createMockPtyClient(): PtyClient {
  return {
    write: mock(() => Promise.resolve()),
    resize: mock(() => Promise.resolve()),
    onData: mock(() => {}),
    spawn: mock(() => Promise.resolve()),
    kill: mock(() => Promise.resolve()),
  } as unknown as PtyClient;
}

/**
 * Creates a mock SelectionController
 */
function createMockSelectionController(): SelectionController {
  return {
    hasSelection: mock(() => false),
    copy: mock(() => Promise.resolve(true)),
    paste: mock(() => Promise.resolve("")),
    clearSelection: mock(() => {}),
    isMultiLinePaste: mock(() => false),
    countPasteLines: mock(() => 1),
  } as unknown as SelectionController;
}

/**
 * Creates a mock TerminalState
 */
function createMockState(): TerminalState {
  return {} as TerminalState;
}

/**
 * Creates a KeyboardHandlerContext for testing
 */
function createTestContext(
  overrides: Partial<KeyboardHandlerContext> = {},
): KeyboardHandlerContext {
  return {
    ptyClient: createMockPtyClient(),
    getState: () => createMockState(),
    getRenderer: () => null,
    selectionController: createMockSelectionController(),
    isEditContextActive: () => false,
    isImeInputFocused: () => false,
    ...overrides,
  };
}

describe("KeyboardHandler", () => {
  describe("handleClipboardShortcut", () => {
    describe("modifier key checks", () => {
      it("should not handle events without Ctrl key", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        // Create event with only Shift (no Ctrl)
        const event = createKeyEvent("c", { shiftKey: true });

        // Call the method directly via type assertion
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Should not be handled
        expect(event.preventDefault).not.toHaveBeenCalled();
        expect(event.stopPropagation).not.toHaveBeenCalled();
      });

      it("should not handle events without Shift key", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        // Create event with only Ctrl (no Shift)
        const event = createKeyEvent("c", { ctrlKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Should not be handled by clipboard shortcut handler
        expect(event.preventDefault).not.toHaveBeenCalled();
        expect(event.stopPropagation).not.toHaveBeenCalled();
      });
    });

    describe("Ctrl+Shift+C handling", () => {
      it("should call handleCopy for Ctrl+Shift+C", async () => {
        const selectionController = createMockSelectionController();
        (
          selectionController.hasSelection as ReturnType<typeof mock>
        ).mockReturnValue(true);

        const context = createTestContext({ selectionController });
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("c", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Wait for async operations
        await new Promise((resolve) => setTimeout(resolve, 10));

        expect(selectionController.copy).toHaveBeenCalled();
      });

      it("should call handleCopy for Ctrl+Shift+C with uppercase C", async () => {
        const selectionController = createMockSelectionController();
        (
          selectionController.hasSelection as ReturnType<typeof mock>
        ).mockReturnValue(true);

        const context = createTestContext({ selectionController });
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("C", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Wait for async operations
        await new Promise((resolve) => setTimeout(resolve, 10));

        expect(selectionController.copy).toHaveBeenCalled();
      });

      it("should call preventDefault and stopPropagation for Ctrl+Shift+C", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("c", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        expect(event.preventDefault).toHaveBeenCalled();
        expect(event.stopPropagation).toHaveBeenCalled();
      });
    });

    describe("Ctrl+Shift+V handling", () => {
      it("should call handlePaste for Ctrl+Shift+V", async () => {
        const selectionController = createMockSelectionController();
        (
          selectionController.paste as ReturnType<typeof mock>
        ).mockResolvedValue("test text");

        const context = createTestContext({ selectionController });
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("v", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Wait for async operations
        await new Promise((resolve) => setTimeout(resolve, 10));

        expect(selectionController.paste).toHaveBeenCalled();
      });

      it("should call handlePaste for Ctrl+Shift+V with uppercase V", async () => {
        const selectionController = createMockSelectionController();
        (
          selectionController.paste as ReturnType<typeof mock>
        ).mockResolvedValue("test text");

        const context = createTestContext({ selectionController });
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("V", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Wait for async operations
        await new Promise((resolve) => setTimeout(resolve, 10));

        expect(selectionController.paste).toHaveBeenCalled();
      });

      it("should call preventDefault and stopPropagation for Ctrl+Shift+V", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("v", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        expect(event.preventDefault).toHaveBeenCalled();
        expect(event.stopPropagation).toHaveBeenCalled();
      });
    });

    describe("other Ctrl+Shift combinations", () => {
      it("should not handle Ctrl+Shift+X", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("x", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Should not handle this key
        expect(event.preventDefault).not.toHaveBeenCalled();
        expect(event.stopPropagation).not.toHaveBeenCalled();
      });

      it("should not handle Ctrl+Shift+A", () => {
        const context = createTestContext();
        const handler = new KeyboardHandler(context);

        const event = createKeyEvent("a", { ctrlKey: true, shiftKey: true });

        // Call the method directly
        (
          handler as unknown as {
            handleClipboardShortcut: (e: KeyboardEvent) => void;
          }
        ).handleClipboardShortcut(event);

        // Should not handle this key
        expect(event.preventDefault).not.toHaveBeenCalled();
        expect(event.stopPropagation).not.toHaveBeenCalled();
      });
    });
  });

  describe("attach/detach", () => {
    it("should register capture phase listener on attach", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);
      const target = new EventTarget();

      const addEventListenerSpy = spyOn(target, "addEventListener");

      handler.attach(target);

      // Should be called twice: once for capture, once for bubble
      expect(addEventListenerSpy).toHaveBeenCalledTimes(2);

      // Check capture listener was added with { capture: true }
      const captureCall = addEventListenerSpy.mock.calls.find(
        (call) =>
          call[2] &&
          typeof call[2] === "object" &&
          (call[2] as AddEventListenerOptions).capture === true,
      );
      expect(captureCall).toBeDefined();
      expect(captureCall?.[0]).toBe("keydown");

      handler.detach();
    });

    it("should remove capture phase listener on detach", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);
      const target = new EventTarget();

      handler.attach(target);

      const removeEventListenerSpy = spyOn(target, "removeEventListener");

      handler.detach();

      // Should be called twice: once for capture, once for bubble
      expect(removeEventListenerSpy).toHaveBeenCalledTimes(2);

      // Check capture listener was removed with { capture: true }
      const captureCall = removeEventListenerSpy.mock.calls.find(
        (call) =>
          call[2] &&
          typeof call[2] === "object" &&
          (call[2] as AddEventListenerOptions).capture === true,
      );
      expect(captureCall).toBeDefined();
      expect(captureCall?.[0]).toBe("keydown");
    });

    it("should handle multiple attach/detach cycles correctly", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);
      const target = new EventTarget();

      // First cycle
      handler.attach(target);
      handler.detach();

      // Second cycle
      handler.attach(target);
      handler.detach();

      // Third cycle - should work without errors
      handler.attach(target);

      // Verify capture listener is registered
      const captureHandler = (
        handler as unknown as {
          boundHandleClipboardShortcut: ((e: KeyboardEvent) => void) | null;
        }
      ).boundHandleClipboardShortcut;
      expect(captureHandler).not.toBeNull();

      handler.detach();
    });

    it("should automatically detach before re-attaching", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);
      const target = new EventTarget();

      const removeEventListenerSpy = spyOn(target, "removeEventListener");

      // First attach
      handler.attach(target);

      // Second attach without explicit detach - should auto-detach first
      handler.attach(target);

      // removeEventListener should have been called for cleanup
      expect(removeEventListenerSpy).toHaveBeenCalled();

      handler.detach();
    });
  });

  describe("handleClipboardShortcut edge cases", () => {
    it("should handle Ctrl+Shift+C with empty selection gracefully", async () => {
      const selectionController = createMockSelectionController();
      // Empty selection - hasSelection returns false
      (
        selectionController.hasSelection as ReturnType<typeof mock>
      ).mockReturnValue(false);

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("c", { ctrlKey: true, shiftKey: true });

      // Call the method directly
      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      // Wait for async operations
      await new Promise((resolve) => setTimeout(resolve, 10));

      // copy should not be called when there's no selection
      expect(selectionController.copy).not.toHaveBeenCalled();
      // But preventDefault/stopPropagation should still be called
      expect(event.preventDefault).toHaveBeenCalled();
      expect(event.stopPropagation).toHaveBeenCalled();
    });

    it("should handle Ctrl+Shift+V with empty clipboard gracefully", async () => {
      const selectionController = createMockSelectionController();
      // Empty clipboard
      (selectionController.paste as ReturnType<typeof mock>).mockResolvedValue(
        "",
      );

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("v", { ctrlKey: true, shiftKey: true });

      // Call the method directly
      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      // Wait for async operations
      await new Promise((resolve) => setTimeout(resolve, 10));

      // paste should be called
      expect(selectionController.paste).toHaveBeenCalled();
      // preventDefault/stopPropagation should be called
      expect(event.preventDefault).toHaveBeenCalled();
      expect(event.stopPropagation).toHaveBeenCalled();
    });

    it("should handle Ctrl+Shift+C during IME composition", () => {
      const context = createTestContext({
        isImeInputFocused: () => true,
      });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("c", { ctrlKey: true, shiftKey: true });
      // Simulate IME composition
      Object.defineProperty(event, "isComposing", { value: true });

      // Call the method directly
      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      // Should still handle the event (capture phase ignores isComposing)
      expect(event.preventDefault).toHaveBeenCalled();
      expect(event.stopPropagation).toHaveBeenCalled();
    });
  });

  // NOTE: IME ON (event.key='Process') tests removed.
  // Investigation revealed that when IME is active, the browser/OS intercepts
  // Ctrl+Shift+C/V before the keydown event reaches JavaScript with correct
  // modifier key states. This is a known limitation shared by other terminals
  // like Tabby. Users should turn off IME before using clipboard shortcuts.

  describe("handleClipboardShortcut (normal operation)", () => {
    it("should use event.key for copy when IME is not blocking", async () => {
      const selectionController = createMockSelectionController();
      (
        selectionController.hasSelection as ReturnType<typeof mock>
      ).mockReturnValue(true);

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      // Normal: key is "c" and code is "KeyC"
      const event = createKeyEvent("c", {
        ctrlKey: true,
        shiftKey: true,
        code: "KeyC",
      });

      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(selectionController.copy).toHaveBeenCalled();
    });

    it("should use event.key for paste when IME is not blocking", async () => {
      const selectionController = createMockSelectionController();
      (selectionController.paste as ReturnType<typeof mock>).mockResolvedValue(
        "test text",
      );

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      // Normal: key is "v" and code is "KeyV"
      const event = createKeyEvent("v", {
        ctrlKey: true,
        shiftKey: true,
        code: "KeyV",
      });

      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(selectionController.paste).toHaveBeenCalled();
    });

    it("should work with non-QWERTY layout (Dvorak) where code differs from key - copy", async () => {
      const selectionController = createMockSelectionController();
      (
        selectionController.hasSelection as ReturnType<typeof mock>
      ).mockReturnValue(true);

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      // Dvorak: physical 'i' key produces 'c', code is "KeyI" but key is "c"
      // IME OFF, so we use event.key which is "c"
      const event = createKeyEvent("c", {
        ctrlKey: true,
        shiftKey: true,
        code: "KeyI",
      });

      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(selectionController.copy).toHaveBeenCalled();
    });

    it("should work with non-QWERTY layout (Dvorak) where code differs from key - paste", async () => {
      const selectionController = createMockSelectionController();
      (selectionController.paste as ReturnType<typeof mock>).mockResolvedValue(
        "test text",
      );

      const context = createTestContext({ selectionController });
      const handler = new KeyboardHandler(context);

      // Dvorak: physical '.' key produces 'v', code is "Period" but key is "v"
      // IME OFF, so we use event.key which is "v"
      const event = createKeyEvent("v", {
        ctrlKey: true,
        shiftKey: true,
        code: "Period",
      });

      (
        handler as unknown as {
          handleClipboardShortcut: (e: KeyboardEvent) => void;
        }
      ).handleClipboardShortcut(event);

      await new Promise((resolve) => setTimeout(resolve, 10));

      expect(selectionController.paste).toHaveBeenCalled();
    });
  });

  describe("Ctrl+J blocking (skk_mode)", () => {
    it("should block Ctrl+J when skk_mode is true (default)", () => {
      // Mock SettingsService.getCached to return skk_mode: true
      const { SettingsService } = require("../../settings/settings-service");
      SettingsService.getCached = () => ({ skk_mode: true, keybinds: {} });

      const ptyClient = createMockPtyClient();
      const context = createTestContext({
        ptyClient,
        getState: () =>
          ({
            getModes: () => ({ cursorKeys: "normal" }),
          }) as unknown as TerminalState,
      });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("j", { ctrlKey: true });
      handler.handleKeyDown(event);

      // Ctrl+J should be blocked - write not called
      expect(ptyClient.write).not.toHaveBeenCalled();
      expect(event.preventDefault).not.toHaveBeenCalled();
    });

    it("should block Ctrl+J when skk_mode is not explicitly set (null settings)", () => {
      const { SettingsService } = require("../../settings/settings-service");
      SettingsService.getCached = () => null;

      const ptyClient = createMockPtyClient();
      const context = createTestContext({
        ptyClient,
        getState: () =>
          ({
            getModes: () => ({ cursorKeys: "normal" }),
          }) as unknown as TerminalState,
      });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("j", { ctrlKey: true });
      handler.handleKeyDown(event);

      // Ctrl+J should be blocked - skk_mode defaults to blocking
      expect(ptyClient.write).not.toHaveBeenCalled();
    });

    it("should allow Ctrl+J through when skk_mode is false", () => {
      const { SettingsService } = require("../../settings/settings-service");
      SettingsService.getCached = () => ({ skk_mode: false, keybinds: {} });

      const ptyClient = createMockPtyClient();
      const context = createTestContext({
        ptyClient,
        getState: () =>
          ({
            getModes: () => ({ cursorKeys: "normal" }),
          }) as unknown as TerminalState,
      });
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("j", { ctrlKey: true });
      handler.handleKeyDown(event);

      // Ctrl+J should pass through - produces 0x0A (LF)
      expect(ptyClient.write).toHaveBeenCalled();
      expect(event.preventDefault).toHaveBeenCalled();
    });
  });

  describe("isSpecialKey", () => {
    it("should return true for Ctrl combinations", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("a", { ctrlKey: true });
      expect(handler.isSpecialKey(event)).toBe(true);
    });

    it("should return true for Alt combinations", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("a", { altKey: true });
      expect(handler.isSpecialKey(event)).toBe(true);
    });

    it("should return true for Meta combinations", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      const event = createKeyEvent("a", { metaKey: true });
      expect(handler.isSpecialKey(event)).toBe(true);
    });

    it("should return true for arrow keys", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      expect(handler.isSpecialKey(createKeyEvent("ArrowUp"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("ArrowDown"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("ArrowLeft"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("ArrowRight"))).toBe(true);
    });

    it("should return true for navigation keys", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      expect(handler.isSpecialKey(createKeyEvent("Home"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("End"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("PageUp"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("PageDown"))).toBe(true);
    });

    it("should return true for function keys", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      expect(handler.isSpecialKey(createKeyEvent("F1"))).toBe(true);
      expect(handler.isSpecialKey(createKeyEvent("F12"))).toBe(true);
    });

    it("should return false for regular characters without modifiers", () => {
      const context = createTestContext();
      const handler = new KeyboardHandler(context);

      expect(handler.isSpecialKey(createKeyEvent("a"))).toBe(false);
      expect(handler.isSpecialKey(createKeyEvent("1"))).toBe(false);
    });
  });
});
