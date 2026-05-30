//! Grid → egui draw routines.
//!
//! Phase 6 swap: the renderer reads the grid through `term_core` accessors
//! (`get_cell_char`, `get_cell_fg/bg/flags`, `get_cursor_*`) instead of the
//! Phase 1 PoC's bespoke `Grid` type. Colors are decoded from the packed
//! `u32` returned by `get_cell_fg/bg`.
//!
//! Sub-phase 2 (dirty-row diff): the per-cell loop below still iterates the
//! full grid on every invocation, but the caller (`window_host::render`)
//! now skips the entire egui run when `App::dirty_rows_this_frame` is empty.
//! egui's immediate-mode pipeline rebuilds tessellation per frame, so true
//! per-row skipping requires a persistent offscreen target — that lives in
//! a future sub-phase. Today the savings come from frame-level skip plus
//! `term_core::clear_dirty()` consumption synchronized with each rendered
//! frame.
//!
//! Sub-phase 3 (cursor + SGR full reflection): `cell_style` honors every
//! `term_core::cell::STYLE_*` flag we track today (bold via weight, dim via
//! alpha, italic via egui italic face, underline as a horizontal line,
//! reverse by swapping fg/bg, hidden by clamping fg to bg, strikethrough
//! as an overlay line). `draw_cursor` reads the cursor's style/blink/
//! visibility/color getters so the renderer is ready to respond as soon
//! as the parser routes for DECSCUSR / DECTCEM / OSC 22 / OSC 12 land in
//! sub-phase 6. Double / curly underline plus SGR 58 underline color
//! await a future term_core extension (only a single `STYLE_UNDERLINE`
//! bit exists today). Per-cell `STYLE_BLINK` is rendered statically
//! (no animation) to avoid two competing blink phases against the
//! cursor; revisit when sub-phase 6 fires.

pub mod app_icon;
pub mod block_drawing;
pub mod box_drawing;
pub mod cursor;
pub mod emoji_resample;
pub mod font;
pub mod terminal_grid_pass;
pub mod theme;

use std::time::Duration;

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use term_core::cell::{
    STYLE_BLINK, STYLE_BOLD, STYLE_DIM, STYLE_HIDDEN, STYLE_ITALIC, STYLE_REVERSE,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINE,
};
use term_core::terminal_core::TerminalCore;
use term_core::{char_width, is_ambiguous_width};

use crate::app::{App, BLINK_HALF_MS};
use crate::render::terminal_grid_pass::CellInput;
use crate::render::theme::{Rgb, Theme};
use crate::selection::Selection;
use crate::settings::AmbiguousWidthMode;

/// Fallback cell width in logical pixels when the rasterizer can't
/// measure "M" (e.g. test builds with a stub rasterizer that returns
/// no glyphs). Picked to roughly match Inconsolata 13pt so failure
/// modes still produce a usable grid.
pub const FALLBACK_CELL_W: f32 = 8.5;
/// Fallback cell height in logical pixels. Mirrors [`FALLBACK_CELL_W`]'s
/// intent — used only when the rasterizer cannot supply metrics for
/// the base font.
pub const FALLBACK_CELL_H: f32 = 17.0;

/// Compute the per-cell width and height (logical pixels) for a given
/// font + size, mirroring the legacy WebView build's
/// `ctx.measureText("M").width` / `ceil(ascent + descent)` path. The
/// returned values are at egui's logical-pixel scale (1.0×); the
/// renderer multiplies by `pixels_per_point` for the physical-pixel
/// metrics handed to wgpu.
///
/// Returns the [`FALLBACK_CELL_W`] / [`FALLBACK_CELL_H`] pair when the
/// rasterizer cannot shape "M" against the base font (typically only
/// in test builds whose font stack has no registered glyphs).
pub fn compute_cell_dims(
    rasterizer: &dyn crate::render::font::traits::GlyphRasterizer,
    fallback: &crate::render::font::fallback::FallbackChain,
    font_size_px: f32,
) -> (f32, f32) {
    let base = fallback.base();
    // Width: shape "M" against the base font and read the advance off
    // the resulting glyph bitmap. For monospace coding fonts every
    // glyph has the same advance, so the single-character probe is
    // sufficient.
    let advance = rasterizer
        .shape("M", base, font_size_px)
        .first()
        .and_then(|g| rasterizer.raster(g.font, g.glyph_id, g.size_px))
        .map(|b| b.advance)
        .filter(|a| a.is_finite() && *a > 0.0)
        .unwrap_or(FALLBACK_CELL_W);
    // Height: ascent + descent matches the WebView build's
    // `ceil(ascent + descent)`. `line_gap` is intentionally excluded
    // so the grid stays tight (most monospace coding fonts ship a
    // zero line gap anyway).
    let height = rasterizer
        .font_metrics(base, font_size_px)
        .map(|m| (m.ascent + m.descent).ceil())
        .filter(|h| h.is_finite() && *h > 0.0)
        .unwrap_or(FALLBACK_CELL_H);
    (advance, height)
}

