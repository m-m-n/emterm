//! Font module aggregator.
//!
//! Phase 2 of font-swash-migration lands the boundary types (`traits.rs`),
//! the glyph cache + atlas (`cache.rs`, `atlas.rs`), and the ab_glyph
//! adapter (`ab_glyph_adapter.rs`). Phase 3 layers the swash adapter,
//! fontdb-backed resolver, and the fallback chain on top.
//!
//! The renderer never imports the engine adapters directly; it always
//! talks to the cache, which owns a boxed `dyn GlyphRasterizer`. That
//! keeps the cache + atlas independent of the active engine and makes
//! the `Settings::font_engine` flag a one-line constructor swap.

pub mod ab_glyph_adapter;
pub mod atlas;
pub mod cache;
pub mod colrv1_painter;
pub mod fallback;
pub mod presentation;
pub mod resolver;
pub mod swash_adapter;
pub mod traits;
pub mod user_dir;

pub use atlas::{Atlas, AtlasRegion};
pub use cache::{CacheStats, GlyphCache, GlyphKey};
pub use fallback::FallbackChain;
pub use traits::{AtlasFormat, FontId, GlyphBitmap, GlyphRasterizer, ShapedGlyph};

// ── Overlay-glyph entry point (block-cursor-glyph-font task0001) ──────
//
// `render::cursor::draw_block_cursor` used to redraw the block cursor's
// covered glyph via `egui::Painter::text(..., FontId::monospace(..))` —
// egui's OWN built-in monospace font, entirely bypassing the swash /
// fallback-chain / atlas pipeline the wgpu `terminal_grid_pass` uses for
// every other cell. The glyph shape under the cursor could therefore
// differ from the surrounding grid (e.g. a slashed-zero font rendering
// as an unslashed zero). [`resolve_overlay_glyph`] is the shared
// resolver both paths now agree on: same `FallbackChain::resolve_for_cluster`
// font pick, same `GlyphRasterizer::shape`, same `GlyphKey`, same
// `GlyphCache::get_or_rasterize` — so an overlay lookup for a glyph the
// grid already drew this frame is a guaranteed cache hit, not a second
// rasterize (IMPLEMENTATION.md D2).

/// A glyph raster ready for the egui overlay path: RGBA8 pixels copied out
/// of the SAME atlas [`GlyphCache`] the wgpu grid pass populates, at the
/// SAME [`GlyphKey`] the grid pass would build for the same
/// `(code_point, size_px, weight)` — see IMPLEMENTATION.md's "render::font
/// overlay-glyph entry point" shared component.
///
/// `pixels` is always RGBA8 (straight, unmultiplied alpha) regardless of
/// the source [`AtlasFormat`], so the caller can hand it to
/// `egui::ColorImage::from_rgba_unmultiplied` unconditionally:
/// - `AtlasFormat::Alpha` / `AtlasFormat::Subpixel` sources become a flat
///   white coverage mask (`needs_tint = true`) — the caller multiplies by
///   the resolved cell color via egui's per-draw tint parameter, matching
///   how the grid pass modulates Alpha-page glyphs by the cell's fg color.
/// - `AtlasFormat::Rgba` sources (color emoji / COLRv1) keep their own
///   color (`needs_tint = false`) — the grid pass samples these as-is too.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayGlyph {
    /// Cache key the raster was resolved at — identical to the key
    /// [`GlyphCache::get_or_rasterize`] would receive for the same
    /// cluster/size/weight from the grid pass's cell-glyph path.
    pub key: GlyphKey,
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub bearing_left: i32,
    pub bearing_top: i32,
    pub advance: f32,
    pub needs_tint: bool,
}

/// Cheap metadata for an overlay glyph — everything a caller needs to
/// place / size the glyph quad and look up an already-cached
/// `egui::TextureHandle`, resolved WITHOUT touching pixel data (task0002
/// r1-p1 / AC-4). The old combined `resolve_overlay_glyph` always ran
/// [`extract_region_rgba`] (a per-glyph `Vec<u8>` allocation) even when the
/// caller's `egui::TextureHandle` cache — keyed by [`key`](Self::key) —
/// already had this exact glyph from a previous frame. Splitting the meta
/// lookup ([`resolve_overlay_glyph_meta`]) from the pixel copy
/// ([`extract_overlay_glyph_pixels`]) lets
/// `render::cursor::get_or_create_overlay_texture`'s `loader` closure be
/// the ONLY caller that ever reaches [`extract_region_rgba`], and it only
/// runs on that texture cache's miss path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayGlyphMeta {
    /// Cache key the raster was resolved at — identical to the key
    /// [`GlyphCache::get_or_rasterize`] would receive for the same
    /// cluster/size/weight from the grid pass's cell-glyph path.
    pub key: GlyphKey,
    pub width: u32,
    pub height: u32,
    pub bearing_left: i32,
    pub bearing_top: i32,
    pub advance: f32,
    pub needs_tint: bool,
    /// Atlas placement backing this glyph. Private: meaningless without
    /// the exact [`GlyphCache`] it was resolved against, and only
    /// [`extract_overlay_glyph_pixels`] (this module) needs it.
    region: AtlasRegion,
}

