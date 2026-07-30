use serde::{Deserialize, Serialize};

use crate::types::*;

// ============================================================
// Null-safe Deserialization Helpers
// ============================================================

/// Generates a deserializer function that treats JSON null as a specific default value.
/// Each field with a custom default needs its own deserializer because serde's
/// `deserialize_with` cannot reference the `default` function.
macro_rules! deserialize_null_with {
    ($fn_name:ident, $type:ty, $default_fn:ident) => {
        #[allow(dead_code)]
        fn $fn_name<'de, D>(deserializer: D) -> Result<$type, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let opt = Option::<$type>::deserialize(deserializer)?;
            Ok(opt.unwrap_or_else($default_fn))
        }
    };
}

// Generate per-field null deserializers
deserialize_null_with!(deserialize_null_font_size, u32, default_font_size);
deserialize_null_with!(deserialize_null_padding, u32, default_padding);
deserialize_null_with!(
    deserialize_null_scrollback_lines,
    u32,
    default_scrollback_lines
);
deserialize_null_with!(deserialize_null_scroll_speed, u32, default_scroll_speed);
deserialize_null_with!(
    deserialize_null_alternate_scroll_enabled,
    bool,
    default_alternate_scroll_enabled
);
deserialize_null_with!(deserialize_null_true, bool, default_true);

// For fields where T::default() is correct (String, Vec, enums with #[default])
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + serde::Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

deserialize_null_with!(deserialize_null_language, String, default_language);
deserialize_null_with!(
    deserialize_null_ui_font_family,
    String,
    default_ui_font_family
);
deserialize_null_with!(
    deserialize_null_markdown_font_size,
    u32,
    default_markdown_font_size
);
deserialize_null_with!(
    deserialize_null_markdown_body_font_family,
    String,
    default_markdown_body_font_family
);
deserialize_null_with!(
    deserialize_null_markdown_code_font_family,
    String,
    default_markdown_code_font_family
);
deserialize_null_with!(
    deserialize_null_editor_command,
    String,
    default_editor_command
);
deserialize_null_with!(
    deserialize_null_sftp_max_concurrent_uploads,
    u16,
    default_sftp_max_concurrent_uploads
);
deserialize_null_with!(
    deserialize_null_clipboard_max_size_osc52,
    u32,
    default_clipboard_max_size_osc52
);

// ============================================================
// Default Value Functions
// ============================================================

fn default_font_size() -> u32 {
    13
}
fn default_padding() -> u32 {
    4
}
fn default_scrollback_lines() -> u32 {
    10000
}
fn default_scroll_speed() -> u32 {
    3
}
fn default_alternate_scroll_enabled() -> bool {
    true
}
fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "auto".to_string()
}
fn default_ui_font_family() -> String {
    "Roboto".to_string()
}
fn default_markdown_body_font_family() -> String {
    String::new()
}
fn default_markdown_code_font_family() -> String {
    String::new()
}
fn default_markdown_font_size() -> u32 {
    14
}
fn default_editor_command() -> String {
    "code --goto {file}:{line}:{col}".to_string()
}
fn default_sftp_max_concurrent_uploads() -> u16 {
    4
}
fn default_clipboard_max_size_osc52() -> u32 {
    10 * 1024 * 1024 // 10 MB
}
/// Field-level default for `shift_enter_behavior`, used only when the key
/// is absent from the source JSON (never called for a present key, even
/// an explicit `null` — see `deserialize_null_default`, which handles
/// that case and always returns a real variant). Returns the
/// `ShiftEnterBehavior::Unresolved` sentinel so
/// `AppSettings::apply_migrations` can tell "key absent" apart from "key
/// present and equal to the default" (FR5).
fn shift_enter_behavior_absent_sentinel() -> ShiftEnterBehavior {
    ShiftEnterBehavior::Unresolved
}

// ============================================================
// Keybind Settings (macro-generated)
// ============================================================

/// Generates KeybindSettings struct with serde attributes, default functions,
/// null deserializers, and Default impl from a single definition table.
///
/// Each entry requires both ident and string literal forms of function names
/// because Rust's `concat!()` does not expand inside `#[serde(...)]` attributes.
macro_rules! define_keybinds {
    (
        $(
            $field:ident,
            $default_fn:ident,
            $null_deser:ident,
            $default_fn_str:literal,
            $null_deser_str:literal,
            $default_val:expr
        );* $(;)?
    ) => {
        // Generate default_keybind_*() functions
        $(
            fn $default_fn() -> String {
                $default_val.to_string()
            }
        )*

        // Generate deserialize_null_keybind_*() functions
        $(
            deserialize_null_with!($null_deser, String, $default_fn);
        )*

        /// Keybind settings structure.
        /// All fields use serde(default) + null deserializer for null handling.
        /// Generated by define_keybinds! macro.
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
        pub struct KeybindSettings {
            $(
                #[serde(
                    default = $default_fn_str,
                    deserialize_with = $null_deser_str
                )]
                pub $field: String,
            )*
        }

        impl Default for KeybindSettings {
            fn default() -> Self {
                Self {
                    $( $field: $default_fn(), )*
                }
            }
        }
    };
}

