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
use swash::{FontRef, NormalizedCoord, Setting};

use super::resolver::{RegisteredFont, Resolver};
use super::traits::{AtlasFormat, FontId, FontMetrics, GlyphBitmap, GlyphRasterizer, ShapedGlyph};

/// Faux-bold strength (px, per side) applied to outline rasterization.
/// Default 0: FreeType disables CFF stem darkening entirely under full
/// hinting (the desktop's `font-hinting=full`), so the WebView build
/// renders without it — any embolden here reads as fatter-than-WebView
/// strokes, most visibly as dark text on light backgrounds (e.g. ls's
/// other-writable dir highlight). The earlier "swash looks thin" symptom
/// that motivated darkening was the sRGB double-encode, fixed separately.
/// Set `EMTERM_STEM_DARKEN` (absolute px, e.g. `0.3`) to re-enable.
fn stem_darken_strength(_size_px: f32) -> f32 {
    use std::sync::OnceLock;
    static OVERRIDE: OnceLock<Option<f32>> = OnceLock::new();
    let ov = OVERRIDE.get_or_init(|| {
        std::env::var("EMTERM_STEM_DARKEN")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
    });
    match *ov {
        Some(v) => v.max(0.0),
        None => 0.0,
    }
}

/// `EMTERM_SUBPIXEL` env toggle: `0` disables RGB subpixel AA (grayscale
/// fallback for displays / users where LCD rendering is undesirable).
/// Anything else — including unset — enables it.
fn subpixel_enabled_from_env() -> bool {
    std::env::var("EMTERM_SUBPIXEL")
        .map(|v| v != "0")
        .unwrap_or(true)
}

/// Parse `settings.variable_font_axes` entries into swash `Setting`s.
/// Axis tags must be exactly 4 printable-ASCII characters (OpenType tag
/// grammar, e.g. `wght` / `wdth` / `slnt`); anything else is warned and
/// skipped. The result is sorted by tag so downstream behavior does not
/// depend on `HashMap` iteration order.
fn parse_axes(map: &HashMap<String, f32>) -> Vec<Setting<f32>> {
    let mut out: Vec<Setting<f32>> = Vec::new();
    for (tag, &value) in map {
        if tag.len() == 4 && tag.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
            out.push(Setting::from((tag.as_str(), value)));
        } else {
            log::warn!(
                "font.variable_axes: invalid axis tag {tag:?} (must be 4 ASCII chars); ignored"
            );
        }
    }
    out.sort_by_key(|s| s.tag);
    out
}

/// Compute the full normalized-coordinate array for `face` under `axes`.
/// Returns an empty vec when the font is not variable or none of its
/// axes appear in `axes` — empty coords are the documented "default
/// instance" for swash's metrics APIs, so non-variable fonts stay on
/// the exact code path they used before axis support landed.
fn normalized_coords(face: &FontRef, axes: &[Setting<f32>]) -> Vec<NormalizedCoord> {
    if axes.is_empty() {
        return Vec::new();
    }
    let vars = face.variations();
    if axes.iter().all(|s| vars.find_by_tag(s.tag).is_none()) {
        return Vec::new();
    }
    vars.normalized_coords(axes.iter().copied()).collect()
}

/// A registered font's byte storage + cached offset/key needed by swash.
#[derive(Clone)]
struct SwashFont {
    bytes: Arc<[u8]>,
    offset: u32,
    /// Normalized variation coordinates for the adapter's configured
    /// axes, cached at ingest. Empty for non-variable fonts (and when no
    /// configured axis exists in the font), which selects the default
    /// instance in `FontRef::metrics` / `glyph_metrics`.
    coords: Vec<NormalizedCoord>,
    /// True when the font has at least one color-table (CBDT, CBLC, COLR,
    /// or SVG). Used to pick the rasterize source list.
    has_color: bool,
    /// True when the face's OS/2 weight is >= 600 (Bold-ish). Stem
    /// darkening is skipped for these faces: FreeType's CFF darkening
    /// fades out toward heavy weights (its purpose is keeping *thin*
    /// stems from washing out), and embolden-ing an already-bold face
    /// fills counters and welds adjacent glyphs together at terminal
    /// sizes.
    is_bold: bool,
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
    /// RGB subpixel anti-aliasing (LCD rendering) for monochrome outline
    /// glyphs. Matches the WebView build, which renders through
    /// WebKitGTK → FreeType with the desktop's `font-antialiasing=rgba`
    /// setting: each of R/G/B is rasterized at a ∓1/3-px horizontal
    /// offset, tripling the effective horizontal resolution. Disabled
    /// (grayscale) with `EMTERM_SUBPIXEL=0`.
    subpixel: bool,
    /// Variable-font axis settings (`native_poc.variable_font_axes`,
    /// e.g. `wght: 450`). Applied uniformly to shaping, rasterization,
    /// and metrics of every registered font; fonts that lack a given
    /// axis ignore it (swash clamps / no-ops per the fvar table).
    axes: Vec<Setting<f32>>,
}

