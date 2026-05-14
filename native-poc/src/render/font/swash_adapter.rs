//! swash-backed `GlyphRasterizer` implementation.
//!
//! Phase 3 of font-swash-migration (FR4, FR9). One adapter instance wraps
//! the entire resolver: it owns the font byte buffers behind `Arc<[u8]>`
//! (so they outlive the adapter) and parses each registered font lazily
//! on first use. Rasterizing routes between Alpha (monochrome) and Rgba
//! (color bitmap / COLR) by inspecting the `swash` `Content` tag returned
//! from `Render::render`.
//!
//! Concurrency: per-font swash `ScaleContext`s are kept in a `Mutex` so
//! the adapter implements `Send + Sync` cheaply. The mutex is only held
//! during shaping / rasterizing, never across the cache lookup.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::shape::ShapeContext;
use swash::zeno::{Format, Vector};
use swash::FontRef;

use super::resolver::{RegisteredFont, Resolver};
use super::traits::{AtlasFormat, FontId, GlyphBitmap, GlyphRasterizer, ShapedGlyph};

/// A registered font's byte storage + cached offset/key needed by swash.
#[derive(Clone)]
struct SwashFont {
    bytes: Arc<[u8]>,
    offset: u32,
    /// True when the font has at least one color-table (CBDT, CBLC, COLR,
    /// or SVG). Used to pick the rasterize source list.
    has_color: bool,
}

impl SwashFont {
    fn font_ref(&self) -> FontRef<'_> {
        // `FontRef::from_index` with index 0 is correct for OTF/TTF;
        // collections (.ttc) would need a different index but the
        // bundled emoji + CJK fonts are single-face files.
        FontRef::from_index(&self.bytes, 0).expect("registered font must parse")
    }
}

#[derive(Default)]
struct Inner {
    fonts: HashMap<FontId, SwashFont>,
    shape_ctx: ShapeContext,
    scale_ctx: ScaleContext,
}

/// Swash adapter shared across the renderer.
pub struct SwashRasterizer {
    inner: Mutex<Inner>,
}

impl SwashRasterizer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Register every parseable font held by `resolver` into this adapter.
    /// Fonts whose byte buffer is empty (system-scan placeholders) are
    /// skipped; only bundled fonts contribute to the swash path today.
    pub fn ingest_resolver(&self, resolver: &Resolver) {
        let mut inner = self.inner.lock();
        for id in resolver
            .by_role(super::resolver::FontRole::Cjk)
            .chain(resolver.by_role(super::resolver::FontRole::Emoji))
            .chain(resolver.by_role(super::resolver::FontRole::Base))
            .chain(resolver.by_role(super::resolver::FontRole::Secondary))
            .chain(resolver.by_role(super::resolver::FontRole::User))
            .map(|f| f.id)
        {
            if let Some(font) = resolver.font(id) {
                Self::ingest_font(&mut inner, font);
            }
        }
    }

    fn ingest_font(inner: &mut Inner, font: &RegisteredFont) {
        if font.bytes.is_empty() {
            return;
        }
        let bytes = font.bytes.clone();
        // Probe color tables. swash exposes a uniform `attributes` /
        // bitmap-strikes API: instead of poking at raw OT tables (which
        // requires the internal `RawFont` trait), we ask the scaler-side
        // probe whether color sources are available. Concretely, we look
        // for any bitmap strike or color palette by attempting to read
        // the COLR / CBDT / SVG sub-systems via swash's high-level api.
        //
        // Heuristic: scan a small set of "marker" codepoints (smiley face
        // for emoji fonts) and look at the swash `Render` Content tag.
        // For our two bundled fonts this is deterministic (Noto Color
        // Emoji has color; Noto Sans CJK JP does not). The probe is
        // narrow enough that it does not slow startup measurably.
        let has_color = probe_color_support(&bytes);
        let entry = SwashFont {
            bytes,
            offset: 0,
            has_color,
        };
        inner.fonts.insert(font.id, entry);
    }

    /// Register a single in-memory font directly (test helper).
    pub fn register_bytes(&self, id: FontId, bytes: Arc<[u8]>) {
        let mut inner = self.inner.lock();
        Self::ingest_font(
            &mut inner,
            &RegisteredFont {
                id,
                role: super::resolver::FontRole::User,
                family: String::new(),
                bytes,
            },
        );
    }

    pub fn known_font_ids(&self) -> Vec<FontId> {
        self.inner.lock().fonts.keys().copied().collect()
    }
}