define_keybinds! {
    copy,             default_keybind_copy,             deserialize_null_keybind_copy,
                      "default_keybind_copy",           "deserialize_null_keybind_copy",
                      "Ctrl+Shift+C";
    paste,            default_keybind_paste,            deserialize_null_keybind_paste,
                      "default_keybind_paste",          "deserialize_null_keybind_paste",
                      "Ctrl+Shift+V";
    select_all,       default_keybind_select_all,       deserialize_null_keybind_select_all,
                      "default_keybind_select_all",     "deserialize_null_keybind_select_all",
                      "Ctrl+Shift+A";
    search,           default_keybind_search,           deserialize_null_keybind_search,
                      "default_keybind_search",         "deserialize_null_keybind_search",
                      "Ctrl+Shift+F";
    new_tab,          default_keybind_new_tab,          deserialize_null_keybind_new_tab,
                      "default_keybind_new_tab",        "deserialize_null_keybind_new_tab",
                      "Ctrl+Shift+T";
    new_tab_global,   default_keybind_new_tab_global,   deserialize_null_keybind_new_tab_global,
                      "default_keybind_new_tab_global", "deserialize_null_keybind_new_tab_global",
                      "Ctrl+Shift+G";
    close_tab,        default_keybind_close_tab,        deserialize_null_keybind_close_tab,
                      "default_keybind_close_tab",      "deserialize_null_keybind_close_tab",
                      "Ctrl+Shift+W";
    next_tab,         default_keybind_next_tab,         deserialize_null_keybind_next_tab,
                      "default_keybind_next_tab",       "deserialize_null_keybind_next_tab",
                      "Ctrl+PageDown";
    prev_tab,         default_keybind_prev_tab,         deserialize_null_keybind_prev_tab,
                      "default_keybind_prev_tab",       "deserialize_null_keybind_prev_tab",
                      "Ctrl+PageUp";
    zoom_in,          default_keybind_zoom_in,          deserialize_null_keybind_zoom_in,
                      "default_keybind_zoom_in",        "deserialize_null_keybind_zoom_in",
                      "Ctrl+Plus";
    zoom_out,         default_keybind_zoom_out,         deserialize_null_keybind_zoom_out,
                      "default_keybind_zoom_out",       "deserialize_null_keybind_zoom_out",
                      "Ctrl+Minus";
    zoom_reset,       default_keybind_zoom_reset,       deserialize_null_keybind_zoom_reset,
                      "default_keybind_zoom_reset",     "deserialize_null_keybind_zoom_reset",
                      "Ctrl+0";
    toggle_fullscreen, default_keybind_toggle_fullscreen, deserialize_null_keybind_toggle_fullscreen,
                      "default_keybind_toggle_fullscreen", "deserialize_null_keybind_toggle_fullscreen",
                      "F11";
    open_settings,    default_keybind_open_settings,    deserialize_null_keybind_open_settings,
                      "default_keybind_open_settings",  "deserialize_null_keybind_open_settings",
                      "Ctrl+,";
    toggle_tab_bar,   default_keybind_toggle_tab_bar,   deserialize_null_keybind_toggle_tab_bar,
                      "default_keybind_toggle_tab_bar", "deserialize_null_keybind_toggle_tab_bar",
                      "Ctrl+Shift+B";
    jump_to_prev_prompt, default_keybind_jump_to_prev_prompt, deserialize_null_keybind_jump_to_prev_prompt,
                      "default_keybind_jump_to_prev_prompt", "deserialize_null_keybind_jump_to_prev_prompt",
                      "Ctrl+Shift+ArrowUp";
    jump_to_next_prompt, default_keybind_jump_to_next_prompt, deserialize_null_keybind_jump_to_next_prompt,
                      "default_keybind_jump_to_next_prompt", "deserialize_null_keybind_jump_to_next_prompt",
                      "Ctrl+Shift+ArrowDown";
    profile_selector, default_keybind_profile_selector, deserialize_null_keybind_profile_selector,
                      "default_keybind_profile_selector", "deserialize_null_keybind_profile_selector",
                      "Ctrl+Shift+P";
}

// ============================================================
// SSH Connection
// ============================================================

fn default_ssh_port() -> u16 {
    22
}

deserialize_null_with!(deserialize_null_ssh_port, u16, default_ssh_port);

/// Key-value pair for SSH -o options.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshOption {
    pub key: String,
    pub value: String,
}

/// SSH connection entry for remote host connections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshConnection {
    pub name: String,
    pub hostname: String,
    #[serde(
        default = "default_ssh_port",
        deserialize_with = "deserialize_null_ssh_port"
    )]
    pub port: u16,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub username: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub identity_file: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ssh_options: Vec<SshOption>,
    /// Legacy field for backward compatibility. Read during deserialization but never serialized.
    #[serde(default, skip_serializing)]
    pub extra_options: String,
}

// ============================================================
// Profile
// ============================================================

/// Terminal profile for per-session shell configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_path: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_args: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub env_vars: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub working_directory: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub is_default: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ssh_connection_name: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub wsl_distro_name: String,
}

// ============================================================
// Settings Structs
// ============================================================

