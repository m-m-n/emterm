//! Tab bar widget (Phase 4-B; MD3-aligned 2026-05-21).
//!
//! Renders a top panel one row of tabs + a trailing "+" button, mirroring
//! the WebView build's Material Design 3 tab strip (`src/styles/tab-bar.css`):
//!
//! - 48 px tall strip with `surface-container` background and a 1 px
//!   `outline-variant` bottom hairline.
//! - Tabs distribute equally with a 120 px minimum width, padding 24 px
//!   horizontally; horizontal scroll kicks in when the floor would
//!   overflow.
//! - Inactive tabs render with `on-surface-variant`; the active tab
//!   switches to `primary` and grows a 3 px bottom indicator
//!   (`width = cell - 32 px`, 3 px corner radius at the top).
//! - Hover overlays a state-layer (currentColor at 8 % alpha) — same
//!   formula as the WebView `.tab::before`.
//!
//! Title rendering (TS-tab-3):
//!
//! - When `mux_session_name` is `Some(name)`, the rendered title is
//!   `[mux:name] <title>` (single space). When `None`, the title is
//!   rendered verbatim.
//!
//! No per-tab close affordance: the WebView build does not show one
//! either; tabs close via `Ctrl+Shift+W` (the keybind layer emits
//! [`crate::ui::AppAction::CloseTab`]). The trailing `+` icon is
//! drawn with `Painter::line_segment` so the visual is font-independent.

use egui::{Align, FontId, Layout, Rect, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2};

use super::md3;
use super::TabEvent;

/// Fixed visual height of the tab strip in egui logical points.
/// Matches `.tab-bar { height: 48px }` in the WebView build.
pub const TAB_BAR_HEIGHT: f32 = 48.0;

/// Effective tab-bar height for layout math, accounting for the runtime
/// tab-bar visibility (`App::show_tab_bar`, seeded from
/// `settings.show_tab_bar` and flipped by the `ToggleTabBar` keybind).
/// Returns 0 when the bar is hidden so the terminal grid / cursor
/// overlay use the freed vertical space.
pub fn effective_tab_bar_height(show_tab_bar: bool) -> f32 {
    if show_tab_bar {
        TAB_BAR_HEIGHT
    } else {
        0.0
    }
}
/// Minimum width of a single tab before horizontal scroll kicks in.
/// Matches `.tab { min-width: 120px }`.
const MIN_TAB_WIDTH: f32 = 120.0;
/// Maximum width of a single tab.
/// Matches `.tab { max-width: 300px }`.
const MAX_TAB_WIDTH: f32 = 300.0;
/// Horizontal padding inside each tab — matches `.tab { padding: 0 24px }`.
const TAB_HORIZONTAL_PAD: f32 = 24.0;
/// Diameter of the trailing "+" icon button. Matches `.tab-button { 40x40 }`.
const NEW_TAB_BUTTON_SIZE: f32 = 40.0;
/// Side length of the "+" glyph drawn inside the new-tab button.
const PLUS_ICON_SIZE: f32 = 12.0;
/// Stroke width of the "+" glyph. Matches `title_bar`'s icon stroke
/// so the two affordances feel visually paired.
const PLUS_ICON_STROKE_WIDTH: f32 = 1.0;
/// Horizontal padding either side of the fixed-button area.
/// Matches `.tab-fixed-area { padding: 0 8px }`.
const FIXED_AREA_PAD: f32 = 8.0;
/// Height of the bottom 1 px hairline drawn under the strip.
const HAIRLINE_HEIGHT: f32 = 1.0;
/// Tab font size — matches `.tab { font-size: 14px }`.
const TAB_FONT_SIZE: f32 = 14.0;
/// Active-tab underline thickness. Matches `.tab.active::after { height: 3px }`.
const ACTIVE_INDICATOR_HEIGHT: f32 = 3.0;
/// Margin between the left/right edges of the cell and the active
/// indicator, so its width matches the CSS `calc(100% - 32px)`.
const ACTIVE_INDICATOR_SIDE_MARGIN: f32 = 16.0;
/// Corner radius of the active indicator, mirroring `border-radius: 3px 3px 0 0`.
const ACTIVE_INDICATOR_RADIUS: f32 = 3.0;
/// Diameter of the unread-activity dot. Matches `.tab-activity-dot { width/height: 8px }`.
const ACTIVITY_DOT_DIAMETER: f32 = 8.0;
/// Gap between the activity dot and the title. Matches
/// `.tab-activity-dot { margin-right: 6px }`.
const ACTIVITY_DOT_MARGIN: f32 = 6.0;
/// Activity-dot show/hide animation duration in seconds. Matches the
/// WebView's `--md-motion-duration-short4` (250 ms) opacity/scale
/// transition.
const ACTIVITY_DOT_ANIM_SECS: f32 = 0.25;
/// Icon-button (state-layer) corner radius — MD3 uses a full pill so the
/// 8 % overlay forms a circle inside the 40 px square.
const ICON_BUTTON_RADIUS: f32 = NEW_TAB_BUTTON_SIZE / 2.0;

