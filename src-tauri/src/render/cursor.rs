//! Cursor + preedit overlay rendering (Phase 4-E).
//!
//! The cursor itself is still drawn inline by [`super::draw_cursor`].
//! This module adds the preedit overlay layer: an underline beneath the
//! row of cells the in-progress IME composition would occupy if it were
//! committed. The overlay starts at the anchor cell (the cursor position
//! at composition start) and wraps within the terminal width when the
//! composition spans past the right edge of the current row.
//!
//! The geometry is computed in pure functions ([`preedit_underline_runs`])
//! so the layout logic is unit-testable without an egui context.
//! [`draw_preedit_overlay`] is the thin egui-painter wrapper that turns
//! the runs into line segments.
//!
//! render-cpu-optimization task0001: the focused filled block cursor also
//! lives here now. It used to be baked into the wgpu grid instances by
//! `collect_cell_inputs`'s (now removed) `block_cursor_cell` fg/bg swap;
//! that coupled grid content to cursor position / blink phase / window
//! focus, which the IMPLEMENTATION.md cross-task invariant forbids. The
//! visibility / suppression rules ([`cursor_screen_row`]) and the rect
//! geometry ([`block_cursor_rect`]) are pure and unit-tested; painting
//! ([`draw_block_cursor`]) is a thin wrapper called from
//! [`super::draw_cursor`]'s block-style, focused branch.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use term_core::terminal_core::TerminalCore;

use crate::app::App;
use crate::fold::FoldLayout;
use crate::ime::preedit::Anchor;
use crate::render::font::{GlyphCache, GlyphKey, OverlayGlyphMeta};
use crate::render::theme::{Rgb, Theme};

/// Cell metrics expected by the overlay routines. Mirrors the values
/// `App` carries (`cell_w_logical` / `cell_h_logical` / `padding`) but
/// is passed in explicitly so the pure layout code is decoupled from
/// the runtime container and can be unit-tested with fabricated values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub left_pad: f32,
    pub top_pad: f32,
}

/// One horizontal underline run beneath the preedit text. A composition
/// that wraps within the terminal width produces multiple runs (one per
/// visual row). Each run is in **logical pixel** coordinates relative to
/// the panel's `min_rect().min`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnderlineRun {
    /// Starting x in logical pixels.
    pub x_start: f32,
    /// Ending x (exclusive) in logical pixels.
    pub x_end: f32,
    /// y of the underline in logical pixels.
    pub y: f32,
}

/// Compute the underline runs for a preedit composition.
///
/// `anchor` is the cursor cell at composition start. `preedit_text_width`
/// is the number of *cells* the composition occupies — the caller is
/// responsible for accounting for double-wide / ambiguous-width
/// characters (we pass an aggregate width so this function stays
/// language-agnostic).
///
/// `cols` is the grid column count; the overlay wraps to a new row when
/// it would exceed the right edge.
///
/// Returns `Vec::new()` for a zero-width composition.
pub fn preedit_underline_runs(
    anchor: Anchor,
    preedit_text_cells: u16,
    cols: u16,
    metrics: FontMetrics,
) -> Vec<UnderlineRun> {
    if preedit_text_cells == 0 || cols == 0 {
        return Vec::new();
    }
    let mut runs = Vec::new();
    let mut remaining = preedit_text_cells as u32;
    // Clamp anchor to the grid so a stale anchor (from before a resize)
    // can't push the overlay off-canvas.
    let mut row = anchor.row;
    let mut col = anchor.col.min(cols.saturating_sub(1));

    while remaining > 0 {
        let available = (cols as u32) - (col as u32);
        let span = remaining.min(available);
        let x_start = metrics.left_pad + (col as f32) * metrics.cell_w;
        let x_end = x_start + (span as f32) * metrics.cell_w;
        // Underline sits just below the cell box, matching the SGR
        // underline convention in `draw_grid`.
        let y = metrics.top_pad + (row as f32) * metrics.cell_h + metrics.cell_h - 1.0;
        runs.push(UnderlineRun { x_start, x_end, y });

        remaining -= span;
        if remaining > 0 {
            // Wrap to the next row, column 0.
            row = row.saturating_add(1);
            col = 0;
        }
    }
    runs
}

