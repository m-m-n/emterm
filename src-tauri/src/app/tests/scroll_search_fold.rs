use super::*;

// ── TS-1 / TS-2 / TS-3 / TS-7: per-tab scroll save/restore (FR3) ─────

/// Two synthetic tabs to exercise the native tab-switch scroll handoff.
fn app_with_two_tabs() -> App {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    // `spawn_new_tab` makes the new tab active AND raises the one-shot
    // scroll-into-view flag; reset both so the test starts from a known
    // clean state (active tab 0, flag down). Use the direct field rather
    // than `switch_to_tab` so we do not exercise the path under test.
    app.active = 0;
    app.scroll_active_tab_into_view = false;
    app.scroll_position = ScrollPosition::Live;
    app
}

#[test]
fn switch_to_tab_saves_outgoing_and_restores_incoming_scroll() {
    // TS-1: switching tabs saves the outgoing tab's scroll position and
    // restores the incoming tab's.
    let mut app = app_with_two_tabs();
    // Scroll up in tab 0, then switch to tab 1 (saved at Live).
    app.scroll_position = ScrollPosition::OffsetFromLive(12);
    app.switch_to_tab(1);
    assert_eq!(
        app.scroll_offset(),
        0,
        "incoming tab 1 was at Live → restores to bottom"
    );
    assert_eq!(
        app.tabs[0].scroll_position,
        ScrollPosition::OffsetFromLive(12),
        "outgoing tab 0's offset was parked into its slot"
    );
    // Returning to tab 0 restores its parked offset.
    app.switch_to_tab(0);
    assert_eq!(
        app.scroll_offset(),
        12,
        "returning to tab 0 restores its saved offset"
    );
}

#[test]
fn switch_to_tab_live_restores_to_bottom() {
    // TS-2: a unit saved at Live restores at the bottom (offset 0).
    let mut app = app_with_two_tabs();
    // Tab 1 stays at Live; tab 0 scrolls up before we leave it.
    app.scroll_position = ScrollPosition::OffsetFromLive(5);
    app.switch_to_tab(1);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    assert_eq!(app.scroll_offset(), 0);
}

#[test]
fn switch_to_tab_offset_restores_to_same_offset() {
    // TS-3: a unit saved at OffsetFromLive(n) restores at offset n.
    let mut app = app_with_two_tabs();
    // Pre-seed tab 1 with a saved offset, then switch into it.
    app.tabs[1].scroll_position = ScrollPosition::OffsetFromLive(8);
    app.switch_to_tab(1);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(8));
    assert_eq!(app.scroll_offset(), 8);
}