/// Resolve `cluster`'s glyph metadata (cache key + placement geometry)
/// through the shared font pipeline: [`FallbackChain::resolve_for_cluster`]
/// picks the font (optionally swapped for its bold variant),
/// `rasterizer.shape` picks the glyph id, and `cache.get_or_rasterize`
/// rasterizes on first miss / returns the cached atlas region on every
/// subsequent call — the exact sequence
/// `terminal_grid_pass::GridInstanceBuilder::glyph_instance` runs for an
/// ordinary cell. Does NOT touch pixel data — see [`OverlayGlyphMeta`].
///
/// Returns `None` when the cluster has no glyph in any font on the chain
/// (`.notdef` — no fallback further than the chain's tofu policy) or when
/// the resolved raster is zero-size (whitespace sentinel) — matching
/// `cursor_glyph_paintable`'s "no glyph artifact" contract; callers still
/// gate the call with `cursor_glyph_paintable` themselves.
pub fn resolve_overlay_glyph_meta(
    rasterizer: &dyn GlyphRasterizer,
    fallback: &FallbackChain,
    cache: &mut GlyphCache,
    cluster: &str,
    size_px: f32,
    bold: bool,
) -> Option<OverlayGlyphMeta> {
    let font_id = fallback.resolve_for_cluster(rasterizer, cluster)?;
    let font_id = if bold {
        fallback.bold_variant(font_id).unwrap_or(font_id)
    } else {
        font_id
    };
    let shaped = rasterizer.shape(cluster, font_id, size_px);
    let g = shaped.first()?;
    if g.glyph_id == 0 {
        return None;
    }
    let key = GlyphKey::new(font_id, g.glyph_id, size_px, 0.0);
    let cached = cache.get_or_rasterize(rasterizer, key)?;
    let region = cached.region;
    if region.is_empty() {
        return None;
    }
    Some(OverlayGlyphMeta {
        key,
        width: region.width,
        height: region.height,
        bearing_left: region.bearing_left,
        bearing_top: region.bearing_top,
        advance: cached.advance,
        needs_tint: !matches!(region.format, AtlasFormat::Rgba),
        region,
    })
}

/// Copy `meta`'s pixels out of `cache`'s atlas and convert to RGBA8 — the
/// expensive half split out of the old combined `resolve_overlay_glyph`
/// (task0002 r1-p1). Callers MUST only invoke this on a texture-cache
/// MISS; see [`OverlayGlyphMeta`]'s doc for why a cache hit never needs
/// to reach here.
pub fn extract_overlay_glyph_pixels(cache: &GlyphCache, meta: &OverlayGlyphMeta) -> Vec<u8> {
    let (pixels, _needs_tint) = extract_region_rgba(cache.atlas(), meta.region);
    pixels
}

/// Resolve `cluster`'s full glyph raster (metadata + pixels) in one call.
/// Implemented as [`resolve_overlay_glyph_meta`] +
/// [`extract_overlay_glyph_pixels`] (task0002 rework) so existing callers
/// that want the full raster unconditionally (this module's own tests)
/// keep working unchanged. `render::cursor::draw_block_cursor`'s
/// production path calls the split functions directly instead, so a
/// texture-cache hit skips pixel extraction entirely (AC-4) — see
/// [`OverlayGlyphMeta`].
///
/// Returns `None` for the same cases [`resolve_overlay_glyph_meta`] does.
pub fn resolve_overlay_glyph(
    rasterizer: &dyn GlyphRasterizer,
    fallback: &FallbackChain,
    cache: &mut GlyphCache,
    cluster: &str,
    size_px: f32,
    bold: bool,
) -> Option<OverlayGlyph> {
    let meta = resolve_overlay_glyph_meta(rasterizer, fallback, cache, cluster, size_px, bold)?;
    let pixels = extract_overlay_glyph_pixels(cache, &meta);
    Some(OverlayGlyph {
        key: meta.key,
        pixels,
        width: meta.width,
        height: meta.height,
        bearing_left: meta.bearing_left,
        bearing_top: meta.bearing_top,
        advance: meta.advance,
        needs_tint: meta.needs_tint,
    })
}

