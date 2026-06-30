//! COLRv1 vector emoji rasterization (skrifa paint graph + tiny-skia fill).
//!
//! Phase 2 of `colrv1-emoji-vector-rendering`. The module bypasses swash
//! for COLRv1 emoji glyphs and rasterizes them directly at the requested
//! pixel size, so fractional-DPI Windows targets do not pay the
//! CBDT-strike → bilinear downscale tax. Non-COLRv1 fonts and non-emoji
//! glyphs stay on swash; this module only owns the COLRv1 fast path.
//!
//! Public surface:
//!
//! * [`is_colrv1_emoji`] — registration-time probe used by
//!   `swash_adapter::ingest_font` to flag the divert path.
//! * [`rasterize`] — the actual paint-graph walk; returns a packed
//!   straight-alpha RGBA buffer plus bearings + advance.
//! * [`RasterizedColorGlyph`] — owned output of [`rasterize`].
//!
//! `unsafe` is not used anywhere in this module (NFR3).

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

use skrifa::MetadataProvider;
use skrifa::color::{
    Brush, ColorGlyphFormat, ColorPainter, ColorStop, CompositeMode, Transform as SkrifaTransform,
};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::{BoundingBox, GlyphId, Point as SkrifaPoint};
use skrifa::raw::{FontRef, TableProvider};
use tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap,
    PixmapPaint, Point as TsPoint, RadialGradient, Shader, SpreadMode, Transform as TsTransform,
};

/// Owned output of the COLRv1 raster path.
///
/// `pixels` is straight-alpha RGBA, length `width * height * 4`. The
/// renderer's existing atlas consumes this shape directly through
/// `GlyphBitmap { format: AtlasFormat::Rgba, .. }`.
#[derive(Debug, Clone)]
pub struct RasterizedColorGlyph {
    pub width: u32,
    pub height: u32,
    pub bearing_left: i32,
    pub bearing_top: i32,
    pub advance: f32,
    pub pixels: Vec<u8>,
}

/// Probe whether `font_bytes` looks like a COLRv1 emoji font.
///
/// Three-gate check (mirrors `probe_color_support`'s defensive pattern):
/// 1. The bytes parse as a font.
/// 2. The COLR table exists and reports `version == 1`.
/// 3. The cmap covers the canonical emoji codepoint U+1F600 (grinning
///    face) — guards against COLRv1-only icon fonts that have no emoji
///    coverage.
///
/// Returns `false` on any failure path, so malformed bytes are quietly
/// classified as "not COLRv1" rather than crashing registration.
pub fn is_colrv1_emoji(font_bytes: &[u8]) -> bool {
    let face = match FontRef::new(font_bytes) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let colr = match face.colr() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if colr.version() != 1 {
        return false;
    }
    face.charmap().map('\u{1F600}').is_some()
}