/// Application settings structure for JSON serialization.
/// All fields use serde(default) + deserialize_null_default for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    // Font
    #[serde(
        default = "default_font_size",
        deserialize_with = "deserialize_null_font_size"
    )]
    pub font_size: u32,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub font_family_primary: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub font_family_secondary: String,

    /// Legacy field for backward compatibility. Read during deserialization but never serialized.
    /// Private to the crate — loaders run the migration via
    /// [`AppSettings::apply_migrations`] instead of touching this directly.
    #[serde(default, skip_serializing)]
    pub(crate) font_family: String,
    /// Deprecated: line_height is no longer configurable (always uses font metrics).
    /// Kept for backward compatibility with existing config files.
    #[serde(default, skip_serializing)]
    pub(crate) _line_height: Option<f32>,

    // Theme / Color
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ui_theme: UiTheme,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ui_theme_preset: UiThemePreset,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub terminal_color_scheme: String,

    // Layout
    #[serde(
        default = "default_padding",
        deserialize_with = "deserialize_null_padding"
    )]
    pub padding: u32,
    #[serde(
        default = "default_scrollback_lines",
        deserialize_with = "deserialize_null_scrollback_lines"
    )]
    pub scrollback_lines: u32,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub show_scrollbar: ScrollbarMode,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub show_tab_bar: bool,

    // Terminal
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_path: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub shell_args: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub cursor_style: CursorStyle,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub cursor_blink: bool,
    #[serde(
        default = "default_scroll_speed",
        deserialize_with = "deserialize_null_scroll_speed"
    )]
    pub scroll_speed: u32,
    /// DECSET 1007 (alternate_scroll): when true, the host translates
    /// AltScreen wheel events into arrow-key bytes so AltScreen apps
    /// scroll their own log. Default `true`. The terminal-side mode bit
    /// (`MODE_ALTERNATE_SCROLL`) is the runtime gate; this setting is
    /// the user-facing opt-out layered on top.
    #[serde(
        default = "default_alternate_scroll_enabled",
        deserialize_with = "deserialize_null_alternate_scroll_enabled"
    )]
    pub alternate_scroll_enabled: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub bell_action: BellAction,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub url_detection: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub copy_on_select: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub fold_enabled: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub file_path_detection: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub bold_brightens_ansi_colors: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub middle_click_paste: bool,
    #[serde(
        default = "shift_enter_behavior_absent_sentinel",
        deserialize_with = "deserialize_null_default"
    )]
    pub shift_enter_behavior: ShiftEnterBehavior,
    /// Legacy field for backward compatibility (FR5 migration input).
    /// Read during deserialization but never serialized; folded into
    /// `shift_enter_behavior` by [`AppSettings::apply_migrations`].
    #[serde(default, skip_serializing)]
    pub(crate) shift_enter_as_alt_enter: Option<bool>,
    #[serde(skip)]
    pub ambiguous_width: bool,
    #[serde(
        default = "default_editor_command",
        deserialize_with = "deserialize_null_editor_command"
    )]
    pub editor_command: String,

    // IME
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub skk_mode: bool,

    // Notification
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub notification_enabled: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub tab_activity_indicator: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub notify_on_process_exit: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub notify_on_output: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub notify_on_bell: bool,
    /// task0007: desktop notification for a blocked/done agent-status
    /// transition on a non-visible pane.
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub agent_status_notifications: bool,

    // Keybinds
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub keybinds: KeybindSettings,

    // Language
    #[serde(
        default = "default_language",
        deserialize_with = "deserialize_null_language"
    )]
    pub language: String,

    // UI Font
    #[serde(
        default = "default_ui_font_family",
        deserialize_with = "deserialize_null_ui_font_family"
    )]
    pub ui_font_family: String,

    // Custom Color Schemes
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub custom_color_schemes: Vec<UserColorScheme>,

    // Profiles
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub profiles: Vec<Profile>,

    // SSH
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ssh_command_path: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ssh_connections: Vec<SshConnection>,

    // SFTP
    #[serde(
        default = "default_sftp_max_concurrent_uploads",
        deserialize_with = "deserialize_null_sftp_max_concurrent_uploads"
    )]
    pub sftp_max_concurrent_uploads: u16,

    // OSC 52 Clipboard
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub clipboard_read_osc52: bool,
    #[serde(
        default = "default_clipboard_max_size_osc52",
        deserialize_with = "deserialize_null_clipboard_max_size_osc52"
    )]
    pub clipboard_max_size_osc52: u32,

    // Log Recording
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub log_recording_enabled: bool,

    // Markdown Viewer
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub markdown_theme_follow_ui: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub markdown_theme: UiTheme,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub markdown_theme_preset: UiThemePreset,
    #[serde(
        default = "default_markdown_body_font_family",
        deserialize_with = "deserialize_null_markdown_body_font_family"
    )]
    pub markdown_body_font_family: String,
    #[serde(
        default = "default_markdown_code_font_family",
        deserialize_with = "deserialize_null_markdown_code_font_family"
    )]
    pub markdown_code_font_family: String,
    #[serde(
        default = "default_markdown_font_size",
        deserialize_with = "deserialize_null_markdown_font_size"
    )]
    pub markdown_font_size: u32,

    // Mux (multiplexer) settings
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub mux: MuxSettings,

    // Status Bar
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_enabled: bool,
    #[serde(
        default = "default_statusbar_app_line1_left",
        deserialize_with = "deserialize_null_statusbar_app_line1_left"
    )]
    pub statusbar_app_line1_left: String,
    #[serde(
        default = "default_statusbar_app_line1_right",
        deserialize_with = "deserialize_null_statusbar_app_line1_right"
    )]
    pub statusbar_app_line1_right: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_app_line2_left: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_app_line2_right: String,
    #[serde(
        default = "default_statusbar_time_format",
        deserialize_with = "deserialize_null_statusbar_time_format"
    )]
    pub statusbar_time_format: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_font_size: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_custom_commands: std::collections::HashMap<String, StatusbarCustomCommand>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub statusbar_refresh_rates: std::collections::HashMap<String, u64>,
}