/// Minimal projection of [`crate::tabs::Tab`] used by the tab bar.
///
/// Constructed once per frame by the app loop. Tests construct these
/// directly.
#[derive(Debug, Clone)]
pub struct TabBarItem {
    /// PTY title (OSC-supplied) or `"shell"` fallback.
    pub title: String,
    /// When `Some`, the tab is in mux mode and the title is prefixed
    /// with `[mux:<session>]` before rendering. Populated by Phase 4-C
    /// once the mux client is wired; Phase 4-B leaves this `None`.
    pub mux_session_name: Option<String>,
    /// When `true`, an unread-activity dot renders left of the title
    /// (mirrors `.tab-activity-dot.visible`). The view-model builder
    /// applies the `settings.tab_activity_indicator` gate, so the
    /// widget just draws what it is told.
    pub has_activity: bool,
    /// Stable per-tab identity (`crate::tabs::Tab::stable_id`) keying
    /// egui animation state. Positional indices shift on tab close /
    /// drag-reorder, which would bleed in-flight dot animations between
    /// tabs; titles are not unique (every fresh tab is "shell"). The
    /// view-model builder MUST set this via `with_stable_id`.
    pub stable_id: u64,
}

impl TabBarItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            mux_session_name: None,
            has_activity: false,
            stable_id: 0,
        }
    }

    pub fn with_mux_session(mut self, name: impl Into<String>) -> Self {
        self.mux_session_name = Some(name.into());
        self
    }

    pub fn with_activity(mut self, has_activity: bool) -> Self {
        self.has_activity = has_activity;
        self
    }

    pub fn with_stable_id(mut self, id: u64) -> Self {
        self.stable_id = id;
        self
    }
}

/// Compute the displayed label for a tab. Pure helper, kept public so
/// TS-tab-3 can exercise it directly without driving egui.
pub fn render_label(item: &TabBarItem) -> String {
    match &item.mux_session_name {
        Some(session) => format!("[mux:{}] {}", session, item.title),
        None => item.title.clone(),
    }
}

