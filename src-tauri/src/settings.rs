//! Runtime settings for the GUI build.
//!
//! `Language` and `settings_path` are re-exported from `crate::settings_core`
//! so the CLI-only build can read them without compiling the rest of this
//! module.

pub use crate::settings_core::{Language, settings_path};

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
pub const MUX_ACTION_NAMES: [&str; 7] = [
    "detach",
    "new-window",
    "next-window",
    "prev-window",
    "rename-window",
    "move-window",
    "toggle-window-sidebar",
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
fn is_legacy_mux_action(action: &str) -> bool {
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

#[derive(Debug, Clone)]
pub struct Settings {
    pub ambiguous_width_mode: AmbiguousWidthMode,
    /// Font rasterizer engine. See [`FontEngine`]. Defaults to `Swash`
    /// per FR6.
    pub font_engine: FontEngine,
    /// Ordered list of additional font families (after the base font and
    /// before the bundled CJK / emoji fonts) to consult during fallback
    /// resolution. Each entry is a fontdb family name.
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub font_family_fallback: Vec<String>,
    /// Variable-font axis settings (e.g. `("wght", 700.0)`). Applied by
    /// the swash adapter to shaping, rasterization, and metrics of every
    /// registered font; ignored (with a WARN) under `font_engine =
    /// ab_glyph`.
    pub variable_font_axes: std::collections::HashMap<String, f32>,
    /// Maximum number of rows preserved above the viewport before old lines
    /// are dropped. Plumbed to `TerminalCore::new` at tab spawn time.
    pub scrollback_lines: u32,
    /// Cap on decoded-image memory in megabytes, held by the image-viewer
    /// router while a `Place` is awaited. Eviction is least-recently-used
    /// (see `crate::viewer::image::ImageViewerRouter`).
    pub image_memory_quota_mb: u32,
    /// Whether OSC 52 *read* (clipboard query `? ` form) is permitted.
    /// Writes are always allowed if size ≤ `clipboard_max_size_osc52`.
    /// Default `true` matches the legacy WebView build, where the same
    /// `settings.json` field controls behavior.
    pub clipboard_read_osc52: bool,
    /// Maximum payload size accepted by OSC 52 in bytes. Payloads above
    /// this cap are dropped and `LOG_OSC52_DENIED` is emitted.
    pub clipboard_max_size_osc52: u32,
    /// Prefix-key chord for mux mode. Stored as a textual spec (e.g.
    /// `"Ctrl+Z"`) and parsed at startup by
    /// `crate::mux::prefix::parse_prefix_key`. Phase 7 will surface this in
    /// `settings.json`; today the field is populated from
    /// [`DEFAULT_MUX_PREFIX_KEY`] and never mutated.
    #[allow(dead_code)] // Phase 4-D status bar / settings UI will consume this.
    pub mux_prefix_key: String,
    /// Resolved mux UI settings (`mux.tab_always_expand` / `keybinds`
    /// / `statusbar.*`). Consumed by the mux tab group, the
    /// prefix latch, and the mux status row.
    pub mux: MuxSettings,
    /// Status-bar widget configuration. See [`StatusBarSettings`]. Phase
    /// 4-D introduces this; today the value is always the default until
    /// Phase 7 wires `settings.json` loading.
    pub statusbar: StatusBarSettings,
    /// Keyboard-shortcut chord specs. See [`KeybindSettings`]. Parsed
    /// into a [`crate::ui::keybinds::KeybindTable`] at startup so the
    /// dispatcher can match against user-configured chords.
    pub keybinds: KeybindSettings,
    /// IME backend configuration. See [`ImeSettings`]. Phase 4-G
    /// introduces `native_integration` (default `true`). Phase 7 wires
    /// JSON loading; until then `Settings::default()` exercises the
    /// default shape only.
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub ime: ImeSettings,
    /// Terminal cell font size in logical points. Mirrors the legacy
    /// WebView build's `font_size`. Plumbed into
    /// `crate::render::theme::Theme::font_size_pt` at tab spawn time
    /// so `TerminalGridPass::CellMetrics::font_size_px` resolves to
    /// this value × the host's `pixels_per_point`.
    pub font_size: f32,
    /// Inner padding (logical pixels) between the window edge and the
    /// terminal cell grid. Plumbed into the renderer's `TOP_PAD` /
    /// `LEFT_PAD` so the user-configured strip surrounds the grid.
    pub padding: u32,
    /// Cursor visual style. See [`CursorStyle`]. Plumbed into the
    /// per-tab `Theme::cursor_style` at spawn time; OSC 22 may still
    /// mutate it at runtime.
    pub cursor_style: CursorStyle,
    /// Whether the terminal cursor blinks. Plumbed into
    /// `TerminalCore::set_cursor_blink` at tab spawn time; DECTCEM /
    /// app-driven mode changes may still override at runtime.
    pub cursor_blink: bool,
    /// UI brightness mode. `Light`/`Dark`/`System`. Picks the light or
    /// dark variant of the `ui::md3` preset palette at startup. `System`
    /// resolves to dark (no desktop-portal brightness lookup yet).
    pub ui_theme: UiTheme,
    /// MD3 accent color preset for the UI chrome. Swaps `md3::PRIMARY`
    /// at runtime so tab indicator / focused borders pick up the user's
    /// hue choice.
    pub ui_theme_preset: UiThemePreset,
    /// UI font family override for the egui-rendered chrome. Prepended
    /// to the `Proportional` chain (tab bar / title bar) by
    /// `window_host::configure_egui_fonts`; the status bar mirrors the
    /// WebView's terminal-font styling and stays on `Monospace`. Empty
    /// string keeps egui's bundled default.
    pub ui_font_family: String,
    /// Terminal color-scheme preset name. When non-empty and matched
    /// either by [`COLOR_SCHEME_PRESETS`] or by [`Settings::custom_color_schemes`],
    /// drives `Theme::{fg, bg, cursor_fg, palette16}` at spawn time.
    pub terminal_color_scheme: String,
    /// User-defined terminal color schemes. Looked up by name from
    /// `terminal_color_scheme`. User schemes override preset names of
    /// the same value.
    pub custom_color_schemes: Vec<UserColorScheme>,
    /// Scrollbar visibility policy for the terminal viewport
    /// (`ui::scrollbar`). `Auto` shows the bar only when scrollback
    /// content exists, mirroring the WebView's `overflow-y: auto`.
    pub show_scrollbar: ScrollbarMode,
    /// Whether the tab bar is drawn. `false` hides it entirely; the
    /// central terminal panel takes the freed vertical space.
    pub show_tab_bar: bool,
    /// When `true`, SGR `bold` promotes indexed ANSI colors 0-7 to their
    /// bright variants (8-15) on the foreground. Matches xterm's
    /// historical behavior and the WebView build's default. Plumbed into
    /// `Theme::bold_brightens_ansi_colors` at spawn time.
    pub bold_brightens_ansi_colors: bool,
    /// Default shell executable for newly spawned tabs. Empty string
    /// keeps the historical `$SHELL` / `/bin/sh` fallback in
    /// `PtySession::spawn`.
    pub shell_path: String,
    /// Argv tail passed to the spawned shell after the executable. Each
    /// entry becomes a separate argv slot.
    pub shell_args: Vec<String>,
    /// Number of grid rows scrolled per wheel notch when the mouse
    /// wheel delivers one line. Clamped at the call site to keep the
    /// behavior identical to the WebView build.
    pub scroll_speed: u32,
    /// DECSET 1007 user opt-out for AltScreen wheel→arrow translation.
    /// When `true` (default), a wheel event in alternate screen with the
    /// terminal-side `MODE_ALTERNATE_SCROLL` bit on emits arrow-key
    /// bytes to the active PTY so AltScreen apps (Claude Code, vim,
    /// less) scroll their own log; when `false`, the wheel falls
    /// through to the eMterm scrollback view as before.
    pub alternate_scroll_enabled: bool,
    /// When `true`, releasing a left-click selection also copies the
    /// resolved text to the system CLIPBOARD selection (the PRIMARY
    /// selection is always updated regardless).
    pub copy_on_select: bool,
    /// When `true`, a middle-click pastes the current PRIMARY selection
    /// into the terminal. When `false`, middle-click is a no-op.
    pub middle_click_paste: bool,
    /// `Shift+Enter` key-rewrite behavior. See [`ShiftEnterBehavior`].
    /// Default `AltEnter` mirrors the legacy `true` behavior (helps
    /// integrate with editors / shells that bind a multi-line
    /// continuation on `M-RET`).
    pub shift_enter_behavior: ShiftEnterBehavior,
    /// When `true` (default), a bare `Ctrl+J` press is withheld from
    /// the PTY. Emacs-style IMEs (SKK) use `Ctrl+J` for mode switching;
    /// without the skip the chord encodes to LF (`0x0A`) and inserts
    /// unwanted newlines. Mirrors the WebView build's keyboard-handler
    /// skip (`src/terminal-app/handlers/keyboard.ts`).
    pub skk_mode: bool,
    /// Bell action when the terminal receives `BEL` (`0x07`). One of
    /// `Sound`, `Visual`, `None`. The native-poc renderer does not yet
    /// implement either side, so this field is captured for forward
    /// compatibility only (see `Settings::default`).
    #[allow(dead_code)]
    pub bell_action: BellAction,
    /// Whether to auto-detect URLs in the terminal grid and underline
    /// them on hover. Consumed by `crate::links::find_link_at` (hover
    /// underline) and the Ctrl+click open path in `window_host`.
    pub url_detection: bool,
    /// Whether to auto-detect file paths in the terminal grid and
    /// underline them on hover. Consumed alongside `url_detection`.
    pub file_path_detection: bool,
    /// Whether the prompt-folding affordance is enabled. Seeds each tab's
    /// `FoldManager` at construction (see `Tab::spawn_shell`); when `false`,
    /// fold clicks are no-ops and no region is collapsed, but C→D / custom
    /// region registration still runs.
    pub fold_enabled: bool,
    /// Editor command template (e.g. `code --goto {file}:{line}:{col}`).
    /// Consumed by the Ctrl+click file-path open path in `window_host`
    /// via `crate::links::build_editor_command`.
    pub editor_command: String,
    /// Master switch for desktop notifications. When `false`, no OS
    /// notification is dispatched for any activity type (the tab
    /// activity dot is governed separately by `tab_activity_indicator`).
    pub notification_enabled: bool,
    /// Whether inactive tabs show a dot indicator when they accumulate
    /// unread activity (output / bell / process exit).
    pub tab_activity_indicator: bool,
    /// Notify when the shell process in an inactive tab exits.
    pub notify_on_process_exit: bool,
    /// Notify when an inactive tab produces new output.
    pub notify_on_output: bool,
    /// Notify when an inactive tab receives BEL (`0x07`).
    pub notify_on_bell: bool,
    /// task0007: desktop notification for a blocked/done agent-status
    /// transition on a pane the user is not looking at (FR9). Read at
    /// notification-fire time (no restart needed on change) alongside the
    /// master [`Settings::notification_enabled`] switch.
    pub agent_status_notifications: bool,
    /// UI language: `Auto` (OS locale), `En`, or `Ja`. Resolved to a
    /// concrete [`crate::i18n::Locale`] once at startup
    /// (`App::with_settings`); `Auto` consults the system locale and
    /// falls back to English for unsupported languages.
    pub language: Language,
    /// Whether WARN/ERROR log lines are appended to `emterm.log`
    /// (release builds only). The file path matches the legacy Tauri
    /// build's `app_log_dir()` so both binaries share a single log
    /// file during the native-poc transition.
    pub log_recording_enabled: bool,

    // ── Markdown viewer (Phase 1) ──
    /// When `true`, the Markdown viewer follows the UI chrome theme
    /// (`ui_theme` / `ui_theme_preset`). When `false`, it uses the
    /// dedicated `markdown_theme` / `markdown_theme_preset`. Mirrors the
    /// WebView build's `markdown_theme_follow_ui`.
    pub markdown_theme_follow_ui: bool,
    /// Brightness mode for the Markdown viewer when `follow_ui = false`.
    pub markdown_theme: UiTheme,
    /// Accent preset for the Markdown viewer when `follow_ui = false`.
    pub markdown_theme_preset: UiThemePreset,
    /// Body font family for the Markdown viewer. Empty → CSS fallback chain.
    pub markdown_body_font_family: String,
    /// Code font family for the Markdown viewer. Empty → CSS fallback chain.
    pub markdown_code_font_family: String,
    /// Base font size (pt) for the Markdown viewer.
    pub markdown_font_size: u32,

    // ── Profiles / SSH / SFTP ──
    /// Terminal profiles (per-tab shell / SSH / WSL spawn presets).
    /// Resolved into spawn overrides by [`crate::profiles::resolve_spawn`];
    /// the `is_default` profile is applied by the `new_tab` keybind
    /// (`new_tab_global` always uses the global settings).
    pub profiles: Vec<app_settings::Profile>,
    /// Path to the ssh(1) binary used to launch SSH profiles. Empty means
    /// "not configured" — SSH profiles fail with a logged error, matching
    /// the WebView's alert.
    pub ssh_command_path: String,
    /// Saved SSH connections, referenced by `Profile::ssh_connection_name`.
    pub ssh_connections: Vec<app_settings::SshConnection>,
    /// Maximum concurrent SFTP uploads. Applied to the in-process upload
    /// pool by `App::with_settings` at startup and `App::apply_settings` on
    /// reload (see `crate::sftp::service::SftpService`).
    pub sftp_max_concurrent_uploads: u16,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            ambiguous_width_mode: AmbiguousWidthMode::default(),
            font_engine: FontEngine::default(),
            font_family_fallback: Vec::new(),
            variable_font_axes: std::collections::HashMap::new(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            image_memory_quota_mb: DEFAULT_IMAGE_MEMORY_QUOTA_MB,
            clipboard_read_osc52: true,
            clipboard_max_size_osc52: DEFAULT_CLIPBOARD_MAX_SIZE_OSC52,
            mux_prefix_key: DEFAULT_MUX_PREFIX_KEY.to_string(),
            mux: MuxSettings::default(),
            statusbar: StatusBarSettings::default(),
            keybinds: KeybindSettings::default(),
            ime: ImeSettings::default(),
            font_size: DEFAULT_FONT_SIZE_PT,
            padding: DEFAULT_PADDING_PX,
            cursor_style: CursorStyle::default(),
            cursor_blink: true,
            ui_theme: UiTheme::default(),
            ui_theme_preset: UiThemePreset::default(),
            ui_font_family: String::new(),
            terminal_color_scheme: String::new(),
            custom_color_schemes: Vec::new(),
            show_scrollbar: ScrollbarMode::default(),
            show_tab_bar: true,
            bold_brightens_ansi_colors: true,
            shell_path: String::new(),
            shell_args: Vec::new(),
            scroll_speed: 3,
            alternate_scroll_enabled: true,
            copy_on_select: false,
            middle_click_paste: true,
            shift_enter_behavior: ShiftEnterBehavior::AltEnter,
            skk_mode: true,
            bell_action: BellAction::default(),
            url_detection: true,
            file_path_detection: true,
            fold_enabled: true,
            editor_command: "code --goto {file}:{line}:{col}".to_string(),
            notification_enabled: true,
            tab_activity_indicator: true,
            notify_on_process_exit: true,
            notify_on_output: false,
            notify_on_bell: true,
            agent_status_notifications: true,
            language: Language::default(),
            log_recording_enabled: false,
            markdown_theme_follow_ui: true,
            markdown_theme: UiTheme::default(),
            markdown_theme_preset: UiThemePreset::default(),
            markdown_body_font_family: String::new(),
            markdown_code_font_family: String::new(),
            markdown_font_size: DEFAULT_MARKDOWN_FONT_SIZE,
            profiles: Vec::new(),
            ssh_command_path: String::new(),
            ssh_connections: Vec::new(),
            sftp_max_concurrent_uploads: DEFAULT_SFTP_MAX_CONCURRENT_UPLOADS,
        }
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Terminal cell font size translated from logical points
    /// (`self.font_size`) into the CSS-compatible pixel size the
    /// rasterizer + cell metrics consume. See [`PT_TO_PX`] for the
    /// conversion rationale.
    pub fn font_size_px(&self) -> f32 {
        self.font_size * PT_TO_PX
    }

    /// Resolve the effective Markdown viewer appearance.
    ///
    /// When `markdown_theme_follow_ui` is `true`, the theme/preset come
    /// from the UI chrome (`ui_theme` / `ui_theme_preset`); otherwise from
    /// the dedicated `markdown_theme` / `markdown_theme_preset`. Fonts and
    /// size always come from the `markdown_*` keys (the WebView build has
    /// no UI-chrome equivalents to inherit).
    pub fn markdown_appearance(&self) -> MarkdownAppearance {
        let (theme, preset) = if self.markdown_theme_follow_ui {
            (self.ui_theme, self.ui_theme_preset)
        } else {
            (self.markdown_theme, self.markdown_theme_preset)
        };
        MarkdownAppearance {
            theme,
            preset,
            body_font_family: self.markdown_body_font_family.clone(),
            code_font_family: self.markdown_code_font_family.clone(),
            font_size: self.markdown_font_size,
        }
    }

    /// Load `settings.json` from the platform config dir; fall back to
    /// [`Settings::default`] on any read / parse failure (logged at warn).
    ///
    /// The path is intentionally identical to the legacy Tauri WebView
    /// build's `AppHandle::path().app_config_dir()` so a single
    /// `settings.json` is shared by both binaries during the native-poc
    /// transition:
    /// - Linux:   `$XDG_CONFIG_HOME/net.laser5.app.emterm/settings.json`
    ///            (default: `$HOME/.config/...`)
    /// - Windows: `%APPDATA%\net.laser5.app.emterm\settings.json`
    pub fn load_or_default() -> Self {
        match settings_path() {
            Some(p) => Self::load_from(&p),
            None => {
                log::warn!("settings: unable to resolve config dir; using defaults");
                Self::default()
            }
        }
    }

    /// Read and merge a specific `settings.json` path into a fresh
    /// [`Settings::default`]. Missing file → defaults (logged at info).
    /// Unreadable / unparseable file → defaults (logged at warn).
    pub fn load_from(path: &std::path::Path) -> Self {
        let mut base = Self::default();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::info!("settings: {} not found; using defaults", path.display());
                return base;
            }
            Err(e) => {
                log::warn!(
                    "settings: failed to read {}: {}; using defaults",
                    path.display(),
                    e
                );
                return base;
            }
        };
        match serde_json::from_slice::<RawSettings>(&bytes) {
            Ok(raw) => {
                raw.merge_into(&mut base);
                log::info!("settings: loaded {}", path.display());
            }
            Err(e) => {
                log::warn!(
                    "settings: failed to parse {}: {}; using defaults",
                    path.display(),
                    e
                );
            }
        }
        base
    }
}