/// Per-cell paint parameters resolved from a `term_core` cell + active
/// palette + selection state.
struct CellStyle {
    fg: Color32,
    bg: Color32,
    // Read by future Resolver-driven weight / style selection; the prior
    // painter.text() path read these for egui font face, which is now gone.
    #[allow(dead_code)]
    bold: bool,
    #[allow(dead_code)]
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

/// Bundle of widget events emitted by the chrome (title bar / tab
/// bar) during a single frame. Either field may be `None` when no
/// interaction landed this frame. The render loop applies them in
/// the order: title bar → tab bar, mirroring their on-screen stack.
pub struct FrameEvents {
    pub title: Option<crate::ui::TitleBarEvent>,
    pub tab: Option<crate::ui::TabEvent>,
}

/// Phase-1 placeholder kept for compatibility; routes to the real renderer
/// when a tab exists.
pub fn draw_placeholder(ctx: &egui::Context, app: &App, window_maximized: bool) -> FrameEvents {
    draw_terminal(ctx, app, window_maximized)
}

/// Draw the active tab. If no tabs exist, draws a hint message. The
/// caller is responsible for applying the returned events (if any) —
/// title-bar actions hit `winit::Window` directly, tab-bar actions go
/// through `App::apply_tab_event` post-frame.
///
/// `window_maximized` is forwarded to the CSD title bar so it can
/// swap the maximize glyph for the restore (overlapped-squares) one
/// when the window is already maximized.
pub fn draw_terminal(ctx: &egui::Context, app: &App, window_maximized: bool) -> FrameEvents {
    // Per-frame theme seeded from settings (font_size_pt + cursor
    // style). Active-tab OSC mutations live on `Tab::theme`; layering
    // those on top of the settings-derived base lets OSC 4/10/11/12/22
    // re-skin the running session without losing the user-configured
    // font size. Falls back to the settings-only base when no tab is
    // attached yet (initial frame).
    let theme = match app.active_tab() {
        Some(tab) => tab.theme.lock().clone(),
        None => Theme::from_settings(app.settings.as_ref()),
    };

    // Custom CSD title bar — sits above everything else so its
    // glyph buttons stay clickable regardless of tab / status state.
    // The window runs with `with_decorations(false)`, so without this
    // there would be no close / minimize / maximize affordance.
    let icon = app_icon::texture_id(ctx);
    let title_event = crate::ui::title_bar::draw(ctx, "eMterm", window_maximized, icon);

    // Phase 4-B: real tab bar widget. We build a lightweight view-
    // model from the live tabs vector once per frame.
    let items: Vec<crate::ui::tab_bar::TabBarItem> = app
        .tabs
        .iter()
        .map(|t| {
            let mut item = crate::ui::tab_bar::TabBarItem::new(t.display_title());
            if let Some(name) = &t.mux_session_name {
                item = item.with_mux_session(name.clone());
            }
            item
        })
        .collect();
    let tab_event = if items.is_empty() || !app.settings.show_tab_bar {
        None
    } else {
        crate::ui::tab_bar::draw(ctx, &items, app.active)
    };

    // Phase 4-D: status-bar panel. Inserted before the central panel
    // (egui sizes top/bottom panels first, then the central panel
    // takes the remaining rect). The widget itself decides top vs
    // bottom from settings.
    let status_vm = app.status_bar_view_model();
    let emoji_resources = crate::ui::status_bar::EmojiResources {
        rasterizer: app.font_rasterizer.as_ref(),
        fallback: &app.font_fallback,
        cache: &app.emoji_texture_cache,
    };
    crate::ui::status_bar::draw(ctx, &status_vm, Some(&emoji_resources));

    egui::CentralPanel::default()
        // Phase 4-H (FR12): the central panel no longer paints the cell
        // background — `TerminalGridPass` clears the swapchain to the
        // theme background and emits per-cell solid quads where the SGR
        // bg differs. Using `Color32::TRANSPARENT` keeps egui's overlay
        // (cursor + IME preedit underline) on top of the wgpu-rendered
        // cells without painting an opaque rect that would hide them.
        .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
        .show(ctx, |ui| {
            if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                draw_cursor(ui, &core, &theme, app);
                // Preedit rendering is owned by the wgpu cell pass via
                // `apply_preedit_overlay` (reverse-video cells). The
                // legacy egui underline overlay was removed so it
                // doesn't stack on top of the inline reverse-video
                // composition cells.
            } else {
                ui.colored_label(Color32::LIGHT_GRAY, "no tab — shell may have exited");
            }
        });

