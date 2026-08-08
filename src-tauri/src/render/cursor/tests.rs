use super::*;

// ── task0006 AC-3: block-cursor origin carries no sidebar term ─────

/// Regression guard for the right-edge placement update:
/// `draw_block_cursor`'s `FontMetrics` construction must not read any
/// mux-sidebar inset into `left_pad` — the persistent sidebar reserves
/// grid WIDTH only, so the block cursor's x-origin is identical with
/// and without it.
#[test]
fn draw_block_cursor_left_pad_has_no_sidebar_term() {
    let src = include_str!("../cursor.rs");
    let production_src = src.split("\nmod tests {").next().unwrap_or(src);
    for needle in [
        "sidebar_inset",
        "mux_sidebar_x_inset",
        "mux_sidebar_grid_inset",
    ] {
        assert!(
            !production_src.contains(needle),
            "cursor.rs's origin math must contain no sidebar term (AC-3): \
             found `{needle}` — the block cursor's x-origin must be \
             identical with and without the persistent sidebar"
        );
    }
}

fn fake_metrics() -> FontMetrics {
    FontMetrics {
        cell_w: 10.0,
        cell_h: 20.0,
        left_pad: 4.0,
        top_pad: 4.0,
    }
}

// ── preedit_cell_width ──────────────────────────────────────────

#[test]
fn cell_width_empty_is_zero() {
    assert_eq!(preedit_cell_width(""), 0);
}

#[test]
fn cell_width_ascii_is_len() {
    assert_eq!(preedit_cell_width("hello"), 5);
}

#[test]
fn cell_width_cjk_is_double() {
    // U+4E00 is wide → 2 cells.
    assert_eq!(preedit_cell_width("一"), 2);
    assert_eq!(preedit_cell_width("一二"), 4);
}

// ── preedit_underline_runs ──────────────────────────────────────

#[test]
fn runs_empty_for_zero_width() {
    let runs = preedit_underline_runs(Anchor { row: 0, col: 0 }, 0, 80, fake_metrics());
    assert!(runs.is_empty());
}

#[test]
fn runs_single_row_for_short_text() {
    let runs = preedit_underline_runs(Anchor { row: 2, col: 5 }, 3, 80, fake_metrics());
    assert_eq!(runs.len(), 1);
    let r = runs[0];
    assert_eq!(r.x_start, 4.0 + 5.0 * 10.0);
    assert_eq!(r.x_end, 4.0 + 5.0 * 10.0 + 3.0 * 10.0);
    assert_eq!(r.y, 4.0 + 2.0 * 20.0 + 20.0 - 1.0);
}

#[test]
fn runs_wrap_when_exceeding_row_width() {
    // anchor at col 78 of 80, width 5 → first run covers 2 cells
    // (cols 78,79), wraps to row+1 for the remaining 3 cells.
    let runs = preedit_underline_runs(Anchor { row: 0, col: 78 }, 5, 80, fake_metrics());
    assert_eq!(runs.len(), 2);
    // First run starts at col 78.
    assert_eq!(runs[0].x_start, 4.0 + 78.0 * 10.0);
    assert_eq!(runs[0].x_end, 4.0 + 80.0 * 10.0);
    // Second run starts at col 0 of next row.
    assert_eq!(runs[1].x_start, 4.0);
    assert_eq!(runs[1].x_end, 4.0 + 3.0 * 10.0);
    // y bumped by one row.
    assert!(runs[1].y > runs[0].y);
    assert_eq!(runs[1].y - runs[0].y, 20.0);
}

#[test]
fn runs_wrap_multiple_full_rows() {
    // 200 cells starting at col 0 of an 80-col grid → 3 runs.
    let runs = preedit_underline_runs(Anchor { row: 0, col: 0 }, 200, 80, fake_metrics());
    assert_eq!(runs.len(), 3);
    // Sanity: total covered cells = 200.
    let total_cells: f32 = runs.iter().map(|r| (r.x_end - r.x_start) / 10.0).sum();
    assert_eq!(total_cells.round() as u16, 200);
}

