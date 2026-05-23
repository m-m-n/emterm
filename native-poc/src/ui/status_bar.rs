//! Status-bar widget.
//!
//! Renders a 3-row [`egui::TopBottomPanel`] (top or bottom, per
//! [`crate::settings::StatusBarPosition`]). Rows top-to-bottom:
//!
//! 1. **App Line 1** — local templates resolved by the
//!    `TemplateEngine` (typically `{time}` / `{cwd}`).
//! 2. **App Line 2** — second template row, auto-hidden when both
//!    sides are empty (FR12).
//! 3. **OSC row** — mux daemon's `StatusUpdateMsg` if the active tab
//!    is attached, otherwise the OSC 777;statusbar dispatcher's
//!    layer state. Auto-hidden when empty unless `show` was
//!    requested.
//!
//! The widget is pure over [`StatusBarViewModel`]; the render
//! pipeline projects the active tab + runtime state into the view
//! model once per frame.

use egui::{Align, Color32, FontFamily, FontId, Layout, RichText};

use crate::html::{CssColor, RichTextRun};
use crate::settings::StatusBarPosition;
use crate::status_bar::{AppRow, OscRow, StatusBarViewModel};

/// Per-row visual height in egui logical points. Three rows render
/// stacked; the panel height multiplies this by the number of
/// visible rows.
pub const ROW_HEIGHT: f32 = 22.0;
/// Default font size for App rows; OSC row also uses this unless the
/// view model overrides `font_size`.
const DEFAULT_FONT_SIZE: f32 = 12.0;

/// Number of rows the status bar will paint for `view_model` this
/// frame (0 when disabled or every row is auto-hidden).
pub fn visible_row_count(view_model: &StatusBarViewModel) -> u32 {
    if !view_model.enabled {
        return 0;
    }
    let has_mux = view_model.mux_session_name.is_some();
    let osc_visible = view_model.osc.should_render(has_mux);
    let app1_visible = true; // FR12: App Line 1 always renders.
    let app2_visible = view_model.app_line2.has_content();
    (osc_visible as u32) + (app1_visible as u32) + (app2_visible as u32)
}

/// Panel height in egui logical points. The terminal grid layout uses
/// this to reserve room above/below the cell area so the bottom row
/// never gets covered by the status-bar panel (and, when the panel
/// sits on top, so cells don't render behind it).
pub fn panel_height_logical(view_model: &StatusBarViewModel) -> f32 {
    ROW_HEIGHT * visible_row_count(view_model) as f32
}

/// Render the status bar. Returns immediately (no panel inserted)
/// when `view_model.enabled` is false.
pub fn draw(ctx: &egui::Context, view_model: &StatusBarViewModel) {
    let visible_rows = visible_row_count(view_model);
    if visible_rows == 0 {
        return;
    }

    let mut panel = match view_model.position {
        StatusBarPosition::Top => egui::TopBottomPanel::top("native-poc-status-bar"),
        StatusBarPosition::Bottom => egui::TopBottomPanel::bottom("native-poc-status-bar"),
    };
    panel = panel.exact_height(ROW_HEIGHT * visible_rows as f32);

    let app1_visible = true;
    let app2_visible = view_model.app_line2.has_content();
    let has_mux = view_model.mux_session_name.is_some();
    let osc_visible = view_model.osc.should_render(has_mux);

    let font_size = view_model.font_size.unwrap_or(DEFAULT_FONT_SIZE);

    panel.show(ctx, |ui| {
        ui.vertical(|ui| {
            // Order top-to-bottom regardless of panel placement.
            // App rows go above the OSC row so the daemon status
            // sits closest to the screen edge (matches WebView).
            if app1_visible {
                draw_app_row(ui, &view_model.app_line1, font_size);
            }
            if app2_visible {
                draw_app_row(ui, &view_model.app_line2, font_size);
            }
            if osc_visible {
                draw_osc_row(
                    ui,
                    &view_model.osc,
                    view_model.mux_session_name.as_deref(),
                    font_size,
                );
            }
        });
    });
}