    // Keep blinking cursors animating. egui only repaints on demand, so we
    // schedule a wake-up at the half-period. Frame-level skip in
    // `window_host::render` still kicks in when `dirty_rows_this_frame`
    // returns empty (i.e. cursor blink-disabled or cursor row never
    // entered the dirty set this frame), so this only wakes us up when
    // we genuinely need to re-evaluate.
    if let Some(tab) = app.active_tab() {
        let core = tab.core.lock();
        if core.get_cursor_blink() {
            ctx.request_repaint_after(Duration::from_millis(BLINK_HALF_MS as u64));
        }
    }

    // Status-bar periodic redraw is now provider-owned: each
    // Provider that needs periodic updates (TimeProvider timer
    // thread, GitBranch / Command worker threads) holds an
    // `Arc<WakeFn>` and invokes it directly. Event-driven providers
    // (CwdProvider) wake on OSC 7 receipt. `egui::Context::
    // request_repaint_after` does not bridge to winit so the prior
    // `request_repaint_after(Duration::from_secs(1))` floor was a
    // no-op in release builds — see SPEC.md Notes section.

    FrameEvents {
        title: title_event,
        tab: tab_event,
    }
}

/// Walk the terminal grid and build a `Vec<CellInput>` suitable for
/// [`crate::render::terminal_grid_pass::TerminalGridPass::prepare`].
///
/// Phase 4-H (FR12): the cell loop that used to call `painter.text()` /
/// `painter.line_segment()` / `painter.rect_filled()` now emits per-cell
/// inputs consumed by the custom wgpu pass. Selection is encoded via the
/// existing fg/bg swap in [`resolve_cell_style`] (no separate selection
/// quad).
///
/// `block_cursor_cell` is `Some((col, row))` when a block-shaped cursor
/// is currently visible (blink-on, style=block, terminal-visible). The
/// matching cell gets its fg/bg swapped so it reads as a filled cursor
/// with the glyph in inverted color — matching the WebView build's
/// rendering. Underline / bar cursor shapes stay on the egui overlay
/// side and pass `None` here.
pub fn collect_cell_inputs(
    core: &TerminalCore,
    theme: &Theme,
    selection: Option<&Selection>,
    width_mode: AmbiguousWidthMode,
    block_cursor_cell: Option<(u16, u16)>,
) -> Vec<CellInput> {
    let cols = core.cols();
    let rows = core.rows();
    let bg_default = rgb_to_egui(theme.bg);
    let mut out: Vec<CellInput> = Vec::with_capacity((cols as usize) * (rows as usize));

    for row in 0..rows {
        let mut col = 0u16;
        while col < cols {
            let mut style = resolve_cell_style(core, theme, col, row, selection);
            if block_cursor_cell == Some((col, row)) {
                std::mem::swap(&mut style.fg, &mut style.bg);
            }
            let ch = core.get_cell_char(col, row);
            let cell_width_cells = visible_width(&ch, width_mode);

            out.push(CellInput {
                col,
                row,
                width_cells: cell_width_cells.max(1),
                glyph: ch,
                fg_rgba: color32_to_rgba(style.fg),
                bg_rgba: color32_to_rgba(style.bg),
                underline: style.underline,
                strikethrough: style.strikethrough,
                draw_background: style.bg != bg_default,
                bg_extend_below: 0.0,
                fit_glyph_to_cell: false,
            });

            col = col.saturating_add(cell_width_cells.max(1) as u16);
        }
    }
    out
}