#[test]
fn runs_clamp_stale_anchor_inside_grid() {
    // Anchor.col 100 on an 80-col grid: a previous resize shrunk
    // the grid but the preedit anchor was captured before. We must
    // not panic and the first run must be inside [0, cols).
    let runs = preedit_underline_runs(Anchor { row: 0, col: 100 }, 3, 80, fake_metrics());
    assert!(!runs.is_empty());
    assert!(runs[0].x_start < 4.0 + 80.0 * 10.0);
}

#[test]
fn runs_zero_cols_returns_empty() {
    // Defensive: a 0-column grid means no rendering surface.
    let runs = preedit_underline_runs(Anchor { row: 0, col: 0 }, 5, 0, fake_metrics());
    assert!(runs.is_empty());
}

// ── block_cursor_rect (AC-2, AC-4) ───────────────────────────────

#[test]
fn block_cursor_rect_normal_one_cell_glyph() {
    let rect = block_cursor_rect(3, 2, 1, 80, 24, fake_metrics());
    assert_eq!(rect.min.x, 4.0 + 3.0 * 10.0);
    assert_eq!(rect.min.y, 4.0 + 2.0 * 20.0);
    assert_eq!(rect.width(), 10.0);
    assert_eq!(rect.height(), 20.0);
}

#[test]
fn block_cursor_rect_wide_glyph_covers_two_cells() {
    // A CJK / emoji glyph under the cursor reports width_cells = 2;
    // the rect must cover both cells' footprint, not just the first.
    let rect = block_cursor_rect(3, 2, 2, 80, 24, fake_metrics());
    assert_eq!(rect.min.x, 4.0 + 3.0 * 10.0);
    assert_eq!(rect.width(), 20.0);
    assert_eq!(rect.height(), 20.0);
}

#[test]
fn block_cursor_rect_empty_cell_is_rect_only_one_cell_wide() {
    // An empty/blank cell under the cursor still gets a normal
    // 1-cell rect (visible_width floors to 1); the "no glyph
    // artifact" half of AC-2 is covered by
    // `cursor_glyph_paintable` below, not the rect geometry.
    let rect = block_cursor_rect(0, 0, 1, 80, 24, fake_metrics());
    assert_eq!(rect.width(), 10.0);
    assert_eq!(rect.height(), 20.0);
}

#[test]
fn block_cursor_rect_last_column_stays_in_bounds() {
    let cols = 80u16;
    let rect = block_cursor_rect(cols - 1, 0, 1, cols, 24, fake_metrics());
    // Right edge must land exactly on the grid's right boundary,
    // never past it.
    assert_eq!(rect.max.x, 4.0 + cols as f32 * 10.0);
}

#[test]
fn block_cursor_rect_last_row_stays_in_bounds() {
    let rows = 24u16;
    let rect = block_cursor_rect(0, rows - 1, 1, 80, rows, fake_metrics());
    assert_eq!(rect.max.y, 4.0 + rows as f32 * 20.0);
}

#[test]
fn block_cursor_rect_wide_glyph_at_last_column_clamps_width() {
    // Defensive: a wide glyph reported at the very last column
    // (should not happen in practice — term_core never places a
    // wide glyph's leading half there) must still clamp its rect
    // to the grid's right edge instead of overflowing it.
    let cols = 80u16;
    let rect = block_cursor_rect(cols - 1, 0, 2, cols, 24, fake_metrics());
    assert_eq!(rect.max.x, 4.0 + cols as f32 * 10.0);
}

#[test]
fn block_cursor_rect_clamps_out_of_range_col_and_row() {
    // Defensive: a stale col/row past the current grid size (e.g.
    // after a shrink-resize) must clamp inside bounds rather than
    // producing an off-canvas rect.
    let rect = block_cursor_rect(200, 200, 1, 80, 24, fake_metrics());
    assert_eq!(rect.min.x, 4.0 + 79.0 * 10.0);
    assert_eq!(rect.min.y, 4.0 + 23.0 * 20.0);
}

// ── resolve_cursor_glyph_col (task0005 AC-5) ──────────────────────

#[test]
fn resolve_cursor_glyph_col_normal_cell_unchanged() {
    assert_eq!(resolve_cursor_glyph_col(5, 1), 5);
}

