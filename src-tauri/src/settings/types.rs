//! Sub-setting types of [`Settings`]: enums with their parse-or-warn
//! helpers, per-feature setting structs, and their `Default` impls.

/// Display width policy for East-Asian ambiguous-width characters
/// (Unicode property `Ambiguous`). xterm's `ambiguousIsNarrow` / `wide`
/// resource matches this enum 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbiguousWidthMode {
    /// One-cell width. Matches Western locales and xterm default.
    #[default]
    Narrow,
    /// Two-cell width. Matches CJK locales where ambiguous code points
    /// (e.g. arrows, box drawing) are conventionally rendered double-wide.
    Wide,
}

impl AmbiguousWidthMode {
    /// Display width contribution of an ambiguous-width code point under
    /// this policy.
    pub fn width_for_ambiguous(&self) -> u8 {
        match self {
            AmbiguousWidthMode::Narrow => 1,
            AmbiguousWidthMode::Wide => 2,
        }
    }
}

/// Default number of off-viewport scrollback rows preserved by each tab's
/// terminal core. Mirrors the legacy WebView build (10 000).
pub const DEFAULT_SCROLLBACK_LINES: u32 = 10_000;

/// Default per-tab image-memory budget in megabytes. Mirrors the
/// `src-tauri` build's image cache quota.
pub const DEFAULT_IMAGE_MEMORY_QUOTA_MB: u32 = 320;

/// Default terminal cell font size in logical points. Matches the
/// legacy WebView build's `font_size` default and the previous
/// hard-coded `Theme::default().font_size_pt`.
pub const DEFAULT_FONT_SIZE_PT: f32 = 13.0;

/// Default Markdown viewer base font size in logical points. Mirrors the
/// WebView build's `markdown_font_size` default (SPEC §Settings).
pub const DEFAULT_MARKDOWN_FONT_SIZE: u32 = 14;

/// Default maximum concurrent SFTP uploads. Mirrors the `src-tauri`
/// build's `sftp_max_concurrent_uploads` default.
pub const DEFAULT_SFTP_MAX_CONCURRENT_UPLOADS: u16 = 4;

/// CSS-compatible points-to-pixels conversion factor (1pt = 4/3 px at
/// the 96-dpi reference resolution that browsers and the legacy
/// WebView build assume). Used to translate `settings.font_size`
/// (logical points) into the pixel size the rasterizer expects.
///
/// Mirrors `src/terminal/renderer-settings.ts::setFontSize` which
/// applies the same `96 / 72` factor before handing the size to
/// `ctx.font = "${px}px"`. Without this conversion, native-poc
/// rasterizes at ~75% of the WebView build's visual size for the
/// same `font_size: 13` settings value.
pub const PT_TO_PX: f32 = 96.0 / 72.0;

/// Default window inner padding (in logical pixels) around the
/// terminal cell grid. Matches the legacy WebView build's `padding`
/// default and the previous hard-coded `render::{TOP_PAD, LEFT_PAD}`.
pub const DEFAULT_PADDING_PX: u32 = 4;

/// Cursor visual style mirrored from the legacy WebView settings.
/// Parsed from `settings.json`'s `cursor_style` string and projected
/// onto `crate::render::theme::CursorStyle` at theme construction
/// time (see [`From<CursorStyle> for crate::render::theme::CursorStyle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

impl CursorStyle {
    /// Parse the textual spec from `settings.json`. Unknown values
    /// fall back to [`CursorStyle::Block`] and emit a single
    /// `warn`-level log for the process lifetime (subsequent unknown
    /// values silently coerce). Mirrors the `FontEngine` warn-once
    /// pattern.
    pub fn parse_or_warn(spec: &str) -> Self {
        // Accept aliases that the legacy build also accepts so a
        // settings.json copied across versions parses cleanly.
        match spec.trim().to_ascii_lowercase().as_str() {
            "block" => Self::Block,
            "underline" | "underscore" => Self::Underline,
            "bar" | "beam" | "ibeam" | "i-beam" | "vertical-bar" => Self::Bar,
            other => {
                warn_unknown_cursor_style_once(other);
                Self::Block
            }
        }
    }

