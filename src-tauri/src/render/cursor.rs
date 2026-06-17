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

use egui::{Color32, Pos2, Stroke};

use crate::ime::preedit::Anchor;

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
}
