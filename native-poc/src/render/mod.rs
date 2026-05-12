//! Grid → egui draw routines.
//!
//! Phase 6 swap: the renderer reads the grid through `term_core` accessors
//! (`get_cell_char`, `get_cell_fg/bg/flags`, `get_cursor_*`) instead of the
//! Phase 1 PoC's bespoke `Grid` type. Colors are decoded from the packed
//! `u32` returned by `get_cell_fg/bg`.

pub mod theme;

use egui::{Align2, Color32, FontFamily, FontId, Pos2, Rect, Stroke, Vec2};
use term_core::cell::STYLE_REVERSE;
use term_core::terminal_core::TerminalCore;

use crate::app::App;
use crate::render::theme::{Rgb, Theme};
use crate::selection::Selection;

const CELL_W: f32 = 8.5; // logical pixels per cell
const CELL_H: f32 = 17.0;
const FONT_SIZE: f32 = 13.0;
const TOP_PAD: f32 = 4.0;
const LEFT_PAD: f32 = 4.0;

/// Phase-1 placeholder kept for compatibility; routes to the real renderer
/// when a tab exists.
pub fn draw_placeholder(ctx: &egui::Context, app: &App) {
    draw_terminal(ctx, app);
}

/// Draw the active tab. If no tabs exist, draws a hint message.
pub fn draw_terminal(ctx: &egui::Context, app: &App) {
    let theme = Theme::default();

    egui::TopBottomPanel::top("native-poc-top-bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, tab) in app.tabs.iter().enumerate() {
                let label = if i == app.active {
                    format!("[{}]", tab.title)
                } else {
                    tab.title.clone()
                };
                ui.label(label);
                ui.separator();
            }
            ui.label(format!("{}x{}", app.cell_size.cols, app.cell_size.rows));
        });
    });

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(rgb_to_egui(theme.bg)))
        .show(ctx, |ui| {
            if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                draw_grid(ui, &core, app.selection.as_ref(), &theme);
            } else {
                ui.colored_label(Color32::LIGHT_GRAY, "no tab — shell may have exited");
            }
        });
}

fn draw_grid(ui: &mut egui::Ui, core: &TerminalCore, selection: Option<&Selection>, theme: &Theme) {
    let origin = ui.min_rect().min + Vec2::new(LEFT_PAD, TOP_PAD);
    let painter = ui.painter();

    let font_id = FontId::new(FONT_SIZE, FontFamily::Monospace);

    let cols = core.cols();
    let rows = core.rows();

    for row in 0..rows {
        for col in 0..cols {
            // Compute pixel rect for this cell.
            let x = origin.x + col as f32 * CELL_W;
            let y = origin.y + row as f32 * CELL_H;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(CELL_W, CELL_H));

            let flags = core.get_cell_flags(col, row);
            let fg = packed_to_egui(core.get_cell_fg(col, row), theme.fg, theme);
            let bg = packed_to_egui(core.get_cell_bg(col, row), theme.bg, theme);
            let (fg, bg) = if (flags & STYLE_REVERSE) != 0 {
                (bg, fg)
            } else {
                (fg, bg)
            };

            // Selection inverts foreground/background.
            let selected = selection.map(|s| s.contains(row, col)).unwrap_or(false);
            let (fg, bg) = if selected { (bg, fg) } else { (fg, bg) };

            if bg != rgb_to_egui(theme.bg) {
                painter.rect_filled(rect, 0.0, bg);
            } else if selected {
                painter.rect_filled(rect, 0.0, bg);
            }

            let ch = core.get_cell_char(col, row);
            if !ch.is_empty() && ch != " " {
                painter.text(Pos2::new(x, y), Align2::LEFT_TOP, ch, font_id.clone(), fg);
            }
        }
    }

    // Block cursor on top.
    let cursor_col = core.get_cursor_col();
    let cursor_row = core.get_cursor_row();
    let cx = origin.x + cursor_col as f32 * CELL_W;
    let cy = origin.y + cursor_row as f32 * CELL_H;
    let cursor_rect = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(CELL_W, CELL_H));
    painter.rect_stroke(
        cursor_rect,
        0.0,
        Stroke::new(1.0, Color32::from_white_alpha(180)),
    );
}

/// Decode `term_core::cell::PackedColor::to_u32()` format inline so we do not
/// depend on the (test-only) `PackedColor::from_u32` helper.
///
/// Layout: `(tag << 24) | (r << 16) | (g << 8) | b`. `tag` legend:
/// `0`=default, `1`=indexed (the index lives in `r`), `2`=truecolor RGB.
fn packed_to_egui(packed: u32, fallback: Rgb, theme: &Theme) -> Color32 {
    let tag = (packed >> 24) as u8;
    let r = (packed >> 16) as u8;
    let g = (packed >> 8) as u8;
    let b = packed as u8;
    match tag {
        0 => rgb_to_egui(fallback),
        1 => {
            // Indexed (0..255). `r` field holds the index.
            if (r as usize) < 16 {
                rgb_to_egui(theme.palette16[r as usize])
            } else {
                rgb_to_egui(palette_256(r))
            }
        }
        2 => Color32::from_rgb(r, g, b),
        _ => rgb_to_egui(fallback),
    }
}

fn rgb_to_egui(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
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