    /// Canonical `settings.json` spelling (the WebView build's select
    /// values). Inverse of [`CursorStyle::parse_or_warn`] for the
    /// settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Underline => "underline",
            Self::Bar => "bar",
        }
    }

    /// Canonical numeric shape encoding consumed by
    /// `term_core::TerminalCore::set_cursor_style` / `get_cursor_style`
    /// and the renderer's cursor overlay: `0 = block`, `1 = underline`,
    /// `2 = bar`. Shared by every settings-default seeding call site
    /// (tab spawn, settings apply) so they cannot drift from one
    /// another.
    pub fn as_cursor_shape_u8(&self) -> u8 {
        match self {
            Self::Block => 0,
            Self::Underline => 1,
            Self::Bar => 2,
        }
    }
}

fn warn_unknown_cursor_style_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.cursor_style: unknown value {:?}, falling back to \"block\"",
            owned
        );
    });
}

/// Default maximum OSC 52 clipboard payload size (10 MiB). Mirrors the
/// legacy `src-tauri/src/commands/config/settings.rs::default_clipboard_max_size_osc52`.
pub const DEFAULT_CLIPBOARD_MAX_SIZE_OSC52: u32 = 10 * 1024 * 1024;

/// Default prefix-key chord for mux mode. Matches the legacy WebView build
/// and the common tmux default. Parsed by
/// `crate::mux::prefix::parse_prefix_key` at startup; an invalid value falls
/// back to this default with a `warn` log so a typo in `settings.json`
/// cannot lock the user out of mux mode.
pub const DEFAULT_MUX_PREFIX_KEY: &str = "Ctrl+Z";

/// Action keys recognized in `mux.keybinds`. Each maps a mux action name to
/// a follow-up chord after the prefix (`Ctrl`-modified defaults
/// `Ctrl+D`/`Ctrl+C`/`Ctrl+N`/`Ctrl+P`/`Ctrl+R`/`Ctrl+T`). SSOT is
/// `DEFAULT_ACTION_BINDINGS` in `src-tauri/src/mux/prefix.rs`; the
/// `src-tauri/web-shared/terminal/mux/prefix-key.ts` table is a mirror.
pub const MUX_ACTION_NAMES: [&str; 8] = [
    "detach",
    "new-window",
    "next-window",
    "prev-window",
    "rename-window",
    "move-window",
    "toggle-window-sidebar",
    "next-agent-window",
];

/// Default follow-up chord for a mux action, or `None` if the action
/// name is unknown. Thin wrapper over the SSOT
/// [`crate::mux::prefix::DEFAULT_ACTION_BINDINGS`]; both the settings
/// loader and `ActionBindings::default()` consult the same table so the
/// two cannot drift.
pub fn default_mux_action_chord(action: &str) -> Option<crate::mux::prefix::PrefixChord> {
    crate::mux::prefix::default_action_chord(action)
}

/// Mux action names that the WebView build (and older native imports)
/// still write into `settings.json` even though SPEC mux-feature-cleanup
/// removed them from [`MUX_ACTION_NAMES`]. The loader drops these
/// without emitting a warn so a normal launch isn't spammed once per
/// legacy entry — the entries are dead either way.
pub(in crate::settings) fn is_legacy_mux_action(action: &str) -> bool {
    matches!(
        action,
        "split-vertical"
            | "split-horizontal"
            | "next-pane"
            | "prev-pane"
            | "close-pane"
            | "zoom-toggle"
            | "copy-mode"
            | "paste"
    )
}

/// Resolved mux UI settings (`mux.*`). Port of the WebView `MuxSettings` /
/// `crates/app_settings::MuxSettings` subset the native build consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct MuxSettings {
    /// Initial expansion state of the tab group (`mux.tab_always_expand`).
    pub tab_always_expand: bool,
    /// Window sidebar placement mode (`mux.window_sidebar_overlay`).
    /// `true` (default) = floating right overlay, `false` = persistent
    /// left panel. Absent or explicit-null keys resolve to the overlay
    /// default (task0001 D3); an explicit `false` always selects the
    /// persistent panel.
    pub window_sidebar_overlay: bool,
    /// Effective per-action follow-up chords (`mux.keybinds`), starting
    /// from the tmux defaults and overlaid with valid user entries.
    /// Invalid or unknown entries are dropped (warn) and the default is
    /// kept. Each value is a [`crate::mux::prefix::PrefixChord`] so that
    /// both bare single-char follow-ups (`"d"`) and modifier-bearing
    /// chords (`"Ctrl+D"`) are first-class — matching the WebView's
    /// `matchActionBinding`.
    pub keybinds: std::collections::HashMap<String, crate::mux::prefix::PrefixChord>,
}

