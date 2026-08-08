use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
use std::cell::{Cell, RefCell};

thread_local! {
    pub(super) static LAST_PLUS_RECT: Cell<Option<Rect>> = const { Cell::new(None) };
    pub(super) static LAST_TAB_CELLS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    pub(super) static LAST_MUX_CELLS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    /// Rects that received `paint_active_indicator` during the last
    /// `layout_tab_strip` pass (both plain-tab and mux sub-tab cells).
    /// FR5 asserts the indicator is painted for exactly the expected
    /// cell(s) without GPU readback.
    pub(super) static LAST_INDICATOR_RECTS: RefCell<Vec<Rect>> =
        const { RefCell::new(Vec::new()) };
    /// The rect passed to `ui.scroll_to_rect` during the last
    /// `layout_tab_strip` pass when the FR4 flag was set, or `None` if the
    /// flag was down / the active cell had no captured rect. TS-5 / TS-6
    /// assert the strip requests scroll-into-view for the correct cell.
    pub(super) static LAST_SCROLL_INTO_VIEW_RECT: Cell<Option<Rect>> =
        const { Cell::new(None) };
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

fn run_with_click(items: &[TabBarItem], active_idx: usize, click_pos: Pos2) -> Option<TabEvent> {
    let ctx = egui::Context::default();

    let mut input1 = RawInput::default();
    input1.screen_rect = Some(screen_rect());
    input1.events.push(Event::PointerMoved(click_pos));
    let mut captured: Option<TabEvent> = None;
    let _ = ctx.run(input1, |ctx| {
        captured = draw(ctx, items, active_idx, false, None);
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
        second = draw(ctx, items, active_idx, false, None);
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

// ── task0005 AC-1: collapsed mux tab label ──────────────────────────

#[test]
fn render_label_collapsed_mux_tab_shows_active_window_name() {
    let it = TabBarItem::new("ignored").with_mux_active_window_name("editor");
    assert_eq!(render_label(&it), "mux: editor");
}

#[test]
fn render_label_collapsed_mux_tab_takes_precedence_over_session_prefix() {
    // A tab can carry both `mux_session_name` (set on attach) and
    // `mux_active_window_name` (set once the window list is populated,
    // same frame in practice) — the collapsed label wins.
    let it = TabBarItem::new("ignored")
        .with_mux_session("main")
        .with_mux_active_window_name("editor");
    assert_eq!(render_label(&it), "mux: editor");
}

#[test]
fn render_label_follows_active_window_rename() {
    // AC-1: the label follows a rename of the active window — since
    // `render_label` is pure over the current `TabBarItem`, a fresh
    // view-model built with the new name changes the label with no
    // extra plumbing.
    let before = TabBarItem::new("ignored").with_mux_active_window_name("logs");
    let after = TabBarItem::new("ignored").with_mux_active_window_name("logs-renamed");
    assert_eq!(render_label(&before), "mux: logs");
    assert_eq!(render_label(&after), "mux: logs-renamed");
}

#[test]
fn render_label_pre_group_mux_session_keeps_prefix_format() {
    // Transitional state: `mux_session_name` set on attach, before the
    // window list (and thus `mux_active_window_name`) is populated.
    let it = TabBarItem::new("nvim").with_mux_session("main");
    assert_eq!(render_label(&it), "[mux:main] nvim");
}

// ── task0006 AC-1/AC-4: agent-status badge color / form ─────────────

#[test]
fn agent_state_color_maps_every_variant_to_its_md3_role() {
    assert_eq!(
        agent_state_color(AgentState::Blocked),
        md3::on_error_container()
    );
    assert_eq!(agent_state_color(AgentState::Working), md3::primary());
    assert_eq!(
        agent_state_color(AgentState::Done),
        md3::on_secondary_container()
    );
    assert_eq!(
        agent_state_color(AgentState::Idle),
        md3::on_surface_variant()
    );
}

#[test]
fn agent_badge_filled_blocked_and_done_follow_unseen() {
    assert!(agent_badge_filled(Aggregated {
        state: AgentState::Blocked,
        unseen: true
    }));
    assert!(!agent_badge_filled(Aggregated {
        state: AgentState::Blocked,
        unseen: false
    }));
    assert!(agent_badge_filled(Aggregated {
        state: AgentState::Done,
        unseen: true
    }));
    assert!(!agent_badge_filled(Aggregated {
        state: AgentState::Done,
        unseen: false
    }));
}

#[test]
fn agent_badge_filled_working_and_idle_are_always_filled_regardless_of_unseen() {
    for unseen in [true, false] {
        assert!(agent_badge_filled(Aggregated {
            state: AgentState::Working,
            unseen
        }));
        assert!(agent_badge_filled(Aggregated {
            state: AgentState::Idle,
            unseen
        }));
    }
}

// ── task0006 AC-2: badge absence reserves no layout space ───────────

fn collect_text_shapes_by_x(shapes: &[egui::epaint::ClippedShape]) -> Vec<(f32, String)> {
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<(f32, String)>) {
        use egui::epaint::Shape;
        match shape {
            Shape::Text(t) => {
                let mut s = String::new();
                for row in &t.galley.rows {
                    for g in &row.glyphs {
                        s.push(g.chr);
                    }
                }
                if !s.is_empty() {
                    out.push((t.pos.x, s));
                }
            }
            Shape::Vec(v) => {
                for s in v {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in shapes {
        walk(&cs.shape, &mut out);
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    out
}

fn title_text_x(items: &[TabBarItem]) -> f32 {
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(screen_rect());
    let output = ctx.run(input, |ctx| {
        let _ = draw(ctx, items, 0, false, None);
    });
    collect_text_shapes_by_x(&output.shapes)
        .into_iter()
        .find(|(_, s)| s.contains("shell"))
        .map(|(x, _)| x)
        .expect("title text shape present")
}

#[test]
fn agent_badge_present_shifts_title_right_when_reserving_its_space() {
    let without = title_text_x(&[item("shell")]);
    let with = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
        state: AgentState::Working,
        unseen: true,
    }))]);
    // The `[badge][dot][gap][title]` group is centred as a unit, so
    // reserving `agent_dot_space` extra width shifts the group's
    // (and thus the title's) center by half that amount — the other
    // half is absorbed by the group's left edge moving left. The
    // reserved width is the unified badge SLOT (task0001 Design 4),
    // not the bare circle diameter.
    let expected_shift = (AGENT_BADGE_SLOT_WIDTH + AGENT_BADGE_GAP) / 2.0;
    assert!(
        (with - without - expected_shift).abs() < 0.5,
        "badge presence should shift the title right by half its reserved space \
         ({expected_shift}px): \
         without={without}, with={with}"
    );
}

#[test]
fn agent_badge_absent_matches_pre_feature_title_position_across_two_renders() {
    // AC-2: two independently-built items with no agent badge (one
    // freshly constructed, one via the builder passing `None`
    // explicitly) must paint the title at the identical x — proving
    // `agent_dot_space` contributes nothing when the badge is absent.
    let plain = title_text_x(&[item("shell")]);
    let explicit_none = title_text_x(&[item("shell").with_agent_badge(None)]);
    assert_eq!(plain, explicit_none);
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
        let _ = draw(ctx, items, active_idx, false, None);
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
        captured = draw(ctx, &items, 0, false, None);
    });
    assert_eq!(captured, None);
}

// ── TS-15: mux tab-group render model (always sub-tabs) ───────────────

use crate::mux::window_group::{MuxWindow, MuxWindowGroup};

fn group_with(n: usize, active: usize) -> MuxWindowGroup {
    let mut g = MuxWindowGroup::new();
    let windows: Vec<MuxWindow> = (0..n)
        .map(|i| MuxWindow {
            id: i as u32,
            name: format!("w{i}"),
        })
        .collect();
    let panes: Vec<u32> = (0..n).map(|i| 100 + i as u32).collect();
    g.seed(windows, panes, active);
    g
}

#[test]
fn render_model_is_one_subtab_per_window_with_active_marker() {
    let cells = mux_group_render_model(&group_with(3, 1));
    assert_eq!(cells.len(), 3);
    let actives: Vec<bool> = cells.iter().map(|c| c.active).collect();
    assert_eq!(actives, vec![false, true, false]);
    // Index + name carried through for the click target / label.
    assert_eq!(cells[0].index, 0);
    assert_eq!(cells[2].name, "w2");
}

#[test]
fn render_model_single_window_still_renders_one_subtab() {
    // WebView parity: a single mux window still renders as a sub-tab.
    let cells = mux_group_render_model(&group_with(1, 0));
    assert_eq!(cells.len(), 1);
    assert!(cells[0].active);
}

#[test]
fn sub_tab_label_is_numbered() {
    let cells = mux_group_render_model(&group_with(2, 0));
    assert_eq!(mux_sub_tab_label(&cells[0]), "[1] w0");
    assert_eq!(mux_sub_tab_label(&cells[1]), "[2] w1");
}

// ── FR1: mux group is rendered by `draw` and clicks route correctly ───

/// A tab whose roster entry carries mux cells (built from a group).
fn mux_item(group: &MuxWindowGroup) -> TabBarItem {
    TabBarItem::new("ignored").with_mux_cells(mux_group_render_model(group))
}

/// Lay out one frame and return the captured mux cell rects in order.
fn mux_cell_rects(items: &[TabBarItem]) -> Vec<Rect> {
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(screen_rect());
    let _ = ctx.run(input, |ctx| {
        let _ = draw(ctx, items, 0, false, None);
    });
    LAST_MUX_CELLS.with(|c| c.borrow().clone())
}

#[test]
fn draw_renders_one_cell_per_window() {
    // 3-window group → 3 sub-tab cells (no compact/header cell).
    let items = vec![mux_item(&group_with(3, 0))];
    let rects = mux_cell_rects(&items);
    assert_eq!(rects.len(), 3);
}

#[test]
fn clicking_subtab_emits_mux_switch_to_that_window() {
    // Cells: [sub0, sub1, sub2]; clicking sub2 → switch window 2.
    let items = vec![mux_item(&group_with(3, 0))];
    let sub2 = mux_cell_rects(&items)[2].center();
    let ev = run_with_click(&items, 0, sub2);
    assert_eq!(ev, Some(TabEvent::MuxSwitch { tab: 0, window: 2 }));
}

#[test]
fn clicking_first_subtab_switches_to_window_zero() {
    let items = vec![mux_item(&group_with(3, 1))];
    let sub0 = mux_cell_rects(&items)[0].center();
    let ev = run_with_click(&items, 0, sub0);
    assert_eq!(ev, Some(TabEvent::MuxSwitch { tab: 0, window: 0 }));
}

#[test]
fn mux_group_cell_count_drives_strip_width_math() {
    // A 3-window group counts as 3 visual cells, not 1 roster entry, so
    // the equal-width layout reserves room for every sub-tab.
    let items = vec![mux_item(&group_with(3, 0))];
    assert_eq!(visual_cell_count(&items), 3);
    // Plain tabs still count one-each (Phase 4-B path unchanged).
    let plain = vec![item("a"), item("b")];
    assert_eq!(visual_cell_count(&plain), 2);
}

// ── FR5: unique active indicator across mixed tabs ───────────────────

/// Lay out one frame and return the rects that received the active
/// indicator (both plain-tab and mux sub-tab cells).
fn indicator_rects(items: &[TabBarItem], active_idx: usize) -> Vec<Rect> {
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(screen_rect());
    let _ = ctx.run(input, |ctx| {
        let _ = draw(ctx, items, active_idx, false, None);
    });
    LAST_INDICATOR_RECTS.with(|c| c.borrow().clone())
}

#[test]
fn ts2_non_active_mux_parent_paints_no_subtab_indicator() {
    // Roster: [plain "a" (active), mux group]. The mux group has an
    // active window (cell 1), but its parent tab (index 1) is NOT the
    // active tab, so no sub-tab indicator is painted. The only indicator
    // is the active plain tab's.
    let items = vec![item("a"), mux_item(&group_with(3, 1))];
    let mux_cells = mux_cell_rects(&items);
    let bars = indicator_rects(&items, 0);
    // No painted indicator coincides with any mux sub-tab cell.
    for cell in &mux_cells {
        assert!(
            !bars.iter().any(|b| b.left() == cell.left()),
            "no sub-tab indicator should be painted for a non-active mux parent; \
             got bars {bars:?} overlapping mux cell {cell:?}"
        );
    }
}

#[test]
fn ts3_active_mux_parent_paints_active_window_subtab_indicator() {
    // Roster: [plain "a", mux group (active)]. Parent tab index 1 is
    // active and window 1 is the active window, so exactly the window-1
    // sub-tab gets the indicator.
    let items = vec![item("a"), mux_item(&group_with(3, 1))];
    let mux_cells = mux_cell_rects(&items);
    let bars = indicator_rects(&items, 1);
    // Exactly one bar, aligned to the active window's sub-tab (cell 1).
    assert_eq!(bars.len(), 1, "exactly one indicator across the strip");
    assert!(
        (bars[0].left() - mux_cells[1].left()).abs() < 0.5,
        "the single indicator should sit on the active window's sub-tab; \
         bar {:?} vs cell {:?}",
        bars[0],
        mux_cells[1]
    );
}

// ── FR4: active-cell scroll-into-view selection ──────────────────────

/// Lay out one frame with the FR4 flag set and return the rect the strip
/// requested scroll-into-view for (if any).
fn scroll_into_view_rect(items: &[TabBarItem], active_idx: usize) -> Option<Rect> {
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(screen_rect());
    LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(None));
    let _ = ctx.run(input, |ctx| {
        let _ = draw(ctx, items, active_idx, true, None);
    });
    LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.get())
}