/// Paint the preedit underline overlay on top of the cursor. Pure
/// rendering wrapper around [`preedit_underline_runs`] — call this from
/// the central panel render path after the cursor has been drawn.
///
/// `cursor_cell` is the anchor (cursor position at composition start),
/// `preedit_text` is the rendering-safe composition string (already
/// sanitized — see [`crate::ime::preedit::State`]), and `font_metrics`
/// supplies the cell geometry.
pub fn draw_cursor_with_preedit(
    painter: &egui::Painter,
    cursor_cell: Anchor,
    preedit_text: &str,
    font_metrics: FontMetrics,
    cols: u16,
    color: Color32,
    panel_origin: Pos2,
) {
    if preedit_text.is_empty() || cols == 0 {
        return;
    }
    let cells = preedit_cell_width(preedit_text);
    let runs = preedit_underline_runs(cursor_cell, cells, cols, font_metrics);
    for run in runs {
        painter.line_segment(
            [
                Pos2::new(panel_origin.x + run.x_start, panel_origin.y + run.y),
                Pos2::new(panel_origin.x + run.x_end, panel_origin.y + run.y),
            ],
            // 2-px underline so the overlay is visible against text the
            // user already typed on the cursor row.
            Stroke::new(2.0, color),
        );
    }
}

/// Approximate cell width of `text` for overlay purposes. We use
/// `unicode-width` semantics (1 for narrow, 2 for wide), matching
/// `term_core`'s grid model. Combining marks contribute 0 width and the
/// floor-to-1 rule from `render::visible_width` does NOT apply here —
/// we want the actual rendered span, which is what the IME would commit
/// into the grid.
pub fn preedit_cell_width(text: &str) -> u16 {
    let mut total: u32 = 0;
    for c in text.chars() {
        let w = term_core::char_width(c as u32);
        total = total.saturating_add(w as u32);
    }
    total.min(u16::MAX as u32) as u16
}

// ── Block cursor overlay (render-cpu-optimization task0001) ───────────

/// Screen row the filled block cursor should paint at this frame, or
/// `None` when it must be suppressed entirely:
///
/// - scrolled back into history (`scroll_offset != 0` — the live cursor
///   position has no meaning over scrollback content, matching the
///   WebView build's `scrollOffset !== 0` guard), or
/// - with an active fold layout, the cursor's absolute buffer row falls
///   inside a collapsed region (hidden by the fold).
///
/// `core_row` is `TerminalCore::get_cursor_row()` — the cursor's row in
/// the *unfolded* live viewport. Without a fold layout this is already
/// the on-screen row (identity). With one, `scrollback_len + core_row`
/// gives the absolute buffer row the layout indexes by, mirroring the
/// fold-aware translation `render::draw_search_highlights` already
/// applies to search-match rows.
pub fn cursor_screen_row(
    scrollback_len: u32,
    core_row: u16,
    scroll_offset: u32,
    fold_layout: Option<&FoldLayout>,
) -> Option<u16> {
    if scroll_offset != 0 {
        return None;
    }
    let Some(layout) = fold_layout else {
        return Some(core_row);
    };
    let abs_row = scrollback_len + core_row as u32;
    if layout.region_at_line(abs_row).is_some() {
        // The cursor's row is inside a collapsed region's body — hidden
        // by the fold summary that replaced it on screen.
        return None;
    }
    let display_line = layout.actual_line_to_display(abs_row);
    if display_line < layout.display_start {
        // Off-screen above the visible window (defensive; the live
        // cursor row should always be within the viewport in practice).
        return None;
    }
    Some((display_line - layout.display_start) as u16)
}

