/**
 * Context menu builder for terminal, tab, and tab bar areas.
 *
 * Creates native Tauri menus dynamically on each right-click
 * to reflect current state (selection, URL, profiles).
 */

import { Menu } from "@tauri-apps/api/menu/menu";
import { MenuItem } from "@tauri-apps/api/menu/menuItem";
import { PredefinedMenuItem } from "@tauri-apps/api/menu/predefinedMenuItem";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { writeText, readText } from "@tauri-apps/plugin-clipboard-manager";
import { t } from "../i18n/index.ts";
import { findUrlAtPosition, getLogicalLine, physicalToLogicalCol } from "../terminal/url-detector";
import { showPasteDialog, sendTextInChunks } from "../clipboard";
import type { TerminalApp } from "../terminal-app";
import type { TabManager } from "../tab-bar/tab-manager";
import type { TabBarUI } from "../tab-bar/tab-bar-ui";
import { SettingsService } from "../settings/settings-service";

/**
 * Context for building the terminal context menu.
 */
export interface TerminalMenuContext {
  app: TerminalApp;
}

/**
 * Context for building tab/tab-bar context menus.
 */
export interface TabBarMenuContext {
  tabManager: TabManager;
  tabBarUI: TabBarUI;
}

/**
 * Build and show the terminal context menu.
 */
export async function showTerminalContextMenu(
  event: MouseEvent,
  ctx: TerminalMenuContext,
): Promise<void> {
  event.preventDefault();

  const { app } = ctx;
  const selection = app.selection;
  const root = app.root;
  const state = app.terminalState;
  const cellSize = app.cellSize;
  const ptyClient = app.pty;

  if (!root || !state) return;

  // Check selection state
  const hasSelection = selection?.hasSelection() ?? false;

  // Detect URL at click position
  let detectedUrl: string | null = null;
  try {
    const rect = root.getBoundingClientRect();
    const col = Math.floor((event.clientX - rect.left) / cellSize.width);
    const row = Math.floor((event.clientY - rect.top) / cellSize.height);

    if (row >= 0 && row < state.rows) {
      const buffer = state.getActiveBuffer();
      const logical = getLogicalLine((r) => buffer.getLine(r), row, state.rows);
      if (logical.text) {
        const logicalCol = physicalToLogicalCol(row, col, logical);
        detectedUrl = findUrlAtPosition(logical.text, logicalCol);
      }
    }
  } catch (e) {
    console.debug("URL detection failed:", e);
  }

  // Build menu items in parallel (each MenuItem.new() is an IPC call)
  const [copyItem, pasteItem, separator, copyUrlItem, openUrlItem] =
    await Promise.all([
      MenuItem.new({
        text: t("contextMenu.copy"),
        enabled: hasSelection,
        action: () => {
          selection?.copy();
        },
      }),
      MenuItem.new({
        text: t("contextMenu.paste"),
        enabled: true,
        action: async () => {
          if (!ptyClient) return;
          try {
            const text = await readText();
            if (!text) return;

            // Auto-scroll to bottom when user pastes during scrollback
            app.exitScrollback();

            const hasNewlines = /[\r\n]/.test(text);
            if (hasNewlines) {
              const lineCount = text.split(/\r\n|\r|\n/).length;
              const result = await showPasteDialog({ text, lineCount });
              if (result.confirmed) {
                await sendTextInChunks(text, (data: Uint8Array) =>
                  ptyClient.write(data),
                );
              }
            } else {
              const bytes = new TextEncoder().encode(text);
              await ptyClient.write(bytes);
            }
          } catch (error) {
            console.error("Failed to paste from context menu:", error);
          } finally {
            // Restore focus to IME input after paste completes
            app.focus();
          }
        },
      }),
      PredefinedMenuItem.new({ item: "Separator" }),
      MenuItem.new({
        text: t("contextMenu.copyUrl"),
        enabled: detectedUrl !== null,
        action: async () => {
          if (detectedUrl) {
            try {
              await writeText(detectedUrl);
            } catch (error) {
              console.error("Failed to copy URL:", error);
            }
          }
        },
      }),
      MenuItem.new({
        text: t("contextMenu.openUrl"),
        enabled: detectedUrl !== null,
        action: async () => {
          if (detectedUrl) {
            try {
              await shellOpen(detectedUrl);
            } catch (error) {
              console.error("Failed to open URL:", error);
            }
          }
        },
      }),
    ]);

  const menu = await Menu.new({
    items: [copyItem, pasteItem, separator, copyUrlItem, openUrlItem],
  });

  try {
    await menu.popup();
  } finally {
    await menu.close();
  }
}

/**
 * Build and show the tab context menu (Close).
 */
export async function showTabContextMenu(
  event: MouseEvent,
  tabId: string,
  tabManager: TabManager,
): Promise<void> {
  event.preventDefault();
  event.stopPropagation();

  const closeItem = await MenuItem.new({
    text: t("contextMenu.closeTab"),
    enabled: true,
    action: () => {
      tabManager.closeTab(tabId);
    },
  });

  const menu = await Menu.new({ items: [closeItem] });
  try {
    await menu.popup();
  } finally {
    await menu.close();
  }
}

/**
 * Build and show the tab bar context menu (New Tab, Open Profile).
 */
export async function showTabBarContextMenu(
  event: MouseEvent,
  ctx: TabBarMenuContext,
): Promise<void> {
  event.preventDefault();

  const { tabManager, tabBarUI } = ctx;
  const settings = SettingsService.getCached();
  const profiles = settings?.profiles ?? [];
  const hasProfiles = profiles.length > 0;

  const [newTabItem, openProfileItem] = await Promise.all([
    MenuItem.new({
      text: t("contextMenu.newTab"),
      enabled: true,
      action: () => {
        tabManager.createTab();
      },
    }),
    MenuItem.new({
      text: t("contextMenu.openProfile"),
      enabled: hasProfiles,
      action: () => {
        if (hasProfiles) {
          tabBarUI.showProfileSelector(profiles);
        }
      },
    }),
  ]);

  const menu = await Menu.new({ items: [newTabItem, openProfileItem] });
  try {
    await menu.popup();
  } finally {
    await menu.close();
  }
}
