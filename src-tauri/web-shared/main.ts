/**
 * eMterm - Terminal Emulator
 * Entry point
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { TerminalApp } from "./terminal-app";
import {
  TabManager,
  TabBarUI,
  TabKeyboardHandler,
  TabDragHandler,
} from "./tab-bar";
import { TabActivityTracker } from "./tab-bar/tab-activity-tracker";
import { NotificationManager } from "./notification/notification-manager";
import { initConsoleBridge } from "./utils/console-bridge";
import { SettingsService, applySettingsToCSS } from "./settings";
import { initI18n, resolveLocale, t } from "./i18n/index.ts";
import { isTerminalTab } from "./tab-bar/types";
import { initWasm } from "./terminal/wasm/loader.ts";
import { initPlatform } from "./platform";
import { StatusBarUI } from "./status-bar";
import { OscLayerController } from "./status-bar/osc-controller";

let tabManager: TabManager | null = null;
let tabBarUI: TabBarUI | null = null;
let keyboardHandler: TabKeyboardHandler | null = null;
let dragHandler: TabDragHandler | null = null;
let activityTracker: TabActivityTracker | null = null;
let notificationManager: NotificationManager | null = null;
let statusBarUI: StatusBarUI | null = null;
let oscLayerController: OscLayerController | null = null;

/**
 * Initialize the terminal application with tab support
 */