#[test]
fn resolve_cursor_glyph_col_wide_leading_unchanged() {
    assert_eq!(resolve_cursor_glyph_col(5, 2), 5);
}

#[test]
fn resolve_cursor_glyph_col_trailing_continuation_resolves_to_leading() {
    // Cursor lands on the width-0 right half of a wide glyph at
    // column 6; the leading half is column 5.
    assert_eq!(resolve_cursor_glyph_col(6, 0), 5);
}

#[test]
fn resolve_cursor_glyph_col_trailing_at_col_zero_is_defensive_noop() {
    // A width-0 cell at column 0 should not occur in practice (a wide
    // glyph's trailing half always has a leading half at col - 1),
    // but must not underflow.
    assert_eq!(resolve_cursor_glyph_col(0, 0), 0);
}

#[test]
fn block_cursor_rect_from_resolved_trailing_col_covers_full_wide_footprint() {
    // AC-5: cursor sits on the trailing half (col 6, width 0);
    // resolving to the leading column (5) and drawing a 2-cell rect
    // anchored there covers the glyph's full footprint instead of an
    // empty 1-cell box at col 6.
    let leading_col = resolve_cursor_glyph_col(6, 0);
    assert_eq!(leading_col, 5);
    let rect = block_cursor_rect(leading_col, 2, 2, 80, 24, fake_metrics());
    assert_eq!(rect.min.x, 4.0 + 5.0 * 10.0);
    assert_eq!(rect.width(), 20.0);
}

#[test]
fn block_cursor_rect_wide_glyph_at_right_edge_via_resolved_col_clamps() {
    // AC-5: the right-edge clamp still applies when the wide glyph's
    // leading column is the second-to-last column (its trailing half
    // occupies the very last column).
    let cols = 80u16;
    let leading_col = resolve_cursor_glyph_col(cols - 1, 0);
    assert_eq!(leading_col, cols - 2);
    let rect = block_cursor_rect(leading_col, 0, 2, cols, 24, fake_metrics());
    assert_eq!(rect.max.x, 4.0 + cols as f32 * 10.0);
}

// ── cursor_glyph_paintable (AC-2: empty cell → no glyph artifact) ─

#[test]
fn cursor_glyph_paintable_false_for_empty_string() {
    assert!(!cursor_glyph_paintable(""));
}

#[test]
fn cursor_glyph_paintable_false_for_whitespace_only() {
    assert!(!cursor_glyph_paintable(" "));
}

#[test]
fn cursor_glyph_paintable_true_for_ascii() {
    assert!(cursor_glyph_paintable("A"));
}

#[test]
fn cursor_glyph_paintable_true_for_wide_and_emoji_glyphs() {
    assert!(cursor_glyph_paintable("一"));
    assert!(cursor_glyph_paintable("😀"));
}

// ── cursor_screen_row (AC-3, AC-5) ────────────────────────────────

#[test]
fn cursor_screen_row_scrolled_back_suppresses_cursor() {
    // AC-3: any non-zero scroll offset suppresses the cursor,
    // regardless of fold layout.
    assert_eq!(cursor_screen_row(0, 5, 1, None), None);
}

#[test]
fn cursor_screen_row_no_fold_layout_is_identity() {
    // Without a fold layout the cursor's viewport row is already
    // the on-screen row.
    assert_eq!(cursor_screen_row(10, 5, 0, None), Some(5));
}

#[test]
fn cursor_screen_row_fold_layout_maps_row_before_collapsed_region() {
    // Mirrors `collect_cell_inputs_fold_layout_maps_rows_and_skips_summary`
    // in `render/mod.rs`: region over actual lines 1..3 collapsed,
    // 5-row viewport, nothing scrolled off (scrollback_len = 0).
    let mut fm = crate::fold::FoldManager::new();
    fm.register_osc133_region(1, 3, "cmd".to_string(), Some(0));
    fm.toggle_fold(1);
    let layout = fm.build_layout(0, 5, 0);

    // Cursor at actual row 0 (before the collapsed region) maps to
    // screen row 0 unchanged.
    assert_eq!(cursor_screen_row(0, 0, 0, Some(&layout)), Some(0));
}