/// Render an App row: left runs flow left-to-right, right runs flow
/// right-to-left. Shared with App Line 1 / 2.
fn draw_app_row(ui: &mut egui::Ui, row: &AppRow, font_size: f32) {
    let font = FontId::new(font_size, FontFamily::Monospace);
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.spacing_mut().item_spacing.x = 8.0;
        // Left side, in source order.
        draw_runs(ui, &row.left, &font);
        // Right side aligned to the panel edge.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Iterating left-to-right in a right-to-left layout
            // produces the visually-expected right-aligned order
            // when the runs are short; for longer run lists we
            // reverse so the source order reads left-to-right on
            // screen.
            for run in row.right.iter().rev() {
                emit_run(ui, run, &font);
            }
        });
    });
}

/// Render the OSC row. The mux session badge (`[mux:<name>]`) is
/// prepended to the left side when present.
fn draw_osc_row(ui: &mut egui::Ui, row: &OscRow, mux_session_name: Option<&str>, font_size: f32) {
    let font = FontId::new(font_size, FontFamily::Monospace);
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(name) = mux_session_name {
            ui.label(
                RichText::new(format!("[mux:{}]", name))
                    .strong()
                    .font(font.clone()),
            );
        }
        if !row.left.is_empty() {
            // OSC row text is post-strip plain text — render as a
            // single label.
            ui.label(RichText::new(&row.left).font(font.clone()));
        }
        if !row.right.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(&row.right)
                        .font(font.clone())
                        .color(Color32::LIGHT_GRAY),
                );
            });
        }
    });
}

fn draw_runs(ui: &mut egui::Ui, runs: &[RichTextRun], font: &FontId) {
    for run in runs {
        emit_run(ui, run, font);
    }
}

fn emit_run(ui: &mut egui::Ui, run: &RichTextRun, font: &FontId) {
    if run.line_break {
        // We render run lists into a single horizontal strip, so
        // line breaks degrade to a fixed-width gap. Multi-line OSC
        // payloads are not in scope for the status bar.
        ui.add_space(8.0);
        return;
    }
    if run.text.is_empty() {
        return;
    }
    let mut rt = RichText::new(&run.text).font(font.clone());
    if run.bold {
        rt = rt.strong();
    }
    if run.italic {
        rt = rt.italics();
    }
    if run.underline {
        rt = rt.underline();
    }
    if let Some(color) = &run.color {
        rt = rt.color(css_color_to_color32(color));
    }
    ui.label(rt);
}