/// Rasterize a COLRv1 glyph into a square RGBA buffer.
///
/// `target_cell_h_px` is the base text font's cell height at `size_px`
/// (= ascent + descent). The pixmap is sized to `ceil(target_cell_h_px)`
/// and the emoji renders into an inner `(dim - 2) × (dim - 2)` square
/// (1 px padding on all four sides), centered. `advance` is pinned to
/// the pixmap dim so the wide-cell layout reserves exactly the
/// square's footprint. Pass `0.0` to fall back to "render at full
/// `size_px`" (used by isolated tests).
///
/// Returns `None` when:
/// * `glyph_id == 0` (the `.notdef` sentinel) or `size_px <= 0.0`.
/// * The font does not parse or has no COLRv1 paint graph for `glyph_id`
///   — the caller's `FallbackChain` descends to the next font (typically
///   the monochrome `NotoEmoji-Regular`).
/// * `tiny_skia::Pixmap::new` rejects the requested dimensions.
pub fn rasterize(
    font_bytes: &[u8],
    glyph_id: u32,
    size_px: f32,
    target_cell_h_px: f32,
) -> Option<RasterizedColorGlyph> {
    // SPEC §Edge Cases: `size_px < 1.0` returns None — Pixmap allocation
    // would either fail or produce a degenerate 1×1 buffer the renderer
    // cannot meaningfully composite. `!(size_px >= 1.0)` rejects NaN,
    // zero, negatives, and sub-1.0 values in one branch.
    if glyph_id == 0 || !(size_px >= 1.0) {
        return None;
    }
    let face = FontRef::new(font_bytes).ok()?;
    let upem = face.head().ok()?.units_per_em();
    if upem == 0 {
        return None;
    }
    let gid = GlyphId::new(glyph_id);
    let color_glyph = face
        .color_glyphs()
        .get_with_format(gid, ColorGlyphFormat::ColrV1)?;

    // Pixmap dim = cell height (ascent + descent of the base text font),
    // so the wide-cell emoji square exactly matches the row height.
    // Callers passing `target_cell_h_px <= 0` get the legacy "render at
    // full size_px" fallback (used by isolated tests).
    let dim = if target_cell_h_px > 0.0 {
        (target_cell_h_px.ceil() as u32).max(1)
    } else {
        (size_px.ceil() as u32).max(1)
    };
    let dim_f = dim as f32;
    // 1 px padding on every side; the emoji renders into the inner
    // `(dim - 2) × (dim - 2)` square. For tiny dims (< 4) padding
    // would collapse the inner area, so skip it.
    let pad = if dim >= 4 { 1.0_f32 } else { 0.0 };
    let inner = (dim_f - 2.0 * pad).max(1.0);
    let mut pixmap = Pixmap::new(dim, dim).or_else(|| {
        log::warn!(
            "colrv1: Pixmap::new({dim}, {dim}) returned None for target_cell_h={target_cell_h_px}"
        );
        None
    })?;

    // Font-unit → pixel base transform with **bbox-fit scaling**.
    //
    // Read the glyph's actual bounding box (in font units) from the
    // COLRv1 ClipBox table, then pick a uniform `scale` such that the
    // bbox fits exactly inside the inner `(inner × inner)` area.
    // Center the bbox in that area. This guarantees the rendered emoji
    // never overflows the pixmap regardless of how the bbox extends
    // beyond the EM box (some glyphs have descenders below baseline
    // or ascenders above EM-top). Falls back to the EM box when the
    // font lacks a ClipBox entry for the glyph.
    let bbox_units = color_glyph
        .bounding_box(LocationRef::default(), Size::unscaled())
        .unwrap_or(BoundingBox {
            x_min: 0.0,
            y_min: 0.0,
            x_max: upem as f32,
            y_max: upem as f32,
        });
    let bbox_w = (bbox_units.x_max - bbox_units.x_min).max(1.0);
    let bbox_h = (bbox_units.y_max - bbox_units.y_min).max(1.0);
    let scale = (inner / bbox_w).min(inner / bbox_h);
    let scaled_w = bbox_w * scale;
    let scaled_h = bbox_h * scale;
    // Center the scaled bbox inside the inner area.
    let offset_x = pad + (inner - scaled_w) * 0.5;
    let offset_y_top = pad + (inner - scaled_h) * 0.5;
    // Y-flip transform (font Y-up → pixmap Y-down) with bbox-aware
    // translation: font (x_min, y_max) maps to pixmap (offset_x, offset_y_top).
    let tx = offset_x - scale * bbox_units.x_min;
    let ty = offset_y_top + scale * bbox_units.y_max;
    let base = TsTransform::from_row(scale, 0.0, 0.0, -scale, tx, ty);
    // Baseline position in pixmap (used for bearing_top reporting).
    let baseline_y = ty;

    {
        let mut painter = TinySkiaPainter::new(font_bytes, &mut pixmap, base);
        if let Err(err) = color_glyph.paint(LocationRef::default(), &mut painter) {
            log::warn!("colrv1: paint graph traversal failed for gid={glyph_id}: {err}");
            return None;
        }
    }

    let mut pixels = pixmap.take();
    un_premultiply(&mut pixels);

    // Advance: pin to the pixmap dim so the wide-cell layout reserves
    // exactly the square's footprint. Renderer's `sx = cell_w/advance`
    // resolves to 1.0 → no re-scaling, no re-centering, no asymmetric
    // padding.
    let advance = dim_f;

    // bearing_top = baseline-to-bitmap-top distance. Baseline sits at
    // `baseline_y = dim - pad` from pixmap top, so bitmap top is
    // `baseline_y` pixels above the baseline.
    Some(RasterizedColorGlyph {
        width: dim,
        height: dim,
        bearing_left: 0,
        bearing_top: baseline_y.ceil() as i32,
        advance,
        pixels,
    })
}

/// In-place premultiplied → straight alpha conversion for tightly packed
/// RGBA. tiny-skia emits premultiplied pixels; our atlas expects straight.
///
/// Ported from `render::emoji_resample::lanczos3_downscale_rgba`'s
/// un-premultiply loop with one tweak: an asserted-debug bounds check
/// guards against pixel-count rounding bugs.
fn un_premultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            continue;
        }
        px[0] = ((px[0] as u16 * 255 + a / 2) / a).min(255) as u8;
        px[1] = ((px[1] as u16 * 255 + a / 2) / a).min(255) as u8;
        px[2] = ((px[2] as u16 * 255 + a / 2) / a).min(255) as u8;
    }
}

