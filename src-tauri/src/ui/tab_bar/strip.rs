//! Tab-strip inner layout: per-item cell layout, drag-and-drop state
//! and drop-position resolution.

use egui::{FontId, Rect, Rounding, Sense, Stroke, Ui, Vec2};

use super::badge::{AGENT_BADGE_GAP, AGENT_BADGE_SLOT_WIDTH, paint_agent_badge};
#[cfg(test)]
use super::tests;
use super::{
    ACTIVITY_DOT_ANIM_SECS, ACTIVITY_DOT_DIAMETER, ACTIVITY_DOT_MARGIN, HAIRLINE_HEIGHT,
    TAB_BAR_HEIGHT, TAB_FONT_SIZE, TAB_HORIZONTAL_PAD, TabBarItem, layout_ellipsized,
    mux_sub_tab_label, paint_active_indicator, paint_centered_label, render_label,
};
use crate::ui::TabEvent;
use crate::ui::emoji_cache::EmojiResources;
use crate::ui::md3;

/// Persistent key under which the current drag origin (`Option<usize>`)
/// is stored in egui's frame memory. Survives across frames so the
/// pending drag is observed by every layout pass until the pointer is
/// released.
const DRAG_FROM_KEY: &str = "native-poc-tab-drag-from";

fn drag_state_id() -> egui::Id {
    egui::Id::new(DRAG_FROM_KEY)
}

/// One drawable cell in the strip: either a plain roster tab or a single
/// cell of a mux tab group belonging to a roster tab.
enum Visual {
    /// Plain roster tab at index `item`.
    Tab { item: usize },
    /// Mux group cell at position `cell` within `items[tab].mux_cells`.
    Mux { tab: usize, cell: usize },
}

/// Count the visual cells the strip renders: a plain tab is one cell; a
/// mux group expands to its cell count (compact → 1, expanded → header +
/// one per window). Used so the equal-width layout math accounts for the
/// expansion. With no mux groups this equals `items.len()`.
pub(in crate::ui::tab_bar) fn visual_cell_count(items: &[TabBarItem]) -> usize {
    items
        .iter()
        .map(|it| match &it.mux_cells {
            Some(cells) if !cells.is_empty() => cells.len(),
            _ => 1,
        })
        .sum()
}

/// Flatten the roster into the ordered visual cells the strip draws.
fn build_visuals(items: &[TabBarItem]) -> Vec<Visual> {
    let mut visuals = Vec::with_capacity(visual_cell_count(items));
    for (i, item) in items.iter().enumerate() {
        match &item.mux_cells {
            Some(cells) if !cells.is_empty() => {
                for c in 0..cells.len() {
                    visuals.push(Visual::Mux { tab: i, cell: c });
                }
            }
            _ => visuals.push(Visual::Tab { item: i }),
        }
    }
    visuals
}