impl Default for MuxSettings {
    fn default() -> Self {
        let mut keybinds = std::collections::HashMap::new();
        for action in MUX_ACTION_NAMES {
            if let Some(c) = default_mux_action_chord(action) {
                keybinds.insert(action.to_string(), c);
            }
        }
        Self {
            tab_always_expand: false,
            window_sidebar_overlay: true,
            keybinds,
        }
    }
}

/// Parse a `mux.keybinds` follow-up spec into a [`PrefixChord`]. Accepts
/// both single printable chars (`"c"`, `","`) and modifier chords
/// (`"Ctrl+D"`, `"Alt+M"`) — matching the WebView's `matchActionBinding`
/// which routes either via `parseKeybind`. Returns `None` for anything
/// the chord parser cannot recognize; the caller warns and keeps the
/// tmux default for that action.
///
/// [`PrefixChord`]: crate::mux::prefix::PrefixChord
pub fn parse_mux_action_chord(spec: &str) -> Option<crate::mux::prefix::PrefixChord> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Defer to the shared `parse_prefix_key` parser so prefix and action
    // follow-ups share one chord-spec grammar. Single printable chars
    // (no `+`) go through the same path: `parse_prefix_key("d")` →
    // `PrefixChord { ctrl: false, ..., key: KeySym::Letter('d') }`.
    crate::mux::prefix::parse_prefix_key(trimmed)
}

/// Font rasterizer engine selector (Phase 4-H / font-swash-migration FR6).
///
/// `Swash` is the default and exercises swash + zeno + fontdb for CJK +
/// color emoji coverage. `AbGlyph` keeps the legacy ab_glyph path live as
/// an escape hatch when a swash bug is suspected. Selection happens once
/// at startup; runtime hot-swap is intentionally not supported (per FR6
/// and NFR3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontEngine {
    #[default]
    Swash,
    AbGlyph,
}

impl FontEngine {
    /// Parse the textual spec from `settings.json`. Unknown values fall
    /// back to [`FontEngine::Swash`] and emit a single `warn`-level log
    /// for the process lifetime (subsequent unknown values silently
    /// coerce). Matches the warn-once parse pattern used across the
    /// settings enums.
    #[allow(dead_code)] // Phase 7: settings.json loader will call this.
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "swash" => Self::Swash,
            "ab_glyph" | "abglyph" => Self::AbGlyph,
            other => {
                warn_unknown_font_engine_once(other);
                Self::Swash
            }
        }
    }
}

#[allow(dead_code)] // Phase 7: invoked from settings.json loader via parse_or_warn.
fn warn_unknown_font_engine_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.font_engine: unknown value {:?}, falling back to \"swash\"",
            owned
        );
    });
}

/// IME-related settings. Phase 4-G introduces the `native_integration`
/// toggle which controls whether `ImeBackendFactory` brings up a real
/// platform IME client (X11 XIM / Wayland zwp_text_input_v3 / Windows
/// IMM32) or installs a passthrough `NullBackend`.
///
/// Backward compatibility: an existing `settings.json` without an
/// `ime` key MUST still parse — see [`Settings::default`] which seeds
/// `ImeSettings::default()` (with `native_integration: true`). The
/// JSON loader is Phase 7's responsibility; this struct only pins the
/// shape today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeSettings {
    /// When `true`, native IME clients are initialized on startup. On
    /// init failure (`XOpenIM` returns NULL, Wayland missing
    /// `zwp_text_input_manager_v3`, `SetWindowSubclass` fails) the App
    /// falls back to a `NullBackend` and behaves like Phase 4. When
    /// `false`, the fallback is taken unconditionally (no native IME).
    pub native_integration: bool,
}

impl Default for ImeSettings {
    fn default() -> Self {
        Self {
            // Phase 4-G default: opt-in by default, opt-out via env
            // (`EMTERM_NATIVE_IME=0`) or settings.
            native_integration: true,
        }
    }
}