#[test]
fn switch_to_tab_all_live_introduces_no_scroll() {
    // TS-7: all tabs at Live → switching introduces no scroll.
    let mut app = app_with_two_tabs();
    assert_eq!(app.tabs[0].scroll_position, ScrollPosition::Live);
    assert_eq!(app.tabs[1].scroll_position, ScrollPosition::Live);
    app.switch_to_tab(1);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    app.switch_to_tab(0);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn tab_scroll_position_default_is_live() {
    let app = app_with_two_tabs();
    assert_eq!(app.tabs[0].scroll_position, ScrollPosition::Live);
    assert_eq!(app.tabs[1].scroll_position, ScrollPosition::Live);
}

// ── FR4: plain-tab keyboard switch raises scroll-into-view flag ─────────

#[test]
fn ts7_keyboard_tab_switch_sets_scroll_into_view_flag() {
    // TS-7: NextTab / PrevTab / JumpTab each commit an active-index change
    // (all route through `switch_to_tab`), which raises the one-shot
    // scroll-into-view flag.
    let mut app = app_with_two_tabs();
    assert!(!app.scroll_active_tab_into_view());
    app.apply_action(crate::ui::AppAction::NextTab);
    assert!(
        app.scroll_active_tab_into_view(),
        "NextTab moved active → flag set"
    );

    // Clear and try PrevTab.
    app.clear_scroll_active_tab_into_view();
    app.apply_action(crate::ui::AppAction::PrevTab);
    assert!(
        app.scroll_active_tab_into_view(),
        "PrevTab moved active → flag set"
    );

    // Clear and try JumpTab to the other tab.
    app.clear_scroll_active_tab_into_view();
    // active is back at 0 (NextTab→1, PrevTab→0); jump to tab 2 (Ctrl+2)
    // which clamps to the last existing tab (idx 1), a real move.
    app.apply_action(crate::ui::AppAction::JumpTab(2));
    assert!(
        app.scroll_active_tab_into_view(),
        "JumpTab moved active → flag set"
    );
}

#[test]
fn ts8_switch_to_already_active_tab_does_not_set_flag() {
    // TS-8: switching to the already-active tab (or out of range) is a
    // no-op `switch_to_tab` early-return, so the flag stays down.
    let mut app = app_with_two_tabs();
    assert_eq!(app.active, 0);
    app.clear_scroll_active_tab_into_view();
    app.switch_to_tab(0); // same index → no-op
    assert!(
        !app.scroll_active_tab_into_view(),
        "no-op switch to the active tab must not set the flag"
    );
    app.switch_to_tab(99); // out of range → no-op
    assert!(
        !app.scroll_active_tab_into_view(),
        "out-of-range switch must not set the flag"
    );
}

#[test]
fn ts7b_mouse_tab_switch_does_not_set_scroll_into_view_flag() {
    // FR4 (keyboard-only): a mouse click that switches tabs routes through
    // `apply_tab_event(TabEvent::Switch)` → `switch_to_tab`, which (post-fix)
    // does NOT raise the scroll-into-view flag. The clicked tab is already
    // visible, so there is nothing to scroll into view; raising the flag
    // on the mouse path is exactly the FR4 violation this guards against.
    let mut app = app_with_two_tabs();
    assert_eq!(app.active, 0);
    app.clear_scroll_active_tab_into_view();
    let _ = app.apply_tab_event(crate::ui::TabEvent::Switch(1));
    assert_eq!(app.active, 1, "mouse switch moved the active tab");
    assert!(
        !app.scroll_active_tab_into_view(),
        "mouse-originated tab switch must NOT set the scroll-into-view flag"
    );
}

#[test]
fn new_tab_sets_scroll_into_view_flag() {
    // A freshly created tab lands at the end of the strip (off-screen when
    // tabs overflow), so it raises the one-shot scroll-into-view flag and
    // surfaces next frame. This holds for every new-tab path (they all
    // funnel through `spawn_new_tab_with_overrides`), and unlike an
    // existing-tab mouse switch, it fires even though `+` is a mouse action
    // — the new tab is one the user has not seen yet.
    let mut app = app_with_two_tabs();
    app.clear_scroll_active_tab_into_view();
    let before = app.tabs.len();
    app.spawn_new_tab();
    assert_eq!(app.tabs.len(), before + 1, "spawned a new tab");
    assert_eq!(app.active, app.tabs.len() - 1, "the new tab is active");
    assert!(
        app.scroll_active_tab_into_view(),
        "a newly created tab must raise the scroll-into-view flag"
    );
}

#[test]
fn auto_research_throttle_allows_then_blocks_then_allows() {
    // Pure-function policy: no prior run always allows; within the
    // window blocks; past the window allows again.
    let t0 = Instant::now();
    assert!(
        auto_research_allowed(None, t0),
        "first auto re-search always runs"
    );
    let just_under = t0 + (AUTO_RESEARCH_THROTTLE - std::time::Duration::from_millis(1));
    assert!(
        !auto_research_allowed(Some(t0), just_under),
        "a run inside the throttle window is blocked"
    );
    let just_over = t0 + AUTO_RESEARCH_THROTTLE;
    assert!(
        auto_research_allowed(Some(t0), just_over),
        "a run at/after the window elapses is allowed"
    );
}

#[test]
fn auto_research_throttled_keeps_dirty_and_does_not_run() {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"needle\r\n");
    }
    app.open_search();
    app.search.query = "needle".to_string();
    app.run_search();
    assert_eq!(app.search.matches.len(), 1);

    // A fresh buffer change arrives and is flagged dirty.
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"second needle\r\n");
    }
    app.on_pty_output(true, 0);
    assert!(app.search.needs_research());

    // Pretend an auto re-search just ran, so the gate is closed.
    app.last_auto_research = Some(Instant::now());
    let ran = app.auto_research_if_dirty();
    assert!(
        !ran,
        "throttled: auto re-search must not run within the window"
    );
    assert!(
        app.search.needs_research(),
        "dirty flag is preserved so the next frame past the gap re-resolves"
    );
    assert_eq!(
        app.search.matches.len(),
        1,
        "matches unchanged while throttled (no execute)"
    );

    // Past the throttle window the same pending dirty re-resolves.
    app.last_auto_research =
        Some(Instant::now() - AUTO_RESEARCH_THROTTLE - std::time::Duration::from_millis(1));
    let ran = app.auto_research_if_dirty();
    assert!(ran, "past the window the pending change re-resolves");
    assert_eq!(app.search.matches.len(), 2);
}