#[test]
fn ts5_flag_set_requests_scroll_into_view_for_active_cell() {
    // TS-5: with the flag set and an active plain tab, the strip selects
    // that cell's rect and requests scroll-into-view exactly once.
    // Many tabs so the strip overflows (the off-screen case is the point);
    // the active cell is captured regardless of visibility.
    let items: Vec<TabBarItem> = (0..12).map(|i| item(&format!("t{i}"))).collect();
    let want = tab_cell_rect(&items, 7, 7);
    let got = scroll_into_view_rect(&items, 7).expect("flag set → a rect is requested");
    assert!(
        (got.left() - want.left()).abs() < 0.5 && (got.right() - want.right()).abs() < 0.5,
        "scroll-into-view should target the active cell; got {got:?} want {want:?}"
    );
}

#[test]
fn ts5_flag_unset_requests_no_scroll_into_view() {
    // Companion to TS-5: with the flag down, no scroll-into-view request is
    // made (the mouse-scroll / unrelated-repaint case).
    let items: Vec<TabBarItem> = (0..12).map(|i| item(&format!("t{i}"))).collect();
    let ctx = egui::Context::default();
    let mut input = RawInput::default();
    input.screen_rect = Some(screen_rect());
    LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.set(None));
    let _ = ctx.run(input, |ctx| {
        let _ = draw(ctx, &items, 7, false, None);
    });
    assert_eq!(
        LAST_SCROLL_INTO_VIEW_RECT.with(|c| c.get()),
        None,
        "flag down → no scroll-into-view request"
    );
}

