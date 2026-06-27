//! Material Design 3 design tokens for the native UI.
//!
//! Values mirror the WebView build's `src/styles.css` `--md-sys-color-*`
//! variables. Keeping the palette in one place means widgets can refer
//! to semantic roles (`surface_container`, `primary`, …) instead of
//! hex literals scattered across `ui/`.
//!
//! ## Preset palettes
//!
//! Each MD3 accent preset (`UiThemePreset::{Purple, Blue, Green,
//! Orange, Pink}`) ships its own surface / outline tints in the WebView
//! build (`src/settings/ui-theme-presets.ts`), in both dark and light
//! variants. [`set_preset`] copies the `Palette` matching the user's
//! preset × `ui_theme` brightness into a process-wide slot at app
//! startup so every accessor (`primary()`, `surface_container()`, …)
//! returns the user-configured hue. Until startup runs, accessors
//! return the purple-dark defaults so unit tests that bypass
//! `App::new` still see a sensible palette.
//!
//! `ui_theme=System` resolves to dark: the WebView build reads
//! `prefers-color-scheme`, but the native build has no desktop-portal
//! lookup wired in yet.

use egui::Color32;

/// Parse a 6-digit `#RRGGBB` literal into [`Color32`]. `const` so the
/// resulting palette can live in `const`s at module scope.
const fn hex(rrggbb: u32) -> Color32 {
    let r = ((rrggbb >> 16) & 0xff) as u8;
    let g = ((rrggbb >> 8) & 0xff) as u8;
    let b = (rrggbb & 0xff) as u8;
    Color32::from_rgb(r, g, b)
}

/// Per-preset token bundle (one per preset × brightness). Field names
/// mirror MD3 `--md-sys-color-*` variables one-to-one.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_container: Color32,
    pub on_primary_container: Color32,
    pub secondary_container: Color32,
    pub on_secondary_container: Color32,
    pub surface: Color32,
    pub surface_container: Color32,
    pub surface_container_low: Color32,
    pub surface_container_high: Color32,
    pub surface_container_highest: Color32,
    pub on_surface: Color32,
    pub on_surface_variant: Color32,
    pub surface_variant: Color32,
    pub outline: Color32,
    pub outline_variant: Color32,
    pub error_container: Color32,
    pub on_error_container: Color32,
}

/// Purple (default) dark palette. Mirrors
/// `UI_THEME_PRESETS.purple.dark` in the WebView build.
const PALETTE_PURPLE: Palette = Palette {
    primary: hex(0xD0BCFF),
    on_primary: hex(0x381E72),
    primary_container: hex(0x4F378B),
    on_primary_container: hex(0xEADDFF),
    secondary_container: hex(0x4A4458),
    on_secondary_container: hex(0xE8DEF8),
    surface: hex(0x141218),
    surface_container: hex(0x211F26),
    surface_container_low: hex(0x1D1B20),
    surface_container_high: hex(0x2B2930),
    surface_container_highest: hex(0x36343B),
    on_surface: hex(0xE6E0E9),
    on_surface_variant: hex(0xCAC4D0),
    surface_variant: hex(0x49454F),
    outline: hex(0x938F99),
    outline_variant: hex(0x49454F),
    error_container: hex(0x8C1D18),
    on_error_container: hex(0xF9DEDC),
};

const PALETTE_BLUE: Palette = Palette {
    primary: hex(0xA8C7FA),
    on_primary: hex(0x062E6F),
    primary_container: hex(0x0842A0),
    on_primary_container: hex(0xD3E3FD),
    secondary_container: hex(0x434659),
    on_secondary_container: hex(0xDEE2F9),
    surface: hex(0x111318),
    surface_container: hex(0x1F2126),
    surface_container_low: hex(0x1A1C20),
    surface_container_high: hex(0x292B30),
    surface_container_highest: hex(0x34363B),
    on_surface: hex(0xE2E2E9),
    on_surface_variant: hex(0xC4C6D0),
    surface_variant: hex(0x44464F),
    outline: hex(0x8E909A),
    outline_variant: hex(0x44464F),
    error_container: hex(0x8C1D18),
    on_error_container: hex(0xF9DEDC),
};

