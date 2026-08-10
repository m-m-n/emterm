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

use egui::scroll_area::ScrollBarVisibility;
use egui::{Align, FontId, Layout, Rect, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2};

use crate::agent_status_model::Aggregated;

use super::TabEvent;
use super::emoji_cache::EmojiResources;
use super::md3;

/// Fixed visual height of the tab strip in egui logical points.
/// Matches `.tab-bar { height: 48px }` in the WebView build.
pub const TAB_BAR_HEIGHT: f32 = 48.0;

/// Effective tab-bar height for layout math, accounting for the runtime
/// tab-bar visibility (`App::show_tab_bar`, seeded from
/// `settings.show_tab_bar` and flipped by the `ToggleTabBar` keybind).
/// Returns 0 when the bar is hidden so the terminal grid / cursor
/// overlay use the freed vertical space.
pub fn effective_tab_bar_height(show_tab_bar: bool) -> f32 {
    if show_tab_bar { TAB_BAR_HEIGHT } else { 0.0 }
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

mod badge;
pub use badge::*;

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
    /// When `Some`, this tab is a mux tab group and renders one sub-tab per
    /// window (`[N] name`) instead of the plain title cell. Built by
    /// [`mux_group_render_model`] from the tab's
    /// [`crate::mux::window_group::MuxWindowGroup`] whenever the group holds
    /// at least one window (FR1, WebView parity). `None` leaves the plain-tab
    /// path untouched.
    ///
    /// mux-vertical-tabs task0005: production code (`render::draw_terminal`)
    /// no longer populates this field — the mux tab group's window list
    /// moved to the `ui::mux_sidebar` widget, and the tab-bar cell collapses
    /// to a single cell labelled via [`Self::mux_active_window_name`]
    /// instead (IMPLEMENTATION.md D1). The field, [`with_mux_cells`], and
    /// the inline sub-tab expansion below are kept in place per that same
    /// decision (reusable render-model, exercised by the tests in this
    /// module) but are inert against any `TabBarItem` the app actually
    /// constructs.
    ///
    /// [`with_mux_cells`]: Self::with_mux_cells
    pub mux_cells: Option<Vec<MuxSubTabCell>>,
    /// task0006 AC-1/AC-2: this tab's aggregated agent-status badge —
    /// highest-priority state across the tab's own status and (for a
    /// mux-attached tab) every pane in its window group. `None` when
    /// nothing has ever reported a state: no badge renders and no layout
    /// space is reserved for it (unlike [`Self::has_activity`]'s dot,
    /// which always occupies its slot). The view-model builder sets this
    /// via [`with_agent_badge`](Self::with_agent_badge).
    pub agent_badge: Option<Aggregated>,
    /// When `Some(name)`, this tab is a mux tab group collapsed to a single
    /// cell (task0005 AC-1): the rendered label becomes `mux: <name>`,
    /// overriding both the plain `title` and the `mux_session_name` prefix
    /// format. `name` is the group's active window's display name, already
    /// live (OSC-renamed) since the caller rebuilds it every frame from
    /// [`crate::mux::window_group::MuxWindowGroup::active_window`].
    pub mux_active_window_name: Option<String>,
}

impl TabBarItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            mux_session_name: None,
            has_activity: false,
            stable_id: 0,
            mux_cells: None,
            agent_badge: None,
            mux_active_window_name: None,
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

    /// Mark this tab as a mux tab group rendered from `cells`. An empty
    /// vec is treated as "not a group" (the plain title renders).
    pub fn with_mux_cells(mut self, cells: Vec<MuxSubTabCell>) -> Self {
        self.mux_cells = if cells.is_empty() { None } else { Some(cells) };
        self
    }

    /// Mark this tab as a collapsed mux tab group whose single cell is
    /// labelled `mux: <name>` (task0005 AC-1).
    pub fn with_mux_active_window_name(mut self, name: impl Into<String>) -> Self {
        self.mux_active_window_name = Some(name.into());
        self
    }

    /// Attach this tab's aggregated agent-status badge (task0006 AC-1).
    pub fn with_agent_badge(mut self, badge: Option<Aggregated>) -> Self {
        self.agent_badge = badge;
        self
    }
}