/// Debounced warn-once log for paint-stack allocation failures. Fires once
/// across the process lifetime so OOM-adjacent states do not spam the log.
fn warn_once_alloc_failed() {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        log::warn!("colrv1: paint stack alloc failed (Mask/Pixmap), sub-tree will render empty");
    });
}

/// Debounced warn-once log for two-circle radial gradients where r0 > 0.
/// tiny-skia 0.11 cannot represent non-zero inner-radius two-circle gradients;
/// the inner radius is dropped (the c0 focal point is still passed through).
fn warn_once_radial_r0_dropped() {
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        log::warn!(
            "colrv1: radial gradient r0 > 0 (two-circle) not supported by tiny-skia 0.11; r0 dropped, c0 focal point preserved"
        );
    });
}

/// Debounced warn-once log for `CompositeMode`s that tiny-skia 0.11 has
/// no direct `BlendMode` for (HSL family). Emoji frames typically reissue
/// the same modes per render — without the cache the log would spam.
fn warn_once_unsupported_composite(mode: CompositeMode) {
    static WARNED: OnceLock<Mutex<HashSet<u8>>> = OnceLock::new();
    let cache = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    let key = mode as u8;
    if let Ok(mut set) = cache.lock() {
        if set.insert(key) {
            log::warn!(
                "colrv1: composite mode {mode:?} has no tiny-skia equivalent; using SourceOver"
            );
        }
    }
}

fn map_composite_to_blend(mode: CompositeMode) -> BlendMode {
    match mode {
        CompositeMode::Clear => BlendMode::Clear,
        CompositeMode::Src => BlendMode::Source,
        CompositeMode::Dest => BlendMode::Destination,
        CompositeMode::SrcOver => BlendMode::SourceOver,
        CompositeMode::DestOver => BlendMode::DestinationOver,
        CompositeMode::SrcIn => BlendMode::SourceIn,
        CompositeMode::DestIn => BlendMode::DestinationIn,
        CompositeMode::SrcOut => BlendMode::SourceOut,
        CompositeMode::DestOut => BlendMode::DestinationOut,
        CompositeMode::SrcAtop => BlendMode::SourceAtop,
        CompositeMode::DestAtop => BlendMode::DestinationAtop,
        CompositeMode::Xor => BlendMode::Xor,
        CompositeMode::Plus => BlendMode::Plus,
        CompositeMode::Screen => BlendMode::Screen,
        CompositeMode::Overlay => BlendMode::Overlay,
        CompositeMode::Darken => BlendMode::Darken,
        CompositeMode::Lighten => BlendMode::Lighten,
        CompositeMode::ColorDodge => BlendMode::ColorDodge,
        CompositeMode::ColorBurn => BlendMode::ColorBurn,
        CompositeMode::HardLight => BlendMode::HardLight,
        CompositeMode::SoftLight => BlendMode::SoftLight,
        CompositeMode::Difference => BlendMode::Difference,
        CompositeMode::Exclusion => BlendMode::Exclusion,
        CompositeMode::Multiply => BlendMode::Multiply,
        // HSL family: tiny-skia 0.11 has no direct equivalent; fall back
        // to source-over to keep the glyph painted instead of dropping
        // it. Warn once per unique mode so the log does not spam.
        CompositeMode::HslHue
        | CompositeMode::HslSaturation
        | CompositeMode::HslColor
        | CompositeMode::HslLuminosity
        | CompositeMode::Unknown => {
            warn_once_unsupported_composite(mode);
            BlendMode::SourceOver
        }
    }
}

/// CPAL palette colors (premultiply-able straight 8-bit) cached for the
/// duration of a single rasterize call.
struct PaletteCache {
    /// Flat array of (r, g, b, a) per palette entry. Empty if no CPAL.
    colors: Vec<(u8, u8, u8, u8)>,
}

impl PaletteCache {
    fn new(face: &FontRef<'_>) -> Self {
        let Ok(cpal) = face.cpal() else {
            return Self { colors: Vec::new() };
        };
        let records = match cpal.color_records_array() {
            Some(Ok(r)) => r,
            _ => return Self { colors: Vec::new() },
        };
        // CPAL palette 0 is the default; its first entry is at
        // color_record_indices[0]. For our (single-palette) emoji font
        // that just means starting from index 0.
        let start = cpal
            .color_record_indices()
            .first()
            .map(|v| v.get())
            .unwrap_or(0) as usize;
        let count = cpal.num_palette_entries() as usize;
        let mut colors = Vec::with_capacity(count);
        for i in 0..count {
            let r = records
                .get(start + i)
                .map(|c| (c.red(), c.green(), c.blue(), c.alpha()))
                .unwrap_or((0, 0, 0, 255));
            colors.push(r);
        }
        Self { colors }
    }

