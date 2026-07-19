//! Mux window-list sidebar.
//!
//! task0005 (frame integration) needs this module to exist per the
//! `ui::mux_sidebar` contract documented in
//! `feature-docs/mux-vertical-tabs/IMPLEMENTATION.md` (Shared Components:
//! "Sidebar width function", "Sidebar widget `ui::mux_sidebar`"), but the
//! sibling task that owns the module's real implementation (task0004) had
//! not landed on the integration branch when this task started. This file
//! is a functionally-correct, minimally-styled stand-in: it satisfies the
//! documented contract exactly (width formula, entry/placement shapes,
//! click-routing output) so `render::draw_terminal` / `window_host` /
//! `render::cursor` have something real to integrate and test against.
//! task0004's visual design (the widget's internal look and layout) is
//! explicitly out of scope for task0005's own plan; when task0004 lands,
//! its conflict on this file resolves via the worktree-task-workflow
//! parent-side-adoption protocol (adopt task0004's version, re-apply any
//! task0005 call-site changes on top).
//!
//! It draws only the list and reports the clicked entry's window index —
//! it never sends mux messages itself. The caller routes the result into
//! the existing `TabEvent::MuxSwitch` application path
//! (`App::apply_tab_event`).

use egui::{Align2, FontId, Rect, Rounding, ScrollArea, Sense, Ui, Vec2};

use super::md3;

/// Minimum sidebar width in logical px (IMPLEMENTATION.md Shared
/// Components: "Sidebar width function").
pub const MIN_WIDTH_PX: f32 = 180.0;
/// Maximum sidebar width in logical px.
pub const MAX_WIDTH_PX: f32 = 320.0;
/// Fraction of the window's logical width the sidebar targets before
/// clamping.
pub const WIDTH_RATIO: f32 = 0.22;

/// Pure width formula: `clamp(180px, 22% of window width, 320px)`.
/// Deterministic, no state — shared by both placement variants and by the
/// terminal-grid geometry inset (task0005 D2).
pub fn width_px(window_width_logical: f32) -> f32 {
    (window_width_logical * WIDTH_RATIO).clamp(MIN_WIDTH_PX, MAX_WIDTH_PX)
}

/// One window entry in the ordered list the sidebar renders.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarEntry {
    /// Window position (0-based) within the group; the click target.
    pub index: usize,
    /// Window display name.
    pub name: String,
    /// Whether this is the active window (highlighted).
    pub active: bool,
}

/// Which placement variant to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Persistent left panel — reserves grid space (drawn between the tab
    /// bar and the central panel).
    Persistent,
    /// Right-edge overlay — draws over the terminal area without
    /// affecting grid geometry (drawn after the central panel).
    Overlay,
}

/// Row height in logical px for each entry.
const ROW_HEIGHT: f32 = 36.0;
/// Horizontal text inset inside a row.
const ROW_PAD_X: f32 = 12.0;
/// Entry label font size.
const ROW_FONT_SIZE: f32 = 14.0;

/// Draw the window list for `placement` at `width` (logical px, normally
/// [`width_px`]'s output) and return the clicked entry's window index, if
/// any this frame.
pub fn draw(
    ctx: &egui::Context,
    entries: &[SidebarEntry],
    placement: Placement,
    width: f32,
) -> Option<usize> {
    match placement {
        Placement::Persistent => draw_persistent(ctx, entries, width),
        Placement::Overlay => draw_overlay(ctx, entries, width),
    }
}

fn draw_persistent(ctx: &egui::Context, entries: &[SidebarEntry], width: f32) -> Option<usize> {
    let mut clicked = None;
    let frame = egui::Frame::none()
        .fill(md3::surface_container())
        .inner_margin(egui::Margin::ZERO);
    egui::SidePanel::left("mux-sidebar-persistent")
        .exact_width(width.max(0.0))
        .resizable(false)
        .frame(frame)
        .show(ctx, |ui| {
            clicked = draw_entry_list(ui, entries);
        });
    clicked
}

fn draw_overlay(ctx: &egui::Context, entries: &[SidebarEntry], width: f32) -> Option<usize> {
    let mut clicked = None;
    let screen = ctx.screen_rect();
    let w = width.max(0.0);
    let rect = Rect::from_min_size(
        egui::pos2(screen.right() - w, screen.top()),
        Vec2::new(w, screen.height()),
    );
    egui::Area::new(egui::Id::new("mux-sidebar-overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(rect, Rounding::ZERO, md3::surface_container_high());
            ui.set_width(w);
            ui.set_height(rect.height());
            clicked = draw_entry_list(ui, entries);
        });
    clicked
}