const PALETTE_GREEN: Palette = Palette {
    primary: hex(0x7DD3A8),
    on_primary: hex(0x003823),
    primary_container: hex(0x005234),
    on_primary_container: hex(0x98F0C2),
    secondary_container: hex(0x374B3E),
    on_secondary_container: hex(0xD0E8D4),
    surface: hex(0x101412),
    surface_container: hex(0x1C201E),
    surface_container_low: hex(0x181C1A),
    surface_container_high: hex(0x262B28),
    surface_container_highest: hex(0x313633),
    on_surface: hex(0xDFE4DF),
    on_surface_variant: hex(0xBFC9C1),
    surface_variant: hex(0x404943),
    outline: hex(0x8A938C),
    outline_variant: hex(0x404943),
    error_container: hex(0x8C1D18),
    on_error_container: hex(0xF9DEDC),
};

const PALETTE_ORANGE: Palette = Palette {
    primary: hex(0xFFB877),
    on_primary: hex(0x4C2700),
    primary_container: hex(0x6C3A00),
    on_primary_container: hex(0xFFDCBE),
    secondary_container: hex(0x56432B),
    on_secondary_container: hex(0xFADEBB),
    surface: hex(0x18120B),
    surface_container: hex(0x261F18),
    surface_container_low: hex(0x211A13),
    surface_container_high: hex(0x302922),
    surface_container_highest: hex(0x3B342D),
    on_surface: hex(0xEDE0D4),
    on_surface_variant: hex(0xD4C4B1),
    surface_variant: hex(0x524436),
    outline: hex(0x9D8E7D),
    outline_variant: hex(0x524436),
    error_container: hex(0x8C1D18),
    on_error_container: hex(0xF9DEDC),
};

const PALETTE_PINK: Palette = Palette {
    primary: hex(0xFFB1C8),
    on_primary: hex(0x5E1133),
    primary_container: hex(0x7B2949),
    on_primary_container: hex(0xFFD9E2),
    secondary_container: hex(0x5B3F47),
    on_secondary_container: hex(0xFFD9E2),
    surface: hex(0x1A1114),
    surface_container: hex(0x271D21),
    surface_container_low: hex(0x221820),
    surface_container_high: hex(0x322830),
    surface_container_highest: hex(0x3D333A),
    on_surface: hex(0xEEDFE3),
    on_surface_variant: hex(0xD4BFC5),
    surface_variant: hex(0x514349),
    outline: hex(0x9D8A90),
    outline_variant: hex(0x514349),
    error_container: hex(0x8C1D18),
    on_error_container: hex(0xF9DEDC),
};

/// Purple light palette. Mirrors `UI_THEME_PRESETS.purple.light` in
/// the WebView build.
const PALETTE_PURPLE_LIGHT: Palette = Palette {
    primary: hex(0x6750A4),
    on_primary: hex(0xFFFFFF),
    primary_container: hex(0xEADDFF),
    on_primary_container: hex(0x21005D),
    secondary_container: hex(0xE8DEF8),
    on_secondary_container: hex(0x1D192B),
    surface: hex(0xFEF7FF),
    surface_container: hex(0xF3EDF7),
    surface_container_low: hex(0xF7F2FA),
    surface_container_high: hex(0xECE6F0),
    surface_container_highest: hex(0xE6E0E9),
    on_surface: hex(0x1D1B20),
    on_surface_variant: hex(0x49454F),
    surface_variant: hex(0xE7E0EC),
    outline: hex(0x79747E),
    outline_variant: hex(0xCAC4D0),
    error_container: hex(0xF9DEDC),
    on_error_container: hex(0x410E0B),
};

const PALETTE_BLUE_LIGHT: Palette = Palette {
    primary: hex(0x0B57D0),
    on_primary: hex(0xFFFFFF),
    primary_container: hex(0xD3E3FD),
    on_primary_container: hex(0x041E49),
    secondary_container: hex(0xDEE2F9),
    on_secondary_container: hex(0x171B2C),
    surface: hex(0xF9F9FF),
    surface_container: hex(0xEFF0F6),
    surface_container_low: hex(0xF3F3FA),
    surface_container_high: hex(0xE8E9EF),
    surface_container_highest: hex(0xE2E2E9),
    on_surface: hex(0x1A1C20),
    on_surface_variant: hex(0x44464F),
    surface_variant: hex(0xE1E2EC),
    outline: hex(0x75767F),
    outline_variant: hex(0xC4C6D0),
    error_container: hex(0xF9DEDC),
    on_error_container: hex(0x410E0B),
};

