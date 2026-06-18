//! Shared scroll-position model.
//!
//! [`ScrollPosition`] is the viewport's place in the scrollback. It is a pure
//! value type with no app-, mux-, or render-layer dependencies, so both the
//! `app` controller (which owns the active unit's live value) and the pure
//! `mux::window_group` model (which parks per-pane positions) can depend on it
//! without forming an `app ↔ mux` module cycle. `app` re-exports it as
//! `crate::app::ScrollPosition` for backward compatibility.

/// Scrollback position.
///
/// `Live` means the user is tracking new output (auto-follow). When PTY
/// output arrives in this state, the viewport advances with it.
///
/// `OffsetFromLive(n)` means the user has scrolled back `n` rows into
/// scrollback. New PTY output preserves this offset so the user does not
/// get yanked back to the bottom mid-read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPosition {
    #[default]
    Live,
    OffsetFromLive(u32),
}