#[test]
fn cursor_screen_row_fold_layout_maps_row_after_collapsed_region() {
    let mut fm = crate::fold::FoldManager::new();
    fm.register_osc133_region(1, 3, "cmd".to_string(), Some(0));
    fm.toggle_fold(1);
    let layout = fm.build_layout(0, 5, 0);

    // Cursor at actual row 3 (L3, past the collapsed body) maps to
    // screen row 2 — the summary row replaced the two hidden rows.
    assert_eq!(cursor_screen_row(0, 3, 0, Some(&layout)), Some(2));
}

#[test]
fn cursor_screen_row_suppressed_when_hidden_by_fold() {
    // AC-5 (second half): a cursor row landing inside the collapsed
    // region's body must be suppressed rather than drawn at the
    // wrong (summary) row.
    let mut fm = crate::fold::FoldManager::new();
    fm.register_osc133_region(1, 3, "cmd".to_string(), Some(0));
    fm.toggle_fold(1);
    let layout = fm.build_layout(0, 5, 0);

    assert_eq!(cursor_screen_row(0, 1, 0, Some(&layout)), None);
    assert_eq!(cursor_screen_row(0, 2, 0, Some(&layout)), None);
}

// ── resolve_cursor_color (task0003 AC-1, AC-2, AC-3, AC-5) ────────

#[test]
fn resolve_cursor_color_reads_theme_cursor_fg_not_theme_fg() {
    // AC-1: the cursor overlay's color source is `Theme.cursor_fg`,
    // distinct from the theme's regular text foreground.
    let mut theme = Theme::default();
    theme.fg = Rgb(0x11, 0x22, 0x33);
    theme.cursor_fg = Rgb(0x44, 0x55, 0x66);
    assert_eq!(resolve_cursor_color(&theme), Rgb(0x44, 0x55, 0x66));
}

#[test]
fn resolve_cursor_color_ignores_sgr_pen_fg() {
    // AC-5: `TerminalCore::get_cursor_fg()` (the SGR pen foreground —
    // confusingly named inside term_core, but unrelated to the
    // terminal cursor's own paint color) must not influence the
    // resolved cursor color. Mutate the SGR pen via
    // `set_cursor_fg` (what an SGR sequence like `\e[38;2;255;0;0m`
    // ultimately calls) and confirm the resolved color is
    // unaffected.
    let mut core = TerminalCore::new(80, 24, 0);
    core.set_cursor_fg(2, 0xff, 0x00, 0x00); // SGR pen -> truecolor red
    let mut theme = Theme::default();
    theme.cursor_fg = Rgb(0x0a, 0x14, 0x1e);
    assert_eq!(resolve_cursor_color(&theme), Rgb(0x0a, 0x14, 0x1e));
    // Sanity: the SGR pen mutation genuinely took effect, so this
    // isn't a vacuous test — the helper's signature has no `core`
    // parameter at all, structurally guaranteeing independence.
    assert_eq!(core.get_cursor_fg(), 0x02_ff_00_00);
}

// ── block-cursor-glyph-font task0001 ──────────────────────────────
//
// AC-1/AC-4/AC-6 (glyph resolution identity, wide-glyph single
// lookup, cache reuse) are unit-tested against `resolve_overlay_glyph`
// directly in `render::font`'s own test module — that function has no
// `egui` dependency, so the resolution logic is exercised there
// without needing a `Painter`. The tests below cover what's specific
// to this module: AC-3 (the tint plumbing) and AC-6's other half
// (the `egui::TextureHandle` reuse cache), plus AC-5 (no reference to
// egui's built-in monospace font selector left on the covered-glyph
// path).