const PALETTE_GREEN_LIGHT: Palette = Palette {
    primary: hex(0x006D3E),
    on_primary: hex(0xFFFFFF),
    primary_container: hex(0x98F0C3),
    on_primary_container: hex(0x002110),
    secondary_container: hex(0xD0E8D4),
    on_secondary_container: hex(0x0B1F13),
    surface: hex(0xF5FBF5),
    surface_container: hex(0xEBF1EB),
    surface_container_low: hex(0xEFF5EF),
    surface_container_high: hex(0xE5EBE5),
    surface_container_highest: hex(0xDEE4DF),
    on_surface: hex(0x181C1A),
    on_surface_variant: hex(0x404943),
    surface_variant: hex(0xDBE5DD),
    outline: hex(0x717972),
    outline_variant: hex(0xBFC9C1),
    error_container: hex(0xF9DEDC),
    on_error_container: hex(0x410E0B),
};

const PALETTE_ORANGE_LIGHT: Palette = Palette {
    primary: hex(0x8B5000),
    on_primary: hex(0xFFFFFF),
    primary_container: hex(0xFFDCBE),
    on_primary_container: hex(0x2D1600),
    secondary_container: hex(0xFADEBB),
    on_secondary_container: hex(0x271904),
    surface: hex(0xFFF8F4),
    surface_container: hex(0xF5EDEA),
    surface_container_low: hex(0xFAF2EE),
    surface_container_high: hex(0xEEE6E3),
    surface_container_highest: hex(0xE9E1DD),
    on_surface: hex(0x211A13),
    on_surface_variant: hex(0x524436),
    surface_variant: hex(0xF0E0CD),
    outline: hex(0x847465),
    outline_variant: hex(0xD4C4B1),
    error_container: hex(0xF9DEDC),
    on_error_container: hex(0x410E0B),
};

const PALETTE_PINK_LIGHT: Palette = Palette {
    primary: hex(0x984061),
    on_primary: hex(0xFFFFFF),
    primary_container: hex(0xFFD9E3),
    on_primary_container: hex(0x3E001D),
    secondary_container: hex(0xFFD9E2),
    on_secondary_container: hex(0x2B151C),
    surface: hex(0xFFF8F8),
    surface_container: hex(0xFAECEF),
    surface_container_low: hex(0xFDF0F2),
    surface_container_high: hex(0xF2E4E8),
    surface_container_highest: hex(0xEBDEE2),
    on_surface: hex(0x22191C),
    on_surface_variant: hex(0x514349),
    surface_variant: hex(0xF0DBE1),
    outline: hex(0x837379),
    outline_variant: hex(0xD4BFC5),
    error_container: hex(0xF9DEDC),
    on_error_container: hex(0x410E0B),
};

/// Resolve the palette for `preset` under `theme`. `System` resolves
/// to dark (no desktop-portal brightness lookup in the native build
/// yet — the WebView build reads `prefers-color-scheme` instead).
fn palette_for(preset: crate::settings::UiThemePreset, theme: crate::settings::UiTheme) -> Palette {
    use crate::settings::UiThemePreset::*;
    let light = theme == crate::settings::UiTheme::Light;
    match (preset, light) {
        (Purple, false) => PALETTE_PURPLE,
        (Blue, false) => PALETTE_BLUE,
        (Green, false) => PALETTE_GREEN,
        (Orange, false) => PALETTE_ORANGE,
        (Pink, false) => PALETTE_PINK,
        (Purple, true) => PALETTE_PURPLE_LIGHT,
        (Blue, true) => PALETTE_BLUE_LIGHT,
        (Green, true) => PALETTE_GREEN_LIGHT,
        (Orange, true) => PALETTE_ORANGE_LIGHT,
        (Pink, true) => PALETTE_PINK_LIGHT,
    }
}

