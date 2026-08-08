use super::*;
use crate::render::font::resolver::Resolver;
use crate::render::font::swash_adapter::SwashRasterizer;
use crate::render::font::traits::{AtlasFormat, FontId, GlyphBitmap, ShapedGlyph};

/// Test rasterizer that returns canned bitmaps from a static table.
struct StubRasterizer {
    ascii_font: FontId,
    emoji_font: FontId,
}

impl GlyphRasterizer for StubRasterizer {
    fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
        // Map ascii -> glyph id = byte value; cluster 'あ' -> 0xAA; '😀' -> 0xBB.
        let first = cluster.chars().next().unwrap_or('\0') as u32;
        let glyph_id = match first {
            0x41..=0x7A => first,
            0x3042 => 0xAA,
            0x1F600 => 0xBB,
            _ => 0,
        };
        vec![ShapedGlyph {
            font,
            glyph_id,
            size_px,
        }]
    }
    fn raster(&self, font: FontId, glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
        if glyph_id == 0 {
            return None;
        }
        if font == self.emoji_font {
            Some(GlyphBitmap {
                format: AtlasFormat::Rgba,
                width: 16,
                height: 16,
                bearing: (0, 0),
                advance: 16.0,
                pixels: vec![0xFF; 16 * 16 * 4],
            })
        } else if font == self.ascii_font {
            Some(GlyphBitmap {
                format: AtlasFormat::Alpha,
                width: 8,
                height: 16,
                bearing: (0, 0),
                advance: 8.0,
                pixels: vec![0xFF; 8 * 16],
            })
        } else {
            None
        }
    }
    fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
        match (font, cp) {
            (f, c) if f == self.ascii_font && (0x41..=0x7A).contains(&c) => true,
            (f, 0x3042) if f != self.ascii_font && f != self.emoji_font => true,
            (f, 0x1F600) if f == self.emoji_font => true,
            _ => false,
        }
    }
}

/// Rasterizer that records every `shape()` call's exact input string
/// (task0002 AC-1/AC-4). Coverage mirrors the real-world quirk
/// documented on `is_pictographic`/`resolve_for_cluster` tests: the
/// mock's emoji font also covers ASCII digits (0-9) and U+26A0, so a
/// keycap cluster / ExtPict+VS16 cluster resolves to the emoji font
/// exactly as the bundled `Noto-COLRv1.ttf` does.
struct RecordingRasterizer {
    ascii_font: FontId,
    emoji_font: FontId,
    shape_calls: Mutex<Vec<String>>,
}

impl GlyphRasterizer for RecordingRasterizer {
    fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
        self.shape_calls.lock().push(cluster.to_string());
        let first = cluster.chars().next().unwrap_or('\0') as u32;
        let glyph_id = match first {
            0x30..=0x39 | 0x41..=0x7A | 0x26A0 => first,
            _ => 0,
        };
        vec![ShapedGlyph {
            font,
            glyph_id,
            size_px,
        }]
    }
    fn raster(&self, font: FontId, glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
        if glyph_id == 0 {
            return None;
        }
        if font == self.emoji_font {
            Some(GlyphBitmap {
                format: AtlasFormat::Rgba,
                width: 16,
                height: 16,
                bearing: (0, 0),
                advance: 16.0,
                pixels: vec![0xFF; 16 * 16 * 4],
            })
        } else {
            Some(GlyphBitmap {
                format: AtlasFormat::Alpha,
                width: 8,
                height: 16,
                bearing: (0, 0),
                advance: 8.0,
                pixels: vec![0xFF; 8 * 16],
            })
        }
    }
    fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
        match (font, cp) {
            (f, c) if f == self.ascii_font && (0x41..=0x7A).contains(&c) => true,
            (f, c) if f == self.emoji_font && (0x30..=0x39).contains(&c) => true,
            (f, 0x26A0) if f == self.emoji_font => true,
            _ => false,
        }
    }
}

/// Standalone wrapper that mirrors `TerminalGridPass::build_instances`
/// without instantiating the wgpu-bearing fields. The logic is
/// identical and lives in the same file so any changes stay in sync.
fn helper_build_instances(
    rasterizer: &dyn GlyphRasterizer,
    fallback: &FallbackChain,
    cache: &Arc<Mutex<GlyphCache>>,
    cells: &[CellInput],
    metrics: CellMetrics,
) -> Vec<CellInstance> {
    // Two-pass ordering, identical to production `build_instances`:
    // all bgs first, then all foreground quads.
    let mut bgs = Vec::with_capacity(cells.len());
    let mut fg = Vec::with_capacity(cells.len() * 2);
    let mut cache_lock = cache.lock();
    let base_metrics = rasterizer.font_metrics(fallback.base(), metrics.font_size_px);
    let base_ascent = base_metrics
        .map(|m| m.ascent)
        .unwrap_or(metrics.font_size_px * 0.8);
    let base_line_height = base_metrics
        .map(|m| m.line_height())
        .unwrap_or(metrics.font_size_px);
    let v_pad = compute_v_pad(metrics.cell_h, base_line_height);
    for cell in cells {
        let x = metrics.origin[0] + cell.col as f32 * metrics.cell_w;
        let y = metrics.origin[1] + cell.row as f32 * metrics.cell_h;
        let w = metrics.cell_w * (cell.width_cells.max(1) as f32);
        let h = metrics.cell_h;
        if cell.draw_background {
            bgs.push(CellInstance {
                cell_xy: [x, y],
                cell_wh: [w, h],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                fg_rgba: pack_rgba(cell.bg_rgba),
                bg_rgba: pack_rgba(cell.bg_rgba),
                page: PAGE_SOLID,
                flags: 0,
            });
        }
        if !cell.glyph.is_empty() && cell.glyph != " " {
            if let Some(font_id) = fallback.resolve_for_cluster(rasterizer, &cell.glyph) {
                // Same VS16-stripping decision as the production
                // `glyph_instance` path (task0002 FR5) — kept in sync
                // so this test-mirror site never diverges.
                let shaping_input = fallback.shaping_cluster(&cell.glyph, font_id);
                let shaped = rasterizer.shape(&shaping_input, font_id, metrics.font_size_px);
                if let Some(g) = shaped.first() {
                    if g.glyph_id != 0 {
                        let key = GlyphKey::new(font_id, g.glyph_id, metrics.font_size_px, 0.0);
                        if let Some(cached) = cache_lock.get_or_rasterize(rasterizer, key) {
                            let region = cached.region;
                            if !region.is_empty() {
                                let page = match region.format {
                                    AtlasFormat::Alpha => PAGE_ALPHA,
                                    AtlasFormat::Rgba => PAGE_RGBA,
                                    AtlasFormat::Subpixel => PAGE_SUBPIXEL,
                                };
                                let glyph_w = region.width as f32;
                                let glyph_h = region.height as f32;
                                let baseline = y + v_pad + base_ascent;
                                let glyph_x = x + region.bearing_left as f32;
                                let glyph_y = baseline - region.bearing_top as f32;
                                fg.push(CellInstance {
                                    cell_xy: [glyph_x, glyph_y],
                                    cell_wh: [glyph_w, glyph_h],
                                    atlas_uv: [
                                        region.x as f32,
                                        region.y as f32,
                                        (region.x + region.width) as f32,
                                        (region.y + region.height) as f32,
                                    ],
                                    fg_rgba: pack_rgba(cell.fg_rgba),
                                    bg_rgba: pack_rgba(cell.bg_rgba),
                                    page,
                                    flags: 0,
                                });
                            }
                        }
                    }
                }
            }
        }
        if cell.underline {
            fg.push(CellInstance {
                cell_xy: [x, y],
                cell_wh: [w, h],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                fg_rgba: pack_rgba(cell.fg_rgba),
                bg_rgba: pack_rgba(cell.bg_rgba),
                page: PAGE_SOLID,
                flags: FLAG_UNDERLINE,
            });
        }
        if cell.strikethrough {
            fg.push(CellInstance {
                cell_xy: [x, y],
                cell_wh: [w, h],
                atlas_uv: [0.0, 0.0, 0.0, 0.0],
                fg_rgba: pack_rgba(cell.fg_rgba),
                bg_rgba: pack_rgba(cell.bg_rgba),
                page: PAGE_SOLID,
                flags: FLAG_STRIKETHROUGH,
            });
        }
    }
    bgs.extend(fg);
    bgs
}

