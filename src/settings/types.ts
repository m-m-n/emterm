/**
 * Settings Types
 *
 * TypeScript type definitions matching Rust AppSettings struct.
 */

// ============================================================
// Enum Type Aliases
// ============================================================

export type UiTheme = "light" | "dark" | "system";
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
  line_height: number;

  // Theme / Color
  ui_theme: UiTheme;
  terminal_color_scheme: string;
  opacity: number;

  // Layout
  padding: number;
  scrollback_lines: number;
  show_scrollbar: ScrollbarMode;

  // Terminal
  shell_path: string;
  shell_args: string[];
  cursor_style: CursorStyle;
  cursor_blink: boolean;
  scroll_speed: number;
  bell_action: BellAction;
  url_detection: boolean;
  copy_on_select: boolean;

  // Keybinds
  keybinds: KeybindSettings;

  // Language
  language: Language;
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
}

// ============================================================
// Font Picker Types
// ============================================================

export interface FontListResponse {
  monospace_fonts: string[];
  all_fonts: string[];
  emoji_fonts: string[];
}

export type FontCategory = "primary" | "secondary" | "emoji";

// ============================================================
// Validation Constants
// ============================================================

export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 32;
export const MIN_LINE_HEIGHT = 0.8;
export const MAX_LINE_HEIGHT = 3.0;
export const LINE_HEIGHT_STEP = 0.1;
export const MIN_OPACITY = 0.3;
export const MAX_OPACITY = 1.0;
export const OPACITY_STEP = 0.05;
export const MIN_PADDING = 0;
export const MAX_PADDING = 32;
export const MIN_SCROLLBACK_LINES = 0;
export const MAX_SCROLLBACK_LINES = 100000;
export const MIN_SCROLL_SPEED = 1;
export const MAX_SCROLL_SPEED = 10;
