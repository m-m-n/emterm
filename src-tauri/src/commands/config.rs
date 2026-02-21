use rust_i18n::t;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

// ============================================================
// Validation Constants
// ============================================================

// Font
pub const MIN_FONT_SIZE: u32 = 8;
pub const MAX_FONT_SIZE: u32 = 32;

// Layout
pub const MIN_PADDING: u32 = 0;
pub const MAX_PADDING: u32 = 32;
pub const MIN_SCROLLBACK_LINES: u32 = 0;
pub const MAX_SCROLLBACK_LINES: u32 = 100000;

// Scroll
pub const MIN_SCROLL_SPEED: u32 = 1;
pub const MAX_SCROLL_SPEED: u32 = 10;

// ============================================================
// Enum Types
// ============================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BellAction {
    Sound,
    #[default]
    Visual,
    None,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScrollbarMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UiThemePreset {
    #[default]
    Purple,
    Blue,
    Green,
    Orange,
}

// ============================================================
// Null-safe Deserialization Helpers
// ============================================================

/// Generates a deserializer function that treats JSON null as a specific default value.
/// Each field with a custom default needs its own deserializer because serde's
/// `deserialize_with` cannot reference the `default` function.
macro_rules! deserialize_null_with {
    ($fn_name:ident, $type:ty, $default_fn:ident) => {
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

// ============================================================
// User Color Scheme
// ============================================================

/// User-defined terminal color scheme.
/// Stored in settings.json under custom_color_schemes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection: String,
    pub ansi_colors: Vec<String>,
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
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub font_family_emoji: String,

    /// Legacy field for backward compatibility. Read during deserialization but never serialized.
    #[serde(default, skip_serializing)]
    font_family: String,
    /// Deprecated: line_height is no longer configurable (always uses font metrics).
    /// Kept for backward compatibility with existing config files.
    #[serde(default, skip_serializing)]
    _line_height: Option<f32>,

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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_family_primary: String::new(),
            font_family_secondary: String::new(),
            font_family_emoji: String::new(),
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
            bell_action: BellAction::default(),
            url_detection: default_true(),
            copy_on_select: false,
            keybinds: KeybindSettings::default(),
            language: default_language(),
            ui_font_family: default_ui_font_family(),
            custom_color_schemes: Vec::new(),
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
            editor_command: default_editor_command(),
            skk_mode: default_true(),
            notification_enabled: default_true(),
            tab_activity_indicator: default_true(),
            notify_on_process_exit: default_true(),
            notify_on_output: false,
            notify_on_bell: default_true(),
        }
    }
}

// ============================================================
// Validation
// ============================================================

/// Validates settings values and returns an error message if invalid.
fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if settings.font_size < MIN_FONT_SIZE || settings.font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.fontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    if settings.padding > MAX_PADDING {
        return Err(t!("validation.padding", min = MIN_PADDING, max = MAX_PADDING).to_string());
    }

    if settings.scrollback_lines > MAX_SCROLLBACK_LINES {
        return Err(t!(
            "validation.scrollbackLines",
            min = MIN_SCROLLBACK_LINES,
            max = MAX_SCROLLBACK_LINES
        )
        .to_string());
    }

    if settings.scroll_speed < MIN_SCROLL_SPEED || settings.scroll_speed > MAX_SCROLL_SPEED {
        return Err(t!(
            "validation.scrollSpeed",
            min = MIN_SCROLL_SPEED,
            max = MAX_SCROLL_SPEED
        )
        .to_string());
    }

    if settings.markdown_font_size < MIN_FONT_SIZE || settings.markdown_font_size > MAX_FONT_SIZE {
        return Err(t!(
            "validation.markdownFontSize",
            min = MIN_FONT_SIZE,
            max = MAX_FONT_SIZE
        )
        .to_string());
    }

    Ok(())
}

// ============================================================
// Commands
// ============================================================

/// Get the config directory path for settings
fn get_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Failed to get config directory: {}", e))?;

    Ok(config_dir.join("settings.json"))
}

