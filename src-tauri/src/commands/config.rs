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
pub const MIN_LINE_HEIGHT: f32 = 0.8;
pub const MAX_LINE_HEIGHT: f32 = 3.0;

// Layout
pub const MIN_PADDING: u32 = 0;
pub const MAX_PADDING: u32 = 32;
pub const MIN_SCROLLBACK_LINES: u32 = 0;
pub const MAX_SCROLLBACK_LINES: u32 = 100000;

// Opacity
pub const MIN_OPACITY: f32 = 0.3;
pub const MAX_OPACITY: f32 = 1.0;

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
deserialize_null_with!(deserialize_null_line_height, f32, default_line_height);
deserialize_null_with!(deserialize_null_opacity, f32, default_opacity);
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

// Keybind null deserializers
deserialize_null_with!(deserialize_null_keybind_copy, String, default_keybind_copy);
deserialize_null_with!(
    deserialize_null_keybind_paste,
    String,
    default_keybind_paste
);
deserialize_null_with!(
    deserialize_null_keybind_select_all,
    String,
    default_keybind_select_all
);
deserialize_null_with!(
    deserialize_null_keybind_search,
    String,
    default_keybind_search
);
deserialize_null_with!(
    deserialize_null_keybind_new_tab,
    String,
    default_keybind_new_tab
);
deserialize_null_with!(
    deserialize_null_keybind_close_tab,
    String,
    default_keybind_close_tab
);
deserialize_null_with!(
    deserialize_null_keybind_next_tab,
    String,
    default_keybind_next_tab
);
deserialize_null_with!(
    deserialize_null_keybind_prev_tab,
    String,
    default_keybind_prev_tab
);
deserialize_null_with!(
    deserialize_null_keybind_zoom_in,
    String,
    default_keybind_zoom_in
);
deserialize_null_with!(
    deserialize_null_keybind_zoom_out,
    String,
    default_keybind_zoom_out
);
deserialize_null_with!(
    deserialize_null_keybind_zoom_reset,
    String,
    default_keybind_zoom_reset
);
deserialize_null_with!(
    deserialize_null_keybind_toggle_fullscreen,
    String,
    default_keybind_toggle_fullscreen
);
deserialize_null_with!(
    deserialize_null_keybind_open_settings,
    String,
    default_keybind_open_settings
);

// ============================================================
// Default Value Functions
// ============================================================

fn default_font_size() -> u32 {
    13
}
fn default_line_height() -> f32 {
    1.2
}
fn default_opacity() -> f32 {
    1.0
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

fn default_keybind_copy() -> String {
    "Ctrl+Shift+C".to_string()
}
fn default_keybind_paste() -> String {
    "Ctrl+Shift+V".to_string()
}
fn default_keybind_select_all() -> String {
    "Ctrl+Shift+A".to_string()
}
fn default_keybind_search() -> String {
    "Ctrl+Shift+F".to_string()
}
fn default_keybind_new_tab() -> String {
    "Ctrl+Shift+T".to_string()
}
fn default_keybind_close_tab() -> String {
    "Ctrl+Shift+W".to_string()
}
fn default_keybind_next_tab() -> String {
    "Ctrl+Tab".to_string()
}
fn default_keybind_prev_tab() -> String {
    "Ctrl+Shift+Tab".to_string()
}
fn default_keybind_zoom_in() -> String {
    "Ctrl+Plus".to_string()
}
fn default_keybind_zoom_out() -> String {
    "Ctrl+Minus".to_string()
}
fn default_keybind_zoom_reset() -> String {
    "Ctrl+0".to_string()
}
fn default_keybind_toggle_fullscreen() -> String {
    "F11".to_string()
}
fn default_keybind_open_settings() -> String {
    "Ctrl+Comma".to_string()
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
    pub font_family: String,
    #[serde(
        default = "default_line_height",
        deserialize_with = "deserialize_null_line_height"
    )]
    pub line_height: f32,

    // Theme / Color
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub ui_theme: UiTheme,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub terminal_color_scheme: String,
    #[serde(
        default = "default_opacity",
        deserialize_with = "deserialize_null_opacity"
    )]
    pub opacity: f32,

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

    // Rich Content
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub inline_images_enabled: bool,
    #[serde(default = "default_true", deserialize_with = "deserialize_null_true")]
    pub markdown_rendering: bool,

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

    // Keybinds
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub keybinds: KeybindSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            font_family: String::new(),
            line_height: default_line_height(),
            ui_theme: UiTheme::default(),
            terminal_color_scheme: String::new(),
            opacity: default_opacity(),
            padding: default_padding(),
            scrollback_lines: default_scrollback_lines(),
            show_scrollbar: ScrollbarMode::default(),
            inline_images_enabled: default_true(),
            markdown_rendering: default_true(),
            shell_path: String::new(),
            shell_args: Vec::new(),
            cursor_style: CursorStyle::default(),
            cursor_blink: default_true(),
            scroll_speed: default_scroll_speed(),
            bell_action: BellAction::default(),
            url_detection: default_true(),
            copy_on_select: false,
            keybinds: KeybindSettings::default(),
        }
    }
}

