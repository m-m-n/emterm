/**
 * NotificationManager Unit Tests
 *
 * Covers: TS-06, TS-07, TS-08
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Mock settings
let mockCachedSettings: Record<string, unknown> | null = {
  notification_enabled: true,
};

mock.module("../settings/settings-service", () => ({
  SettingsService: {
    getCached: () => mockCachedSettings,
  },
}));

// Mock i18n - return key as-is for predictable test assertions
mock.module("../i18n/index.ts", () => ({
  t: (key: string) => {
    const map: Record<string, string> = {
      "settings.notification.body.processExit": "Process exited",
      "settings.notification.body.newOutput": "New output",
      "settings.notification.body.bell": "Bell",
    };
    return map[key] ?? key;
  },
}));

// Mock notification plugin
const mockSendNotification = mock(() => {});
const mockIsPermissionGranted = mock(async () => true);
const mockRequestPermission = mock(async () => "granted" as const);

mock.module("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: mockIsPermissionGranted,
  requestPermission: mockRequestPermission,
  sendNotification: mockSendNotification,
}));

mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async () => null),
}));

import { NotificationManager } from "./notification-manager";

describe("NotificationManager", () => {
  let manager: NotificationManager;

  beforeEach(() => {
    mockCachedSettings = { notification_enabled: true };
    mockSendNotification.mockClear();
    mockIsPermissionGranted.mockClear();
    mockRequestPermission.mockClear();
    manager = new NotificationManager();
  });

  afterEach(() => {
    manager.dispose();
  });

  // TS-06: respects window focus state
  describe("TS-06: window focus state", () => {
    test("no notification when window is active (default)", () => {
      // Window starts as active by default
      manager.notify("tab-1", "Test Tab", "bell");

      expect(mockSendNotification).toHaveBeenCalledTimes(0);
    });

    test("notification sent when window is inactive", async () => {
      // Wait for permission check to complete
      await new Promise((r) => setTimeout(r, 10));

      // Simulate window blur
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Test Tab", "bell");

      expect(mockSendNotification).toHaveBeenCalledTimes(1);
    });

    test("windowActive getter reflects focus state", () => {
      expect(manager.windowActive).toBe(true);

      window.dispatchEvent(new Event("blur"));
      expect(manager.windowActive).toBe(false);

      window.dispatchEvent(new Event("focus"));
      expect(manager.windowActive).toBe(true);
    });

    test("no notification when notification_enabled is false", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      mockCachedSettings = { notification_enabled: false };
      manager.notify("tab-1", "Test Tab", "bell");

      expect(mockSendNotification).toHaveBeenCalledTimes(0);
    });
  });

  // TS-07: notification throttle
  describe("TS-07: notification throttle 1/5sec per tab", () => {
    test("second notify within 5s is suppressed", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Test Tab", "bell");
      manager.notify("tab-1", "Test Tab", "output");

      expect(mockSendNotification).toHaveBeenCalledTimes(1);
    });

    test("different tabs throttle independently", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Test Tab 1", "bell");
      manager.notify("tab-2", "Test Tab 2", "bell");

      expect(mockSendNotification).toHaveBeenCalledTimes(2);
    });
  });

  // TS-08: process_exit bypasses throttle
  describe("TS-08: process_exit bypasses throttle", () => {
    test("notification sent even within throttle window", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      // First notification sets throttle
      manager.notify("tab-1", "Test Tab", "bell");

      // process_exit bypasses throttle
      manager.notify("tab-1", "Test Tab", "process_exit");

      expect(mockSendNotification).toHaveBeenCalledTimes(2);
    });

    test("process_exit resets throttle timer (no timer leak)", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      // First notification sets throttle
      manager.notify("tab-1", "Test Tab", "bell");

      // process_exit bypasses and resets throttle timer
      manager.notify("tab-1", "Test Tab", "process_exit");

      // A third non-bypass notification should still be throttled
      manager.notify("tab-1", "Test Tab", "output");

      expect(mockSendNotification).toHaveBeenCalledTimes(2);
    });
  });

  // clearThrottle
  describe("clearThrottle", () => {
    test("clears throttle for a tab", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Test Tab", "bell");
      expect(mockSendNotification).toHaveBeenCalledTimes(1);

      // Clear throttle
      manager.clearThrottle("tab-1");

      // Should be able to notify again
      manager.notify("tab-1", "Test Tab", "output");
      expect(mockSendNotification).toHaveBeenCalledTimes(2);
    });
  });

  // notification content format
  describe("notification content format", () => {
    test("sends notification with correct title and body", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "My Shell", "process_exit");

      expect(mockSendNotification).toHaveBeenCalledWith({
        title: "eMterm",
        body: "My Shell: Process exited",
      });
    });

    test("sanitizes ANSI sequences and control characters in tab title", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Tab\x1b[31m\x00Title", "bell");

      expect(mockSendNotification).toHaveBeenCalledWith({
        title: "eMterm",
        body: "TabTitle: Bell",
      });
    });

    test("sanitizes complex ANSI sequences", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Hello\x1b[1;32;48;5;123mWorld\x1b[0m", "bell");

      expect(mockSendNotification).toHaveBeenCalledWith({
        title: "eMterm",
        body: "HelloWorld: Bell",
      });
    });
  });

  // dispose
  describe("dispose", () => {
    test("cleans up event listeners and timers", async () => {
      await new Promise((r) => setTimeout(r, 10));
      window.dispatchEvent(new Event("blur"));

      manager.notify("tab-1", "Test Tab", "bell");
      manager.dispose();

      // After dispose, focus/blur should not affect state
      // (listeners removed - but we verify no errors)
      expect(() => {
        window.dispatchEvent(new Event("focus"));
        window.dispatchEvent(new Event("blur"));
      }).not.toThrow();
    });
  });
});