/// Overlay an in-progress IME preedit composition onto an existing
/// `Vec<CellInput>` produced by [`collect_cell_inputs`].
///
/// Replaces the cells starting at `anchor` with one entry per character
/// of `text`, drawn in reverse video (theme.fg as background, theme.bg
/// as foreground) so composition stands out against the surrounding
/// committed text. Ambiguous-width characters (e.g. ▽ U+25BD) are
/// forced to a 1-cell footprint with their glyphs scaled to fit.
/// Wraps to the next row when the composition exceeds the right edge.
///
/// `bg_extend_below_px` extends the reverse-video bg quad downward by
/// the given physical-pixel amount so glyph descenders that rasterize
/// past `cell_h` are covered by the inverted background. Caller
/// supplies a value already scaled by `pixels_per_point`.
pub fn apply_preedit_overlay(
    cells: &mut Vec<CellInput>,
    anchor: crate::ime::preedit::Anchor,
    text: &str,
    theme: &Theme,
    cols: u16,
    rows: u16,
    bg_extend_below_px: f32,
) {
    if text.is_empty() || cols == 0 || rows == 0 {
        return;
    }
    let bg_default = rgb_to_egui(theme.bg);
    let fg_preedit = rgb_to_egui(theme.bg);
    let bg_preedit = rgb_to_egui(theme.fg);
    let bg_extend_below = bg_extend_below_px.max(0.0);

    let mut row = anchor.row.min(rows.saturating_sub(1));
    let mut col = anchor.col.min(cols.saturating_sub(1));
    let mut overlay: Vec<CellInput> = Vec::new();

    // Split on extended grapheme cluster boundaries so codepoint sequences
    // that compose into a single visual glyph (emoji + VS-16, ZWJ
    // sequences, regional indicator pairs, combining marks, …) land in
    // one cell. Without this, e.g. "⚠️" (U+26A0 + U+FE0F) renders as the
    // bare warning sign in one cell followed by an invisible variation
    // selector glyph in the next.
    use unicode_segmentation::UnicodeSegmentation;
    for cluster in text.graphemes(true) {
        if row >= rows {
            break;
        }
        let s: String = cluster.to_string();
        // Force ambiguous-width chars (e.g. ▽) to 1 cell so the
        // composition footprint matches the user's visual expectation
        // of "1 character = 1 cell" during preedit.
        //
        // VS-16 (U+FE0F) explicitly requests emoji presentation; widen
        // to 2 cells so the colored emoji glyph (rather than the bare
        // BW codepoint) gets a wide slot to render in. Mirrors
        // `term_core::print_handler::flush_grapheme_buffer`.
        let has_vs16 = cluster.chars().any(|c| c as u32 == 0xFE0F);
        let w_raw = visible_width(&s, AmbiguousWidthMode::Narrow);
        let w = if has_vs16 { 2u16 } else { w_raw.max(1) as u16 };
        if col + w > cols {
            row = row.saturating_add(1);
            col = 0;
            if row >= rows {
                break;
            }
        }
        overlay.push(CellInput {
            col,
            row,
            width_cells: w as u8,
            glyph: s,
            fg_rgba: color32_to_rgba(fg_preedit),
            bg_rgba: color32_to_rgba(bg_preedit),
            underline: false,
            strikethrough: false,
            draw_background: bg_preedit != bg_default,
            bg_extend_below,
            fit_glyph_to_cell: true,
        });
        col = col.saturating_add(w);
    }

    if overlay.is_empty() {
        return;
    }

    // Remove any existing cells whose footprint overlaps a preedit cell
    // so the same column isn't drawn twice (the wgpu pass instances each
    // CellInput in submission order without a depth test).
    use std::collections::HashSet;
    let mut occupied: HashSet<(u16, u16)> = HashSet::new();
    for o in &overlay {
        for k in 0..o.width_cells.max(1) as u16 {
            occupied.insert((o.row, o.col.saturating_add(k)));
        }
    }
    cells.retain(|c| {
        for k in 0..c.width_cells.max(1) as u16 {
            if occupied.contains(&(c.row, c.col.saturating_add(k))) {
                return false;
            }
        }
        true
    });
    cells.extend(overlay);
}