/// Render the tab bar into a top panel, returning at most one
/// [`TabEvent`] this frame.
pub fn draw(ctx: &egui::Context, items: &[TabBarItem], active_idx: usize) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;

    let frame = egui::Frame::none()
        .fill(md3::surface_container())
        .inner_margin(egui::Margin::ZERO);

    egui::TopBottomPanel::top("native-poc-tab-bar")
        .frame(frame)
        .exact_height(TAB_BAR_HEIGHT)
        .show_separator_line(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            // Total room for the scrollable tab strip (everything minus the
            // fixed "+" / gear area on the right).
            let panel_w = ui.available_width();
            let fixed_w = NEW_TAB_BUTTON_SIZE * 2.0 + FIXED_AREA_PAD * 2.0;
            let scroll_w = (panel_w - fixed_w).max(0.0);

            // Hairline at the very bottom — drawn last so it stays on top
            // of the per-tab fills.
            let panel_rect = ui.max_rect();

            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // ── Tab strip ───────────────────────────────────────
                let n = items.len().max(1) as f32;
                let ideal_w = (scroll_w / n).clamp(MIN_TAB_WIDTH, MAX_TAB_WIDTH);
                let needed_w = MIN_TAB_WIDTH * n;

                // Horizontal scroll only engages when the floor (MIN ×
                // count) exceeds the available strip width. Keeping the
                // common path scroll-free preserves a predictable cell
                // origin for the click-to-tab tests below.
                // The strip always occupies the full scroll_w span (even
                // when the tabs need less) so the fixed "+" / gear area
                // that follows stays pinned to the panel's right edge,
                // mirroring the WebView's `.tab-fixed-area`.
                if needed_w > scroll_w {
                    ui.allocate_ui_with_layout(
                        Vec2::new(scroll_w, TAB_BAR_HEIGHT),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ScrollArea::horizontal()
                                .id_salt("native-poc-tab-strip")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        event =
                                            layout_tab_strip(ui, items, active_idx, MIN_TAB_WIDTH);
                                    });
                                });
                        },
                    );
                } else {
                    ui.allocate_ui_with_layout(
                        Vec2::new(scroll_w, TAB_BAR_HEIGHT),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            // `allocate_ui_with_layout` only advances the
                            // parent cursor by what the child actually
                            // used; pin the child's min width so the
                            // fixed-button area lands at the right edge
                            // even when the tabs need less room.
                            ui.set_min_width(scroll_w);
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            event = layout_tab_strip(ui, items, active_idx, ideal_w);
                        },
                    );
                }

                // ── Fixed-button area ("+") ─────────────────────────
                ui.add_space(FIXED_AREA_PAD);
                // 1 px vertical separator on the left edge of the fixed area
                // mirrors `.tab-fixed-area { border-left }`.
                let sep_x = ui.cursor().min.x - FIXED_AREA_PAD;
                ui.painter().vline(
                    sep_x,
                    panel_rect.top()..=(panel_rect.bottom() - HAIRLINE_HEIGHT),
                    Stroke::new(1.0, md3::outline_variant()),
                );

                let plus_resp = draw_icon_button(ui, NEW_TAB_BUTTON_SIZE);
                #[cfg(test)]
                {
                    tests::LAST_PLUS_RECT.with(|c| c.set(Some(plus_resp.rect)));
                }
                if plus_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::New);
                }
                // Gear button — open (or focus) the Settings tab.
                // Mirrors the WebView `.tab-button-settings` next to "+".
                let gear_resp = draw_gear_button(ui, NEW_TAB_BUTTON_SIZE);
                if gear_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::OpenSettings);
                }
                ui.add_space(FIXED_AREA_PAD);
            });

            // Bottom 1 px hairline (outline-variant).
            let painter = ui.painter();
            let y = panel_rect.bottom() - HAIRLINE_HEIGHT / 2.0;
            painter.hline(
                panel_rect.left()..=panel_rect.right(),
                y,
                Stroke::new(HAIRLINE_HEIGHT, md3::outline_variant()),
            );
        });

    event
}

/// Persistent key under which the current drag origin (`Option<usize>`)
/// is stored in egui's frame memory. Survives across frames so the
/// pending drag is observed by every layout pass until the pointer is
/// released.
const DRAG_FROM_KEY: &str = "native-poc-tab-drag-from";

fn drag_state_id() -> egui::Id {
    egui::Id::new(DRAG_FROM_KEY)
}