impl Default for SwashRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphRasterizer for SwashRasterizer {
    fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
        let mut inner = self.inner.lock();
        let Some(swash_font) = inner.fonts.get(&font).cloned() else {
            return Vec::new();
        };
        let mut shaper = inner
            .shape_ctx
            .builder(swash_font.font_ref())
            .size(size_px)
            .build();
        shaper.add_str(cluster);
        let mut out = Vec::new();
        shaper.shape_with(|cluster_glyphs| {
            for g in cluster_glyphs.glyphs {
                out.push(ShapedGlyph {
                    font,
                    glyph_id: g.id as u32,
                    size_px,
                });
            }
        });
        out
    }

    fn raster(&self, font: FontId, glyph_id: u32, size_px: f32) -> Option<GlyphBitmap> {
        let mut inner = self.inner.lock();
        let swash_font = inner.fonts.get(&font).cloned()?;
        if glyph_id == 0 {
            return None;
        }
        let face = swash_font.font_ref();
        let mut scaler = inner
            .scale_ctx
            .builder(face)
            .size(size_px)
            .hint(false)
            .build();
        // Source order: when the font has color tables, try the color
        // sources first; otherwise go straight to the alpha outline. This
        // keeps the alpha-only ASCII path on the R8 atlas.
        let sources: &[Source] = if swash_font.has_color {
            &[
                Source::ColorBitmap(StrikeWith::BestFit),
                Source::ColorOutline(0),
                Source::Outline,
            ]
        } else {
            &[Source::Outline]
        };
        let image = Render::new(sources)
            .format(Format::Alpha)
            .offset(Vector::ZERO)
            .render(&mut scaler, glyph_id as u16)?;
        let w = image.placement.width;
        let h = image.placement.height;
        let format = match image.content {
            Content::Color => AtlasFormat::Rgba,
            _ => AtlasFormat::Alpha,
        };
        // For Alpha output swash may still emit a Mask buffer (1 BPP);
        // either way, the `data` length matches `w*h*bpp`.
        let pixels = image.data;
        // Compute horizontal advance via the shaper-free path: swash's
        // `font_ref` exposes glyph metrics directly.
        let advance = {
            let scale = size_px / face.metrics(&[]).units_per_em as f32;
            let raw = face.glyph_metrics(&[]).advance_width(glyph_id as u16);
            raw * scale
        };
        Some(GlyphBitmap {
            format,
            width: w,
            height: h,
            bearing: (image.placement.left, image.placement.top),
            advance,
            pixels,
        })
    }

    fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
        let inner = self.inner.lock();
        let Some(swash_font) = inner.fonts.get(&font) else {
            return false;
        };
        let face = swash_font.font_ref();
        let c = match char::from_u32(cp) {
            Some(c) => c,
            None => return false,
        };
        face.charmap().map(c) != 0
    }
}

// Silence: `offset` is kept for forward-compatibility with .ttc face
// indexing but is unused on today's single-face bundled fonts.
#[allow(dead_code)]
fn _offset_compat(f: &SwashFont) -> u32 {
    f.offset
}

/// Probe whether a font supplies color glyphs.
///
/// We try a single render on a known emoji codepoint (U+1F600). If the
/// font's charmap covers it and swash returns `Content::Color`, the font
/// is color-capable; otherwise it is monochrome. For fonts that lack the
/// probe codepoint we fall back to "no color".
fn probe_color_support(bytes: &[u8]) -> bool {
    let face = match FontRef::from_index(bytes, 0) {
        Some(f) => f,
        None => return false,
    };
    let glyph_id = face.charmap().map('\u{1F600}');
    if glyph_id == 0 {
        return false;
    }
    let mut sctx = ScaleContext::new();
    let mut scaler = sctx.builder(face).size(64.0).hint(false).build();
    let img = match Render::new(&[
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::ColorOutline(0),
        Source::Outline,
    ])
    .format(Format::Alpha)
    .offset(Vector::ZERO)
    .render(&mut scaler, glyph_id)
    {
        Some(img) => img,
        None => return false,
    };
    matches!(img.content, Content::Color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rasterizer_with_emoji() -> SwashRasterizer {
        let r = SwashRasterizer::new();
        r.register_bytes(
            FontId(1),
            Arc::<[u8]>::from(super::super::resolver::BUNDLED_EMOJI_FONT),
        );
        r
    }

    fn rasterizer_with_cjk() -> SwashRasterizer {
        let r = SwashRasterizer::new();
        r.register_bytes(
            FontId(1),
            Arc::<[u8]>::from(super::super::resolver::BUNDLED_CJK_FONT),
        );
        r
    }

    /// TS-font-8: swash rasterizes ASCII 'A' to a non-empty alpha bitmap
    /// with sensible advance.
    #[test]
    fn swash_rasters_ascii_alpha() {
        let r = rasterizer_with_cjk();
        // 'A' must exist in Noto Sans CJK JP (it ships Latin glyphs).
        let cluster_glyphs = r.shape("A", FontId(1), 32.0);
        assert!(!cluster_glyphs.is_empty(), "shape returned no glyphs");
        let g = cluster_glyphs[0];
        let bitmap = r
            .raster(g.font, g.glyph_id, g.size_px)
            .expect("ASCII raster");
        assert_eq!(bitmap.format, AtlasFormat::Alpha);
        assert!(!bitmap.is_empty(), "ASCII bitmap empty: {:?}", bitmap);
        assert!(bitmap.advance > 0.0, "ASCII advance must be > 0");
    }

    /// TS-font-9: swash rasterizes U+1F600 to RGBA; at least one non-zero
    /// RGB byte exists.
    #[test]
    fn swash_rasters_emoji_rgba() {
        let r = rasterizer_with_emoji();
        let face =
            FontRef::from_index(super::super::resolver::BUNDLED_EMOJI_FONT, 0).expect("emoji font");
        let glyph_id = face.charmap().map('\u{1F600}') as u32;
        assert!(glyph_id > 0, "emoji font must cover U+1F600");
        let bitmap = r.raster(FontId(1), glyph_id, 64.0).expect("emoji raster");
        assert_eq!(bitmap.format, AtlasFormat::Rgba);
        let any_color = bitmap
            .pixels
            .chunks_exact(4)
            .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0);
        assert!(any_color, "emoji bitmap had only zero RGB bytes");
    }

    #[test]
    fn unknown_font_id_returns_none() {
        let r = SwashRasterizer::new();
        assert!(r.raster(FontId(99), 1, 13.0).is_none());
    }

    #[test]
    fn has_codepoint_for_emoji_font_covers_grin() {
        let r = rasterizer_with_emoji();
        assert!(r.has_codepoint(FontId(1), 0x1F600));
        assert!(!r.has_codepoint(FontId(1), 0xE000_0001));
    }
}