/// Pack an `egui::Color32` (already non-premultiplied RGBA8) into the
/// little-endian `[r, g, b, a]` layout the `CellInput` carries. The shader
/// re-expands this via `unpack4x8unorm`.
fn color32_to_rgba(c: Color32) -> [u8; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

/// Cursor overlay: shape from `get_cursor_style`, blink from
/// `get_cursor_blink` modulated by `App::blink_visible_now`, visibility
/// from `get_cursor_visible`, color from `get_cursor_fg` (falls back to
/// the theme foreground when the field is at default).
fn draw_cursor(ui: &mut egui::Ui, core: &TerminalCore, theme: &Theme, app: &App) {
    if !core.get_cursor_visible() {
        return;
    }
    // Blink only when focused. An unfocused window holds the cursor at
    // its "on" phase so the steady outline is always visible — matches
    // WezTerm.
    if app.window_focused {
        let blink_enabled = core.get_cursor_blink();
        if !app.blink_visible_now(blink_enabled) {
            return;
        }
    }

    // Pin the cursor origin to the *same* logical-px anchor the wgpu
    // grid pass uses (see `window_host::cell_metrics_px`: origin =
    // `(LEFT_PAD, TITLE_BAR + TAB_BAR + status_top + TOP_PAD) * scale`).
    // Reading the origin from `ui.min_rect().min` introduced a
    // couple-pixel drift whenever egui's central panel added implicit
    // padding, which made the block cursor visibly overflow the
    // bottom of its cell. Status-bar top inset is omitted here on
    // purpose: the egui cursor overlay is painted inside the central
    // panel whose `min_rect` is already pushed down by the egui
    // top-status panel, so adding the inset would double-count it.
    let pad = app.settings.padding as f32;
    let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.settings.show_tab_bar);
    let origin = Pos2::new(pad, crate::ui::title_bar::TITLE_BAR_HEIGHT + tab_h + pad);
    let painter = ui.painter();

    let cell_w = app.cell_w_logical;
    let cell_h = app.cell_h_logical;
    let cx = origin.x + core.get_cursor_col() as f32 * cell_w;
    let cy = origin.y + core.get_cursor_row() as f32 * cell_h;

    let cursor_color = packed_to_egui(core.get_cursor_fg(), theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));

    match core.get_cursor_style() {
        // 1 = underline. term_core clamps to 0..=2; once parser routes for
        // DECSCUSR land the mapping (block / underline / bar) becomes
        // observable here.
        1 => {
            let uy = cy + cell_h - 2.0;
            painter.line_segment(
                [Pos2::new(cx, uy), Pos2::new(cx + cell_w, uy)],
                Stroke::new(2.0, cursor_color),
            );
        }
        2 => {
            // Vertical bar at the left edge of the cell.
            painter.line_segment(
                [Pos2::new(cx, cy), Pos2::new(cx, cy + cell_h)],
                Stroke::new(2.0, cursor_color),
            );
        }
        _ => {
            // Block cursor: focused → filled cell (the grid pass swaps
            // fg/bg on the cursor cell, see `collect_cell_inputs`'s
            // `block_cursor_cell` param). Unfocused → hollow outline
            // here, matching WezTerm. egui 0.29 lacks
            // `StrokeKind::Inside`, so a centered 1-px stroke would
            // bleed half a pixel above / below the cell — inset the
            // rect by half the stroke width to keep the visible
            // outline flush with the IME reverse-video box.
            if !app.window_focused {
                const STROKE_W: f32 = 1.0;
                let inset = STROKE_W * 0.5;
                let rect = Rect::from_min_size(
                    Pos2::new(cx + inset, cy + inset),
                    Vec2::new(cell_w - STROKE_W, cell_h - STROKE_W),
                );
                painter.rect_stroke(rect, 0.0, Stroke::new(STROKE_W, cursor_color));
            }
        }
    }
}