#[test]
fn auto_research_preserves_current_index() {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"hit\r\nhit\r\nhit\r\n");
    }
    app.open_search();
    app.search.query = "hit".to_string();
    app.run_search();
    assert_eq!(app.search.matches.len(), 3);
    // Navigate to the second hit; the auto re-search must keep it.
    app.search_next();
    assert_eq!(app.search.current_index, 1);

    {
        let mut core = app.tabs[0].core.lock();
        core.process_pty_data(b"hit\r\n");
    }
    app.on_pty_output(true, 0);
    // No prior auto re-search → throttle allows immediately.
    let ran = app.auto_research_if_dirty();
    assert!(ran);
    assert_eq!(app.search.matches.len(), 4, "new occurrence picked up");
    assert_eq!(
        app.search.current_index, 1,
        "auto re-search preserved the navigation cursor"
    );
}

#[test]
fn background_tab_output_does_not_dirty_active_search() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab(); // active is now tab 1
    {
        let mut core = app.tabs[1].core.lock();
        core.process_pty_data(b"needle\r\n");
    }
    app.open_search();
    app.search.query = "needle".to_string();
    app.run_search();
    assert!(
        !app.search.needs_research(),
        "clean immediately after search"
    );

    // A *background* tab (tab 0) produced output. Per H3 this must NOT
    // invalidate the active tab's cached search document.
    app.on_pty_output(false, 0);
    assert!(
        !app.search.needs_research(),
        "background-tab output leaves the active search clean"
    );

    // Active-tab output does invalidate it.
    app.on_pty_output(true, 0);
    assert!(
        app.search.needs_research(),
        "active-tab output marks the search document dirty"
    );
}

#[test]
fn reap_exited_tab_closes_open_search() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab(); // two tabs; active is tab 1
    app.open_search();
    app.search.query = "x".to_string();
    assert!(app.search_visible());

    // Mark a tab exited so the reap path in `pump_all` removes it,
    // shifting the active buffer. The open overlay must close (H4).
    app.tabs[1].exited = true;
    app.pump_all();
    assert_eq!(app.tabs.len(), 1, "exited tab was reaped");
    assert!(
        !app.search_visible(),
        "reap shifted the active buffer → search closed"
    );
}

// ── Search fold auto-expand (SPEC: Search Integration) ──────────────

/// Build a tab with one occurrence of "needle" in scrollback and return
/// the absolute row the match lands on. The core is small (80×4) so a
/// handful of `\r\n` lines push the needle into scrollback quickly.
fn app_with_needle_in_scrollback() -> (App, u32) {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let mut core = app.tabs[0].core.lock();
        // Write the needle line then overflow the 4-row viewport so it
        // spills into scrollback.
        core.process_pty_data(b"needle\r\n");
        for _ in 0..8 {
            core.process_pty_data(b"\r\n");
        }
    }
    // Run a search to discover the actual abs_row of the match.
    app.open_search();
    app.search.query = "needle".to_string();
    if let Some(tab) = app.tabs.get(app.active) {
        let core = tab.core.lock();
        app.search.execute(&core);
    }
    let abs_row = app.search.matches[0].segments[0].abs_row;
    // Reset navigation cursor; tests call run_search / search_next themselves.
    app.search.current_index = -1;
    (app, abs_row)
}