/// Keybind settings structure.
/// All fields use serde(default) + deserialize_null_default for null handling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeybindSettings {
    #[serde(
        default = "default_keybind_copy",
        deserialize_with = "deserialize_null_keybind_copy"
    )]
    pub copy: String,
    #[serde(
        default = "default_keybind_paste",
        deserialize_with = "deserialize_null_keybind_paste"
    )]
    pub paste: String,
    #[serde(
        default = "default_keybind_select_all",
        deserialize_with = "deserialize_null_keybind_select_all"
    )]
    pub select_all: String,
    #[serde(
        default = "default_keybind_search",
        deserialize_with = "deserialize_null_keybind_search"
    )]
    pub search: String,
    #[serde(
        default = "default_keybind_new_tab",
        deserialize_with = "deserialize_null_keybind_new_tab"
    )]
    pub new_tab: String,
    #[serde(
        default = "default_keybind_close_tab",
        deserialize_with = "deserialize_null_keybind_close_tab"
    )]
    pub close_tab: String,
    #[serde(
        default = "default_keybind_next_tab",
        deserialize_with = "deserialize_null_keybind_next_tab"
    )]
    pub next_tab: String,
    #[serde(
        default = "default_keybind_prev_tab",
        deserialize_with = "deserialize_null_keybind_prev_tab"
    )]
    pub prev_tab: String,
    #[serde(
        default = "default_keybind_zoom_in",
        deserialize_with = "deserialize_null_keybind_zoom_in"
    )]
    pub zoom_in: String,
    #[serde(
        default = "default_keybind_zoom_out",
        deserialize_with = "deserialize_null_keybind_zoom_out"
    )]
    pub zoom_out: String,
    #[serde(
        default = "default_keybind_zoom_reset",
        deserialize_with = "deserialize_null_keybind_zoom_reset"
    )]
    pub zoom_reset: String,
    #[serde(
        default = "default_keybind_toggle_fullscreen",
        deserialize_with = "deserialize_null_keybind_toggle_fullscreen"
    )]
    pub toggle_fullscreen: String,
    #[serde(
        default = "default_keybind_open_settings",
        deserialize_with = "deserialize_null_keybind_open_settings"
    )]
    pub open_settings: String,
}

impl Default for KeybindSettings {
    fn default() -> Self {
        Self {
            copy: default_keybind_copy(),
            paste: default_keybind_paste(),
            select_all: default_keybind_select_all(),
            search: default_keybind_search(),
            new_tab: default_keybind_new_tab(),
            close_tab: default_keybind_close_tab(),
            next_tab: default_keybind_next_tab(),
            prev_tab: default_keybind_prev_tab(),
            zoom_in: default_keybind_zoom_in(),
            zoom_out: default_keybind_zoom_out(),
            zoom_reset: default_keybind_zoom_reset(),
            toggle_fullscreen: default_keybind_toggle_fullscreen(),
            open_settings: default_keybind_open_settings(),
        }
    }
}

// ============================================================
// Validation
// ============================================================

