/**
 * Settings Applier
 *
 * Applies settings changes to both CSS variables and terminal renderers.
 * Uses a unified pattern for extensibility.
 */

import type {
  AppSettings,
  UiTheme,
  CursorStyle,
  ScrollbarMode,
} from "./types";

/**
 * Settings that can be applied to renderers.
 */
export interface RendererSettings {
  fontSize: number;
  fontFamily: string;
  lineHeight: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  opacity: number;
  colorScheme: string;
}

/** Listener for system theme media query */
let systemThemeListener: ((e: MediaQueryListEvent) => void) | null = null;

/**
 * Apply all settings to the application.
 * Updates CSS variables and notifies all terminal renderers.
 */
export function applySettings(settings: AppSettings): void {
  applyFontSize(settings.font_size);
  applyFontFamily(settings.font_family);
  applyLineHeight(settings.line_height);
  applyUiTheme(settings.ui_theme);
  applyTerminalColorScheme(settings.terminal_color_scheme);
  applyPadding(settings.padding);
  applyScrollbar(settings.show_scrollbar);
  applyOpacity(settings.opacity);
  applyCursorStyle(settings.cursor_style);
  applyCursorBlink(settings.cursor_blink);
}

/**
 * Apply font size setting.
 * @param fontSize - Font size in points
 */
export function applyFontSize(fontSize: number): void {
  const root = document.documentElement;
  root.style.setProperty("--terminal-font-size", `${fontSize}pt`);

  // Notify renderers
  notifyRenderers("fontSize", fontSize);
}

/**
 * Apply font family setting.
 * Empty string means system monospace (browser fallback).
 */
export function applyFontFamily(fontFamily: string): void {
  const root = document.documentElement;
  if (fontFamily) {
    root.style.setProperty("--terminal-font-family", fontFamily);
  } else {
    root.style.removeProperty("--terminal-font-family");
  }

  notifyRenderers("fontFamily", fontFamily);
}

/**
 * Apply line height setting.
 * Sets the line-height as a multiplier (e.g., 1.2).
 */
export function applyLineHeight(lineHeight: number): void {
  const root = document.documentElement;
  root.style.setProperty("--terminal-line-height", String(lineHeight));

  notifyRenderers("lineHeight", lineHeight);
}

/**
 * Apply UI theme setting.
 * "system" respects prefers-color-scheme media query.
 */
export function applyUiTheme(theme: UiTheme): void {
  const root = document.documentElement;

  // Clean up previous system theme listener
  if (systemThemeListener) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.removeEventListener("change", systemThemeListener);
    systemThemeListener = null;
  }

  if (theme === "system") {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const resolved = mq.matches ? "dark" : "light";
    root.setAttribute("data-theme", resolved);

    // Listen for system theme changes
    systemThemeListener = (e: MediaQueryListEvent) => {
      root.setAttribute("data-theme", e.matches ? "dark" : "light");
    };
    mq.addEventListener("change", systemThemeListener);
  } else {
    root.setAttribute("data-theme", theme);
  }
}

/**
 * Apply padding setting.
 * @param padding - Padding in pixels
 */
export function applyPadding(padding: number): void {
  const root = document.documentElement;
  root.style.setProperty("--terminal-padding", `${padding}px`);
}

/**
 * Apply scrollbar mode setting.
 */
export function applyScrollbar(mode: ScrollbarMode): void {
  const root = document.documentElement;
  root.style.setProperty("--terminal-scrollbar-mode", mode);

  // Map scrollbar mode to CSS overflow-y value
  const overflowMap: Record<ScrollbarMode, string> = {
    always: "scroll",
    never: "hidden",
    auto: "auto",
  };
  root.style.setProperty(
    "--terminal-scrollbar-overflow",
    overflowMap[mode] || "auto",
  );
}

/**
 * Apply opacity setting.
 * @param opacity - Opacity value (0.3-1.0)
 */
export function applyOpacity(opacity: number): void {
  const root = document.documentElement;
  root.style.setProperty("--terminal-opacity", String(opacity));

  notifyRenderers("opacity", opacity);
}

/** Terminal color scheme CSS variable names */
const TERMINAL_COLOR_VARS = [
  "--terminal-foreground",
  "--terminal-background",
  "--terminal-cursor-color",
  "--terminal-selection-bg",
  "--terminal-color-0",
  "--terminal-color-1",
  "--terminal-color-2",
  "--terminal-color-3",
  "--terminal-color-4",
  "--terminal-color-5",
  "--terminal-color-6",
  "--terminal-color-7",
  "--terminal-color-8",
  "--terminal-color-9",
  "--terminal-color-10",
  "--terminal-color-11",
  "--terminal-color-12",
  "--terminal-color-13",
  "--terminal-color-14",
  "--terminal-color-15",
] as const;

/**
 * Apply terminal color scheme setting.
 * "default" or empty string removes custom overrides (uses CSS theme defaults).
 * Other values set terminal color CSS variables from a preset.
 */
export function applyTerminalColorScheme(scheme: string): void {
  const root = document.documentElement;

  if (!scheme || scheme === "default" || scheme === "emterm") {
    // Remove all custom terminal color overrides
    for (const varName of TERMINAL_COLOR_VARS) {
      root.style.removeProperty(varName);
    }
    root.removeAttribute("data-terminal-color-scheme");
    // Notify renderers with "emterm" for default
    notifyRenderers("colorScheme", "emterm");
    return;
  }

  // Store the scheme name as a data attribute
  root.setAttribute("data-terminal-color-scheme", scheme);
  // Notify renderers with the scheme name
  notifyRenderers("colorScheme", scheme);
}

/**
 * Apply cursor style setting.
 */
export function applyCursorStyle(cursorStyle: CursorStyle): void {
  notifyRenderers("cursorStyle", cursorStyle);
}

/**
 * Apply cursor blink setting.
 */
export function applyCursorBlink(cursorBlink: boolean): void {
  notifyRenderers("cursorBlink", cursorBlink);
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