use crate::render::font::{
    AtlasFormat, FallbackChain, FontId as GlyphFontId, GlyphBitmap, GlyphCache, GlyphKey,
    GlyphRasterizer, ShapedGlyph, extract_overlay_glyph_pixels, resolve_overlay_glyph,
    resolve_overlay_glyph_meta, test_hooks,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use term_core::cell::STYLE_REVERSE;

/// AC-5: no reference to egui's built-in monospace `FontId`
/// constructor may remain in this file — the overlay glyph now
/// comes from `resolve_overlay_glyph`'s shared swash/atlas raster
/// instead. The needle is assembled from two fragments at runtime so
/// this assertion doesn't match its own source line.
#[test]
fn draw_block_cursor_no_longer_uses_egui_monospace_font() {
    let source = include_str!("../cursor.rs");
    let type_name = "FontId";
    let ctor = "monospace";
    let needle = format!("{type_name}::{ctor}");
    assert!(
        !source.contains(&needle),
        "cursor.rs must not reference egui's built-in monospace FontId \
         constructor after block-cursor-glyph-font task0001"
    );
}

/// AC-3: the overlay glyph's tint is
/// `resolve_cell_style_from_packed(...).bg` for the covered cell —
/// `draw_block_cursor` computes `style` via this exact call and
/// passes `style.bg` as `paint_overlay_glyph`'s `tint` argument (see
/// its body). Reverse video swaps fg/bg at the packed level, so the
/// resolved tint under reverse must equal the NON-reversed
/// resolution's `fg` — proving the overlay would actually track
/// reverse video, not just echo a static color.
#[test]
fn overlay_glyph_tint_tracks_reverse_video_via_resolve_cell_style_from_packed() {
    let theme = Theme::default();
    let packed_fg = 0x01_00_00_01u32; // indexed, palette index irrelevant here
    let packed_bg = 0x01_00_00_04u32;
    let plain =
        super::super::resolve_cell_style_from_packed(&theme, packed_fg, packed_bg, 0, false);
    let reversed = super::super::resolve_cell_style_from_packed(
        &theme,
        packed_fg,
        packed_bg,
        STYLE_REVERSE,
        false,
    );
    assert_eq!(reversed.bg, plain.fg);
}

/// AC-3 (selection half): selecting the covered cell swaps fg/bg on
/// top of any reverse already in effect — the overlay tint must
/// follow, since it reads the same `style.bg` selection already
/// swapped.
#[test]
fn overlay_glyph_tint_tracks_selection_via_resolve_cell_style_from_packed() {
    let theme = Theme::default();
    let packed_fg = 0x01_00_00_01u32;
    let packed_bg = 0x01_00_00_04u32;
    let unselected =
        super::super::resolve_cell_style_from_packed(&theme, packed_fg, packed_bg, 0, false);
    let selected =
        super::super::resolve_cell_style_from_packed(&theme, packed_fg, packed_bg, 0, true);
    assert_eq!(selected.bg, unselected.fg);
}

/// Fake rasterizer for the tests below: always resolves to glyph id
/// 7, counts `raster` calls.
struct FakeRasterizer {
    calls: AtomicUsize,
}

impl GlyphRasterizer for FakeRasterizer {
    fn shape(&self, _cluster: &str, font: GlyphFontId, size_px: f32) -> Vec<ShapedGlyph> {
        vec![ShapedGlyph {
            font,
            glyph_id: 7,
            size_px,
        }]
    }
    fn raster(&self, _font: GlyphFontId, _glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some(GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: 4,
            height: 4,
            bearing: (0, 4),
            advance: 8.0,
            pixels: vec![0xFF; 16],
        })
    }
}

/// AC-4: a wide glyph's cursor lands on the trailing half (col 6,
/// width 0, per `resolve_cursor_glyph_col`'s contract); resolving
/// the overlay glyph once for the leading column's character (what
/// `draw_block_cursor`'s single `resolve_overlay_glyph` call site
/// does) fires exactly one rasterize call.
#[test]
fn wide_glyph_overlay_lookup_fires_once_for_leading_column_char() {
    let leading_col = resolve_cursor_glyph_col(6, 0);
    assert_eq!(leading_col, 5);

    let rasterizer = FakeRasterizer {
        calls: AtomicUsize::new(0),
    };
    let fallback = FallbackChain::new(GlyphFontId(1), []);
    let mut cache = GlyphCache::new();
    let glyph = resolve_overlay_glyph(&rasterizer, &fallback, &mut cache, "一", 13.0, false)
        .expect("wide glyph must resolve");
    assert_eq!(glyph.width, 4);
    assert_eq!(rasterizer.calls.load(Ordering::SeqCst), 1);
}