impl AppSettings {
    /// Run the legacy-field migrations after deserialization. Every loader
    /// (the Tauri build's `load_settings` and native-poc's child settings
    /// window) MUST call this before using the struct, so both binaries
    /// interpret the same `settings.json` identically.
    ///
    /// Returns `true` when any legacy field was migrated to a new key,
    /// signaling to the caller that it should persist the new schema.
    ///
    /// Currently migrated:
    /// - `font_family` → `font_family_primary` (when the new key is empty).
    /// - `shift_enter_as_alt_enter` → `shift_enter_behavior` (only when the
    ///   new key was truly absent from the source JSON; see the field's
    ///   migration below).
    pub fn apply_migrations(&mut self) -> bool {
        let mut migrated = false;

        // Legacy primary-font key.
        if !self.font_family.is_empty() && self.font_family_primary.is_empty() {
            self.font_family_primary = std::mem::take(&mut self.font_family);
            migrated = true;
        } else if !self.font_family.is_empty() {
            self.font_family.clear();
            migrated = true;
        }

        // Legacy shift-enter boolean. The new key wins whenever it was
        // PRESENT in the source JSON — including an explicit `null`, which
        // `deserialize_null_default` already resolves to a real variant
        // (the wire default) before this runs. Only the deserialize-time
        // `Unresolved` sentinel (set by `shift_enter_behavior_absent_
        // sentinel`, and only reachable when the key was fully absent)
        // means the legacy boolean should be folded in (FR5). The legacy
        // key is never serialized again (`skip_serializing`), so
        // `migrated` is reported whenever it was present so the caller can
        // rewrite the file with the new shape, regardless of whether it
        // actually won.
        let shift_enter_behavior_was_absent =
            self.shift_enter_behavior == ShiftEnterBehavior::Unresolved;
        if let Some(legacy) = self.shift_enter_as_alt_enter.take() {
            if shift_enter_behavior_was_absent {
                self.shift_enter_behavior = if legacy {
                    ShiftEnterBehavior::AltEnter
                } else {
                    ShiftEnterBehavior::None
                };
            }
            migrated = true;
        }
        // No legacy key to fold in either: resolve the sentinel to the
        // real wire default so it never survives past this method.
        if self.shift_enter_behavior == ShiftEnterBehavior::Unresolved {
            self.shift_enter_behavior = ShiftEnterBehavior::default();
        }

        migrated
    }

    /// Persistence-side migration hook intended for loaders that
    /// understand `.bak` rotation. Currently a thin alias for
    /// [`Self::apply_migrations`] — exposed under a stable name so
    /// callers can wire a one-time `.bak` write next to the migration
    /// boundary without depending on the historical method name.
    pub fn migrate_legacy(&mut self) -> bool {
        self.apply_migrations()
    }
}

/// Multiplexer settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxSettings {
    #[serde(default = "default_mux_prefix")]
    pub prefix: String,
    #[serde(default)]
    pub tab_always_expand: bool,
    /// Window list display mode: `true` (default) shows the floating
    /// overlay card, `false` the always-present side panel. Absent or
    /// explicit-null keys resolve to the overlay default (task0001 D3);
    /// an explicit `false` always selects the persistent panel.
    #[serde(default = "default_true")]
    pub window_sidebar_overlay: bool,
    #[serde(default)]
    pub tmux_conf_imported: bool,
    #[serde(default)]
    pub keybinds: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub statusbar: MuxStatusbarSettings,
}

// ============================================================
// Status Bar
// ============================================================

fn default_statusbar_app_line1_left() -> String {
    "{time}".to_string()
}
fn default_statusbar_app_line1_right() -> String {
    "{cwd}".to_string()
}
fn default_statusbar_time_format() -> String {
    "HH:mm:ss".to_string()
}
fn default_statusbar_custom_command_interval() -> u64 {
    1000
}

deserialize_null_with!(
    deserialize_null_statusbar_app_line1_left,
    String,
    default_statusbar_app_line1_left
);
deserialize_null_with!(
    deserialize_null_statusbar_app_line1_right,
    String,
    default_statusbar_app_line1_right
);
deserialize_null_with!(
    deserialize_null_statusbar_time_format,
    String,
    default_statusbar_time_format
);
/// Custom command definition for status bar variables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusbarCustomCommand {
    /// Single executable path only (no arguments, no shell expansion).
    pub executable: String,
    /// Polling interval in milliseconds.
    #[serde(default = "default_statusbar_custom_command_interval")]
    pub interval_ms: u64,
}