/// Process-wide palette slot seeded at startup by [`set_preset`] and
/// re-written when the in-app settings panel changes the UI theme /
/// preset. Widgets read individual tokens through the accessors below.
/// `RwLock` (not `OnceLock`) so `App::apply_settings` can swap the
/// palette live; `Palette` is `Copy`, so readers take a snapshot and
/// never hold the lock across a frame.
static PALETTE_SLOT: std::sync::RwLock<Option<Palette>> = std::sync::RwLock::new(None);

/// Install the user's MD3 preset × brightness. Call during app startup
/// before any widget reads palette accessors; the settings panel calls
/// it again (via `App::apply_settings`) so a theme change re-skins the
/// chrome on the next frame.
pub fn set_preset(preset: crate::settings::UiThemePreset, theme: crate::settings::UiTheme) {
    *PALETTE_SLOT
        .write()
        .expect("md3 palette lock poisoned (writer panicked)") = Some(palette_for(preset, theme));
}

/// Active palette resolved from `settings.ui_theme_preset` ×
/// `settings.ui_theme`. Falls back to [`PALETTE_PURPLE`] when
/// [`set_preset`] has not run yet.
fn current() -> Palette {
    PALETTE_SLOT
        .read()
        .expect("md3 palette lock poisoned (writer panicked)")
        .unwrap_or(PALETTE_PURPLE)
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

/// Selected nav item background (settings category nav).
pub fn secondary_container() -> Color32 {
    current().secondary_container
}

/// Text/icon color over [`secondary_container`].
pub fn on_secondary_container() -> Color32 {
    current().on_secondary_container
}

/// Base surface (settings content area background).
pub fn surface() -> Color32 {
    current().surface
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

/// Form-control borders (MD3 outlined text fields / selects).
pub fn outline() -> Color32 {
    current().outline
}

/// Subtle borders (tab bar bottom hairline, list separators).
pub fn outline_variant() -> Color32 {
    current().outline_variant
}

/// Readonly inputs, toggle track (off), keybind input background. Mirrors
/// the WebView's `--md-sys-color-surface-variant` per preset.
#[allow(dead_code)]
pub fn surface_variant() -> Color32 {
    current().surface_variant
}

/// Destructive button background (dialog destructive primary, error
/// banner). Mirrors `--md-sys-color-error-container`.
pub fn error_container() -> Color32 {
    current().error_container
}

/// Text/icon over [`error_container`]. Mirrors
/// `--md-sys-color-on-error-container`.
pub fn on_error_container() -> Color32 {
    current().on_error_container
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
        use crate::settings::UiTheme::Dark;
        use crate::settings::UiThemePreset::*;
        assert_eq!(
            palette_for(Purple, Dark).primary,
            Color32::from_rgb(0xD0, 0xBC, 0xFF)
        );
        assert_eq!(
            palette_for(Blue, Dark).primary,
            Color32::from_rgb(0xA8, 0xC7, 0xFA)
        );
        assert_eq!(
            palette_for(Green, Dark).primary,
            Color32::from_rgb(0x7D, 0xD3, 0xA8)
        );
        assert_eq!(
            palette_for(Orange, Dark).primary,
            Color32::from_rgb(0xFF, 0xB8, 0x77)
        );
        assert_eq!(
            palette_for(Pink, Dark).primary,
            Color32::from_rgb(0xFF, 0xB1, 0xC8)
        );
    }

    #[test]
    fn preset_surface_container_matches_webview() {
        use crate::settings::UiTheme::Dark;
        use crate::settings::UiThemePreset::*;
        // Pink preset's surface_container differs from purple — this is
        // exactly the discrepancy the user noticed when running with
        // `ui_theme_preset: "pink"`: tab bar background still rendered as
        // purple's #211F26 instead of pink's #271D21.
        assert_eq!(
            palette_for(Pink, Dark).surface_container,
            Color32::from_rgb(0x27, 0x1D, 0x21)
        );
        assert_eq!(
            palette_for(Green, Dark).surface_container,
            Color32::from_rgb(0x1C, 0x20, 0x1E)
        );
    }

    #[test]
    fn light_palette_matches_webview() {
        use crate::settings::UiTheme::Light;
        use crate::settings::UiThemePreset::*;
        // Spot-check `UI_THEME_PRESETS.*.light` in
        // `src/settings/ui-theme-presets.ts`.
        let purple = palette_for(Purple, Light);
        assert_eq!(purple.primary, Color32::from_rgb(0x67, 0x50, 0xA4));
        assert_eq!(
            purple.surface_container,
            Color32::from_rgb(0xF3, 0xED, 0xF7)
        );
        assert_eq!(purple.on_surface, Color32::from_rgb(0x1D, 0x1B, 0x20));
        assert_eq!(
            palette_for(Blue, Light).primary,
            Color32::from_rgb(0x0B, 0x57, 0xD0)
        );
        assert_eq!(
            palette_for(Green, Light).primary,
            Color32::from_rgb(0x00, 0x6D, 0x3E)
        );
        assert_eq!(
            palette_for(Orange, Light).primary,
            Color32::from_rgb(0x8B, 0x50, 0x00)
        );
        assert_eq!(
            palette_for(Pink, Light).primary,
            Color32::from_rgb(0x98, 0x40, 0x61)
        );
    }

    #[test]
    fn error_container_is_hue_agnostic_per_brightness() {
        // FR6: all dark presets share #8C1D18 / #F9DEDC, and all light
        // presets share #F9DEDC / #410E0B. This guards against a future
        // refactor accidentally re-coupling them to the accent hue.
        use crate::settings::UiTheme::{Dark, Light};
        use crate::settings::UiThemePreset::*;
        for preset in [Purple, Blue, Green, Orange, Pink] {
            assert_eq!(
                palette_for(preset, Dark).error_container,
                Color32::from_rgb(0x8C, 0x1D, 0x18),
                "dark error_container for {preset:?}"
            );
            assert_eq!(
                palette_for(preset, Dark).on_error_container,
                Color32::from_rgb(0xF9, 0xDE, 0xDC),
                "dark on_error_container for {preset:?}"
            );
            assert_eq!(
                palette_for(preset, Light).error_container,
                Color32::from_rgb(0xF9, 0xDE, 0xDC),
                "light error_container for {preset:?}"
            );
            assert_eq!(
                palette_for(preset, Light).on_error_container,
                Color32::from_rgb(0x41, 0x0E, 0x0B),
                "light on_error_container for {preset:?}"
            );
        }
    }

    #[test]
    fn surface_variant_matches_webview_per_preset() {
        // Spot-check that `surface-variant` in `Palette` mirrors the
        // values in `ui-theme-presets.ts` for the affected presets.
        use crate::settings::UiTheme::{Dark, Light};
        use crate::settings::UiThemePreset::*;
        assert_eq!(
            palette_for(Purple, Dark).surface_variant,
            Color32::from_rgb(0x49, 0x45, 0x4F)
        );
        assert_eq!(
            palette_for(Blue, Dark).surface_variant,
            Color32::from_rgb(0x44, 0x46, 0x4F)
        );
        assert_eq!(
            palette_for(Green, Dark).surface_variant,
            Color32::from_rgb(0x40, 0x49, 0x43)
        );
        assert_eq!(
            palette_for(Orange, Dark).surface_variant,
            Color32::from_rgb(0x52, 0x44, 0x36)
        );
        assert_eq!(
            palette_for(Pink, Dark).surface_variant,
            Color32::from_rgb(0x51, 0x43, 0x49)
        );
        assert_eq!(
            palette_for(Purple, Light).surface_variant,
            Color32::from_rgb(0xE7, 0xE0, 0xEC)
        );
        assert_eq!(
            palette_for(Orange, Light).surface_variant,
            Color32::from_rgb(0xF0, 0xE0, 0xCD)
        );
        assert_eq!(
            palette_for(Pink, Light).surface_variant,
            Color32::from_rgb(0xF0, 0xDB, 0xE1)
        );
    }

    #[test]
    fn system_theme_resolves_to_dark() {
        use crate::settings::UiTheme::{Dark, System};
        use crate::settings::UiThemePreset::Purple;
        // No desktop-portal brightness lookup yet — System must keep
        // matching the dark palette so existing setups don't shift.
        assert_eq!(
            palette_for(Purple, System).surface_container,
            palette_for(Purple, Dark).surface_container
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
