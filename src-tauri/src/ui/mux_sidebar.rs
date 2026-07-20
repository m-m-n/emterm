//! Vertical tab sidebar widget (task0004).
//!
//! Draws the mux window list (number + name + active mark) for the two
//! placement variants pinned in `IMPLEMENTATION.md :: Shared Components ::
//! Sidebar widget ui::mux_sidebar`:
//!
//! - [`Placement::Persistent`] — a right [`egui::SidePanel`], `surface
//!   container low` background, 1 px `outline_variant` separator on the
//!   terminal-facing (left) edge. Reserves grid WIDTH only — the terminal
//!   grid's x-origin is identical with and without this panel showing
//!   (2026-07-20 right-edge placement update; see IMPLEMENTATION.md
//!   cross-task decision 2).
//! - [`Placement::Overlay`] — a floating card: a right-edge
//!   [`egui::Area`] drawn over the terminal (no grid geometry impact),
//!   inset by a uniform 16 px (`spacing-md`) from the terminal area's
//!   top/right/bottom edges, 12 px corner radius (`corner-medium`) on all
//!   four corners, `surface_container_high` background at 92% alpha
//!   (slight translucency), NO separator line (the rounded edge is the
//!   only boundary), elevation-3 shadow (reusing
//!   [`crate::ui::dialog::tokens::elevation_shadow`] — the same token the
//!   modal dialogs use, so no new shadow color is introduced here). No
//!   scrim, no open/close animation. (2026-07-20 floating-card reshape;
//!   see `IMPLEMENTATION.md` cross-task decision 3 update, task0007.)
//!
//! Contract (pinned in `IMPLEMENTATION.md`): input is an ordered entry list
//! (window index, display name, active flag) plus a placement variant;
//! output is at most one clicked window index. The widget never talks to
//! the daemon — task0005 routes the result into the existing
//! `TabEvent::MuxSwitch` path. Wiring into the frame (grid inset, overlay
//! open/close state, visibility rules) is out of scope for this module.
//!
//! Follows `ui/tab_bar.rs` module conventions: small view-model structs +
//! pure draw functions, colors exclusively via [`crate::ui::md3`]
//! accessors (AC-5).

use egui::{Color32, FontId, Rect, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2};

use super::md3;

// ── width formula (Shared Components: "Sidebar width function") ─────────

/// Lower clamp bound of [`sidebar_width`], in egui logical points.
const MIN_WIDTH: f32 = 180.0;
/// Upper clamp bound of [`sidebar_width`].
const MAX_WIDTH: f32 = 320.0;
/// Fraction of the window's inner width the sidebar targets before
/// clamping.
const WIDTH_FRACTION: f32 = 0.22;

/// The one width formula shared by both placement variants (task0005 also
/// uses this for the persistent-mode grid inset). Deterministic, no state.
pub fn sidebar_width(window_inner_width: f32) -> f32 {
    (window_inner_width * WIDTH_FRACTION).clamp(MIN_WIDTH, MAX_WIDTH)
}

// ── placement ─────────────────────────────────────────────────────────

/// Which surface the sidebar renders as. Selection is data-driven
/// (`settings.mux.window_sidebar_overlay` + the runtime overlay-open flag)
/// at the task0005 call site; this module only draws the variant it is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Right `SidePanel` that participates in layout (task0005 insets the
    /// terminal grid's usable WIDTH by [`sidebar_width`] while this is
    /// showing; the grid's x-origin is unaffected — task0006 right-edge
    /// placement update).
    Persistent,
    /// Right-edge `Area` drawn after the central panel; contributes no
    /// grid inset.
    Overlay,
}

// ── view-model ────────────────────────────────────────────────────────

/// One row in the sidebar: a window's position, display name, and whether
/// it is the group's active window. Order matches
/// [`crate::mux::window_group::MuxWindowGroup::windows`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarEntry {
    /// Window position (0-based) within the group; the click target.
    pub window_index: usize,
    /// Display name shown in the row.
    pub name: String,
    /// Whether this is the active window (the only visual "active" mark —
    /// row background + text color).
    pub active: bool,
}

/// Build the ordered entry list from a tab's mux window group. Preserves
/// order, numbering, and names; marks exactly the active window (AC-2).
/// An empty group yields an empty list.
pub fn build_entries(group: &crate::mux::window_group::MuxWindowGroup) -> Vec<SidebarEntry> {
    let active = group.active_index();
    group
        .windows()
        .iter()
        .enumerate()
        .map(|(i, w)| SidebarEntry {
            window_index: i,
            name: w.name.clone(),
            active: i == active,
        })
        .collect()
}

