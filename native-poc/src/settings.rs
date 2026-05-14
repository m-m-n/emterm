//! Runtime settings for native-poc.
//!
//! Phase 7 will load these from `~/.config/emterm/settings.json`. Until
//! that lands, sub-phase 3 introduces the in-memory shape so the renderer
//! has a single place to read ambiguous-width policy (and future fields)
//! from.

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

/// Default maximum OSC 52 clipboard payload size (10 MiB). Mirrors the
/// legacy `src-tauri/src/commands/config/settings.rs::default_clipboard_max_size_osc52`.
pub const DEFAULT_CLIPBOARD_MAX_SIZE_OSC52: u32 = 10 * 1024 * 1024;

/// Default prefix-key chord for mux mode. Matches the legacy WebView build
/// and the common tmux default. Parsed by
/// `crate::mux::prefix::parse_prefix_key` at startup; an invalid value falls
/// back to this default with a `warn` log so a typo in `settings.json`
/// cannot lock the user out of mux mode.
pub const DEFAULT_MUX_PREFIX_KEY: &str = "Ctrl+B";

/// Position of the egui status-bar widget relative to the terminal grid.
/// `Top` inserts an [`egui::TopBottomPanel::top`]; `Bottom` inserts an
/// [`egui::TopBottomPanel::bottom`]. Phase 4-D introduces this; later
/// phases (settings-UI) may surface a runtime toggle.
///
/// `Top` is constructed today only by [`StatusBarPosition::parse_or_warn`]
/// (Phase 7 will route `settings.json` through that helper). Until then
/// the bin path always sees `Bottom`, so we silence the dead-code lint
/// on the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusBarPosition {
    #[allow(dead_code)] // Phase 7: settings.json loader will construct this.
    Top,
    #[default]
    Bottom,
}

impl StatusBarPosition {
    /// Parse the textual spec from `settings.json`. Unknown values fall back
    /// to [`StatusBarPosition::Bottom`] and emit a single `warn`-level log
    /// for the duration of the process (subsequent unknown values are
    /// silently coerced). The warn-once latch is process-wide so a typo
    /// repeated across reloads doesn't flood the log.
    #[allow(dead_code)] // Phase 7: settings.json loader will call this.
    pub fn parse_or_warn(spec: &str) -> Self {
        match spec.trim().to_ascii_lowercase().as_str() {
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            other => {
                warn_unknown_position_once(other);
                Self::Bottom
            }
        }
    }
}