fn ascii_cell(col: u16, row: u16, ch: &str) -> CellInput {
    CellInput {
        col,
        row,
        width_cells: 1,
        glyph: ch.into(),
        fg_rgba: [255, 255, 255, 255],
        bg_rgba: [0, 0, 0, 255],
        underline: false,
        strikethrough: false,
        draw_background: false,
        bg_extend_below: 0.0,
        fit: GlyphFit::None,
        bold: false,
    }
}

fn metrics() -> CellMetrics {
    CellMetrics {
        cell_w: 8.5,
        cell_h: 17.0,
        origin: [0.0, 0.0],
        font_size_px: 13.0,
    }
}

fn build_stack() -> (
    Arc<StubRasterizer>,
    Arc<FallbackChain>,
    Arc<Mutex<GlyphCache>>,
) {
    let ascii = FontId(1);
    let cjk = FontId(2);
    let emoji = FontId(3);
    let raster = Arc::new(StubRasterizer {
        ascii_font: ascii,
        emoji_font: emoji,
    });
    let chain = Arc::new(FallbackChain::new(ascii, [cjk, emoji]));
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    (raster, chain, cache)
}

/// Fresh [`GridInstanceBuilder`] wired to the same `StubRasterizer`
/// stack `build_stack` sets up for `helper_build_instances` — used by
/// the task0003 row-cache tests to exercise the *real* rebuild /
/// concatenation implementation (not a hand-duplicated mirror) without
/// a wgpu device.
fn instance_builder() -> GridInstanceBuilder {
    let (raster, chain, cache) = build_stack();
    GridInstanceBuilder::new(cache, chain, raster as Arc<dyn GlyphRasterizer>)
}

/// TS-font-13: `TerminalGridPass::prepare` emits one (glyph) instance
/// per non-empty cell. We exercise the CPU-side `build_instances`
/// helper here — it is the path GPU `prepare` calls before uploading.
#[test]
fn build_instances_one_per_non_empty_cell() {
    let (raster, chain, cache) = build_stack();
    let cells = vec![
        ascii_cell(0, 0, "A"),
        ascii_cell(1, 0, "B"),
        ascii_cell(2, 0, "C"),
        ascii_cell(3, 0, " "), // whitespace → no glyph instance
        ascii_cell(4, 0, ""),  // empty cluster → no glyph instance
    ];
    let inst = helper_build_instances(&*raster, &chain, &cache, &cells, metrics());
    // Exactly 3 glyph instances; whitespace + empty produce nothing
    // (draw_background = false → no bg quad either).
    assert_eq!(inst.len(), 3);
    for i in &inst {
        assert_eq!(i.page, PAGE_ALPHA);
        // UV is non-empty for hit glyphs.
        assert!(i.atlas_uv[2] > i.atlas_uv[0]);
        assert!(i.atlas_uv[3] > i.atlas_uv[1]);
    }
}

/// TS-font-14: per-instance `page` tag encodes Alpha for ASCII and
/// RGBA for color emoji.
#[test]
fn build_instances_records_page_kind_per_glyph() {
    let (raster, chain, cache) = build_stack();
    let cells = vec![
        ascii_cell(0, 0, "A"),
        CellInput {
            col: 2,
            row: 0,
            width_cells: 2,
            glyph: "\u{1F600}".into(), // 😀
            fg_rgba: [255, 255, 255, 255],
            bg_rgba: [0, 0, 0, 255],
            underline: false,
            strikethrough: false,
            draw_background: false,
            bg_extend_below: 0.0,
            fit: GlyphFit::None,
            bold: false,
        },
    ];
    let inst = helper_build_instances(&*raster, &chain, &cache, &cells, metrics());
    assert_eq!(inst.len(), 2);
    // First cell: alpha; second: rgba.
    assert_eq!(inst[0].page, PAGE_ALPHA);
    assert_eq!(inst[1].page, PAGE_RGBA);
}

// ── clip_quad_to_cell_x ──────────────────────────────────

/// A quad already inside the cell passes through untouched.
#[test]
fn clip_quad_inside_cell_is_unchanged() {
    let r = clip_quad_to_cell_x(10.0, 8.0, 0.0, 8.0, 9.0, 18.0);
    assert_eq!(r, Some((10.0, 8.0, 0.0, 8.0)));
}

/// The call site in `glyph_instance` snaps fractional cell bounds via
/// `.round()` before passing them to `clip_quad_to_cell_x`. This test
/// demonstrates that contract: a pixel-snapped quad (glyph_x=11.0,
/// glyph_w=8.0) that fits perfectly inside a fractional-scale cell
/// [10.75, 19.5] would be wrongly trimmed if the raw bounds were passed,
/// but after the call-site snap to [11.0, 20.0] the quad passes through
/// unchanged (no sub-pixel sliver is shaved off).
#[test]
fn clip_quad_call_site_snaps_fractional_cell_bounds() {
    let (glyph_x, glyph_w) = (11.0_f32, 8.0_f32);
    let (u0, u1) = (0.0_f32, 8.0_f32);
    // Raw fractional cell bounds (1.5× HiDPI example).
    let cell_left_raw = 10.75_f32;
    let cell_right_raw = 19.5_f32;
    // Without snapping, left_trim = 10.75 - 11.0 = -0.25 (no left clip),
    // but right_trim = (11.0+8.0) - 19.5 = -0.5, which is also ≤ 0, so
    // the raw bounds actually pass here too — the real hazard is when the
    // fractional cell_left > glyph_x, which shaves the left side.
    // Use a case where the fractional left is strictly above glyph_x:
    // cell [11.25, 20.0] → left_trim = 0.25 → wrong UV shift without snap.
    let cell_left_frac = 11.25_f32;
    let cell_right_frac = 20.0_f32;
    // Without snap: left_trim > 0 → quad and UV are modified (wrong).
    let without_snap =
        clip_quad_to_cell_x(glyph_x, glyph_w, u0, u1, cell_left_frac, cell_right_frac);
    assert_ne!(
        without_snap,
        Some((glyph_x, glyph_w, u0, u1)),
        "raw fractional bounds wrongly trim a fitting quad"
    );
    // With snap (as the call site does): [11.25.round(), 20.0.round()] = [11.0, 20.0].
    let with_snap = clip_quad_to_cell_x(
        glyph_x,
        glyph_w,
        u0,
        u1,
        cell_left_frac.round(),
        cell_right_frac.round(),
    );
    assert_eq!(
        with_snap,
        Some((glyph_x, glyph_w, u0, u1)),
        "snapped bounds leave a fitting pixel-aligned quad unchanged"
    );
    let _ = (cell_left_raw, cell_right_raw); // documented above; not used in assertions
}

