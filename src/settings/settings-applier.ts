/**
 * Settings Applier
 *
 * Applies settings changes to both CSS variables and terminal renderers.
 * Uses a unified pattern for extensibility.
 */

import type { AppSettings } from "./types";

/**
 * Settings that can be applied to renderers.
 */
export interface RendererSettings {
  fontSize: number;
  // Future extensions:
  // colorScheme: ColorScheme;
  // fontFamily: string;
  // cursorStyle: CursorStyle;
}

/**
 * Apply all settings to the application.
 * Updates CSS variables and notifies all terminal renderers.
 */
export function applySettings(settings: AppSettings): void {
  applyFontSize(settings.font_size);
  // Future: applyColorScheme(settings.color_scheme);
}

/**
 * Apply font size setting.
 * @param fontSize - Font size in points
 */
export function applyFontSize(fontSize: number): void {
  // 1. Update CSS variables
  const root = document.documentElement;
  root.style.setProperty("--terminal-font-size", `${fontSize}pt`);
  const lineHeight = fontSize + 2;
  root.style.setProperty("--terminal-line-height", `${lineHeight}pt`);

  // 2. Update all terminal renderers
  notifyRenderers("fontSize", fontSize);
}

/**
 * Notify all terminal renderers of a setting change.
 * @param setting - The setting key
 * @param value - The new value
 */
function notifyRenderers<K extends keyof RendererSettings>(
  setting: K,
  value: RendererSettings[K],
): void {
  if (typeof window !== "undefined" && (window as any).tabManager) {
    (window as any).tabManager.updateAllTerminalsSetting(setting, value);
  }
}

/**
 * Legacy export for backward compatibility during transition.
 * @deprecated Use applySettings instead
 */
export function applySettingsToCSS(settings: AppSettings): void {
  applySettings(settings);
}