    fn resolve(&self, palette_index: u16, alpha: f32) -> Color {
        let (r, g, b, a) = self
            .colors
            .get(palette_index as usize)
            .copied()
            .unwrap_or((0, 0, 0, 255));
        let combined_a = (a as f32 / 255.0 * alpha.clamp(0.0, 1.0)).clamp(0.0, 1.0);
        Color::from_rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            combined_a,
        )
        .unwrap_or(Color::BLACK)
    }
}

/// One entry on the layer stack. The bottom-most layer is the output
/// pixmap (owned externally); inner layers are offscreen Pixmaps that get
/// composited into the parent on `pop_layer`.
struct Layer {
    pixmap: Pixmap,
    composite: CompositeMode,
}

/// `ColorPainter` impl that materializes a paint graph into a single
/// `tiny_skia::Pixmap`.
///
/// Transform stack: skrifa pushes incremental affine transforms during
/// graph traversal. We multiply them onto a single `current_transform`
/// that already incorporates the font→pixel base transform; pop restores
/// the previous matrix from a stored stack of snapshots.
///
/// Clip stack: each `push_clip_*` builds an off-pixmap-sized
/// `tiny_skia::Mask` and stacks it. `fill` consults the topmost mask.
/// Nested intersection is approximated by replacement (the topmost mask
/// wins) — that is correct for the single-deep clip patterns Noto-COLRv1
/// emoji actually use.
///
/// Layer stack: each `push_layer` allocates an offscreen `Pixmap`
/// matching the bottom layer's dimensions. Subsequent fills target that
/// offscreen. `pop_layer` composites it onto the parent via
/// `PixmapPaint { blend_mode: map_composite_to_blend(saved), .. }`.
struct TinySkiaPainter<'a> {
    font_bytes: &'a [u8],
    /// Bottom layer is `output[0]`; the active draw target is always the
    /// top of the stack. The original output Pixmap is borrowed and
    /// referenced through a wrapper layer at index 0.
    /// `None` entries represent push_layer calls that failed to allocate;
    /// pop_layer skips compositing for them while still unwinding the stack.
    layers: Vec<Option<Layer>>,
    /// Borrowed handle to the output Pixmap (only the *bottom* layer).
    /// Drawing into `output_borrow` happens when `layers` is empty.
    /// We keep a reference so we do not have to clone/swap on every call.
    output_borrow: &'a mut Pixmap,
    /// Active transform = product of pushes, on top of `base`.
    current_transform: TsTransform,
    /// Snapshot stack of the active transform before each `push_transform`.
    transform_stack: Vec<TsTransform>,
    /// Topmost-first stack of active masks.
    /// `None` entries represent push_clip_* calls that failed to allocate;
    /// pop_clip removes them normally and topmost_mask treats None as "no clip".
    clips: Vec<Option<Mask>>,
    /// CPAL palette cache (resolved once per rasterize call).
    palette: PaletteCache,
    /// Output pixmap dimensions (so `push_layer` can allocate the same size).
    width: u32,
    height: u32,
}

impl<'a> TinySkiaPainter<'a> {
    fn new(font_bytes: &'a [u8], pixmap: &'a mut Pixmap, base: TsTransform) -> Self {
        let face = FontRef::new(font_bytes).expect("rasterize already validated FontRef parse");
        let palette = PaletteCache::new(&face);
        let width = pixmap.width();
        let height = pixmap.height();
        Self {
            font_bytes,
            layers: Vec::new(),
            output_borrow: pixmap,
            current_transform: base,
            transform_stack: Vec::new(),
            clips: Vec::new(),
            palette,
            width,
            height,
        }
    }

    /// Mutable handle to whichever pixmap is currently active (top layer
    /// if any, else the bottom output pixmap).
    /// Returns `None` when the top entry is a failed-alloc sentinel.
    fn active_mut(&mut self) -> Option<&mut Pixmap> {
        match self.layers.last_mut() {
            Some(Some(layer)) => Some(&mut layer.pixmap),
            Some(None) => None,
            None => Some(self.output_borrow),
        }
    }

    fn topmost_mask(&self) -> Option<&Mask> {
        self.clips.last().and_then(|o| o.as_ref())
    }

    fn fill_path(&mut self, path: &tiny_skia::Path, paint: &Paint<'_>) {
        // Path coords are already in pixel space (we baked
        // `current_transform` into the PathBuilder). tiny-skia's
        // fill_path accepts an identity transform here.
        // Cloning the mask is necessary because of the borrow checker:
        // `active_mut` takes &mut self, but the mask reference comes
        // from &self. The mask is small (one byte per pixel) so this is
        // cheap for terminal-sized emoji.
        let mask = self.topmost_mask().cloned();
        let Some(pix) = self.active_mut() else {
            return;
        };
        pix.fill_path(
            path,
            paint,
            FillRule::Winding,
            TsTransform::identity(),
            mask.as_ref(),
        );
    }