fn default_mux_prefix() -> String {
    "Ctrl+Z".to_string()
}

impl Default for MuxSettings {
    fn default() -> Self {
        Self {
            prefix: default_mux_prefix(),
            tab_always_expand: false,
            window_sidebar_overlay: true,
            tmux_conf_imported: false,
            keybinds: std::collections::HashMap::new(),
            statusbar: MuxStatusbarSettings::default(),
        }
    }
}

fn default_mux_statusbar_interval() -> u64 {
    5000
}

/// Mux status bar settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MuxStatusbarSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub left: String,
    #[serde(default)]
    pub right: String,
    #[serde(default)]
    pub commands: std::collections::HashMap<String, MuxStatusbarCommand>,
}

/// A registered command for the mux status bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxStatusbarCommand {
    pub executable: String,
    #[serde(default = "default_mux_statusbar_interval")]
    pub interval_ms: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_family_primary: String::new(),
            font_family_secondary: String::new(),
            font_family: String::new(),
            _line_height: None,
            ui_theme: UiTheme::default(),
            ui_theme_preset: UiThemePreset::default(),
            terminal_color_scheme: String::new(),
            padding: default_padding(),
            scrollback_lines: default_scrollback_lines(),
            show_scrollbar: ScrollbarMode::default(),
            show_tab_bar: default_true(),
            shell_path: String::new(),
            shell_args: Vec::new(),
            cursor_style: CursorStyle::default(),
            cursor_blink: default_true(),
            scroll_speed: default_scroll_speed(),
            alternate_scroll_enabled: default_alternate_scroll_enabled(),
            bell_action: BellAction::default(),
            url_detection: default_true(),
            copy_on_select: false,
            keybinds: KeybindSettings::default(),
            language: default_language(),
            ui_font_family: default_ui_font_family(),
            custom_color_schemes: Vec::new(),
            profiles: Vec::new(),
            ssh_command_path: String::new(),
            ssh_connections: Vec::new(),
            sftp_max_concurrent_uploads: default_sftp_max_concurrent_uploads(),
            markdown_theme_follow_ui: default_true(),
            markdown_theme: UiTheme::default(),
            markdown_theme_preset: UiThemePreset::default(),
            markdown_body_font_family: default_markdown_body_font_family(),
            markdown_code_font_family: default_markdown_code_font_family(),
            markdown_font_size: default_markdown_font_size(),
            fold_enabled: default_true(),
            file_path_detection: default_true(),
            bold_brightens_ansi_colors: default_true(),
            middle_click_paste: default_true(),
            shift_enter_behavior: ShiftEnterBehavior::default(),
            shift_enter_as_alt_enter: None,
            ambiguous_width: default_true(),
            editor_command: default_editor_command(),
            skk_mode: default_true(),
            notification_enabled: default_true(),
            tab_activity_indicator: default_true(),
            notify_on_process_exit: default_true(),
            notify_on_output: false,
            notify_on_bell: default_true(),
            agent_status_notifications: default_true(),
            clipboard_read_osc52: default_true(),
            clipboard_max_size_osc52: default_clipboard_max_size_osc52(),
            log_recording_enabled: false,
            mux: MuxSettings::default(),
            statusbar_enabled: false,
            statusbar_app_line1_left: default_statusbar_app_line1_left(),
            statusbar_app_line1_right: default_statusbar_app_line1_right(),
            statusbar_app_line2_left: String::new(),
            statusbar_app_line2_right: String::new(),
            statusbar_time_format: default_statusbar_time_format(),
            statusbar_font_size: None,
            statusbar_custom_commands: std::collections::HashMap::new(),
            statusbar_refresh_rates: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_mux_statusbar_settings_default() {
        let settings = MuxStatusbarSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.left, "");
        assert_eq!(settings.right, "");
        assert!(settings.commands.is_empty());
    }

    #[test]
    fn test_mux_statusbar_settings_full_config() {
        let json = r#"{
            "enabled": true,
            "left": "{hostname} | {cmd:git_branch}",
            "right": "{cwd}",
            "commands": {
                "git_branch": {
                    "executable": "/usr/bin/git-branch-name",
                    "interval_ms": 3000
                }
            }
        }"#;
        let settings: MuxStatusbarSettings = serde_json::from_str(json).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.left, "{hostname} | {cmd:git_branch}");
        assert_eq!(settings.right, "{cwd}");
        assert_eq!(settings.commands.len(), 1);
        let cmd = settings.commands.get("git_branch").unwrap();
        assert_eq!(cmd.executable, "/usr/bin/git-branch-name");
        assert_eq!(cmd.interval_ms, 3000);
    }

    #[test]
    fn test_mux_statusbar_settings_missing_fields() {
        let json = r#"{"enabled": true}"#;
        let settings: MuxStatusbarSettings = serde_json::from_str(json).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.left, "");
        assert_eq!(settings.right, "");
        assert!(settings.commands.is_empty());
    }

    #[test]
    fn test_mux_statusbar_command_default_interval() {
        let json = r#"{"executable": "/usr/bin/date"}"#;
        let cmd: MuxStatusbarCommand = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.executable, "/usr/bin/date");
        assert_eq!(cmd.interval_ms, 5000);
    }

    #[test]
    fn test_mux_settings_with_statusbar() {
        let json = r#"{
            "prefix": "ctrl+a",
            "statusbar": {
                "enabled": true,
                "left": "test",
                "right": "right",
                "commands": {}
            }
        }"#;
        let settings: MuxSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.prefix, "ctrl+a");
        assert!(settings.statusbar.enabled);
        assert_eq!(settings.statusbar.left, "test");
    }

    #[test]
    fn test_mux_settings_without_statusbar_uses_default() {
        let json = r#"{"prefix": "ctrl+b"}"#;
        let settings: MuxSettings = serde_json::from_str(json).unwrap();
        assert!(!settings.statusbar.enabled);
        assert_eq!(settings.statusbar.left, "");
    }

    // ── window_sidebar_overlay (task0001 AC-1/AC-3/AC-4/AC-5) ───────────

    #[test]
    fn test_mux_settings_window_sidebar_overlay_missing_defaults_overlay() {
        // AC-1: a settings JSON without the field resolves to the overlay
        // display mode (`true`).
        let json = r#"{"prefix": "ctrl+b"}"#;
        let settings: MuxSettings = serde_json::from_str(json).unwrap();
        assert!(settings.window_sidebar_overlay);
    }

    #[test]
    fn test_mux_settings_window_sidebar_overlay_explicit_false_is_persistent() {
        // AC-3: an explicit `false` resolves to the persistent display
        // mode, overriding the overlay default.
        let json = r#"{"window_sidebar_overlay": false}"#;
        let settings: MuxSettings = serde_json::from_str(json).unwrap();
        assert!(!settings.window_sidebar_overlay);
    }

    #[test]
    fn test_mux_settings_window_sidebar_overlay_explicit_true_is_overlay() {
        // AC-4: `true` resolves to the overlay display mode.
        let json = r#"{"window_sidebar_overlay": true}"#;
        let settings: MuxSettings = serde_json::from_str(json).unwrap();
        assert!(settings.window_sidebar_overlay);
    }

    #[test]
    fn test_mux_settings_window_sidebar_overlay_round_trips() {
        // AC-5: serializing default settings then re-loading them
        // round-trips the overlay display mode, and a round trip of an
        // explicitly-persistent value preserves the persistent mode.
        let default_settings = MuxSettings::default();
        let json = serde_json::to_string(&default_settings).unwrap();
        let restored: MuxSettings = serde_json::from_str(&json).unwrap();
        assert!(restored.window_sidebar_overlay);

        let mut persistent_settings = MuxSettings::default();
        persistent_settings.window_sidebar_overlay = false;
        let json = serde_json::to_string(&persistent_settings).unwrap();
        let restored: MuxSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.window_sidebar_overlay);
    }

    /// Full-coverage round-trip: the exhaustive struct literal (no
    /// `..Default::default()`) forces a compile error here whenever a new
    /// field is added, so the round-trip assertions stay complete. Lives in
    /// this crate (moved from src-tauri's config tests) because the literal
    /// must name the crate-private legacy fields.
    #[test]
    fn test_round_trip_preserves_all_fields() {
        let settings = AppSettings {
            font_size: 16,
            font_family_primary: "Fira Code".to_string(),
            font_family_secondary: "Noto Sans JP".to_string(),
            font_family: String::new(),
            _line_height: None,
            ui_theme: UiTheme::Dark,
            ui_theme_preset: UiThemePreset::Blue,
            terminal_color_scheme: "monokai".to_string(),
            padding: 8,
            scrollback_lines: 5000,
            show_scrollbar: ScrollbarMode::Always,
            show_tab_bar: false,
            shell_path: "/bin/zsh".to_string(),
            shell_args: vec!["--login".to_string(), "-i".to_string()],
            cursor_style: CursorStyle::Bar,
            cursor_blink: false,
            scroll_speed: 5,
            alternate_scroll_enabled: false,
            bell_action: BellAction::None,
            url_detection: false,
            copy_on_select: true,
            keybinds: KeybindSettings {
                copy: "Ctrl+C".to_string(),
                paste: "Ctrl+V".to_string(),
                ..KeybindSettings::default()
            },
            language: "ja".to_string(),
            ui_font_family: "Noto Sans".to_string(),
            custom_color_schemes: Vec::new(),
            profiles: vec![Profile {
                name: "Dev".to_string(),
                shell_path: "/bin/zsh".to_string(),
                shell_args: vec!["--login".to_string()],
                env_vars: "FOO=bar\nBAZ=qux".to_string(),
                working_directory: "/home/user/projects".to_string(),
                is_default: true,
                ssh_connection_name: String::new(),
                wsl_distro_name: String::new(),
            }],
            ssh_command_path: String::new(),
            ssh_connections: Vec::new(),
            sftp_max_concurrent_uploads: 4,
            clipboard_read_osc52: true,
            clipboard_max_size_osc52: 10 * 1024 * 1024,
            log_recording_enabled: false,
            markdown_theme_follow_ui: false,
            markdown_theme: UiTheme::Light,
            markdown_theme_preset: UiThemePreset::Green,
            markdown_body_font_family: "Noto Sans".to_string(),
            markdown_code_font_family: "Fira Code".to_string(),
            markdown_font_size: 16,
            fold_enabled: false,
            file_path_detection: false,
            bold_brightens_ansi_colors: false,
            middle_click_paste: false,
            shift_enter_behavior: ShiftEnterBehavior::KittyCsiU,
            shift_enter_as_alt_enter: None,
            ambiguous_width: false,
            editor_command: "vim +{line} {file}".to_string(),
            skk_mode: false,
            notification_enabled: false,
            tab_activity_indicator: false,
            notify_on_process_exit: false,
            notify_on_output: true,
            notify_on_bell: false,
            agent_status_notifications: false,
            mux: MuxSettings::default(),
            statusbar_enabled: true,
            statusbar_app_line1_left: "{git_branch}".to_string(),
            statusbar_app_line1_right: "{time}".to_string(),
            statusbar_app_line2_left: "line2".to_string(),
            statusbar_app_line2_right: "right2".to_string(),
            statusbar_time_format: "HH:mm".to_string(),
            statusbar_font_size: Some(11.0),
            statusbar_custom_commands: std::collections::HashMap::new(),
            statusbar_refresh_rates: std::collections::HashMap::new(),
        };

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.font_size, 16);
        assert_eq!(restored.font_family_primary, "Fira Code");
        assert_eq!(restored.font_family_secondary, "Noto Sans JP");
        assert_eq!(restored.ui_theme, UiTheme::Dark);
        assert_eq!(restored.ui_theme_preset, UiThemePreset::Blue);
        assert_eq!(restored.terminal_color_scheme, "monokai");
        assert_eq!(restored.padding, 8);
        assert_eq!(restored.scrollback_lines, 5000);
        assert_eq!(restored.show_scrollbar, ScrollbarMode::Always);
        assert_eq!(restored.shell_path, "/bin/zsh");
        assert_eq!(restored.shell_args, vec!["--login", "-i"]);
        assert_eq!(restored.cursor_style, CursorStyle::Bar);
        assert!(!restored.cursor_blink);
        assert_eq!(restored.scroll_speed, 5);
        assert!(!restored.alternate_scroll_enabled);
        assert_eq!(restored.bell_action, BellAction::None);
        assert!(!restored.url_detection);
        assert!(restored.copy_on_select);
        assert!(!restored.file_path_detection);
        assert!(!restored.bold_brightens_ansi_colors);
        assert!(!restored.middle_click_paste);
        assert_eq!(restored.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
        assert_eq!(restored.editor_command, "vim +{line} {file}");
        assert!(!restored.skk_mode);
        assert_eq!(restored.keybinds.copy, "Ctrl+C");
        assert_eq!(restored.keybinds.paste, "Ctrl+V");
        assert_eq!(restored.keybinds.select_all, "Ctrl+Shift+A");
        assert_eq!(restored.ui_font_family, "Noto Sans");
        assert_eq!(restored.language, "ja");
        assert!(!restored.show_tab_bar);
        assert!(!restored.markdown_theme_follow_ui);
        assert_eq!(restored.markdown_theme, UiTheme::Light);
        assert_eq!(restored.markdown_theme_preset, UiThemePreset::Green);
        assert_eq!(restored.markdown_body_font_family, "Noto Sans");
        assert_eq!(restored.markdown_code_font_family, "Fira Code");
        assert_eq!(restored.markdown_font_size, 16);
        assert!(!restored.notification_enabled);
        assert!(!restored.tab_activity_indicator);
        assert!(!restored.notify_on_process_exit);
        assert!(restored.notify_on_output);
        assert!(!restored.notify_on_bell);
        assert!(!restored.agent_status_notifications);
        assert_eq!(restored.profiles.len(), 1);
        assert_eq!(restored.profiles[0].name, "Dev");
        assert_eq!(restored.profiles[0].shell_path, "/bin/zsh");
        assert_eq!(restored.profiles[0].shell_args, vec!["--login"]);
        assert_eq!(restored.profiles[0].env_vars, "FOO=bar\nBAZ=qux");
        assert_eq!(
            restored.profiles[0].working_directory,
            "/home/user/projects"
        );
        assert!(restored.profiles[0].is_default);
    }

    #[test]
    fn apply_migrations_moves_legacy_font_family_when_primary_unset() {
        let mut s: AppSettings = serde_json::from_str(r#"{"font_family": "Legacy Mono"}"#).unwrap();
        s.apply_migrations();
        assert_eq!(s.font_family_primary, "Legacy Mono");
        assert!(s.font_family.is_empty());
    }

    #[test]
    fn apply_migrations_drops_legacy_font_family_when_primary_set() {
        let mut s: AppSettings = serde_json::from_str(
            r#"{"font_family": "Legacy Mono", "font_family_primary": "New Mono"}"#,
        )
        .unwrap();
        s.apply_migrations();
        assert_eq!(s.font_family_primary, "New Mono");
        assert!(s.font_family.is_empty());
    }

    // ── shift_enter_behavior (task0003 AC-1) ────────────────────────────

    #[test]
    fn shift_enter_behavior_defaults_to_alt_enter() {
        assert_eq!(
            AppSettings::default().shift_enter_behavior,
            ShiftEnterBehavior::AltEnter
        );
    }

    #[test]
    fn shift_enter_behavior_each_wire_value_round_trips_through_serde() {
        for (json_value, variant) in [
            ("none", ShiftEnterBehavior::None),
            ("alt_enter", ShiftEnterBehavior::AltEnter),
            ("kitty_csi_u", ShiftEnterBehavior::KittyCsiU),
            ("lf", ShiftEnterBehavior::Lf),
        ] {
            let s: AppSettings =
                serde_json::from_str(&format!(r#"{{"shift_enter_behavior": "{json_value}"}}"#))
                    .unwrap();
            assert_eq!(s.shift_enter_behavior, variant);
            let v = serde_json::to_value(&s).unwrap();
            assert_eq!(v["shift_enter_behavior"], json!(json_value));
        }
    }

    #[test]
    fn shift_enter_behavior_null_resolves_to_default() {
        let s: AppSettings = serde_json::from_str(r#"{"shift_enter_behavior": null}"#).unwrap();
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    // ── agent_status_notifications (task0007 AC-5) ──────────────────────

    #[test]
    fn agent_status_notifications_defaults_to_true() {
        assert!(AppSettings::default().agent_status_notifications);
    }

    #[test]
    fn agent_status_notifications_missing_key_resolves_to_default_true() {
        let s: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(s.agent_status_notifications);
    }

    #[test]
    fn agent_status_notifications_null_resolves_to_default_true() {
        let s: AppSettings =
            serde_json::from_str(r#"{"agent_status_notifications": null}"#).unwrap();
        assert!(s.agent_status_notifications);
    }

    #[test]
    fn agent_status_notifications_explicit_false_deserializes() {
        let s: AppSettings =
            serde_json::from_str(r#"{"agent_status_notifications": false}"#).unwrap();
        assert!(!s.agent_status_notifications);
    }

    #[test]
    fn serializing_default_settings_never_emits_legacy_shift_enter_key() {
        let v = serde_json::to_value(AppSettings::default()).unwrap();
        assert!(
            v.get("shift_enter_as_alt_enter").is_none(),
            "legacy shift_enter_as_alt_enter key must not be serialized"
        );
        assert_eq!(v["shift_enter_behavior"], json!("alt_enter"));
    }

    #[test]
    fn apply_migrations_maps_legacy_true_to_alt_enter_when_new_key_absent() {
        let mut s: AppSettings =
            serde_json::from_str(r#"{"shift_enter_as_alt_enter": true}"#).unwrap();
        assert!(s.apply_migrations());
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn apply_migrations_maps_legacy_false_to_none_when_new_key_absent() {
        let mut s: AppSettings =
            serde_json::from_str(r#"{"shift_enter_as_alt_enter": false}"#).unwrap();
        assert!(s.apply_migrations());
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
    }

    #[test]
    fn apply_migrations_new_key_present_wins_over_legacy() {
        let mut s: AppSettings = serde_json::from_str(
            r#"{"shift_enter_behavior": "kitty_csi_u", "shift_enter_as_alt_enter": false}"#,
        )
        .unwrap();
        s.apply_migrations();
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
    }

    /// AC-1 (task0004 rework, rd2-apply-migrations-precedence): the new
    /// key wins even when its PRESENT value happens to equal the wire
    /// default (`alt_enter`) — this is the case the old "resolved value
    /// equals the default means absent" heuristic got wrong, since a
    /// present-and-default value is indistinguishable from absent under
    /// that heuristic.
    #[test]
    fn apply_migrations_new_key_present_as_default_value_wins_over_legacy() {
        let mut s: AppSettings = serde_json::from_str(
            r#"{"shift_enter_behavior": "alt_enter", "shift_enter_as_alt_enter": false}"#,
        )
        .unwrap();
        s.apply_migrations();
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    /// AC-2 (task0004 rework): an explicit `null` for the new key counts
    /// as PRESENT (it resolves to the wire default), so it still wins
    /// over the legacy boolean rather than falling through to it.
    #[test]
    fn apply_migrations_new_key_present_as_explicit_null_wins_over_legacy() {
        let mut s: AppSettings = serde_json::from_str(
            r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": false}"#,
        )
        .unwrap();
        s.apply_migrations();
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn apply_migrations_no_legacy_key_leaves_default_unmigrated() {
        let mut s = AppSettings::default();
        assert!(!s.apply_migrations());
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    /// Companion to `apply_migrations_no_legacy_key_leaves_default_
    /// unmigrated` for the JSON-deserialize path specifically: when
    /// BOTH keys are absent from the source JSON, the deserialize-time
    /// `Unresolved` sentinel must still resolve to the real wire default
    /// (not leak past `apply_migrations`).
    #[test]
    fn apply_migrations_neither_key_present_resolves_sentinel_to_default() {
        let mut s: AppSettings = serde_json::from_str(r#"{}"#).unwrap();
        assert!(!s.apply_migrations());
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
        // The sentinel must not survive serialization either.
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["shift_enter_behavior"], json!("alt_enter"));
    }
}