/// Inconsolata 'm' at 13 pt: bearing −1, bitmap 11 px wide in a 9-px
/// cell. Both overhangs trim, and the UV range shrinks by the same
/// amount on each side (1:1 texel mapping preserved).
#[test]
fn clip_quad_overhang_trims_both_sides_and_uv() {
    // Cell [9, 18), quad [8, 19) → clipped to [9, 18).
    let r = clip_quad_to_cell_x(8.0, 11.0, 100.0, 111.0, 9.0, 18.0);
    let (x, w, u0, u1) = r.expect("clipped quad survives");
    assert_eq!((x, w), (9.0, 9.0));
    assert_eq!((u0, u1), (101.0, 110.0));
}

/// A quad entirely outside the cell clips to nothing.
#[test]
fn clip_quad_outside_cell_returns_none() {
    assert_eq!(clip_quad_to_cell_x(20.0, 5.0, 0.0, 5.0, 0.0, 9.0), None);
    assert_eq!(clip_quad_to_cell_x(0.0, 0.0, 0.0, 0.0, 0.0, 9.0), None);
}

/// Subpixel-mode swash output routes to the PAGE_SUBPIXEL shader
/// branch (per-channel fg/bg compositing).
#[test]
fn integration_swash_subpixel_maps_to_subpixel_page() {
    let mut resolver = Resolver::new();
    let (cjk_id, emoji_id, _mono_id, _base_id, _sym_id) = resolver.register_bundled();
    let swash = Arc::new(SwashRasterizer::with_subpixel(true));
    swash.ingest_resolver(&resolver);
    let chain = Arc::new(FallbackChain::new(cjk_id, [emoji_id]));
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let cells = vec![ascii_cell(0, 0, "d")];
    let raster_ref: &dyn GlyphRasterizer = &*swash;
    let inst = helper_build_instances(raster_ref, &chain, &cache, &cells, metrics());
    assert_eq!(inst.len(), 1, "exactly one glyph instance for 'd'");
    assert_eq!(
        inst[0].page, PAGE_SUBPIXEL,
        "subpixel raster must select the subpixel shader page"
    );
}

/// TS-font-int-2: headless render of a single cell containing U+3042
/// using the swash engine. The pass emits a non-empty instance and
/// does not panic.
#[test]
fn integration_swash_renders_cjk_cell_cpu_side() {
    // Build a swash rasterizer + resolver against the bundled fonts.
    let mut resolver = Resolver::new();
    let (cjk_id, emoji_id, _mono_id, _base_id, _sym_id) = resolver.register_bundled();
    let swash = Arc::new(SwashRasterizer::with_subpixel(false));
    swash.ingest_resolver(&resolver);
    // Chain: cjk first (no base font registered against swash here,
    // so 'A' would tofu — TS-font-int-2 only tests U+3042).
    let chain = Arc::new(FallbackChain::new(cjk_id, [emoji_id]));
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let cells = vec![CellInput {
        col: 0,
        row: 0,
        width_cells: 2,
        glyph: "\u{3042}".into(), // あ
        fg_rgba: [255, 255, 255, 255],
        bg_rgba: [0, 0, 0, 255],
        underline: false,
        strikethrough: false,
        draw_background: false,
        bg_extend_below: 0.0,
        fit: GlyphFit::None,
        bold: false,
    }];
    let raster_ref: &dyn GlyphRasterizer = &*swash;
    let inst = helper_build_instances(
        raster_ref,
        &chain,
        &cache,
        &cells,
        CellMetrics {
            cell_w: 16.0,
            cell_h: 24.0,
            origin: [0.0, 0.0],
            font_size_px: 18.0,
        },
    );
    assert_eq!(inst.len(), 1, "exactly one glyph instance for U+3042");
    assert_eq!(inst[0].page, PAGE_ALPHA, "CJK is monochrome → alpha page");
    assert!(
        inst[0].atlas_uv[2] > inst[0].atlas_uv[0],
        "non-empty UV width"
    );
}

// ── task0002 FR5: VS16 stripped before emoji shaping ────────────────

/// AC-1: a keycap cluster (`5 FE0F 20E3`) routed to the color emoji
/// font is shaped with U+FE0F stripped. Exercised through the
/// PRODUCTION path (`GridInstanceBuilder::build_instances`, which
/// calls `glyph_instance` per cell) rather than the `helper_*` mirror.
#[test]
fn glyph_instance_strips_vs16_for_keycap_cluster() {
    let ascii = FontId(1);
    let emoji = FontId(2);
    let raster = Arc::new(RecordingRasterizer {
        ascii_font: ascii,
        emoji_font: emoji,
        shape_calls: Mutex::new(Vec::new()),
    });
    let mut chain = FallbackChain::new(ascii, [emoji]);
    chain.set_emoji(emoji);
    let chain = Arc::new(chain);
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let builder =
        GridInstanceBuilder::new(cache, chain, raster.clone() as Arc<dyn GlyphRasterizer>);

    let cluster: String = ['5', '\u{FE0F}', '\u{20E3}'].iter().collect();
    let cells = vec![ascii_cell(0, 0, &cluster)];
    let inst = builder.build_instances(&cells, metrics());
    assert_eq!(inst.len(), 1, "keycap cluster must still emit a glyph");

    let calls = raster.shape_calls.lock();
    assert_eq!(calls.len(), 1, "exactly one shape() call for one cell");
    assert!(
        !calls[0].contains('\u{FE0F}'),
        "shaping input must not contain VS16, got {:?}",
        calls[0]
    );
    let expected: String = ['5', '\u{20E3}'].iter().collect();
    assert_eq!(calls[0], expected);
}

/// AC-4: a non-emoji-routed cluster's shaping input is byte-identical
/// to the cell content — no stripping outside the emoji path.
#[test]
fn glyph_instance_leaves_non_emoji_cluster_byte_identical() {
    let ascii = FontId(1);
    let emoji = FontId(2);
    let raster = Arc::new(RecordingRasterizer {
        ascii_font: ascii,
        emoji_font: emoji,
        shape_calls: Mutex::new(Vec::new()),
    });
    let mut chain = FallbackChain::new(ascii, [emoji]);
    chain.set_emoji(emoji);
    let chain = Arc::new(chain);
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let builder =
        GridInstanceBuilder::new(cache, chain, raster.clone() as Arc<dyn GlyphRasterizer>);

    let cells = vec![ascii_cell(0, 0, "A")];
    let _inst = builder.build_instances(&cells, metrics());

    let calls = raster.shape_calls.lock();
    assert_eq!(calls.as_slice(), ["A".to_string()]);
}