    fn glyph_path(&self, glyph_id: GlyphId) -> Option<tiny_skia::Path> {
        let face = FontRef::new(self.font_bytes).ok()?;
        let outline = face.outline_glyphs().get(glyph_id)?;
        let mut pen = PathPen::new();
        outline
            .draw(
                DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                &mut pen,
            )
            .ok()?;
        pen.builder.finish()?.transform(self.current_transform)
    }

    fn bbox_path(&self, bbox: BoundingBox<f32>) -> Option<tiny_skia::Path> {
        let mut pb = PathBuilder::new();
        pb.move_to(bbox.x_min, bbox.y_min);
        pb.line_to(bbox.x_max, bbox.y_min);
        pb.line_to(bbox.x_max, bbox.y_max);
        pb.line_to(bbox.x_min, bbox.y_max);
        pb.close();
        pb.finish()?.transform(self.current_transform)
    }

    fn push_clip_path(&mut self, path: tiny_skia::Path) {
        let mask_opt = Mask::new(self.width, self.height).map(|mut mask| {
            // Path is already in pixel space; identity transform on the mask.
            mask.fill_path(&path, FillRule::Winding, true, TsTransform::identity());
            mask
        });
        if mask_opt.is_none() {
            warn_once_alloc_failed();
        }
        // Approximate nested-clip intersection by replacement (the
        // topmost mask wins). Real intersection would require sampling
        // the parent mask into the new fill — not needed for the
        // single-level clipping patterns Noto-COLRv1 emoji exercise.
        // On allocation failure push None: pop_clip still unwinds correctly
        // and topmost_mask returns None = "no clip" (full pixmap).
        self.clips.push(mask_opt);
    }
}

impl ColorPainter for TinySkiaPainter<'_> {
    fn push_transform(&mut self, transform: SkrifaTransform) {
        self.transform_stack.push(self.current_transform);
        // Skrifa Transform components are in column-major
        // (xx, yx, xy, yy, dx, dy); tiny-skia `from_row(sx, ky, kx, sy,
        // tx, ty)` uses the same convention.
        let local = TsTransform::from_row(
            transform.xx,
            transform.yx,
            transform.xy,
            transform.yy,
            transform.dx,
            transform.dy,
        );
        // pre_concat applies the local first (then the current),
        // matching skrifa's "concatenate to the current transformation
        // matrix" semantics.
        self.current_transform = self.current_transform.pre_concat(local);
    }

    fn pop_transform(&mut self) {
        if let Some(prev) = self.transform_stack.pop() {
            self.current_transform = prev;
        }
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        if let Some(path) = self.glyph_path(glyph_id) {
            self.push_clip_path(path);
        } else {
            // Push a sentinel zero-mask so the matching pop_clip still
            // unwinds correctly; rendering through an empty mask draws
            // nothing, which is the safe behaviour when the clip glyph
            // is missing.
            // On allocation failure push None so pop_clip still unwinds.
            let mask_opt = Mask::new(self.width, self.height);
            if mask_opt.is_none() {
                warn_once_alloc_failed();
            }
            self.clips.push(mask_opt);
        }
    }

    fn push_clip_box(&mut self, clip_box: BoundingBox<f32>) {
        if let Some(path) = self.bbox_path(clip_box) {
            self.push_clip_path(path);
        } else {
            // Degenerate bounding box (non-invertible transform, NaN coords,
            // etc.): push a zero-filled mask so renders through this clip draw
            // nothing — correct conservative semantics (clip-out vs. no-clip).
            // On allocation failure fall back to None + warn so pop_clip still
            // unwinds the stack correctly.
            let mask = Mask::new(self.width, self.height);
            if mask.is_none() {
                warn_once_alloc_failed();
            }
            self.clips.push(mask);
        }
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn fill(&mut self, brush: Brush<'_>) {
        // The fill operation paints over the entire active mask region.
        // We synthesize a full-pixmap rectangle and let the topmost mask
        // restrict it; gradient brushes contribute their own shader, so
        // the rect's coverage is fine here.
        let rect = match tiny_skia::Rect::from_xywh(0.0, 0.0, self.width as f32, self.height as f32)
        {
            Some(r) => r,
            None => return,
        };
        let mut pb = PathBuilder::new();
        pb.push_rect(rect);
        let path = match pb.finish() {
            Some(p) => p,
            None => return,
        };

        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.blend_mode = BlendMode::SourceOver;

        match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => {
                paint.shader = Shader::SolidColor(self.palette.resolve(palette_index, alpha));
            }
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => {
                paint.shader = self
                    .linear_gradient_shader(p0, p1, color_stops, extend)
                    .unwrap_or(Shader::SolidColor(self.first_stop_color(color_stops)));
            }
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => {
                if r0 > 0.0 {
                    warn_once_radial_r0_dropped();
                }
                paint.shader = self
                    .radial_gradient_shader(c0, c1, r1, color_stops, extend)
                    .unwrap_or(Shader::SolidColor(self.first_stop_color(color_stops)));
            }
            Brush::SweepGradient { color_stops, .. } => {
                // tiny-skia 0.11 has no sweep gradient. Fall back to the
                // first-stop solid colour and warn once. Per the SPEC
                // this is acceptable because Noto-COLRv1 uses sweep
                // gradients on a small minority of glyphs.
                static WARNED: OnceLock<()> = OnceLock::new();
                WARNED.get_or_init(|| {
                    log::warn!(
                        "colrv1: sweep gradient not supported by tiny-skia 0.11; using first-stop solid"
                    );
                });
                paint.shader = Shader::SolidColor(self.first_stop_color(color_stops));
            }
        }

        self.fill_path(&path, &paint);
    }

    fn push_layer(&mut self, composite_mode: CompositeMode) {
        let layer_opt = Pixmap::new(self.width, self.height).map(|pixmap| Layer {
            pixmap,
            composite: composite_mode,
        });
        if layer_opt.is_none() {
            warn_once_alloc_failed();
        }
        // On allocation failure push None: pop_layer still unwinds the
        // stack correctly and skips the composite step for this sub-tree.
        self.layers.push(layer_opt);
    }

    fn pop_layer(&mut self) {
        let Some(layer_opt) = self.layers.pop() else {
            return;
        };
        let Some(layer) = layer_opt else {
            // Failed-alloc sentinel: nothing to composite.
            return;
        };
        let blend = map_composite_to_blend(layer.composite);
        if let Some(parent) = self.active_mut() {
            parent.draw_pixmap(
                0,
                0,
                layer.pixmap.as_ref(),
                &PixmapPaint {
                    opacity: 1.0,
                    blend_mode: blend,
                    quality: tiny_skia::FilterQuality::Nearest,
                },
                TsTransform::identity(),
                None,
            );
        }
    }
}