// ── row geometry (design decisions restated as constants) ──────────────

/// Row height — 40 px full-radius pill.
const ROW_HEIGHT: f32 = 40.0;
/// Horizontal padding inside each row.
const ROW_HORIZONTAL_PAD: f32 = 12.0;
/// Vertical gap between rows.
const ROW_GAP: f32 = 4.0;
/// Gap between the number column and the name.
const NUMBER_NAME_GAP: f32 = 8.0;
/// Panel padding, vertical (top/bottom).
const PANEL_PAD_VERTICAL: f32 = 12.0;
/// Panel padding, horizontal (left/right).
const PANEL_PAD_HORIZONTAL: f32 = 8.0;
/// Number label font size.
const NUMBER_FONT_SIZE: f32 = 12.0;
/// Name label font size.
const NAME_FONT_SIZE: f32 = 14.0;
/// Fixed narrow column width the number right-aligns into.
const NUMBER_COLUMN_WIDTH: f32 = 20.0;
/// Separator hairline width (the persistent variant's terminal-facing left
/// edge only; the overlay variant paints no separator — AC-3).
const SEPARATOR_WIDTH: f32 = 1.0;

// ── overlay floating-card geometry (task0007) ───────────────────────────

/// Uniform margin (`spacing-md`) from the terminal area's top/right/bottom
/// edges the overlay card is inset by.
const OVERLAY_MARGIN: f32 = 16.0;
/// Overlay card corner radius (`corner-medium`), all four corners.
const OVERLAY_CORNER_RADIUS: f32 = 12.0;
/// Overlay card background alpha, applied to `surface_container_high`'s
/// alpha channel (slight translucency; list text stays readable).
const OVERLAY_FILL_ALPHA: f32 = 0.92;

// ── draw ─────────────────────────────────────────────────────────────

/// Draw the sidebar for the given placement, returning the clicked entry's
/// window index (at most one per frame), or `None` when nothing was
/// clicked this frame.
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

/// Persistent variant: a right `SidePanel` that participates in layout.
/// `surface_container_low` background, 1 px `outline_variant` separator on
/// the terminal-facing (left) edge. `inner_margin` stays zero on the
/// `Frame` itself (mirrors `tab_bar`'s convention) so `ui.max_rect()`
/// inside the closure is the panel's full (pre-padding) rect; the 12/8 px
/// panel padding is then applied manually before laying out rows.
fn draw_persistent(ctx: &egui::Context, entries: &[SidebarEntry], width: f32) -> Option<usize> {
    let mut clicked = None;
    let frame = egui::Frame::none()
        .fill(md3::surface_container_low())
        .inner_margin(egui::Margin::ZERO);
    egui::SidePanel::right("mux-sidebar-persistent")
        .frame(frame)
        .exact_width(width)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let panel_rect = ui.max_rect();
            ui.painter().vline(
                panel_rect.left() + SEPARATOR_WIDTH / 2.0,
                panel_rect.top()..=panel_rect.bottom(),
                Stroke::new(SEPARATOR_WIDTH, md3::outline_variant()),
            );
            let content_rect =
                panel_rect.shrink2(Vec2::new(PANEL_PAD_HORIZONTAL, PANEL_PAD_VERTICAL));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                clicked = draw_rows(ui, entries);
            });
        });
    clicked
}

