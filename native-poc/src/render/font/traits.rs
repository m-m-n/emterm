//! Glyph rasterizer trait + shared shape/bitmap types.
//!
//! This module is the single boundary between the renderer-side glyph cache
//! (`render::font::cache`) and the per-engine adapters (swash / ab_glyph).
//! The trait is intentionally small: shaping returns a sequence of glyph
//! ids per cluster, and rasterizing returns an atlas-upload-ready bitmap.
//!
//! Phase 4-H scope: the trait is consumed by `GlyphCache` (Phase 2) and
//! today's renderer wiring is still through the egui `painter.text()` path
//! (egui owns ASCII glyphs). The trait + cache exist so future per-cell
//! drawing (Phase 3+) can route CJK / color-emoji glyphs through swash
//! without touching egui's own text path.

/// Opaque identifier for a font registered in the resolver.
///
/// `FontId(0)` is reserved as a sentinel for "unresolved / not yet
/// registered"; production resolvers always issue ids starting at `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontId(pub u32);

/// Atlas region kind for a rasterized glyph.
///
/// `Alpha` glyphs live in an R8 (single-channel) texture and are colored
/// per-cell via the cell's foreground SGR color. `Rgba` glyphs are color
/// bitmaps (Noto Color Emoji CBDT, COLR v1) and are sampled as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtlasFormat {
    Alpha,
    Rgba,
}

/// Output of a single-glyph rasterize call.
///
/// `pixels.len()` is always `width * height * bytes_per_pixel(format)`:
/// 1 for Alpha, 4 for Rgba.
#[derive(Debug, Clone)]
pub struct GlyphBitmap {
    pub format: AtlasFormat,
    pub width: u32,
    pub height: u32,
    /// Bearing in pixels from the pen origin to the top-left of the bitmap.
    /// `bearing.0` is the horizontal left side bearing (positive = right);
    /// `bearing.1` is the vertical top side bearing (positive = up from
    /// baseline, matching FreeType / swash conventions).
    pub bearing: (i32, i32),
    /// Horizontal advance in pixels (not 26.6 / fractional). Subpixel
    /// fractions are folded into the cache key, not carried here.
    pub advance: f32,
    pub pixels: Vec<u8>,
}

impl GlyphBitmap {
    pub fn bytes_per_pixel(&self) -> usize {
        match self.format {
            AtlasFormat::Alpha => 1,
            AtlasFormat::Rgba => 4,
        }
    }

    /// True if the bitmap occupies zero pixels (zero-size sentinel).
    /// Useful for cluster terminators and whitespace.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.pixels.is_empty()
    }
}

/// A shaped glyph: identifies which `(font, glyph)` pair represents one
/// visible advance unit of a cluster, plus the size at which it should be
/// rasterized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    pub font: FontId,
    pub glyph_id: u32,
    pub size_px: f32,
}

/// Minimal glyph engine surface.
///
/// All methods must be safe to call from the renderer thread.  Adapters
/// keep their own caches / context state internally; the renderer never
/// touches per-engine handles directly.
pub trait GlyphRasterizer: Send + Sync {
    /// Shape a grapheme cluster against a single font, returning the list
    /// of glyphs the cluster decomposes into.
    ///
    /// Implementations that cannot shape (e.g. the ab_glyph adapter)
    /// return a single-glyph result derived from the cluster's first
    /// codepoint, or an empty `Vec` if the codepoint has no glyph in the
    /// requested font.
    fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph>;

    /// Rasterize a single glyph at the requested size in pixels.
    ///
    /// Returns `None` when the requested `(font, glyph_id)` pair is not
    /// available in this engine (the cache then walks the fallback chain
    /// or stores a sentinel). Successful returns may carry zero-size
    /// bitmaps (whitespace); the cache treats those as empty regions.
    fn raster(&self, font: FontId, glyph_id: u32, size_px: f32) -> Option<GlyphBitmap>;

    /// Best-effort "does this font cover this codepoint?" probe used by
    /// the fallback chain. Implementations may cache the result. The
    /// default implementation defers to `shape` and returns `true` if any
    /// glyph came back with a non-zero `glyph_id`.
    fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
        let cluster: String = char::from_u32(cp)
            .map(|c| c.to_string())
            .unwrap_or_default();
        if cluster.is_empty() {
            return false;
        }
        self.shape(&cluster, font, 16.0)
            .iter()
            .any(|g| g.glyph_id != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_id_sentinel_default_is_zero() {
        assert_eq!(FontId::default(), FontId(0));
    }

    #[test]
    fn glyph_bitmap_bytes_per_pixel_alpha_is_1() {
        let b = GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: 4,
            height: 4,
            bearing: (0, 0),
            advance: 4.0,
            pixels: vec![0; 16],
        };
        assert_eq!(b.bytes_per_pixel(), 1);
    }

    #[test]
    fn glyph_bitmap_bytes_per_pixel_rgba_is_4() {
        let b = GlyphBitmap {
            format: AtlasFormat::Rgba,
            width: 4,
            height: 4,
            bearing: (0, 0),
            advance: 4.0,
            pixels: vec![0; 64],
        };
        assert_eq!(b.bytes_per_pixel(), 4);
    }

    #[test]
    fn glyph_bitmap_is_empty_for_zero_dim() {
        let b = GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: 0,
            height: 4,
            bearing: (0, 0),
            advance: 0.0,
            pixels: vec![],
        };
        assert!(b.is_empty());
    }
}
