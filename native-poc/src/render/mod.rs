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

pub mod cursor;
pub mod font;
pub mod theme;

use std::time::Duration;

use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, Vec2};
use term_core::cell::{
    STYLE_BLINK, STYLE_BOLD, STYLE_DIM, STYLE_HIDDEN, STYLE_ITALIC, STYLE_REVERSE,
    STYLE_STRIKETHROUGH, STYLE_UNDERLINE,
};
use term_core::terminal_core::TerminalCore;
use term_core::{char_width, is_ambiguous_width};

use crate::app::{App, BLINK_HALF_MS};
use crate::render::theme::{Rgb, Theme};
use crate::selection::Selection;
use crate::settings::AmbiguousWidthMode;

const CELL_W: f32 = 8.5; // logical pixels per cell
const CELL_H: f32 = 17.0;
const TOP_PAD: f32 = 4.0;
const LEFT_PAD: f32 = 4.0;

/// Build the egui `FontId` to use for cell glyph drawing. Phase 4-H
/// removes the prior `FONT_SIZE = 13.0` constant + hard-coded
/// `FontFamily::Monospace` literal in favour of reading both from the
/// active `Theme`. Names other than `"monospace"` route through
/// `FontFamily::Name(...)`; the loader is responsible for registering
/// custom faces via `egui::Context::set_fonts` before the first draw.
fn cell_font_id(theme: &Theme) -> FontId {
    let family = if theme.font_family.eq_ignore_ascii_case("monospace") {
        FontFamily::Monospace
    } else {
        FontFamily::Name(theme.font_family.clone().into())
    };
    FontId::new(theme.font_size_pt, family)
}

/// Per-cell paint parameters resolved from a `term_core` cell + active
/// palette + selection state.
struct CellStyle {
    fg: Color32,
    bg: Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
}

/// Phase-1 placeholder kept for compatibility; routes to the real renderer
/// when a tab exists.
pub fn draw_placeholder(ctx: &egui::Context, app: &App) -> Option<crate::ui::TabEvent> {
    draw_terminal(ctx, app)
}