/// AC-5: an ExtPict + VS16 cluster (U+26A0 U+FE0F) still renders its
/// emoji glyph after stripping — the pre-existing "accidental
/// correctness" case (`shaped.first()`) must survive the fix.
#[test]
fn glyph_instance_ext_pict_vs16_cluster_still_renders_after_stripping() {
    let ascii = FontId(1);
    let emoji = FontId(2);
    let raster = Arc::new(RecordingRasterizer {
        ascii_font: ascii,
        emoji_font: emoji,
        shape_calls: Mutex::new(Vec::new()),
    });
    let mut chain = FallbackChain::new(ascii, [emoji]);
    chain.set_emoji(emoji);
    let chain = Arc::new(chain);
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let builder =
        GridInstanceBuilder::new(cache, chain, raster.clone() as Arc<dyn GlyphRasterizer>);

    let cluster: String = ['\u{26A0}', '\u{FE0F}'].iter().collect();
    let cells = vec![ascii_cell(0, 0, &cluster)];
    let inst = builder.build_instances(&cells, metrics());
    assert_eq!(inst.len(), 1, "warning-sign + VS16 must still render");
    assert_eq!(
        inst[0].page, PAGE_RGBA,
        "must render via the color emoji font"
    );

    let calls = raster.shape_calls.lock();
    assert_eq!(calls.as_slice(), ["\u{26A0}".to_string()]);
}

/// Design decision 4 (IMPLEMENTATION.md): the test-mirror site
/// (`helper_build_instances`) applies the same stripping decision as
/// the production `glyph_instance` path — no divergence between the
/// main and secondary shaping call sites.
#[test]
fn helper_build_instances_strips_vs16_for_keycap_cluster() {
    let ascii = FontId(1);
    let emoji = FontId(2);
    let raster = RecordingRasterizer {
        ascii_font: ascii,
        emoji_font: emoji,
        shape_calls: Mutex::new(Vec::new()),
    };
    let mut chain = FallbackChain::new(ascii, [emoji]);
    chain.set_emoji(emoji);
    let cache = Arc::new(Mutex::new(GlyphCache::new()));

    let cluster: String = ['5', '\u{FE0F}', '\u{20E3}'].iter().collect();
    let cells = vec![ascii_cell(0, 0, &cluster)];
    let raster_ref: &dyn GlyphRasterizer = &raster;
    let inst = helper_build_instances(raster_ref, &chain, &cache, &cells, metrics());
    assert_eq!(inst.len(), 1, "keycap cluster must still emit a glyph");

    let calls = raster.shape_calls.lock();
    assert_eq!(calls.len(), 1);
    assert!(
        !calls[0].contains('\u{FE0F}'),
        "secondary site must also strip VS16, got {:?}",
        calls[0]
    );
}

/// AC-2: shaping the VS16-stripped keycap cluster with the bundled
/// color-emoji font (via swash, the real adapter) yields a single
/// glyph — the GSUB `<digit> + U+20E3` ligature match. This is the
/// real-font regression the stripping fixes: swash's ligature
/// matcher does not skip VS16, so the unstripped cluster decomposes.
#[test]
fn integration_swash_keycap_cluster_shapes_to_single_glyph_after_stripping() {
    let mut resolver = Resolver::new();
    let (_cjk_id, emoji_id, _mono_id, base_id, _sym_id) = resolver.register_bundled();
    let swash = SwashRasterizer::with_subpixel(false);
    swash.ingest_resolver(&resolver);
    let mut chain = FallbackChain::new(base_id, [emoji_id]);
    chain.set_emoji(emoji_id);

    let cluster: String = ['5', '\u{FE0F}', '\u{20E3}'].iter().collect();
    let raster_ref: &dyn GlyphRasterizer = &swash;
    let font_id = chain
        .resolve_for_cluster(raster_ref, &cluster)
        .expect("keycap cluster must resolve to a font");
    assert_eq!(
        font_id, emoji_id,
        "keycap cluster must resolve via the color emoji font"
    );

    let shaping_input = chain.shaping_cluster(&cluster, font_id);
    assert!(!shaping_input.contains('\u{FE0F}'));
    let shaped = swash.shape(&shaping_input, font_id, 17.0);
    assert_eq!(
        shaped.len(),
        1,
        "stripped keycap cluster must shape to a single ligature glyph, got {:?}",
        shaped
    );
    assert_ne!(shaped[0].glyph_id, 0, "ligature glyph must not be .notdef");
}

/// AC-5 (real-font companion): U+26A0 + VS16 still shapes to a
/// nonzero glyph via the color emoji font after VS16 is stripped.
#[test]
fn integration_swash_ext_pict_vs16_cluster_still_renders_after_stripping() {
    let mut resolver = Resolver::new();
    let (_cjk_id, emoji_id, _mono_id, base_id, _sym_id) = resolver.register_bundled();
    let swash = SwashRasterizer::with_subpixel(false);
    swash.ingest_resolver(&resolver);
    let mut chain = FallbackChain::new(base_id, [emoji_id]);
    chain.set_emoji(emoji_id);

    let cluster: String = ['\u{26A0}', '\u{FE0F}'].iter().collect();
    let raster_ref: &dyn GlyphRasterizer = &swash;
    let font_id = chain
        .resolve_for_cluster(raster_ref, &cluster)
        .expect("warning sign + VS16 must resolve to a font");
    assert_eq!(
        font_id, emoji_id,
        "warning sign + VS16 must resolve via the color emoji font"
    );

    let shaping_input = chain.shaping_cluster(&cluster, font_id);
    assert_eq!(shaping_input.as_ref(), "\u{26A0}");
    let shaped = swash.shape(&shaping_input, font_id, 17.0);
    let glyph = shaped
        .first()
        .expect("shaping must yield at least one glyph");
    assert_ne!(glyph.glyph_id, 0, "warning-sign glyph must not be .notdef");
}

#[test]
fn pack_rgba_byte_order_is_little_endian_rgba() {
    // [r=0x11, g=0x22, b=0x33, a=0xFF] packs as 0xFF332211.
    let p = pack_rgba([0x11, 0x22, 0x33, 0xFF]);
    assert_eq!(p, 0xFF332211);
}

#[test]
fn cell_instance_stride_matches_layout() {
    // The wgpu pipeline encodes the stride; if this changes, the
    // VertexAttribute offsets above must be updated.
    assert_eq!(CellInstance::STRIDE, 48);
}

#[test]
fn empty_cells_produce_no_instances() {
    let (raster, chain, cache) = build_stack();
    let inst = helper_build_instances(&*raster, &chain, &cache, &[], metrics());
    assert!(inst.is_empty());
}