/// Validates settings values and returns an error message if invalid.
fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    if settings.font_size < MIN_FONT_SIZE || settings.font_size > MAX_FONT_SIZE {
        return Err(format!(
            "font_size must be between {} and {}",
            MIN_FONT_SIZE, MAX_FONT_SIZE
        ));
    }

    if settings.line_height < MIN_LINE_HEIGHT || settings.line_height > MAX_LINE_HEIGHT {
        return Err(format!(
            "line_height must be between {} and {}",
            MIN_LINE_HEIGHT, MAX_LINE_HEIGHT
        ));
    }

    if settings.opacity < MIN_OPACITY || settings.opacity > MAX_OPACITY {
        return Err(format!(
            "opacity must be between {} and {}",
            MIN_OPACITY, MAX_OPACITY
        ));
    }

    if settings.padding > MAX_PADDING {
        return Err(format!(
            "padding must be between {} and {}",
            MIN_PADDING, MAX_PADDING
        ));
    }

    if settings.scrollback_lines > MAX_SCROLLBACK_LINES {
        return Err(format!(
            "scrollback_lines must be between {} and {}",
            MIN_SCROLLBACK_LINES, MAX_SCROLLBACK_LINES
        ));
    }

    if settings.scroll_speed < MIN_SCROLL_SPEED || settings.scroll_speed > MAX_SCROLL_SPEED {
        return Err(format!(
            "scroll_speed must be between {} and {}",
            MIN_SCROLL_SPEED, MAX_SCROLL_SPEED
        ));
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
        Ok(settings) => Ok(settings),
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
        assert_eq!(settings.font_family, "");
        assert_eq!(settings.line_height, 1.2);
        assert_eq!(settings.ui_theme, UiTheme::System);
        assert_eq!(settings.terminal_color_scheme, "");
        assert_eq!(settings.opacity, 1.0);
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.show_scrollbar, ScrollbarMode::Auto);
        assert!(settings.inline_images_enabled);
        assert!(settings.markdown_rendering);
        assert_eq!(settings.shell_path, "");
        assert!(settings.shell_args.is_empty());
        assert_eq!(settings.cursor_style, CursorStyle::Block);
        assert!(settings.cursor_blink);
        assert_eq!(settings.scroll_speed, 3);
        assert_eq!(settings.bell_action, BellAction::Visual);
        assert!(settings.url_detection);
        assert!(!settings.copy_on_select);
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
        assert_eq!(keybinds.next_tab, "Ctrl+Tab");
        assert_eq!(keybinds.prev_tab, "Ctrl+Shift+Tab");
        assert_eq!(keybinds.zoom_in, "Ctrl+Plus");
        assert_eq!(keybinds.zoom_out, "Ctrl+Minus");
        assert_eq!(keybinds.zoom_reset, "Ctrl+0");
        assert_eq!(keybinds.toggle_fullscreen, "F11");
        assert_eq!(keybinds.open_settings, "Ctrl+Comma");
    }

    // -- Deserialization --

    #[test]
    fn test_deserialize_empty_json() {
        let json = r#"{}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.line_height, 1.2);
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
        assert_eq!(settings.font_family, "");
        assert_eq!(settings.line_height, 1.2);
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
            "opacity": null,
            "padding": null,
            "scrollback_lines": null,
            "scroll_speed": null,
            "cursor_blink": null,
            "inline_images_enabled": null,
            "markdown_rendering": null,
            "url_detection": null
        }"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.font_size, 13);
        assert_eq!(settings.line_height, 1.2);
        assert_eq!(settings.opacity, 1.0);
        assert_eq!(settings.padding, 4);
        assert_eq!(settings.scrollback_lines, 10000);
        assert_eq!(settings.scroll_speed, 3);
        assert!(settings.cursor_blink);
        assert!(settings.inline_images_enabled);
        assert!(settings.markdown_rendering);
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

    // -- Serialization --

    #[test]
    fn test_serialize_enums_lowercase() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"ui_theme\":\"system\""));
        assert!(json.contains("\"cursor_style\":\"block\""));
        assert!(json.contains("\"bell_action\":\"visual\""));
        assert!(json.contains("\"show_scrollbar\":\"auto\""));
    }

    // -- Round-trip --

    #[test]
    fn test_round_trip_preserves_all_fields() {
        let settings = AppSettings {
            font_size: 16,
            font_family: "Fira Code".to_string(),
            line_height: 1.5,
            ui_theme: UiTheme::Dark,
            terminal_color_scheme: "monokai".to_string(),
            opacity: 0.8,
            padding: 8,
            scrollback_lines: 5000,
            show_scrollbar: ScrollbarMode::Always,
            inline_images_enabled: false,
            markdown_rendering: false,
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
        };

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.font_size, 16);
        assert_eq!(restored.font_family, "Fira Code");
        assert_eq!(restored.line_height, 1.5);
        assert_eq!(restored.ui_theme, UiTheme::Dark);
        assert_eq!(restored.terminal_color_scheme, "monokai");
        assert_eq!(restored.opacity, 0.8);
        assert_eq!(restored.padding, 8);
        assert_eq!(restored.scrollback_lines, 5000);
        assert_eq!(restored.show_scrollbar, ScrollbarMode::Always);
        assert!(!restored.inline_images_enabled);
        assert!(!restored.markdown_rendering);
        assert_eq!(restored.shell_path, "/bin/zsh");
        assert_eq!(restored.shell_args, vec!["--login", "-i"]);
        assert_eq!(restored.cursor_style, CursorStyle::Bar);
        assert!(!restored.cursor_blink);
        assert_eq!(restored.scroll_speed, 5);
        assert_eq!(restored.bell_action, BellAction::None);
        assert!(!restored.url_detection);
        assert!(restored.copy_on_select);
        assert_eq!(restored.keybinds.copy, "Ctrl+C");
        assert_eq!(restored.keybinds.paste, "Ctrl+V");
        assert_eq!(restored.keybinds.select_all, "Ctrl+Shift+A");
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
    fn test_validate_rejects_line_height_below_min() {
        let mut settings = AppSettings::default();
        settings.line_height = 0.5;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_line_height_above_max() {
        let mut settings = AppSettings::default();
        settings.line_height = 3.5;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_opacity_below_min() {
        let mut settings = AppSettings::default();
        settings.opacity = 0.1;
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn test_validate_rejects_opacity_above_max() {
        let mut settings = AppSettings::default();
        settings.opacity = 1.5;
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

        settings.opacity = MIN_OPACITY;
        assert!(validate_settings(&settings).is_ok());

        settings.opacity = MAX_OPACITY;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MIN_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());

        settings.scroll_speed = MAX_SCROLL_SPEED;
        assert!(validate_settings(&settings).is_ok());
    }
}