fn resolve_cell_style(
    core: &TerminalCore,
    theme: &Theme,
    col: u16,
    row: u16,
    selection: Option<&Selection>,
) -> CellStyle {
    let flags = core.get_cell_flags(col, row);
    let packed_fg = core.get_cell_fg(col, row);
    let packed_bg = core.get_cell_bg(col, row);

    let bold = (flags & STYLE_BOLD) != 0;
    let dim = (flags & STYLE_DIM) != 0;
    let italic = (flags & STYLE_ITALIC) != 0;
    let underline = (flags & STYLE_UNDERLINE) != 0;
    // STYLE_BLINK is rendered statically today; cursor blink owns the
    // wake-up cadence. A future sub-phase can multiplex per-cell blink
    // off the same blink_started clock if needed.
    let _blink = (flags & STYLE_BLINK) != 0;
    let reverse = (flags & STYLE_REVERSE) != 0;
    let hidden = (flags & STYLE_HIDDEN) != 0;
    let strikethrough = (flags & STYLE_STRIKETHROUGH) != 0;

    // Reverse: swap source packed colors BEFORE bold-brighten / decoding
    // so the bold-brighten promotion sees the perceived foreground (FR7
    // in the WebView build: bold-brighten is foreground-only and applies
    // *after* reverse).
    let (effective_fg_packed, effective_bg_packed) = if reverse {
        (packed_bg, packed_fg)
    } else {
        (packed_fg, packed_bg)
    };

    // Bold-brightens: when `settings.bold_brightens_ansi_colors` is on
    // and the cell's foreground is an indexed color in `0..8`, promote
    // it to the bright variant (`idx + 8`). Truecolor / default-tag
    // foregrounds are untouched. Mirrors
    // `attributes.ts::getEffectiveForeground` in the WebView build.
    let effective_fg_packed = if bold && theme.bold_brightens_ansi_colors {
        bold_brighten_packed(effective_fg_packed)
    } else {
        effective_fg_packed
    };

    let mut fg = packed_to_egui(effective_fg_packed, theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));
    let mut bg = packed_to_egui(effective_bg_packed, theme.bg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.bg));

    // Selection: invert again on top of any reverse already in effect.
    let selected = selection.map(|s| s.contains(row, col)).unwrap_or(false);
    if selected {
        std::mem::swap(&mut fg, &mut bg);
    }

    // Dim: 50% alpha against the cell's background. We approximate by
    // pulling fg halfway toward bg; this preserves opacity so subsequent
    // overlay primitives (underline / strikethrough) still respect the
    // dim look without alpha-compositing tricks.
    if dim {
        fg = blend_toward(fg, bg, 0.5);
    }

    // Hidden / conceal: clamp fg to bg so the glyph is invisible. We do
    // this last so reverse / selection still produce the expected
    // background swatch.
    if hidden {
        fg = bg;
    }

    CellStyle {
        fg,
        bg,
        bold,
        italic,
        underline,
        strikethrough,
    }
}

/// Linear blend two RGBA colors. `t = 0.0` returns `a`; `t = 1.0` returns
/// `b`. Used for the dim attribute fallback.
fn blend_toward(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |x: u8, y: u8| -> u8 {
        let f = x as f32 + (y as f32 - x as f32) * t;
        f.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        a.a(),
    )
}

/// Compute display width of a grapheme under the active ambiguous-width
/// policy. Returns at least 1 so the iterator never wedges.
fn visible_width(ch: &str, mode: AmbiguousWidthMode) -> u8 {
    let cp = ch.chars().next().map(|c| c as u32).unwrap_or(0);
    if cp == 0 {
        return 1;
    }
    if is_ambiguous_width(cp) {
        return mode.width_for_ambiguous();
    }
    let w = char_width(cp);
    w.max(1)
}

/// Decode `term_core::cell::PackedColor::to_u32()` into an egui color.
/// Returns `None` only for the `Default` tag, in which case the caller
/// substitutes the active palette fallback. `tag` legend:
/// `0`=default, `1`=indexed (the index lives in `r`), `2`=truecolor RGB.
/// Promote indexed-color packed value 0-7 → 8-15 (xterm "bold brightens"
/// behavior). Truecolor / default-tag values pass through unchanged so
/// the caller can apply this unconditionally to bolded foregrounds.
fn bold_brighten_packed(packed: u32) -> u32 {
    let tag = (packed >> 24) as u8;
    if tag != 1 {
        return packed;
    }
    let idx = (packed >> 16) as u8;
    if idx >= 8 {
        return packed;
    }
    // Clear the old index byte and write idx+8 back into the same slot.
    (packed & 0xFF00_FFFF) | ((idx as u32 + 8) << 16)
}

fn packed_to_egui(packed: u32, _fallback: Rgb, theme: &Theme) -> Option<Color32> {
    let tag = (packed >> 24) as u8;
    let r = (packed >> 16) as u8;
    let g = (packed >> 8) as u8;
    let b = packed as u8;
    match tag {
        0 => None,
        1 => Some(rgb_to_egui(palette_lookup(theme, r))),
        2 => Some(Color32::from_rgb(r, g, b)),
        _ => None,
    }
}