async function main(): Promise<void> {
  // Initialize console bridge to forward logs to stdout/stderr
  initConsoleBridge();

  // Hold a perpetual Web Lock so WebKitGTK doesn't aggressively suspend
  // rAF/setTimeout when the window loses focus. WebKitGTK has no native
  // background-throttling toggle (Tauri issue #5250 marks Linux as
  // unsupported), and a held lock is the documented userland workaround.
  // The lock callback returns a never-resolving Promise so the hold lasts
  // for the page lifetime.
  if (typeof navigator !== "undefined" && navigator.locks) {
    navigator.locks.request(
      "emterm-keepalive",
      { mode: "exclusive" },
      () => new Promise<void>(() => {}),
    ).catch((err) => {
      console.warn("[WARN][FRONTEND] Web Lock keepalive request failed:", err);
    });
  }

  // Suppress browser default context menu globally.
  // Custom context menus are shown by specific handlers on terminal, tab, and tab bar areas.
  document.addEventListener("contextmenu", (e) => e.preventDefault());

  // Initialize WASM module before any terminal processing (fail-fast on error)
  await initWasm();

  // Resolve and cache the platform identifier before any selection / paste /
  // settings UI code runs. This lets isLinux() / isWindows() be consulted
  // synchronously from hot paths.
  await initPlatform();

  // Load and apply settings at startup
  try {
    const settings = await SettingsService.load();
    applySettingsToCSS(settings);

    // Initialize i18n with resolved locale
    const resolvedLocale = resolveLocale(settings.language ?? "auto");
    initI18n(resolvedLocale);

    // Sync backend locale (fire-and-forget)
    invoke("set_language", { language: resolvedLocale }).catch((err) => {
      console.warn("Failed to sync backend language:", err);
    });

    // Auto-detect SSH command path on startup if empty (fire-and-forget)
    if (!settings.ssh_command_path) {
      invoke<string>("detect_ssh_command").then(async (detected) => {
        if (detected) {
          try {
            const current = SettingsService.getCached() ?? await SettingsService.load();
            if (!current.ssh_command_path) {
              await SettingsService.save({ ...current, ssh_command_path: detected });
            }
          } catch (e) {
            console.warn("Failed to save detected SSH path:", e);
          }
        }
      }).catch((err) => {
        console.warn("SSH auto-detection failed:", err);
      });
    }
  } catch (error) {
    console.error("Failed to load settings at startup:", error);
    // Continue with defaults - initialize i18n with auto-detected locale
    const resolvedLocale = resolveLocale("auto");
    initI18n(resolvedLocale);
  }

  const tabBarContainer = document.getElementById("tab-bar");
  const contentContainer = document.getElementById("tab-content-area");

  if (!tabBarContainer || !contentContainer) {
    console.error("Tab bar or content container not found");
    // Fallback to legacy single terminal mode
    return initLegacyMode();
  }

  // Cache for mux status bar content (per-tab) for synchronous restoration on tab switch
  const statusBarCache = new Map<string, { left: string; right: string }>();

  // Create TabManager
  // Use a temporary variable to allow callback closure to reference it
  const manager = new TabManager({
    container: contentContainer,
    createTerminalApp: async (tabContainer, spawnOptions) => {
      const app = new TerminalApp(tabContainer, {
        spawnOverrides: spawnOptions,
        sshConnectionName: spawnOptions?.sshConnectionName,
      });
      await app.init();

      // Wire status bar OSC callback
      if (oscLayerController) {
        app.statusBarOscCallback = (command, param1, param2) => {
          const activeTab = manager.getActiveTab();
          if (activeTab && manager.getTerminalApp(activeTab.id) !== app) return;
          oscLayerController?.handleCommand(command, param1, param2);
        };
      }

      // Wire mux status update callback to OSC layer
      app.muxStatusUpdateCallback = (msg) => {
        const activeTab = manager.getActiveTab();
        if (activeTab && manager.getTerminalApp(activeTab.id) !== app) return;
        // Cache status bar content for synchronous restoration on tab switch
        const tabForApp = manager.getTabs().find(
          (t) => manager.getTerminalApp(t.id) === app,
        );
        if (msg.left === "" && msg.right === "") {
          if (tabForApp) statusBarCache.delete(tabForApp.id);
          oscLayerController?.handleCommand("clear");
        } else {
          if (tabForApp) statusBarCache.set(tabForApp.id, { left: msg.left, right: msg.right });
          oscLayerController?.handleCommand("set", "left", msg.left);
          oscLayerController?.handleCommand("set", "right", msg.right);
        }
        // Status bar height may have changed — recheck terminal size after layout reflow
        requestAnimationFrame(() => {
          app.recheckSize();
        });
      };

      // Connect PTY exit event to TabManager
      const sessionId = app.pty?.getSessionId();
      if (sessionId) {
        app.onSessionExit((exitedSessionId) => {
          // Notify activity tracker before tab closes
          const exitTab = manager.getTabs().find(
            (t) => isTerminalTab(t) && t.sessionId === exitedSessionId,
          );
          if (exitTab) {
            activityTracker?.markActivity(exitTab.id, "process_exit");
          }
          manager.handleSessionExit(exitedSessionId);
        });
      }

      return app;
    },
  });
  tabManager = manager;

  // Register before-close guard for active SFTP uploads
  manager.addBeforeCloseGuard(async (tabId: string) => {
    const app = manager.getTerminalApp(tabId);
    if (!app?.uploadManager?.hasActiveUploads()) return true;

    const message = t("sftp.tabClose.message");
    if (!confirm(message)) return false;

    // User confirmed - cancel all uploads before closing
    await app.uploadManager.cancelAllUploads();
    return true;
  });

  // Handle last tab closed - exit application
  tabManager.onLastTabClosed(async () => {
    try {
      const appWindow = getCurrentWebviewWindow();
      await appWindow.close();
    } catch (error) {
      console.error("Failed to close window:", error);
    }
  });

  // Create TabBarUI
  tabBarUI = new TabBarUI({
    container: tabBarContainer,
    tabManager,
  });
  tabBarUI.init();

  // Initialize Status Bar
  const statusBarContainer = document.getElementById("status-bar");
  if (statusBarContainer) {
    statusBarUI = new StatusBarUI(statusBarContainer);
    statusBarUI.init();

    // Wire OSC layer controller
    const renderer = statusBarUI.getRenderer();
    if (renderer) {
      oscLayerController = new OscLayerController(renderer);
    }

    // Wire CWD source from active tab's terminal state
    statusBarUI.setCwdSource(() => {
      const activeTab = manager.getActiveTab();
      if (!activeTab) return "";
      const app = manager.getTerminalApp(activeTab.id);
      if (!app) return "";
      try {
        return app.terminalState.workingDirectory || "";
      } catch {
        return "";
      }
    });

    // Wire command executor via Tauri backend
    statusBarUI.setCommandExecutor(async (cmd: string, args: string[], cwd: string) => {
      return await invoke<string>("run_statusbar_shell_command", {
        program: cmd,
        args,
        cwd,
      });
    });

    // Listen for settings changes to update status bar
    window.addEventListener("emterm-statusbar-settings", ((e: CustomEvent) => {
      statusBarUI?.applySettings(e.detail);
      // Status bar visibility/height may have changed — recheck terminal sizes
      requestAnimationFrame(() => {
        const activeTab = manager.getActiveTab();
        if (activeTab) {
          manager.getTerminalApp(activeTab.id)?.recheckSize();
        }
      });
    }) as EventListener);
  }

  // Apply initial tab bar visibility from settings
  try {
    const settings = await SettingsService.load();
    if (settings.show_tab_bar === false) {
      tabBarUI.setVisible(false);
    }
    // Apply initial status bar settings
    if (statusBarUI) {
      statusBarUI.applySettings(settings);
    }
  } catch (error) {
    console.warn("Failed to apply initial tab bar visibility:", error);
  }

  // Create keyboard handler with toggle callback
  const tabBarUIRef = tabBarUI;
  keyboardHandler = new TabKeyboardHandler(tabManager, {
    tabBarUI: tabBarUIRef,
    onToggleTabBar: async () => {
      const newVisible = !tabBarUIRef.isVisible();
      tabBarUIRef.setVisible(newVisible);
      // Save to settings
      try {
        const currentSettings = SettingsService.getCached();
        if (currentSettings) {
          await SettingsService.save({
            ...currentSettings,
            show_tab_bar: newVisible,
          });
        }
      } catch (error) {
        console.warn("Failed to save tab bar visibility:", error);
      }
    },
    onOpenSettings: () => {
      tabBarUIRef.openOrFocusSettingsTab();
    },
  });
  keyboardHandler.attach(document);

  // Listen for profile launch events from settings UI
  document.addEventListener("profile:launch", ((event: CustomEvent) => {
    tabBarUIRef.createTabWithProfile(event.detail);
  }) as EventListener);

  // Create activity tracker and notification manager
  activityTracker = new TabActivityTracker(manager);
  notificationManager = new NotificationManager();

  // Connect activity tracker to notification manager and tab bar UI
  activityTracker.onActivity((tabId, type) => {
    const tab = manager.getTab(tabId);
    if (tab) {
      notificationManager?.notify(tabId, tab.title, type);
      const settings = SettingsService.getCached();
      if (!settings || settings.tab_activity_indicator) {
        tabBarUI?.showActivityDot(tabId);
      }
    }
  });
  activityTracker.onClear((tabId) => {
    tabBarUI?.hideActivityDot(tabId);
  });

  // Wire up activity callbacks when tabs are created
  manager.on("tab:created", ({ tab }) => {
    if (!isTerminalTab(tab)) return;
    const app = manager.getTerminalApp(tab.id);
    if (!app) return;

    app.onBellActivity(() => {
      activityTracker?.markActivity(tab.id, "bell");
    });
    app.onOutputActivity(() => {
      activityTracker?.markActivity(tab.id, "output");
    });

    // Wire mux state change to tab bar sub-tab rendering.
    // While mux mode is active (windowCount >= 1) we always render as a
    // tab group so the `[N] title` badge is visible even for a single
    // window. Only mux-mode-exit (windowCount === 0) falls back to the
    // normal tab/title path.
    app.onMuxStateChange = (info) => {
      if (info.windowCount === 0) {
        // Mux mode exited -- clear sub-tabs, restore title, and clear OSC layer
        tabBarUI?.clearMuxSubTabs(tab.id);
        manager.updateTabTitle(tab.id, "Terminal");
        statusBarCache.delete(tab.id);
        const activeTab = manager.getActiveTab();
        if (activeTab && activeTab.id === tab.id) {
          oscLayerController?.handleCommand("clear");
        }
      } else {
        // In-mux rendering (1+ windows) — always group, always with [N] badge
        const windows = info.windowNames.map((name, i) => ({
          name,
          active: i === info.activeWindow,
        }));
        tabBarUI?.renderMuxSubTabs(tab.id, windows);
      }
    };

    // Wire mux window tab clicks to window switching
    if (tabBarUI) {
      tabBarUI.onMuxWindowClick = (clickedTabId, windowIndex) => {
        const clickedApp = manager.getTerminalApp(clickedTabId);
        if (clickedApp) {
          clickedApp.switchToMuxWindow(windowIndex);
        }
      };
    }
  });

  // Clean up notification throttle when tab closes
  manager.on("tab:closed", ({ tabId }) => {
    notificationManager?.clearThrottle(tabId);
  });

  // Create drag handler for tab reordering
  dragHandler = new TabDragHandler({
    tabManager,
    tabBarUI,
  });
  dragHandler.init();

  // Re-attach drag listeners when tab DOM elements are replaced (mux group transform)
  tabBarUI.onTabElementReplaced = (tabId) => {
    dragHandler?.reattachTabListeners(tabId);
  };

  // Focus the terminal when a tab is activated and update global references.
  //
  // The try/catch wrappers below are intentionally narrow — each guards one
  // WASM-touching operation (`setTabActive`). After system suspend/resume
  // these can surface `RuntimeError: Out of bounds memory access`. Routing
  // the error into the shared recovery entry point lets the rest of the
  // activation flow (focus, globals, mux status) continue so the tab does
  // not end up in a half-switched state.
  manager.on("tab:activated", ({ tab, previousTabId }) => {
    // Deactivate rendering on previous tab to reduce CPU/GPU load
    if (previousTabId) {
      const prevApp = manager.getTerminalApp(previousTabId);
      if (prevApp) {
        try {
          prevApp.setTabActive(false);
        } catch (error) {
          console.error("[ERROR][FRONTEND] setTabActive(false) failed:", error);
          prevApp.tryRecoverFromWasmCrash?.(error);
        }
      }
    }

    const app = manager.getTerminalApp(tab.id);
    if (!app) return;

    // Resume rendering on the active tab
    try {
      app.setTabActive(true);
    } catch (error) {
      console.error("[ERROR][FRONTEND] setTabActive(true) failed:", error);
      app.tryRecoverFromWasmCrash?.(error);
      // Continue: remaining steps do not touch WASM directly. If recovery
      // is async the render will refresh once reinitWasm finishes.
    }

    // Focus the IME handler for the active tab.
    // Use rAF to ensure focus is restored after browser's default
    // mousedown focus handling completes (mouse clicks on tab bar
    // elements can steal focus away from the terminal).
    app.focus();
    requestAnimationFrame(() => app.focus());

    // Update global references for E2E testing
    window.terminalApp = app;
    window.terminalState = app.terminalState;
    window.terminalRenderer = app.terminalRenderer;

    // Handle mux status bar on tab switch:
    // - Mux tab: restore cached status bar immediately, then request fresh data
    // - Non-mux tab: clear OSC layer
    if (app.isInMuxMode) {
      const cached = statusBarCache.get(tab.id);
      if (cached) {
        oscLayerController?.handleCommand("set", "left", cached.left);
        oscLayerController?.handleCommand("set", "right", cached.right);
      }
      app.sendMuxRequestStatusUpdate();
    } else {
      oscLayerController?.handleCommand("clear");
    }
  });

  // Create initial tab
  await tabManager.createTab();

  // Expose for E2E testing
  window.tabManager = tabManager;
  const activeTab = tabManager.getActiveTab();
  if (activeTab) {
    const app = tabManager.getTerminalApp(activeTab.id);
    if (app) {
      window.terminalApp = app;
      window.terminalState = app.terminalState;
      window.terminalRenderer = app.terminalRenderer;
    }
  }
}