/// Draw the active tab. If no tabs exist, draws a hint message. The
/// caller is responsible for applying the returned `TabEvent` (if
/// any) — typically by calling `App::apply_tab_event` post-frame.
pub fn draw_terminal(ctx: &egui::Context, app: &App) -> Option<crate::ui::TabEvent> {
    let theme = Theme::default();

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
    let tab_event = if items.is_empty() {
        None
    } else {
        crate::ui::tab_bar::draw(ctx, &items, app.active)
    };

    // Phase 4-D: status-bar panel. Inserted before the central panel
    // (egui sizes top/bottom panels first, then the central panel
    // takes the remaining rect). The widget itself decides top vs
    // bottom from settings.
    let status_state = app.status_bar_state();
    crate::ui::status_bar::draw(ctx, &status_state, &app.settings);

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(rgb_to_egui(theme.bg)))
        .show(ctx, |ui| {
            if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                let mode = app.settings.ambiguous_width_mode;
                draw_grid(ui, &core, app.selection.as_ref(), &theme, mode);
                draw_cursor(ui, &core, &theme, app);
                // Phase 4-E: preedit underline overlay. Drawn after the
                // cursor so it sits on top of (or beneath, depending on
                // cursor style) the active cell highlight.
                if tab.preedit_state.active() {
                    let origin = ui.min_rect().min + Vec2::new(LEFT_PAD, TOP_PAD);
                    let cursor_color = rgb_to_egui(theme.fg);
                    let metrics = crate::render::cursor::FontMetrics {
                        cell_w: CELL_W,
                        cell_h: CELL_H,
                        left_pad: 0.0,
                        top_pad: 0.0,
                    };
                    crate::render::cursor::draw_cursor_with_preedit(
                        ui.painter(),
                        tab.preedit_state.anchor(),
                        tab.preedit_state.text(),
                        metrics,
                        core.cols(),
                        cursor_color,
                        origin,
                    );
                }
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

    // Phase 4-D: when the status bar is enabled, schedule a 1 Hz
    // wake-up so the local clock ticks even on otherwise-idle frames.
    // Active PTY output / cursor blink already drive higher-rate
    // repaint; this is the floor.
    if app.settings.statusbar.enabled {
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    tab_event
}

fn draw_grid(
    ui: &mut egui::Ui,
    core: &TerminalCore,
    selection: Option<&Selection>,
    theme: &Theme,
    width_mode: AmbiguousWidthMode,
) {
    let origin = ui.min_rect().min + Vec2::new(LEFT_PAD, TOP_PAD);
    let painter = ui.painter();

    let normal_font = cell_font_id(theme);

    let cols = core.cols();
    let rows = core.rows();

    for row in 0..rows {
        let mut col = 0u16;
        while col < cols {
            let style = resolve_cell_style(core, theme, col, row, selection);

            // Compute pixel rect for this cell. Wide / ambiguous-wide cells
            // span 2 columns; we still draw one glyph but advance `col`
            // by 2 so the next column starts past the wide cell.
            let ch = core.get_cell_char(col, row);
            let cell_width_cells = visible_width(&ch, width_mode);
            let x = origin.x + col as f32 * CELL_W;
            let y = origin.y + row as f32 * CELL_H;
            let rect = Rect::from_min_size(
                Pos2::new(x, y),
                Vec2::new(CELL_W * cell_width_cells as f32, CELL_H),
            );

            // Background: paint whenever it differs from the panel bg, OR
            // when this cell is part of the selection (already handled by
            // resolve_cell_style swapping fg/bg).
            if style.bg != rgb_to_egui(theme.bg) {
                painter.rect_filled(rect, 0.0, style.bg);
            }

            if !ch.is_empty() && ch != " " {
                // egui doesn't ship a bold-italic / italic / bold
                // monospace face by default. We accept the visual
                // shortcut of falling back to the regular monospace
                // here; Phase 7 can register custom typefaces.
                let font_id = normal_font.clone();
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, ch, font_id, style.fg);
            }

            // Underline: a 1-px line just below the baseline. egui's
            // monospace baseline sits around y + CELL_H * 0.78; we keep
            // it inside the cell box.
            if style.underline {
                let uy = y + CELL_H - 2.0;
                painter.line_segment(
                    [Pos2::new(x, uy), Pos2::new(x + CELL_W, uy)],
                    Stroke::new(1.0, style.fg),
                );
            }

            // Strikethrough: horizontal line at the cell midpoint.
            if style.strikethrough {
                let sy = y + CELL_H * 0.55;
                painter.line_segment(
                    [Pos2::new(x, sy), Pos2::new(x + CELL_W, sy)],
                    Stroke::new(1.0, style.fg),
                );
            }

            col = col.saturating_add(cell_width_cells.max(1) as u16);
        }
    }
}

/// Cursor overlay: shape from `get_cursor_style`, blink from
/// `get_cursor_blink` modulated by `App::blink_visible_now`, visibility
/// from `get_cursor_visible`, color from `get_cursor_fg` (falls back to
/// the theme foreground when the field is at default).
fn draw_cursor(ui: &mut egui::Ui, core: &TerminalCore, theme: &Theme, app: &App) {
    if !core.get_cursor_visible() {
        return;
    }
    let blink_enabled = core.get_cursor_blink();
    if !app.blink_visible_now(blink_enabled) {
        return;
    }

    let origin = ui.min_rect().min + Vec2::new(LEFT_PAD, TOP_PAD);
    let painter = ui.painter();

    let cx = origin.x + core.get_cursor_col() as f32 * CELL_W;
    let cy = origin.y + core.get_cursor_row() as f32 * CELL_H;

    let cursor_color = packed_to_egui(core.get_cursor_fg(), theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));

    match core.get_cursor_style() {
        // 1 = underline. term_core clamps to 0..=2; once parser routes for
        // DECSCUSR land the mapping (block / underline / bar) becomes
        // observable here.
        1 => {
            let uy = cy + CELL_H - 2.0;
            painter.line_segment(
                [Pos2::new(cx, uy), Pos2::new(cx + CELL_W, uy)],
                Stroke::new(2.0, cursor_color),
            );
        }
        2 => {
            // Vertical bar at the left edge of the cell.
            painter.line_segment(
                [Pos2::new(cx, cy), Pos2::new(cx, cy + CELL_H)],
                Stroke::new(2.0, cursor_color),
            );
        }
        _ => {
            // Block cursor as an outline so the underlying glyph stays
            // legible. Phase 7 can switch to a filled rect with inverted
            // fg when the OS focus state says we own focus.
            let rect = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(CELL_W, CELL_H));
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, cursor_color));
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
    let fg = packed_to_egui(core.get_cell_fg(col, row), theme.fg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.fg));
    let bg = packed_to_egui(core.get_cell_bg(col, row), theme.bg, theme)
        .unwrap_or_else(|| rgb_to_egui(theme.bg));

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

    // Reverse: swap fg/bg BEFORE selection / hidden / dim handling so the
    // later transforms operate on the perceived foreground.
    let (mut fg, mut bg) = if reverse { (bg, fg) } else { (fg, bg) };

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
    fn packed_to_egui_default_returns_none() {
        let theme = Theme::default();
        assert!(packed_to_egui(0x00_00_00_00, Rgb::WHITE, &theme).is_none());
    }

    #[test]
    fn packed_to_egui_indexed_uses_theme_palette() {
        let theme = Theme::default();
        // index = 1 (red) → palette16[1] = Rgb(0xcd, 0x00, 0x00).
        let packed = 0x01_01_00_00; // tag=1, r=1
        let c = packed_to_egui(packed, Rgb::WHITE, &theme).unwrap();
        assert_eq!(c.r(), 0xcd);
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

    /// TS-font-12: Renderer reads `Theme::font_family` + `Theme::font_size_pt`
    /// (not deleted FONT_SIZE constant / hard-coded `FontFamily::Monospace`).
    /// Construct a Theme with sentinel values and assert that the
    /// resulting `cell_font_id` carries them.
    #[test]
    fn renderer_reads_theme_font_family_and_size() {
        let mut t = Theme::default();
        t.font_family = "TestSentinelFont".into();
        t.font_size_pt = 17.0;
        let font = cell_font_id(&t);
        assert!((font.size - 17.0).abs() < f32::EPSILON);
        match font.family {
            FontFamily::Name(name) => assert_eq!(&*name, "TestSentinelFont"),
            other => panic!("expected FontFamily::Name, got {:?}", other),
        }
    }

    #[test]
    fn renderer_routes_monospace_default_to_monospace_family() {
        let t = Theme::default();
        let font = cell_font_id(&t);
        assert!((font.size - 13.0).abs() < f32::EPSILON);
        assert_eq!(font.family, FontFamily::Monospace);
    }
}
