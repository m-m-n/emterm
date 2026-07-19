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

/// `Shift+Enter` key-rewrite behavior. Wire values: `none` / `alt_enter`
/// (default) / `kitty_csi_u`. Mirrors `src-tauri::settings::ShiftEnterBehavior`
/// (the native runtime's own copy of this enum, consumed at the
/// `window_host` key-event rewrite site); this copy is the shared
/// `settings.json` schema consumed by the child settings window.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ShiftEnterBehavior {
    None,
    #[default]
    AltEnter,
    KittyCsiU,
    /// Internal sentinel: never a valid wire value. Produced only by
    /// `AppSettings`'s field-level deserialize default when the
    /// `shift_enter_behavior` key is absent from the source JSON, so
    /// `AppSettings::apply_migrations` can distinguish "key absent" from
    /// "key present and resolved to the default" when deciding whether to
    /// fold in the legacy `shift_enter_as_alt_enter` boolean (FR5).
    /// `apply_migrations` always resolves this to a real variant before
    /// the struct is used further. `#[serde(skip)]` makes it unreachable
    /// from wire input and turns an accidental serialize into a hard
    /// error instead of writing an invalid wire value.
    #[doc(hidden)]
    #[serde(skip)]
    Unresolved,
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