/// AC-6: resolving the same glyph key's `egui::TextureHandle` twice
/// via [`get_or_create_overlay_texture`] must not create a second
/// texture — the "no new `egui::TextureHandle` per frame for a glyph
/// already in the shared cache" property (NFR1). `loader` is a
/// counting stub standing in for `egui::Context::load_texture`, so
/// this test drives the real caching logic without a live paint pass.
#[test]
fn get_or_create_overlay_texture_reuses_handle_for_same_key() {
    let ctx = egui::Context::default();
    let key = GlyphKey::new(GlyphFontId(1), 5, 13.0, 0.0);
    let calls = std::sync::Arc::new(AtomicUsize::new(0));

    let make_loader = {
        let ctx = ctx.clone();
        let calls = calls.clone();
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            ctx.load_texture(
                "test-overlay-glyph",
                egui::ColorImage::new([1, 1], egui::Color32::WHITE),
                egui::TextureOptions::LINEAR,
            )
        }
    };

    let first = get_or_create_overlay_texture(&ctx, key, make_loader.clone());
    let second = get_or_create_overlay_texture(&ctx, key, make_loader);
    // `TextureHandle` has no `Debug` impl, so `assert_eq!` can't be
    // used directly (it needs `Debug` for the failure message).
    assert!(first == second, "expected the same cached texture handle");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "second resolve of the same glyph key must reuse the cached texture"
    );
}

/// Distinct glyph keys must NOT share a texture — the cache is keyed
/// per-glyph, not a single shared slot.
#[test]
fn get_or_create_overlay_texture_creates_distinct_handles_for_distinct_keys() {
    let ctx = egui::Context::default();
    let key_a = GlyphKey::new(GlyphFontId(1), 5, 13.0, 0.0);
    let key_b = GlyphKey::new(GlyphFontId(1), 6, 13.0, 0.0);

    let loader = {
        let ctx = ctx.clone();
        move || {
            ctx.load_texture(
                "test-overlay-glyph",
                egui::ColorImage::new([1, 1], egui::Color32::WHITE),
                egui::TextureOptions::LINEAR,
            )
        }
    };

    let a = get_or_create_overlay_texture(&ctx, key_a, loader.clone());
    let b = get_or_create_overlay_texture(&ctx, key_b, loader);
    assert!(a != b, "distinct glyph keys must not share a texture");
}

// ── block-cursor-glyph-font task0002 rework (review round 1) ──────

// ── overlay_font_px (AC-1, r1-c2) ──────────────────────────────────

#[test]
fn overlay_font_px_applies_pixels_per_point_like_grid_pass() {
    // Mirrors `window_host::render`'s
    // `theme.font_size_px() * pixels_per_point.max(1.0)`.
    let font_size_pt = 13.0;
    let font_size_px = font_size_pt * crate::settings::PT_TO_PX; // theme.font_size_px()
    let grid_font_px_at_2x = font_size_px * 2.0f32.max(1.0);
    assert_eq!(overlay_font_px(font_size_pt, 2.0), grid_font_px_at_2x);
}

#[test]
fn overlay_font_px_clamps_sub_1x_scale_to_1() {
    // `pixels_per_point` below 1.0 (should not normally happen) is
    // clamped, matching the grid pass' `.max(1.0)` guard.
    let font_size_pt = 13.0;
    assert_eq!(
        overlay_font_px(font_size_pt, 0.5),
        overlay_font_px(font_size_pt, 1.0)
    );
}