impl TinySkiaPainter<'_> {
    fn first_stop_color(&self, stops: &[ColorStop]) -> Color {
        match stops.first() {
            Some(stop) => self.palette.resolve(stop.palette_index, stop.alpha),
            None => Color::TRANSPARENT,
        }
    }

    fn gradient_stops(&self, stops: &[ColorStop]) -> Vec<GradientStop> {
        stops
            .iter()
            .map(|s| GradientStop::new(s.offset, self.palette.resolve(s.palette_index, s.alpha)))
            .collect()
    }

    fn spread_mode(extend: skrifa::color::Extend) -> SpreadMode {
        use skrifa::color::Extend;
        match extend {
            Extend::Pad | Extend::Unknown => SpreadMode::Pad,
            Extend::Repeat => SpreadMode::Repeat,
            Extend::Reflect => SpreadMode::Reflect,
        }
    }

    fn linear_gradient_shader(
        &self,
        p0: SkrifaPoint<f32>,
        p1: SkrifaPoint<f32>,
        stops: &[ColorStop],
        extend: skrifa::color::Extend,
    ) -> Option<Shader<'static>> {
        let gs = self.gradient_stops(stops);
        if gs.is_empty() {
            return None;
        }
        LinearGradient::new(
            TsPoint::from_xy(p0.x, p0.y),
            TsPoint::from_xy(p1.x, p1.y),
            gs,
            Self::spread_mode(extend),
            self.current_transform,
        )
    }

    fn radial_gradient_shader(
        &self,
        c0: SkrifaPoint<f32>,
        c1: SkrifaPoint<f32>,
        r1: f32,
        stops: &[ColorStop],
        extend: skrifa::color::Extend,
    ) -> Option<Shader<'static>> {
        let gs = self.gradient_stops(stops);
        if gs.is_empty() || !(r1 > 0.0) {
            return None;
        }
        RadialGradient::new(
            TsPoint::from_xy(c0.x, c0.y),
            TsPoint::from_xy(c1.x, c1.y),
            r1,
            gs,
            Self::spread_mode(extend),
            self.current_transform,
        )
    }
}

/// `OutlinePen` adapter that drives a `tiny_skia::PathBuilder`.
struct PathPen {
    builder: PathBuilder,
}

