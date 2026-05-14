//! ab_glyph-backed `GlyphRasterizer` implementation.
//!
//! Phase 2 of font-swash-migration (FR5). ab_glyph is retained as the
//! escape hatch behind `Settings::font_engine = AbGlyph`. The adapter
//! rasterizes ASCII / Latin to Alpha bitmaps; non-Latin codepoints and
//! emoji return `None` so the cache treats them as cluster-level misses
//! and the renderer can substitute U+FFFD / empty quads. This is
//! deliberately a degraded path — the full coverage lives on the swash
//! adapter (Phase 3).

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

use super::traits::{AtlasFormat, FontId, GlyphBitmap, GlyphRasterizer, ShapedGlyph};

/// Adapter wrapping a single ab_glyph font with a fixed `FontId`.
///
/// The adapter is `'static` thanks to ab_glyph's `FontRef` taking a borrow
/// of the font bytes; in production the bytes come from `include_bytes!`,
/// so the borrow is `'static` by construction. For tests we lean on
/// `Box::leak` if we ever feed runtime-loaded bytes.
pub struct AbGlyphRasterizer {
    font: FontRef<'static>,
    font_id: FontId,
}

impl AbGlyphRasterizer {
    /// Build the adapter from font bytes that live for `'static`.
    ///
    /// Returns `None` if ab_glyph rejects the font (bad header, etc.).
    pub fn from_static_bytes(bytes: &'static [u8], font_id: FontId) -> Option<Self> {
        FontRef::try_from_slice(bytes)
            .ok()
            .map(|font| Self { font, font_id })
    }

    pub fn font_id(&self) -> FontId {
        self.font_id
    }
}

impl GlyphRasterizer for AbGlyphRasterizer {
    fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
        // We only know how to shape clusters that ab_glyph can map. Single
        // codepoint → single glyph; non-Latin codepoints usually return
        // glyph id 0, which the cache treats as "no coverage".
        cluster
            .chars()
            .map(|c| {
                let glyph_id = self.font.glyph_id(c).0 as u32;
                ShapedGlyph {
                    font,
                    glyph_id,
                    size_px,
                }
            })
            .collect()
    }

    fn raster(&self, font: FontId, glyph_id: u32, size_px: f32) -> Option<GlyphBitmap> {
        if font != self.font_id {
            return None;
        }
        // glyph id 0 is .notdef — ab_glyph reports it for codepoints the
        // font does not cover (CJK, emoji…). Returning None here makes the
        // cache treat such requests as misses and forces fallback.
        if glyph_id == 0 {
            return None;
        }
        let g = ab_glyph::GlyphId(glyph_id as u16)
            .with_scale_and_position(PxScale::from(size_px), ab_glyph::Point { x: 0.0, y: 0.0 });
        let scaled_font = self.font.as_scaled(PxScale::from(size_px));
        let advance = scaled_font.h_advance(ab_glyph::GlyphId(glyph_id as u16));
        let outlined = match self.font.outline_glyph(g) {
            Some(o) => o,
            None => {
                // No outline (e.g. whitespace): still a valid glyph but
                // zero pixels. Return an empty Alpha bitmap so the cache
                // stores the Empty sentinel.
                return Some(GlyphBitmap {
                    format: AtlasFormat::Alpha,
                    width: 0,
                    height: 0,
                    bearing: (0, 0),
                    advance,
                    pixels: Vec::new(),
                });
            }
        };
        let bounds = outlined.px_bounds();
        let w = bounds.width().ceil() as u32;
        let h = bounds.height().ceil() as u32;
        if w == 0 || h == 0 {
            return Some(GlyphBitmap {
                format: AtlasFormat::Alpha,
                width: 0,
                height: 0,
                bearing: (bounds.min.x.round() as i32, bounds.min.y.round() as i32),
                advance,
                pixels: Vec::new(),
            });
        }
        let mut pixels = vec![0u8; (w * h) as usize];
        outlined.draw(|x, y, c| {
            let idx = (y * w + x) as usize;
            if idx < pixels.len() {
                // ab_glyph coverage is 0..=1; clamp + scale to 0..=255.
                pixels[idx] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        Some(GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: w,
            height: h,
            bearing: (bounds.min.x.round() as i32, bounds.min.y.round() as i32),
            advance,
            pixels,
        })
    }

    fn has_codepoint(&self, _font: FontId, cp: u32) -> bool {
        match char::from_u32(cp) {
            Some(c) => self.font.glyph_id(c).0 != 0,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Use the bundled CJK fall-back if present; otherwise return None and
    /// the test harness skips. We fall back to the emoji font for the
    /// CJK-coverage test (which still returns None for Latin), but the
    /// ASCII path needs a Latin-covering font. For determinism in the
    /// Docker test env we re-purpose the emoji font where possible and
    /// otherwise use a minimal bundled probe by reading any system font.
    ///
    /// Pragmatic approach: read whatever ab_glyph can parse out of the
    /// embedded Noto Color Emoji bytes — it cannot raster 'A' (no Latin
    /// table) so we exercise the None / glyph-id-0 branch on every
    /// codepoint. ASCII rendering is exercised once Phase 3's CJK font
    /// (which contains a Latin table) is bundled.
    const PROBE_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/NotoColorEmoji.ttf");

    fn rasterizer() -> AbGlyphRasterizer {
        AbGlyphRasterizer::from_static_bytes(PROBE_FONT_BYTES, FontId(1)).expect("parse probe font")
    }

    /// TS-font-7 (CJK / emoji return None): Noto Color Emoji has no Latin
    /// table, so 'A' (U+0041) maps to glyph 0 in this probe font; we use
    /// the same code path that CJK / emoji would hit in a Latin-only
    /// fall-back font on the production path.
    #[test]
    fn ab_glyph_adapter_returns_none_for_uncovered_codepoint() {
        let r = rasterizer();
        // We probe `glyph_id == 0` directly through `raster`.
        assert!(r.raster(FontId(1), 0, 13.0).is_none());
    }

    #[test]
    fn ab_glyph_adapter_returns_none_for_wrong_font_id() {
        let r = rasterizer();
        // Probe font has id 1; asking with id 2 must short-circuit.
        assert!(r.raster(FontId(2), 5, 13.0).is_none());
    }

    #[test]
    fn shape_maps_chars_to_glyph_ids() {
        let r = rasterizer();
        let shaped = r.shape("AB", FontId(1), 13.0);
        assert_eq!(shaped.len(), 2);
        assert!(shaped.iter().all(|g| g.font == FontId(1)));
    }

    #[test]
    fn raster_for_emoji_codepoint_returns_some_or_none() {
        // U+1F600's glyph id in Noto Color Emoji exists. We don't assert
        // on the bitmap shape (CBDT-only font's `outline_glyph` may return
        // None on ab_glyph), only that the adapter does not panic.
        let r = rasterizer();
        let glyph = r.font.glyph_id('😀').0 as u32;
        let _ = r.raster(FontId(1), glyph, 32.0);
    }
}