/// `build_instances` emits all background quads before any
/// foreground quad (glyph / box-drawing / decoration). Without this
/// ordering, row N+1's bg quad — pushed after row N's glyph in the
/// per-cell loop — would overwrite row N glyph overhang via the
/// no-depth-test draw, clipping tall single-cell glyphs like
/// U+25FB ◻ at the cell bottom.
#[test]
fn build_instances_emits_all_bgs_before_any_glyph() {
    let (raster, chain, cache) = build_stack();
    let mut a = ascii_cell(0, 0, "A");
    a.draw_background = true;
    let mut b = ascii_cell(0, 1, "B");
    b.draw_background = true;
    let mut c = ascii_cell(0, 2, "C");
    c.draw_background = true;
    c.underline = true;
    let inst = helper_build_instances(&*raster, &chain, &cache, &[a, b, c], metrics());
    // 3 bgs (SOLID, no flags) + 3 glyphs (ALPHA) + 1 underline (SOLID, FLAG_UNDERLINE).
    assert_eq!(inst.len(), 7);
    // First three instances must all be plain bg quads.
    for i in &inst[..3] {
        assert_eq!(i.page, PAGE_SOLID);
        assert_eq!(i.flags, 0);
    }
    // Remaining instances are the foreground pass: 3 glyphs then 1 underline.
    let fg_pages: Vec<u32> = inst[3..].iter().map(|i| i.page).collect();
    let fg_flags: Vec<u32> = inst[3..].iter().map(|i| i.flags).collect();
    assert_eq!(
        fg_pages,
        vec![PAGE_ALPHA, PAGE_ALPHA, PAGE_ALPHA, PAGE_SOLID]
    );
    assert_eq!(fg_flags, vec![0, 0, 0, FLAG_UNDERLINE]);
}

// ── clip_quad_to_cell_y ──────────────────────────────────

/// A vertically-fitting quad passes through `clip_quad_to_cell_y`
/// unchanged (twin of the X-axis fitting-quad case).
#[test]
fn clip_quad_y_inside_cell_is_unchanged() {
    let r = clip_quad_to_cell_y(10.0, 8.0, 0.0, 8.0, 9.0, 18.0);
    assert_eq!(r, Some((10.0, 8.0, 0.0, 8.0)));
}

/// Top + bottom overhang shaves equal V-axis margins, preserving
/// the 1:1 texel-to-pixel mapping for the visible portion. Mirrors
/// `clip_quad_overhang_trims_both_sides_and_uv` for the Y axis.
#[test]
fn clip_quad_y_overhang_trims_both_sides_and_uv() {
    // Cell [9, 18), quad [8, 19) on the Y axis → clipped to [9, 18).
    let r = clip_quad_to_cell_y(8.0, 11.0, 100.0, 111.0, 9.0, 18.0);
    let (y, h, v0, v1) = r.expect("clipped quad survives");
    assert_eq!((y, h), (9.0, 9.0));
    assert_eq!((v0, v1), (101.0, 110.0));
}

/// A quad entirely outside the cell vertically clips to nothing.
#[test]
fn clip_quad_y_outside_cell_returns_none() {
    assert_eq!(clip_quad_to_cell_y(20.0, 5.0, 0.0, 5.0, 0.0, 9.0), None);
    assert_eq!(clip_quad_to_cell_y(0.0, 0.0, 0.0, 0.0, 0.0, 9.0), None);
}

/// Decoration flags emit dedicated solid-color instances on top of
/// the glyph instance.
#[test]
fn decoration_flags_emit_solid_instances() {
    let (raster, chain, cache) = build_stack();
    let mut cell = ascii_cell(0, 0, "A");
    cell.underline = true;
    cell.strikethrough = true;
    let inst = helper_build_instances(&*raster, &chain, &cache, &[cell], metrics());
    // 1 glyph + 1 underline + 1 strikethrough.
    assert_eq!(inst.len(), 3);
    let pages: Vec<u32> = inst.iter().map(|i| i.page).collect();
    let flags: Vec<u32> = inst.iter().map(|i| i.flags).collect();
    assert_eq!(pages, vec![PAGE_ALPHA, PAGE_SOLID, PAGE_SOLID]);
    assert_eq!(flags, vec![0, FLAG_UNDERLINE, FLAG_STRIKETHROUGH]);
}

// ── task0003 AC-4: persistent-buffer growth policy ─────────────────

/// AC-4: capacity never decreases once the required size already fits.
#[test]
fn grow_capacity_never_decreases_when_it_already_fits() {
    assert_eq!(grow_capacity(1000, 500), 1000);
    assert_eq!(grow_capacity(1000, 1000), 1000);
}

/// AC-4: capacity always covers the required size, even growing from
/// zero (the first-ever `prepare` call).
#[test]
fn grow_capacity_always_covers_required_size() {
    assert!(grow_capacity(0, 4096) >= 4096);
    assert!(grow_capacity(100, 5000) >= 5000);
    assert!(grow_capacity(0, 1_000_000) >= 1_000_000);
}

/// AC-4: a small requirement is floored at `MIN_BUFFER_CAPACITY_BYTES`
/// rather than allocating the bare minimum every time.
#[test]
fn grow_capacity_floors_small_requirements() {
    assert_eq!(grow_capacity(0, 48), MIN_BUFFER_CAPACITY_BYTES);
}

/// AC-4: geometric growth bounds the number of reallocations under a
/// monotonically increasing requirement — doubling the required size
/// 20 times triggers far fewer than 20 capacity changes.
#[test]
fn grow_capacity_geometric_growth_bounds_reallocation_count() {
    let mut capacity = 0u64;
    let mut required = 48u64;
    let mut reallocations = 0;
    for _ in 0..20 {
        let new_capacity = grow_capacity(capacity, required);
        if new_capacity != capacity {
            reallocations += 1;
            capacity = new_capacity;
        }
        assert!(capacity >= required, "capacity must always cover required");
        required *= 2;
    }
    assert!(
        reallocations < 20,
        "geometric growth should need fewer reallocations than linear regrowth, got {reallocations}"
    );
}

// ── task0003: RowCache concatenation (mechanical, synthetic instances) ──

/// A distinguishable synthetic `CellInstance` for `RowCache` ordering
/// tests: `fg_rgba` carries an identity tag so assertions can name
/// which row/pass an instance came from without needing real glyph
/// shaping.
fn tagged_instance(tag: u32) -> CellInstance {
    CellInstance {
        cell_xy: [0.0, 0.0],
        cell_wh: [0.0, 0.0],
        atlas_uv: [0.0, 0.0, 0.0, 0.0],
        fg_rgba: tag,
        bg_rgba: 0,
        page: PAGE_SOLID,
        flags: 0,
    }
}

/// `RowCache::concat_all` emits every row's `bg` entries (in row
/// order) before any row's `fg` entries (in row order) — the two-pass
/// invariant that keeps the row-cache path byte-identical to a
/// from-scratch `build_instances` call (see the `RowCache` doc).
#[test]
fn row_cache_concat_all_orders_all_bgs_before_any_fg() {
    let mut cache = RowCache::default();
    cache.resize(3);
    cache.set(
        0,
        RowInstances {
            bg: vec![tagged_instance(100)],
            fg: vec![tagged_instance(101)],
        },
    );
    cache.set(
        1,
        RowInstances {
            bg: vec![tagged_instance(200)],
            fg: vec![tagged_instance(201)],
        },
    );
    cache.set(
        2,
        RowInstances {
            bg: vec![tagged_instance(300)],
            fg: vec![tagged_instance(301)],
        },
    );
    let tags: Vec<u32> = cache.concat_all().iter().map(|i| i.fg_rgba).collect();
    assert_eq!(tags, vec![100, 200, 300, 101, 201, 301]);
}

