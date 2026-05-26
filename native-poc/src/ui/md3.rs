//! Material Design 3 (dark) design tokens for the native UI.
//!
//! Values mirror the WebView build's `src/styles.css` `--md-sys-color-*`
//! variables. Keeping the palette in one place means widgets can refer
//! to semantic roles (`surface_container`, `primary`, …) instead of
//! hex literals scattered across `ui/`.
//!
//! Currently dark-theme only. A future light-theme switch would add a
//! parallel module (`md3_light`) and pick one at runtime via the same
//! constant names.
//!
//! ## Preset palettes
//!
//! Each MD3 accent preset (`UiThemePreset::{Purple, Blue, Green,
//! Orange, Pink}`) ships its own surface / outline tints in the WebView
//! build (`src/settings/ui-theme-presets.ts`). [`set_preset`] copies
//! the matching `Palette` into a process-wide slot at app startup so
//! every accessor (`primary()`, `surface_container()`, …) returns the
//! user-configured hue. Until startup runs, accessors return the
//! purple defaults so unit tests that bypass `App::new` still see a
//! sensible palette.

use egui::Color32;

/// Parse a 6-digit `#RRGGBB` literal into [`Color32`]. `const` so the
/// resulting palette can live in `const`s at module scope.
const fn hex(rrggbb: u32) -> Color32 {
    let r = ((rrggbb >> 16) & 0xff) as u8;
    let g = ((rrggbb >> 8) & 0xff) as u8;
    let b = (rrggbb & 0xff) as u8;
    Color32::from_rgb(r, g, b)
}

/// Per-preset dark-variant token bundle. Field names mirror MD3
/// `--md-sys-color-*` variables one-to-one.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub surface_container: Color32,
    pub surface_container_low: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub outline_variant: Color32,
}

/// Purple (default) dark palette. Mirrors
/// `UI_THEME_PRESETS.purple.dark` in the WebView build.
const PALETTE_PURPLE: Palette = Palette {
    primary: hex(0xD0BCFF),
    on_primary: hex(0x381E72),
    primary_container: hex(0x4F378B),
    on_primary_container: hex(0xEADDFF),
    surface_container: hex(0x211F26),
    surface_container_low: hex(0x1D1B20),
    surface_container_high: hex(0x2B2930),
    surface_container_highest: hex(0x36343B),
    on_surface: hex(0xE6E0E9),
    on_surface_variant: hex(0xCAC4D0),
    outline_variant: hex(0x49454F),
};

const PALETTE_BLUE: Palette = Palette {
    primary: hex(0xA8C7FA),
    on_primary: hex(0x062E6F),
    primary_container: hex(0x0842A0),
    on_primary_container: hex(0xD3E3FD),
    surface_container: hex(0x1F2126),
    surface_container_low: hex(0x1A1C20),
    surface_container_high: hex(0x292B30),
    surface_container_highest: hex(0x34363B),
    on_surface: hex(0xE2E2E9),
    on_surface_variant: hex(0xC4C6D0),
    outline_variant: hex(0x44464F),
};

const PALETTE_GREEN: Palette = Palette {
    primary: hex(0x7DD3A8),
    on_primary: hex(0x003823),
    primary_container: hex(0x005234),
    on_primary_container: hex(0x98F0C2),
    surface_container: hex(0x1C201E),
    surface_container_low: hex(0x181C1A),
    surface_container_high: hex(0x262B28),
    surface_container_highest: hex(0x313633),
    on_surface: hex(0xDFE4DF),
    on_surface_variant: hex(0xBFC9C1),
    outline_variant: hex(0x404943),
};

const PALETTE_ORANGE: Palette = Palette {
    primary: hex(0xFFB877),
    on_primary: hex(0x4C2700),
    primary_container: hex(0x6C3A00),
    on_primary_container: hex(0xFFDCBE),
    surface_container: hex(0x261F18),
    surface_container_low: hex(0x211A13),
    surface_container_high: hex(0x302922),
    surface_container_highest: hex(0x3B342D),
    on_surface: hex(0xEDE0D4),
    on_surface_variant: hex(0xD4C4B1),
    outline_variant: hex(0x524436),
};

const PALETTE_PINK: Palette = Palette {
    primary: hex(0xFFB1C8),
    on_primary: hex(0x5E1133),
    primary_container: hex(0x7B2949),
    on_primary_container: hex(0xFFD9E2),
    surface_container: hex(0x271D21),
    surface_container_low: hex(0x221820),
    surface_container_high: hex(0x322830),
    surface_container_highest: hex(0x3D333A),
    on_surface: hex(0xEEDFE3),
    on_surface_variant: hex(0xD4BFC5),
    outline_variant: hex(0x514349),
};

/// Resolve the dark-variant palette for `preset`.
fn palette_for(preset: crate::settings::UiThemePreset) -> Palette {
    use crate::settings::UiThemePreset::*;
    match preset {
        Purple => PALETTE_PURPLE,
        Blue => PALETTE_BLUE,
        Green => PALETTE_GREEN,
        Orange => PALETTE_ORANGE,
        Pink => PALETTE_PINK,
    }
}

/// Process-wide palette slot seeded at startup by [`set_preset`].
/// Widgets read individual tokens through the accessors below so a
/// single `OnceLock` write is sufficient.
static PALETTE_SLOT: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();

/// Install the user's MD3 preset. Idempotent (`OnceLock` semantics) —
/// only the first call takes effect so a stray reload cannot drift the
/// chrome mid-session. Call once during app startup, before any widget
/// reads palette accessors.
pub fn set_preset(preset: crate::settings::UiThemePreset) {
    let _ = PALETTE_SLOT.set(palette_for(preset));
}

