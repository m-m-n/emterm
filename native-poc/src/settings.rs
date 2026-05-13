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

#[derive(Debug, Clone)]
pub struct Settings {
    pub ambiguous_width_mode: AmbiguousWidthMode,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ambiguous_width_mode: AmbiguousWidthMode::default(),
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            image_memory_quota_mb: DEFAULT_IMAGE_MEMORY_QUOTA_MB,
            clipboard_read_osc52: true,
            clipboard_max_size_osc52: DEFAULT_CLIPBOARD_MAX_SIZE_OSC52,
            mux_prefix_key: DEFAULT_MUX_PREFIX_KEY.to_string(),
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
}