/// `RowCache::resize` to a different row count drops every existing
/// entry (task0003 D3: resize is one of the "full cache drop"
/// triggers).
#[test]
fn row_cache_resize_to_different_count_drops_existing_entries() {
    let mut cache = RowCache::default();
    cache.resize(2);
    cache.set(
        0,
        RowInstances {
            bg: vec![tagged_instance(1)],
            fg: vec![],
        },
    );
    cache.resize(3);
    assert!(
        cache.concat_all().is_empty(),
        "resize to a new row count must drop stale entries"
    );
}

/// `RowCache::resize` to the SAME row count is a no-op — existing
/// entries survive. This is what makes "no dirty rows" a true
/// full-cache-reuse frame rather than an accidental full rebuild.
#[test]
fn row_cache_resize_to_same_count_preserves_existing_entries() {
    let mut cache = RowCache::default();
    cache.resize(2);
    cache.set(
        0,
        RowInstances {
            bg: vec![tagged_instance(1)],
            fg: vec![],
        },
    );
    cache.resize(2);
    let tags: Vec<u32> = cache.concat_all().iter().map(|i| i.fg_rgba).collect();
    assert_eq!(tags, vec![1]);
}

// ── task0003 AC-1/AC-2/AC-3: row-cache equivalence & rebuild counting ──

/// AC-1 (SPEC TS-4): after an initial full-grid rebuild, mutating a
/// single row and rebuilding only that row (the "write a character"
/// scenario) reproduces exactly the same instance sequence a
/// from-scratch full rebuild of the new overall state would produce.
#[test]
fn row_cache_equivalence_after_single_row_write() {
    let mut builder = instance_builder();
    let m = metrics();

    let frame1 = vec![
        ascii_cell(0, 0, "A"),
        ascii_cell(1, 0, "B"),
        ascii_cell(0, 1, "C"),
        ascii_cell(1, 1, "D"),
        ascii_cell(0, 2, "E"),
        ascii_cell(1, 2, "F"),
    ];
    let (instances1, rebuilt1) = builder.rebuild_and_collect(&[0, 1, 2], &frame1, m, 3);
    assert_eq!(rebuilt1, 3, "first frame rebuilds every row");
    assert_eq!(instances1, builder.build_instances(&frame1, m));

    // Frame 2: only row 1 changes ("C" -> "X"); rows 0/2 are clean and
    // must be served from cache without rebuilding.
    let row1_only = vec![ascii_cell(0, 1, "X"), ascii_cell(1, 1, "D")];
    let (instances2, rebuilt2) = builder.rebuild_and_collect(&[1], &row1_only, m, 3);
    assert_eq!(
        rebuilt2, 1,
        "AC-3: a single-row write rebuilds exactly one row"
    );

    let frame2_full = vec![
        ascii_cell(0, 0, "A"),
        ascii_cell(1, 0, "B"),
        ascii_cell(0, 1, "X"),
        ascii_cell(1, 1, "D"),
        ascii_cell(0, 2, "E"),
        ascii_cell(1, 2, "F"),
    ];
    // Ground truth computed against the SAME builder (same glyph
    // cache) so atlas allocation order for the one newly-seen glyph
    // ('X') is identical regardless of which path requested it first.
    let ground_truth = builder.build_instances(&frame2_full, m);
    assert_eq!(instances2, ground_truth);
}

/// AC-3: a stable frame (empty dirty set) rebuilds zero rows and
/// reuses the entire cache — the instance sequence is unchanged.
#[test]
fn row_cache_stable_frame_rebuilds_zero_rows_and_reuses_cache() {
    let mut builder = instance_builder();
    let m = metrics();
    let frame = vec![ascii_cell(0, 0, "A"), ascii_cell(0, 1, "B")];
    let (instances1, rebuilt1) = builder.rebuild_and_collect(&[0, 1], &frame, m, 2);
    assert_eq!(rebuilt1, 2);

    let (instances2, rebuilt2) = builder.rebuild_and_collect(&[], &[], m, 2);
    assert_eq!(rebuilt2, 0, "AC-3: empty dirty set rebuilds zero rows");
    assert_eq!(
        instances2, instances1,
        "an empty dirty set must reuse every cached row unchanged"
    );
}

/// AC-2 (invalidation matrix, consumption side): whatever subset of
/// rows the caller marks dirty — a single row (selection/hover-style),
/// a scattered pair (two independent highlight changes), or every row
/// (scroll/resize/font/theme-style full invalidation) — rebuilding
/// exactly that subset and reusing the rest still reproduces a
/// from-scratch full rebuild of the resulting state. Dirty-set
/// *semantics* (which trigger maps to which subset) is task0002's
/// concern (consumed as-is here); this test covers the row cache's
/// handling of an arbitrary dirty-row shape.
#[test]
fn row_cache_equivalence_holds_for_various_dirty_row_shapes() {
    let base = vec![
        ascii_cell(0, 0, "A"),
        ascii_cell(0, 1, "B"),
        ascii_cell(0, 2, "C"),
        ascii_cell(0, 3, "D"),
    ];
    let scenarios: [(&[u16], Vec<CellInput>); 3] = [
        // Single row dirty (e.g. a selection/hover change on row 2).
        (&[2], vec![ascii_cell(0, 2, "Z")]),
        // Scattered rows dirty (e.g. two independent highlight
        // changes on rows 0 and 3).
        (&[0, 3], vec![ascii_cell(0, 0, "Y"), ascii_cell(0, 3, "W")]),
        // Every row dirty (scroll / resize / font-or-theme-change
        // style full invalidation).
        (
            &[0, 1, 2, 3],
            vec![
                ascii_cell(0, 0, "P"),
                ascii_cell(0, 1, "Q"),
                ascii_cell(0, 2, "R"),
                ascii_cell(0, 3, "S"),
            ],
        ),
    ];
    for (dirty_rows, mutated_cells) in scenarios {
        let mut builder = instance_builder();
        let m = metrics();
        let (_, rebuilt_initial) = builder.rebuild_and_collect(&[0, 1, 2, 3], &base, m, 4);
        assert_eq!(rebuilt_initial, 4);

        let (partial, rebuilt) = builder.rebuild_and_collect(dirty_rows, &mutated_cells, m, 4);
        assert_eq!(rebuilt, dirty_rows.len());

        // Ground truth: the full grid with exactly `mutated_cells`
        // overlaid on `base` at the same (row, col).
        let mut full = base.clone();
        for mutated in &mutated_cells {
            if let Some(existing) = full
                .iter_mut()
                .find(|c| c.row == mutated.row && c.col == mutated.col)
            {
                *existing = mutated.clone();
            }
        }
        let ground_truth = builder.build_instances(&full, m);
        assert_eq!(
            partial, ground_truth,
            "dirty rows {dirty_rows:?} must reproduce a full rebuild"
        );
    }
}

// ── task0006: RowCache::rotate_for_scroll_event (pure) ──────────────