/// Backwards-compatible alias kept so other call sites that only care
/// about the accent migration keep working.
pub fn set_primary_preset(preset: crate::settings::UiThemePreset) {
    set_preset(preset);
}

/// Active palette resolved from `settings.ui_theme_preset`. Falls back
/// to [`PALETTE_PURPLE`] when [`set_preset`] has not run yet.
fn current() -> &'static Palette {
    PALETTE_SLOT.get().unwrap_or(&PALETTE_PURPLE)
}

// ──────────────────────────────────────────────────────────────────────
// Color-role accessors
// ──────────────────────────────────────────────────────────────────────

/// Active states, focus borders, primary buttons, active tab indicator.
pub fn primary() -> Color32 {
    current().primary
}

#[allow(dead_code)]
pub fn on_primary() -> Color32 {
    current().on_primary
}

#[allow(dead_code)]
pub fn primary_container() -> Color32 {
    current().primary_container
}

#[allow(dead_code)]
pub fn on_primary_container() -> Color32 {
    current().on_primary_container
}

/// Tab bar, list item cards, color palette editor.
pub fn surface_container() -> Color32 {
    current().surface_container
}

/// CSD title bar, settings nav background.
pub fn surface_container_low() -> Color32 {
    current().surface_container_low
}

#[allow(dead_code)]
pub fn surface_container_high() -> Color32 {
    current().surface_container_high
}

/// Hover states, color hex inputs.
pub fn surface_container_highest() -> Color32 {
    current().surface_container_highest
}

#[allow(dead_code)]
pub fn on_surface() -> Color32 {
    current().on_surface
}

/// Secondary text, inactive icons.
pub fn on_surface_variant() -> Color32 {
    current().on_surface_variant
}

/// Subtle borders (tab bar bottom hairline, list separators).
pub fn outline_variant() -> Color32 {
    current().outline_variant
}

// ──────────────────────────────────────────────────────────────────────
// State-layer helpers
// ──────────────────────────────────────────────────────────────────────

/// MD3 hover state-layer opacity (`0.08`). Apply over `currentColor`.
pub const STATE_LAYER_HOVER: f32 = 0.08;
/// MD3 focus/pressed state-layer opacity (`0.12`).
#[allow(dead_code)]
pub const STATE_LAYER_PRESSED: f32 = 0.12;

/// Multiply `base` by `opacity` to produce a state-layer fill.
///
/// The MD3 spec applies the layer in compositing space, but for opaque
/// backgrounds blending with the resolved background by `opacity` matches
/// the visual outcome and avoids needing a separate alpha pass.
pub fn state_layer(base: Color32, opacity: f32) -> Color32 {
    let a = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_decoding_matches_spec() {
        assert_eq!(PALETTE_PURPLE.primary, Color32::from_rgb(0xD0, 0xBC, 0xFF));
        assert_eq!(
            PALETTE_PURPLE.surface_container,
            Color32::from_rgb(0x21, 0x1F, 0x26)
        );
        assert_eq!(
            PALETTE_PURPLE.on_surface_variant,
            Color32::from_rgb(0xCA, 0xC4, 0xD0)
        );
        assert_eq!(
            PALETTE_PURPLE.outline_variant,
            Color32::from_rgb(0x49, 0x45, 0x4F)
        );
    }

    #[test]
    fn preset_primary_table_matches_webview() {
        use crate::settings::UiThemePreset::*;
        assert_eq!(
            palette_for(Purple).primary,
            Color32::from_rgb(0xD0, 0xBC, 0xFF)
        );
        assert_eq!(
            palette_for(Blue).primary,
            Color32::from_rgb(0xA8, 0xC7, 0xFA)
        );
        assert_eq!(
            palette_for(Green).primary,
            Color32::from_rgb(0x7D, 0xD3, 0xA8)
        );
        assert_eq!(
            palette_for(Orange).primary,
            Color32::from_rgb(0xFF, 0xB8, 0x77)
        );
        assert_eq!(
            palette_for(Pink).primary,
            Color32::from_rgb(0xFF, 0xB1, 0xC8)
        );
    }

    #[test]
    fn preset_surface_container_matches_webview() {
        use crate::settings::UiThemePreset::*;
        // Pink preset's surface_container differs from purple — this is
        // exactly the discrepancy the user noticed when running with
        // `ui_theme_preset: "pink"`: tab bar background still rendered as
        // purple's #211F26 instead of pink's #271D21.
        assert_eq!(
            palette_for(Pink).surface_container,
            Color32::from_rgb(0x27, 0x1D, 0x21)
        );
        assert_eq!(
            palette_for(Green).surface_container,
            Color32::from_rgb(0x1C, 0x20, 0x1E)
        );
    }

    #[test]
    fn state_layer_alpha_scales_with_opacity() {
        let layer = state_layer(PALETTE_PURPLE.on_surface_variant, STATE_LAYER_HOVER);
        // 0.08 * 255 = 20.4 → truncated to 20.
        // egui::Color32 stores premultiplied sRGB values, so the RGB
        // channels are no longer the raw base — `from_rgba_unmultiplied`
        // applies gamma-correct alpha pre-multiplication internally. We
        // only verify the alpha channel here; the channel-level blend
        // is exercised visually through the renderer.
        assert_eq!(layer.a(), 20);
    }

    #[test]
    fn state_layer_clamps_out_of_range_opacity() {
        assert_eq!(state_layer(PALETTE_PURPLE.primary, -1.0).a(), 0);
        assert_eq!(state_layer(PALETTE_PURPLE.primary, 5.0).a(), 255);
    }
}