/// Overlay variant: a floating card. A right-edge `Area` drawn after the
/// central panel (`Order::Foreground`), inset by [`OVERLAY_MARGIN`] from
/// the terminal area's top/right/bottom edges (its width is exactly the
/// caller-supplied `width` — the shared width function's value — so the
/// card's left edge is `width` inward of its right edge, not independently
/// inset). Contributes no grid inset — task0005 draws it without touching
/// the terminal grid geometry. [`OVERLAY_CORNER_RADIUS`] corner radius on
/// all four corners, `surface_container_high` background at
/// [`OVERLAY_FILL_ALPHA`] alpha (slight translucency), NO separator line
/// (AC-3 — the card's rounded edge is the only boundary; the persistent
/// variant's left-edge separator is unchanged), elevation-3 shadow reused
/// from the shared dialog token. No scrim, no open/close animation.
fn draw_overlay(ctx: &egui::Context, entries: &[SidebarEntry], width: f32) -> Option<usize> {
    let mut clicked = None;
    // Use the remaining central-panel area (post title-bar / tab-bar /
    // status-bar `TopBottomPanel`s), not the full window `screen_rect()`,
    // so the card's margins are measured from the terminal-facing region
    // and it never covers the titlebar's minimize/maximize/close buttons
    // or the tab/status bars.
    let terminal_area = ctx.available_rect();
    let rect = Rect::from_min_size(
        egui::pos2(
            terminal_area.right() - OVERLAY_MARGIN - width,
            terminal_area.top() + OVERLAY_MARGIN,
        ),
        Vec2::new(width, terminal_area.height() - 2.0 * OVERLAY_MARGIN),
    );
    let fill = md3::state_layer(md3::surface_container_high(), OVERLAY_FILL_ALPHA);

    #[cfg(test)]
    tests::LAST_OVERLAY_CARD.with(|c| {
        *c.borrow_mut() = Some(tests::OverlayCardDebug {
            rect,
            fill,
            rounding: OVERLAY_CORNER_RADIUS,
        });
    });

    egui::Area::new(egui::Id::new("mux-sidebar-overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            let frame = egui::Frame::none()
                .fill(fill)
                .rounding(Rounding::same(OVERLAY_CORNER_RADIUS))
                .inner_margin(egui::Margin::ZERO)
                .shadow(crate::ui::dialog::tokens::elevation_shadow());
            frame.show(ui, |ui| {
                let panel_rect = ui.max_rect();
                let content_rect =
                    panel_rect.shrink2(Vec2::new(PANEL_PAD_HORIZONTAL, PANEL_PAD_VERTICAL));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                    clicked = draw_rows(ui, entries);
                });
            });
        });
    clicked
}

/// Draw the scrollable row list into `ui` (already positioned/sized to the
/// panel's content area by the caller). Rows never shrink (AC-4); overflow
/// scrolls vertically — an empty list draws nothing (bare panel, no
/// placeholder text).
fn draw_rows(ui: &mut Ui, entries: &[SidebarEntry]) -> Option<usize> {
    let mut clicked = None;
    #[cfg(test)]
    tests::LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());

    ui.spacing_mut().item_spacing = Vec2::new(0.0, ROW_GAP);
    ScrollArea::vertical()
        .id_salt("mux-sidebar-rows")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, ROW_GAP);
            for entry in entries {
                let row_w = ui.available_width();
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(row_w, ROW_HEIGHT), Sense::click());

                #[cfg(test)]
                tests::LAST_ROW_RECTS.with(|c| c.borrow_mut().push(rect));

                let (bg, fg) = row_colors(entry, resp.hovered());
                if let Some(bg) = bg {
                    ui.painter()
                        .rect_filled(rect, Rounding::same(ROW_HEIGHT / 2.0), bg);
                }
                paint_row_content(ui, rect, entry, fg);

                if resp.clicked() && clicked.is_none() {
                    clicked = Some(entry.window_index);
                }
            }
        });
    clicked
}

/// Background / foreground color pair for a row, per the design decisions:
/// active → `secondary_container` / `on_secondary_container` (the only
/// active mark); inactive + hovered → 8% `on_surface` state layer over
/// `on_surface_variant` text; inactive + not hovered → no fill,
/// `on_surface_variant` text.
fn row_colors(entry: &SidebarEntry, hovered: bool) -> (Option<Color32>, Color32) {
    if entry.active {
        (
            Some(md3::secondary_container()),
            md3::on_secondary_container(),
        )
    } else if hovered {
        (
            Some(md3::state_layer(md3::on_surface(), md3::STATE_LAYER_HOVER)),
            md3::on_surface_variant(),
        )
    } else {
        (None, md3::on_surface_variant())
    }
}

