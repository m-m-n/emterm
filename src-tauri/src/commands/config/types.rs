use serde::{Deserialize, Serialize};

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
    Pink,
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
