/**
 * TabActivityTracker Unit Tests
 *
 * Covers: TS-01, TS-02, TS-03, TS-04, TS-05, TS-12, TS-13, TS-14
 */

import { describe, test, expect, beforeEach, afterEach, mock } from "bun:test";

// Mock settings service before import
let mockCachedSettings: Record<string, unknown> | null = {
  notification_enabled: true,
  tab_activity_indicator: true,
  notify_on_process_exit: true,
  notify_on_output: true,
  notify_on_bell: true,
};

mock.module("../settings/settings-service", () => ({
  SettingsService: {
    getCached: () => mockCachedSettings,
  },
}));

mock.module("@tauri-apps/api/core", () => ({
  invoke: mock(async () => null),
}));

import { TabActivityTracker } from "./tab-activity-tracker";
import type { Tab, TabEventType, TabEventHandler, UnsubscribeFn } from "./types";

/**
 * Minimal mock TabManager for testing
 */
function createMockTabManager() {
  let activeTabId: string | null = "tab-1";
  const tabs = new Map<string, Tab>();
  const handlers = new Map<string, TabEventHandler<TabEventType>[]>();

  // Default tabs
  tabs.set("tab-1", { id: "tab-1", type: "terminal", title: "Tab 1", sessionId: "s1" } as Tab);
  tabs.set("tab-2", { id: "tab-2", type: "terminal", title: "Tab 2", sessionId: "s2" } as Tab);
  tabs.set("tab-3", { id: "tab-3", type: "terminal", title: "Tab 3", sessionId: "s3" } as Tab);

  return {
    getActiveTab: () => {
      if (!activeTabId) return null;
      return tabs.get(activeTabId) ?? null;
    },
    getTab: (id: string) => tabs.get(id) ?? null,
    on: (event: string, handler: TabEventHandler<TabEventType>): UnsubscribeFn => {
      const list = handlers.get(event) ?? [];
      list.push(handler);
      handlers.set(event, list);
      return () => {
        const idx = list.indexOf(handler);
        if (idx >= 0) list.splice(idx, 1);
      };
    },
    // Test helpers
    _setActiveTabId: (id: string | null) => { activeTabId = id; },
    _addTab: (tab: Tab) => { tabs.set(tab.id, tab); },
    _removeTab: (id: string) => { tabs.delete(id); },
    _emit: (event: string, payload: unknown) => {
      const list = handlers.get(event) ?? [];
      for (const h of list) {
        (h as (p: unknown) => void)(payload);
      }
    },
  };
}