/// Process-wide latch for the "unknown statusbar.position" warning so a
/// typo in `settings.json` is logged once, not once per frame / reload.
#[allow(dead_code)] // Phase 7: invoked from settings.json loader via parse_or_warn.
fn warn_unknown_position_once(seen: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    let owned = seen.to_string();
    ONCE.call_once(move || {
        log::warn!(
            "settings.statusbar.position: unknown value {:?}, falling back to \"bottom\"",
            owned
        );
    });
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
    /// coerce). Matches the `StatusBarPosition::parse_or_warn` pattern.
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

/// Statusbar-related settings. Phase 4-D introduces the
/// `enabled` + `position` pair. Backward compatibility: an existing
/// `settings.json` without a `statusbar` key MUST still parse — see
/// [`Settings::default`] which seeds these to their built-in defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusBarSettings {
    /// When `false`, the status-bar widget is not inserted at all (the
    /// central terminal panel covers the full window). Default: `true`.
    pub enabled: bool,
    /// Panel placement; see [`StatusBarPosition`]. Default: `Bottom`.
    pub position: StatusBarPosition,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            position: StatusBarPosition::default(),
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
    /// Explicit emoji font family override. When `None`, the bundled
    /// Noto Color Emoji is used (plus Segoe UI Emoji as a Windows
    /// secondary fallback).
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub emoji_font: Option<String>,
    /// Variable-font axis settings (e.g. `("wght", 700.0)`). Currently
    /// captured for forward compatibility; swash adapter wiring lands
    /// later.
    #[allow(dead_code)] // Phase 7+: variable font axis support.
    pub variable_font_axes: std::collections::HashMap<String, f32>,
    /// Maximum number of rows preserved above the viewport before old lines
    /// are dropped. Plumbed to `TerminalCore::new` at tab spawn time.
    pub scrollback_lines: u32,
    /// Per-tab cap on image-overlay GPU memory in megabytes. Plumbed to
    /// `ImageLayer::new(quota_bytes)` at tab spawn time. Eviction is
    /// least-recently-used (see `crate::image::ImageLayerState`).
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
    /// `"Ctrl+B"`) and parsed at startup by
    /// `crate::mux::prefix::parse_prefix_key`. Phase 7 will surface this in
    /// `settings.json`; today the field is populated from
    /// [`DEFAULT_MUX_PREFIX_KEY`] and never mutated.
    #[allow(dead_code)] // Phase 4-D status bar / settings UI will consume this.
    pub mux_prefix_key: String,
    /// Status-bar widget configuration. See [`StatusBarSettings`]. Phase
    /// 4-D introduces this; today the value is always the default until
    /// Phase 7 wires `settings.json` loading.
    pub statusbar: StatusBarSettings,
    /// IME backend configuration. See [`ImeSettings`]. Phase 4-G
    /// introduces `native_integration` (default `true`). Phase 7 wires
    /// JSON loading; until then `Settings::default()` exercises the
    /// default shape only.
    #[allow(dead_code)] // Phase 7: settings.json loader will populate this.
    pub ime: ImeSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ambiguous_width_mode: AmbiguousWidthMode::default(),
            font_engine: FontEngine::default(),
            font_family_fallback: Vec::new(),
            emoji_font: None,
            variable_font_axes: std::collections::HashMap::new(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            image_memory_quota_mb: DEFAULT_IMAGE_MEMORY_QUOTA_MB,
            clipboard_read_osc52: true,
            clipboard_max_size_osc52: DEFAULT_CLIPBOARD_MAX_SIZE_OSC52,
            mux_prefix_key: DEFAULT_MUX_PREFIX_KEY.to_string(),
            statusbar: StatusBarSettings::default(),
            ime: ImeSettings::default(),
        }
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_mux_prefix_key_is_ctrl_b() {
        let s = Settings::new();
        assert_eq!(s.mux_prefix_key, "Ctrl+B");
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
    fn default_statusbar_is_enabled_at_bottom() {
        let s = Settings::new();
        assert!(
            s.statusbar.enabled,
            "statusbar.enabled must default to true"
        );
        assert_eq!(
            s.statusbar.position,
            StatusBarPosition::Bottom,
            "statusbar.position must default to Bottom"
        );
    }

    #[test]
    fn statusbar_position_parses_top_and_bottom() {
        assert_eq!(
            StatusBarPosition::parse_or_warn("top"),
            StatusBarPosition::Top
        );
        assert_eq!(
            StatusBarPosition::parse_or_warn("Top"),
            StatusBarPosition::Top
        );
        assert_eq!(
            StatusBarPosition::parse_or_warn("BOTTOM"),
            StatusBarPosition::Bottom
        );
        assert_eq!(
            StatusBarPosition::parse_or_warn("  bottom  "),
            StatusBarPosition::Bottom
        );
    }

    #[test]
    fn statusbar_position_unknown_falls_back_to_bottom() {
        // The first unknown value triggers a `warn!` (latched by `Once`);
        // subsequent calls still coerce to Bottom but do not re-warn. We
        // assert the coercion contract only (Once side-effect is observed
        // by reading the log; not under test here).
        assert_eq!(
            StatusBarPosition::parse_or_warn("middle"),
            StatusBarPosition::Bottom
        );
        assert_eq!(
            StatusBarPosition::parse_or_warn("side"),
            StatusBarPosition::Bottom
        );
        assert_eq!(
            StatusBarPosition::parse_or_warn(""),
            StatusBarPosition::Bottom
        );
    }

    #[test]
    fn statusbar_settings_default_round_trip() {
        // The Default impl on StatusBarSettings must match the value seeded
        // into the parent Settings struct so callers can compare safely.
        let s = Settings::new();
        assert_eq!(s.statusbar, StatusBarSettings::default());
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
    fn settings_emoji_font_default_none() {
        let s = Settings::new();
        assert!(s.emoji_font.is_none());
    }

    #[test]
    fn settings_variable_font_axes_default_empty() {
        let s = Settings::new();
        assert!(s.variable_font_axes.is_empty());
    }
}