/// Resolve the column whose glyph/style should paint the block cursor
/// (task0005 AC-5, finding c0732dd907681dc1): `col` unchanged for a normal
/// or wide-leading cell, or `col - 1` when `col` is the width-0 trailing
/// half of a wide glyph (`cell_width == 0`) — the cursor must draw the
/// glyph's full 2-cell footprint anchored at its leading column, not an
/// empty 1-cell box over the (blank) trailing cell.
///
/// `cell_width` is `TerminalCore::get_cell_width(col, row)` at the raw
/// cursor column. A trailing cell at column 0 (defensive — should not
/// occur: a wide glyph's trailing half always has a leading half at
/// `col - 1`) returns `col` unchanged rather than underflowing.
pub fn resolve_cursor_glyph_col(col: u16, cell_width: u8) -> u16 {
    if cell_width == 0 && col > 0 {
        col - 1
    } else {
        col
    }
}

/// Geometry of the filled block cursor's rectangle: the cell at
/// `(col, screen_row)`, widened to `width_cells` columns so a wide glyph
/// under the cursor (CJK character, emoji) has its full footprint
/// inverted rather than just its leading half.
///
/// `col` / `screen_row` are clamped inside `0..cols` / `0..rows` and the
/// width is clamped so the rect never extends past the grid's right
/// edge — this keeps the last-column / last-row cursor position inside
/// grid bounds even if a caller passes a stale or off-by-one value.
pub fn block_cursor_rect(
    col: u16,
    screen_row: u16,
    width_cells: u8,
    cols: u16,
    rows: u16,
    metrics: FontMetrics,
) -> Rect {
    let col = col.min(cols.saturating_sub(1));
    let row = screen_row.min(rows.saturating_sub(1));
    let max_width = cols.saturating_sub(col).max(1);
    let width_cells = (width_cells.max(1) as u16).min(max_width);
    let x = metrics.left_pad + col as f32 * metrics.cell_w;
    let y = metrics.top_pad + row as f32 * metrics.cell_h;
    Rect::from_min_size(
        Pos2::new(x, y),
        Vec2::new(metrics.cell_w * width_cells as f32, metrics.cell_h),
    )
}

/// Whether the glyph under the cursor is worth painting on top of the
/// filled rect. An empty cell (no character, or bare whitespace) leaves
/// no visible mark once inverted, so the overlay paints the rect only —
/// no stray glyph artifact.
pub fn cursor_glyph_paintable(glyph: &str) -> bool {
    !glyph.trim().is_empty()
}

/// Resolve the terminal cursor overlay's paint color (task0003 AC-1,
/// AC-2, AC-3, AC-5): the theme's effective cursor color — the active
/// color scheme's cursor color, or an OSC 12 override while one is
/// active; OSC 112 resets it back to the scheme color (see
/// `Theme::apply_osc`). This is never `TerminalCore::get_cursor_fg()`
/// (the SGR pen foreground the next printed character would use — an
/// unrelated piece of state despite the similar getter name) and never
/// a per-cell resolved style color. Shared by every cursor paint site:
/// underline / bar / unfocused hollow-block outline in
/// [`super::draw_cursor`], and the focused filled block's fill in
/// [`draw_block_cursor`] below — so all cursor shapes agree on one
/// color source (IMPLEMENTATION.md D3).
pub fn resolve_cursor_color(theme: &Theme) -> Rgb {
    theme.cursor_fg
}

/// HiDPI-aware font size in physical pixels for the overlay glyph raster
/// (task0002 AC-1, r1-c2): mirrors `window_host::render`'s
/// `theme.font_size_px() * pixels_per_point.max(1.0)` computation that
/// feeds the wgpu grid pass' `CellMetrics::font_size_px`. Before this fix
/// the overlay resolved at `runtime_font_size_pt * PT_TO_PX` alone — on a
/// HiDPI host (`pixels_per_point > 1.0`) that built a DIFFERENT
/// `GlyphKey` than the grid pass and rasterized the overlay glyph at
/// logical (1x) resolution while the grid pass rasterized at the host's
/// real scale factor.
pub fn overlay_font_px(runtime_font_size_pt: f32, pixels_per_point: f32) -> f32 {
    runtime_font_size_pt * crate::settings::PT_TO_PX * pixels_per_point.max(1.0)
}