/// Keyboard-shortcut chord specs mirrored from the legacy WebView
/// build's `AppSettings.keybinds`. Each field holds the textual chord
/// (e.g. `"Ctrl+Shift+C"`) exactly as written in `settings.json`; the
/// strings are parsed into [`crate::ui::keybinds::Chord`]s by
/// `KeybindTable::from_settings` at startup.
///
/// A subset of these actions is dispatched by native-poc today
/// (`copy`, `paste`, `new_tab`, `close_tab`, `next_tab`, `prev_tab`,
/// `select_all`, `zoom_in`, `zoom_out`, `zoom_reset`,
/// `toggle_fullscreen`, `toggle_tab_bar`). The remaining specs are
/// captured for forward compatibility so a `settings.json` shared with
/// the WebView build round-trips without data loss; each carries an
/// `#[allow(dead_code)]` until a native-poc feature consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindSettings {
    /// Copy the current selection to the system clipboard.
    pub copy: String,
    /// Paste the system clipboard into the active terminal.
    pub paste: String,
    /// Select the entire visible viewport of the active tab. Dispatched
    /// to `AppAction::SelectAll`.
    pub select_all: String,
    /// Open the in-terminal search overlay. Dispatched to
    /// `AppAction::OpenSearch`.
    pub search: String,
    /// Open a new tab in the focused window.
    pub new_tab: String,
    /// Open a new tab in a new window. Captured for forward
    /// compatibility; native-poc has no multi-window support yet.
    #[allow(dead_code)]
    pub new_tab_global: String,
    /// Close the active tab.
    pub close_tab: String,
    /// Switch to the next tab.
    pub next_tab: String,
    /// Switch to the previous tab.
    pub prev_tab: String,
    /// Increase the runtime terminal font size by one point (clamped).
    /// Dispatched to `AppAction::ZoomIn`.
    pub zoom_in: String,
    /// Decrease the runtime terminal font size by one point (clamped).
    /// Dispatched to `AppAction::ZoomOut`.
    pub zoom_out: String,
    /// Reset the runtime terminal font size to `settings.font_size`.
    /// Dispatched to `AppAction::ZoomReset`.
    pub zoom_reset: String,
    /// Toggle borderless full-screen mode. Dispatched to
    /// `AppAction::ToggleFullscreen`.
    pub toggle_fullscreen: String,
    /// Open (or switch to) the in-app settings tab. Dispatched to
    /// `AppAction::OpenSettings`.
    pub open_settings: String,
    /// Toggle the tab bar visibility. Dispatched to
    /// `AppAction::ToggleTabBar`.
    pub toggle_tab_bar: String,
    /// Jump to the previous shell prompt (OSC 133). Dispatched to
    /// `AppAction::JumpToPrevPrompt`.
    pub jump_to_prev_prompt: String,
    /// Jump to the next shell prompt (OSC 133). Dispatched to
    /// `AppAction::JumpToNextPrompt`.
    pub jump_to_next_prompt: String,
    /// Open the profile selector. Captured for forward compatibility;
    /// native-poc has no profile selector yet.
    #[allow(dead_code)]
    pub profile_selector: String,
}

impl Default for KeybindSettings {
    fn default() -> Self {
        // Defaults mirror `src-tauri`'s `AppSettings.keybinds` exactly so
        // a single `settings.json` behaves identically across both builds.
        Self {
            copy: "Ctrl+Shift+C".to_string(),
            paste: "Ctrl+Shift+V".to_string(),
            select_all: "Ctrl+Shift+A".to_string(),
            search: "Ctrl+Shift+F".to_string(),
            new_tab: "Ctrl+Shift+T".to_string(),
            new_tab_global: "Ctrl+Shift+G".to_string(),
            close_tab: "Ctrl+Shift+W".to_string(),
            next_tab: "Ctrl+PageDown".to_string(),
            prev_tab: "Ctrl+PageUp".to_string(),
            zoom_in: "Ctrl+Plus".to_string(),
            zoom_out: "Ctrl+Minus".to_string(),
            zoom_reset: "Ctrl+0".to_string(),
            toggle_fullscreen: "F11".to_string(),
            open_settings: "Ctrl+,".to_string(),
            toggle_tab_bar: "Ctrl+Shift+B".to_string(),
            jump_to_prev_prompt: "Ctrl+Shift+ArrowUp".to_string(),
            jump_to_next_prompt: "Ctrl+Shift+ArrowDown".to_string(),
            profile_selector: "Ctrl+Shift+P".to_string(),
        }
    }
}

/// A single user-defined custom command for the
/// `{cmd:<name>}` template variable. The command is run by the
/// matching [`crate::status_bar::providers::CommandProvider`] worker
/// thread on a fixed interval; its stdout's first line becomes the
/// substituted value.
///
/// Name validation (`[a-zA-Z0-9_-]+`) is enforced by the worker layer
/// before spawning, so an invalid name never reaches `Command::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCommand {
    /// Path or basename of the executable. Resolved via `PATH` unless
    /// it contains a `/`. `~/` is expanded by the worker layer.
    pub executable: String,
    /// Re-run interval in milliseconds. Clamped to a minimum of
    /// 1000 ms by the worker layer (SPEC FR6 / US3 acceptance
    /// criteria). Defaults to 1000 ms when the field is omitted in
    /// `settings.json` (Phase 7).
    pub interval_ms: u64,
}

