/**
 * Settings Applier
 *
 * Applies settings changes to both CSS variables and terminal renderers.
 * Uses a unified pattern for extensibility.
 */

import type {
  AppSettings,
  UiTheme,
  UiThemePreset,
  CursorStyle,
  ScrollbarMode,
  UserColorScheme,
} from "./types";
import { getColorSchemePreset, rgbToCSS } from "../terminal/colors";
import { UI_THEME_PRESETS, applyPresetColors } from "./ui-theme-presets";
import {
  MARKDOWN_THEME_PRESETS,
  MARKDOWN_COLOR_TO_CSS_VAR,
  type MarkdownThemeColors,
} from "./markdown-theme-presets";

/**
 * Settings that can be applied to renderers.
 */
export interface RendererSettings {
  fontSize: number;
  fontFamily: string;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  colorScheme: string;
  userColorScheme: UserColorScheme | null;
  foldEnabled: boolean;
  boldBrightensAnsiColors: boolean;
}

/** Listener for system theme media query (UI theme) */
let systemThemeListener: ((e: MediaQueryListEvent) => void) | null = null;

/** Listener for system theme media query (Markdown color theme) */
let markdownSystemThemeListener: ((e: MediaQueryListEvent) => void) | null =
  null;

/**
 * MD3 baseline error accent for the Markdown palette, written per effective
 * theme as `--markdown-error`. The preset palettes carry no error color, and
 * the accent is preset-independent (the same MD3 error color across presets)
 * but light/dark distinct — so it is supplied here alongside the palette and
 * follows live OS theme flips like the rest of the `--markdown-*` variables.
 * This keeps the front matter parse-error styling drawing from the theme-aware
 * palette path (NFR2) with no color literal in the stylesheet.
 */
const MARKDOWN_ERROR_ACCENT: Record<"light" | "dark", string> = {
  light: "#ba1a1a",
  dark: "#f2b8b5",
};

/**
 * Apply all settings to the application.
 * Updates CSS variables and notifies all terminal renderers.
 */
export function applySettings(settings: AppSettings): void {
  applyFontSize(settings.font_size);
  applyFontFamily(settings.font_family_primary, settings.font_family_secondary);
  applyUiTheme(settings.ui_theme, settings.ui_theme_preset);
  applyTerminalColorScheme(
    settings.terminal_color_scheme,
    settings.custom_color_schemes,
  );
  applyPadding(settings.padding);
  applyScrollbar(settings.show_scrollbar);
  applyCursorStyle(settings.cursor_style);
  applyCursorBlink(settings.cursor_blink);
  applyUiFont(settings.ui_font_family);
  applyMarkdownSettings(
    settings.markdown_body_font_family,
    settings.markdown_code_font_family,
    settings.markdown_font_size,
  );
  applyMarkdownColorTheme({
    followUi: settings.markdown_theme_follow_ui,
    mdTheme: settings.markdown_theme,
    mdPreset: settings.markdown_theme_preset,
    uiTheme: settings.ui_theme,
    uiPreset: settings.ui_theme_preset,
  });
  applyFoldEnabled(settings.fold_enabled);
  applyBoldBrightensAnsiColors(settings.bold_brightens_ansi_colors);
  applyStatusBar(settings);
}

/**
 * Apply status bar settings.
 * Dispatches a custom event that StatusBarUI listens for.
 */
export function applyStatusBar(settings: AppSettings): void {
  if (
    typeof window !== "undefined" &&
    typeof window.dispatchEvent === "function"
  ) {
    window.dispatchEvent(
      new CustomEvent("emterm-statusbar-settings", { detail: settings }),
    );
  }
}

/**
 * Apply fold enabled setting.
 * Notifies terminal instances to enable/disable FoldManager.
 */
export function applyFoldEnabled(enabled: boolean): void {
  notifyRenderers("foldEnabled", enabled);
}

/**
 * Apply bold-brightens ANSI colors setting.
 * Notifies terminal instances to enable/disable bold-to-bright conversion.
 */