/// Baseline y in the SAME coordinate space `cell_top_y` is expressed in
/// (task0002 AC-2, r1-c1): mirrors
/// `terminal_grid_pass::GridInstanceBuilder::build_instances_split`'s
/// `let baseline = y + v_pad + base_ascent;` exactly — `v_pad`
/// ([`crate::render::font::compute_v_pad`]) centers the line vertically
/// inside a cell taller than the font's natural line height, and
/// `base_ascent` is the base font's real ascent. The pre-rework overlay
/// used `base_ascent` alone, so the covered glyph floated above the
/// grid's baseline whenever `v_pad > 0.0`.
fn overlay_baseline_y(cell_top_y: f32, v_pad: f32, base_ascent: f32) -> f32 {
    cell_top_y + v_pad + base_ascent
}

/// Paint the focused block cursor's filled overlay on top of the grid:
/// the cell rect is filled with the theme's cursor color
/// ([`resolve_cursor_color`] — task0003 D3, never the covered cell's own
/// SGR-derived color) and the covered glyph (if any) is redrawn on top in
/// the covered cell's fully-resolved BACKGROUND color — reverse video /
/// selection / dim / hidden already applied, the same
/// [`super::resolve_cell_style_from_packed`] pipeline every other cell
/// goes through — so the glyph stays legible against the cursor fill. A
/// wide (2-cell) glyph under the cursor has its full 2-cell footprint
/// filled.
///
/// This replaces the fg/bg swap `collect_cell_inputs` used to bake into
/// the grid instance for the cursor cell (removed `block_cursor_cell`
/// parameter); grid instance data is now independent of cursor state.
///
/// task0005 AC-5: `TerminalCore::get_cursor_col()` can report the width-0
/// trailing half of a wide glyph (the cell the cursor logically advances
/// past when a program repositions it there); [`resolve_cursor_glyph_col`]
/// resolves that back to the leading column so the glyph, style, and rect
/// all reflect the actual wide character rather than its blank
/// continuation cell.
///
/// block-cursor-glyph-font task0001: the covered-glyph redraw used to go
/// through `egui::Painter::text` with egui's own built-in monospace
/// `FontId` constructor — not the font the wgpu `terminal_grid_pass`
/// draws the surrounding grid with. That made the covered glyph visibly
/// diverge from the grid (e.g. a slashed-zero terminal font rendering an
/// unslashed zero under the cursor). The glyph is now resolved via
/// [`crate::render::font::resolve_overlay_glyph_meta`] — the SAME
/// `FallbackChain` / `GlyphRasterizer` / `GlyphCache` triple
/// `terminal_grid_pass::GridInstanceBuilder::glyph_instance` uses — and
/// painted as an egui texture tinted with the resolved cell color
/// ([`get_or_create_overlay_texture`] caches one `egui::TextureHandle`
/// per glyph in `egui::Context`'s own persistent storage, so a glyph
/// already drawn this frame by the grid is a cache hit here too, not a
/// second rasterize). Per the render-cpu-optimization task0001 invariant cited
/// above the function doc, this stays entirely inside the egui overlay
/// layer: the wgpu grid instance stream is untouched and still
/// independent of cursor state (IMPLEMENTATION.md D1).
///
/// block-cursor-glyph-font task0002 (rework, closing review round 1's
/// HIGH findings r1-c1/r1-c2/r1-p1 and MEDIUM r1-c4): three axes now
/// mirror the grid pass exactly instead of approximating it —
/// - **HiDPI** (r1-c2): `font_px` includes `painter.ctx().pixels_per_point()`
///   ([`overlay_font_px`]), matching `window_host::render`'s
///   `theme.font_size_px() * pixels_per_point.max(1.0)`. Without this the
///   overlay's `GlyphKey` (and its raster resolution) silently diverged
///   from the grid's on a 2x host.
/// - **Baseline** (r1-c1): the baseline includes `v_pad`
///   ([`crate::render::font::compute_v_pad`], the SAME formula
///   `terminal_grid_pass::GridInstanceBuilder::build_instances_split`
///   uses), not just `base_ascent` — see [`overlay_baseline_y`].
/// - **Shrink-to-fit** (r1-c4): a wide-advance fallback glyph is shrunk
///   horizontally via [`crate::render::font::overlay_horizontal_fit_scale`],
///   mirroring the grid pass's `GlyphFit::HorizontalOnly` reference-width
///   selection.
/// - **Per-frame allocation** (r1-p1): [`resolve_overlay_glyph_meta`]
///   resolves the cache key + geometry WITHOUT touching pixel data;
///   [`paint_overlay_glyph`]'s `loader` closure — invoked only on an
///   `egui::TextureHandle` cache MISS — is the only place that reaches
///   `extract_overlay_glyph_pixels` / builds an `egui::ColorImage`.
///
/// [`resolve_overlay_glyph_meta`]: crate::render::font::resolve_overlay_glyph_meta
///
/// Suppressed per [`cursor_screen_row`]: scrolled back into history, or
/// the cursor's row is hidden inside a collapsed fold region. The
/// caller ([`super::draw_cursor`]) only reaches this function once the
/// focused / cursor-visible / block-style / blink-on gate has already
/// passed.
pub fn draw_block_cursor(painter: &egui::Painter, core: &TerminalCore, theme: &Theme, app: &App) {
    let scrollback_len = core.get_scrollback_length();
    let content_row = core.get_cursor_row();
    let screen_row = match cursor_screen_row(
        scrollback_len,
        content_row,
        app.scroll_offset(),
        app.fold_layout(),
    ) {
        Some(r) => r,
        None => return,
    };
    let raw_col = core.get_cursor_col();
    // AC-5: when the cursor sits on the width-0 trailing half of a wide
    // glyph, resolve to the leading column so the rect + glyph below
    // reflect the actual character instead of the blank continuation cell.
    let col = resolve_cursor_glyph_col(raw_col, core.get_cell_width(raw_col, content_row));

    let flags = core.get_cell_flags(col, content_row);
    let packed_fg = core.get_cell_fg(col, content_row);
    let packed_bg = core.get_cell_bg(col, content_row);
    let abs_row = scrollback_len + content_row as u32;
    let selected = app
        .selection
        .as_ref()
        .map(|s| s.contains(abs_row, col))
        .unwrap_or(false);
    let style = super::resolve_cell_style_from_packed(theme, packed_fg, packed_bg, flags, selected);

    let ch = core.get_cell_char(col, content_row);
    let width_cells = super::visible_width(&ch, app.settings.ambiguous_width_mode);

    let pad = app.settings.padding as f32;
    let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    // task0005 D2: same x-origin inset the main grid uses
    // (`window_host::cell_metrics_px`) so the block cursor stays aligned
    // next to a persistent mux sidebar (AC-5). `painter.ctx()` stands in
    // for the `WindowHost`-cached window width this overlay has no access
    // to (mirrors `render::draw_cursor` / `draw_search_highlights`).
    let sidebar_inset = app.mux_sidebar_x_inset(painter.ctx().screen_rect().width());
    let metrics = FontMetrics {
        cell_w: app.cell_w_logical,
        cell_h: app.cell_h_logical,
        left_pad: pad + sidebar_inset,
        top_pad: crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad,
    };
    let rect = block_cursor_rect(
        col,
        screen_row,
        width_cells,
        core.cols(),
        core.rows(),
        metrics,
    );

    painter.rect_filled(rect, 0.0, super::rgb_to_egui(resolve_cursor_color(theme)));
    if cursor_glyph_paintable(&ch) {
        // r1-c2: HiDPI — same `pixels_per_point` the grid pass' wgpu
        // `CellMetrics` applies (`window_host::render`), so the overlay's
        // `GlyphKey` and raster resolution match the grid's instead of
        // silently resolving at 1x on a 2x host.
        let scale = painter.ctx().pixels_per_point().max(1.0);
        let font_px = overlay_font_px(app.runtime_font_size_pt, scale);
        // AC-1/D2: same fallback chain, rasterizer, and cache the grid
        // pass uses — see `crate::render::font::resolve_overlay_glyph_meta`.
        // Meta only (r1-p1): no pixel extraction / `egui::ColorImage`
        // happens here — see `paint_overlay_glyph`'s doc.
        let meta = {
            let mut cache = app.font_cache.lock();
            crate::render::font::resolve_overlay_glyph_meta(
                app.font_rasterizer.as_ref(),
                &app.font_fallback,
                &mut cache,
                &ch,
                font_px,
                style.bold,
            )
        };
        if let Some(meta) = meta {
            // r1-c1: same v_pad + base_ascent source the grid pass
            // anchors every glyph to
            // (`GridInstanceBuilder::build_instances_split`), computed at
            // the SAME (HiDPI-scaled) `font_px` / `cell_h` so the overlay
            // glyph sits on the identical line the grid would have drawn
            // it on — not `base_ascent` alone (the pre-rework bug).
            let base_metrics = app
                .font_rasterizer
                .font_metrics(app.font_fallback.base(), font_px);
            let base_ascent_px = base_metrics.map(|m| m.ascent).unwrap_or(font_px * 0.8);
            let base_line_height_px = base_metrics.map(|m| m.line_height()).unwrap_or(font_px);
            let cell_h_px = metrics.cell_h * scale;
            let v_pad_px = crate::render::font::compute_v_pad(cell_h_px, base_line_height_px);
            // Convert the physical-pixel baseline offset back down to the
            // logical/point space `rect` (and every other `egui::Painter`
            // call in this module) already operates in.
            let baseline = overlay_baseline_y(rect.min.y, v_pad_px / scale, base_ascent_px / scale);
            // AC-3: tint is the covered cell's fully-resolved bg color.
            paint_overlay_glyph(
                painter,
                &meta,
                rect,
                baseline,
                scale,
                style.bg,
                &app.font_cache,
            );
        }
    }
}