impl Default for CustomCommand {
    fn default() -> Self {
        Self {
            executable: String::new(),
            interval_ms: 1000,
        }
    }
}

/// Statusbar-related settings. Phase 4-D introduced `enabled` +
/// `position`. The Status-Bar Native Port phase extends the shape to
/// reach feature parity with the WebView build's status-bar
/// configuration. Backward compatibility: an existing `settings.json`
/// without a `statusbar` key MUST still parse — see
/// [`Settings::default`] which seeds these to their built-in defaults.
///
/// `Eq` is not derived because `font_size: Option<f32>` is not `Eq`.
/// Equality comparisons in tests use `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusBarSettings {
    /// When `false`, the status-bar widget is not inserted at all (the
    /// central terminal panel covers the full window). Default: `true`.
    pub enabled: bool,
    /// App Line 1 left-aligned template. Default `"{time}"`.
    pub app_line1_left: String,
    /// App Line 1 right-aligned template. Default `"{cwd}"`.
    pub app_line1_right: String,
    /// App Line 2 left-aligned template. Default empty (row hidden).
    pub app_line2_left: String,
    /// App Line 2 right-aligned template. Default empty (row hidden).
    pub app_line2_right: String,
    /// Time format spec consumed by `{time}`. Default `"HH:mm:ss"`.
    pub time_format: String,
    /// Optional font-size override in egui logical points. `None`
    /// keeps the renderer's default.
    pub font_size: Option<f32>,
    /// User-defined custom commands keyed by template name. Look-up
    /// happens via `{cmd:<name>}`.
    pub custom_commands: std::collections::HashMap<String, CustomCommand>,
    /// Per-provider refresh intervals (milliseconds). Recognised keys:
    /// `"time"` (default 1000), `"git_branch"` (default 5000), plus any
    /// custom command name. Unrecognised keys are ignored.
    pub refresh_rates: std::collections::HashMap<String, u64>,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            app_line1_left: "{time}".to_string(),
            app_line1_right: "{cwd}".to_string(),
            app_line2_left: String::new(),
            app_line2_right: String::new(),
            time_format: "HH:mm:ss".to_string(),
            font_size: None,
            custom_commands: std::collections::HashMap::new(),
            refresh_rates: std::collections::HashMap::new(),
        }
    }
}

/// Effective Markdown viewer appearance, resolved from the `markdown_*`
/// settings honoring `markdown_theme_follow_ui`. Phase 4 serializes this
/// into the child viewer payload; Phase 5 applies it to the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownAppearance {
    /// Effective brightness mode (UI source when `follow_ui`, else markdown).
    pub theme: UiTheme,
    /// Effective accent preset (UI source when `follow_ui`, else markdown).
    pub preset: UiThemePreset,
    /// Body font family (always the markdown_* value).
    pub body_font_family: String,
    /// Code font family (always the markdown_* value).
    pub code_font_family: String,
    /// Base font size in pt (always the markdown_* value).
    pub font_size: u32,
}

/// UI brightness mode mirrored from the legacy WebView settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiTheme {
    Light,
    Dark,
    #[default]
    System,
}

impl UiTheme {
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            "system" | "" => Self::System,
            other => {
                warn_unknown_ui_theme_once(other);
                Self::System
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`UiTheme::parse_or_warn`] for the settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }
}

fn warn_unknown_ui_theme_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.ui_theme: unknown value {:?}, falling back to \"system\"",
            owned
        );
    });
}

// `Language` and `parse_or_warn` live in `crate::settings_core` so the
// CLI dispatcher can use them without compiling the rest of this module.
// The re-export at the top of this file keeps the existing
// `crate::settings::Language` path working for GUI call sites.

/// MD3 accent-color preset mirrored from the legacy WebView settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiThemePreset {
    #[default]
    Purple,
    Blue,
    Green,
    Orange,
    Pink,
}