/// Inner layout: lay out one tab cell per item. Returns at most one
/// [`TabEvent`] this frame.
fn layout_tab_strip(
    ui: &mut Ui,
    items: &[TabBarItem],
    active_idx: usize,
    tab_width: f32,
) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;
    let drag_id = drag_state_id();
    let mut drag_from: Option<usize> = ui.ctx().memory(|m| m.data.get_temp(drag_id));

    // Capture cell rects locally so the post-loop drop-target math does
    // not depend on the test-only thread_local hooks.
    let mut cell_rects: Vec<Rect> = Vec::with_capacity(items.len());

    #[cfg(test)]
    tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().clear());

    for (i, item) in items.iter().enumerate() {
        let is_active = i == active_idx;
        let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);
        let (rect, cell_resp) = ui.allocate_exact_size(cell_size, Sense::click_and_drag());

        cell_rects.push(rect);
        #[cfg(test)]
        tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().push(rect));

        // Detect drag start. egui's `drag_started_by` fires the frame
        // after the pointer exceeds the click-vs-drag distance, so a
        // simple click does not enter drag mode.
        if drag_from.is_none() && cell_resp.drag_started_by(egui::PointerButton::Primary) {
            drag_from = Some(i);
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(drag_id, i as usize));
        }

        // Background — the strip itself inherits `surface-container` from
        // the parent panel frame; we only paint the hover state-layer.
        // Tabs currently being dragged dim slightly so the user knows
        // which one they picked up.
        let painter = ui.painter();
        if drag_from == Some(i) {
            painter.rect_filled(
                rect,
                Rounding::ZERO,
                md3::state_layer(md3::primary(), md3::STATE_LAYER_HOVER),
            );
        } else if cell_resp.hovered() {
            painter.rect_filled(
                rect,
                Rounding::ZERO,
                md3::state_layer(
                    if is_active {
                        md3::primary()
                    } else {
                        md3::on_surface_variant()
                    },
                    md3::STATE_LAYER_HOVER,
                ),
            );
        }

        // Label sub-rect. Drawn via the painter directly so the
        // parent layout's cursor is not perturbed (ui.put would
        // shift subsequent allocations).
        let label_left = rect.left() + TAB_HORIZONTAL_PAD;
        let label_right = rect.right() - TAB_HORIZONTAL_PAD;
        let label_rect = Rect::from_min_max(
            egui::pos2(label_left, rect.top()),
            egui::pos2(label_right.max(label_left), rect.bottom()),
        );

        let label_text = render_label(item);
        let text_color = if is_active {
            md3::primary()
        } else {
            md3::on_surface_variant()
        };
        let font_id = FontId::proportional(TAB_FONT_SIZE);
        // Activity-dot slot. Like the WebView flexbox (`.tab-activity-dot`
        // hides via opacity/scale, not display:none), the 8 px dot +
        // 6 px gap always occupy layout space so the title does not
        // shift when the dot appears.
        let dot_space = ACTIVITY_DOT_DIAMETER + ACTIVITY_DOT_MARGIN;
        // egui has no native truncation helper for direct painter text,
        // so we measure with `Fonts::layout_no_wrap` and ellipsize when
        // the result overflows the label rect.
        let max_w = (label_rect.width() - dot_space).max(0.0);
        let galley = ui.fonts(|fonts| {
            let mut text = label_text.clone();
            let mut galley = fonts.layout_no_wrap(text.clone(), font_id.clone(), text_color);
            if galley.size().x > max_w && !text.is_empty() {
                let ell = "…";
                while text.chars().count() > 1 {
                    text.pop();
                    let candidate = format!("{text}{ell}");
                    let g = fonts.layout_no_wrap(candidate, font_id.clone(), text_color);
                    if g.size().x <= max_w {
                        galley = g;
                        break;
                    }
                }
            }
            galley
        });
        // Centre the [dot][gap][title] group as one unit, mirroring the
        // WebView's `justify-content: center` flex row.
        let group_w = dot_space + galley.size().x;
        let group_left = label_rect.center().x - group_w / 2.0;

        // Dot show/hide animates scale + opacity over 250 ms — the
        // `.tab-activity-dot` transition. `animate_bool_with_time`
        // requests repaints while in flight, so the fade plays out
        // without an explicit redraw hook. Keyed on the tab's stable
        // identity (NOT the positional index, which shifts on tab
        // close / reorder and would bleed animation state across tabs).
        let dot_t = ui.ctx().animate_bool_with_time(
            egui::Id::new(("native-poc-tab-activity-dot", item.stable_id)),
            item.has_activity,
            ACTIVITY_DOT_ANIM_SECS,
        );
        if dot_t > 0.0 {
            let dot_center = egui::pos2(
                group_left + ACTIVITY_DOT_DIAMETER / 2.0,
                label_rect.center().y,
            );
            ui.painter().circle_filled(
                dot_center,
                (ACTIVITY_DOT_DIAMETER / 2.0) * dot_t,
                md3::primary().gamma_multiply(dot_t),
            );
        }

        let text_x = group_left + dot_space;
        let text_y = label_rect.center().y - galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(text_x, text_y), galley, text_color);

        // Single click responder for the whole cell switches tabs.
        // Skip when a drag is in flight — the release at the end of a
        // drag must not double-fire a click. Close lives on the
        // `Ctrl+Shift+W` keybind path; the WebView build has no
        // per-tab `×` either, so we keep the cell click-surface
        // dedicated to switching.
        if cell_resp.clicked() && drag_from.is_none() && event.is_none() && !is_active {
            event = Some(TabEvent::Switch(i));
        }

        // Active-tab indicator: 3 px bar at the bottom, side-margined to
        // match `width: calc(100% - 32px)`.
        if is_active {
            let painter = ui.painter();
            let bar = Rect::from_min_max(
                egui::pos2(
                    rect.left() + ACTIVE_INDICATOR_SIDE_MARGIN,
                    rect.bottom() - ACTIVE_INDICATOR_HEIGHT - HAIRLINE_HEIGHT,
                ),
                egui::pos2(
                    rect.right() - ACTIVE_INDICATOR_SIDE_MARGIN,
                    rect.bottom() - HAIRLINE_HEIGHT,
                ),
            );
            painter.rect_filled(
                bar,
                Rounding {
                    nw: ACTIVE_INDICATOR_RADIUS,
                    ne: ACTIVE_INDICATOR_RADIUS,
                    sw: 0.0,
                    se: 0.0,
                },
                md3::primary(),
            );
        }
    }

    // Post-loop: handle drag-in-progress (indicator) and drop (event).
    if let Some(from) = drag_from {
        // `latest_pos` survives across release frames, unlike
        // `interact_pos` which returns `None` once the pointer leaves
        // the interaction state (e.g. on the release frame itself).
        let pointer_pos = ui.input(|i| i.pointer.latest_pos());
        let target = pointer_pos.map(|p| drop_target_index(&cell_rects, p.x));

        // Draw a vertical primary-coloured indicator at the drop slot.
        if let Some(target) = target {
            if let Some(indicator_x) = drop_indicator_x(&cell_rects, target) {
                let y0 = cell_rects[0].top();
                let y1 = cell_rects[0].bottom() - HAIRLINE_HEIGHT;
                ui.painter()
                    .vline(indicator_x, y0..=y1, Stroke::new(2.0, md3::primary()));
            }
        }

        // Release ends the drag. `drag_started_by` already guards the
        // click-vs-drag threshold (egui's default 4 px), so by the
        // time `drag_from` is set we know this was an actual drag.
        // No separate threshold check is needed here, and one would be
        // hostile to implement: `press_origin()` returns `None` on the
        // release frame because the button is no longer held.
        let released = ui.input(|i| i.pointer.any_released());
        if released {
            if let Some(to) = target {
                if to != from && to != from + 1 {
                    event = Some(TabEvent::Reorder { from, to });
                }
            }
            ui.ctx().memory_mut(|m| m.data.remove::<usize>(drag_id));
        }
    }

    event
}