/// Loads application settings from the config file.
///
/// Returns default settings if:
/// - The file doesn't exist
/// - The file cannot be parsed
///
/// All fields always have valid values due to serde(default) + deserialize_null_default.
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let config_path = get_config_path(&app)?;

    // If file doesn't exist, return defaults
    if !config_path.exists() {
        return Ok(AppSettings::default());
    }

    // Read file contents
    let contents = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to read settings file: {}", e);
            return Ok(AppSettings::default());
        }
    };

    // Parse JSON — serde(default) handles missing fields,
    // deserialize_null_default handles null values
    match serde_json::from_str::<AppSettings>(&contents) {
        Ok(mut settings) => {
            // Migration: move legacy font_family to font_family_primary if needed
            if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
                settings.font_family_primary = std::mem::take(&mut settings.font_family);
            } else {
                settings.font_family.clear();
            }

            Ok(settings)
        }
        Err(e) => {
            log::warn!("Failed to parse settings file: {}", e);
            Ok(AppSettings::default())
        }
    }
}

/// Saves application settings to the config file.
///
/// Returns an error if:
/// - Any field fails validation
/// - The config directory cannot be created
/// - The file cannot be written
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    validate_settings(&settings)?;

    let config_path = get_config_path(&app)?;

    // Create config directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    // Serialize settings to JSON
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    // Write to file
    fs::write(&config_path, json).map_err(|e| format!("Failed to write settings file: {}", e))?;

    Ok(())
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Default values --

    #[test]
    fn test_app_settings_default() {
        let settings = AppSettings::default();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
        assert_eq!(settings.terminal_color_scheme, "");
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.show_scrollbar, ScrollbarMode::Auto);
        assert_eq!(settings.shell_path, "");
        assert!(settings.shell_args.is_empty());
        assert_eq!(settings.cursor_style, CursorStyle::Block);
        assert!(settings.cursor_blink);
        assert_eq!(settings.scroll_speed, 3);
        assert_eq!(settings.bell_action, BellAction::Visual);
        assert!(settings.url_detection);
        assert!(!settings.copy_on_select);
        assert!(settings.file_path_detection);
        assert!(settings.bold_brightens_ansi_colors);
        assert!(settings.middle_click_paste);
        assert_eq!(settings.editor_command, "code --goto {file}:{line}:{col}");
        assert!(settings.skk_mode);
        assert_eq!(settings.language, "auto");
        assert_eq!(settings.ui_font_family, "Roboto");
        assert!(settings.show_tab_bar);
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
        // Notification defaults
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_keybind_settings_default() {
        let keybinds = KeybindSettings::default();
        assert_eq!(keybinds.copy, "Ctrl+Shift+C");
        assert_eq!(keybinds.paste, "Ctrl+Shift+V");
        assert_eq!(keybinds.select_all, "Ctrl+Shift+A");
        assert_eq!(keybinds.search, "Ctrl+Shift+F");
        assert_eq!(keybinds.new_tab, "Ctrl+Shift+T");
        assert_eq!(keybinds.close_tab, "Ctrl+Shift+W");
        assert_eq!(keybinds.next_tab, "Ctrl+PageDown");
        assert_eq!(keybinds.prev_tab, "Ctrl+PageUp");
        assert_eq!(keybinds.zoom_in, "Ctrl+Plus");
        assert_eq!(keybinds.zoom_out, "Ctrl+Minus");
        assert_eq!(keybinds.zoom_reset, "Ctrl+0");
        assert_eq!(keybinds.toggle_fullscreen, "F11");
        assert_eq!(keybinds.open_settings, "Ctrl+,");
        assert_eq!(keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    // -- Deserialization --

    #[test]
    fn test_deserialize_empty_json() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.cursor_style, CursorStyle::Block);
        assert!(settings.cursor_blink);
        assert_eq!(settings.keybinds.copy, "Ctrl+Shift+C");
    }

    #[test]
    fn test_deserialize_old_format() {
        let json = r#"{"font_size": 16}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 16);
        // All new fields use defaults
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn test_deserialize_null_font_size() {
        let json = r#"{"font_size": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
    }

    #[test]
    fn test_deserialize_null_enum() {
        let json = r#"{"ui_theme": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme, UiTheme::System);
    }

    #[test]
    fn test_deserialize_null_keybind() {
        let json = r#"{"keybinds": {"copy": null}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        // null keybind falls back to custom default via deserialize_null_keybind_copy
        assert_eq!(settings.keybinds.copy, "Ctrl+Shift+C");
        // Non-null keybinds still use serde(default) function
        assert_eq!(settings.keybinds.paste, "Ctrl+Shift+V");
    }

    #[test]
    fn test_deserialize_null_all_custom_defaults() {
        let json = r#"{
            "font_size": null,
            "line_height": null,
            "padding": null,
            "scrollback_lines": null,
            "scroll_speed": null,
            "cursor_blink": null,
            "url_detection": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.scroll_speed, 3);
        assert!(settings.cursor_blink);
        assert!(settings.url_detection);
    }

    #[test]
    fn test_deserialize_ignores_unknown_fields() {
        let json = r#"{"font_size": 14, "unknown_field": "value"}"#;
        // serde by default ignores unknown fields (no deny_unknown_fields)
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 14);
    }

    #[test]
    fn test_deserialize_invalid_enum_errors() {
        let json = r#"{"ui_theme": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_cursor_style_errors() {
        let json = r#"{"cursor_style": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_bell_action_errors() {
        let json = r#"{"bell_action": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_scrollbar_mode_errors() {
        let json = r#"{"show_scrollbar": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    // -- UiThemePreset --

    #[test]
    fn test_ui_theme_preset_default_is_purple() {
        assert_eq!(UiThemePreset::default(), UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_ui_theme_preset_values() {
        let test_cases = vec![
            (r#""purple""#, UiThemePreset::Purple),
            (r#""blue""#, UiThemePreset::Blue),
            (r#""green""#, UiThemePreset::Green),
            (r#""orange""#, UiThemePreset::Orange),
        ];
        for (json, expected) in test_cases {
            let result: UiThemePreset = serde_json::from_str(json).unwrap();
            assert_eq!(result, expected, "Failed for {}", json);
        }
    }

    #[test]
    fn test_deserialize_null_ui_theme_preset() {
        let json = r#"{"ui_theme_preset": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_missing_ui_theme_preset() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_invalid_ui_theme_preset_errors() {
        let json = r#"{"ui_theme_preset": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_ui_theme_preset_round_trip() {
        let test_cases = vec![
            UiThemePreset::Purple,
            UiThemePreset::Blue,
            UiThemePreset::Green,
            UiThemePreset::Orange,
        ];
        for preset in test_cases {
            let json = serde_json::to_string(&preset).unwrap();
            let restored: UiThemePreset = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, preset);
        }
    }

    // -- Serialization --

    #[test]
    fn test_serialize_enums_lowercase() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"ui_theme\":\"system\""));
        assert!(json.contains("\"ui_theme_preset\":\"purple\""));
        assert!(json.contains("\"cursor_style\":\"block\""));
        assert!(json.contains("\"bell_action\":\"visual\""));
        assert!(json.contains("\"show_scrollbar\":\"auto\""));
        assert!(json.contains("\"markdown_theme\":\"system\""));
        assert!(json.contains("\"markdown_theme_preset\":\"purple\""));
        assert!(json.contains("\"markdown_theme_follow_ui\":true"));
    }

    // -- Round-trip --

    #[test]
    fn test_round_trip_preserves_all_fields() {
        let settings = AppSettings {
            font_size: 16,
            font_family_primary: "Fira Code".to_string(),
            font_family_secondary: "Noto Sans JP".to_string(),
            font_family_emoji: "Noto Color Emoji".to_string(),
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
            editor_command: "vim +{line} {file}".to_string(),
            skk_mode: false,
            notification_enabled: false,
            tab_activity_indicator: false,
            notify_on_process_exit: false,
            notify_on_output: true,
            notify_on_bell: false,
        };

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.font_size, 16);
        assert_eq!(restored.font_family_primary, "Fira Code");
        assert_eq!(restored.font_family_secondary, "Noto Sans JP");
        assert_eq!(restored.font_family_emoji, "Noto Color Emoji");
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
        assert_eq!(restored.bell_action, BellAction::None);
        assert!(!restored.url_detection);
        assert!(restored.copy_on_select);
        assert!(!restored.file_path_detection);
        assert!(!restored.bold_brightens_ansi_colors);
        assert!(!restored.middle_click_paste);
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
    }

    #[test]
    fn test_shell_args_round_trip() {
        let json = r#"{"shell_args": ["--login", "-i"]}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.shell_args, vec!["--login", "-i"]);

        let serialized = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.shell_args, vec!["--login", "-i"]);
    }

    // -- Validation --

    // -- Language field deserialization --

    #[test]
    fn test_deserialize_missing_language_defaults_to_auto() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "auto");
    }

    #[test]
    fn test_deserialize_null_language_defaults_to_auto() {
        let json = r#"{"language": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "auto");
    }

    #[test]
    fn test_deserialize_language_ja() {
        let json = r#"{"language": "ja"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "ja");
    }

    #[test]
    fn test_deserialize_language_en() {
        let json = r#"{"language": "en"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_language_round_trip() {
        let mut settings = AppSettings::default();
        settings.language = "ja".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.language, "ja");
    }

    // -- UI font family tests --

    #[test]
    fn test_deserialize_missing_ui_font_family_defaults_to_roboto() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Roboto");
    }

    #[test]
    fn test_deserialize_null_ui_font_family_defaults_to_roboto() {
        let json = r#"{"ui_font_family": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Roboto");
    }

    #[test]
    fn test_deserialize_ui_font_family_custom_value() {
        let json = r#"{"ui_font_family": "Noto Sans"}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui_font_family, "Noto Sans");
    }

    #[test]
    fn test_ui_font_family_round_trip() {
        let mut settings = AppSettings::default();
        settings.ui_font_family = "Open Sans".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.ui_font_family, "Open Sans");
    }

    #[test]
    fn test_validate_valid_settings() {
        let settings = AppSettings::default();
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_rejects_font_size_below_min() {
        let mut settings = AppSettings::default();
        settings.font_size = 7;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_font_size_above_max() {
        let mut settings = AppSettings::default();
        settings.font_size = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scroll_speed_below_min() {
        let mut settings = AppSettings::default();
        settings.scroll_speed = 0;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scroll_speed_above_max() {
        let mut settings = AppSettings::default();
        settings.scroll_speed = 11;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_padding_above_max() {
        let mut settings = AppSettings::default();
        settings.padding = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_scrollback_above_max() {
        let mut settings = AppSettings::default();
        settings.scrollback_lines = 100001;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_accepts_boundary_values() {
        let mut settings = AppSettings::default();
        settings.font_size = MIN_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());

        settings.font_size = MAX_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MIN_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MAX_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());
    }

    // -- Font family migration --

    #[test]
    fn test_migrate_legacy_font_family_to_primary() {
        let json = r#"{"font_family": "Fira Code"}"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        // Simulate migration (load_settings does this)
        if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
            settings.font_family_primary = std::mem::take(&mut settings.font_family);
        } else {
            settings.font_family.clear();
        }
        assert_eq!(settings.font_family_primary, "Fira Code");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
    }

    #[test]
    fn test_migrate_font_family_primary_takes_precedence() {
        let json = r#"{"font_family": "Old Font", "font_family_primary": "New Font"}"#;
        let mut settings: AppSettings = serde_json::from_str(json).unwrap();
        if !settings.font_family.is_empty() && settings.font_family_primary.is_empty() {
            settings.font_family_primary = std::mem::take(&mut settings.font_family);
        } else {
            settings.font_family.clear();
        }
        assert_eq!(settings.font_family_primary, "New Font");
    }

    #[test]
    fn test_font_family_not_serialized() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("\"font_family\""));
        assert!(json.contains("\"font_family_primary\""));
        assert!(json.contains("\"font_family_secondary\""));
        assert!(json.contains("\"font_family_emoji\""));
    }

    #[test]
    fn test_deserialize_null_font_family_fields() {
        let json = r#"{"font_family_primary": null, "font_family_secondary": null, "font_family_emoji": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_family_primary, "");
        assert_eq!(settings.font_family_secondary, "");
        assert_eq!(settings.font_family_emoji, "");
    }

    #[test]
    fn test_three_font_family_fields_round_trip() {
        let mut settings = AppSettings::default();
        settings.font_family_primary = "JetBrains Mono".to_string();
        settings.font_family_secondary = "Noto Sans JP".to_string();
        settings.font_family_emoji = "Noto Color Emoji".to_string();
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.font_family_primary, "JetBrains Mono");
        assert_eq!(restored.font_family_secondary, "Noto Sans JP");
        assert_eq!(restored.font_family_emoji, "Noto Color Emoji");
    }

    // -- show_tab_bar tests --

    #[test]
    fn test_deserialize_missing_show_tab_bar_defaults_to_true() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.show_tab_bar);
    }

    #[test]
    fn test_deserialize_null_show_tab_bar_defaults_to_true() {
        let json = r#"{"show_tab_bar": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.show_tab_bar);
    }

    #[test]
    fn test_show_tab_bar_false_round_trip() {
        let mut settings = AppSettings::default();
        settings.show_tab_bar = false;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.show_tab_bar);
    }

    // -- toggle_tab_bar keybind tests --

    #[test]
    fn test_deserialize_missing_toggle_tab_bar_keybind_defaults() {
        let json = r#"{"keybinds": {}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    #[test]
    fn test_deserialize_null_toggle_tab_bar_keybind_defaults() {
        let json = r#"{"keybinds": {"toggle_tab_bar": null}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+B");
    }

    #[test]
    fn test_toggle_tab_bar_keybind_custom_value() {
        let json = r#"{"keybinds": {"toggle_tab_bar": "Ctrl+Shift+H"}}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.keybinds.toggle_tab_bar, "Ctrl+Shift+H");
    }

    // -- UserColorScheme tests --

    #[test]
    fn test_deserialize_missing_custom_color_schemes_defaults_to_empty() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.custom_color_schemes.is_empty());
    }

    #[test]
    fn test_deserialize_null_custom_color_schemes_defaults_to_empty() {
        let json = r#"{"custom_color_schemes": null}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.custom_color_schemes.is_empty());
    }

    #[test]
    fn test_user_color_scheme_round_trip() {
        let scheme = UserColorScheme {
            name: "my_theme".to_string(),
            foreground: "#f8f8f2".to_string(),
            background: "#282a36".to_string(),
            cursor: "#f8f8f2".to_string(),
            selection: "#44475a".to_string(),
            ansi_colors: vec![
                "#21222c".to_string(),
                "#ff5555".to_string(),
                "#50fa7b".to_string(),
                "#f1fa8c".to_string(),
                "#bd93f9".to_string(),
                "#ff79c6".to_string(),
                "#8be9fd".to_string(),
                "#f8f8f2".to_string(),
                "#6272a4".to_string(),
                "#ff6e6e".to_string(),
                "#69ff94".to_string(),
                "#ffffa5".to_string(),
                "#d6acff".to_string(),
                "#ff92df".to_string(),
                "#a4ffff".to_string(),
                "#ffffff".to_string(),
            ],
        };

        let json = serde_json::to_string(&scheme).unwrap();
        let restored: UserColorScheme = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, scheme);
    }

    #[test]
    fn test_settings_with_custom_color_schemes_round_trip() {
        let mut settings = AppSettings::default();
        settings.custom_color_schemes = vec![
            UserColorScheme {
                name: "theme1".to_string(),
                foreground: "#ffffff".to_string(),
                background: "#000000".to_string(),
                cursor: "#ffffff".to_string(),
                selection: "#333333".to_string(),
                ansi_colors: (0..16)
                    .map(|i| format!("#{:02x}{:02x}{:02x}", i * 16, i * 16, i * 16))
                    .collect(),
            },
            UserColorScheme {
                name: "theme2".to_string(),
                foreground: "#00ff00".to_string(),
                background: "#001100".to_string(),
                cursor: "#00ff00".to_string(),
                selection: "#003300".to_string(),
                ansi_colors: (0..16).map(|i| format!("#00{:02x}00", i * 16)).collect(),
            },
        ];

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.custom_color_schemes.len(), 2);
        assert_eq!(restored.custom_color_schemes[0].name, "theme1");
        assert_eq!(restored.custom_color_schemes[1].name, "theme2");
    }

    #[test]
    fn test_app_settings_default_has_empty_custom_color_schemes() {
        let settings = AppSettings::default();
        assert!(settings.custom_color_schemes.is_empty());
    }

    // -- Markdown Viewer settings tests --

    #[test]
    fn test_markdown_settings_defaults() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_missing_markdown_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_null_markdown_fields_use_defaults() {
        let json = r#"{
            "markdown_body_font_family": null,
            "markdown_code_font_family": null,
            "markdown_font_size": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "");
        assert_eq!(settings.markdown_code_font_family, "");
        assert_eq!(settings.markdown_font_size, 14);
    }

    #[test]
    fn test_deserialize_markdown_fields_explicit_values() {
        let json = r#"{
            "markdown_body_font_family": "Noto Sans",
            "markdown_code_font_family": "Fira Code",
            "markdown_font_size": 18
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.markdown_body_font_family, "Noto Sans");
        assert_eq!(settings.markdown_code_font_family, "Fira Code");
        assert_eq!(settings.markdown_font_size, 18);
    }

    #[test]
    fn test_validate_markdown_font_size_below_min() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = 7;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_markdown_font_size_above_max() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = 33;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_markdown_font_size_min_boundary() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = MIN_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());
    }

    #[test]
    fn test_validate_markdown_font_size_max_boundary() {
        let mut settings = AppSettings::default();
        settings.markdown_font_size = MAX_FONT_SIZE;
        assert!(validate_settings(&settings).is_ok());
    }

    // -- Markdown Theme settings tests --

    #[test]
    fn test_markdown_theme_follow_ui_default_is_true() {
        let settings = AppSettings::default();
        assert!(settings.markdown_theme_follow_ui);
    }

    #[test]
    fn test_markdown_theme_default_is_system() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_theme, UiTheme::System);
    }

    #[test]
    fn test_markdown_theme_preset_default_is_purple() {
        let settings = AppSettings::default();
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_missing_markdown_theme_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_deserialize_null_markdown_theme_fields_use_defaults() {
        let json = r#"{
            "markdown_theme_follow_ui": null,
            "markdown_theme": null,
            "markdown_theme_preset": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.markdown_theme_follow_ui);
        assert_eq!(settings.markdown_theme, UiTheme::System);
        assert_eq!(settings.markdown_theme_preset, UiThemePreset::Purple);
    }

    #[test]
    fn test_markdown_theme_fields_round_trip() {
        let mut settings = AppSettings::default();
        settings.markdown_theme_follow_ui = false;
        settings.markdown_theme = UiTheme::Dark;
        settings.markdown_theme_preset = UiThemePreset::Orange;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.markdown_theme_follow_ui);
        assert_eq!(restored.markdown_theme, UiTheme::Dark);
        assert_eq!(restored.markdown_theme_preset, UiThemePreset::Orange);
    }

    #[test]
    fn test_deserialize_invalid_markdown_theme_errors() {
        let json = r#"{"markdown_theme": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_markdown_theme_preset_errors() {
        let json = r#"{"markdown_theme_preset": "invalid"}"#;
        let result = serde_json::from_str::<AppSettings>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_markdown_settings_round_trip() {
        let mut settings = AppSettings::default();
        settings.markdown_body_font_family = "Georgia".to_string();
        settings.markdown_code_font_family = "JetBrains Mono".to_string();
        settings.markdown_font_size = 20;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.markdown_body_font_family, "Georgia");
        assert_eq!(restored.markdown_code_font_family, "JetBrains Mono");
        assert_eq!(restored.markdown_font_size, 20);
    }

    // -- Notification settings tests --

    #[test]
    fn test_deserialize_missing_notification_fields_use_defaults() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_deserialize_null_notification_fields_use_defaults() {
        let json = r#"{
            "notification_enabled": null,
            "tab_activity_indicator": null,
            "notify_on_process_exit": null,
            "notify_on_output": null,
            "notify_on_bell": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(settings.notification_enabled);
        assert!(settings.tab_activity_indicator);
        assert!(settings.notify_on_process_exit);
        assert!(!settings.notify_on_output);
        assert!(settings.notify_on_bell);
    }

    #[test]
    fn test_notification_settings_round_trip() {
        let mut settings = AppSettings::default();
        settings.notification_enabled = false;
        settings.tab_activity_indicator = false;
        settings.notify_on_process_exit = false;
        settings.notify_on_output = true;
        settings.notify_on_bell = false;
        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!(!restored.notification_enabled);
        assert!(!restored.tab_activity_indicator);
        assert!(!restored.notify_on_process_exit);
        assert!(restored.notify_on_output);
        assert!(!restored.notify_on_bell);
    }
}