/**
 * Legacy single-terminal mode (fallback)
 */
async function initLegacyMode(): Promise<void> {
  const container = document.getElementById("terminal");
  if (!container) {
    console.error("Terminal element not found");
    return;
  }

  const app = new TerminalApp(container);
  await app.init();

  window.terminalApp = app;
  window.terminalState = app.terminalState;
  window.terminalRenderer = app.terminalRenderer;
}

/**
 * Cleanup resources before unload
 */
function cleanup(): void {
  keyboardHandler?.detach();
  dragHandler?.dispose();
  activityTracker?.dispose();
  notificationManager?.dispose();
  tabBarUI?.dispose();
  statusBarUI?.dispose();
  tabManager?.dispose();

  tabManager = null;
  tabBarUI = null;
  statusBarUI = null;
  oscLayerController = null;
  keyboardHandler = null;
  dragHandler = null;
  activityTracker = null;
  notificationManager = null;
}

// Initialize when DOM is ready
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", main);
} else {
  main();
}

// Cleanup on page unload
window.addEventListener("beforeunload", cleanup);

// Type declarations for E2E testing globals
declare global {
  interface Window {
    tabManager: TabManager | null;
    terminalApp: TerminalApp | null;
    terminalState: import("./terminal/state").TerminalState | null;
    terminalRenderer: import("./terminal").ITerminalRenderer | null;
    ptyClient: import("./pty/client").PtyClient | null;
  }
}