#[test]
fn ts6_active_cell_selection_picks_active_mux_subtab_in_active_tab() {
    // TS-6: the active visual cell inside an active mux tab is the active
    // window's sub-tab, not the tab as a whole. Roster: [plain "a", mux
    // group (window 1 active)]. With the mux tab (index 1) active, the
    // requested rect is the window-1 sub-tab cell.
    let items = vec![item("a"), mux_item(&group_with(3, 1))];
    let mux_cells = mux_cell_rects(&items);
    let got = scroll_into_view_rect(&items, 1).expect("flag set → a rect is requested");
    assert!(
        (got.left() - mux_cells[1].left()).abs() < 0.5,
        "scroll-into-view should target the active window's sub-tab; \
         got {:?} want {:?}",
        got,
        mux_cells[1]
    );
}

#[test]
fn ts6_active_cell_selection_picks_plain_cell_at_active_idx() {
    // TS-6 (plain side): with a plain tab active, the active cell is the
    // plain-tab cell at `active_idx`.
    let items = vec![item("a"), item("b"), item("c")];
    let want = tab_cell_rect(&items, 2, 2);
    let got = scroll_into_view_rect(&items, 2).expect("flag set → a rect is requested");
    assert!(
        (got.left() - want.left()).abs() < 0.5,
        "scroll-into-view should target the active plain cell; got {got:?} want {want:?}"
    );
}

