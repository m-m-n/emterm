//! Reusable Material Design 3 form widgets for egui.
//!
//! Extracted from the settings panel so later phases (keybind editor,
//! mux / notification / Markdown categories, profile dialogs) compose
//! the same components instead of re-styling egui stock widgets per
//! screen. Shapes and tokens follow `doc/UI-DESIGN-GUIDELINES.yaml`:
//!
//! - [`apply_visuals`] — re-skin egui built-ins (sliders, drag values,
//!   popups, selection) with the active md3 palette
//! - [`switch`] — MD3 switch (52×32 track, 16/24px handle)
//! - [`outlined_text_input`] — MD3 outlined text field (1px `outline`
//!   border, 4px radius, 2px `primary` focus border)
//! - [`outlined_select`] / [`outlined_select_frame`] — MD3 outlined
//!   select over an egui ComboBox
//! - [`nav_pill`] — 48px full-radius navigation item (icon + label),
//!   `secondary-container` when selected
//! - [`form_row`] / [`toggle_row`] / [`subsection`] / [`hint`] — the
//!   settings form building blocks (`.settings-row`,
//!   `.settings-row-toggle`)

use egui::{Color32, Rect, RichText, Rounding, Sense, Stroke, Vec2};

use crate::ui::md3;

/// MD3 outlined control width (`.settings-select` / text field
/// max-width).
pub const CONTROL_WIDTH: f32 = 320.0;
/// Width cap for a label/control toggle row (`.settings-row-toggle`).
pub const ROW_MAX_WIDTH: f32 = 480.0;
/// Vertical gap between a form-row label and its control.
pub const ROW_GAP: f32 = 8.0;
/// Bottom margin of a form row (`.settings-row`).
pub const ROW_MARGIN: f32 = 24.0;
/// Height of a navigation pill (`.settings-nav-item`).
pub const NAV_PILL_HEIGHT: f32 = 48.0;
/// Corner radius shared by the outlined controls (corner-extra-small).
const CONTROL_ROUNDING: f32 = 4.0;

/// Re-skin egui's built-in widgets (sliders, drag values, combo
/// popups, text cursors) with the md3 tokens so nothing renders in
/// egui's stock dark-gray look. Call once at the top of an md3-styled
/// container; the change is scoped to that `Ui`.
pub fn apply_visuals(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(md3::on_surface());
    v.widgets.inactive.bg_fill = md3::surface_container_highest();
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, md3::outline());
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, md3::on_surface_variant());
    v.widgets.inactive.rounding = Rounding::same(CONTROL_ROUNDING);
    v.widgets.hovered.bg_fill = md3::surface_container_highest();
    v.widgets.hovered.weak_bg_fill = md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, md3::on_surface());
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, md3::on_surface());
    v.widgets.hovered.rounding = Rounding::same(CONTROL_ROUNDING);
    v.widgets.active.bg_fill = md3::primary();
    v.widgets.active.weak_bg_fill = md3::state_layer(md3::primary(), md3::STATE_LAYER_PRESSED);
    v.widgets.active.bg_stroke = Stroke::new(2.0, md3::primary());
    v.widgets.active.fg_stroke = Stroke::new(1.0, md3::on_surface());
    v.widgets.active.rounding = Rounding::same(CONTROL_ROUNDING);
    v.widgets.open.bg_fill = md3::surface_container_high();
    v.widgets.open.bg_stroke = Stroke::new(1.0, md3::outline());
    v.widgets.open.fg_stroke = Stroke::new(1.0, md3::on_surface());
    v.widgets.open.rounding = Rounding::same(CONTROL_ROUNDING);
    v.selection.bg_fill = md3::state_layer(md3::primary(), 0.35);
    v.selection.stroke = Stroke::new(1.0, md3::primary());
    // Slider rail fills with primary up to the handle.
    v.slider_trailing_fill = true;
    // Combo popup background.
    v.window_fill = md3::surface_container_high();
    v.window_stroke = Stroke::new(1.0, md3::outline_variant());
    v.popup_shadow = egui::epaint::Shadow::NONE;
}

/// MD3 switch (`.settings-toggle`): 52×32 full-radius track. Off =
/// `surface-container-highest` track + 2px `outline` border + 16px
/// `outline` handle; on = `primary` track + 24px `on-primary` handle.
/// Returns true when toggled this frame.
pub fn switch(ui: &mut egui::Ui, value: &mut bool) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(52.0, 32.0), Sense::click());
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    let on = *value;
    let painter = ui.painter();
    let rounding = Rounding::same(16.0);
    if on {
        painter.rect_filled(rect, rounding, md3::primary());
    } else {
        painter.rect_filled(rect, rounding, md3::surface_container_highest());
        painter.rect_stroke(rect.shrink(1.0), rounding, Stroke::new(2.0, md3::outline()));
    }
    let (handle_r, cx, handle_color) = if on {
        (12.0, rect.right() - 16.0, md3::on_primary())
    } else {
        (8.0, rect.left() + 16.0, md3::outline())
    };
    painter.circle_filled(egui::pos2(cx, rect.center().y), handle_r, handle_color);
    resp.changed()
}