fn css_color_to_color32(color: &CssColor) -> Color32 {
    color.to_egui().unwrap_or(Color32::LIGHT_GRAY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_bar::{AppRow, OscRow, StatusBarViewModel};
    use egui::RawInput;

    fn make_text_run(text: &str) -> RichTextRun {
        RichTextRun {
            text: text.to_string(),
            bold: false,
            italic: false,
            underline: false,
            color: None,
            line_break: false,
        }
    }

    fn collected_text(items: &[egui::epaint::ClippedShape]) -> String {
        let mut out = String::new();
        for cs in items {
            walk_shape(&cs.shape, &mut out);
        }
        out
    }

    fn walk_shape(shape: &egui::epaint::Shape, out: &mut String) {
        use egui::epaint::Shape;
        match shape {
            Shape::Text(t) => {
                for row in &t.galley.rows {
                    for g in &row.glyphs {
                        out.push(g.chr);
                    }
                    out.push('\n');
                }
            }
            Shape::Vec(v) => {
                for s in v {
                    walk_shape(s, out);
                }
            }
            _ => {}
        }
    }

    fn run_one_frame(vm: &StatusBarViewModel) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 200.0),
        ));
        let output = ctx.run(input, |ctx| {
            draw(ctx, vm);
            egui::CentralPanel::default().show(ctx, |_ui| {});
        });
        output.shapes
    }

    fn run_with_central_rect(
        vm: &StatusBarViewModel,
    ) -> (Vec<egui::epaint::ClippedShape>, egui::Rect) {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 200.0));
        let mut input = RawInput::default();
        input.screen_rect = Some(screen);
        let mut central_rect = egui::Rect::NOTHING;
        let output = ctx.run(input, |ctx| {
            draw(ctx, vm);
            egui::CentralPanel::default().show(ctx, |ui| {
                central_rect = ui.max_rect();
            });
        });
        (output.shapes, central_rect)
    }

    // TS-23 (replacement): disabled view model inserts no panel.
    #[test]
    fn disabled_view_model_does_not_insert_panel() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = false;
        let (_shapes, central_off) = run_with_central_rect(&vm);

        let mut vm_on = StatusBarViewModel::default();
        vm_on.enabled = true;
        vm_on.app_line1.left = vec![make_text_run("hi")];
        let (_shapes_on, central_on) = run_with_central_rect(&vm_on);

        assert!(
            central_off.height() > central_on.height(),
            "disabled status bar must leave the central panel taller \
             (off={central_off:?}, on={central_on:?})"
        );
    }

    // TS-24: App Line 1 always renders; App Line 2 hidden when empty.
    #[test]
    fn app_line2_auto_hides_when_empty() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run("L1")];
        // app_line2 left/right are empty
        let (_shapes, central_one_row) = run_with_central_rect(&vm);

        let mut vm_two = vm.clone();
        vm_two.app_line2.left = vec![make_text_run("L2")];
        let (_shapes_two, central_two_row) = run_with_central_rect(&vm_two);

        // Adding a second row shrinks the central panel by ROW_HEIGHT.
        assert!(
            central_one_row.height() > central_two_row.height(),
            "Adding App Line 2 must shrink central panel; \
             one_row={central_one_row:?} two_row={central_two_row:?}"
        );
    }

    // TS-25: OSC row populated from mux state shows session badge.
    #[test]
    fn mux_session_renders_badge_and_osc_text() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run("L1")];
        vm.mux_session_name = Some("main".to_string());
        vm.osc = OscRow {
            left: "1:shell 2:nvim*".to_string(),
            right: "host01".to_string(),
            forced_visible: Some(true),
        };
        let shapes = run_one_frame(&vm);
        let text = collected_text(&shapes);
        assert!(
            text.contains("[mux:main]"),
            "session badge missing: {text:?}"
        );
        assert!(text.contains("1:shell"), "window list missing: {text:?}");
        assert!(text.contains("host01"), "right segment missing: {text:?}");
    }

    // TS-26: OSC row hidden when no content and no mux session.
    #[test]
    fn osc_row_hidden_when_empty_and_no_mux() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run("only_app_row")];
        let shapes = run_one_frame(&vm);
        let text = collected_text(&shapes);
        // The text must show app row but no `[mux:` prefix.
        assert!(text.contains("only_app_row"));
        assert!(!text.contains("[mux:"));
    }

    // OSC row sourced from the dispatcher (no mux) shows even
    // without a session badge.
    #[test]
    fn osc_row_from_dispatcher_renders_without_mux_badge() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run("L1")];
        vm.osc = OscRow {
            left: "manual-left".to_string(),
            right: "manual-right".to_string(),
            forced_visible: Some(true),
        };
        let shapes = run_one_frame(&vm);
        let text = collected_text(&shapes);
        assert!(text.contains("manual-left"));
        assert!(text.contains("manual-right"));
        assert!(!text.contains("[mux:"));
    }

    // Enabled view model with content reserves panel height.
    #[test]
    fn enabled_status_bar_reserves_panel_height() {
        let mut vm_off = StatusBarViewModel::default();
        vm_off.enabled = false;
        let mut vm_on = StatusBarViewModel::default();
        vm_on.enabled = true;
        vm_on.app_line1.left = vec![make_text_run("x")];
        let (_, central_off) = run_with_central_rect(&vm_off);
        let (_, central_on) = run_with_central_rect(&vm_on);
        assert!(
            central_off.height() > central_on.height(),
            "enabling the status bar must shrink the central panel \
             (off={central_off:?}, on={central_on:?})"
        );
    }

    // Both forced_visible=Some(false) skips OSC even when content is
    // present.
    #[test]
    fn osc_force_hide_skips_row() {
        let mut vm = StatusBarViewModel::default();
        vm.enabled = true;
        vm.app_line1.left = vec![make_text_run("L1")];
        vm.osc = OscRow {
            left: "hidden".to_string(),
            right: String::new(),
            forced_visible: Some(false),
        };
        let shapes = run_one_frame(&vm);
        let text = collected_text(&shapes);
        assert!(!text.contains("hidden"));
    }
}