describe("TabActivityTracker", () => {
  let tracker: TabActivityTracker;
  let mockManager: ReturnType<typeof createMockTabManager>;

  beforeEach(() => {
    mockCachedSettings = {
      notification_enabled: true,
      tab_activity_indicator: true,
      notify_on_process_exit: true,
      notify_on_output: true,
      notify_on_bell: true,
    };
    mockManager = createMockTabManager();
    tracker = new TabActivityTracker(mockManager as unknown as import("./tab-manager").TabManager);
  });

  afterEach(() => {
    tracker.dispose();
  });

  // TS-01: markActivity() sets flag for correct tab
  describe("TS-01: markActivity sets flag", () => {
    test("hasActivity returns true for marked tab only", () => {
      // tab-2 is inactive (active is tab-1)
      tracker.markActivity("tab-2", "bell");

      expect(tracker.hasActivity("tab-2")).toBe(true);
      expect(tracker.hasActivity("tab-1")).toBe(false);
      expect(tracker.hasActivity("tab-3")).toBe(false);
    });

    test("invokes activity callbacks with correct arguments", () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "process_exit");

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("tab-2", "process_exit");
    });
  });

  // TS-02: clearActivity() resets flag
  describe("TS-02: clearActivity resets flag", () => {
    test("hasActivity returns false after clear", () => {
      tracker.markActivity("tab-2", "bell");
      expect(tracker.hasActivity("tab-2")).toBe(true);

      tracker.clearActivity("tab-2");
      expect(tracker.hasActivity("tab-2")).toBe(false);
    });

    test("invokes clear callbacks", () => {
      const callback = mock(() => {});
      tracker.onClear(callback);

      tracker.markActivity("tab-2", "bell");
      tracker.clearActivity("tab-2");

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith("tab-2");
    });

    test("does not invoke clear callback if no activity", () => {
      const callback = mock(() => {});
      tracker.onClear(callback);

      tracker.clearActivity("tab-2");

      expect(callback).toHaveBeenCalledTimes(0);
    });

    test("clears activity on tab activation event", () => {
      tracker.markActivity("tab-2", "bell");
      expect(tracker.hasActivity("tab-2")).toBe(true);

      // Simulate tab activation event
      mockManager._emit("tab:activated", { tab: { id: "tab-2" } });

      expect(tracker.hasActivity("tab-2")).toBe(false);
    });
  });

  // TS-03: does not mark activity on active tab
  describe("TS-03: active tab ignored", () => {
    test("active tab's hasActivity remains false", () => {
      // tab-1 is the active tab
      tracker.markActivity("tab-1", "bell");

      expect(tracker.hasActivity("tab-1")).toBe(false);
    });

    test("callback not invoked for active tab", () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-1", "process_exit");

      expect(callback).toHaveBeenCalledTimes(0);
    });

    test("ignores settings tabs", () => {
      mockManager._addTab({ id: "settings-1", type: "settings", title: "Settings" } as Tab);
      mockManager._setActiveTabId("tab-1");

      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("settings-1", "bell");

      expect(callback).toHaveBeenCalledTimes(0);
      expect(tracker.hasActivity("settings-1")).toBe(false);
    });
  });

  // TS-04: output throttling
  describe("TS-04: output throttle 1/sec per tab", () => {
    test("second markActivity(output) within 1s is suppressed", () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "output");
      tracker.markActivity("tab-2", "output");

      expect(callback).toHaveBeenCalledTimes(1);
    });

    test("non-output events are not throttled", () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "bell");
      tracker.markActivity("tab-2", "bell");

      expect(callback).toHaveBeenCalledTimes(2);
    });

    test("different tabs throttle independently", () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "output");
      tracker.markActivity("tab-3", "output");

      expect(callback).toHaveBeenCalledTimes(2);
    });

    test("output throttle resets after timeout", async () => {
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "output");
      expect(callback).toHaveBeenCalledTimes(1);

      // Wait for throttle to expire
      await new Promise((r) => setTimeout(r, TabActivityTracker.OUTPUT_THROTTLE_MS + 50));

      tracker.markActivity("tab-2", "output");
      expect(callback).toHaveBeenCalledTimes(2);
    });
  });

  // TS-05: settings flags enable/disable triggers
  describe("TS-05: settings flags", () => {
    test("disabled process_exit trigger does not invoke callback", () => {
      mockCachedSettings = {
        ...mockCachedSettings,
        notify_on_process_exit: false,
      };
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "process_exit");

      expect(callback).toHaveBeenCalledTimes(0);
    });

    test("disabled output trigger does not invoke callback", () => {
      mockCachedSettings = {
        ...mockCachedSettings,
        notify_on_output: false,
      };
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "output");

      expect(callback).toHaveBeenCalledTimes(0);
    });

    test("disabled bell trigger does not invoke callback", () => {
      mockCachedSettings = {
        ...mockCachedSettings,
        notify_on_bell: false,
      };
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "bell");

      expect(callback).toHaveBeenCalledTimes(0);
    });

    test("null settings allows all triggers (defaults)", () => {
      mockCachedSettings = null;
      const callback = mock(() => {});
      tracker.onActivity(callback);

      tracker.markActivity("tab-2", "bell");
      tracker.markActivity("tab-2", "process_exit");

      expect(callback).toHaveBeenCalledTimes(2);
    });
  });

  // TS-12: rapid tab switching during activity
  describe("TS-12: rapid tab switching", () => {
    test("no stale indicators after switch", () => {
      const clearCb = mock(() => {});
      tracker.onClear(clearCb);

      // Mark activity on tab-2
      tracker.markActivity("tab-2", "bell");
      expect(tracker.hasActivity("tab-2")).toBe(true);

      // Switch to tab-2 (clears)
      mockManager._setActiveTabId("tab-2");
      mockManager._emit("tab:activated", { tab: { id: "tab-2" } });
      expect(tracker.hasActivity("tab-2")).toBe(false);

      // Quickly switch to tab-3
      mockManager._setActiveTabId("tab-3");
      mockManager._emit("tab:activated", { tab: { id: "tab-3" } });

      // Mark activity on tab-2 again (now inactive)
      tracker.markActivity("tab-2", "output");
      expect(tracker.hasActivity("tab-2")).toBe(true);

      // Switch back to tab-2
      mockManager._setActiveTabId("tab-2");
      mockManager._emit("tab:activated", { tab: { id: "tab-2" } });
      expect(tracker.hasActivity("tab-2")).toBe(false);

      expect(clearCb).toHaveBeenCalledTimes(2);
    });
  });

  // TS-13: multiple tabs receiving activity simultaneously
  describe("TS-13: multiple tabs simultaneous activity", () => {
    test("each tab shows independent indicator", () => {
      tracker.markActivity("tab-2", "bell");
      tracker.markActivity("tab-3", "process_exit");

      expect(tracker.hasActivity("tab-2")).toBe(true);
      expect(tracker.hasActivity("tab-3")).toBe(true);

      // Clear one
      tracker.clearActivity("tab-2");
      expect(tracker.hasActivity("tab-2")).toBe(false);
      expect(tracker.hasActivity("tab-3")).toBe(true);
    });
  });

  // TS-14: tab closed while having activity indicator
  describe("TS-14: tab closed with activity", () => {
    test("no errors, state cleaned up", () => {
      tracker.markActivity("tab-2", "bell");
      expect(tracker.hasActivity("tab-2")).toBe(true);

      // Simulate tab close event
      mockManager._emit("tab:closed", { tabId: "tab-2", wasActive: false });

      // State should be cleaned up
      expect(tracker.hasActivity("tab-2")).toBe(false);
    });

    test("output throttle timer cleaned up on tab close", () => {
      tracker.markActivity("tab-2", "output");

      // Close tab (should clean up throttle timer)
      mockManager._emit("tab:closed", { tabId: "tab-2", wasActive: false });

      // No errors and state is clean
      expect(tracker.hasActivity("tab-2")).toBe(false);
    });
  });

  // Additional: unsubscribe
  describe("unsubscribe", () => {
    test("onActivity unsubscribe stops callback", () => {
      const callback = mock(() => {});
      const unsub = tracker.onActivity(callback);

      tracker.markActivity("tab-2", "bell");
      expect(callback).toHaveBeenCalledTimes(1);

      unsub();
      tracker.markActivity("tab-2", "process_exit");
      expect(callback).toHaveBeenCalledTimes(1);
    });

    test("onClear unsubscribe stops callback", () => {
      const callback = mock(() => {});
      const unsub = tracker.onClear(callback);

      tracker.markActivity("tab-2", "bell");
      tracker.clearActivity("tab-2");
      expect(callback).toHaveBeenCalledTimes(1);

      unsub();
      tracker.markActivity("tab-2", "process_exit");
      tracker.clearActivity("tab-2");
      expect(callback).toHaveBeenCalledTimes(1);
    });
  });

  // dispose
  describe("dispose", () => {
    test("cleans up all state", () => {
      tracker.markActivity("tab-2", "bell");
      tracker.markActivity("tab-3", "output");

      tracker.dispose();

      expect(tracker.hasActivity("tab-2")).toBe(false);
      expect(tracker.hasActivity("tab-3")).toBe(false);
    });
  });
});