/// MD3 outlined text field: transparent fill, 1px `outline` border
/// (4px radius), 2px `primary` border while focused. Returns true when
/// the text changed this frame.
pub fn outlined_text_input(ui: &mut egui::Ui, value: &mut String, width: f32) -> bool {
    let frame = egui::Frame::none()
        .stroke(Stroke::new(1.0, md3::outline()))
        .rounding(Rounding::same(CONTROL_ROUNDING))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .fill(Color32::TRANSPARENT);
    let out = frame.show(ui, |ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(width - 24.0)
                .frame(false)
                .font(egui::TextStyle::Body)
                .text_color(md3::on_surface()),
        )
    });
    let inner = out.inner;
    if inner.has_focus() {
        ui.painter().rect_stroke(
            out.response.rect,
            Rounding::same(CONTROL_ROUNDING),
            Stroke::new(2.0, md3::primary()),
        );
    } else if out.response.hovered() {
        ui.painter().rect_stroke(
            out.response.rect,
            Rounding::same(CONTROL_ROUNDING),
            Stroke::new(1.0, md3::on_surface()),
        );
    }
    inner.changed()
}

/// MD3 visuals for the *interior* of a popup (ComboBox menu, context
/// menu). Popups render in their own egui Area and inherit the global
/// style, not the md3-skinned panel `Ui`, so every popup closure must
/// re-apply the tokens itself.
pub fn apply_popup_visuals(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(md3::on_surface());
    // selectable_value rows: selected fill + hover state layer.
    v.selection.bg_fill = md3::state_layer(md3::primary(), 0.35);
    v.selection.stroke = Stroke::new(1.0, md3::primary());
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.weak_bg_fill = md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = md3::state_layer(md3::primary(), md3::STATE_LAYER_PRESSED);
    v.widgets.active.bg_stroke = Stroke::NONE;
}

/// MD3 outlined select over a fixed `(value, label)` set. Returns true
/// when the selection changed.
pub fn outlined_select<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id: &str,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    let current_label = options
        .iter()
        .find(|(v, _)| v == current)
        .map(|(_, l)| *l)
        .unwrap_or("");
    outlined_select_frame(ui, |ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(
                RichText::new(current_label)
                    .size(14.0)
                    .color(md3::on_surface()),
            )
            .width(CONTROL_WIDTH - 24.0)
            .show_ui(ui, |ui| {
                apply_popup_visuals(ui);
                for (value, label) in options {
                    if ui.selectable_value(current, *value, *label).changed() {
                        changed = true;
                    }
                }
            });
    });
    changed
}

/// Outline wrapper for an egui ComboBox so the closed select reads as
/// an MD3 outlined field (egui's stock combo button is a filled gray
/// pill). The inner margin + interact height match
/// [`outlined_text_input`]'s box so selects and text fields align.
pub fn outlined_select_frame(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::none()
        .stroke(Stroke::new(1.0, md3::outline()))
        .rounding(Rounding::same(CONTROL_ROUNDING))
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
        .fill(Color32::TRANSPARENT);
    frame.show(ui, |ui| {
        // The combo button itself goes transparent; the frame above
        // carries the outline. Bump the interact height so the closed
        // select matches the text field's inner box (~20px line).
        ui.spacing_mut().interact_size.y = 20.0;
        let v = ui.visuals_mut();
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.hovered.weak_bg_fill =
            md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER);
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.active.weak_bg_fill =
            md3::state_layer(md3::on_surface(), md3::STATE_LAYER_PRESSED);
        v.widgets.active.bg_stroke = Stroke::NONE;
        v.widgets.open.bg_fill = Color32::TRANSPARENT;
        v.widgets.open.bg_stroke = Stroke::NONE;
        add(ui);
    });
}

/// 48px full-radius navigation pill: 24px icon (drawn by `draw_icon`
/// into the box it is given) + 14px label. Selected =
/// `secondary-container` fill / `on-secondary-container` ink; hover =
/// 8% `on-surface` state layer.
pub fn nav_pill(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    draw_icon: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, NAV_PILL_HEIGHT), Sense::click());
    let painter = ui.painter();
    let rounding = Rounding::same(NAV_PILL_HEIGHT / 2.0);

    if selected {
        painter.rect_filled(rect, rounding, md3::secondary_container());
    } else if resp.hovered() {
        painter.rect_filled(
            rect,
            rounding,
            md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER),
        );
    }
    let ink = if selected {
        md3::on_secondary_container()
    } else {
        md3::on_surface_variant()
    };

    // Icon box: 24px square, 16px from the left edge (`.settings-nav-
    // item` padding) — label follows after a 12px gap.
    let icon_box = Rect::from_center_size(
        egui::pos2(rect.left() + 16.0 + 12.0, rect.center().y),
        Vec2::splat(24.0),
    );
    draw_icon(painter, icon_box, ink);

    painter.text(
        egui::pos2(icon_box.right() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
        ink,
    );
    resp
}

/// `.settings-row`: label stacked above the control, 8px gap, 24px
/// bottom margin.
pub fn form_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(RichText::new(label).size(14.0).color(md3::on_surface()));
    ui.add_space(ROW_GAP);
    let result = add_control(ui);
    ui.add_space(ROW_MARGIN);
    result
}

/// `.settings-row-toggle`: horizontal label / switch pair,
/// space-between, capped at [`ROW_MAX_WIDTH`]. Returns true when
/// toggled.
pub fn toggle_row(ui: &mut egui::Ui, label: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.allocate_ui_with_layout(
        Vec2::new(ROW_MAX_WIDTH, 32.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(RichText::new(label).size(14.0).color(md3::on_surface()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                changed = switch(ui, value);
            });
        },
    );
    ui.add_space(ROW_MARGIN * 0.5);
    changed
}

/// Subsection heading inside a settings category.
pub fn subsection(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(title)
            .size(16.0)
            .color(md3::primary())
            .strong(),
    );
    ui.add_space(12.0);
}

/// Hint line under a control (`.settings-hint` equivalent).
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(12.0)
            .color(md3::on_surface_variant()),
    );
}
