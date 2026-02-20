/**
 * Settings Types
 *
 * TypeScript type definitions matching Rust AppSettings struct.
 */

// ============================================================
// Enum Type Aliases
// ============================================================

export type UiTheme = "light" | "dark" | "system";
export type UiThemePreset = "purple" | "blue" | "green" | "orange";
export type CursorStyle = "block" | "underline" | "bar";
export type BellAction = "sound" | "visual" | "none";
export type ScrollbarMode = "auto" | "always" | "never";
export type Language = "auto" | "en" | "ja";

// ============================================================
// Settings Interfaces
// ============================================================

/**
 * Application settings structure.
 * Matches Rust AppSettings exactly for JSON serialization.
 *
 * All fields are always valid (never null) because
 * the backend applies defaults before returning settings.
 */
export interface AppSettings {
  // Font
  font_size: number;
  font_family_primary: string;
  font_family_secondary: string;
  font_family_emoji: string;

  // Theme / Color
  ui_theme: UiTheme;
  ui_theme_preset: UiThemePreset;
  terminal_color_scheme: string;

  // Layout
  padding: number;
  scrollback_lines: number;
  show_scrollbar: ScrollbarMode;
  show_tab_bar: boolean;

  // Terminal
  shell_path: string;
  shell_args: string[];
  cursor_style: CursorStyle;
  cursor_blink: boolean;
  scroll_speed: number;
  bell_action: BellAction;
  url_detection: boolean;
  copy_on_select: boolean;
  fold_enabled: boolean;
  file_path_detection: boolean;
  bold_brightens_ansi_colors: boolean;
  middle_click_paste: boolean;
  editor_command: string;

  // Notification
  notification_enabled: boolean;
  tab_activity_indicator: boolean;
  notify_on_process_exit: boolean;
  notify_on_output: boolean;
  notify_on_bell: boolean;

  // Keybinds
  keybinds: KeybindSettings;

  // Language
  language: Language;

  // UI Font
  ui_font_family: string;

  // Custom Color Schemes
  custom_color_schemes: UserColorScheme[];

  // Markdown Viewer Theme
  markdown_theme_follow_ui: boolean;
  markdown_theme: UiTheme;
  markdown_theme_preset: UiThemePreset;

  // Markdown Viewer Font
  markdown_body_font_family: string;
  markdown_code_font_family: string;
  markdown_font_size: number;
}

export interface KeybindSettings {
  copy: string;
  paste: string;
  select_all: string;
  search: string;
  new_tab: string;
  close_tab: string;
  next_tab: string;
  prev_tab: string;
  zoom_in: string;
  zoom_out: string;
  zoom_reset: string;
  toggle_fullscreen: string;
  open_settings: string;
  toggle_tab_bar: string;
  jump_to_prev_prompt: string;
  jump_to_next_prompt: string;
}

// ============================================================
// Font Picker Types
// ============================================================

export interface FontListResponse {
  monospace_fonts: string[];
  all_fonts: string[];
  emoji_fonts: string[];
}

export type FontCategory = "primary" | "secondary" | "emoji" | "ui" | "markdown-body" | "markdown-code";

// ============================================================
// User Color Scheme
// ============================================================

/**
 * User-defined terminal color scheme.
 * Stored in settings.json under custom_color_schemes.
 */
export interface UserColorScheme {
  name: string;
  foreground: string; // "#RRGGBB"
  background: string; // "#RRGGBB"
  cursor: string; // "#RRGGBB"
  selection: string; // "#RRGGBB"
  ansi_colors: string[]; // 16 "#RRGGBB" strings
}

// ============================================================
// Validation Constants
// ============================================================

export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 32;
export const MIN_PADDING = 0;
export const MAX_PADDING = 32;
export const MIN_SCROLLBACK_LINES = 0;
export const MAX_SCROLLBACK_LINES = 100000;
export const MIN_SCROLL_SPEED = 1;
export const MAX_SCROLL_SPEED = 10;