// `settings_path` is defined in `crate::settings_core` and re-exported
// at the top of this module.

// ============================================================
// settings.json schema (deserialize side)
// ============================================================
//
// `RawSettings` mirrors the on-disk JSON layout. Top-level keys match
// the legacy Tauri WebView build (`src-tauri/.../config/settings.rs`'s
// `AppSettings`) so a single `settings.json` works for both binaries.
// native-poc-specific fields live under a dedicated `native_poc:` block
// to avoid colliding with future src-tauri additions.
//
// Every field is `Option<_>` with `#[serde(default)]` on the struct, so:
// - missing keys leave the corresponding `Settings` field untouched
// - explicit `null` is treated as "absent" (Option = None)
// - unknown keys are ignored (forward compatibility with newer
//   src-tauri settings written by the legacy build)
//
// Exception: `shift_enter_behavior` is `Option<Option<String>>` because
// its precedence over the legacy `shift_enter_as_alt_enter` boolean
// depends on distinguishing "key absent" from "key present with null"
// (FR5 / AC-3) — see its field doc comment.

/// Deserializes a JSON field into `Option<Option<T>>`, distinguishing an
/// absent key (`#[serde(default)]` on the container yields `None`; this
/// function is never invoked for a missing key) from a key present with
/// `null` (`Some(None)`) or a concrete value (`Some(Some(v))`). Used
/// where the difference between "key absent" and "key explicitly null"
/// changes precedence against a legacy fallback key — see
/// `RawSettings::shift_enter_behavior` / FR5.
fn deserialize_present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawSettings {
    // ── src-tauri / WebView build compatible (flat keys) ──
    scrollback_lines: Option<u32>,
    clipboard_read_osc52: Option<bool>,
    clipboard_max_size_osc52: Option<u32>,
    font_family_primary: Option<String>,
    font_family_secondary: Option<String>,
    font_size: Option<f32>,
    padding: Option<u32>,
    cursor_style: Option<String>,
    cursor_blink: Option<bool>,
    mux: Option<RawMux>,
    keybinds: Option<RawKeybinds>,

    statusbar_enabled: Option<bool>,
    statusbar_app_line1_left: Option<String>,
    statusbar_app_line1_right: Option<String>,
    statusbar_app_line2_left: Option<String>,
    statusbar_app_line2_right: Option<String>,
    statusbar_time_format: Option<String>,
    statusbar_font_size: Option<f32>,
    statusbar_custom_commands: Option<std::collections::HashMap<String, RawCustomCommand>>,
    statusbar_refresh_rates: Option<std::collections::HashMap<String, u64>>,

    // ── UI chrome ──
    ui_theme: Option<String>,
    ui_theme_preset: Option<String>,
    ui_font_family: Option<String>,
    terminal_color_scheme: Option<String>,
    custom_color_schemes: Option<Vec<RawUserColorScheme>>,
    show_scrollbar: Option<String>,
    show_tab_bar: Option<bool>,
    bold_brightens_ansi_colors: Option<bool>,

    // ── Terminal behavior ──
    shell_path: Option<String>,
    shell_args: Option<Vec<String>>,
    scroll_speed: Option<u32>,
    alternate_scroll_enabled: Option<bool>,
    copy_on_select: Option<bool>,
    middle_click_paste: Option<bool>,
    /// New wire key (task0001/task0003). Wins over the legacy
    /// `shift_enter_as_alt_enter` boolean below whenever this key was
    /// PRESENT in the source JSON — including when it is explicitly
    /// `null` (FR5 / AC-3). `Option<Option<String>>` (rather than the
    /// plain `Option<String>` every other field in this struct uses)
    /// distinguishes "key absent" (`None`, the struct-level
    /// `#[serde(default)]`) from "key present with `null`"
    /// (`Some(None)`, produced by [`deserialize_present_option`]) so
    /// `merge_into` can resolve that precedence correctly; a plain
    /// `Option<String>` would conflate the two (both deserialize to
    /// `None`).
    #[serde(deserialize_with = "deserialize_present_option")]
    shift_enter_behavior: Option<Option<String>>,
    /// Legacy boolean, migration input only (FR5): `true` -> `AltEnter`,
    /// `false` -> `None`. Never written back to `settings.json`.
    shift_enter_as_alt_enter: Option<bool>,
    skk_mode: Option<bool>,
    bell_action: Option<String>,
    url_detection: Option<bool>,
    file_path_detection: Option<bool>,
    fold_enabled: Option<bool>,
    editor_command: Option<String>,
    /// src-tauri-flat ambiguous-width toggle. `true` ≡ `Wide`, `false` ≡
    /// `Narrow`. `native_poc.ambiguous_width_mode` (string form) wins
    /// over this when both are present.
    ambiguous_width: Option<bool>,

    // ── Notifications ──
    notification_enabled: Option<bool>,
    tab_activity_indicator: Option<bool>,
    notify_on_process_exit: Option<bool>,
    notify_on_output: Option<bool>,
    notify_on_bell: Option<bool>,
    agent_status_notifications: Option<bool>,

    // ── Language / logging ──
    language: Option<String>,
    log_recording_enabled: Option<bool>,

    // ── Markdown viewer (flat keys, src-tauri compatible) ──
    markdown_theme_follow_ui: Option<bool>,
    markdown_theme: Option<String>,
    markdown_theme_preset: Option<String>,
    markdown_body_font_family: Option<String>,
    markdown_code_font_family: Option<String>,
    markdown_font_size: Option<u32>,

    // ── Profiles / SSH / SFTP (src-tauri compatible). The entry types
    // come from the shared `app_settings` crate so per-field defaults
    // and null-handling match the legacy build exactly. ──
    profiles: Option<Vec<app_settings::Profile>>,
    ssh_command_path: Option<String>,
    ssh_connections: Option<Vec<app_settings::SshConnection>>,
    sftp_max_concurrent_uploads: Option<u16>,

    // ── native-poc-specific (nested) ──
    native_poc: Option<RawNativePoc>,
}