/// Compute the drop-target insertion index given the strip's cell
/// rects and the pointer's current `x`. The result lies in
/// `0..=cells.len()`. The pointer is considered to drop "before" a
/// cell if it sits in that cell's left half, and "after" if it sits
/// in the right half. Outside the strip, drops clamp to the closest
/// edge.
fn drop_target_index(cells: &[Rect], pointer_x: f32) -> usize {
    if cells.is_empty() {
        return 0;
    }
    if pointer_x < cells[0].left() {
        return 0;
    }
    if pointer_x > cells[cells.len() - 1].right() {
        return cells.len();
    }
    for (i, rect) in cells.iter().enumerate() {
        if pointer_x < rect.center().x {
            return i;
        }
    }
    cells.len()
}

/// X position of the drop indicator for the given insertion index.
/// `index == 0` → left edge of the first cell; `index == cells.len()`
/// → right edge of the last cell; otherwise the boundary between
/// `cells[index - 1]` and `cells[index]`.
fn drop_indicator_x(cells: &[Rect], index: usize) -> Option<f32> {
    if cells.is_empty() {
        return None;
    }
    if index == 0 {
        return Some(cells[0].left());
    }
    if index >= cells.len() {
        return Some(cells[cells.len() - 1].right());
    }
    Some(cells[index].left())
}