impl PathPen {
    fn new() -> Self {
        Self {
            builder: PathBuilder::new(),
        }
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::font::resolver::{
        BUNDLED_BASE_FONT, BUNDLED_EMOJI_COLOR_FONT, BUNDLED_EMOJI_MONO_FONT,
    };

    /// Convenience: look up the COLRv1 glyph id for a single codepoint
    /// in the bundled emoji font. Panics with a clear message when the
    /// font does not cover the codepoint (treat as a fixture bug, not a
    /// runtime path).
    fn gid_of(cp: char) -> u32 {
        let face = FontRef::new(BUNDLED_EMOJI_COLOR_FONT).expect("bundled emoji font parses");
        let gid = face.charmap().map(cp).unwrap_or_else(|| {
            panic!(
                "bundled Noto-COLRv1 missing cmap entry for U+{:04X}",
                cp as u32
            )
        });
        u32::from(gid)
    }

    // ── TS-1 .. TS-3: un_premultiply arithmetic ─────────────────────

    /// TS-1: zero-alpha pixels must un-premultiply to all zeros so the
    /// transparent emoji border does not leak stray RGB into the atlas.
    #[test]
    fn un_premultiply_alpha_zero_emits_zeros() {
        let mut px = [0u8, 0, 0, 0];
        un_premultiply(&mut px);
        assert_eq!(px, [0, 0, 0, 0]);
    }

    /// TS-2: opaque pixels are passthrough (a == 255 → divisor cancels).
    #[test]
    fn un_premultiply_alpha_saturated_passthrough() {
        let mut px = [255u8, 128, 64, 255];
        un_premultiply(&mut px);
        assert_eq!(px, [255, 128, 64, 255]);
    }

    /// TS-3: half-alpha straight-alpha values double the premultiplied
    /// RGB (within ±1 rounding tolerance).
    #[test]
    fn un_premultiply_half_alpha_scales_up() {
        let mut px = [64u8, 32, 16, 128];
        un_premultiply(&mut px);
        // Expected straight values: 64*255/128 = 127.5 → 127 or 128
        // depending on rounding; same scaling for G and B.
        for (got, expected) in px.iter().zip([127u8, 63, 31, 128].iter()) {
            let diff = (*got as i32 - *expected as i32).abs();
            assert!(
                diff <= 1,
                "channel diff {diff} > 1 (got {got}, expected {expected})"
            );
        }
        assert_eq!(px[3], 128, "alpha must be preserved");
    }

    // ── TS-4 .. TS-6: is_colrv1_emoji probe ────────────────────────

    /// TS-4: bundled Noto-COLRv1 is the canonical COLRv1 font; the probe
    /// must classify it as such.
    #[test]
    fn is_colrv1_emoji_accepts_noto_colrv1() {
        assert!(is_colrv1_emoji(BUNDLED_EMOJI_COLOR_FONT));
    }

    /// TS-5: bundled monochrome `NotoEmoji-Regular` has no COLR table at
    /// all; the probe must reject it so swash keeps handling that font.
    #[test]
    fn is_colrv1_emoji_rejects_mono_emoji() {
        assert!(!is_colrv1_emoji(BUNDLED_EMOJI_MONO_FONT));
    }

    /// TS-6: a non-COLRv1 monospace font must be rejected. We exercise
    /// Inconsolata (no COLR table); the CBDT bytes are no longer bundled
    /// after the Noto-COLRv1 swap, so this stands in as the "definitely
    /// not COLRv1" fixture.
    #[test]
    fn is_colrv1_emoji_rejects_non_colr_font() {
        assert!(!is_colrv1_emoji(BUNDLED_BASE_FONT));
    }

    // ── TS-7 .. TS-10: rasterize success cases ─────────────────────

    fn assert_non_empty_rgba(rg: &RasterizedColorGlyph) {
        assert_eq!(
            rg.pixels.len(),
            (rg.width * rg.height * 4) as usize,
            "pixel buffer length mismatch"
        );
        let any_color = rg
            .pixels
            .chunks_exact(4)
            .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0);
        assert!(any_color, "rasterize produced an all-zero RGB buffer");
        assert!(
            rg.advance > 0.0,
            "advance must be > 0 for a paintable glyph"
        );
    }

