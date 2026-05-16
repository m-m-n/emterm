//! Tab bar widget (Phase 4-B).
//!
//! Renders an egui top panel with one button per tab plus a "+" affordance,
//! and emits at most one [`TabEvent`] per frame so the app loop can apply
//! the change atomically.
//!
//! The widget is decoupled from [`crate::tabs::Tab`] by taking a slice of
//! lightweight [`TabBarItem`] values. The caller is expected to project
//! its `Vec<Tab>` into a `Vec<TabBarItem>` once per frame. Tests construct
//! these directly without needing a PTY or terminal core.
//!
//! Layout rules (TS-tab-2 / acceptance):
//!
//! - Fixed-height top panel.
//! - One button per tab, equal width distributed across the available
//!   space with `MIN_TAB_WIDTH` as the floor. When the total minimum
//!   width exceeds the panel, egui's `ScrollArea` enables horizontal
//!   scrolling.
//! - Active tab is visually marked by an underline strip plus a slightly
//!   elevated background.
//! - Each tab carries a small "×" close affordance on its right edge.
//! - The trailing "+" button appends a new tab.
//!
//! Title rendering (TS-tab-3):
//!
//! - When `mux_session_name` is `Some(name)`, the rendered title is
//!   `[mux:name] <title>` (with a single space between the prefix and
//!   the PTY title). When `None`, the title is rendered verbatim.

use egui::{Align, Color32, Layout, RichText, ScrollArea, Sense, Stroke, Ui, Vec2};

use super::TabEvent;

/// Fixed visual height of the tab strip in egui logical points.
pub const TAB_BAR_HEIGHT: f32 = 28.0;
/// Minimum width of a single tab button before horizontal scroll
/// kicks in.
const MIN_TAB_WIDTH: f32 = 80.0;
/// Width reserved for the trailing "+" affordance.
const NEW_TAB_BUTTON_WIDTH: f32 = 28.0;
/// Internal padding inside a tab button.
const TAB_INNER_PAD: f32 = 6.0;

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
}

impl TabBarItem {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            mux_session_name: None,
        }
    }

    pub fn with_mux_session(mut self, name: impl Into<String>) -> Self {
        self.mux_session_name = Some(name.into());
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
    egui::TopBottomPanel::top("native-poc-tab-bar")
        .exact_height(TAB_BAR_HEIGHT)
        .show(ctx, |ui| {
            // Equal-width distribution sized to the panel; if the per-
            // tab floor is exceeded by total tab width we wrap the
            // strip in a horizontal `ScrollArea`.
            let n = items.len().max(1) as f32;
            let panel_w = ui.available_width();
            let needed_w = MIN_TAB_WIDTH * n + NEW_TAB_BUTTON_WIDTH;
            if needed_w > panel_w {
                ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        event = layout_tab_strip(ui, items, active_idx, MIN_TAB_WIDTH);
                    });
            } else {
                let tab_width = ((panel_w - NEW_TAB_BUTTON_WIDTH) / n).max(MIN_TAB_WIDTH);
                event = layout_tab_strip(ui, items, active_idx, tab_width);
            }
        });
    event
}