impl SwashRasterizer {
    pub fn new() -> Self {
        Self::with_subpixel(subpixel_enabled_from_env())
    }

    /// Explicit subpixel toggle (test helper + future settings hook).
    pub fn with_subpixel(subpixel: bool) -> Self {
        Self::with_subpixel_and_axes(subpixel, &HashMap::new())
    }

    /// Settings-driven constructor: subpixel from env, variable-font
    /// axes from `Settings::variable_font_axes`.
    pub fn with_axes(axes: &HashMap<String, f32>) -> Self {
        Self::with_subpixel_and_axes(subpixel_enabled_from_env(), axes)
    }

    /// Fully explicit constructor (test helper).
    pub fn with_subpixel_and_axes(subpixel: bool, axes: &HashMap<String, f32>) -> Self {
        let axes = parse_axes(axes);
        if !axes.is_empty() {
            let listed: Vec<String> = axes.iter().map(|s| s.to_string()).collect();
            log::info!("font.variable_axes = {}", listed.join(", "));
        }
        Self {
            inner: Mutex::new(Inner::default()),
            subpixel,
            axes,
        }
    }

    /// True when monochrome glyphs rasterize as RGB subpixel masks.
    pub fn subpixel(&self) -> bool {
        self.subpixel
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
                Self::ingest_font(&mut inner, font, &self.axes);
            }
        }
    }

    fn ingest_font(inner: &mut Inner, font: &RegisteredFont, axes: &[Setting<f32>]) {
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
        let parsed = FontRef::from_index(&bytes, 0);
        let is_bold = parsed
            .as_ref()
            .map(|f| f.attributes().weight().0 >= 600)
            .unwrap_or(false);
        let coords = parsed
            .as_ref()
            .map(|f| normalized_coords(f, axes))
            .unwrap_or_default();
        let entry = SwashFont {
            bytes,
            offset: 0,
            coords,
            has_color,
            is_bold,
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
            &self.axes,
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
            .variations(self.axes.iter().copied())
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
            // Hinting ON: snaps stems to the pixel grid the same way the
            // WebView build's FreeType path does. At terminal sizes
            // (~17 px) unhinted outlines smear across pixel boundaries
            // and read as thin / washed-out.
            .hint(true)
            .variations(self.axes.iter().copied())
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
        // Monochrome outlines rasterize as RGB subpixel masks when LCD
        // rendering is on (zeno renders each channel at a ∓0.3-px
        // horizontal offset — RGB stripe order). Color sources ignore
        // the format and still come back as `Content::Color`.
        let mask_format = if self.subpixel {
            Format::Subpixel
        } else {
            Format::Alpha
        };
        // Approximate FreeType's CFF stem darkening (the WebView
        // build renders text through WebKitGTK → FreeType, which
        // thickens small glyphs by ~0.4 px so they don't wash out
        // on dark backgrounds under gamma-space blending). Without
        // this, swash's outlines rasterize noticeably thinner and
        // lighter than the WebView build at terminal sizes. Only
        // outline sources embolden; color bitmaps are unaffected.
        // Bold faces skip darkening entirely — FreeType fades it out
        // toward heavy weights, and darkening an already-bold face
        // welds adjacent glyphs together at the terminal cell pitch.
        let darken = if swash_font.is_bold {
            0.0
        } else {
            stem_darken_strength(size_px)
        };
        let image = Render::new(sources)
            .format(mask_format)
            .offset(Vector::ZERO)
            .embolden(darken)
            .render(&mut scaler, glyph_id as u16)?;
        let w = image.placement.width;
        let h = image.placement.height;
        let format = match image.content {
            Content::Color => AtlasFormat::Rgba,
            Content::SubpixelMask => AtlasFormat::Subpixel,
            Content::Mask => AtlasFormat::Alpha,
        };
        // For Alpha output swash may still emit a Mask buffer (1 BPP);
        // either way, the `data` length matches `w*h*bpp`.
        let pixels = image.data;
        // Compute horizontal advance via the shaper-free path: swash's
        // `font_ref` exposes glyph metrics directly.
        let advance = {
            let coords = swash_font.coords.as_slice();
            let scale = size_px / face.metrics(coords).units_per_em as f32;
            let raw = face.glyph_metrics(coords).advance_width(glyph_id as u16);
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

    fn font_metrics(&self, font: FontId, size_px: f32) -> Option<FontMetrics> {
        let inner = self.inner.lock();
        let swash_font = inner.fonts.get(&font)?;
        let face = swash_font.font_ref();
        let m = face.metrics(&swash_font.coords);
        let upem = m.units_per_em as f32;
        if upem <= 0.0 || size_px <= 0.0 {
            return None;
        }
        let scale = size_px / upem;
        Some(FontMetrics {
            ascent: m.ascent * scale,
            descent: m.descent * scale,
            line_gap: m.leading * scale,
        })
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
    use swash::Tag;

    fn rasterizer_with_emoji() -> SwashRasterizer {
        // Explicit grayscale so the assertions stay deterministic
        // regardless of the EMTERM_SUBPIXEL env default.
        let r = SwashRasterizer::with_subpixel(false);
        r.register_bytes(
            FontId(1),
            Arc::<[u8]>::from(super::super::resolver::BUNDLED_EMOJI_FONT),
        );
        r
    }

    fn rasterizer_with_cjk() -> SwashRasterizer {
        let r = SwashRasterizer::with_subpixel(false);
        r.register_bytes(
            FontId(1),
            Arc::<[u8]>::from(super::super::resolver::BUNDLED_CJK_FONT),
        );
        r
    }

    fn subpixel_rasterizer_with_cjk() -> SwashRasterizer {
        let r = SwashRasterizer::with_subpixel(true);
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

    /// Subpixel mode rasterizes monochrome outlines as 4-byte-per-pixel
    /// RGB coverage masks whose left/right edges carry asymmetric R/B
    /// values (the ∓0.3-px channel offsets) — the property that makes
    /// LCD rendering visibly smoother than grayscale.
    #[test]
    fn swash_rasters_ascii_subpixel_mask() {
        let r = subpixel_rasterizer_with_cjk();
        let cluster_glyphs = r.shape("d", FontId(1), 17.0);
        assert!(!cluster_glyphs.is_empty(), "shape returned no glyphs");
        let g = cluster_glyphs[0];
        let bitmap = r
            .raster(g.font, g.glyph_id, g.size_px)
            .expect("subpixel raster");
        assert_eq!(bitmap.format, AtlasFormat::Subpixel);
        assert_eq!(
            bitmap.pixels.len(),
            (bitmap.width * bitmap.height * 4) as usize,
            "subpixel mask must be 4 bytes per pixel"
        );
        let any_asymmetric = bitmap
            .pixels
            .chunks_exact(4)
            .any(|px| px[0] != px[2] && (px[0] > 0 || px[2] > 0));
        assert!(
            any_asymmetric,
            "subpixel mask must have at least one R≠B edge pixel"
        );
    }

    #[test]
    fn unknown_font_id_returns_none() {
        let r = SwashRasterizer::with_subpixel(false);
        assert!(r.raster(FontId(99), 1, 13.0).is_none());
    }

    #[test]
    fn has_codepoint_for_emoji_font_covers_grin() {
        let r = rasterizer_with_emoji();
        assert!(r.has_codepoint(FontId(1), 0x1F600));
        assert!(!r.has_codepoint(FontId(1), 0xE000_0001));
    }

    // ── variable_font_axes ────────────────────────────────────────────

    /// Valid 4-char ASCII tags parse; anything else is skipped. The
    /// result is sorted by tag regardless of HashMap iteration order.
    #[test]
    fn parse_axes_filters_invalid_tags() {
        let mut map = HashMap::new();
        map.insert("wght".to_string(), 700.0);
        map.insert("wdth".to_string(), 90.0);
        map.insert("weight".to_string(), 1.0); // too long
        map.insert("wt".to_string(), 1.0); // too short
        map.insert("wgh\u{3042}".to_string(), 1.0); // non-ASCII (5 bytes anyway)
        let axes = parse_axes(&map);
        assert_eq!(axes.len(), 2);
        let tags: Vec<Tag> = axes.iter().map(|s| s.tag).collect();
        let mut expected = vec![
            Setting::from(("wght", 0.0)).tag,
            Setting::from(("wdth", 0.0)).tag,
        ];
        expected.sort();
        assert_eq!(tags, expected);
        assert!(axes.windows(2).all(|w| w[0].tag <= w[1].tag));
    }

    /// Locate any installed variable font via fontconfig and return its
    /// bytes. Portable across dev machines (no hardcoded per-user path);
    /// returns `None` in environments without fontconfig or without any
    /// variable font (e.g. Docker CI), so the caller skips cleanly. Only
    /// fonts the adapter actually treats as variable (non-empty
    /// normalized coords for a `wght` probe) and single-face files (the
    /// adapter parses index 0 only) are accepted.
    fn find_variable_font_via_fontconfig() -> Option<Arc<[u8]>> {
        let mut probe_map = HashMap::new();
        probe_map.insert("wght".to_string(), 900.0);
        let probe = parse_axes(&probe_map);
        let out = std::process::Command::new("fc-list")
            .args([":variable", "file"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let path = line.split(':').next().unwrap_or("").trim();
            if path.is_empty() || path.ends_with(".ttc") || path.ends_with(".TTC") {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let is_variable = FontRef::from_index(&bytes, 0)
                .map(|f| !normalized_coords(&f, &probe).is_empty())
                .unwrap_or(false);
            if is_variable {
                return Some(Arc::from(bytes));
            }
        }
        None
    }

    /// On a real variable font, a `wght` setting must change both the
    /// cached coords and the rasterized ink. Host-dependent: silently
    /// skips when no variable font is installed (e.g. Docker CI),
    /// so the deterministic suite is unaffected.
    #[test]
    fn axes_affect_variable_font_when_available() {
        let Some(bytes) = find_variable_font_via_fontconfig() else {
            eprintln!("skip: no variable font on this host");
            return;
        };
        let mut heavy_map = HashMap::new();
        heavy_map.insert("wght".to_string(), 900.0);
        let heavy = SwashRasterizer::with_subpixel_and_axes(false, &heavy_map);
        heavy.register_bytes(FontId(1), bytes.clone());
        assert!(
            !heavy.inner.lock().fonts[&FontId(1)].coords.is_empty(),
            "variable font must cache non-empty coords"
        );
        let plain = SwashRasterizer::with_subpixel(false);
        plain.register_bytes(FontId(1), bytes);
        let g = heavy.shape("A", FontId(1), 24.0)[0];
        let ink = |bm: &GlyphBitmap| bm.pixels.iter().map(|&p| p as u64).sum::<u64>();
        let bm_heavy = heavy.raster(g.font, g.glyph_id, 24.0).unwrap();
        let bm_plain = plain.raster(g.font, g.glyph_id, 24.0).unwrap();
        assert!(
            ink(&bm_heavy) > ink(&bm_plain),
            "wght 900 must put more ink on the page than the default instance ({} vs {})",
            ink(&bm_heavy),
            ink(&bm_plain)
        );
    }

    /// Non-variable fonts must stay on the default-instance path: cached
    /// coords are empty and rasterization output is byte-identical to an
    /// adapter without any axis settings (bundled CJK is a static OTF).
    #[test]
    fn axes_are_noop_on_non_variable_font() {
        let mut map = HashMap::new();
        map.insert("wght".to_string(), 700.0);
        let with_axes = SwashRasterizer::with_subpixel_and_axes(false, &map);
        with_axes.register_bytes(
            FontId(1),
            Arc::<[u8]>::from(super::super::resolver::BUNDLED_CJK_FONT),
        );
        assert!(
            with_axes.inner.lock().fonts[&FontId(1)].coords.is_empty(),
            "static font must cache empty (default-instance) coords"
        );
        let plain = rasterizer_with_cjk();
        let g_a = with_axes.shape("A", FontId(1), 17.0)[0];
        let g_b = plain.shape("A", FontId(1), 17.0)[0];
        assert_eq!(g_a.glyph_id, g_b.glyph_id);
        let bm_a = with_axes.raster(g_a.font, g_a.glyph_id, 17.0).unwrap();
        let bm_b = plain.raster(g_b.font, g_b.glyph_id, 17.0).unwrap();
        assert_eq!(
            bm_a.pixels, bm_b.pixels,
            "axes must not alter static-font rasters"
        );
        assert_eq!(bm_a.advance, bm_b.advance);
        let m_a = with_axes.font_metrics(FontId(1), 17.0).unwrap();
        let m_b = plain.font_metrics(FontId(1), 17.0).unwrap();
        assert_eq!(m_a.ascent, m_b.ascent);
        assert_eq!(m_a.descent, m_b.descent);
    }
}