#[test]
fn ts4_draw_with_non_active_mux_parent_leaves_active_window_unchanged() {
    // The gate reads `mux_cell.active` (a copied bool) and `tab` /
    // `active_idx` (plain indices); it never touches `MuxWindowGroup`.
    // Drawing with a non-active parent must not change the render model's
    // active window.
    let group = group_with(3, 1);
    let before = mux_group_render_model(&group);
    let items = vec![item("a"), mux_item(&group)];
    let _ = indicator_rects(&items, 0); // parent (index 1) not active
    let after = mux_group_render_model(&group);
    assert_eq!(
        before, after,
        "drawing with a non-active mux parent must not mutate the active window"
    );
}

// ── agent-badge-emoji task0001 AC-1: 4 states × unseen/seen table ───

#[test]
fn badge_presentation_resolves_all_eight_state_unseen_combinations() {
    // Table mirrors SPEC 4.2 / IMPLEMENTATION.md's presentation table:
    // working/idle unseen-independent, blocked ❓/❔, done ✅/💤 (the
    // 💤 alias of IDLE_BADGE_EMOJI). `fallback_filled` mirrors
    // `agent_badge_filled`'s per-state semantics (D3).
    let cases: [(AgentState, bool, &str, bool); 8] = [
        (AgentState::Working, true, WORKING_BADGE_EMOJI, true),
        (AgentState::Working, false, WORKING_BADGE_EMOJI, true),
        (AgentState::Idle, true, IDLE_BADGE_EMOJI, true),
        (AgentState::Idle, false, IDLE_BADGE_EMOJI, true),
        (AgentState::Blocked, true, BLOCKED_BADGE_EMOJI_UNSEEN, true),
        (AgentState::Blocked, false, BLOCKED_BADGE_EMOJI_SEEN, false),
        (AgentState::Done, true, DONE_BADGE_EMOJI_UNSEEN, true),
        (AgentState::Done, false, IDLE_BADGE_EMOJI, false),
    ];
    for (state, unseen, expected_cluster, expected_fallback_filled) in cases {
        assert_eq!(
            badge_presentation(Aggregated { state, unseen }),
            BadgePresentation::Emoji {
                cluster: expected_cluster,
                fallback_filled: expected_fallback_filled,
            },
            "{state:?} unseen={unseen}"
        );
    }
}

