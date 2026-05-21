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

use egui::Color32;

/// Parse a 6-digit `#RRGGBB` literal into [`Color32`]. `const` so the
/// resulting palette can live in `const`s at module scope.
const fn hex(rrggbb: u32) -> Color32 {
    let r = ((rrggbb >> 16) & 0xff) as u8;
    let g = ((rrggbb >> 8) & 0xff) as u8;
    let b = (rrggbb & 0xff) as u8;
    Color32::from_rgb(r, g, b)
}

// ──────────────────────────────────────────────────────────────────────
// Color roles (dark theme)
// ──────────────────────────────────────────────────────────────────────
//
// `#[allow(dead_code)]` is applied to the role constants that are not yet
// used in `ui/` — they sit here so future widgets (buttons, dialogs, …)
// can refer to MD3 roles by name without re-introducing hex literals.

/// Active states, focus borders, primary buttons, active tab indicator.
pub const PRIMARY: Color32 = hex(0xD0BCFF);
/// Text on `primary` surfaces.
#[allow(dead_code)]
pub const ON_PRIMARY: Color32 = hex(0x381E72);
/// Slider track, keybind capture, default badge.
#[allow(dead_code)]
pub const PRIMARY_CONTAINER: Color32 = hex(0x4F378B);
/// Text on `primary_container`.
#[allow(dead_code)]
pub const ON_PRIMARY_CONTAINER: Color32 = hex(0xEADDFF);

/// Tab bar, list item cards, color palette editor.
pub const SURFACE_CONTAINER: Color32 = hex(0x211F26);
/// Settings nav background.
#[allow(dead_code)]
pub const SURFACE_CONTAINER_LOW: Color32 = hex(0x1D1B20);
/// Modal dialog backgrounds.
#[allow(dead_code)]
pub const SURFACE_CONTAINER_HIGH: Color32 = hex(0x2B2930);
/// Hover states, color hex inputs.
#[allow(dead_code)]
pub const SURFACE_CONTAINER_HIGHEST: Color32 = hex(0x36343B);

/// Primary text.
#[allow(dead_code)]
pub const ON_SURFACE: Color32 = hex(0xE6E0E9);
/// Secondary text, inactive icons.
pub const ON_SURFACE_VARIANT: Color32 = hex(0xCAC4D0);
/// Subtle borders (tab bar bottom hairline, list separators).
pub const OUTLINE_VARIANT: Color32 = hex(0x49454F);

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
        assert_eq!(PRIMARY, Color32::from_rgb(0xD0, 0xBC, 0xFF));
        assert_eq!(SURFACE_CONTAINER, Color32::from_rgb(0x21, 0x1F, 0x26));
        assert_eq!(ON_SURFACE_VARIANT, Color32::from_rgb(0xCA, 0xC4, 0xD0));
        assert_eq!(OUTLINE_VARIANT, Color32::from_rgb(0x49, 0x45, 0x4F));
    }

    #[test]
    fn state_layer_alpha_scales_with_opacity() {
        let layer = state_layer(ON_SURFACE_VARIANT, STATE_LAYER_HOVER);
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
        assert_eq!(state_layer(PRIMARY, -1.0).a(), 0);
        assert_eq!(state_layer(PRIMARY, 5.0).a(), 255);
    }
}