/// Copy `region`'s pixels out of `atlas`'s backing page and convert to
/// RGBA8. See [`OverlayGlyph::pixels`] for the per-format conversion
/// rule. Pure / allocation-only — no rasterization happens here, only a
/// byte copy out of already-rasterized atlas storage.
///
/// task0002 AC-4 / r1-p1: this is the exact call the split overlay API
/// (`resolve_overlay_glyph_meta` / `extract_overlay_glyph_pixels`) exists
/// to make conditional on a texture-cache MISS instead of unconditional
/// per-frame. `#[cfg(test)]` records every invocation via
/// [`test_hooks`] so tests can assert a cache-hit frame never reaches it.
fn extract_region_rgba(atlas: &Atlas, region: AtlasRegion) -> (Vec<u8>, bool) {
    #[cfg(test)]
    test_hooks::record_extract_region_rgba_call();
    let (page_w, _) = match region.format {
        AtlasFormat::Alpha => atlas.alpha_dim(),
        AtlasFormat::Rgba | AtlasFormat::Subpixel => atlas.rgba_dim(),
    };
    let page_bytes = match region.format {
        AtlasFormat::Alpha => atlas.alpha_bytes(),
        AtlasFormat::Rgba | AtlasFormat::Subpixel => atlas.rgba_bytes(),
    };
    let mut out = Vec::with_capacity((region.width * region.height * 4) as usize);
    match region.format {
        AtlasFormat::Alpha => {
            for row in 0..region.height {
                let row_start = ((region.y + row) * page_w + region.x) as usize;
                for x in 0..region.width as usize {
                    let a = page_bytes[row_start + x];
                    out.extend_from_slice(&[255, 255, 255, a]);
                }
            }
            (out, true)
        }
        AtlasFormat::Rgba => {
            for row in 0..region.height {
                let row_start = ((region.y + row) * page_w + region.x) as usize * 4;
                let row_len = region.width as usize * 4;
                out.extend_from_slice(&page_bytes[row_start..row_start + row_len]);
            }
            (out, false)
        }
        AtlasFormat::Subpixel => {
            // Approximate coverage as the mean of the 3 subpixel
            // channels — the overlay glyph is a single-cell cursor
            // highlight, not the LCD-quality text the grid pass renders,
            // so per-channel subpixel offsets are not worth reproducing
            // here.
            for row in 0..region.height {
                let row_start = ((region.y + row) * page_w + region.x) as usize * 4;
                for x in 0..region.width as usize {
                    let px = row_start + x * 4;
                    let cov = ((page_bytes[px] as u32
                        + page_bytes[px + 1] as u32
                        + page_bytes[px + 2] as u32)
                        / 3) as u8;
                    out.extend_from_slice(&[255, 255, 255, cov]);
                }
            }
            (out, true)
        }
    }
}

// ── Shared placement math (block-cursor-glyph-font task0002 rework) ───
//
// Review round 1 (task0001) found the overlay glyph path diverging from
// `terminal_grid_pass::GridInstanceBuilder::build_instances_split` /
// `glyph_instance` on baseline (r1-c1) and shrink-to-fit (r1-c4). The two
// functions below are the pure formulas both paths must agree on;
// exposing them here (rather than duplicating the arithmetic inline in
// `render::cursor`) is what keeps them from drifting apart again.

/// Vertical centering pad (task0002 AC-2/AC-3, r1-c1): the SAME
/// `((cell_h - line_height) * 0.5).max(0.0)` formula
/// `terminal_grid_pass::GridInstanceBuilder::build_instances_split`
/// computes as `v_pad` to center a row's line inside a cell taller than
/// the base font's natural line height. `render::cursor::draw_block_cursor`
/// calls this with the SAME `(cell_h, line_height)` inputs (in the same
/// physical-pixel units the grid pass uses) so the overlay's covered-glyph
/// baseline lands on the identical line the grid pass drew it on, instead
/// of floating above it (round 1 finding r1-c1: the pre-rework overlay
/// omitted this term entirely).
pub fn compute_v_pad(cell_h: f32, line_height: f32) -> f32 {
    ((cell_h - line_height) * 0.5).max(0.0)
}