// ── agent-badge-emoji task0001 AC-2: done+seen aliases idle ─────────

#[test]
fn badge_presentation_done_seen_is_the_same_cluster_constant_as_idle() {
    let BadgePresentation::Emoji { cluster, .. } = badge_presentation(Aggregated {
        state: AgentState::Done,
        unseen: false,
    });
    assert_eq!(cluster, IDLE_BADGE_EMOJI);
    assert!(
        std::ptr::eq(cluster, IDLE_BADGE_EMOJI),
        "done+seen must reuse the IDLE_BADGE_EMOJI constant itself, not a \
         duplicate literal with the same content"
    );
}

// ── agent-badge-emoji task0001 AC-3: new cluster constant format ────

#[test]
fn new_badge_emoji_constants_are_single_codepoint_no_vs16() {
    let cases: [(&str, char); 3] = [
        (BLOCKED_BADGE_EMOJI_UNSEEN, '\u{2753}'),
        (BLOCKED_BADGE_EMOJI_SEEN, '\u{2754}'),
        (DONE_BADGE_EMOJI_UNSEEN, '\u{2705}'),
    ];
    for (cluster, expected_codepoint) in cases {
        let chars: Vec<char> = cluster.chars().collect();
        assert_eq!(
            chars,
            vec![expected_codepoint],
            "cluster must be exactly the expected single codepoint, with \
             no VS-16 (U+FE0F) suffix"
        );
    }
}

