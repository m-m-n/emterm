/**
 * Notification Manager
 *
 * Handles desktop notification dispatch with window focus tracking and throttling.
 * Uses tauri-plugin-notification for OS native notifications.
 */

import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { ActivityType } from "../tab-bar/types";
import { SettingsService } from "../settings/settings-service";
import { t } from "../i18n/index.ts";

export class NotificationManager {
  private isWindowActive = true;
  private notificationThrottleTimers: Map<
    string,
    ReturnType<typeof setTimeout>
  > = new Map();
  private permissionGranted: boolean | null = null;
  private handleFocus: () => void;
  private handleBlur: () => void;

  /** Notification throttle window in milliseconds */
  static readonly NOTIFICATION_THROTTLE_MS = 5000;

  constructor() {
    // Track window focus state via blur/focus events
    this.handleFocus = () => {
      this.isWindowActive = true;
    };
    this.handleBlur = () => {
      this.isWindowActive = false;
    };
    window.addEventListener("focus", this.handleFocus);
    window.addEventListener("blur", this.handleBlur);

    // Check initial permission state
    this.checkPermission();
  }

  /**
   * Whether the window is currently active/focused
   */
  get windowActive(): boolean {
    return this.isWindowActive;
  }

  /**
   * Send a desktop notification (respects throttle and settings)
   */
  notify(tabId: string, tabTitle: string, activityType: ActivityType): void {
    // Check notification_enabled setting
    const settings = SettingsService.getCached();
    if (settings && !settings.notification_enabled) return;

    // Don't notify when window is active
    if (this.isWindowActive) return;

    // Apply per-tab throttle (process_exit bypasses)
    if (activityType !== "process_exit") {
      if (this.notificationThrottleTimers.has(tabId)) return;
    }

    // Clear existing timer before setting new one to prevent timer leak
    const existingTimer = this.notificationThrottleTimers.get(tabId);
    if (existingTimer) {
      clearTimeout(existingTimer);
    }

    // Set throttle timer
    this.notificationThrottleTimers.set(
      tabId,
      setTimeout(() => {
        this.notificationThrottleTimers.delete(tabId);
      }, NotificationManager.NOTIFICATION_THROTTLE_MS),
    );

    // Send notification
    this.sendDesktopNotification(tabTitle, activityType);
  }

  /**
   * Clear throttle timer for a tab (called on tab close)
   */
  clearThrottle(tabId: string): void {
    const timer = this.notificationThrottleTimers.get(tabId);
    if (timer) {
      clearTimeout(timer);
      this.notificationThrottleTimers.delete(tabId);
    }
  }

  /**
   * Dispose all resources
   */
  dispose(): void {
    window.removeEventListener("focus", this.handleFocus);
    window.removeEventListener("blur", this.handleBlur);

    for (const timer of this.notificationThrottleTimers.values()) {
      clearTimeout(timer);
    }
    this.notificationThrottleTimers.clear();
  }

  /**
   * Check and request notification permission
   */
  private async checkPermission(): Promise<void> {
    try {
      this.permissionGranted = await isPermissionGranted();
      if (!this.permissionGranted) {
        const permission = await requestPermission();
        this.permissionGranted = permission === "granted";
        if (!this.permissionGranted) {
          console.warn("Notification permission denied");
        }
      }
    } catch (error) {
      console.warn("Failed to check notification permission:", error);
      this.permissionGranted = false;
    }
  }

  /**
   * Send the actual desktop notification
   */
  private sendDesktopNotification(
    tabTitle: string,
    activityType: ActivityType,
  ): void {
    if (this.permissionGranted === false) return;

    // Sanitize tab title: strip ANSI escape sequences, then remaining control chars, then truncate
    const sanitizedTitle = tabTitle
      .replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
      .replace(/[\x00-\x1f\x7f-\x9f]/g, "")
      .slice(0, 100);

    const body = this.formatNotificationBody(sanitizedTitle, activityType);

    try {
      sendNotification({ title: "eMterm", body });
    } catch (error) {
      console.warn("Failed to send notification:", error);
    }
  }

  /**
   * Format notification body based on activity type
   */
  private formatNotificationBody(
    tabTitle: string,
    activityType: ActivityType,
  ): string {
    switch (activityType) {
      case "process_exit":
        return `${tabTitle}: ${t("settings.notification.body.processExit")}`;
      case "output":
        return `${tabTitle}: ${t("settings.notification.body.newOutput")}`;
      case "bell":
        return `${tabTitle}: ${t("settings.notification.body.bell")}`;
    }
  }
}