/// Horizontal shrink-to-fit factor for the overlay glyph (task0002 AC-5,
/// r1-c4): mirrors `terminal_grid_pass::GridInstanceBuilder::glyph_instance`'s
/// `GlyphFit::HorizontalOnly` reference-width selection — a font's DESIGN
/// `advance` is the shrink reference when the rasterizer reported one (so
/// an ordinary monospace glyph with `advance == cell_w` isn't crushed by
/// its own AA overhang), falling back to the raster's own pixel width
/// (`glyph_w`) when the advance is missing / non-finite / non-positive,
/// matching the same guard `glyph_instance` applies. `cell_w` is the
/// covered footprint width (already accounting for a wide glyph's
/// multi-cell span, as `terminal_grid_pass`'s own `w` does). Returns `1.0`
/// (no shrink) when `cell_w` or the resolved reference is non-positive —
/// defensive, `terminal_grid_pass` never hits that case since `cell_w` is
/// always a positive cell pitch there.
pub fn overlay_horizontal_fit_scale(cell_w: f32, advance: f32, glyph_w: f32) -> f32 {
    if !(cell_w > 0.0) {
        return 1.0;
    }
    let reference = if advance.is_finite() && advance > 0.0 {
        advance
    } else {
        glyph_w
    };
    if !(reference > 0.0) {
        return 1.0;
    }
    (cell_w / reference).min(1.0)
}

/// Test-only invocation counter for [`extract_region_rgba`] (task0002
/// AC-4). A `thread_local` (rather than a plain global) so tests remain
/// isolated even if the harness ever runs them on separate threads;
/// `--test-threads=1` (this project's convention) already serializes
/// execution, but the thread-local adds no cost and removes the
/// assumption.
#[cfg(test)]
pub(crate) mod test_hooks {
    use std::cell::Cell;

    thread_local! {
        static EXTRACT_REGION_RGBA_CALLS: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn record_extract_region_rgba_call() {
        EXTRACT_REGION_RGBA_CALLS.with(|c| c.set(c.get() + 1));
    }

    pub(crate) fn extract_region_rgba_call_count() -> usize {
        EXTRACT_REGION_RGBA_CALLS.with(|c| c.get())
    }

    pub(crate) fn reset_extract_region_rgba_call_count() {
        EXTRACT_REGION_RGBA_CALLS.with(|c| c.set(0));
    }
}

#[cfg(test)]
mod overlay_glyph_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fake rasterizer: always resolves `cluster` to glyph id 7 on
    /// whatever font it is asked about, and counts `raster` calls so
    /// tests can prove the cache — not a second rasterize — served a
    /// repeat lookup.
    struct FakeRasterizer {
        calls: AtomicUsize,
    }

    impl GlyphRasterizer for FakeRasterizer {
        fn shape(&self, _cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
            vec![ShapedGlyph {
                font,
                glyph_id: 7,
                size_px,
            }]
        }
        fn raster(&self, _font: FontId, _glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(GlyphBitmap {
                format: AtlasFormat::Alpha,
                width: 4,
                height: 4,
                bearing: (1, 3),
                advance: 8.0,
                pixels: vec![0x80; 16],
            })
        }
    }

    fn fake() -> (FakeRasterizer, FallbackChain, GlyphCache) {
        (
            FakeRasterizer {
                calls: AtomicUsize::new(0),
            },
            FallbackChain::new(FontId(1), []),
            GlyphCache::new(),
        )
    }

    /// AC-1: the overlay path's cache key is byte-identical to the key
    /// the grid path (`GridInstanceBuilder::glyph_instance`) would build
    /// for the same cluster/size/weight — resolving that key a second
    /// time (mirroring the grid pass's own `cache.get_or_rasterize` call)
    /// hits the cache instead of rasterizing again.
    #[test]
    fn resolve_overlay_glyph_shares_cache_key_and_raster_with_grid_path() {
        let (rasterizer, fallback, mut cache) = fake();

        let overlay = resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("ASCII cluster must resolve");
        assert_eq!(rasterizer.calls.load(Ordering::SeqCst), 1);

        // Mirror the grid path's own resolution steps exactly
        // (`GridInstanceBuilder::glyph_instance`) and confirm it lands on
        // the identical `GlyphKey`.
        let grid_font = fallback.resolve_for_cluster(&rasterizer, "A").unwrap();
        let grid_shaped = rasterizer.shape("A", grid_font, 13.0);
        let grid_key = GlyphKey::new(grid_font, grid_shaped[0].glyph_id, 13.0, 0.0);
        assert_eq!(overlay.key, grid_key);

        // A grid-path lookup at that key must be a cache hit — same
        // raster, no second rasterize call.
        let grid_cached = cache.get_or_rasterize(&rasterizer, grid_key).unwrap();
        assert_eq!(
            rasterizer.calls.load(Ordering::SeqCst),
            1,
            "grid-path lookup at the overlay's key must hit the cache"
        );
        assert_eq!(grid_cached.advance, overlay.advance);
    }