/// Per-row pixel height used by the rotation tests below (arbitrary;
/// distinct from [`metrics`]'s `cell_h` so these tests are visibly
/// independent of it).
const ROTATE_TEST_CELL_H: f32 = 20.0;

/// A synthetic instance carrying both an identity tag (`fg_rgba`) and
/// an explicit Y position, so rotation tests can assert on content
/// identity AND on the Y-translation `rotate_for_scroll_event` must
/// apply: a cached instance's `cell_xy` is baked for the screen row
/// it was BUILT at, so moving it to a different cache slot without
/// also translating its Y coordinate would paint it at its OLD row's
/// pixel position (the bug this task's first implementation attempt
/// hit — see the equivalence regression tests further below).
fn tagged_instance_at(tag: u32, y: f32) -> CellInstance {
    CellInstance {
        cell_xy: [0.0, y],
        cell_wh: [0.0, 0.0],
        atlas_uv: [0.0, 0.0, 0.0, 0.0],
        fg_rgba: tag,
        bg_rgba: 0,
        page: PAGE_SOLID,
        flags: 0,
    }
}

/// AC-2: rotate-by-1 shifts every cached row toward index 0 by one
/// position, translates each kept instance's Y so it paints at its
/// NEW row's pixel position, and empties the vacated bottom slot.
#[test]
fn row_cache_rotate_for_scroll_event_rotates_by_one() {
    let mut cache = RowCache::default();
    cache.resize(3);
    cache.set(
        0,
        RowInstances {
            bg: vec![tagged_instance_at(1, 0.0 * ROTATE_TEST_CELL_H)],
            fg: vec![],
        },
    );
    cache.set(
        1,
        RowInstances {
            bg: vec![tagged_instance_at(2, 1.0 * ROTATE_TEST_CELL_H)],
            fg: vec![],
        },
    );
    cache.set(
        2,
        RowInstances {
            bg: vec![tagged_instance_at(3, 2.0 * ROTATE_TEST_CELL_H)],
            fg: vec![],
        },
    );

    cache.rotate_for_scroll_event(SCROLL_DIRECTION_UP, 1, ROTATE_TEST_CELL_H);

    let row0 = cache.rows[0].as_ref().unwrap();
    assert_eq!(row0.bg[0].fg_rgba, 2, "row0 now holds what was row1");
    assert_eq!(
        row0.bg[0].cell_xy[1],
        0.0 * ROTATE_TEST_CELL_H,
        "moved content must paint at its NEW row's Y, not its old one"
    );
    let row1 = cache.rows[1].as_ref().unwrap();
    assert_eq!(row1.bg[0].fg_rgba, 3, "row1 now holds what was row2");
    assert_eq!(row1.bg[0].cell_xy[1], 1.0 * ROTATE_TEST_CELL_H);
    assert!(
        cache.rows[2].is_none(),
        "vacated bottom slot must be None (must-rebuild)"
    );
}

/// AC-2: an accumulated count > 1 rotates by the full accumulated
/// amount in one call (mirrors several lines emitted between two
/// rendered frames — AC-3's scenario), Y-translating by
/// `count * cell_h`.
#[test]
fn row_cache_rotate_for_scroll_event_rotates_by_accumulated_count() {
    let mut cache = RowCache::default();
    cache.resize(5);
    for i in 0..5u32 {
        cache.set(
            i as u16,
            RowInstances {
                bg: vec![tagged_instance_at(i, i as f32 * ROTATE_TEST_CELL_H)],
                fg: vec![],
            },
        );
    }

    cache.rotate_for_scroll_event(SCROLL_DIRECTION_UP, 3, ROTATE_TEST_CELL_H);

    let row0 = cache.rows[0].as_ref().unwrap();
    assert_eq!(row0.bg[0].fg_rgba, 3);
    assert_eq!(row0.bg[0].cell_xy[1], 0.0 * ROTATE_TEST_CELL_H);
    let row1 = cache.rows[1].as_ref().unwrap();
    assert_eq!(row1.bg[0].fg_rgba, 4);
    assert_eq!(row1.bg[0].cell_xy[1], 1.0 * ROTATE_TEST_CELL_H);
    assert!(cache.rows[2].is_none());
    assert!(cache.rows[3].is_none());
    assert!(cache.rows[4].is_none());
}

/// AC-2: a count that reaches/exceeds the row count drops the whole
/// cache rather than rotating out of bounds.
#[test]
fn row_cache_rotate_for_scroll_event_count_ge_row_count_drops_all() {
    let mut cache = RowCache::default();
    cache.resize(3);
    for i in 0..3u16 {
        cache.set(
            i,
            RowInstances {
                bg: vec![tagged_instance(i as u32)],
                fg: vec![],
            },
        );
    }

    cache.rotate_for_scroll_event(SCROLL_DIRECTION_UP, 3, ROTATE_TEST_CELL_H);

    assert!(
        cache.concat_all().is_empty(),
        "count >= row_count must drop every cached entry"
    );
}

/// AC-2: an unrecognized direction code degenerates to a full cache
/// drop. term_core does not currently emit anything but the "Up"
/// encoding — this exercises the defensive branch against a
/// future/unknown value rather than trusting it means "Up".
#[test]
fn row_cache_rotate_for_scroll_event_unknown_direction_drops_all() {
    let mut cache = RowCache::default();
    cache.resize(3);
    for i in 0..3u16 {
        cache.set(
            i,
            RowInstances {
                bg: vec![tagged_instance(i as u32)],
                fg: vec![],
            },
        );
    }

    cache.rotate_for_scroll_event(SCROLL_DIRECTION_UP + 1, 1, ROTATE_TEST_CELL_H);

    assert!(
        cache.concat_all().is_empty(),
        "an unrecognized direction must drop every cached entry"
    );
}

/// `count == 0` (no pending scroll event) is a no-op — every cached
/// entry survives untouched (content AND position).
#[test]
fn row_cache_rotate_for_scroll_event_zero_count_is_noop() {
    let mut cache = RowCache::default();
    cache.resize(2);
    cache.set(
        0,
        RowInstances {
            bg: vec![tagged_instance_at(9, 3.0 * ROTATE_TEST_CELL_H)],
            fg: vec![],
        },
    );

    cache.rotate_for_scroll_event(SCROLL_DIRECTION_UP, 0, ROTATE_TEST_CELL_H);

    let row0 = cache.rows[0].as_ref().unwrap();
    assert_eq!(row0.bg[0].fg_rgba, 9);
    assert_eq!(row0.bg[0].cell_xy[1], 3.0 * ROTATE_TEST_CELL_H);
}

// ── task0006: row cache tracks term_core's live-tail scroll ─────────
// regression (review round-2 critical finding 779c9130c103c55b): the
// per-row cache must rotate to track term_core's full-screen
// count==1 scroll optimization (`ring_buffer::scroll_up_internal`),
// not just rebuild whatever rows the core names dirty — every other
// row's on-screen position shifted too.

/// Ground-truth full-grid `CellInput`s for `core`'s current viewport
/// state, using a fixed default theme/selection/hover/fold — the
/// input the row-cache path must reproduce exactly after any given
/// sequence of scroll/dirty operations.
fn full_grid_inputs(core: &term_core::terminal_core::TerminalCore) -> Vec<CellInput> {
    crate::render::collect_cell_inputs(
        core,
        &crate::render::theme::Theme::default(),
        None,
        crate::settings::AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        None,
    )
}