impl UiThemePreset {
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "purple" | "" => Self::Purple,
            "blue" => Self::Blue,
            "green" => Self::Green,
            "orange" => Self::Orange,
            "pink" => Self::Pink,
            other => {
                warn_unknown_ui_theme_preset_once(other);
                Self::Purple
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`UiThemePreset::parse_or_warn`] for the settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Orange => "orange",
            Self::Pink => "pink",
        }
    }
}

fn warn_unknown_ui_theme_preset_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.ui_theme_preset: unknown value {:?}, falling back to \"purple\"",
            owned
        );
    });
}

/// Scrollbar visibility policy mirrored from the legacy WebView settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbarMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl ScrollbarMode {
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Self::Auto,
            "always" => Self::Always,
            "never" => Self::Never,
            other => {
                warn_unknown_scrollbar_mode_once(other);
                Self::Auto
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`ScrollbarMode::parse_or_warn`] for the settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[allow(dead_code)]
fn warn_unknown_scrollbar_mode_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.show_scrollbar: unknown value {:?}, falling back to \"auto\"",
            owned
        );
    });
}

/// Bell action mirrored from the legacy WebView settings. Dispatched
/// by `App::pump_all` when a tab drains a BEL: `Visual` flashes the
/// terminal area for 150 ms, `Sound` plays the 800 Hz beep via
/// `crate::bell`, `None` ignores the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BellAction {
    Sound,
    #[default]
    Visual,
    None,
}

impl BellAction {
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "sound" => Self::Sound,
            "visual" | "" => Self::Visual,
            "none" => Self::None,
            other => {
                warn_unknown_bell_action_once(other);
                Self::Visual
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`BellAction::parse_or_warn`] for the settings-panel save path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sound => "sound",
            Self::Visual => "visual",
            Self::None => "none",
        }
    }
}

#[allow(dead_code)]
fn warn_unknown_bell_action_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.bell_action: unknown value {:?}, falling back to \"visual\"",
            owned
        );
    });
}

/// `Shift+Enter` key-rewrite behavior. Replaces the legacy
/// `shift_enter_as_alt_enter` boolean (migrated in
/// [`RawSettings::merge_into`]). Consulted at the `window_host` key-event
/// rewrite site (task0001 design D1). See IMPLEMENTATION.md's "Setting
/// wire contract" for the shared serde values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShiftEnterBehavior {
    /// Shift is dropped; `Shift+Enter` sends the same bytes as plain
    /// `Enter`. Mirrors the legacy `false` value.
    None,
    /// Shift is dropped and Alt is set; `Shift+Enter` sends the same
    /// bytes as `Alt+Enter` (M-RET). Mirrors the legacy `true` value.
    #[default]
    AltEnter,
    /// `Shift+Enter` sends the literal Kitty keyboard protocol CSI u
    /// sequence for Enter with the Shift modifier (`ESC [ 1 3 ; 2 u`),
    /// bypassing the key encoder.
    KittyCsiU,
    /// `Shift+Enter` sends the single byte 0x0a (line feed), bypassing the
    /// key encoder. See task0001 design D1.
    Lf,
}

impl ShiftEnterBehavior {
    /// Parse the `settings.json` wire value. Unknown strings fall back to
    /// the default (`AltEnter`) and emit a single `warn`-level log for the
    /// process lifetime (subsequent unknown values silently coerce).
    /// Mirrors [`BellAction::parse_or_warn`] / [`CursorStyle::parse_or_warn`].
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "alt_enter" => Self::AltEnter,
            "kitty_csi_u" => Self::KittyCsiU,
            "lf" => Self::Lf,
            other => {
                warn_unknown_shift_enter_behavior_once(other);
                Self::AltEnter
            }
        }
    }

    /// Canonical `settings.json` spelling. Inverse of
    /// [`ShiftEnterBehavior::parse_or_warn`] for the settings-panel save
    /// path.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AltEnter => "alt_enter",
            Self::KittyCsiU => "kitty_csi_u",
            Self::Lf => "lf",
        }
    }
}

fn warn_unknown_shift_enter_behavior_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.shift_enter_behavior: unknown value {:?}, falling back to \"alt_enter\"",
            owned
        );
    });
}

/// User-defined terminal color scheme. Mirrors
/// `src-tauri/src/commands/config/types.rs::UserColorScheme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserColorScheme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection: String,
    /// 16 ANSI colors (`#RRGGBB`). Entries beyond 16 are ignored; fewer
    /// than 16 entries leaves the trailing slots at the xterm default.
    pub ansi_colors: Vec<String>,
}