#[derive(Debug, serde::Deserialize)]
struct RawUserColorScheme {
    name: String,
    #[serde(default)]
    foreground: String,
    #[serde(default)]
    background: String,
    #[serde(default)]
    cursor: String,
    #[serde(default)]
    selection: String,
    #[serde(default)]
    ansi_colors: Vec<String>,
}

/// `statusbar` (the retired `mux.statusbar.*` object, formerly
/// deserialized into `RawMuxStatusbar`) is deliberately NOT a field here
/// (mux-status-bar-removal task0001, FR4/FR8b): with no field to name it,
/// serde's default unknown-field handling (no `deny_unknown_fields`
/// anywhere in this loader) leaves a stale `settings.json` still carrying
/// that key loading successfully, silently ignoring it.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawMux {
    prefix: Option<String>,
    tab_always_expand: Option<bool>,
    window_sidebar_overlay: Option<bool>,
    keybinds: Option<std::collections::HashMap<String, String>>,
}

/// Deserialize side of the nested `"keybinds"` block. Mirrors
/// `src-tauri`'s `AppSettings.keybinds` (snake_case string values).
/// Every field is `Option<String>` with `#[serde(default)]` so missing
/// keys, explicit `null`, and unknown keys all leave the corresponding
/// [`KeybindSettings`] field at its default.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawKeybinds {
    copy: Option<String>,
    paste: Option<String>,
    select_all: Option<String>,
    search: Option<String>,
    new_tab: Option<String>,
    new_tab_global: Option<String>,
    close_tab: Option<String>,
    next_tab: Option<String>,
    prev_tab: Option<String>,
    zoom_in: Option<String>,
    zoom_out: Option<String>,
    zoom_reset: Option<String>,
    toggle_fullscreen: Option<String>,
    open_settings: Option<String>,
    toggle_tab_bar: Option<String>,
    jump_to_prev_prompt: Option<String>,
    jump_to_next_prompt: Option<String>,
    profile_selector: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawCustomCommand {
    executable: String,
    interval_ms: Option<u64>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawNativePoc {
    ambiguous_width_mode: Option<String>,
    font_engine: Option<String>,
    image_memory_quota_mb: Option<u32>,
    ime: Option<RawIme>,
    font_family_fallback: Option<Vec<String>>,
    variable_font_axes: Option<std::collections::HashMap<String, f32>>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct RawIme {
    native_integration: Option<bool>,
}

impl RawSettings {
    /// Apply every present field onto `dst`. Absent fields leave the
    /// corresponding `Settings` field at its prior value (typically the
    /// default that `dst` was seeded with).
    fn merge_into(self, dst: &mut Settings) {
        // ── flat keys (src-tauri compatible) ──
        if let Some(v) = self.scrollback_lines {
            dst.scrollback_lines = v;
        }
        if let Some(v) = self.clipboard_read_osc52 {
            dst.clipboard_read_osc52 = v;
        }
        if let Some(v) = self.clipboard_max_size_osc52 {
            dst.clipboard_max_size_osc52 = v;
        }
        if let Some(v) = self.font_size {
            // `font_size` from `settings.json` is a logical-point value;
            // sanitize against absurd inputs that would render an
            // unusable grid (and would also break the cell-metrics
            // computation). The legacy WebView build does the same
            // clamp in `applySettings`.
            if v.is_finite() && v > 0.0 {
                dst.font_size = v;
            } else {
                log::warn!(
                    "settings.font_size: invalid value {:?}, keeping default {}",
                    v,
                    DEFAULT_FONT_SIZE_PT
                );
            }
        }
        if let Some(v) = self.padding {
            dst.padding = v;
        }
        if let Some(v) = self.cursor_style.filter(|s| !s.trim().is_empty()) {
            dst.cursor_style = CursorStyle::parse_or_warn(&v);
        }
        if let Some(v) = self.cursor_blink {
            dst.cursor_blink = v;
        }

        // Font fallback derived from the src-tauri-compatible flat keys.
        // primary -> secondary becomes `font_family_fallback`; the bundled
        // CJK / emoji fonts continue to live at the resolver layer. If
        // `native_poc.font_family_fallback` is also present it overrides
        // this further down (explicit wins).
        let mut fb = Vec::new();
        if let Some(v) = self.font_family_primary.filter(|s| !s.trim().is_empty()) {
            fb.push(v);
        }
        if let Some(v) = self.font_family_secondary.filter(|s| !s.trim().is_empty()) {
            fb.push(v);
        }
        if !fb.is_empty() {
            dst.font_family_fallback = fb;
        }

        if let Some(mux) = self.mux {
            if let Some(v) = mux.prefix.filter(|s| !s.trim().is_empty()) {
                dst.mux_prefix_key = v;
            }
            if let Some(v) = mux.tab_always_expand {
                dst.mux.tab_always_expand = v;
            }
            if let Some(v) = mux.window_sidebar_overlay {
                dst.mux.window_sidebar_overlay = v;
            }
            if let Some(kb) = mux.keybinds {
                for (action, spec) in kb {
                    // Unknown action names are ignored (forward compat).
                    if default_mux_action_chord(&action).is_none() {
                        // Pre-SPEC-mux-feature-cleanup actions can still
                        // be present in `settings.json` files written by
                        // the WebView build (or older native builds).
                        // They are dead bindings the WebView frontend
                        // ignores; silently drop them here so a normal
                        // launch doesn't repeat the same warn for every
                        // legacy entry.
                        if is_legacy_mux_action(&action) {
                            log::debug!(
                                "settings.mux.keybinds: legacy action {:?}, dropped",
                                action
                            );
                        } else {
                            log::warn!(
                                "settings.mux.keybinds: unknown action {:?}, ignored",
                                action
                            );
                        }
                        continue;
                    }
                    // Blank entries keep the default (matching mux.prefix).
                    if spec.trim().is_empty() {
                        continue;
                    }
                    match parse_mux_action_chord(&spec) {
                        Some(c) => {
                            dst.mux.keybinds.insert(action, c);
                        }
                        None => {
                            log::warn!(
                                "settings.mux.keybinds.{}: invalid chord {:?}, keeping default",
                                action,
                                spec
                            );
                        }
                    }
                }
            }
            // `mux.statusbar.*` (retired, mux-status-bar-removal task0001)
            // is deliberately not merged here — see `RawMux`'s doc comment
            // for the FR4/FR8b tolerance contract.
        }

        // keybinds (nested, snake_case). Blank / whitespace-only specs
        // are dropped so a `"copy": ""` entry keeps the default chord
        // (matching the `mux.prefix` treatment above).
        if let Some(kb) = self.keybinds {
            if let Some(v) = kb.copy.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.copy = v;
            }
            if let Some(v) = kb.paste.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.paste = v;
            }
            if let Some(v) = kb.select_all.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.select_all = v;
            }
            if let Some(v) = kb.search.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.search = v;
            }
            if let Some(v) = kb.new_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.new_tab = v;
            }
            if let Some(v) = kb.new_tab_global.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.new_tab_global = v;
            }
            if let Some(v) = kb.close_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.close_tab = v;
            }
            if let Some(v) = kb.next_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.next_tab = v;
            }
            if let Some(v) = kb.prev_tab.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.prev_tab = v;
            }
            if let Some(v) = kb.zoom_in.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_in = v;
            }
            if let Some(v) = kb.zoom_out.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_out = v;
            }
            if let Some(v) = kb.zoom_reset.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.zoom_reset = v;
            }
            if let Some(v) = kb.toggle_fullscreen.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.toggle_fullscreen = v;
            }
            if let Some(v) = kb.open_settings.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.open_settings = v;
            }
            if let Some(v) = kb.toggle_tab_bar.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.toggle_tab_bar = v;
            }
            if let Some(v) = kb.jump_to_prev_prompt.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.jump_to_prev_prompt = v;
            }
            if let Some(v) = kb.jump_to_next_prompt.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.jump_to_next_prompt = v;
            }
            if let Some(v) = kb.profile_selector.filter(|s| !s.trim().is_empty()) {
                dst.keybinds.profile_selector = v;
            }
        }

        // statusbar (flat src-tauri shape -> nested native-poc shape)
        if let Some(v) = self.statusbar_enabled {
            dst.statusbar.enabled = v;
        }
        if let Some(v) = self.statusbar_app_line1_left {
            dst.statusbar.app_line1_left = v;
        }
        if let Some(v) = self.statusbar_app_line1_right {
            dst.statusbar.app_line1_right = v;
        }
        if let Some(v) = self.statusbar_app_line2_left {
            dst.statusbar.app_line2_left = v;
        }
        if let Some(v) = self.statusbar_app_line2_right {
            dst.statusbar.app_line2_right = v;
        }
        if let Some(v) = self.statusbar_time_format {
            dst.statusbar.time_format = v;
        }
        if let Some(v) = self.statusbar_font_size {
            dst.statusbar.font_size = Some(v);
        }
        if let Some(v) = self.statusbar_custom_commands {
            dst.statusbar.custom_commands = v
                .into_iter()
                .map(|(k, c)| {
                    (
                        k,
                        CustomCommand {
                            executable: c.executable,
                            interval_ms: c.interval_ms.unwrap_or(1000),
                        },
                    )
                })
                .collect();
        }
        if let Some(v) = self.statusbar_refresh_rates {
            dst.statusbar.refresh_rates = v;
        }

        // ── UI chrome ──
        if let Some(v) = self.ui_theme {
            dst.ui_theme = UiTheme::parse_or_warn(&v);
        }
        if let Some(v) = self.ui_theme_preset {
            dst.ui_theme_preset = UiThemePreset::parse_or_warn(&v);
        }
        if let Some(v) = self.ui_font_family.filter(|s| !s.trim().is_empty()) {
            dst.ui_font_family = v;
        }
        if let Some(v) = self.terminal_color_scheme {
            dst.terminal_color_scheme = v;
        }
        if let Some(v) = self.custom_color_schemes {
            dst.custom_color_schemes = v
                .into_iter()
                .map(|r| UserColorScheme {
                    name: r.name,
                    foreground: r.foreground,
                    background: r.background,
                    cursor: r.cursor,
                    selection: r.selection,
                    ansi_colors: r.ansi_colors,
                })
                .collect();
        }
        if let Some(v) = self.show_scrollbar {
            dst.show_scrollbar = ScrollbarMode::parse_or_warn(&v);
        }
        if let Some(v) = self.show_tab_bar {
            dst.show_tab_bar = v;
        }
        if let Some(v) = self.bold_brightens_ansi_colors {
            dst.bold_brightens_ansi_colors = v;
        }

        // ── Terminal behavior ──
        if let Some(v) = self.shell_path.filter(|s| !s.trim().is_empty()) {
            dst.shell_path = v;
        }
        if let Some(v) = self.shell_args {
            dst.shell_args = v;
        }
        if let Some(v) = self.scroll_speed {
            // Mirrors src-tauri's MIN/MAX (1..=10). Out-of-range values
            // are clamped so a typo can't lock the wheel at 0 or fly the
            // viewport at e.g. 1000 lines/notch.
            dst.scroll_speed = v.clamp(1, 10);
        }
        if let Some(v) = self.alternate_scroll_enabled {
            dst.alternate_scroll_enabled = v;
        }
        if let Some(v) = self.copy_on_select {
            dst.copy_on_select = v;
        }
        if let Some(v) = self.middle_click_paste {
            dst.middle_click_paste = v;
        }
        // FR5 migration: the new key wins whenever it was PRESENT in the
        // source JSON — including an explicit `null`, which resolves to
        // the default (AC-3) rather than falling through to the legacy
        // boolean. Only when the new key is fully ABSENT does the legacy
        // boolean apply (true -> AltEnter, false -> None). Neither
        // present leaves `dst` at the default seeded by
        // `Settings::default()` (AltEnter). The legacy key is read-only
        // here and is never written back to settings.json.
        match self.shift_enter_behavior {
            Some(Some(v)) => dst.shift_enter_behavior = ShiftEnterBehavior::parse_or_warn(&v),
            Some(None) => dst.shift_enter_behavior = ShiftEnterBehavior::default(),
            None => {
                if let Some(legacy) = self.shift_enter_as_alt_enter {
                    dst.shift_enter_behavior = if legacy {
                        ShiftEnterBehavior::AltEnter
                    } else {
                        ShiftEnterBehavior::None
                    };
                }
            }
        }
        if let Some(v) = self.skk_mode {
            dst.skk_mode = v;
        }
        if let Some(v) = self.bell_action {
            dst.bell_action = BellAction::parse_or_warn(&v);
        }
        if let Some(v) = self.url_detection {
            dst.url_detection = v;
        }
        if let Some(v) = self.file_path_detection {
            dst.file_path_detection = v;
        }
        if let Some(v) = self.fold_enabled {
            dst.fold_enabled = v;
        }
        if let Some(v) = self.editor_command.filter(|s| !s.trim().is_empty()) {
            dst.editor_command = v;
        }
        if let Some(v) = self.notification_enabled {
            dst.notification_enabled = v;
        }
        if let Some(v) = self.tab_activity_indicator {
            dst.tab_activity_indicator = v;
        }
        if let Some(v) = self.notify_on_process_exit {
            dst.notify_on_process_exit = v;
        }
        if let Some(v) = self.notify_on_output {
            dst.notify_on_output = v;
        }
        if let Some(v) = self.notify_on_bell {
            dst.notify_on_bell = v;
        }
        if let Some(v) = self.agent_status_notifications {
            dst.agent_status_notifications = v;
        }

        // ── Language / logging ──
        if let Some(v) = self.language {
            dst.language = Language::parse_or_warn(&v);
        }
        if let Some(v) = self.log_recording_enabled {
            dst.log_recording_enabled = v;
        }

        // ── Markdown viewer ──
        if let Some(v) = self.markdown_theme_follow_ui {
            dst.markdown_theme_follow_ui = v;
        }
        if let Some(v) = self.markdown_theme {
            dst.markdown_theme = UiTheme::parse_or_warn(&v);
        }
        if let Some(v) = self.markdown_theme_preset {
            dst.markdown_theme_preset = UiThemePreset::parse_or_warn(&v);
        }
        // Fonts: empty string is a valid value (→ CSS fallback chain), so
        // unlike `ui_font_family` we do not drop blanks here.
        if let Some(v) = self.markdown_body_font_family {
            dst.markdown_body_font_family = v;
        }
        if let Some(v) = self.markdown_code_font_family {
            dst.markdown_code_font_family = v;
        }
        if let Some(v) = self.markdown_font_size {
            dst.markdown_font_size = v;
        }

        // ── Profiles / SSH / SFTP ──
        if let Some(v) = self.profiles {
            dst.profiles = v;
        }
        if let Some(v) = self.ssh_command_path.filter(|s| !s.trim().is_empty()) {
            dst.ssh_command_path = v;
        }
        if let Some(v) = self.ssh_connections {
            dst.ssh_connections = v;
        }
        if let Some(v) = self.sftp_max_concurrent_uploads {
            dst.sftp_max_concurrent_uploads = v;
        }

        // Flat-key bridge: only seed the native-poc enum when the user
        // hasn't already set the more explicit `native_poc.ambiguous_width_mode`
        // string. The native_poc block runs *after* this, so if both are
        // present the string form wins, matching the "explicit overrides
        // flat keys" pattern used elsewhere in this loader.
        if let Some(v) = self.ambiguous_width {
            dst.ambiguous_width_mode = if v {
                AmbiguousWidthMode::Wide
            } else {
                AmbiguousWidthMode::Narrow
            };
        }

        // ── native_poc.* (explicit overrides win over flat keys) ──
        if let Some(np) = self.native_poc {
            if let Some(v) = np.ambiguous_width_mode {
                dst.ambiguous_width_mode = parse_ambiguous_width_or_warn(&v);
            }
            if let Some(v) = np.font_engine {
                dst.font_engine = FontEngine::parse_or_warn(&v);
            }
            if let Some(v) = np.image_memory_quota_mb {
                dst.image_memory_quota_mb = v;
            }
            if let Some(ime) = np.ime {
                if let Some(b) = ime.native_integration {
                    dst.ime.native_integration = b;
                }
            }
            if let Some(v) = np.font_family_fallback {
                dst.font_family_fallback = v;
            }
            if let Some(v) = np.variable_font_axes {
                dst.variable_font_axes = v;
            }
        }
    }
}