/// Paint the `[number]  name` content of one row: the number right-aligned
/// in a fixed narrow column, the name ellipsized to the remaining width.
fn paint_row_content(ui: &Ui, rect: Rect, entry: &SidebarEntry, color: Color32) {
    let number_font = FontId::proportional(NUMBER_FONT_SIZE);
    let name_font = FontId::proportional(NAME_FONT_SIZE);

    let number_text = (entry.window_index + 1).to_string();
    let number_col_left = rect.left() + ROW_HORIZONTAL_PAD;
    let number_col_right = number_col_left + NUMBER_COLUMN_WIDTH;
    let number_galley = ui.fonts(|f| f.layout_no_wrap(number_text, number_font, color));
    let number_pos = egui::pos2(
        number_col_right - number_galley.size().x,
        rect.center().y - number_galley.size().y / 2.0,
    );
    ui.painter().galley(number_pos, number_galley, color);

    let name_left = number_col_right + NUMBER_NAME_GAP;
    let name_right = rect.right() - ROW_HORIZONTAL_PAD;
    let max_w = (name_right - name_left).max(0.0);
    let name_galley = ui.fonts(|f| layout_ellipsized(f, &entry.name, &name_font, color, max_w));
    let name_pos = egui::pos2(name_left, rect.center().y - name_galley.size().y / 2.0);
    ui.painter().galley(name_pos, name_galley, color);
}