// ── agent-badge-emoji task0001 AC-4/AC-5: fallback circle resolution ─

#[test]
fn resolve_badge_render_mode_emoji_with_texture_blits_the_texture() {
    for state in [
        AgentState::Working,
        AgentState::Idle,
        AgentState::Blocked,
        AgentState::Done,
    ] {
        for unseen in [true, false] {
            let presentation = badge_presentation(Aggregated { state, unseen });
            assert_eq!(
                resolve_badge_render_mode(presentation, true),
                BadgeRenderMode::EmojiTexture,
                "{state:?} unseen={unseen}"
            );
        }
    }
}

#[test]
fn resolve_badge_render_mode_working_idle_fallback_is_always_filled() {
    // AC-5: texture unavailable, working/idle fallback stays filled
    // regardless of unseen (unchanged from before this task).
    for state in [AgentState::Working, AgentState::Idle] {
        for unseen in [true, false] {
            let presentation = badge_presentation(Aggregated { state, unseen });
            assert_eq!(
                resolve_badge_render_mode(presentation, false),
                BadgeRenderMode::Circle { filled: true },
                "{state:?} unseen={unseen}"
            );
        }
    }
}

#[test]
fn resolve_badge_render_mode_blocked_done_fallback_follows_unseen() {
    // AC-4: texture unavailable, blocked/done fall back to a circle
    // that preserves the existing unseen=filled / seen=ring shape —
    // never a blank slot, never the toolkit's default text path.
    for state in [AgentState::Blocked, AgentState::Done] {
        for unseen in [true, false] {
            let presentation = badge_presentation(Aggregated { state, unseen });
            assert_eq!(
                resolve_badge_render_mode(presentation, false),
                BadgeRenderMode::Circle { filled: unseen },
                "{state:?} unseen={unseen}"
            );
        }
    }
}

// ── task0001 AC-3: emoji texture blit via a stub rasterizer ─────────
//
// Standing up the REAL swash + bundled-font stack in a unit test is
// impractical (per the task's Test Notes) — this stub proves the
// paint path picks the texture branch and aspect-fits it into the
// slot; actual rasterization quality is a manual check (TS3).

struct StubEmojiRasterizer;

impl crate::render::font::traits::GlyphRasterizer for StubEmojiRasterizer {
    fn shape(
        &self,
        _cluster: &str,
        font: crate::render::font::traits::FontId,
        size_px: f32,
    ) -> Vec<crate::render::font::traits::ShapedGlyph> {
        vec![crate::render::font::traits::ShapedGlyph {
            font,
            glyph_id: 1,
            size_px,
        }]
    }

    fn raster(
        &self,
        _font: crate::render::font::traits::FontId,
        _glyph_id: u32,
        size_px: f32,
    ) -> Option<crate::render::font::traits::GlyphBitmap> {
        let n = size_px.round().max(1.0) as u32;
        Some(crate::render::font::traits::GlyphBitmap {
            format: crate::render::font::traits::AtlasFormat::Rgba,
            width: n,
            height: n,
            bearing: (0, 0),
            advance: size_px,
            pixels: vec![255u8; (n as usize) * (n as usize) * 4],
        })
    }

    fn has_codepoint(&self, _font: crate::render::font::traits::FontId, _cp: u32) -> bool {
        true
    }
}