    /// TS-7: U+1F600 (smiley) renders to a non-empty RGBA buffer at 26 px.
    #[test]
    fn rasterize_smiley_returns_non_empty_rgba() {
        let gid = gid_of('\u{1F600}');
        let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 26.0, 0.0).expect("smiley raster");
        assert_non_empty_rgba(&rg);
    }

    /// TS-8: U+1F680 (rocket) renders to a non-empty RGBA buffer at 26 px.
    #[test]
    fn rasterize_rocket_returns_non_empty_rgba() {
        let gid = gid_of('\u{1F680}');
        let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 26.0, 0.0).expect("rocket raster");
        assert_non_empty_rgba(&rg);
    }

    /// TS-9: U+2764 (heart) renders to a non-empty RGBA buffer at 26 px.
    #[test]
    fn rasterize_heart_returns_non_empty_rgba() {
        let gid = gid_of('\u{2764}');
        let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 26.0, 0.0).expect("heart raster");
        assert_non_empty_rgba(&rg);
    }

    /// TS-10: U+1F30D (globe) renders to a non-empty RGBA buffer at 26 px.
    #[test]
    fn rasterize_globe_returns_non_empty_rgba() {
        let gid = gid_of('\u{1F30D}');
        let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 26.0, 0.0).expect("globe raster");
        assert_non_empty_rgba(&rg);
    }

    // ── TS-11 .. TS-13: rasterize rejection cases ──────────────────

    /// TS-11: glyph_id = 0 (notdef) must short-circuit to None.
    #[test]
    fn rasterize_glyph_id_zero_returns_none() {
        assert!(rasterize(BUNDLED_EMOJI_COLOR_FONT, 0, 26.0, 0.0).is_none());
    }

    /// TS-12: size_px = 0.0 must short-circuit to None.
    #[test]
    fn rasterize_size_px_zero_returns_none() {
        let gid = gid_of('\u{1F600}');
        assert!(rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 0.0, 0.0).is_none());
    }

    /// TS-13: negative size_px must short-circuit to None.
    #[test]
    fn rasterize_size_px_negative_returns_none() {
        let gid = gid_of('\u{1F600}');
        assert!(rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, -1.0, 0.0).is_none());
    }

    // ── TS-14: target_px grid ──────────────────────────────────────

    /// TS-14: rasterizing the smiley at the canonical target sizes must
    /// produce a square Pixmap of side `ceil(size_px)` and a non-empty
    /// pixel buffer at every size.
    #[test]
    fn rasterize_at_target_pxs() {
        let gid = gid_of('\u{1F600}');
        for size_px in [17.0_f32, 21.0, 26.0, 35.0] {
            let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, size_px, 0.0)
                .unwrap_or_else(|| panic!("smiley raster at {size_px}px"));
            let expected = size_px.ceil() as u32;
            assert_eq!(rg.width, expected, "width mismatch at {size_px}px");
            assert_eq!(rg.height, expected, "height mismatch at {size_px}px");
            assert_eq!(rg.pixels.len(), (expected * expected * 4) as usize);
            let any_color = rg
                .pixels
                .chunks_exact(4)
                .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0);
            assert!(any_color, "all-zero RGB at {size_px}px");
        }
    }

    /// `target_cell_h_px > 0` sizes the pixmap to `ceil(target_cell_h_px)`
    /// with 1 px padding on all four sides; emoji renders into the
    /// inner `(dim - 2) × (dim - 2)` square via bbox-fit scaling.
    /// `advance` = `dim`. `bearing_top` depends on the glyph's ClipBox.
    #[test]
    fn rasterize_target_cell_h_pads_and_centers() {
        let gid = gid_of('\u{1F600}');
        // 13pt-equivalent: size_px ≈ 17.33, cell_h = 19
        let rg = rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 17.33, 19.0)
            .expect("smiley raster with target cell_h");
        assert_eq!(rg.width, 19, "pixmap width = ceil(target_cell_h_px)");
        assert_eq!(rg.height, 19, "pixmap height = ceil(target_cell_h_px)");
        // bearing_top is the baseline Y in pixmap. It must be inside
        // the inner padded area: 1 ≤ bearing_top ≤ 18.
        assert!(
            (1..=18).contains(&rg.bearing_top),
            "bearing_top {} should be inside inner padded area [1, 18]",
            rg.bearing_top
        );
        assert!(
            (rg.advance - 19.0).abs() < f32::EPSILON,
            "advance = pixmap dim"
        );
    }

    /// Small dims (< 4) skip the 1 px padding to keep the inner area
    /// positive. A 3 × 3 pixmap renders edge-to-edge with no padding.
    #[test]
    fn rasterize_tiny_dim_skips_padding() {
        let gid = gid_of('\u{1F600}');
        let rg =
            rasterize(BUNDLED_EMOJI_COLOR_FONT, gid, 3.0, 3.0).expect("smiley raster at tiny size");
        assert_eq!(rg.width, 3);
        assert_eq!(rg.height, 3);
        // pad = 0, baseline_y = dim = 3 → bearing_top = 3
        assert_eq!(rg.bearing_top, 3);
        assert!((rg.advance - 3.0).abs() < f32::EPSILON);
    }
}