/// Inner layout: lay out one tab cell per item. Returns at most one
/// [`TabEvent`] this frame.
pub(in crate::ui::tab_bar) fn layout_tab_strip(
    ui: &mut Ui,
    items: &[TabBarItem],
    active_idx: usize,
    tab_width: f32,
    scroll_active_into_view: bool,
    emoji: Option<&EmojiResources<'_>>,
) -> Option<TabEvent> {
    let mut event: Option<TabEvent> = None;
    let drag_id = drag_state_id();
    let mut drag_from: Option<usize> = ui.ctx().memory(|m| m.data.get_temp(drag_id));

    // FR4: the active visual cell's rect, captured during the layout pass so a
    // single `scroll_to_rect` can pull it into view when the flag is set. The
    // active cell is the plain-tab cell at `active_idx`, or — inside the active
    // mux tab — the active window's sub-tab cell.
    let mut active_cell_rect: Option<Rect> = None;

    let visuals = build_visuals(items);
    // Drag-reorder applies to plain-tab cells only — mux sub-tab cells (a
    // group's expanded `[N] name` cells) carry `Sense::click()` not
    // `click_and_drag()`, so the cell-level drag is naturally restricted
    // there. The post-loop drop math works over `cell_rects` (plain tabs
    // only) and uses `cell_rosters[cell_idx]` to map the drop-target cell
    // index back to a roster insertion point. With this mapping, plain-tab
    // reorder stays live even when one tab is expanded as a mux group
    // (pre-fix the whole drag path was disabled globally when any tab was
    // mux-attached — coarse coupling between unrelated features).
    let mut cell_rects: Vec<Rect> = Vec::with_capacity(items.len());
    let mut cell_rosters: Vec<usize> = Vec::with_capacity(items.len());

    #[cfg(test)]
    tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().clear());
    #[cfg(test)]
    tests::LAST_MUX_CELLS.with(|c| c.borrow_mut().clear());
    #[cfg(test)]
    tests::LAST_INDICATOR_RECTS.with(|c| c.borrow_mut().clear());

    for visual in &visuals {
        let i = match *visual {
            Visual::Tab { item } => item,
            Visual::Mux { tab, cell } => {
                // ── mux window sub-tab (`[N] name`) ─────────────────
                let mux_cell = &items[tab].mux_cells.as_ref().expect("mux group cells")[cell];
                let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);
                let (rect, cell_resp) = ui.allocate_exact_size(cell_size, Sense::click());

                #[cfg(test)]
                tests::LAST_MUX_CELLS.with(|c| c.borrow_mut().push(rect));

                let is_active_cell = mux_cell.active;
                let color = if is_active_cell {
                    md3::primary()
                } else {
                    md3::on_surface_variant()
                };

                if cell_resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        Rounding::ZERO,
                        md3::state_layer(color, md3::STATE_LAYER_HOVER),
                    );
                }
                paint_centered_label(ui, rect, &mux_sub_tab_label(mux_cell), color);
                // FR5: paint the sub-tab active-indicator bar only when this
                // mux group's parent tab is the active tab. A non-active mux
                // parent shows no bar, so exactly one indicator is visible
                // across the whole strip. The label color above keeps its
                // existing `mux_cell.active`-based emphasis (only the bar is
                // gated). `tab` and `active_idx` are plain indices and
                // `mux_cell.active` is a copied bool, so the gate never touches
                // `MuxWindowGroup` (active-window state is unchanged).
                if tab == active_idx && is_active_cell {
                    paint_active_indicator(ui, rect);
                    // FR4: the active visual cell inside the active mux tab.
                    active_cell_rect = Some(rect);
                }

                // Click switches to this window (WebView parity: sub-tab
                // click → switch; there is no compact/expand toggle).
                if cell_resp.clicked() && event.is_none() {
                    event = Some(TabEvent::MuxSwitch {
                        tab,
                        window: mux_cell.index,
                    });
                }
                continue;
            }
        };

        let item = &items[i];
        let is_active = i == active_idx;
        let cell_size = Vec2::new(tab_width, TAB_BAR_HEIGHT);
        let (rect, cell_resp) = ui.allocate_exact_size(cell_size, Sense::click_and_drag());

        cell_rects.push(rect);
        cell_rosters.push(i);
        #[cfg(test)]
        tests::LAST_TAB_CELLS.with(|c| c.borrow_mut().push(rect));

        // Detect drag start. egui's `drag_started_by` fires the frame
        // after the pointer exceeds the click-vs-drag distance, so a
        // simple click does not enter drag mode.
        if drag_from.is_none() && cell_resp.drag_started_by(egui::PointerButton::Primary) {
            drag_from = Some(i);
            ui.ctx().memory_mut(|m| m.data.insert_temp(drag_id, i));
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
        // Agent-status badge slot (task0006 AC-1/AC-2): unlike the activity
        // dot below, this slot is only reserved when a badge is present —
        // no reserved space and no layout shift for a tab that has never
        // reported a state.
        let agent_dot_space = if item.agent_badge.is_some() {
            AGENT_BADGE_SLOT_WIDTH + AGENT_BADGE_GAP
        } else {
            0.0
        };
        // Activity-dot slot. Like the WebView flexbox (`.tab-activity-dot`
        // hides via opacity/scale, not display:none), the 8 px dot +
        // 6 px gap always occupy layout space so the title does not
        // shift when the dot appears.
        let dot_space = ACTIVITY_DOT_DIAMETER + ACTIVITY_DOT_MARGIN;
        // egui has no native truncation helper for direct painter text,
        // so we measure with `Fonts::layout_no_wrap` and ellipsize when
        // the result overflows the label rect.
        let max_w = (label_rect.width() - agent_dot_space - dot_space).max(0.0);
        let galley =
            ui.fonts(|fonts| layout_ellipsized(fonts, &label_text, &font_id, text_color, max_w));
        // Centre the [agent badge][dot][gap][title] group as one unit,
        // mirroring the WebView's `justify-content: center` flex row.
        let group_w = agent_dot_space + dot_space + galley.size().x;
        let group_left = label_rect.center().x - group_w / 2.0;

        if let Some(badge) = item.agent_badge {
            let badge_center = egui::pos2(
                group_left + AGENT_BADGE_SLOT_WIDTH / 2.0,
                label_rect.center().y,
            );
            paint_agent_badge(ui, badge_center, badge, emoji);
        }
        let after_agent_badge = group_left + agent_dot_space;

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
                after_agent_badge + ACTIVITY_DOT_DIAMETER / 2.0,
                label_rect.center().y,
            );
            ui.painter().circle_filled(
                dot_center,
                (ACTIVITY_DOT_DIAMETER / 2.0) * dot_t,
                md3::primary().gamma_multiply(dot_t),
            );
        }

        let text_x = after_agent_badge + dot_space;
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
            paint_active_indicator(ui, rect);
            // FR4: the active plain-tab cell.
            active_cell_rect = Some(rect);
        }
    }

    // Post-loop: handle drag-in-progress (indicator) and drop (event).
    // `cell_rects` holds only plain-tab cells; we use `cell_rosters` to map
    // a drop-target cell index back to a roster insertion point so a mux
    // group expanding in the middle of the strip doesn't break drag math.
    if cell_rects.is_empty() {
        // Strip is all mux cells (no plain tabs); clean up any latched drag.
        if drag_from.is_some() && ui.input(|i| i.pointer.any_released()) {
            ui.ctx().memory_mut(|m| m.data.remove::<usize>(drag_id));
        }
    } else if let Some(from) = drag_from {
        // `latest_pos` survives across release frames, unlike
        // `interact_pos` which returns `None` once the pointer leaves
        // the interaction state (e.g. on the release frame itself).
        let pointer_pos = ui.input(|i| i.pointer.latest_pos());
        let target_cell = pointer_pos.map(|p| drop_target_index(&cell_rects, p.x));

        // Draw a vertical primary-coloured indicator at the drop slot.
        if let Some(target_cell) = target_cell {
            if let Some(indicator_x) = drop_indicator_x(&cell_rects, target_cell) {
                let y0 = cell_rects[0].top();
                let y1 = cell_rects[0].bottom() - HAIRLINE_HEIGHT;
                ui.painter()
                    .vline(indicator_x, y0..=y1, Stroke::new(2.0, md3::primary()));
            }
        }

        // Release ends the drag. `drag_started_by` already guards the
        // click-vs-drag threshold (egui's default 4 px), so by the time
        // `drag_from` is set we know this was an actual drag.
        let released = ui.input(|i| i.pointer.any_released());
        if released {
            if let Some(target_cell) = target_cell {
                // Map the cell-space drop target back to a roster insertion
                // point: a drop before `cell_rects[c]` inserts before the
                // roster index `cell_rosters[c]`; a drop at `cell_rects.len()`
                // (past the rightmost plain-tab cell) inserts at the end of
                // the roster (past any trailing mux group too).
                let to = if target_cell < cell_rosters.len() {
                    cell_rosters[target_cell]
                } else {
                    items.len()
                };
                if to != from && to != from + 1 {
                    event = Some(TabEvent::Reorder { from, to });
                }
            }
            ui.ctx().memory_mut(|m| m.data.remove::<usize>(drag_id));
        }
    }

    // FR4: scroll the active visual cell into view exactly once when the
    // keyboard-switch flag is set. `scroll_to_rect` is a no-op when the rect is
    // already visible, so an already-on-screen active cell stays put (the
    // harmless same-window-digit case). Best-effort: if the active cell was not
    // laid out this frame (no rect captured), nothing happens.
    if scroll_active_into_view {
        if let Some(rect) = active_cell_rect {
            #[cfg(test)]
            tests::LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(Some(rect)));
            ui.scroll_to_rect(rect, None);
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
pub(in crate::ui::tab_bar) fn drop_target_index(cells: &[Rect], pointer_x: f32) -> usize {
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
pub(in crate::ui::tab_bar) fn drop_indicator_x(cells: &[Rect], index: usize) -> Option<f32> {
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