#[test]
fn search_next_auto_expands_collapsed_region_containing_match() {
    // A collapsed fold region wrapping the needle's absolute row must be
    // expanded when `search_next` navigates to that match — mirroring the
    // WebView's `foldManager.expandRegionContaining(match.lineIndex)`
    // call in search.ts:154.
    let (mut app, abs_row) = app_with_needle_in_scrollback();
    // Wrap the needle in a collapsed fold region.
    let region_start = abs_row;
    let region_end = abs_row + 5;
    app.tabs[0]
        .folds
        .register_osc133_region(region_start, region_end, "cmd".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(region_start);
    assert!(
        app.tabs[0]
            .folds
            .get_region_at_line(abs_row)
            .unwrap()
            .collapsed,
        "region must be collapsed before navigation"
    );

    // Navigate to the first match. `search_next` wraps from -1 to 0.
    app.search_next();

    assert!(
        !app.tabs[0]
            .folds
            .get_region_at_line(abs_row)
            .unwrap()
            .collapsed,
        "search_next must expand the collapsed region containing the match"
    );
}

#[test]
fn run_search_auto_expands_collapsed_region_on_initial_confirm() {
    // On the initial search confirm (`run_search`) the current match (first
    // hit) is also scrolled into view, so the same auto-expand must fire.
    let (mut app, abs_row) = app_with_needle_in_scrollback();
    let region_start = abs_row;
    let region_end = abs_row + 5;
    app.tabs[0]
        .folds
        .register_osc133_region(region_start, region_end, "cmd".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(region_start);

    app.run_search();

    assert!(
        !app.tabs[0]
            .folds
            .get_region_at_line(abs_row)
            .unwrap()
            .collapsed,
        "run_search must expand the collapsed region containing the first match"
    );
}

#[test]
fn search_does_not_expand_unrelated_collapsed_regions() {
    // A collapsed region that does NOT contain the current match must stay
    // collapsed — expand_region_containing is scoped to the match row.
    let (mut app, abs_row) = app_with_needle_in_scrollback();
    // Place the fold region well away from the needle.
    let unrelated_start = abs_row.saturating_sub(1).max(1) - 1; // one before
    // Guard: skip when there's no room for a region before abs_row.
    if unrelated_start == 0 {
        return;
    }
    let unrelated_end = unrelated_start + 1;
    if unrelated_end > abs_row {
        // No room — skip rather than overlap.
        return;
    }
    app.tabs[0].folds.register_osc133_region(
        unrelated_start,
        unrelated_end,
        "other".to_string(),
        Some(0),
    );
    app.tabs[0].folds.toggle_fold(unrelated_start);
    assert!(
        app.tabs[0]
            .folds
            .get_region_at_line(unrelated_start)
            .unwrap()
            .collapsed
    );

    app.search_next();

    assert!(
        app.tabs[0]
            .folds
            .get_region_at_line(unrelated_start)
            .unwrap()
            .collapsed,
        "unrelated collapsed region must stay collapsed"
    );
}

#[test]
fn visual_bell_reports_progress_while_live() {
    let mut app = App::new();
    app.visual_bell_started = Some(Instant::now());
    let t = app
        .visual_bell_progress()
        .expect("flash just started — progress must be live");
    assert!((0.0..1.0).contains(&t), "progress {t} out of range");
    assert!(app.needs_bell_repaint(), "live flash must request frames");
    // The latch survives polls while the flash is still in-flight.
    assert!(app.visual_bell_started.is_some());
}

#[test]
fn visual_bell_clears_after_flash_duration() {
    let mut app = App::new();
    // Back-date the flash past its 150 ms lifetime.
    app.visual_bell_started = Instant::now().checked_sub(Duration::from_millis(BELL_FLASH_MS + 50));
    assert!(
        app.visual_bell_started.is_some(),
        "test clock too close to process start to back-date"
    );
    assert_eq!(app.visual_bell_progress(), None);
    // One final repaint to erase the overlay…
    assert!(app.needs_bell_repaint());
    // …then the latch is gone.
    assert!(!app.needs_bell_repaint());
    assert!(app.visual_bell_started.is_none());
}

/// task0005 AC-4: the erase-frame signal is set exactly on the expiry
/// turn (not while the flash is still live) and reads `false` again
/// once consumed.
#[test]
fn bell_erase_pending_false_while_flash_still_live() {
    let mut app = App::new();
    app.visual_bell_started = Some(Instant::now());
    assert!(app.needs_bell_repaint(), "live flash still requests frames");
    assert!(
        !app.take_bell_erase_pending(),
        "erase-frame signal must not fire while the flash is still decaying"
    );
}

/// task0005 AC-4: on the turn the flash crosses its expiry,
/// `needs_bell_repaint` both clears `visual_bell_started` (existing
/// behavior) and latches the erase-frame signal so the render skip
/// decision does not skip this frame — the frame after that, with the
/// flash long gone and nothing else pending, must read the signal as
/// already consumed (`false`) again.
#[test]
fn bell_erase_pending_true_exactly_once_after_expiry() {
    let mut app = App::new();
    app.visual_bell_started = Instant::now().checked_sub(Duration::from_millis(BELL_FLASH_MS + 50));
    assert!(app.needs_bell_repaint(), "expiry turn still returns true");
    assert!(
        app.take_bell_erase_pending(),
        "the erase frame must not be skipped"
    );
    assert!(
        !app.take_bell_erase_pending(),
        "the signal is one-shot: a later idle frame reads it as false again"
    );
}

// ── Scrollback state machine ─────────────────────

#[test]
fn scroll_position_default_is_live() {
    let app = App::new();
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    assert_eq!(app.scroll_offset(), 0);
}

#[test]
fn scroll_up_by_advances_offset() {
    let mut app = App::new();
    app.scroll_up_by(3);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(3));
    app.scroll_up_by(2);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
}

#[test]
fn scroll_up_by_clamps_to_scrollback_lines() {
    let mut app = App::new();
    // Default scrollback_lines = 10_000.
    app.scroll_up_by(99_999);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
}

#[test]
fn scroll_down_to_zero_snaps_to_live() {
    let mut app = App::new();
    app.scroll_up_by(5);
    app.scroll_down_by(5);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn scroll_down_below_zero_saturates_at_live() {
    let mut app = App::new();
    app.scroll_up_by(3);
    app.scroll_down_by(99);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn scroll_to_top_uses_scrollback_ceiling() {
    let mut app = App::new();
    app.scroll_to_top();
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(10_000));
}

#[test]
fn scroll_to_live_clears_offset() {
    let mut app = App::new();
    app.scroll_up_by(7);
    app.scroll_to_live();
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn alt_screen_suppresses_scroll_up() {
    let mut app = App::new();
    app.alt_screen = true;
    app.scroll_up_by(5);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn alt_screen_suppresses_scroll_to_top() {
    let mut app = App::new();
    app.alt_screen = true;
    app.scroll_to_top();
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn set_alt_screen_true_forces_live() {
    let mut app = App::new();
    app.scroll_up_by(5);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(5));
    app.set_alt_screen(true);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    assert!(app.alt_screen);
}

#[test]
fn set_alt_screen_false_preserves_live() {
    let mut app = App::new();
    app.set_alt_screen(true);
    app.set_alt_screen(false);
    assert!(!app.alt_screen);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn on_pty_output_in_live_is_noop() {
    let mut app = App::new();
    app.needs_full_redraw = false;
    app.on_pty_output(true, 0);
    // No offset change.
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    // No redraw forced (already at live, nothing visual shifted).
    assert!(!app.needs_full_redraw);
}

// ── TS-1..TS-4: scroll-stick (`on_pty_output` Δ branch) ──────────

#[test]
fn on_pty_output_in_live_ignores_delta_and_stays_live() {
    // TS-1: a non-zero Δ on a `Live` view must not snap us into
    // `OffsetFromLive`; the Δ branch only fires when already parked.
    let mut app = App::new();
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    app.on_pty_output(true, 5);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn on_pty_output_in_offset_adds_delta() {
    // TS-2: parked at OffsetFromLive(10) with a generous capacity →
    // Δ=3 advances the offset to 13 and forces a redraw.
    let settings = crate::settings::Settings {
        scrollback_lines: 1000,
        ..Default::default()
    };
    let mut app = App::with_settings(settings);
    app.scroll_position = ScrollPosition::OffsetFromLive(10);
    app.needs_full_redraw = false;
    app.on_pty_output(true, 3);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(13));
    assert!(app.needs_full_redraw);
}

#[test]
fn on_pty_output_in_offset_clamps_to_scrollback_lines() {
    // TS-3: n + Δ would exceed scrollback_lines → clamp at the cap.
    let settings = crate::settings::Settings {
        scrollback_lines: 1000,
        ..Default::default()
    };
    let mut app = App::with_settings(settings);
    app.scroll_position = ScrollPosition::OffsetFromLive(995);
    app.on_pty_output(true, 10);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(1000));
}

#[test]
fn on_pty_output_zero_delta_in_offset_preserves_offset_but_sets_redraw() {
    // TS-4: Δ=0 (capacity-bound or empty pump) preserves the offset.
    // The explicit branch still sets `needs_full_redraw` — that is
    // the prior observable contract (the row-content shift past
    // capacity still needs a repaint).
    let mut app = App::new();
    app.scroll_position = ScrollPosition::OffsetFromLive(7);
    app.needs_full_redraw = false;
    app.on_pty_output(false, 0);
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(7));
    assert!(app.needs_full_redraw);
}

// ── Prompt-to-prompt navigation (OSC 133) ────────────────

use crate::prompts::PromptMarkKind;
use crate::prompts::ResolvedPromptMark;

#[test]
fn jump_prev_scrolls_to_mark_above_view_top() {
    // 100 scrollback rows; a prompt at absolute row 40.
    let mut app = app_with_prompts(100, &[40]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    assert!(scrollback_len >= 100, "expected ≥100 scrollback rows");
    // Start at live (view top = scrollback_len). Prev finds row 40.
    app.jump_to_prompt(JumpDirection::Prev);
    assert_eq!(
        app.scroll_offset(),
        scrollback_len - 40,
        "mark row 40 should sit at the view top"
    );
}

#[test]
fn jump_next_scrolls_to_mark_below_view_top() {
    let mut app = app_with_prompts(100, &[40, 70]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    // Scroll so the view top is at row 50 (between the two marks).
    app.scroll_set_offset(scrollback_len - 50);
    // Next from top=50 finds row 70.
    app.jump_to_prompt(JumpDirection::Next);
    assert_eq!(app.scroll_offset(), scrollback_len - 70);
}

#[test]
fn jump_prev_with_no_mark_above_goes_to_top() {
    // Mark is below the current view top, so Prev finds nothing.
    let mut app = app_with_prompts(100, &[80]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    // View top at row 10 (offset = scrollback_len - 10). No mark < 10.
    app.scroll_set_offset(scrollback_len - 10);
    app.jump_to_prompt(JumpDirection::Prev);
    // Falls to the top — clamped to the scrollback_lines ceiling, which
    // is well above scrollback_len here, so the offset equals
    // scrollback_len (the actual top).
    assert_eq!(app.scroll_offset(), scrollback_len);
}

#[test]
fn jump_next_with_no_mark_below_goes_to_live() {
    let mut app = app_with_prompts(100, &[20]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    // View top at row 50; the only mark (20) is above, so Next finds none.
    app.scroll_set_offset(scrollback_len - 50);
    app.jump_to_prompt(JumpDirection::Next);
    // Falls to the live tail.
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn jump_to_viewport_mark_resolves_to_live() {
    // A mark inside the viewport (row >= scrollback_len) → offset 0.
    let mut app = app_with_prompts(100, &[]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    app.tabs[0].prompts.push(ResolvedPromptMark {
        kind: PromptMarkKind::PromptStart,
        row: scrollback_len + 2, // inside the live viewport
        exit_code: None,
    });
    // Scroll up first so we are not already at live.
    app.scroll_set_offset(scrollback_len - 30);
    app.jump_to_prompt(JumpDirection::Next);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn jump_is_noop_on_alt_screen() {
    let mut app = app_with_prompts(100, &[40]);
    app.alt_screen = true;
    let before = app.scroll_position;
    app.jump_to_prompt(JumpDirection::Prev);
    assert_eq!(app.scroll_position, before);
}

#[test]
fn jump_with_no_tabs_is_noop() {
    let mut app = App::new();
    // No tabs at all.
    app.jump_to_prompt(JumpDirection::Prev);
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

// ── Prompt-jump fold auto-expand (Phase 2 fold step 5) ───

#[test]
fn jump_prev_auto_expands_collapsed_region_containing_mark() {
    // A prompt mark at absolute row 40 lives inside a collapsed fold
    // region [35, 50). Jumping back to it must expand that region so
    // the prompt is visible (mirroring the WebView
    // `expandRegionContaining(marker.lineIndex)`).
    let mut app = app_with_prompts(100, &[40]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    app.tabs[0]
        .folds
        .register_osc133_region(35, 50, "cmd".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(40);
    assert!(app.tabs[0].folds.get_region_at_line(40).unwrap().collapsed);

    app.jump_to_prompt(JumpDirection::Prev);

    // Region expanded …
    assert!(!app.tabs[0].folds.get_region_at_line(40).unwrap().collapsed);
    // … and the scroll offset still places the mark row at the view top.
    assert_eq!(app.scroll_offset(), scrollback_len - 40);
}

#[test]
fn jump_next_auto_expands_collapsed_region_containing_mark() {
    let mut app = app_with_prompts(100, &[40, 70]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    app.tabs[0]
        .folds
        .register_osc133_region(65, 80, "cmd".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(70);
    // View top at row 50 so Next finds the mark at row 70.
    app.scroll_set_offset(scrollback_len - 50);

    app.jump_to_prompt(JumpDirection::Next);

    assert!(!app.tabs[0].folds.get_region_at_line(70).unwrap().collapsed);
    assert_eq!(app.scroll_offset(), scrollback_len - 70);
}

#[test]
fn jump_does_not_touch_unrelated_collapsed_regions() {
    // A collapsed region that does NOT contain the jump target stays
    // collapsed (expand_region_containing only acts on the mark's region).
    let mut app = app_with_prompts(100, &[40]);
    app.tabs[0]
        .folds
        .register_osc133_region(60, 70, "other".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(60);

    app.jump_to_prompt(JumpDirection::Prev);

    assert!(app.tabs[0].folds.get_region_at_line(60).unwrap().collapsed);
}

#[test]
fn jump_with_no_fold_region_at_mark_is_fine() {
    // No fold regions at all: jump behaves exactly as before.
    let mut app = app_with_prompts(100, &[40]);
    let scrollback_len = app.tabs[0].core.lock().get_scrollback_length();
    app.jump_to_prompt(JumpDirection::Prev);
    assert_eq!(app.scroll_offset(), scrollback_len - 40);
}

// ── Fold click toggle (Phase 2 fold step 5) ──────────────

/// Build an `App` with one tab carrying `scrollback` scrollback rows and
/// a single OSC 133 fold region `[start, end)`. The region is collapsed
/// when `collapsed` is set. Returns the app plus the live `rows` /
/// `scrollback_len` so tests can compute display geometry exactly.
fn app_with_fold(scrollback: u32, region: (u32, u32), collapsed: bool) -> (App, u16, u32) {
    let mut app = app_with_prompts(scrollback, &[]);
    let (start, end) = region;
    app.tabs[0]
        .folds
        .register_osc133_region(start, end, "cmd".to_string(), Some(0));
    if collapsed {
        app.tabs[0].folds.toggle_fold(start);
    }
    let (rows, scrollback_len) = {
        let core = app.tabs[0].core.lock();
        (core.rows(), core.get_scrollback_length())
    };
    (app, rows, scrollback_len)
}

#[test]
fn fold_click_on_summary_row_expands_region() {
    // Collapsed region [5, 15) (9 rows hidden). Summary sits at display
    // line 5; scroll so display_start = 5 → the summary is at the top
    // screen row (display_row 0).
    let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), true);
    let total_display = scrollback_len + rows as u32 - 9; // hides 9 rows
    let display_start = 5u32;
    let offset = total_display - rows as u32 - display_start;
    app.scroll_set_offset(offset);

    let acted = app.handle_fold_click(0);

    assert!(acted, "clicking the summary row should act");
    assert!(
        !app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed,
        "summary click must expand the region"
    );
}

#[test]
fn fold_click_inside_expanded_region_collapses_with_scroll_adjust() {
    // Expanded region [5, 35) (30 rows). Scroll so display_start = 10:
    // the region start (display line 5) is above the view top, but the
    // click row 0 (display line 10) lands inside the still-visible body.
    // Collapsing it must pull the offset down by line_count - 1 = 29 to
    // keep the click visually anchored.
    let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 35), false);
    // No region collapsed yet → total_display == total_actual.
    let total_display = scrollback_len + rows as u32;
    let display_start = 10u32;
    let offset = total_display - rows as u32 - display_start; // == scrollback_len - 10
    app.scroll_set_offset(offset);
    let before_offset = app.scroll_offset();

    let acted = app.handle_fold_click(0);

    assert!(
        acted,
        "clicking inside an expanded region should collapse it"
    );
    assert!(
        app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed,
        "interior click must collapse the region"
    );
    // Summary (display line 5) was above the view top (display_start 10)
    // → offset shifts down by line_count - 1 = 29.
    assert_eq!(app.scroll_offset(), before_offset - 29);
}

#[test]
fn fold_click_inside_region_in_viewport_does_not_adjust_scroll() {
    // Expanded region [5, 15). With display_start = 0 the region start
    // (display line 5) is at/below the view top, so collapsing it must
    // NOT shift the scroll offset (mirrors the WebView's
    // `regionDisplayLine < displayStart` guard being false).
    let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), false);
    let total_display = scrollback_len + rows as u32; // nothing collapsed yet
    let offset = total_display - rows as u32; // display_start = 0
    app.scroll_set_offset(offset);
    let before_offset = app.scroll_offset();

    // display_start = 0, so display_row 5 = display line 5 = region start.
    let acted = app.handle_fold_click(5);

    assert!(acted);
    assert!(app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
    assert_eq!(
        app.scroll_offset(),
        before_offset,
        "region start at/below view top must not shift the offset"
    );
}

#[test]
fn fold_click_outside_any_region_is_noop() {
    // Click a screen row that maps to a buffer line outside the region.
    let (mut app, rows, scrollback_len) = app_with_fold(100, (5, 15), false);
    let total_display = scrollback_len + rows as u32;
    let offset = total_display - rows as u32; // display_start = 0
    app.scroll_set_offset(offset);

    // display_row 20 = display line 20 = actual 20, outside [5, 15).
    let acted = app.handle_fold_click(20);

    assert!(!acted, "a click outside any region must be a no-op");
    assert!(!app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn fold_click_below_grid_is_rejected() {
    // A display_row >= rows (a click below the last grid row) is rejected,
    // matching the WebView `displayRow >= rows` guard.
    let (mut app, rows, _sb) = app_with_fold(100, (5, 15), true);
    assert!(!app.handle_fold_click(rows));
    assert!(!app.handle_fold_click(rows + 5));
    // Region unchanged (still collapsed).
    assert!(app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn fold_click_with_no_active_tab_is_noop() {
    let mut app = App::new();
    assert!(app.tabs.is_empty());
    assert!(!app.handle_fold_click(0));
}

#[test]
fn fold_click_when_folding_disabled_is_noop() {
    let (mut app, _rows, _sb) = app_with_fold(100, (5, 15), false);
    app.tabs[0].folds.set_enabled(false);
    // display_start 0 so display_row 5 would otherwise hit the region.
    let acted = app.handle_fold_click(5);
    assert!(!acted, "disabled folding must reject the click");
    assert!(!app.tabs[0].folds.get_region_at_line(5).unwrap().collapsed);
}

#[test]
fn on_pty_output_preserves_offset() {
    let mut app = App::new();
    app.scroll_up_by(4);
    app.needs_full_redraw = false;
    app.on_pty_output(true, 0);
    // Offset preserved: user is not pulled to the bottom.
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(4));
    // Viewport content shifted underneath us, so a repaint is needed.
    assert!(app.needs_full_redraw);
}