/// Compute the displayed label for a tab. Pure helper, kept public so
/// TS-tab-3 can exercise it directly without driving egui.
///
/// task0005 AC-1: a collapsed mux tab group (`mux_active_window_name`
/// `Some`) renders `mux: <active window name>`, taking precedence over both
/// the plain title and the `[mux:<session>]` prefix format below (the
/// latter only remains visible for the brief pre-window-list-populated
/// window, where `mux_session_name` is set but the group has no windows
/// yet).
pub fn render_label(item: &TabBarItem) -> String {
    if let Some(name) = &item.mux_active_window_name {
        return format!("mux: {name}");
    }
    match &item.mux_session_name {
        Some(session) => format!("[mux:{}] {}", session, item.title),
        None => item.title.clone(),
    }
}

/// Render the tab bar into a top panel, returning at most one
/// [`TabEvent`] this frame.
///
/// `scroll_active_into_view` (FR4) is a one-shot signal raised by the app's
/// keyboard tab/window switch handlers: when `true`, the strip scrolls the
/// active visual cell into view exactly once this frame. The caller
/// (`render::draw_terminal`) reads the value from `App`; the flag is cleared
/// post-frame in `window_host` (where `&mut App` is available), so passing a
/// stale `true` here is never a problem — it only matters for the frame the
/// app raised it.
pub fn draw(
    ctx: &egui::Context,
    items: &[TabBarItem],
    active_idx: usize,
    scroll_active_into_view: bool,
    emoji: Option<&EmojiResources<'_>>,
) -> Option<TabEvent> {
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
                // A mux tab group expands into multiple visual cells
                // (compact → 1, expanded → header + one per window), so the
                // width math counts cells, not roster entries.
                let n = visual_cell_count(items).max(1) as f32;
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
                            // FR2: the strip is a horizontal-only `ScrollArea`,
                            // so egui's default (`always_scroll_the_only_direction
                            // = false`) ignores a plain (no-modifier) vertical
                            // wheel delta. Enabling it on this scope folds the
                            // vertical wheel onto the single (horizontal) axis,
                            // so a hovered wheel scrolls the strip. egui reads
                            // this flag from the `ui` that `ScrollArea::show` is
                            // called on, so set it here before `.show`.
                            // FR3 (Shift+wheel) folds onto the horizontal axis
                            // via this same flag: the tab-bar wheel forward in
                            // `window_host` strips the modifier, so egui's
                            // input-layer shift→horizontal swap never fires —
                            // the horizontal scroll comes purely from this
                            // fold, shift or not.
                            ui.style_mut().always_scroll_the_only_direction = true;
                            ScrollArea::horizontal()
                                .id_salt("native-poc-tab-strip")
                                .auto_shrink([false, false])
                                // FR1: keep the strip horizontally scrollable but
                                // never paint a scrollbar (WebView parity — the
                                // CSS strip hides its scrollbar too).
                                .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        event = layout_tab_strip(
                                            ui,
                                            items,
                                            active_idx,
                                            MIN_TAB_WIDTH,
                                            scroll_active_into_view,
                                            emoji,
                                        );
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
                            event = layout_tab_strip(
                                ui,
                                items,
                                active_idx,
                                ideal_w,
                                scroll_active_into_view,
                                emoji,
                            );
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

mod strip;
use strip::*;

/// Draw the active-tab / active-sub-tab indicator: a 3 px primary bar at
/// the bottom, side-margined to match the WebView `width: calc(100% - 32px)`.
fn paint_active_indicator(ui: &Ui, rect: Rect) {
    #[cfg(test)]
    tests::LAST_INDICATOR_RECTS.with(|c| c.borrow_mut().push(rect));
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
    ui.painter().rect_filled(
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

/// Lay out `text` in `font_id` / `color`, ellipsizing with `…` when it
/// overflows `max_w`. Uses a binary search over char boundaries (O(log N)
/// layouts, one allocation for the winning candidate) instead of the
/// naive char-pop loop (O(N²) plus N `format!` allocations per frame). The
/// tab strip calls this on every cell every frame, so the cost matters
/// when window names are long.
fn layout_ellipsized(
    fonts: &egui::text::Fonts,
    text: &str,
    font_id: &FontId,
    color: egui::Color32,
    max_w: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = fonts.layout_no_wrap(text.to_string(), font_id.clone(), color);
    if full.size().x <= max_w || text.is_empty() {
        return full;
    }
    let ell = "…";
    let char_offsets: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut lo = 0usize;
    let mut hi = char_offsets.len().saturating_sub(2);
    let mut best: Option<std::sync::Arc<egui::Galley>> = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let candidate = &text[..char_offsets[mid]];
        let g = fonts.layout_no_wrap(format!("{candidate}{ell}"), font_id.clone(), color);
        if g.size().x <= max_w {
            best = Some(g);
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    best.unwrap_or(full)
}

/// Paint a single-line label centred in `rect`, ellipsizing when it
/// overflows the horizontal padding box. Used for mux group cells (which
/// carry no activity dot, unlike the plain-tab path).
fn paint_centered_label(ui: &Ui, rect: Rect, text: &str, color: egui::Color32) {
    let font_id = FontId::proportional(TAB_FONT_SIZE);
    let label_left = rect.left() + TAB_HORIZONTAL_PAD;
    let label_right = rect.right() - TAB_HORIZONTAL_PAD;
    let label_rect = Rect::from_min_max(
        egui::pos2(label_left, rect.top()),
        egui::pos2(label_right.max(label_left), rect.bottom()),
    );
    let max_w = label_rect.width().max(0.0);
    let galley = ui.fonts(|fonts| layout_ellipsized(fonts, text, &font_id, color, max_w));
    let pos = egui::pos2(
        label_rect.center().x - galley.size().x / 2.0,
        label_rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
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

// ── mux tab group render-model ───────────────────────────────────────────

/// One window sub-tab in a mux tab group, in left-to-right order. The widget
/// draws each cell labelled `[N] name` and a click switches to that window.
/// The model is built from the tab's
/// [`crate::mux::window_group::MuxWindowGroup`] by [`mux_group_render_model`].
///
/// WebView parity: an attached mux tab always renders one sub-tab per window
/// (no compact `mux (N)` cell, no expand/collapse toggle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxSubTabCell {
    /// Window position (0-based) within the group; the click target.
    pub index: usize,
    /// Window display name.
    pub name: String,
    /// Whether this is the active window (highlighted).
    pub active: bool,
}

/// Render-model for the mux tab group: one [`MuxSubTabCell`] per window, in
/// order, with the active window marked. Mirrors the WebView
/// `renderMuxSubTabs` (always one numbered sub-tab per window).
pub fn mux_group_render_model(
    group: &crate::mux::window_group::MuxWindowGroup,
) -> Vec<MuxSubTabCell> {
    let active = group.active_index();
    group
        .windows()
        .iter()
        .enumerate()
        .map(|(i, w)| MuxSubTabCell {
            index: i,
            name: w.name.clone(),
            active: i == active,
        })
        .collect()
}

/// The `[N] name` label shown on a sub-tab (number badge + window name),
/// mirroring the WebView `mux-window-number` + `tab-title` spans.
fn mux_sub_tab_label(cell: &MuxSubTabCell) -> String {
    format!("[{}] {}", cell.index + 1, cell.name)
}

#[cfg(test)]
mod tests;