/// A fallback chain that resolves every cluster through the "emoji"
/// font id — pairs with [`StubEmojiRasterizer`], which covers every
/// codepoint.
fn stub_emoji_fallback() -> crate::render::font::fallback::FallbackChain {
    use crate::render::font::traits::FontId;
    let mut chain = crate::render::font::fallback::FallbackChain::new(FontId(1), [FontId(2)]);
    chain.set_emoji(FontId(2));
    chain
}

/// Collect the rects of every textured (image-blit) `Shape::Rect` —
/// egui's `Image::paint_at` emits a `RectShape` with a non-default
/// `fill_texture_id`, distinguishing an emoji blit from any other
/// filled rect the strip paints.
fn collect_textured_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<Rect> {
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<Rect>) {
        use egui::epaint::Shape;
        match shape {
            Shape::Rect(r) if r.fill_texture_id != egui::TextureId::default() => {
                out.push(r.rect);
            }
            Shape::Vec(v) => {
                for s in v {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

/// Collect the radii of every `Shape::Circle` painted this frame.
fn collect_circle_radii(shapes: &[egui::epaint::ClippedShape]) -> Vec<f32> {
    fn walk(shape: &egui::epaint::Shape, out: &mut Vec<f32>) {
        use egui::epaint::Shape;
        match shape {
            Shape::Circle(c) => out.push(c.radius),
            Shape::Vec(v) => {
                for s in v {
                    walk(s, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

#[test]
fn ac3_working_and_idle_badges_with_emoji_resources_paint_texture_not_circle() {
    for state in [AgentState::Working, AgentState::Idle] {
        let items = vec![item("shell").with_agent_badge(Some(Aggregated {
            state,
            unseen: true,
        }))];
        let rasterizer = StubEmojiRasterizer;
        let fallback = stub_emoji_fallback();
        let cache = parking_lot::Mutex::new(crate::ui::emoji_cache::EmojiTextureCache::new());
        let emoji = EmojiResources {
            rasterizer: &rasterizer,
            fallback: &fallback,
            cache: &cache,
        };
        let ctx = egui::Context::default();
        let mut input = RawInput::default();
        input.screen_rect = Some(screen_rect());
        let output = ctx.run(input, |ctx| {
            let _ = draw(ctx, &items, 0, false, Some(&emoji));
        });

        let textured = collect_textured_rects(&output.shapes);
        assert!(
            !textured.is_empty(),
            "{state:?}: expected a textured rect (emoji blit) for the badge"
        );
        for r in &textured {
            assert!(
                r.width() <= AGENT_BADGE_SLOT_WIDTH + 0.01
                    && r.height() <= AGENT_BADGE_SLOT_WIDTH + 0.01,
                "{state:?}: emoji blit must aspect-fit inside the \
                 {AGENT_BADGE_SLOT_WIDTH}px slot; got {r:?}"
            );
        }

        // Replace, not combine (Design 4): no badge-sized (4px radius)
        // circle should also be painted for this state.
        let radii = collect_circle_radii(&output.shapes);
        let badge_radius = AGENT_BADGE_DIAMETER / 2.0;
        assert!(
            !radii.iter().any(|r| (*r - badge_radius).abs() < 0.01),
            "{state:?}: the emoji must replace the dot entirely, not paint \
             alongside it; circle radii found: {radii:?}"
        );
    }
}

// ── task0001 AC-5: unified 12px badge slot ───────────────────────────

#[test]
fn ac5_working_to_done_transition_causes_no_title_shift() {
    // The reserved slot width is unified across ALL states (Design
    // 4), so a badge state transition must never move the title even
    // though `working` and `done` render different emoji clusters
    // (agent-badge-emoji task0001) and (with no emoji resources
    // supplied here) different fallback-circle shapes.
    let working = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
        state: AgentState::Working,
        unseen: true,
    }))]);
    let done = title_text_x(&[item("shell").with_agent_badge(Some(Aggregated {
        state: AgentState::Done,
        unseen: true,
    }))]);
    assert_eq!(
        working, done,
        "a working -> done badge transition must cause no title x-shift"
    );
}