/// Shared row list drawn by both placement variants.
fn draw_entry_list(ui: &mut Ui, entries: &[SidebarEntry]) -> Option<usize> {
    let mut clicked = None;
    ScrollArea::vertical()
        .id_salt("mux-sidebar-entries")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in entries {
                let row_w = ui.available_width();
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(row_w, ROW_HEIGHT), Sense::click());
                let color = if entry.active {
                    md3::primary()
                } else {
                    md3::on_surface_variant()
                };
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        Rounding::ZERO,
                        md3::state_layer(color, md3::STATE_LAYER_HOVER),
                    );
                }
                ui.painter().text(
                    egui::pos2(rect.left() + ROW_PAD_X, rect.center().y),
                    Align2::LEFT_CENTER,
                    &entry.name,
                    FontId::proportional(ROW_FONT_SIZE),
                    color,
                );
                if resp.clicked() && clicked.is_none() {
                    clicked = Some(entry.index);
                }
            }
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};

    // ── width_px: clamp(180px, 22% of window width, 320px) ────────────────

    #[test]
    fn width_px_clamps_to_minimum_on_narrow_window() {
        assert_eq!(width_px(400.0), MIN_WIDTH_PX);
    }

    #[test]
    fn width_px_clamps_to_maximum_on_wide_window() {
        assert_eq!(width_px(3000.0), MAX_WIDTH_PX);
    }

    #[test]
    fn width_px_uses_ratio_in_the_middle_range() {
        // 1000 * 0.22 = 220, inside [180, 320].
        assert!((width_px(1000.0) - 220.0).abs() < f32::EPSILON);
    }

    // ── draw: click routing ────────────────────────────────────────────

    fn entries(n: usize, active: usize) -> Vec<SidebarEntry> {
        (0..n)
            .map(|i| SidebarEntry {
                index: i,
                name: format!("w{i}"),
                active: i == active,
            })
            .collect()
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 600.0))
    }

    /// Lay out one frame and return the rect of entry `i` by re-deriving
    /// its geometry from the row height (rows stack top-to-bottom inside
    /// the panel/area, starting at the panel's content origin).
    fn run_and_click(
        items: &[SidebarEntry],
        placement: Placement,
        click_pos: Pos2,
    ) -> Option<usize> {
        let ctx = egui::Context::default();
        let width = 200.0;

        // egui::Area runs an invisible "sizing pass" the first time its Id
        // is seen (it doesn't know the content's real size yet), which can
        // leave its geometry constrained to a stale guess for that frame.
        // Two warm-up passes let the Persistent/Overlay geometry settle
        // before the click frame, mirroring how a real multi-frame app
        // loop would behave.
        for _ in 0..2 {
            let mut input1 = RawInput::default();
            input1.screen_rect = Some(screen_rect());
            input1.events.push(Event::PointerMoved(click_pos));
            let _ = ctx.run(input1, |ctx| {
                let _ = draw(ctx, items, placement, width);
            });
        }

        let mut input2 = RawInput::default();
        input2.screen_rect = Some(screen_rect());
        input2.events.push(Event::PointerMoved(click_pos));
        input2.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
        input2.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        let mut clicked = None;
        let _ = ctx.run(input2, |ctx| {
            clicked = draw(ctx, items, placement, width);
        });
        clicked
    }

    #[test]
    fn clicking_first_persistent_row_reports_window_zero() {
        let items = entries(3, 1);
        // First row sits at the panel's top-left content origin.
        let click = Pos2::new(20.0, ROW_HEIGHT / 2.0 + 2.0);
        let got = run_and_click(&items, Placement::Persistent, click);
        assert_eq!(got, Some(0));
    }

    #[test]
    fn clicking_second_persistent_row_reports_window_one() {
        let items = entries(3, 0);
        let click = Pos2::new(20.0, ROW_HEIGHT * 1.5 + 2.0);
        let got = run_and_click(&items, Placement::Persistent, click);
        assert_eq!(got, Some(1));
    }

    #[test]
    fn clicking_overlay_row_reports_window_index() {
        let items = entries(2, 0);
        // Overlay sits at the right edge; width = 200, screen width = 1000
        // → overlay spans x in [800, 1000].
        let click = Pos2::new(820.0, ROW_HEIGHT / 2.0 + 2.0);
        let got = run_and_click(&items, Placement::Overlay, click);
        assert_eq!(got, Some(0));
    }

    #[test]
    fn no_click_reports_none() {
        let items = entries(2, 0);
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut clicked = Some(usize::MAX);
        let _ = ctx.run(input, |ctx| {
            clicked = draw(ctx, &items, Placement::Persistent, 200.0);
        });
        assert_eq!(clicked, None);
    }

    #[test]
    fn empty_entries_never_panics_and_reports_none() {
        let items: Vec<SidebarEntry> = Vec::new();
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut clicked = Some(usize::MAX);
        let _ = ctx.run(input, |ctx| {
            clicked = draw(ctx, &items, Placement::Overlay, 200.0);
        });
        assert_eq!(clicked, None);
    }
}
