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
import { initConsoleBridge } from "./utils/console-bridge";
import { SettingsService, applySettingsToCSS } from "./settings";
import { initI18n, resolveLocale } from "./i18n/index.ts";

let tabManager: TabManager | null = null;
let tabBarUI: TabBarUI | null = null;
let keyboardHandler: TabKeyboardHandler | null = null;
let dragHandler: TabDragHandler | null = null;

/**
 * Initialize the terminal application with tab support
 */
async function main(): Promise<void> {
  // Initialize console bridge to forward logs to stdout/stderr
  initConsoleBridge();

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

  // Create TabManager
  // Use a temporary variable to allow callback closure to reference it
  const manager = new TabManager({
    container: contentContainer,
    createTerminalApp: async (tabContainer) => {
      const app = new TerminalApp(tabContainer);
      await app.init();

      // Connect PTY exit event to TabManager
      const sessionId = app.pty?.getSessionId();
      if (sessionId) {
        app.onSessionExit((exitedSessionId) => {
          manager.handleSessionExit(exitedSessionId);
        });
      }

      return app;
    },
  });
  tabManager = manager;

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

  // Apply initial tab bar visibility from settings
  try {
    const settings = await SettingsService.load();
    if (settings.show_tab_bar === false) {
      tabBarUI.setVisible(false);
    }
  } catch (error) {
    console.warn("Failed to apply initial tab bar visibility:", error);
  }

  // Create keyboard handler with toggle callback
  const tabBarUIRef = tabBarUI;
  keyboardHandler = new TabKeyboardHandler(tabManager, {
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

  // Create drag handler for tab reordering
  dragHandler = new TabDragHandler({
    tabManager,
    tabBarUI,
  });
  dragHandler.init();

  // Focus the terminal when a tab is activated and update global references
  manager.on("tab:activated", ({ tab }) => {
    const app = manager.getTerminalApp(tab.id);
    if (app) {
      // Focus the IME handler for the active tab
      app.focus();

      // Update global references for E2E testing
      window.terminalApp = app;
      window.terminalState = app.terminalState;
      window.terminalRenderer = app.terminalRenderer;
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
  tabBarUI?.dispose();
  tabManager?.dispose();

  tabManager = null;
  tabBarUI = null;
  keyboardHandler = null;
  dragHandler = null;
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
