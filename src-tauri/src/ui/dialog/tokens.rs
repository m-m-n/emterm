//! Shared layout / shape / shadow constants for modal dialogs.
//!
//! These mirror the normative `dialogs:` and `tokens.elevation:` blocks
//! in `doc/UI-DESIGN-GUIDELINES.yaml`. The drift test in
//! `crate::ui::dialog::tests` parses the yaml and asserts the numeric
//! values here match the SSOT.
//!
//! Callers (the `Dialog` helper itself, plus `profile_selector.rs`) use
//! these constants instead of hard-coding 28.0 / 24.0 / etc., so a
//! future token change updates every dialog at once.

use egui::Color32;

/// Scrim alpha (`dialogs.scrim: rgba(0, 0, 0, 0.50)`).
///
/// Stored as an `f32` in `[0.0, 1.0]` so the drift test can compare
/// against the parsed yaml string with negligible rounding error.
pub const SCRIM_ALPHA: f32 = 0.50;

/// Scrim color packed as an opaque-black + alpha pair, in egui's
/// premultiplied space (matches `profile_selector.rs`'s
/// `Color32::from_rgba_premultiplied(0, 0, 0, 128)`).
pub const SCRIM_COLOR: Color32 =
    Color32::from_rgba_premultiplied(0, 0, 0, (SCRIM_ALPHA * 255.0) as u8);

/// Dialog corner radius in logical pixels (`dialogs.layout.corner-radius:
/// 28px`, `tokens.shape.corner-extra-large`).
pub const CORNER_RADIUS: f32 = 28.0;

/// Dialog content padding in logical pixels (`dialogs.layout.padding:
/// 24px`, `tokens.spacing.lg`).
pub const PADDING: f32 = 24.0;

/// Gap between action buttons (`dialogs.layout.actions-gap: 8px`).
pub const ACTIONS_GAP: f32 = 8.0;

/// Vertical gap between the title and the first body element
/// (`dialogs.layout.title-to-body-margin: 16px`).
pub const TITLE_TO_BODY_MARGIN: f32 = 16.0;

/// Vertical gap above the actions row (`dialogs.layout.actions-top-margin:
/// 16px`).
pub const ACTIONS_TOP_MARGIN: f32 = 16.0;

/// Vertical gap between consecutive body widgets
/// (`dialogs.layout.body-item-spacing: 8px`). Applied to
/// `ui.spacing_mut().item_spacing.y` for the body region so that
/// label / input / hint rows breathe per MD3 spec instead of
/// collapsing to egui's 6px default.
pub const BODY_ITEM_SPACING: f32 = 8.0;

/// Fixed surface width for a standard dialog
/// (`dialogs.layout.width-standard: 480px`). The dialog helper applies
/// it as both `set_min_width` and `set_max_width` so the surface stays
/// pinned to this value across reopens.
#[allow(dead_code)]
pub const WIDTH_STANDARD: f32 = 480.0;

/// Fixed surface width for a compact dialog
/// (`dialogs.layout.width-compact: 400px`). Default for Rename / Move /
/// Upload / Overwrite / Close-guard.
pub const WIDTH_COMPACT: f32 = 400.0;

/// Title typescale font size (`title-large`).
pub const TITLE_LARGE_SIZE: f32 = 22.0;

/// Body typescale font size (`body-medium`).
#[allow(dead_code)]
pub const BODY_MEDIUM_SIZE: f32 = 14.0;

/// Elevation-3 shadow components (`dialogs.elevation: elevation-3`,
/// `box-shadow: 0 8px 32px rgba(0,0,0,0.30)`). Split into individual
/// constants so callers building [`egui::epaint::Shadow`] can read each
/// part directly.
pub const ELEVATION_SHADOW_OFFSET_Y: f32 = 8.0;
pub const ELEVATION_SHADOW_BLUR: f32 = 32.0;
pub const ELEVATION_SHADOW_SPREAD: f32 = 0.0;
/// Alpha component of the elevation-3 shadow (`0.30 * 255 ≈ 77`).
pub const ELEVATION_SHADOW_ALPHA: u8 = 77;

/// Pre-built [`egui::epaint::Shadow`] for the standard dialog elevation.
pub fn elevation_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: egui::vec2(0.0, ELEVATION_SHADOW_OFFSET_Y),
        blur: ELEVATION_SHADOW_BLUR,
        spread: ELEVATION_SHADOW_SPREAD,
        color: Color32::from_black_alpha(ELEVATION_SHADOW_ALPHA),
    }
}