/// Inner layout: lay out one button per tab + the "+" button at the
/// right edge.
fn layout_tab_strip(
    ui: &mut Ui,
    items: &[TabBarItem],
    active_idx: usize,
    tab_width: f32,
) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;

    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);

        for (i, item) in items.iter().enumerate() {
            let close_w = 18.0;
            // Reserve space for the close button plus padding.
            let label_w = (tab_width - close_w - 2.0 * TAB_INNER_PAD)
                .max(MIN_TAB_WIDTH - close_w - 2.0 * TAB_INNER_PAD);

            let is_active = i == active_idx;
            let (bg, underline) = if is_active {
                (
                    ui.visuals().widgets.active.bg_fill,
                    Some(ui.visuals().selection.stroke),
                )
            } else {
                (ui.visuals().widgets.inactive.bg_fill, None)
            };

            let label = render_label(item);
            let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);

            let (rect, _) = ui.allocate_exact_size(cell_size, Sense::hover());
            // Background fill for the whole cell so the active state
            // shows behind both the title and the close button.
            ui.painter().rect_filled(rect, 0.0, bg);

            // Inner content: title label (left) + close × (right).
            let label_rect = egui::Rect::from_min_size(
                rect.min + Vec2::new(TAB_INNER_PAD, 2.0),
                Vec2::new(label_w, TAB_BAR_HEIGHT - 4.0),
            );
            let close_rect = egui::Rect::from_min_size(
                egui::pos2(rect.right() - TAB_INNER_PAD - close_w, rect.top() + 2.0),
                Vec2::new(close_w, TAB_BAR_HEIGHT - 4.0),
            );

            let title_label = if is_active {
                RichText::new(label.clone()).strong()
            } else {
                RichText::new(label.clone())
            };
            let title_resp = ui.put(
                label_rect,
                egui::Label::new(title_label)
                    .truncate()
                    .selectable(false)
                    .sense(Sense::click()),
            );
            if title_resp.clicked() && !is_active && event.is_none() {
                event = Some(TabEvent::Switch(i));
            }

            let close_resp = ui.put(close_rect, egui::Button::new("×").frame(false));
            if close_resp.clicked() && event.is_none() {
                event = Some(TabEvent::Close(i));
            }

            // Active-tab underline (drawn on top of the cell).
            if let Some(stroke) = underline {
                let y = rect.bottom() - 1.5;
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.left() + 2.0, y),
                        egui::pos2(rect.right() - 2.0, y),
                    ],
                    Stroke::new(stroke.width.max(1.5), stroke.color),
                );
            } else {
                let y = rect.bottom() - 0.5;
                ui.painter().line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    Stroke::new(1.0, Color32::from_gray(40)),
                );
            }
        }

        // "+" — append a new tab. Allocate explicit size to keep the
        // hit-box stable across screen widths (tests rely on this).
        let plus_size = Vec2::new(NEW_TAB_BUTTON_WIDTH, TAB_BAR_HEIGHT - 4.0);
        let plus_resp = ui.add_sized(plus_size, egui::Button::new("+"));
        #[cfg(test)]
        {
            tests::LAST_PLUS_RECT.with(|c| c.set(Some(plus_resp.rect)));
        }
        if plus_resp.clicked() && event.is_none() {
            event = Some(TabEvent::New);
        }
    });

    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
    use std::cell::Cell;

    // Test-only hook: the widget stores the rect of the "+" button in
    // here at the end of every layout pass so tests can compute a
    // synthetic click position deterministically.
    thread_local! {
        pub(super) static LAST_PLUS_RECT: Cell<Option<Rect>> = const { Cell::new(None) };
    }

    fn item(title: &str) -> TabBarItem {
        TabBarItem::new(title)
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 100.0))
    }

    /// Capture the screen position of the first hovered widget by
    /// running one egui pass; useful for "where did the tab end up".
    fn pos_of_first_hovered(ctx: &egui::Context) -> Option<Pos2> {
        ctx.pointer_latest_pos()
    }

    /// Drive a single egui frame: layout + a synthetic mouse press +
    /// release at `click_pos`. Returns whatever `TabEvent` the widget
    /// emitted across the two passes.
    fn run_with_click(
        items: &[TabBarItem],
        active_idx: usize,
        click_pos: Pos2,
    ) -> Option<TabEvent> {
        let ctx = egui::Context::default();

        // First pass: lay out the widget so egui knows where the buttons
        // are. Hover the pointer over the click target.
        let mut input1 = RawInput::default();
        input1.screen_rect = Some(screen_rect());
        input1.events.push(Event::PointerMoved(click_pos));
        let mut captured: Option<TabEvent> = None;
        let _ = ctx.run(input1, |ctx| {
            captured = draw(ctx, items, active_idx);
        });

        // Second pass: press + release at the same position. egui
        // requires a press and a release in the same frame for
        // `.clicked()` to fire.
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
            // Silence unused-pos helper warning in some builds.
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
        // Phase 4-B contract: Tab::title defaults to "shell" when no
        // OSC title was set. The widget should not rewrite it.
        let it = item("shell");
        assert_eq!(render_label(&it), "shell");
    }

    // ── TS-tab-1: simulated interaction → TabEvent ──────────

    #[test]
    fn clicking_plus_emits_new() {
        // The "+" button position depends on the panel's actual usable
        // width (egui inserts side margins inside the top panel). We
        // run one layout pass to record its rect, then click its
        // centre on the second pass.
        let items = vec![item("a"), item("b")];

        // Pass 1: layout only — capture the rect.
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, &items, 0);
        });
        let rect = LAST_PLUS_RECT
            .with(|c| c.get())
            .expect("+ button rect should be captured during layout");
        let click_pos = rect.center();

        // Pass 2 / 3: drive press + release at the centre.
        let ev = run_with_click(&items, 0, click_pos);
        assert_eq!(ev, Some(TabEvent::New));
    }

    #[test]
    fn clicking_inactive_tab_emits_switch() {
        // Two tabs, ~ (800 - 28)/2 = ~386 px wide each. Aim for the
        // middle of the second tab's title area, well away from its
        // close-button column.
        let items = vec![item("alpha"), item("beta")];
        let ev = run_with_click(&items, 0, Pos2::new(450.0, 14.0));
        assert_eq!(ev, Some(TabEvent::Switch(1)));
    }

    #[test]
    fn clicking_close_on_first_tab_emits_close_zero() {
        // Tab 0's close × sits near the right edge of its frame:
        // roughly column = tab_width - inner_pad - close_w/2.
        // tab_width ≈ (800 - 28) / 2 ≈ 386 ⇒ close near x ≈ 372.
        let items = vec![item("alpha"), item("beta")];
        let ev = run_with_click(&items, 0, Pos2::new(372.0, 14.0));
        assert_eq!(ev, Some(TabEvent::Close(0)));
    }

    // ── A "no items" edge case is not expected; draw() requires the
    //    caller to keep at least one tab in the vector. We do guard
    //    against div-by-zero internally via `.max(1)`.

    #[test]
    fn draw_with_single_tab_does_not_panic_and_emits_nothing_without_click() {
        let items = vec![item("solo")];
        // No click events — just lay out and assert no event escapes.
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