/// Draw the trailing 40 px "+" icon button. The "+" is composed of
/// two `line_segment` calls (vertical + horizontal stroke) so the
/// glyph is font-independent and aligns visually with the
/// `title_bar` icons. Hover swaps in the MD3 state-layer overlay
/// inside a full-radius pill so the layer reads as a circle.
fn draw_icon_button(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();

    if resp.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(ICON_BUTTON_RADIUS),
            md3::state_layer(md3::on_surface_variant(), md3::STATE_LAYER_HOVER),
        );
    }

    let bbox = Rect::from_center_size(rect.center(), Vec2::splat(PLUS_ICON_SIZE));
    let stroke = Stroke::new(PLUS_ICON_STROKE_WIDTH, md3::on_surface_variant());
    let cx = bbox.center().x;
    let cy = bbox.center().y;
    painter.line_segment(
        [egui::pos2(bbox.left(), cy), egui::pos2(bbox.right(), cy)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(cx, bbox.top()), egui::pos2(cx, bbox.bottom())],
        stroke,
    );

    resp
}

/// Circular hover-highlight button with a line-drawn gear glyph.
/// Painter-rendered (like the "+" button) so it follows the md3 tokens
/// without shipping an icon font.
fn draw_gear_button(ui: &mut Ui, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let painter = ui.painter();

    if resp.hovered() {
        painter.rect_filled(
            rect,
            Rounding::same(ICON_BUTTON_RADIUS),
            md3::state_layer(md3::on_surface_variant(), md3::STATE_LAYER_HOVER),
        );
    }

    let center = rect.center();
    let color = md3::on_surface_variant();
    // Gear glyph: hub ring + outer ring + 8 radial teeth.
    let hub_r = 2.5;
    let ring_r = 5.0;
    let tooth_r = 7.5;
    painter.circle_stroke(center, hub_r, Stroke::new(1.2, color));
    painter.circle_stroke(center, ring_r, Stroke::new(1.6, color));
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::FRAC_PI_4;
        let dir = Vec2::new(angle.cos(), angle.sin());
        painter.line_segment(
            [center + dir * ring_r, center + dir * tooth_r],
            Stroke::new(2.0, color),
        );
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use std::cell::{Cell, RefCell};

    thread_local! {
        pub(super) static LAST_PLUS_RECT: Cell<Option<Rect>> = const { Cell::new(None) };
        pub(super) static LAST_TAB_CELLS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    }

    fn item(title: &str) -> TabBarItem {
        TabBarItem::new(title)
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 100.0))
    }

    fn pos_of_first_hovered(ctx: &egui::Context) -> Option<Pos2> {
        ctx.pointer_latest_pos()
    }

    fn run_with_click(
        items: &[TabBarItem],
        active_idx: usize,
        click_pos: Pos2,
    ) -> Option<TabEvent> {
        let ctx = egui::Context::default();

        let mut input1 = RawInput::default();
        input1.screen_rect = Some(screen_rect());
        input1.events.push(Event::PointerMoved(click_pos));
        let mut captured: Option<TabEvent> = None;
        let _ = ctx.run(input1, |ctx| {
            captured = draw(ctx, items, active_idx);
        });

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
        let mut second: Option<TabEvent> = None;
        let _ = ctx.run(input2, |ctx| {
            second = draw(ctx, items, active_idx);
            let _ = pos_of_first_hovered(ctx);
        });

        second.or(captured)
    }

    // ── TS-tab-3: mux mode title prefix ─────────────────────

    #[test]
    fn render_label_passthrough_when_no_mux_session() {
        let it = item("zsh");
        assert_eq!(render_label(&it), "zsh");
    }

    #[test]
    fn render_label_prepends_mux_prefix_when_session_present() {
        let it = TabBarItem::new("nvim").with_mux_session("foo");
        assert_eq!(render_label(&it), "[mux:foo] nvim");
    }

    #[test]
    fn render_label_keeps_default_shell_title() {
        let it = item("shell");
        assert_eq!(render_label(&it), "shell");
    }

    // ── TS-tab-1: simulated interaction → TabEvent ──────────

    /// Drive one layout-only frame to populate the test thread_local
    /// rects (`+` button, tab cells), then return the requested
    /// rect. Eliminates hard-coded screen coordinates and keeps the
    /// tests robust against panel padding tweaks.
    fn capture_rect_with<F>(items: &[TabBarItem], active_idx: usize, pick: F) -> Rect
    where
        F: Fn() -> Option<Rect>,
    {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, active_idx);
        });
        pick().expect("layout hook should have captured the rect")
    }

    fn plus_rect(items: &[TabBarItem], active_idx: usize) -> Rect {
        capture_rect_with(items, active_idx, || LAST_PLUS_RECT.with(|c| c.get()))
    }

    fn tab_cell_rect(items: &[TabBarItem], active_idx: usize, i: usize) -> Rect {
        capture_rect_with(items, active_idx, || {
            LAST_TAB_CELLS.with(|c| c.borrow().get(i).copied())
        })
    }

    #[test]
    fn clicking_plus_emits_new() {
        let items = vec![item("a"), item("b")];
        let target = plus_rect(&items, 0).center();
        let ev = run_with_click(&items, 0, target);
        assert_eq!(ev, Some(TabEvent::New));
    }

    #[test]
    fn clicking_inactive_tab_emits_switch() {
        // Click the centre of tab 1's label sub-rect (i.e. the cell
        // centre, well clear of the close column on the right).
        let items = vec![item("alpha"), item("beta")];
        let cell = tab_cell_rect(&items, 0, 1);
        let click = Pos2::new(cell.left() + TAB_HORIZONTAL_PAD + 4.0, cell.center().y);
        let ev = run_with_click(&items, 0, click);
        assert_eq!(ev, Some(TabEvent::Switch(1)));
    }

    #[test]
    fn tab_cells_lay_out_side_by_side_without_overlap() {
        let items = vec![item("alpha"), item("beta")];
        let _ = plus_rect(&items, 0);
        let cells: Vec<_> = LAST_TAB_CELLS.with(|c| c.borrow().clone());
        assert_eq!(cells.len(), 2);
        assert!(
            (cells[0].right() - cells[1].left()).abs() < 0.5,
            "tab cells should sit edge-to-edge; got {:?} / {:?}",
            cells[0],
            cells[1]
        );
    }

    // ── drop_target_index / drop_indicator_x ────────────────

    fn rect(x0: f32, x1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, 0.0), Pos2::new(x1, 48.0))
    }

    #[test]
    fn drop_target_left_of_strip_clamps_to_zero() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, -10.0), 0);
    }

    #[test]
    fn drop_target_right_of_strip_returns_len() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, 300.0), 2);
    }

    #[test]
    fn drop_target_uses_cell_centre_as_boundary() {
        // Cells: [0, 100] (centre 50), [100, 200] (centre 150)
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_target_index(&cells, 49.0), 0, "left half of cell 0");
        assert_eq!(drop_target_index(&cells, 51.0), 1, "right half of cell 0");
        assert_eq!(drop_target_index(&cells, 149.0), 1, "left half of cell 1");
        assert_eq!(drop_target_index(&cells, 151.0), 2, "right half of cell 1");
    }

    #[test]
    fn drop_indicator_x_pins_to_cell_boundaries() {
        let cells = vec![rect(0.0, 100.0), rect(100.0, 200.0)];
        assert_eq!(drop_indicator_x(&cells, 0), Some(0.0));
        assert_eq!(drop_indicator_x(&cells, 1), Some(100.0));
        assert_eq!(drop_indicator_x(&cells, 2), Some(200.0));
    }

    #[test]
    fn drop_indicator_x_empty_is_none() {
        let cells: Vec<Rect> = Vec::new();
        assert_eq!(drop_indicator_x(&cells, 0), None);
    }

    #[test]
    fn draw_with_single_tab_does_not_panic_and_emits_nothing_without_click() {
        let items = vec![item("solo")];
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut captured: Option<TabEvent> = None;
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, &items, 0);
        });
        assert_eq!(captured, None);
    }
}