export function applyBoldBrightensAnsiColors(enabled: boolean): void {
  notifyRenderers("boldBrightensAnsiColors", enabled);
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
 * Build a CSS font-family value from user-configured font fields.
 * Order: primary, secondary. Empty fields are omitted. Emoji and
 * symbol coverage is handled by the bundled font stack on the Rust
 * side, so they do not appear in the chain. Returns an empty string
 * when no fonts are configured.
 */
export function buildFontFamilyChain(
  primary: string,
  secondary: string,
): string {
  const parts: string[] = [];
  if (primary) parts.push(primary);
  if (secondary) parts.push(secondary);
  return parts.join(", ");
}

/**
 * Apply font family setting from two separate fields.
 * Sets --terminal-font-family CSS variable when fonts are configured.
 * Renderer receives the user chain or "monospace" as default.
 */
export function applyFontFamily(primary: string, secondary: string): void {
  const chain = buildFontFamilyChain(primary, secondary);
  const root = document.documentElement;
  if (chain) {
    root.style.setProperty("--terminal-font-family", chain);
  } else {
    root.style.removeProperty("--terminal-font-family");
  }

  notifyRenderers("fontFamily", chain || "monospace");
}

/**
 * Apply UI theme setting with preset colors.
 * "system" respects prefers-color-scheme media query.
 * Preset colors are applied as CSS variables on :root.
 */
export function applyUiTheme(
  theme: UiTheme,
  preset: UiThemePreset = "purple",
): void {
  const root = document.documentElement;

  // Clean up previous system theme listener
  if (systemThemeListener) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.removeEventListener("change", systemThemeListener);
    systemThemeListener = null;
  }

  // Fallback to "purple" if preset is invalid
  const safePreset = UI_THEME_PRESETS[preset] ? preset : "purple";

  if (theme === "system") {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const resolved = mq.matches ? "dark" : "light";
    root.setAttribute("data-theme", resolved);
    root.style.colorScheme = resolved;
    applyPresetColors(UI_THEME_PRESETS[safePreset][resolved]);

    // Listen for system theme changes
    systemThemeListener = (e: MediaQueryListEvent) => {
      const newResolved = e.matches ? "dark" : "light";
      root.setAttribute("data-theme", newResolved);
      root.style.colorScheme = newResolved;
      applyPresetColors(UI_THEME_PRESETS[safePreset][newResolved]);
    };
    mq.addEventListener("change", systemThemeListener);
  } else {
    root.setAttribute("data-theme", theme);
    root.style.colorScheme = theme;
    applyPresetColors(UI_THEME_PRESETS[safePreset][theme]);
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
 * Other values set terminal color CSS variables from a preset or user scheme.
 *
 * @param scheme - Scheme name to apply
 * @param userSchemes - Optional array of user-defined color schemes
 */
export function applyTerminalColorScheme(
  scheme: string,
  userSchemes?: UserColorScheme[],
): void {
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

  // Check if it's a user scheme first
  const userScheme = userSchemes?.find((s) => s.name === scheme);
  if (userScheme) {
    applyUserColorScheme(root, userScheme);
    // Pass the full user scheme object to renderers
    notifyRenderers("userColorScheme", userScheme);
    return;
  }

  // Fall back to preset lookup
  const preset = getColorSchemePreset(scheme);
  if (preset) {
    root.style.setProperty(
      "--terminal-background",
      rgbToCSS(preset.background),
    );
  }

  // Notify renderers with the scheme name
  notifyRenderers("colorScheme", scheme);
}

/**
 * Apply a user-defined color scheme by setting all CSS variables.
 */
function applyUserColorScheme(
  root: HTMLElement,
  scheme: UserColorScheme,
): void {
  root.style.setProperty("--terminal-foreground", scheme.foreground);
  root.style.setProperty("--terminal-background", scheme.background);
  root.style.setProperty("--terminal-cursor-color", scheme.cursor);
  root.style.setProperty("--terminal-selection-bg", scheme.selection);

  for (let i = 0; i < 16; i++) {
    const color = scheme.ansi_colors[i];
    if (color) {
      root.style.setProperty(`--terminal-color-${i}`, color);
    }
  }
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
 * Apply UI font family setting.
 * Sets the --ui-font-family CSS variable for application UI text.
 * Empty or whitespace-only values are ignored (uses CSS default).
 */
export function applyUiFont(fontFamily: string): void {
  const root = document.documentElement;
  const trimmed = fontFamily?.trim();
  if (trimmed) {
    root.style.setProperty("--ui-font-family", trimmed);
  } else {
    root.style.removeProperty("--ui-font-family");
  }
}

/**
 * Apply Markdown viewer settings.
 * Sets CSS variables for Markdown fullscreen overlay fonts and size.
 * Empty font strings remove the property so CSS fallback chains apply.
 */
export function applyMarkdownSettings(
  bodyFont: string,
  codeFont: string,
  fontSize: number,
): void {
  const root = document.documentElement;
  const trimmedBody = bodyFont?.trim();
  if (trimmedBody) {
    root.style.setProperty("--markdown-body-font-family", trimmedBody);
  } else {
    root.style.removeProperty("--markdown-body-font-family");
  }
  const trimmedCode = codeFont?.trim();
  if (trimmedCode) {
    root.style.setProperty("--markdown-code-font-family", trimmedCode);
  } else {
    root.style.removeProperty("--markdown-code-font-family");
  }
  root.style.setProperty("--markdown-body-font-size", `${fontSize}pt`);
}

/**
 * Options for applyMarkdownColorTheme.
 */
export interface MarkdownColorThemeOptions {
  followUi: boolean;
  mdTheme: UiTheme;
  mdPreset: UiThemePreset;
  uiTheme: UiTheme;
  uiPreset: UiThemePreset;
}

/**
 * Apply Markdown viewer color theme.
 * Resolves the effective theme/preset (follow UI or independent) and applies
 * the corresponding palette to --markdown-* CSS color variables — including the
 * per-theme --markdown-error accent — initially and on every live system-theme
 * change, so all theme-aware markdown colors stay in sync.
 */
export function applyMarkdownColorTheme(
  options: MarkdownColorThemeOptions,
): void {
  const { followUi, mdTheme, mdPreset, uiTheme, uiPreset } = options;
  const root = document.documentElement;

  // Clean up previous markdown system theme listener
  if (markdownSystemThemeListener) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.removeEventListener("change", markdownSystemThemeListener);
    markdownSystemThemeListener = null;
  }

  const effectiveTheme = followUi ? uiTheme : mdTheme;
  const effectivePreset = followUi ? uiPreset : mdPreset;

  // Fallback to "purple" if preset is invalid
  const safePreset = MARKDOWN_THEME_PRESETS[effectivePreset]
    ? effectivePreset
    : "purple";

  const applyPalette = (mode: "dark" | "light") => {
    const palette = MARKDOWN_THEME_PRESETS[safePreset][mode];
    for (const [key, cssVar] of Object.entries(MARKDOWN_COLOR_TO_CSS_VAR)) {
      root.style.setProperty(cssVar, palette[key as keyof MarkdownThemeColors]);
    }
    // The preset palettes carry no error color; supply the MD3 error accent per
    // effective theme so the front matter parse-error styling draws from the
    // same theme-aware --markdown-* path (NFR2) and follows live OS flips.
    root.style.setProperty("--markdown-error", MARKDOWN_ERROR_ACCENT[mode]);
  };

  if (effectiveTheme === "system") {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const resolved = mq.matches ? "dark" : "light";
    applyPalette(resolved);

    // Listen for system theme changes
    markdownSystemThemeListener = (e: MediaQueryListEvent) => {
      const newResolved = e.matches ? "dark" : "light";
      applyPalette(newResolved);
    };
    mq.addEventListener("change", markdownSystemThemeListener);
  } else {
    applyPalette(effectiveTheme);
  }
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