/// Paint `meta`'s raster into the overlay so its baseline lands at
/// `baseline` (already converted to the logical/point space `rect` is
/// expressed in — see [`overlay_baseline_y`]) and its horizontal
/// footprint stays inside `rect`'s width (r1-c4:
/// [`crate::render::font::overlay_horizontal_fit_scale`] shrinks a
/// wide-advance fallback glyph rather than letting it bleed past the
/// cursor rect). `scale` is `painter.ctx().pixels_per_point().max(1.0)`:
/// `meta`'s width / height / bearing / advance were all resolved at a
/// font size already multiplied by `scale` (r1-c2), so they are divided
/// back down here to size the destination rect correctly in egui's point
/// space (egui itself re-multiplies by `pixels_per_point` when
/// rasterizing the final frame).
///
/// `tint` colors `meta.needs_tint` (Alpha / Subpixel-sourced, a flat-white
/// coverage mask) rasters; Rgba-sourced rasters (color emoji / COLRv1)
/// keep their own color and paint with a neutral white tint (no-op
/// multiply) instead.
///
/// r1-p1: `loader` — and therefore
/// `crate::render::font::extract_overlay_glyph_pixels` / building an
/// `egui::ColorImage` — only runs on an `egui::TextureHandle` cache MISS
/// inside [`get_or_create_overlay_texture`]. A cache HIT (the common
/// case: the cursor sits on the same glyph across most frames) never
/// locks `font_cache` again and never extracts pixels.
///
/// `egui::Painter` I/O has no test hook (matches this module's existing
/// `painter.text` / `painter.line_segment` philosophy — see the module
/// doc); the pure geometry and caching feeding this call
/// ([`crate::render::font::resolve_overlay_glyph_meta`],
/// [`get_or_create_overlay_texture`]) are unit-tested instead. The final
/// on-screen pixels are covered by manual visual check (MT-1).
fn paint_overlay_glyph(
    painter: &egui::Painter,
    meta: &OverlayGlyphMeta,
    rect: Rect,
    baseline: f32,
    scale: f32,
    tint: Color32,
    font_cache: &parking_lot::Mutex<GlyphCache>,
) {
    let ctx = painter.ctx();
    let texture = get_or_create_overlay_texture(ctx, meta.key, || {
        let cache = font_cache.lock();
        let pixels = crate::render::font::extract_overlay_glyph_pixels(&cache, meta);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [meta.width as usize, meta.height as usize],
            &pixels,
        );
        ctx.load_texture(
            "emterm-cursor-overlay-glyph",
            image,
            egui::TextureOptions::LINEAR,
        )
    });

    let glyph_w = meta.width as f32 / scale;
    let glyph_h = meta.height as f32 / scale;
    let glyph_x = rect.min.x + meta.bearing_left as f32 / scale;
    let glyph_y = baseline - meta.bearing_top as f32 / scale;

    // r1-c4: shrink horizontally when the fallback glyph's design advance
    // exceeds the covered cell footprint, mirroring the grid pass'
    // `GlyphFit::HorizontalOnly` shrink so a wide CJK / Dingbat fallback
    // under the cursor doesn't bleed past the cursor rect.
    let sx = crate::render::font::overlay_horizontal_fit_scale(
        rect.width(),
        meta.advance / scale,
        glyph_w,
    );
    let dest = Rect::from_min_size(
        Pos2::new(glyph_x, glyph_y),
        Vec2::new(glyph_w * sx, glyph_h),
    );
    let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
    let tint = if meta.needs_tint {
        tint
    } else {
        Color32::WHITE
    };
    painter.image(texture.id(), dest, uv, tint);
}