fn parse_ambiguous_width_or_warn(spec: &str) -> AmbiguousWidthMode {
    match spec.trim().to_ascii_lowercase().as_str() {
        "wide" => AmbiguousWidthMode::Wide,
        "narrow" => AmbiguousWidthMode::Narrow,
        other => {
            warn_unknown_ambiguous_width_once(other);
            AmbiguousWidthMode::Narrow
        }
    }
}

fn warn_unknown_ambiguous_width_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.native_poc.ambiguous_width_mode: unknown value {:?}, falling back to \"narrow\"",
            owned
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mux_action_names_match_prefix_ssot() {
        // `MUX_ACTION_NAMES` is a second in-Rust list of the mux action names
        // whose authority is `crate::mux::prefix::DEFAULT_ACTION_BINDINGS`.
        // Assert they stay identical (same names, same order) so adding/removing
        // an action in the SSOT without updating this list fails CI instead of
        // silently dropping a default-seed.
        let ssot: Vec<&str> = crate::mux::prefix::DEFAULT_ACTION_BINDINGS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            MUX_ACTION_NAMES.as_slice(),
            ssot.as_slice(),
            "MUX_ACTION_NAMES drifted from prefix::DEFAULT_ACTION_BINDINGS"
        );
    }

    #[test]
    fn default_scrollback_lines_is_ten_thousand() {
        let s = Settings::new();
        assert_eq!(s.scrollback_lines, 10_000);
    }

    #[test]
    fn default_image_memory_quota_is_320_mb() {
        let s = Settings::new();
        assert_eq!(s.image_memory_quota_mb, 320);
    }

    #[test]
    fn default_clipboard_read_osc52_is_true() {
        let s = Settings::new();
        assert!(s.clipboard_read_osc52);
    }

    #[test]
    fn default_clipboard_max_size_osc52_is_10_mib() {
        let s = Settings::new();
        assert_eq!(s.clipboard_max_size_osc52, 10 * 1024 * 1024);
    }

    // ── TS-settings-1: mux.prefix_key default ────────────────────────────

    #[test]
    fn default_mux_prefix_key_is_ctrl_z() {
        let s = Settings::new();
        assert_eq!(s.mux_prefix_key, "Ctrl+Z");
    }

    #[test]
    fn default_mux_prefix_key_parses_to_default_chord() {
        // Cross-check: the default spec must parse cleanly under the
        // prefix-key parser introduced in Phase 4-C.
        let s = Settings::new();
        let chord =
            crate::mux::prefix::parse_prefix_key(&s.mux_prefix_key).expect("parse default chord");
        assert_eq!(chord, crate::mux::prefix::PrefixChord::default());
    }

    // ── TS-settings-1: statusbar defaults + position fallback ───────────

    #[test]
    fn default_statusbar_is_enabled() {
        let s = Settings::new();
        assert!(
            s.statusbar.enabled,
            "statusbar.enabled must default to true"
        );
    }

    #[test]
    fn statusbar_settings_default_round_trip() {
        // The Default impl on StatusBarSettings must match the value seeded
        // into the parent Settings struct so callers can compare safely.
        let s = Settings::new();
        assert_eq!(s.statusbar, StatusBarSettings::default());
    }

    // ── TS-20: status-bar settings extension defaults ─────────────────

    #[test]
    fn default_status_bar_app_line1_templates() {
        let s = Settings::new();
        assert_eq!(s.statusbar.app_line1_left, "{time}");
        assert_eq!(s.statusbar.app_line1_right, "{cwd}");
    }

    #[test]
    fn default_status_bar_app_line2_templates_are_empty() {
        let s = Settings::new();
        assert!(s.statusbar.app_line2_left.is_empty());
        assert!(s.statusbar.app_line2_right.is_empty());
    }

    #[test]
    fn default_status_bar_time_format_is_hhmmss() {
        let s = Settings::new();
        assert_eq!(s.statusbar.time_format, "HH:mm:ss");
    }

    #[test]
    fn default_status_bar_font_size_is_none() {
        let s = Settings::new();
        assert!(s.statusbar.font_size.is_none());
    }

    #[test]
    fn default_status_bar_custom_commands_is_empty() {
        let s = Settings::new();
        assert!(s.statusbar.custom_commands.is_empty());
    }

    #[test]
    fn default_status_bar_refresh_rates_is_empty() {
        let s = Settings::new();
        assert!(s.statusbar.refresh_rates.is_empty());
    }

    #[test]
    fn custom_command_default_interval_is_1000ms() {
        let c = CustomCommand::default();
        assert_eq!(c.interval_ms, 1000);
        assert!(c.executable.is_empty());
    }

    // ── TS-settings-1: ime.native_integration defaults to true ─────

    /// `Settings::default().ime.native_integration` must default to `true`.
    /// Phase 7 (JSON loader) will rely on this default when a settings file
    /// omits the `ime` block or the `native_integration` key. Pinning the
    /// shape here keeps the Phase 4-G factory's "opt-out only" contract.
    #[test]
    fn default_ime_native_integration_is_true() {
        let s = Settings::new();
        assert!(s.ime.native_integration);
    }

    #[test]
    fn ime_settings_default_round_trip() {
        let s = Settings::new();
        assert_eq!(s.ime, ImeSettings::default());
    }

    #[test]
    fn ime_settings_default_is_native_integration_true() {
        let ime = ImeSettings::default();
        assert!(ime.native_integration);
    }

    // ── font-swash-migration: FontEngine + font-related Settings ────────

    /// TS-font-1: `FontEngine::default()` is `Swash`.
    #[test]
    fn font_engine_default_is_swash() {
        assert_eq!(FontEngine::default(), FontEngine::Swash);
    }

    /// TS-font-2: parse `"ab_glyph"` succeeds; unknown values warn-log
    /// and fall back to Swash.
    #[test]
    fn font_engine_parses_known_values() {
        assert_eq!(FontEngine::parse_or_warn("swash"), FontEngine::Swash);
        assert_eq!(FontEngine::parse_or_warn("ab_glyph"), FontEngine::AbGlyph);
        assert_eq!(FontEngine::parse_or_warn("AbGlyph"), FontEngine::AbGlyph);
        assert_eq!(FontEngine::parse_or_warn("  swash  "), FontEngine::Swash);
    }

    #[test]
    fn font_engine_unknown_falls_back_to_swash() {
        assert_eq!(FontEngine::parse_or_warn("blink"), FontEngine::Swash);
        assert_eq!(FontEngine::parse_or_warn(""), FontEngine::Swash);
    }

    /// FR9 / Settings schema additions: the new font-related fields exist
    /// on Settings and carry sensible defaults.
    #[test]
    fn settings_carry_font_engine_default_swash() {
        let s = Settings::new();
        assert_eq!(s.font_engine, FontEngine::Swash);
    }

    #[test]
    fn settings_font_family_fallback_default_empty() {
        let s = Settings::new();
        assert!(s.font_family_fallback.is_empty());
    }

    #[test]
    fn settings_variable_font_axes_default_empty() {
        let s = Settings::new();
        assert!(s.variable_font_axes.is_empty());
    }

    // ── settings.json loader (Phase 7) ─────────────────────────────────

    fn load_json(s: &str) -> Settings {
        let raw: RawSettings = serde_json::from_str(s).expect("parse RawSettings");
        let mut base = Settings::default();
        raw.merge_into(&mut base);
        base
    }

    #[test]
    fn loader_empty_object_keeps_all_defaults() {
        let s = load_json("{}");
        let d = Settings::default();
        assert_eq!(s.scrollback_lines, d.scrollback_lines);
        assert_eq!(s.image_memory_quota_mb, d.image_memory_quota_mb);
        assert_eq!(s.clipboard_read_osc52, d.clipboard_read_osc52);
        assert_eq!(s.clipboard_max_size_osc52, d.clipboard_max_size_osc52);
        assert_eq!(s.mux_prefix_key, d.mux_prefix_key);
        assert_eq!(s.ambiguous_width_mode, d.ambiguous_width_mode);
        assert_eq!(s.font_engine, d.font_engine);
        assert_eq!(s.statusbar, d.statusbar);
        assert_eq!(s.ime, d.ime);
    }

    #[test]
    fn loader_unknown_keys_are_ignored() {
        // Forward compat: src-tauri may add keys native-poc does not yet
        // consume. They must not break the loader.
        let s = load_json(
            r#"{"some_future_key": 42, "another": {"nested": true}, "scrollback_lines": 1234}"#,
        );
        assert_eq!(s.scrollback_lines, 1234);
    }

    #[test]
    fn loader_explicit_null_falls_back_to_default() {
        let s = load_json(r#"{"scrollback_lines": null, "clipboard_read_osc52": null}"#);
        assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
        assert!(s.clipboard_read_osc52);
    }

    #[test]
    fn loader_flat_keys_are_applied() {
        let s = load_json(
            r#"{
                "scrollback_lines": 50000,
                "clipboard_read_osc52": false,
                "clipboard_max_size_osc52": 1024
            }"#,
        );
        assert_eq!(s.scrollback_lines, 50_000);
        assert!(!s.clipboard_read_osc52);
        assert_eq!(s.clipboard_max_size_osc52, 1024);
    }

    #[test]
    fn loader_mux_prefix_overrides_default() {
        let s = load_json(r#"{"mux": {"prefix": "Ctrl+A"}}"#);
        assert_eq!(s.mux_prefix_key, "Ctrl+A");
    }

    // ── TS-4: mux settings loader (tab_always_expand / keybinds) ─────────

    #[test]
    fn default_mux_settings_match_webview() {
        let s = Settings::new();
        assert!(!s.mux.tab_always_expand);
        // `Ctrl`-modified default action chords.
        let chord = |spec: &str| parse_mux_action_chord(spec).unwrap();
        assert_eq!(s.mux.keybinds.get("detach"), Some(&chord("Ctrl+D")));
        assert_eq!(s.mux.keybinds.get("new-window"), Some(&chord("Ctrl+C")));
        assert_eq!(s.mux.keybinds.get("next-window"), Some(&chord("Ctrl+N")));
        assert_eq!(s.mux.keybinds.get("prev-window"), Some(&chord("Ctrl+P")));
        assert_eq!(s.mux.keybinds.get("rename-window"), Some(&chord("Ctrl+R")));
        assert_eq!(s.mux.keybinds.get("move-window"), Some(&chord("Ctrl+T")));
    }

    #[test]
    fn loader_mux_tab_always_expand() {
        let s = load_json(r#"{"mux": {"tab_always_expand": true}}"#);
        assert!(s.mux.tab_always_expand);
    }

    // ── window_sidebar_overlay (task0001 AC-1/AC-2/AC-3/AC-4) ────────────

    #[test]
    fn loader_mux_window_sidebar_overlay_missing_defaults_overlay() {
        // AC-1: a settings JSON without the field resolves to the overlay
        // display mode (`true`).
        let s = load_json(r#"{"mux": {"prefix": "Ctrl+A"}}"#);
        assert!(s.mux.window_sidebar_overlay);
    }

    #[test]
    fn loader_mux_window_sidebar_overlay_null_resolves_overlay() {
        // AC-2: a settings JSON with the field `null` resolves to the
        // overlay display mode (the loader treats null as "not
        // specified", matching the missing-key case).
        let s = load_json(r#"{"mux": {"window_sidebar_overlay": null}}"#);
        assert!(s.mux.window_sidebar_overlay);
    }

    #[test]
    fn loader_mux_window_sidebar_overlay_explicit_false_is_persistent() {
        // AC-3: an explicit `false` resolves to the persistent display
        // mode in the runtime settings the GUI reads, overriding the
        // overlay default (compatibility guarantee: a saved persistent
        // choice is never changed under the user).
        let s = load_json(r#"{"mux": {"window_sidebar_overlay": false}}"#);
        assert!(!s.mux.window_sidebar_overlay);
    }

    #[test]
    fn loader_mux_window_sidebar_overlay_true() {
        // AC-4: `true` resolves to `true` in the runtime settings the GUI
        // reads.
        let s = load_json(r#"{"mux": {"window_sidebar_overlay": true}}"#);
        assert!(s.mux.window_sidebar_overlay);
    }

    #[test]
    fn loader_mux_keybinds_override_valid() {
        let s = load_json(r#"{"mux": {"keybinds": {"next-window": "j", "prev-window": "k"}}}"#);
        let chord = |spec: &str| parse_mux_action_chord(spec).unwrap();
        assert_eq!(s.mux.keybinds.get("next-window"), Some(&chord("j")));
        assert_eq!(s.mux.keybinds.get("prev-window"), Some(&chord("k")));
        // Untouched actions keep their defaults.
        assert_eq!(s.mux.keybinds.get("new-window"), Some(&chord("Ctrl+C")));
    }

    #[test]
    fn loader_mux_keybinds_modifier_chord_accepted() {
        // Regression: modifier-bearing chords (`Ctrl+D`, `Alt+M`, …) are now
        // first-class follow-ups, matching the WebView's
        // `matchActionBinding`. tmux.conf import writes these back when the
        // user binds, e.g., `bind C-d detach-client`.
        let s = load_json(r#"{"mux": {"keybinds": {"detach": "Ctrl+D", "next-window": "Alt+N"}}}"#);
        let ctrl_d = parse_mux_action_chord("Ctrl+D").unwrap();
        let alt_n = parse_mux_action_chord("Alt+N").unwrap();
        assert_eq!(s.mux.keybinds.get("detach"), Some(&ctrl_d));
        assert_eq!(s.mux.keybinds.get("next-window"), Some(&alt_n));
    }

    #[test]
    fn loader_mux_keybinds_unparseable_keeps_default() {
        // Garbage spec is still rejected and keeps the default chord.
        let s = load_json(r#"{"mux": {"keybinds": {"next-window": "+++"}}}"#);
        let default_n = parse_mux_action_chord("Ctrl+N").unwrap();
        assert_eq!(s.mux.keybinds.get("next-window"), Some(&default_n));
    }

    #[test]
    fn loader_mux_keybinds_empty_keeps_default() {
        let s = load_json(r#"{"mux": {"keybinds": {"next-window": ""}}}"#);
        let default_n = parse_mux_action_chord("Ctrl+N").unwrap();
        assert_eq!(s.mux.keybinds.get("next-window"), Some(&default_n));
    }

    #[test]
    fn loader_mux_keybinds_unknown_action_ignored() {
        let s = load_json(r#"{"mux": {"keybinds": {"frobnicate": "z"}}}"#);
        assert!(!s.mux.keybinds.contains_key("frobnicate"));
    }

    #[test]
    fn loader_mux_keybinds_legacy_actions_dropped_silently() {
        // Pre-cleanup `mux.keybinds` entries that the WebView build still
        // emits must be dropped (no map entry) without a `warn!` storm.
        // We can't easily assert "no warn fired" from here, but the
        // dropped-from-the-map invariant is the user-observable half.
        let s = load_json(
            r#"{
                "mux": {
                    "keybinds": {
                        "next-pane": "o",
                        "copy-mode": "Ctrl+[",
                        "paste": "Ctrl+]"
                    }
                }
            }"#,
        );
        assert!(!s.mux.keybinds.contains_key("next-pane"));
        assert!(!s.mux.keybinds.contains_key("copy-mode"));
        assert!(!s.mux.keybinds.contains_key("paste"));
    }

    /// FR4/FR8b (mux-status-bar-removal task0001, TS3): a `settings.json`
    /// written by an older eMterm build may still contain the retired
    /// `mux.statusbar` object (the removed mux status-bar settings
    /// schema). Loading it must not fail -- `RawMux` no longer names the
    /// key, so it is silently ignored, exactly like any other
    /// unrecognized JSON field.
    #[test]
    fn loader_tolerates_stale_mux_statusbar_key() {
        let s = load_json(
            r#"{
                "mux": {
                    "prefix": "Ctrl+A",
                    "statusbar": {
                        "enabled": true,
                        "left": "L",
                        "right": "R",
                        "commands": {
                            "branch": {"executable": "/usr/bin/git-branch", "interval_ms": 7500}
                        }
                    }
                }
            }"#,
        );
        assert_eq!(s.mux_prefix_key, "Ctrl+A");
    }

    // ── Notification settings ───────────────────────────────────────────

    #[test]
    fn default_notification_settings_match_webview_build() {
        // Mirrors src-tauri's AppSettings defaults: everything on except
        // notify_on_output (opt-in, mirrors the WebView build).
        let s = Settings::new();
        assert!(s.notification_enabled);
        assert!(s.tab_activity_indicator);
        assert!(s.notify_on_process_exit);
        assert!(!s.notify_on_output);
        assert!(s.notify_on_bell);
    }

    #[test]
    fn loader_notification_flat_keys_are_applied() {
        let s = load_json(
            r#"{
                "notification_enabled": false,
                "tab_activity_indicator": false,
                "notify_on_process_exit": false,
                "notify_on_output": true,
                "notify_on_bell": false
            }"#,
        );
        assert!(!s.notification_enabled);
        assert!(!s.tab_activity_indicator);
        assert!(!s.notify_on_process_exit);
        assert!(s.notify_on_output);
        assert!(!s.notify_on_bell);
    }

    #[test]
    fn loader_notification_null_keys_keep_defaults() {
        let s = load_json(
            r#"{
                "notification_enabled": null,
                "tab_activity_indicator": null,
                "notify_on_process_exit": null,
                "notify_on_output": null,
                "notify_on_bell": null
            }"#,
        );
        let d = Settings::default();
        assert_eq!(s.notification_enabled, d.notification_enabled);
        assert_eq!(s.tab_activity_indicator, d.tab_activity_indicator);
        assert_eq!(s.notify_on_process_exit, d.notify_on_process_exit);
        assert_eq!(s.notify_on_output, d.notify_on_output);
        assert_eq!(s.notify_on_bell, d.notify_on_bell);
    }

    // ── agent_status_notifications (task0007 AC-5) ───────────────────────

    #[test]
    fn default_agent_status_notifications_is_true() {
        assert!(Settings::new().agent_status_notifications);
    }

    #[test]
    fn loader_agent_status_notifications_flat_key_is_applied() {
        let s = load_json(r#"{"agent_status_notifications": false}"#);
        assert!(!s.agent_status_notifications);
    }

    #[test]
    fn loader_agent_status_notifications_null_keeps_default() {
        let s = load_json(r#"{"agent_status_notifications": null}"#);
        assert_eq!(
            s.agent_status_notifications,
            Settings::default().agent_status_notifications
        );
    }

    // ── language / log recording / skk_mode ─────────────────────────

    #[test]
    fn default_language_is_auto() {
        assert_eq!(Settings::new().language, Language::Auto);
    }

    #[test]
    fn default_log_recording_is_disabled() {
        assert!(!Settings::new().log_recording_enabled);
    }

    #[test]
    fn default_skk_mode_is_enabled() {
        assert!(Settings::new().skk_mode);
    }

    #[test]
    fn loader_language_log_recording_skk_mode_flat_keys_are_applied() {
        let s = load_json(
            r#"{
                "language": "ja",
                "log_recording_enabled": true,
                "skk_mode": false
            }"#,
        );
        assert_eq!(s.language, Language::Ja);
        assert!(s.log_recording_enabled);
        assert!(!s.skk_mode);
    }

    #[test]
    fn loader_language_parses_all_supported_values() {
        assert_eq!(
            load_json(r#"{"language": "auto"}"#).language,
            Language::Auto
        );
        assert_eq!(load_json(r#"{"language": "en"}"#).language, Language::En);
        assert_eq!(load_json(r#"{"language": "ja"}"#).language, Language::Ja);
        // Unknown values warn and fall back to auto.
        assert_eq!(load_json(r#"{"language": "fr"}"#).language, Language::Auto);
    }

    #[test]
    fn loader_language_log_recording_skk_mode_null_keys_keep_defaults() {
        let s = load_json(
            r#"{
                "language": null,
                "log_recording_enabled": null,
                "skk_mode": null
            }"#,
        );
        let d = Settings::default();
        assert_eq!(s.language, d.language);
        assert_eq!(s.log_recording_enabled, d.log_recording_enabled);
        assert_eq!(s.skk_mode, d.skk_mode);
    }

    #[test]
    fn loader_empty_mux_prefix_keeps_default() {
        let s = load_json(r#"{"mux": {"prefix": ""}}"#);
        assert_eq!(s.mux_prefix_key, DEFAULT_MUX_PREFIX_KEY);
    }

    #[test]
    fn loader_flat_statusbar_keys_map_to_nested() {
        let s = load_json(
            r#"{
                "statusbar_enabled": false,
                "statusbar_app_line1_left": "{hostname}",
                "statusbar_app_line1_right": "{git_branch}",
                "statusbar_app_line2_left": "L2L",
                "statusbar_app_line2_right": "L2R",
                "statusbar_time_format": "HH:mm",
                "statusbar_font_size": 18.5,
                "statusbar_refresh_rates": {"time": 2000, "git_branch": 10000}
            }"#,
        );
        assert!(!s.statusbar.enabled);
        assert_eq!(s.statusbar.app_line1_left, "{hostname}");
        assert_eq!(s.statusbar.app_line1_right, "{git_branch}");
        assert_eq!(s.statusbar.app_line2_left, "L2L");
        assert_eq!(s.statusbar.app_line2_right, "L2R");
        assert_eq!(s.statusbar.time_format, "HH:mm");
        assert_eq!(s.statusbar.font_size, Some(18.5));
        assert_eq!(s.statusbar.refresh_rates.get("time"), Some(&2000));
        assert_eq!(s.statusbar.refresh_rates.get("git_branch"), Some(&10000));
    }

    #[test]
    fn loader_statusbar_custom_commands_default_interval_when_omitted() {
        let s = load_json(
            r#"{
                "statusbar_custom_commands": {
                    "weather": {"executable": "/usr/bin/curl"}
                }
            }"#,
        );
        let c = s.statusbar.custom_commands.get("weather").unwrap();
        assert_eq!(c.executable, "/usr/bin/curl");
        assert_eq!(c.interval_ms, 1000);
    }

    #[test]
    fn loader_statusbar_custom_commands_explicit_interval_kept() {
        let s = load_json(
            r#"{
                "statusbar_custom_commands": {
                    "weather": {"executable": "x", "interval_ms": 30000}
                }
            }"#,
        );
        assert_eq!(
            s.statusbar
                .custom_commands
                .get("weather")
                .unwrap()
                .interval_ms,
            30_000
        );
    }

    #[test]
    fn loader_font_family_primary_secondary_populate_fallback() {
        let s = load_json(
            r#"{"font_family_primary": "JetBrains Mono", "font_family_secondary": "Noto Sans JP"}"#,
        );
        assert_eq!(
            s.font_family_fallback,
            vec!["JetBrains Mono".to_string(), "Noto Sans JP".to_string()]
        );
    }

    #[test]
    fn loader_blank_font_family_strings_are_dropped() {
        let s = load_json(r#"{"font_family_primary": "  ", "font_family_secondary": ""}"#);
        // Blank entries must not be pushed; the field stays empty
        // (matching Settings::default()).
        assert!(s.font_family_fallback.is_empty());
    }

    #[test]
    fn loader_native_poc_font_engine_overrides() {
        let s = load_json(r#"{"native_poc": {"font_engine": "ab_glyph"}}"#);
        assert_eq!(s.font_engine, FontEngine::AbGlyph);
    }

    #[test]
    fn loader_native_poc_ambiguous_width_wide() {
        let s = load_json(r#"{"native_poc": {"ambiguous_width_mode": "wide"}}"#);
        assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Wide);
    }

    #[test]
    fn loader_native_poc_ambiguous_width_unknown_falls_back_to_narrow() {
        let s = load_json(r#"{"native_poc": {"ambiguous_width_mode": "huge"}}"#);
        assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Narrow);
    }

    #[test]
    fn loader_native_poc_image_memory_quota_overrides() {
        let s = load_json(r#"{"native_poc": {"image_memory_quota_mb": 128}}"#);
        assert_eq!(s.image_memory_quota_mb, 128);
    }

    #[test]
    fn loader_native_poc_ime_native_integration_overrides() {
        let s = load_json(r#"{"native_poc": {"ime": {"native_integration": false}}}"#);
        assert!(!s.ime.native_integration);
    }

    #[test]
    fn loader_native_poc_font_family_fallback_overrides_flat_keys() {
        let s = load_json(
            r#"{
                "font_family_primary": "A",
                "native_poc": {"font_family_fallback": ["X", "Y", "Z"]}
            }"#,
        );
        assert_eq!(
            s.font_family_fallback,
            vec!["X".to_string(), "Y".to_string(), "Z".to_string()]
        );
    }

    #[test]
    fn loader_native_poc_variable_font_axes_overrides() {
        let s = load_json(r#"{"native_poc": {"variable_font_axes": {"wght": 700.0}}}"#);
        assert_eq!(s.variable_font_axes.get("wght").copied(), Some(700.0));
    }

    #[test]
    fn load_from_missing_file_returns_defaults() {
        let p = std::path::PathBuf::from("/tmp/__nonexistent_emterm_settings_xyz_998877.json");
        // Defensive: ensure the path really does not exist.
        let _ = std::fs::remove_file(&p);
        let s = Settings::load_from(&p);
        // Spot-check a couple of fields against Default.
        assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
        assert_eq!(s.mux_prefix_key, DEFAULT_MUX_PREFIX_KEY);
    }

    #[test]
    fn load_from_invalid_json_returns_defaults() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!(
            "emterm_settings_invalid_{}.json",
            std::process::id()
        ));
        std::fs::write(&p, b"{ not json").expect("write tmp settings");
        let s = Settings::load_from(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(s.scrollback_lines, DEFAULT_SCROLLBACK_LINES);
    }

    // ── font_size / padding / cursor_style / cursor_blink loader ──────

    #[test]
    fn default_font_size_is_13() {
        let s = Settings::new();
        assert!((s.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
    }

    #[test]
    fn default_padding_is_4() {
        let s = Settings::new();
        assert_eq!(s.padding, DEFAULT_PADDING_PX);
    }

    #[test]
    fn default_cursor_style_is_block() {
        let s = Settings::new();
        assert_eq!(s.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn default_cursor_blink_is_true() {
        let s = Settings::new();
        assert!(s.cursor_blink);
    }

    #[test]
    fn cursor_style_parses_known_values() {
        assert_eq!(CursorStyle::parse_or_warn("block"), CursorStyle::Block);
        assert_eq!(
            CursorStyle::parse_or_warn("Underline"),
            CursorStyle::Underline
        );
        assert_eq!(CursorStyle::parse_or_warn("BAR"), CursorStyle::Bar);
        assert_eq!(CursorStyle::parse_or_warn("beam"), CursorStyle::Bar);
        assert_eq!(CursorStyle::parse_or_warn("  block "), CursorStyle::Block);
    }

    #[test]
    fn cursor_style_unknown_falls_back_to_block() {
        assert_eq!(CursorStyle::parse_or_warn("rectangle"), CursorStyle::Block);
        assert_eq!(CursorStyle::parse_or_warn(""), CursorStyle::Block);
    }

    #[test]
    fn cursor_style_as_cursor_shape_u8_maps_block_underline_bar() {
        // AC-1: block -> 0, underline -> 1, bar -> 2.
        assert_eq!(CursorStyle::Block.as_cursor_shape_u8(), 0);
        assert_eq!(CursorStyle::Underline.as_cursor_shape_u8(), 1);
        assert_eq!(CursorStyle::Bar.as_cursor_shape_u8(), 2);
    }

    #[test]
    fn loader_font_size_overrides_default() {
        let s = load_json(r#"{"font_size": 15.5}"#);
        assert!((s.font_size - 15.5).abs() < f32::EPSILON);
    }

    #[test]
    fn loader_font_size_zero_or_negative_keeps_default() {
        let s_zero = load_json(r#"{"font_size": 0}"#);
        assert!((s_zero.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
        let s_neg = load_json(r#"{"font_size": -3}"#);
        assert!((s_neg.font_size - DEFAULT_FONT_SIZE_PT).abs() < f32::EPSILON);
    }

    #[test]
    fn loader_padding_overrides_default() {
        let s = load_json(r#"{"padding": 12}"#);
        assert_eq!(s.padding, 12);
    }

    #[test]
    fn loader_padding_zero_is_accepted() {
        let s = load_json(r#"{"padding": 0}"#);
        assert_eq!(s.padding, 0);
    }

    #[test]
    fn loader_cursor_style_overrides_default() {
        let s = load_json(r#"{"cursor_style": "bar"}"#);
        assert_eq!(s.cursor_style, CursorStyle::Bar);
    }

    #[test]
    fn loader_cursor_style_empty_keeps_default() {
        let s = load_json(r#"{"cursor_style": ""}"#);
        assert_eq!(s.cursor_style, CursorStyle::Block);
    }

    #[test]
    fn loader_cursor_blink_can_be_disabled() {
        let s = load_json(r#"{"cursor_blink": false}"#);
        assert!(!s.cursor_blink);
    }

    // ── shift_enter_behavior loader (task0001 AC-1 / AC-2) ─────────────

    #[test]
    fn default_shift_enter_behavior_is_alt_enter() {
        // AC-1: the default is `alt_enter`.
        let s = Settings::new();
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn shift_enter_behavior_parses_each_wire_value() {
        assert_eq!(
            ShiftEnterBehavior::parse_or_warn("none"),
            ShiftEnterBehavior::None
        );
        assert_eq!(
            ShiftEnterBehavior::parse_or_warn("alt_enter"),
            ShiftEnterBehavior::AltEnter
        );
        assert_eq!(
            ShiftEnterBehavior::parse_or_warn("kitty_csi_u"),
            ShiftEnterBehavior::KittyCsiU
        );
        assert_eq!(
            ShiftEnterBehavior::parse_or_warn("lf"),
            ShiftEnterBehavior::Lf
        );
    }

    #[test]
    fn loader_shift_enter_behavior_new_key_overrides_default_for_each_value() {
        // AC-2: new key present (each value) -> that value.
        let s = load_json(r#"{"shift_enter_behavior": "none"}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
        let s = load_json(r#"{"shift_enter_behavior": "alt_enter"}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
        let s = load_json(r#"{"shift_enter_behavior": "kitty_csi_u"}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::KittyCsiU);
        let s = load_json(r#"{"shift_enter_behavior": "lf"}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::Lf);
    }

    #[test]
    fn loader_shift_enter_behavior_null_keeps_default() {
        // AC-2: new key null -> default.
        let s = load_json(r#"{"shift_enter_behavior": null}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn loader_shift_enter_behavior_unknown_string_falls_back_to_default() {
        // AC-2: new key unknown string -> default.
        let s = load_json(r#"{"shift_enter_behavior": "bogus"}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn loader_shift_enter_behavior_legacy_key_only_migrates_true_to_alt_enter() {
        // AC-2 / FR5: legacy key only, true -> alt_enter.
        let s = load_json(r#"{"shift_enter_as_alt_enter": true}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn loader_shift_enter_behavior_legacy_key_only_migrates_false_to_none() {
        // AC-2 / FR5: legacy key only, false -> none.
        let s = load_json(r#"{"shift_enter_as_alt_enter": false}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
    }

    #[test]
    fn loader_shift_enter_behavior_both_keys_new_key_wins() {
        // AC-2: both keys present -> new key wins over the legacy value
        // (here the legacy value alone would resolve to `alt_enter`, but
        // the new key explicitly says `none`).
        let s = load_json(r#"{"shift_enter_behavior": "none", "shift_enter_as_alt_enter": true}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::None);
    }

    #[test]
    fn loader_shift_enter_behavior_neither_key_keeps_default() {
        // AC-2: neither key present -> default.
        let s = load_json(r#"{}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn loader_shift_enter_behavior_explicit_null_wins_over_legacy_true() {
        // AC-3: the new key present-but-null must be distinguished from
        // the new key being absent — present null resolves to the
        // default and wins over the legacy key, even though here the
        // legacy value alone would ALSO resolve to `alt_enter` (so this
        // case alone would not catch a regression to the old
        // "null == absent" behavior; see the `_false` case below).
        let s = load_json(r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": true}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn loader_shift_enter_behavior_explicit_null_wins_over_legacy_false() {
        // AC-3: present null -> default (`alt_enter`), NOT `none` (which
        // the legacy `false` value alone would produce). This is the
        // regression case: conflating "null" with "absent" would
        // incorrectly fall through to the legacy boolean here.
        let s = load_json(r#"{"shift_enter_behavior": null, "shift_enter_as_alt_enter": false}"#);
        assert_eq!(s.shift_enter_behavior, ShiftEnterBehavior::AltEnter);
    }

    #[test]
    fn load_from_valid_file_applies_overrides() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("emterm_settings_valid_{}.json", std::process::id()));
        std::fs::write(
            &p,
            br#"{
                "scrollback_lines": 7,
                "native_poc": {"ambiguous_width_mode": "wide", "font_engine": "ab_glyph"}
            }"#,
        )
        .expect("write tmp settings");
        let s = Settings::load_from(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(s.scrollback_lines, 7);
        assert_eq!(s.ambiguous_width_mode, AmbiguousWidthMode::Wide);
        assert_eq!(s.font_engine, FontEngine::AbGlyph);
    }

    // ── keybinds defaults + loader ─────────────────────────────────────

    #[test]
    fn default_keybinds_match_src_tauri() {
        let kb = Settings::new().keybinds;
        assert_eq!(kb.copy, "Ctrl+Shift+C");
        assert_eq!(kb.paste, "Ctrl+Shift+V");
        assert_eq!(kb.select_all, "Ctrl+Shift+A");
        assert_eq!(kb.search, "Ctrl+Shift+F");
        assert_eq!(kb.new_tab, "Ctrl+Shift+T");
        assert_eq!(kb.new_tab_global, "Ctrl+Shift+G");
        assert_eq!(kb.close_tab, "Ctrl+Shift+W");
        assert_eq!(kb.next_tab, "Ctrl+PageDown");
        assert_eq!(kb.prev_tab, "Ctrl+PageUp");
        assert_eq!(kb.zoom_in, "Ctrl+Plus");
        assert_eq!(kb.zoom_out, "Ctrl+Minus");
        assert_eq!(kb.zoom_reset, "Ctrl+0");
        assert_eq!(kb.toggle_fullscreen, "F11");
        assert_eq!(kb.open_settings, "Ctrl+,");
        assert_eq!(kb.toggle_tab_bar, "Ctrl+Shift+B");
        assert_eq!(kb.jump_to_prev_prompt, "Ctrl+Shift+ArrowUp");
        assert_eq!(kb.jump_to_next_prompt, "Ctrl+Shift+ArrowDown");
        assert_eq!(kb.profile_selector, "Ctrl+Shift+P");
    }

    #[test]
    fn keybinds_default_round_trip() {
        let s = Settings::new();
        assert_eq!(s.keybinds, KeybindSettings::default());
    }

    #[test]
    fn loader_keybinds_override_only_specified_keys() {
        let s = load_json(r#"{"keybinds": {"new_tab": "Ctrl+Shift+N", "copy": "Ctrl+Insert"}}"#);
        // Overridden keys take the new spec.
        assert_eq!(s.keybinds.new_tab, "Ctrl+Shift+N");
        assert_eq!(s.keybinds.copy, "Ctrl+Insert");
        // Everything else stays at the default.
        let d = KeybindSettings::default();
        assert_eq!(s.keybinds.paste, d.paste);
        assert_eq!(s.keybinds.close_tab, d.close_tab);
        assert_eq!(s.keybinds.next_tab, d.next_tab);
        assert_eq!(s.keybinds.prev_tab, d.prev_tab);
    }

    #[test]
    fn loader_keybinds_blank_specs_keep_defaults() {
        let s = load_json(r#"{"keybinds": {"copy": "", "paste": "   ", "new_tab": "Ctrl+T"}}"#);
        let d = KeybindSettings::default();
        // Blank / whitespace-only specs are dropped.
        assert_eq!(s.keybinds.copy, d.copy);
        assert_eq!(s.keybinds.paste, d.paste);
        // A non-blank spec still applies.
        assert_eq!(s.keybinds.new_tab, "Ctrl+T");
    }

    #[test]
    fn loader_keybinds_null_keeps_defaults() {
        let s = load_json(r#"{"keybinds": null}"#);
        assert_eq!(s.keybinds, KeybindSettings::default());
    }

    #[test]
    fn loader_keybinds_unknown_keys_do_not_panic() {
        let s =
            load_json(r#"{"keybinds": {"some_future_action": "Ctrl+Z", "copy": "Ctrl+Insert"}}"#);
        // Unknown keys are ignored; known keys still apply.
        assert_eq!(s.keybinds.copy, "Ctrl+Insert");
        assert_eq!(s.keybinds.paste, KeybindSettings::default().paste);
    }

    // ── Markdown viewer settings (Phase 1 / TS-9, TS-10) ────────────────

    #[test]
    fn markdown_settings_defaults_match_spec() {
        // SPEC §Settings: follow_ui=true, theme=System, preset=Purple,
        // fonts empty, size 14.
        let s = Settings::new();
        assert!(s.markdown_theme_follow_ui);
        assert_eq!(s.markdown_theme, UiTheme::System);
        assert_eq!(s.markdown_theme_preset, UiThemePreset::Purple);
        assert_eq!(s.markdown_body_font_family, "");
        assert_eq!(s.markdown_code_font_family, "");
        assert_eq!(s.markdown_font_size, 14);
    }

    #[test]
    fn loader_markdown_flat_keys_are_applied() {
        let s = load_json(
            r#"{
                "markdown_theme_follow_ui": false,
                "markdown_theme": "light",
                "markdown_theme_preset": "green",
                "markdown_body_font_family": "Noto Sans",
                "markdown_code_font_family": "Fira Code",
                "markdown_font_size": 16
            }"#,
        );
        assert!(!s.markdown_theme_follow_ui);
        assert_eq!(s.markdown_theme, UiTheme::Light);
        assert_eq!(s.markdown_theme_preset, UiThemePreset::Green);
        assert_eq!(s.markdown_body_font_family, "Noto Sans");
        assert_eq!(s.markdown_code_font_family, "Fira Code");
        assert_eq!(s.markdown_font_size, 16);
    }

    #[test]
    fn loader_markdown_null_keys_keep_defaults() {
        let s = load_json(
            r#"{
                "markdown_theme_follow_ui": null,
                "markdown_theme": null,
                "markdown_theme_preset": null,
                "markdown_body_font_family": null,
                "markdown_code_font_family": null,
                "markdown_font_size": null
            }"#,
        );
        let d = Settings::default();
        assert_eq!(s.markdown_theme_follow_ui, d.markdown_theme_follow_ui);
        assert_eq!(s.markdown_theme, d.markdown_theme);
        assert_eq!(s.markdown_theme_preset, d.markdown_theme_preset);
        assert_eq!(s.markdown_body_font_family, d.markdown_body_font_family);
        assert_eq!(s.markdown_code_font_family, d.markdown_code_font_family);
        assert_eq!(s.markdown_font_size, d.markdown_font_size);
    }

    #[test]
    fn loader_markdown_unknown_theme_falls_back() {
        let s = load_json(r#"{"markdown_theme": "chartreuse", "markdown_theme_preset": "cyan"}"#);
        // Unknown enum values coerce to documented defaults.
        assert_eq!(s.markdown_theme, UiTheme::System);
        assert_eq!(s.markdown_theme_preset, UiThemePreset::Purple);
    }

    // ── Profiles / SSH / SFTP ────────────────────────────────────────

    #[test]
    fn loader_profiles_and_ssh_defaults() {
        let s = load_json("{}");
        assert!(s.profiles.is_empty());
        assert!(s.ssh_connections.is_empty());
        assert_eq!(s.ssh_command_path, "");
        assert_eq!(
            s.sftp_max_concurrent_uploads,
            DEFAULT_SFTP_MAX_CONCURRENT_UPLOADS
        );
    }

    #[test]
    fn loader_profiles_and_ssh_parse_src_tauri_shape() {
        let s = load_json(
            r#"{
                "profiles": [
                    {
                        "name": "dev",
                        "shell_path": "/bin/zsh",
                        "shell_args": ["-l"],
                        "env_vars": "FOO=bar",
                        "working_directory": "/tmp",
                        "is_default": true,
                        "ssh_connection_name": "",
                        "wsl_distro_name": ""
                    },
                    { "name": "minimal" }
                ],
                "ssh_command_path": "/usr/bin/ssh",
                "ssh_connections": [
                    {
                        "name": "work",
                        "hostname": "example.com",
                        "port": 2222,
                        "username": "user",
                        "identity_file": "~/.ssh/id_rsa",
                        "ssh_options": [
                            { "key": "ServerAliveInterval", "value": "60" }
                        ]
                    },
                    { "name": "bare", "hostname": "h", "port": null }
                ],
                "sftp_max_concurrent_uploads": 8
            }"#,
        );
        assert_eq!(s.profiles.len(), 2);
        assert_eq!(s.profiles[0].name, "dev");
        assert!(s.profiles[0].is_default);
        assert_eq!(s.profiles[0].shell_args, vec!["-l".to_string()]);
        // Partial entries fill the app_settings per-field defaults.
        assert_eq!(s.profiles[1].name, "minimal");
        assert!(!s.profiles[1].is_default);
        assert_eq!(s.ssh_command_path, "/usr/bin/ssh");
        assert_eq!(s.ssh_connections.len(), 2);
        assert_eq!(s.ssh_connections[0].port, 2222);
        assert_eq!(s.ssh_connections[0].ssh_options.len(), 1);
        // `null` port falls back to 22 (src-tauri deserializer parity).
        assert_eq!(s.ssh_connections[1].port, 22);
        assert_eq!(s.sftp_max_concurrent_uploads, 8);
    }

    #[test]
    fn appearance_follow_ui_true_uses_ui_theme_source() {
        let s = load_json(
            r#"{
                "ui_theme": "dark",
                "ui_theme_preset": "blue",
                "markdown_theme_follow_ui": true,
                "markdown_theme": "light",
                "markdown_theme_preset": "green",
                "markdown_body_font_family": "Body",
                "markdown_code_font_family": "Code",
                "markdown_font_size": 20
            }"#,
        );
        let a = s.markdown_appearance();
        // follow_ui = true -> theme/preset come from the UI chrome source.
        assert_eq!(a.theme, UiTheme::Dark);
        assert_eq!(a.preset, UiThemePreset::Blue);
        // Fonts and size always come from the markdown_* keys.
        assert_eq!(a.body_font_family, "Body");
        assert_eq!(a.code_font_family, "Code");
        assert_eq!(a.font_size, 20);
    }

    #[test]
    fn appearance_follow_ui_false_uses_markdown_theme_source() {
        let s = load_json(
            r#"{
                "ui_theme": "dark",
                "ui_theme_preset": "blue",
                "markdown_theme_follow_ui": false,
                "markdown_theme": "light",
                "markdown_theme_preset": "green"
            }"#,
        );
        let a = s.markdown_appearance();
        // follow_ui = false -> theme/preset come from the markdown_* source.
        assert_eq!(a.theme, UiTheme::Light);
        assert_eq!(a.preset, UiThemePreset::Green);
    }
}