/// AC-1: fill the viewport, clear dirty state, emit one line that
/// causes a single-line full-screen scroll, render via the cache
/// path — the concatenated instances match a from-scratch full
/// rebuild of the post-scroll state, byte-exact.
#[test]
fn row_cache_scroll_regression_single_line_scroll_matches_full_rebuild() {
    let mut core = term_core::terminal_core::TerminalCore::new(4, 3, 100);
    core.process_pty_data(b"AAAA\r\nBBBB\r\nCCCC");
    core.clear_dirty();

    let mut builder = instance_builder();
    let m = metrics();
    let row_count = core.rows();

    // Initial cache build: matches the state right after a full
    // render (every row present in the cache).
    let all_rows: Vec<u16> = (0..row_count).collect();
    let initial_inputs = full_grid_inputs(&core);
    let (initial_instances, rebuilt) =
        builder.rebuild_and_collect(&all_rows, &initial_inputs, m, row_count);
    assert_eq!(rebuilt, row_count as usize);
    assert_eq!(
        initial_instances,
        builder.build_instances(&initial_inputs, m)
    );

    // Trigger a single-line full-screen scroll: the cursor sits at
    // the bottom row after the writes above, so a line feed rolls
    // the viewport (term_core::terminal_core::TerminalCore::line_feed
    // -> scroll_up_internal(1)).
    core.process_pty_data(b"\r\nDDDD");
    assert_eq!(
        core.get_scroll_event_direction(),
        1,
        "expected an Up scroll event"
    );
    assert_eq!(core.get_scroll_event_count(), 1);

    // task0006 fix: rotate the cache to track the shift BEFORE
    // rebuilding whatever rows the core reports dirty, then clear
    // the event exactly once.
    builder.apply_scroll_event(
        core.get_scroll_event_direction(),
        core.get_scroll_event_count(),
        m.cell_h,
    );
    core.clear_scroll_event();

    let dirty_rows = core.get_dirty_rows();
    let dirty_cells = crate::render::collect_cell_inputs(
        &core,
        &crate::render::theme::Theme::default(),
        None,
        crate::settings::AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&dirty_rows),
    );
    let (cached_instances, _) =
        builder.rebuild_and_collect(&dirty_rows, &dirty_cells, m, row_count);

    let ground_truth = builder.build_instances(&full_grid_inputs(&core), m);
    assert_eq!(cached_instances, ground_truth);
}

/// AC-3: several lines emitted between two rendered frames
/// (accumulated scroll count > 1, never consumed in between) still
/// produce a correct frame via the cache path once consumed.
#[test]
fn row_cache_scroll_regression_multi_scroll_between_frames_matches_full_rebuild() {
    let mut core = term_core::terminal_core::TerminalCore::new(4, 5, 100);
    core.process_pty_data(b"AAAA\r\nBBBB\r\nCCCC\r\nDDDD\r\nEEEE");
    core.clear_dirty();

    let mut builder = instance_builder();
    let m = metrics();
    let row_count = core.rows();
    let all_rows: Vec<u16> = (0..row_count).collect();
    let initial_inputs = full_grid_inputs(&core);
    let (_, rebuilt) = builder.rebuild_and_collect(&all_rows, &initial_inputs, m, row_count);
    assert_eq!(rebuilt, row_count as usize);

    // Three line feeds at the bottom row, none of them consumed as a
    // frame in between — the core accumulates a single ScrollEvent
    // with count == 3 (ring_buffer::scroll_up_internal's count==1
    // full-screen branch fires three separate times).
    core.process_pty_data(b"\r\nFFFF\r\nGGGG\r\nHHHH");
    assert_eq!(core.get_scroll_event_direction(), 1);
    assert_eq!(
        core.get_scroll_event_count(),
        3,
        "three separate single-line scrolls must accumulate to count == 3"
    );

    builder.apply_scroll_event(
        core.get_scroll_event_direction(),
        core.get_scroll_event_count(),
        m.cell_h,
    );
    core.clear_scroll_event();

    let dirty_rows = core.get_dirty_rows();
    let dirty_cells = crate::render::collect_cell_inputs(
        &core,
        &crate::render::theme::Theme::default(),
        None,
        crate::settings::AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&dirty_rows),
    );
    let (cached_instances, _) =
        builder.rebuild_and_collect(&dirty_rows, &dirty_cells, m, row_count);

    let ground_truth = builder.build_instances(&full_grid_inputs(&core), m);
    assert_eq!(cached_instances, ground_truth);
}

/// AC-4: the scroll event is cleared after consumption — a second
/// frame with no new PTY output rotates by zero (no-op) and rebuilds
/// only its own (empty) dirty set, reusing every cached row from the
/// first frame's rotation + rebuild.
#[test]
fn row_cache_scroll_event_cleared_after_consumption_second_frame_rotates_by_zero() {
    let mut core = term_core::terminal_core::TerminalCore::new(4, 3, 100);
    core.process_pty_data(b"AAAA\r\nBBBB\r\nCCCC");
    core.clear_dirty();

    let mut builder = instance_builder();
    let m = metrics();
    let row_count = core.rows();
    let all_rows: Vec<u16> = (0..row_count).collect();
    let initial_inputs = full_grid_inputs(&core);
    builder.rebuild_and_collect(&all_rows, &initial_inputs, m, row_count);

    // Frame 1: one scroll, consumed.
    core.process_pty_data(b"\r\nDDDD");
    assert_eq!(core.get_scroll_event_count(), 1);
    builder.apply_scroll_event(
        core.get_scroll_event_direction(),
        core.get_scroll_event_count(),
        m.cell_h,
    );
    core.clear_scroll_event();
    let dirty_rows = core.get_dirty_rows();
    let dirty_cells = crate::render::collect_cell_inputs(
        &core,
        &crate::render::theme::Theme::default(),
        None,
        crate::settings::AmbiguousWidthMode::Narrow,
        None,
        0,
        None,
        Some(&dirty_rows),
    );
    builder.rebuild_and_collect(&dirty_rows, &dirty_cells, m, row_count);
    core.clear_dirty();

    // Frame 2: no new PTY output. The scroll event must already be
    // clear — a stale nonzero count here would wrongly rotate the
    // cache again against content that never moved.
    assert_eq!(
        core.get_scroll_event_count(),
        0,
        "scroll event must be cleared after the first frame consumed it"
    );
    builder.apply_scroll_event(
        core.get_scroll_event_direction(),
        core.get_scroll_event_count(),
        m.cell_h,
    );
    let dirty_rows2 = core.get_dirty_rows();
    assert!(
        dirty_rows2.is_empty(),
        "no new output => nothing dirty on the second frame"
    );
    let (instances2, rebuilt2) = builder.rebuild_and_collect(&dirty_rows2, &[], m, row_count);
    assert_eq!(rebuilt2, 0, "AC-4: zero-count rotation rebuilds zero rows");

    let ground_truth = builder.build_instances(&full_grid_inputs(&core), m);
    assert_eq!(instances2, ground_truth);
}