/// Look up (or create via `loader`) the cached `egui::TextureHandle` for
/// `key`, using `egui::Context`'s own persistent storage
/// (`Context::data` / `data_mut`) so resolving the SAME glyph again —
/// this frame or a later one — reuses the texture instead of registering
/// a fresh one with egui's texture manager (AC-6 / NFR1: "no new
/// `egui::TextureHandle` per frame for a glyph already in the shared
/// cache"). `loader` is the only thing that actually calls
/// `egui::Context::load_texture`; test-injectable so tests can count
/// creations with a stub instead of driving a real paint pass.
fn get_or_create_overlay_texture<L>(
    ctx: &egui::Context,
    key: GlyphKey,
    loader: L,
) -> egui::TextureHandle
where
    L: FnOnce() -> egui::TextureHandle,
{
    let id = egui::Id::new(("emterm-cursor-overlay-glyph-texture", key));
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let handle = loader();
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let source = include_str!("cursor.rs");
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

        let meta =
            resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", font_px, false)
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
            let meta =
                resolve_overlay_glyph_meta(&rasterizer, &fallback, &mut cache, "A", 13.0, false)
                    .expect("must resolve");
            let ctx_for_loader = ctx.clone();
            let _texture = get_or_create_overlay_texture(&ctx, meta.key, || {
                let pixels = extract_overlay_glyph_pixels(&cache, &meta);
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [meta.width as usize, meta.height as usize],
                    &pixels,
                );
                ctx_for_loader.load_texture(
                    "test-overlay-glyph",
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
        }

        assert_eq!(
            test_hooks::extract_region_rgba_call_count(),
            1,
            "second paint of the same glyph must not re-extract pixels"
        );
    }
}