fn rgb_to_egui(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// Resolve a palette index to an `Rgb`. Indices 0..16 come from the
/// active theme's 16-color palette (which OSC 4 / OSC 104 will later
/// mutate); 16..256 use the standard xterm 6x6x6 cube + grayscale ramp.
fn palette_lookup(theme: &Theme, idx: u8) -> Rgb {
    if (idx as usize) < 16 {
        theme.palette16[idx as usize]
    } else {
        palette_256(idx)
    }
}

/// Standard xterm 256-color palette mapping for indices 16..255.
fn palette_256(idx: u8) -> Rgb {
    if idx < 16 {
        Theme::default().palette16[idx as usize]
    } else if idx < 232 {
        // 6x6x6 color cube.
        let i = idx - 16;
        let r = i / 36;
        let g = (i % 36) / 6;
        let b = i % 6;
        let to_byte = |n: u8| -> u8 {
            if n == 0 {
                0
            } else {
                55 + n * 40
            }
        };
        Rgb(to_byte(r), to_byte(g), to_byte(b))
    } else {
        // Grayscale ramp.
        let n = idx - 232;
        let v = 8 + n * 10;
        Rgb(v, v, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_narrow_for_ascii() {
        assert_eq!(visible_width("A", AmbiguousWidthMode::Narrow), 1);
        assert_eq!(visible_width("a", AmbiguousWidthMode::Wide), 1);
    }

    #[test]
    fn visible_width_wide_for_cjk() {
        // U+4E00 is "wide" unconditionally — both modes must report 2.
        assert_eq!(visible_width("一", AmbiguousWidthMode::Narrow), 2);
        assert_eq!(visible_width("一", AmbiguousWidthMode::Wide), 2);
    }

    #[test]
    fn visible_width_respects_ambiguous_mode() {
        // U+25A0 (BLACK SQUARE) is in the Unicode "Ambiguous" East-Asian
        // width class.
        assert_eq!(visible_width("■", AmbiguousWidthMode::Narrow), 1);
        assert_eq!(visible_width("■", AmbiguousWidthMode::Wide), 2);
    }

    #[test]
    fn visible_width_minimum_one_for_empty_or_combining() {
        assert_eq!(visible_width("", AmbiguousWidthMode::Narrow), 1);
        // U+0301 (combining acute accent) reports width 0 from
        // display_width; visible_width must floor to 1 so iteration
        // makes progress.
        assert_eq!(visible_width("\u{0301}", AmbiguousWidthMode::Narrow), 1);
    }

    #[test]
    fn blend_toward_endpoints_match_inputs() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(blend_toward(a, b, 0.0), a);
        assert_eq!(blend_toward(a, b, 1.0).r(), 255);
    }

    #[test]
    fn blend_toward_midpoint_is_average() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(200, 100, 50);
        let m = blend_toward(a, b, 0.5);
        assert_eq!(m.r(), 100);
        assert_eq!(m.g(), 50);
        assert_eq!(m.b(), 25);
    }

    #[test]
    fn bold_brighten_packed_promotes_indexed_0_7() {
        // tag=1 (indexed), index=3 (yellow) → index=11 (bright yellow)
        let packed_red = (1u32 << 24) | (1u32 << 16);
        assert_eq!(
            bold_brighten_packed(packed_red),
            (1u32 << 24) | (9u32 << 16)
        );

        let packed_yellow = (1u32 << 24) | (3u32 << 16);
        assert_eq!(
            bold_brighten_packed(packed_yellow),
            (1u32 << 24) | (11u32 << 16)
        );
    }

    #[test]
    fn bold_brighten_packed_leaves_already_bright_alone() {
        // index 8..16 are already bright; pass through unchanged.
        let packed = (1u32 << 24) | (10u32 << 16);
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn bold_brighten_packed_leaves_truecolor_alone() {
        // tag=2 (truecolor); RGB bits live where the indexed-form `index`
        // byte does, so blindly adding 8 would corrupt the red channel.
        let packed = (2u32 << 24) | 0x00_AA_BB_CC;
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn bold_brighten_packed_leaves_default_tag_alone() {
        // tag=0 (default fg). bold_brighten must not mutate.
        let packed = 0u32;
        assert_eq!(bold_brighten_packed(packed), packed);
    }

    #[test]
    fn packed_to_egui_default_returns_none() {
        let theme = Theme::default();
        assert!(packed_to_egui(0x00_00_00_00, Rgb::WHITE, &theme).is_none());
    }

    #[test]
    fn packed_to_egui_indexed_uses_theme_palette() {
        let theme = Theme::default();
        // index = 1 (red) → palette16[1] = WezTerm scheme Rgb(0xff, 0x00, 0x00).
        let packed = 0x01_01_00_00; // tag=1, r=1
        let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
        assert_eq!(c.r(), 0xff);
        assert_eq!(c.g(), 0x00);
        assert_eq!(c.b(), 0x00);
    }

    #[test]
    fn packed_to_egui_truecolor_returns_exact_rgb() {
        let theme = Theme::default();
        let packed = 0x02_AA_BB_CC; // tag=2, r=AA, g=BB, b=CC
        let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
        assert_eq!((c.r(), c.g(), c.b()), (0xAA, 0xBB, 0xCC));
    }

    // ── font-swash-migration: Theme dead_code resolution (FR10) ────────

    /// TS-font-11: `Theme::default().font_family` is `"monospace"` and
    /// `font_size_pt` is `13.0` (regression guard).
    #[test]
    fn theme_default_font_family_is_monospace() {
        let t = Theme::default();
        assert_eq!(t.font_family, "monospace");
        assert!((t.font_size_pt - 13.0).abs() < f32::EPSILON);
    }

    // ── Phase 4-H: collect_cell_inputs ────────────────────────────────

    /// `collect_cell_inputs` produces exactly `cols * rows` entries —
    /// one per logical cell — even when the grid is mostly blank. The
    /// `TerminalGridPass::build_instances` consumer filters
    /// whitespace / empty clusters internally, so the renderer can
    /// pass the full grid through without an extra pre-filter pass.
    #[test]
    fn collect_cell_inputs_emits_one_entry_per_cell() {
        let mut core = TerminalCore::new(5, 2, 100);
        core.process_pty_data(b"ABCDE");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(&core, &theme, None, AmbiguousWidthMode::Narrow, None);
        // 5 cols × 2 rows = 10 cell entries.
        assert_eq!(inputs.len(), 10);
        // Row 0 should carry the literal glyphs in column order.
        let row0: String = inputs
            .iter()
            .filter(|c| c.row == 0)
            .map(|c| c.glyph.as_str())
            .collect();
        assert_eq!(row0, "ABCDE");
    }

    /// Wide CJK cells advance the iterator by two columns and report
    /// `width_cells = 2`. The cell at `col+1` would normally be the
    /// trailing half of the wide glyph; `collect_cell_inputs` skips it
    /// (`col` advances past it) so a single instance covers the whole
    /// wide rectangle.
    #[test]
    fn collect_cell_inputs_handles_wide_cells() {
        let mut core = TerminalCore::new(4, 1, 100);
        core.process_pty_data("あA".as_bytes());
        let theme = Theme::default();
        let inputs = collect_cell_inputs(&core, &theme, None, AmbiguousWidthMode::Narrow, None);
        assert_eq!(inputs[0].glyph, "あ");
        assert_eq!(inputs[0].width_cells, 2);
        // Column 2 holds the 'A'; column 1 was skipped (trailing half of あ).
        let a = inputs.iter().find(|c| c.glyph == "A").expect("A present");
        assert_eq!(a.col, 2);
        assert_eq!(a.width_cells, 1);
    }

    /// Decoration flags propagate from `STYLE_UNDERLINE` /
    /// `STYLE_STRIKETHROUGH` SGR bits onto the `CellInput`.
    #[test]
    fn collect_cell_inputs_propagates_decoration_flags() {
        let mut core = TerminalCore::new(3, 1, 100);
        // SGR 4 = underline; SGR 9 = strikethrough.
        core.process_pty_data(b"\x1b[4mU\x1b[0m\x1b[9mS\x1b[0mN");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(&core, &theme, None, AmbiguousWidthMode::Narrow, None);
        let u = inputs.iter().find(|c| c.glyph == "U").expect("U present");
        let s = inputs.iter().find(|c| c.glyph == "S").expect("S present");
        let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
        assert!(u.underline);
        assert!(!u.strikethrough);
        assert!(s.strikethrough);
        assert!(!s.underline);
        assert!(!n.underline);
        assert!(!n.strikethrough);
    }

    /// Non-default background colors set `draw_background = true`; the
    /// default-background cells leave it `false` so the wgpu pass can
    /// skip the background quad (the swapchain clear covers it).
    #[test]
    fn collect_cell_inputs_draw_background_only_when_non_default() {
        let mut core = TerminalCore::new(3, 1, 100);
        // SGR 41 = red background.
        core.process_pty_data(b"\x1b[41mR\x1b[0mN");
        let theme = Theme::default();
        let inputs = collect_cell_inputs(&core, &theme, None, AmbiguousWidthMode::Narrow, None);
        let r = inputs.iter().find(|c| c.glyph == "R").expect("R present");
        let n = inputs.iter().find(|c| c.glyph == "N").expect("N present");
        assert!(r.draw_background);
        assert!(!n.draw_background);
    }
}
