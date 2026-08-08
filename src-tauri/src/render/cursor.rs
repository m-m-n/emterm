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
    // task0006 (right-edge persistent placement): the terminal grid's
    // x-origin is identical with and without the persistent mux sidebar —
    // it only reserves usable WIDTH on the right, so no inset belongs
    // here. Matches `window_host::cell_metrics_px`'s un-inset origin_x.
    let metrics = FontMetrics {
        cell_w: app.cell_w_logical,
        cell_h: app.cell_h_logical,
        left_pad: pad,
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
mod tests;