/// AC-1: the overlay's `GlyphKey` at HiDPI scale matches what the grid
/// pass would build for the same cluster/size — proving
/// `pixels_per_point` genuinely propagates into the cache key (not
/// just into a display-only scale), closing r1-c2.
#[test]
fn overlay_glyph_key_matches_grid_key_at_hidpi_scale() {
    let rasterizer = FakeRasterizer {
        calls: AtomicUsize::new(0),
    };
    let fallback = FallbackChain::new(GlyphFontId(1), []);
    let mut cache = GlyphCache::new();
    let runtime_font_size_pt = 13.0;
    let scale = 2.0;
    let font_px = overlay_font_px(runtime_font_size_pt, scale);

    let meta = resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", font_px, false)
        .expect("must resolve");

    // Mirror the grid pass' own key derivation for the same scale
    // (`window_host::render` -> `CellMetrics::font_size_px` ->
    // `GridInstanceBuilder::glyph_instance`).
    let grid_font_size_px = runtime_font_size_pt * crate::settings::PT_TO_PX * scale;
    let grid_font = fallback.resolve_for_cluster(&rasterizer, "A").unwrap();
    let grid_shaped = rasterizer.shape("A", grid_font, grid_font_size_px);
    let grid_key = GlyphKey::new(grid_font, grid_shaped[0].glyph_id, grid_font_size_px, 0.0);

    assert_eq!(meta.key, grid_key);

    // Sanity: at 1x scale the key differs, proving scale genuinely
    // participates rather than being lost to size-bucket rounding.
    let font_px_1x = overlay_font_px(runtime_font_size_pt, 1.0);
    let mut cache_1x = GlyphCache::new();
    let meta_1x = resolve_overlay_glyph_meta(
        &rasterizer,
        &fallback,
        &mut cache_1x,
        "A",
        font_px_1x,
        false,
    )
    .unwrap();
    assert_ne!(meta.key, meta_1x.key);
}

// ── overlay_baseline_y (AC-2, r1-c1) ────────────────────────────────

#[test]
fn overlay_baseline_y_matches_grid_pass_formula() {
    // Known metrics mirroring `build_instances_split`: cell_h=20,
    // base_line_height=16 -> v_pad=2.0; base_ascent=13.0.
    let v_pad = crate::render::font::compute_v_pad(20.0, 16.0);
    let baseline = overlay_baseline_y(4.0, v_pad, 13.0);
    // Grid pass: `let baseline = y + v_pad + base_ascent;` with
    // y = cell top (4.0 here, mirroring `origin.y + row*cell_h`).
    assert_eq!(baseline, 4.0 + 2.0 + 13.0);
}

#[test]
fn overlay_baseline_y_no_pad_when_line_height_exceeds_cell() {
    // A font whose natural line height exceeds the cell height (small
    // cell, tall font) must not push the baseline UP past the cell
    // top — `compute_v_pad`'s `.max(0.0)` clamp means no extra offset.
    let v_pad = crate::render::font::compute_v_pad(10.0, 16.0);
    assert_eq!(v_pad, 0.0);
    assert_eq!(overlay_baseline_y(4.0, v_pad, 13.0), 4.0 + 13.0);
}

// ── r1-p1 / AC-4: pixel extraction skipped on texture cache hit ────

/// AC-4 / r1-p1: on a texture-cache HIT (the second consecutive paint
/// of the same glyph — the common case: the cursor sits on the same
/// glyph across most frames), `extract_region_rgba` must not run and
/// no `egui::ColorImage` is built. Drives the ACTUAL sequence
/// `draw_block_cursor` / `paint_overlay_glyph` run
/// (`resolve_overlay_glyph_meta` + `get_or_create_overlay_texture`
/// with a loader that only calls `extract_overlay_glyph_pixels` on
/// miss), counting real invocations via
/// `render::font::test_hooks` rather than asserting on plumbing in
/// isolation.
#[test]
fn second_paint_of_same_glyph_skips_pixel_extraction() {
    test_hooks::reset_extract_region_rgba_call_count();

    let ctx = egui::Context::default();
    let rasterizer = FakeRasterizer {
        calls: AtomicUsize::new(0),
    };
    let fallback = FallbackChain::new(GlyphFontId(1), []);
    let mut cache = GlyphCache::new();

    for _ in 0..2 {
        let meta = resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
            .expect("must resolve");
        let ctx_for_loader = ctx.clone();
        let _texture = get_or_create_overlay_texture(&ctx, meta.key, || {
            let pixels = extract_overlay_glyph_pixels(&cache, &meta);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [meta.width as usize, meta.height as usize],
                &pixels,
            );
            ctx_for_loader.load_texture("test-overlay-glyph", image, egui::TextureOptions::LINEAR)
        });
    }

    assert_eq!(
        test_hooks::extract_region_rgba_call_count(),
        1,
        "second paint of the same glyph must not re-extract pixels"
    );
}