    /// AC-6 / NFR1: a second overlay resolve of the same glyph must not
    /// re-rasterize — the expensive step the "no per-frame allocation"
    /// property protects.
    #[test]
    fn resolve_overlay_glyph_second_call_is_cache_hit() {
        let (rasterizer, fallback, mut cache) = fake();
        let first = resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("first resolve");
        let second = resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("second resolve");
        assert_eq!(first, second);
        assert_eq!(rasterizer.calls.load(Ordering::SeqCst), 1);
    }

    /// AC-4: a wide (CJK) glyph resolved once (the leading-column call
    /// `draw_block_cursor` makes) fires exactly one rasterize call — the
    /// "glyph-request helper invoked exactly once" Test Notes ask for.
    #[test]
    fn resolve_overlay_glyph_wide_glyph_fires_once() {
        let (rasterizer, fallback, mut cache) = fake();
        let glyph = resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "一", 13.0, false)
            .expect("CJK cluster must resolve");
        assert_eq!(glyph.width, 4);
        assert_eq!(rasterizer.calls.load(Ordering::SeqCst), 1);
    }

    /// Alpha-format rasters convert to a flat-white coverage mask so the
    /// caller can tint them with the resolved cell color (AC-3's color
    /// plumbing feeds into this tint).
    #[test]
    fn alpha_raster_converts_to_white_coverage_mask_needing_tint() {
        let (rasterizer, fallback, mut cache) = fake();
        let glyph =
            resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "A", 13.0, false).unwrap();
        assert!(glyph.needs_tint);
        assert_eq!(
            glyph.pixels.len(),
            (glyph.width * glyph.height * 4) as usize
        );
        // Every pixel is opaque-white RGB with the source alpha coverage
        // (0x80, from `FakeRasterizer::raster`) preserved.
        for px in glyph.pixels.chunks_exact(4) {
            assert_eq!(&px[0..3], &[255, 255, 255]);
            assert_eq!(px[3], 0x80);
        }
    }

    /// Rgba-format rasters (color emoji / COLRv1) pass their own color
    /// through unchanged and must NOT be tinted by the caller.
    #[test]
    fn rgba_raster_passes_through_color_without_tint() {
        struct ColorRasterizer;
        impl GlyphRasterizer for ColorRasterizer {
            fn shape(&self, _cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
                vec![ShapedGlyph {
                    font,
                    glyph_id: 9,
                    size_px,
                }]
            }
            fn raster(&self, _font: FontId, _glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
                Some(GlyphBitmap {
                    format: AtlasFormat::Rgba,
                    width: 2,
                    height: 1,
                    bearing: (0, 0),
                    advance: 4.0,
                    pixels: vec![10, 20, 30, 255, 40, 50, 60, 255],
                })
            }
        }
        let rasterizer = ColorRasterizer;
        let fallback = FallbackChain::new(FontId(1), []);
        let mut cache = GlyphCache::new();
        let glyph =
            resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "\u{1F600}", 26.0, false)
                .unwrap();
        assert!(!glyph.needs_tint);
        assert_eq!(glyph.pixels, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    /// A cluster with no glyph in any font on the chain returns `None`
    /// (tofu / `.notdef` case) — `glyph_id == 0` from `shape` short-
    /// circuits before ever calling `raster`.
    #[test]
    fn resolve_overlay_glyph_returns_none_for_unmapped_glyph() {
        struct TofuRasterizer;
        impl GlyphRasterizer for TofuRasterizer {
            fn shape(&self, _cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
                vec![ShapedGlyph {
                    font,
                    glyph_id: 0,
                    size_px,
                }]
            }
            fn raster(&self, _font: FontId, _glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
                panic!("must not be called for glyph_id == 0");
            }
        }
        let rasterizer = TofuRasterizer;
        let fallback = FallbackChain::new(FontId(1), []);
        let mut cache = GlyphCache::new();
        assert!(
            resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "\u{E000}", 13.0, false)
                .is_none()
        );
    }

    /// `bold: true` swaps in the fallback chain's registered bold variant
    /// — the overlay path must honor the same substitution
    /// `GridInstanceBuilder::glyph_instance` applies for SGR-bold cells.
    #[test]
    fn resolve_overlay_glyph_bold_uses_bold_variant_font() {
        let rasterizer = FakeRasterizer {
            calls: AtomicUsize::new(0),
        };
        let mut fallback = FallbackChain::new(FontId(1), []);
        fallback.set_bold_variant(FontId(1), FontId(2));
        let mut cache = GlyphCache::new();
        let glyph =
            resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "A", 13.0, true).unwrap();
        assert_eq!(glyph.key.font, FontId(2));
    }

    // ── task0002 rework: meta / pixel-extraction split (r1-p1, AC-4) ──

    /// [`resolve_overlay_glyph_meta`] alone must never touch
    /// [`extract_region_rgba`] — the whole point of the split is that the
    /// cheap meta lookup (cache key + geometry) is independent of the
    /// pixel-extraction step.
    #[test]
    fn resolve_overlay_glyph_meta_never_extracts_pixels() {
        test_hooks::reset_extract_region_rgba_call_count();
        let (rasterizer, fallback, mut cache) = fake();
        let meta = resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("must resolve");
        assert_eq!(meta.width, 4);
        assert_eq!(meta.height, 4);
        assert_eq!(
            test_hooks::extract_region_rgba_call_count(),
            0,
            "meta-only resolve must not extract pixels"
        );
    }

    /// [`extract_overlay_glyph_pixels`] is the only thing that reaches
    /// [`extract_region_rgba`], and produces the exact same bytes the old
    /// combined [`resolve_overlay_glyph`] returned.
    #[test]
    fn extract_overlay_glyph_pixels_matches_combined_resolve() {
        test_hooks::reset_extract_region_rgba_call_count();
        let (rasterizer, fallback, mut cache) = fake();
        let meta = resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("must resolve");
        let pixels = extract_overlay_glyph_pixels(&cache, &meta);
        assert_eq!(
            test_hooks::extract_region_rgba_call_count(),
            1,
            "extracting pixels must invoke extract_region_rgba exactly once"
        );

        let (rasterizer2, fallback2, mut cache2) = fake();
        let combined =
            resolve_overlay_glyph(&rasterizer2, &fallback2, &mut cache2, "A", 13.0, false)
                .expect("must resolve");
        assert_eq!(pixels, combined.pixels);
    }

    // ── compute_v_pad (AC-2/AC-3) ──────────────────────────────────────

    #[test]
    fn compute_v_pad_matches_grid_pass_formula() {
        // Mirrors `build_instances_split`'s `let v_pad = ((metrics.cell_h
        // - base_line_height) * 0.5).max(0.0);` — cell_h=20,
        // line_height=16 → v_pad=2.0.
        assert_eq!(compute_v_pad(20.0, 16.0), 2.0);
    }

    #[test]
    fn compute_v_pad_clamps_negative_to_zero() {
        // line_height taller than the cell must never produce negative
        // padding (matches the grid pass's `.max(0.0)`).
        assert_eq!(compute_v_pad(10.0, 16.0), 0.0);
    }

    // ── overlay_horizontal_fit_scale (AC-5) ─────────────────────────────

    #[test]
    fn overlay_horizontal_fit_scale_shrinks_wide_advance_glyph() {
        // A CJK Dingbat fallback glyph with advance ≈ 2x the cell width.
        assert_eq!(overlay_horizontal_fit_scale(10.0, 20.0, 8.0), 0.5);
    }

    #[test]
    fn overlay_horizontal_fit_scale_no_shrink_when_advance_fits() {
        assert_eq!(overlay_horizontal_fit_scale(10.0, 9.0, 11.0), 1.0);
    }

    #[test]
    fn overlay_horizontal_fit_scale_falls_back_to_glyph_width_when_advance_missing() {
        // advance == 0.0 (rasterizer did not report one) → use glyph_w.
        assert_eq!(overlay_horizontal_fit_scale(10.0, 0.0, 20.0), 0.5);
    }

    #[test]
    fn overlay_horizontal_fit_scale_defensive_zero_cell_width() {
        assert_eq!(overlay_horizontal_fit_scale(0.0, 20.0, 8.0), 1.0);
    }
}
