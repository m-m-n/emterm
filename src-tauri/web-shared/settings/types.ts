/**
 * Settings Types
 *
 * TypeScript type definitions matching Rust AppSettings struct.
 */

// ============================================================
// Enum Type Aliases
// ============================================================

export type UiTheme = "light" | "dark" | "system";
export type UiThemePreset = "purple" | "blue" | "green" | "orange" | "pink";
export type CursorStyle = "block" | "underline" | "bar";
export type BellAction = "sound" | "visual" | "none";
export type ScrollbarMode = "auto" | "always" | "never";
export type Language = "auto" | "en" | "ja";
export type ShiftEnterBehavior = "none" | "alt_enter" | "kitty_csi_u" | "lf";

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
  alternate_scroll_enabled: boolean;
  bell_action: BellAction;
  url_detection: boolean;
  copy_on_select: boolean;
  fold_enabled: boolean;
  file_path_detection: boolean;
  bold_brightens_ansi_colors: boolean;
  middle_click_paste: boolean;
  shift_enter_behavior: ShiftEnterBehavior;
  editor_command: string;

  // IME
  skk_mode: boolean;

  // Notification
  notification_enabled: boolean;
  tab_activity_indicator: boolean;
  notify_on_process_exit: boolean;
  notify_on_output: boolean;
  notify_on_bell: boolean;
  agent_status_notifications: boolean;

  // Keybinds
  keybinds: KeybindSettings;

  // Language
  language: Language;

  // UI Font
  ui_font_family: string;

  // Custom Color Schemes
  custom_color_schemes: UserColorScheme[];

  // Profiles
  profiles: Profile[];

  // Markdown Viewer Theme
  markdown_theme_follow_ui: boolean;
  markdown_theme: UiTheme;
  markdown_theme_preset: UiThemePreset;

  // Markdown Viewer Font
  markdown_body_font_family: string;
  markdown_code_font_family: string;
  markdown_font_size: number;

  // SSH
  ssh_command_path: string;
  ssh_connections: SshConnection[];

  // SFTP
  sftp_max_concurrent_uploads: number;

  // OSC 52 Clipboard
  clipboard_read_osc52: boolean;
  clipboard_max_size_osc52: number;

  // Log Recording
  log_recording_enabled: boolean;

  // Mux (multiplexer) settings
  mux: MuxSettings;

  // Status Bar
  statusbar_enabled: boolean;
  statusbar_app_line1_left: string;
  statusbar_app_line1_right: string;
  statusbar_app_line2_left: string;
  statusbar_app_line2_right: string;
  statusbar_time_format: string;
  statusbar_font_size: number | null;
  statusbar_custom_commands: Record<string, StatusbarCustomCommand>;
  statusbar_refresh_rates: Record<string, number>;
}

export interface MuxSettings {
  prefix: string;
  tab_always_expand: boolean;
  tmux_conf_imported: boolean;
  window_sidebar_overlay: boolean;
  keybinds: Record<string, string>;
}

/**
 * One default mux action binding, sourced from the Rust SSOT
 * (`crate::mux::prefix::DEFAULT_ACTION_BINDINGS`) via the
 * `get_mux_action_defaults` IPC command. The settings panel reads these
 * instead of duplicating the default table in TypeScript. Ordered as the
 * backend declares them (display order).
 */
export interface MuxActionDefault {
  action: string;
  key: string;
}

export interface KeybindSettings {
  copy: string;
  paste: string;
  select_all: string;
  search: string;
  new_tab: string;
  new_tab_global: string;
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
  profile_selector: string;
}

// ============================================================
// Font Picker Types
// ============================================================

export interface FontListResponse {
  monospace_fonts: string[];
  all_fonts: string[];
  emoji_fonts: string[];
}

export type FontCategory =
  "primary" | "secondary" | "ui" | "markdown-body" | "markdown-code";

// ============================================================
// Profile
// ============================================================

export interface Profile {
  name: string;
  shell_path: string;
  shell_args: string[];
  env_vars: string;
  working_directory: string;
  is_default: boolean;
  ssh_connection_name: string;
  wsl_distro_name: string;
}

// ============================================================
// SSH Connection
// ============================================================

export interface SshOption {
  key: string;
  value: string;
}

export interface SshConnection {
  name: string;
  hostname: string;
  port: number;
  username: string;
  identity_file: string;
  ssh_options: SshOption[];
}

/** Parsed host entry from ~/.ssh/config */
export interface SshConfigHost {
  host: string;
  hostname: string;
  port: number;
  user: string;
  identity_file: string;
}

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

// ============================================================
// Status Bar Types
// ============================================================

export interface StatusbarCustomCommand {
  executable: string;
  interval_ms: number;
}

export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 32;
export const MIN_PADDING = 0;
export const MAX_PADDING = 32;
export const MIN_SCROLLBACK_LINES = 0;
export const MAX_SCROLLBACK_LINES = 100000;
export const MIN_SCROLL_SPEED = 1;
export const MAX_SCROLL_SPEED = 10;