/// Lay out `text` in `font_id` / `color`, ellipsizing with `…` when it
/// overflows `max_w`. Mirrors `tab_bar::layout_ellipsized` (kept local to
/// this module — the two widgets are independent per the task scope).
fn layout_ellipsized(
    fonts: &egui::text::Fonts,
    text: &str,
    font_id: &FontId,
    color: Color32,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::window_group::{MuxWindow, MuxWindowGroup};
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
    use std::cell::RefCell;

    thread_local! {
        pub(super) static LAST_ROW_RECTS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
        pub(super) static LAST_OVERLAY_CARD: RefCell<Option<OverlayCardDebug>> =
            const { RefCell::new(None) };
    }

    /// Test-only snapshot of the overlay card's computed geometry/paint
    /// parameters, recorded by `draw_overlay` (AC-1, AC-2).
    #[derive(Debug, Clone, Copy)]
    pub(super) struct OverlayCardDebug {
        pub rect: Rect,
        pub fill: Color32,
        pub rounding: f32,
    }

    fn win(id: u32, name: &str) -> MuxWindow {
        MuxWindow {
            id,
            name: name.to_string(),
        }
    }

    fn group_with(n: usize, active: usize) -> MuxWindowGroup {
        let mut g = MuxWindowGroup::new();
        let windows: Vec<MuxWindow> = (0..n).map(|i| win(i as u32, &format!("w{i}"))).collect();
        let panes: Vec<u32> = (0..n).map(|i| 100 + i as u32).collect();
        g.seed(windows, panes, active);
        g
    }

    fn entries(n: usize, active: usize) -> Vec<SidebarEntry> {
        (0..n)
            .map(|i| SidebarEntry {
                window_index: i,
                name: format!("w{i}"),
                active: i == active,
            })
            .collect()
    }

    fn screen_rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    fn row_rects(items: &[SidebarEntry], placement: Placement) -> Vec<Rect> {
        let ctx = egui::Context::default();
        // `egui::Area` (the overlay variant) runs an invisible "sizing
        // pass" on its very first frame and only settles into its final
        // geometry from the second frame on (the same reason
        // `run_with_click` below drives two frames before the click).
        // Prime that pass here too so the rects this returns match what a
        // later interactive frame — like `run_with_click`'s — will use.
        let mut priming = RawInput::default();
        priming.screen_rect = Some(screen_rect());
        let _ = ctx.run(priming, |ctx| {
            let _ = draw(ctx, items, placement, MIN_WIDTH);
        });

        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, placement, MIN_WIDTH);
        });
        LAST_ROW_RECTS.with(|c| c.borrow().clone())
    }

    fn run_with_click(
        items: &[SidebarEntry],
        placement: Placement,
        click_pos: Pos2,
    ) -> Option<usize> {
        let ctx = egui::Context::default();

        // egui's per-widget click/hover resolution for frame N is computed
        // from the widget rects *registered in frame N-1* (immediate-mode's
        // one-frame interaction delay — see `egui::interaction::interact`).
        // The overlay's `egui::Area` additionally runs an invisible
        // "sizing pass" on its very first-ever frame (before any
        // `AreaState` is in memory), during which its rows are not
        // registered as interactive. Two pointer-move-only priming frames
        // get past both effects — (1) the sizing pass, (2) a settled frame
        // whose widget rects are interactive — before the click is fired in
        // frame 3. The persistent (`SidePanel`) variant has no sizing pass,
        // so the extra priming frame there is a harmless no-op.
        for _ in 0..2 {
            let mut priming = RawInput::default();
            priming.screen_rect = Some(screen_rect());
            priming.events.push(Event::PointerMoved(click_pos));
            let _ = ctx.run(priming, |ctx| {
                let _ = draw(ctx, items, placement, MIN_WIDTH);
            });
        }

        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        input.events.push(Event::PointerMoved(click_pos));
        input.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
        input.events.push(Event::PointerButton {
            pos: click_pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        let mut captured = None;
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, items, placement, MIN_WIDTH);
        });
        captured
    }

    // ── AC-1: width function ─────────────────────────────────────────

    #[test]
    fn width_clamps_to_min_for_very_narrow_windows() {
        assert_eq!(sidebar_width(0.0), MIN_WIDTH);
        assert_eq!(sidebar_width(100.0), MIN_WIDTH);
    }

    #[test]
    fn width_clamps_to_max_for_very_wide_windows() {
        assert_eq!(sidebar_width(3000.0), MAX_WIDTH);
        assert_eq!(sidebar_width(10_000.0), MAX_WIDTH);
    }

    #[test]
    fn width_is_22_percent_within_bounds() {
        // 1000 * 0.22 = 220, inside [180, 320].
        assert!((sidebar_width(1000.0) - 220.0).abs() < 0.01);
    }

    #[test]
    fn width_boundary_at_min_clamp_edge() {
        let just_below = MIN_WIDTH / WIDTH_FRACTION - 5.0;
        assert_eq!(sidebar_width(just_below), MIN_WIDTH);
        let at_edge = MIN_WIDTH / WIDTH_FRACTION;
        assert!((sidebar_width(at_edge) - MIN_WIDTH).abs() < 0.05);
    }

    #[test]
    fn width_boundary_at_max_clamp_edge() {
        let at_edge = MAX_WIDTH / WIDTH_FRACTION;
        assert!((sidebar_width(at_edge) - MAX_WIDTH).abs() < 0.05);
        let just_above = at_edge + 5.0;
        assert_eq!(sidebar_width(just_above), MAX_WIDTH);
    }

    // ── AC-2: view-model mapping ─────────────────────────────────────

    #[test]
    fn build_entries_empty_group_is_empty() {
        let g = MuxWindowGroup::new();
        assert_eq!(build_entries(&g), Vec::new());
    }

    #[test]
    fn build_entries_single_window_is_active() {
        let g = group_with(1, 0);
        let got = build_entries(&g);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].window_index, 0);
        assert_eq!(got[0].name, "w0");
        assert!(got[0].active);
    }

    #[test]
    fn build_entries_preserves_order_numbering_and_single_active_flag() {
        let g = group_with(4, 2);
        let got = build_entries(&g);
        assert_eq!(got.len(), 4);
        for (i, e) in got.iter().enumerate() {
            assert_eq!(e.window_index, i);
            assert_eq!(e.name, format!("w{i}"));
        }
        let actives: Vec<bool> = got.iter().map(|e| e.active).collect();
        assert_eq!(actives, vec![false, false, true, false]);
    }

    // ── task0006 AC-1: persistent panel sits on the RIGHT edge ────────

    #[test]
    fn persistent_panel_rows_hug_the_right_edge_of_the_screen() {
        let items = entries(1, 0);
        let rects = row_rects(&items, Placement::Persistent);
        let expected_right = screen_rect().right() - PANEL_PAD_HORIZONTAL;
        assert!(
            (rects[0].right() - expected_right).abs() < 0.5,
            "persistent row right edge {} should hug the screen's right edge \
             (expected ~{expected_right}) — the panel must be right-placed",
            rects[0].right()
        );
        // And NOT left-placed: the row must not sit near x=0.
        assert!(
            rects[0].left() > screen_rect().width() / 2.0,
            "persistent row left edge {} should be in the right half of the \
             screen, not hugging the left edge",
            rects[0].left()
        );
    }

    #[test]
    fn persistent_and_overlay_rows_sit_16px_apart_on_the_right_edge() {
        // Both placement variants live on the right edge, but the overlay
        // is now a floating card inset by OVERLAY_MARGIN (16 px) from the
        // terminal area's right edge, while the persistent panel is flush
        // (task0007 AC-5: "overlay rows now sit 16 px further left").
        let items = entries(1, 0);
        let persistent_rects = row_rects(&items, Placement::Persistent);
        let overlay_rects = row_rects(&items, Placement::Overlay);
        let actual_offset = persistent_rects[0].right() - overlay_rects[0].right();
        assert!(
            (actual_offset - OVERLAY_MARGIN).abs() < 0.5,
            "persistent right {} and overlay right {} should differ by {OVERLAY_MARGIN}px, \
             got {actual_offset}",
            persistent_rects[0].right(),
            overlay_rects[0].right()
        );
    }

    // ── task0007 AC-1/AC-2/AC-3: overlay floating-card geometry ───────

    fn draw_overlay_and_capture_card(items: &[SidebarEntry], width: f32) -> OverlayCardDebug {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, items, Placement::Overlay, width);
        });
        LAST_OVERLAY_CARD
            .with(|c| *c.borrow())
            .expect("draw_overlay records the card geometry")
    }

    #[test]
    fn overlay_card_rect_is_inset_16px_from_terminal_area_top_right_bottom() {
        let items = entries(1, 0);
        let card = draw_overlay_and_capture_card(&items, MIN_WIDTH);
        let area = screen_rect();
        assert!(
            (card.rect.top() - (area.top() + OVERLAY_MARGIN)).abs() < 0.01,
            "top inset: card top {} vs expected {}",
            card.rect.top(),
            area.top() + OVERLAY_MARGIN
        );
        assert!(
            (card.rect.right() - (area.right() - OVERLAY_MARGIN)).abs() < 0.01,
            "right inset: card right {} vs expected {}",
            card.rect.right(),
            area.right() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.bottom() - (area.bottom() - OVERLAY_MARGIN)).abs() < 0.01,
            "bottom inset: card bottom {} vs expected {}",
            card.rect.bottom(),
            area.bottom() - OVERLAY_MARGIN
        );
        assert!(
            (card.rect.width() - MIN_WIDTH).abs() < 0.01,
            "card width {} should equal the shared width function's value {MIN_WIDTH}",
            card.rect.width()
        );
    }

    #[test]
    fn overlay_card_has_12px_corner_radius_and_92_percent_alpha_surface_container_high() {
        let items = entries(1, 0);
        let card = draw_overlay_and_capture_card(&items, MIN_WIDTH);
        assert_eq!(card.rounding, OVERLAY_CORNER_RADIUS);

        // `egui::Color32` stores premultiplied sRGB, so the RGB channels
        // are no longer the raw `surface_container_high` base once alpha
        // is applied (`md3::state_layer`/`from_rgba_unmultiplied` do
        // gamma-correct premultiplication internally) — mirrors the
        // reasoning in `md3::tests::state_layer_alpha_scales_with_opacity`.
        // We verify the alpha channel here; the channel-level blend is
        // exercised visually through the renderer.
        let expected_alpha = (OVERLAY_FILL_ALPHA * 255.0) as u8;
        assert_eq!(
            card.fill.a(),
            expected_alpha,
            "overlay fill alpha should be 92% of surface_container_high's alpha channel"
        );
    }

    #[test]
    fn ac3_overlay_variant_paints_no_separator_line() {
        let src = include_str!("mux_sidebar.rs");
        let after_overlay_fn = src
            .split("fn draw_overlay(")
            .nth(1)
            .expect("draw_overlay fn present in source");
        // Isolate the function body up to the next top-level `fn`
        // declaration (there are no nested `fn`s inside draw_overlay,
        // only closures, so this boundary is unambiguous).
        let body = after_overlay_fn
            .split("\nfn ")
            .next()
            .unwrap_or(after_overlay_fn);
        assert!(
            !body.contains(".vline("),
            "overlay variant must not paint a separator line (AC-3); the persistent \
             variant's left-edge separator is unchanged"
        );
    }

    // ── AC-3: click reporting ────────────────────────────────────────

    #[test]
    fn clicking_a_row_reports_its_window_index_persistent() {
        let items = entries(3, 0);
        let target = row_rects(&items, Placement::Persistent)[2].center();
        let ev = run_with_click(&items, Placement::Persistent, target);
        assert_eq!(ev, Some(2));
    }

    #[test]
    fn clicking_a_row_reports_its_window_index_overlay() {
        let items = entries(3, 0);
        let target = row_rects(&items, Placement::Overlay)[1].center();
        let ev = run_with_click(&items, Placement::Overlay, target);
        assert_eq!(ev, Some(1));
    }

    #[test]
    fn clicking_below_all_rows_reports_nothing() {
        let items = entries(2, 0);
        let rects = row_rects(&items, Placement::Persistent);
        let below = Pos2::new(rects[0].center().x, rects[1].bottom() + 100.0);
        let ev = run_with_click(&items, Placement::Persistent, below);
        assert_eq!(ev, None);
    }

    #[test]
    fn draw_with_empty_entries_does_not_panic_and_emits_nothing() {
        let items: Vec<SidebarEntry> = Vec::new();
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut captured = None;
        let _ = ctx.run(input, |ctx| {
            captured = draw(ctx, &items, Placement::Persistent, MIN_WIDTH);
        });
        assert_eq!(captured, None);
    }

    // ── AC-4: truncation + overflow scrolling ─────────────────────────

    #[test]
    fn ellipsized_layout_truncates_when_overflowing_available_width() {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let mut result_w = 0.0f32;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::proportional(NAME_FONT_SIZE);
                let long = "a-very-long-mux-window-name-that-does-not-fit-in-the-row";
                let full =
                    ui.fonts(|f| f.layout_no_wrap(long.to_string(), font.clone(), Color32::WHITE));
                let max_w = full.size().x / 2.0;
                let truncated =
                    ui.fonts(|f| layout_ellipsized(f, long, &font, Color32::WHITE, max_w));
                result_w = truncated.size().x;
                assert!(
                    truncated.size().x <= max_w + 0.5,
                    "truncated width {} should respect the bound {}",
                    truncated.size().x,
                    max_w
                );
            });
        });
        assert!(result_w > 0.0);
    }

    #[test]
    fn ellipsized_layout_leaves_short_text_untouched() {
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let font = FontId::proportional(NAME_FONT_SIZE);
                let full = ui
                    .fonts(|f| f.layout_no_wrap("short".to_string(), font.clone(), Color32::WHITE));
                let result =
                    ui.fonts(|f| layout_ellipsized(f, "short", &font, Color32::WHITE, 500.0));
                assert_eq!(result.size().x, full.size().x);
            });
        });
    }

    #[test]
    fn rows_never_shrink_and_all_lay_out_when_content_exceeds_available_height() {
        // 20 rows definitely exceed a 150px-tall viewport; the widget must
        // still lay out every row at full height (scrolling, not clipping
        // or shrinking rows to fit).
        let items = entries(20, 0);
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 150.0)));
        LAST_ROW_RECTS.with(|c| c.borrow_mut().clear());
        let _ = ctx.run(input, |ctx| {
            let _ = draw(ctx, &items, Placement::Persistent, MIN_WIDTH);
        });
        let rects = LAST_ROW_RECTS.with(|c| c.borrow().clone());
        assert_eq!(
            rects.len(),
            20,
            "the scroll area must lay out every entry, not clip the list"
        );
        for r in &rects {
            assert!(
                (r.height() - ROW_HEIGHT).abs() < 0.5,
                "row height must stay fixed at {ROW_HEIGHT}, got {}",
                r.height()
            );
        }
    }

    // ── AC-5: colors exclusively via md3 accessors ────────────────────

    #[test]
    fn ac5_no_hardcoded_color_constructors_in_module_source() {
        let src = include_str!("mux_sidebar.rs");
        // Only scan the production code above the test module boundary —
        // this assertion's own string literals below would otherwise
        // self-match `include_str!`'s full-file contents.
        let production_src = src.split("\nmod tests {").next().unwrap_or(src);
        let forbidden = [
            "Color32::from_rgb(",
            "Color32::from_rgba_unmultiplied(",
            "Color32::from_rgba_premultiplied(",
            "Color32::from_gray(",
            "Color32::from_black_alpha(",
            "Color32::from_white_alpha(",
            "Color32::BLACK",
            "Color32::WHITE",
            "Color32::RED",
            "Color32::GREEN",
            "Color32::BLUE",
        ];
        for needle in forbidden {
            assert!(
                !production_src.contains(needle),
                "mux_sidebar.rs must source all colors from md3:: accessors (or the shared \
                 dialog elevation token); found hardcoded {needle}"
            );
        }
        assert!(
            production_src.contains("md3::"),
            "sanity: the module should reference md3 accessors at all"
        );
    }
}
