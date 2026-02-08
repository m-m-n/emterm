/**
 * Tab Activity Tracker
 *
 * Monitors inactive tab events and manages per-tab activity state.
 * Emits callbacks for UI dot indicator and desktop notification.
 */

import type { TabManager } from "./tab-manager";
import type { ActivityType, UnsubscribeFn } from "./types";
import { isTerminalTab } from "./types";
import { SettingsService } from "../settings/settings-service";

interface TabActivityState {
  hasActivity: boolean;
  lastActivityTime: number;
}

type ActivityCallback = (tabId: string, type: ActivityType) => void;
type ClearCallback = (tabId: string) => void;

export class TabActivityTracker {
  private activityStates: Map<string, TabActivityState> = new Map();
  private outputThrottleTimers: Map<string, ReturnType<typeof setTimeout>> =
    new Map();
  private activityCallbacks: ActivityCallback[] = [];
  private clearCallbacks: ClearCallback[] = [];
  private unsubscribes: UnsubscribeFn[] = [];

  /** Output throttle window in milliseconds */
  static readonly OUTPUT_THROTTLE_MS = 1000;

  constructor(private tabManager: TabManager) {
    // Subscribe to tab lifecycle events
    this.unsubscribes.push(
      tabManager.on("tab:activated", ({ tab }) => {
        this.clearActivity(tab.id);
      }),
    );

    this.unsubscribes.push(
      tabManager.on("tab:closed", ({ tabId }) => {
        this.cleanupTab(tabId);
      }),
    );
  }

  /**
   * Check if a tab has unread activity
   */
  hasActivity(tabId: string): boolean {
    return this.activityStates.get(tabId)?.hasActivity ?? false;
  }

  /**
   * Mark activity on a tab. Ignores active tab and respects settings.
   */
  markActivity(tabId: string, type: ActivityType): void {
    // Don't mark activity on the active tab
    const activeTab = this.tabManager.getActiveTab();
    if (activeTab && activeTab.id === tabId) return;

    // Don't mark activity on settings tabs
    const tab = this.tabManager.getTab(tabId);
    if (!tab || !isTerminalTab(tab)) return;

    // Check settings for this trigger type
    const settings = SettingsService.getCached();
    if (settings) {
      if (type === "process_exit" && !settings.notify_on_process_exit) return;
      if (type === "output" && !settings.notify_on_output) return;
      if (type === "bell" && !settings.notify_on_bell) return;
    }

    // For output events, apply throttle (max 1/sec per tab)
    if (type === "output") {
      if (this.outputThrottleTimers.has(tabId)) return;
      this.outputThrottleTimers.set(
        tabId,
        setTimeout(() => {
          this.outputThrottleTimers.delete(tabId);
        }, TabActivityTracker.OUTPUT_THROTTLE_MS),
      );
    }

    // Set activity state
    this.activityStates.set(tabId, {
      hasActivity: true,
      lastActivityTime: Date.now(),
    });

    // Invoke callbacks
    for (const cb of this.activityCallbacks) {
      cb(tabId, type);
    }
  }

  /**
   * Clear activity for a tab (called on tab activation)
   */
  clearActivity(tabId: string): void {
    const state = this.activityStates.get(tabId);
    if (!state?.hasActivity) return;

    this.activityStates.set(tabId, {
      hasActivity: false,
      lastActivityTime: state.lastActivityTime,
    });

    for (const cb of this.clearCallbacks) {
      cb(tabId);
    }
  }

  /**
   * Register activity callback (dot indicator + notification)
   */
  onActivity(callback: ActivityCallback): UnsubscribeFn {
    this.activityCallbacks.push(callback);
    return () => {
      const idx = this.activityCallbacks.indexOf(callback);
      if (idx >= 0) this.activityCallbacks.splice(idx, 1);
    };
  }

  /**
   * Register clear callback (hide dot indicator)
   */
  onClear(callback: ClearCallback): UnsubscribeFn {
    this.clearCallbacks.push(callback);
    return () => {
      const idx = this.clearCallbacks.indexOf(callback);
      if (idx >= 0) this.clearCallbacks.splice(idx, 1);
    };
  }

  /**
   * Cleanup state and timers for a closed tab
   */
  private cleanupTab(tabId: string): void {
    this.activityStates.delete(tabId);
    const timer = this.outputThrottleTimers.get(tabId);
    if (timer) {
      clearTimeout(timer);
      this.outputThrottleTimers.delete(tabId);
    }
  }

  /**
   * Dispose all resources
   */
  dispose(): void {
    for (const unsub of this.unsubscribes) {
      unsub();
    }
    this.unsubscribes = [];
    this.activityCallbacks = [];
    this.clearCallbacks = [];

    // Clear all throttle timers
    for (const timer of this.outputThrottleTimers.values()) {
      clearTimeout(timer);
    }
    this.outputThrottleTimers.clear();
    this.activityStates.clear();
  }
}
