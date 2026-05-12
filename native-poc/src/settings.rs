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

#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub ambiguous_width_mode: AmbiguousWidthMode,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }
}
