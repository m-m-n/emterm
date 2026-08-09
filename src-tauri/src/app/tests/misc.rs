use super::*;

// ── Tab-bar runtime toggle ────────────────────────────────────────

#[test]
fn show_tab_bar_seeds_from_settings() {
    let app = App::new();
    assert_eq!(app.show_tab_bar, app.settings.show_tab_bar);
}

// ── Select-all ────────────────────────────────────────────────────

#[test]
fn select_all_without_active_tab_is_noop() {
    let mut app = App::new();
    // No tabs spawned (App::new does not call spawn_initial_tab).
    assert!(app.tabs.is_empty());
    app.select_all();
    assert!(
        app.selection.is_none(),
        "select_all with no active tab must not set a selection"
    );
}

#[test]
fn select_all_action_routes_through_apply_action() {
    let mut app = App::new();
    // With no tabs this is a no-op, but it must not panic and must
    // report `false` (no exit request).
    let exit = app.apply_action(crate::ui::AppAction::SelectAll);
    assert!(!exit);
}

#[test]
fn select_all_spans_visible_viewport_at_live() {
    // At live (offset 0) with some scrollback, select_all anchors at the
    // viewport top (= scrollback_len) and spans the on-screen rows.
    let mut app = app_with_prompts(50, &[]);
    let (cols, rows, scrollback_len) = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.rows(), core.get_scrollback_length())
    };
    app.select_all();
    let sel = app.selection.expect("select_all set a selection");
    assert_eq!(
        sel.anchor,
        Pos {
            row: scrollback_len,
            col: 0
        }
    );
    assert_eq!(
        sel.extent,
        Pos {
            row: scrollback_len + (rows - 1) as u32,
            col: cols - 1
        }
    );
}

#[test]
fn select_all_uses_visible_start_when_scrolled() {
    // Scrolled back, select_all starts at the scrolled visible_start, not
    // at the live tail.
    let mut app = app_with_prompts(50, &[]);
    let (rows, scrollback_len) = {
        let core = app.tabs[0].core.lock();
        (core.rows(), core.get_scrollback_length())
    };
    app.scroll_set_offset(10);
    let visible_start = scrollback_len - 10;
    app.select_all();
    let sel = app.selection.expect("select_all set a selection");
    assert_eq!(sel.anchor.row, visible_start);
    assert_eq!(sel.extent.row, visible_start + (rows - 1) as u32);
}

// Known flaky when the full suite runs in parallel (host-load dependent);
// passes in isolation and with --test-threads=1. Rerun this test alone
// before treating a failure as a regression.
#[test]
fn pump_all_shifts_selection_by_eviction_delta() {
    // A selection in absolute rows is shifted down by the active tab's
    // accumulated eviction delta when `pump_all` runs.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.selection = Some(Selection {
        anchor: Pos { row: 20, col: 0 },
        extent: Pos { row: 24, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 20, col: 0 },
    });
    // Drive an eviction of 5 rows through the prompt-mark backfill, which
    // is what `pump` calls in production. This populates the tab's
    // `pending_eviction_delta`.
    app.tabs[0].test_backfill_eviction(5);
    app.pump_all();
    let sel = app.selection.expect("selection survives the shift");
    assert_eq!(sel.anchor, Pos { row: 15, col: 0 });
    assert_eq!(sel.extent, Pos { row: 19, col: 3 });
}

#[test]
fn pump_all_drops_selection_when_fully_evicted() {
    // When the eviction delta exceeds both endpoints, the selection is
    // dropped entirely.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.selection = Some(Selection {
        anchor: Pos { row: 2, col: 0 },
        extent: Pos { row: 6, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 2, col: 0 },
    });
    app.tabs[0].test_backfill_eviction(10);
    app.pump_all();
    assert!(
        app.selection.is_none(),
        "fully-evicted selection must be dropped"
    );
}

#[test]
fn pump_all_clears_selection_on_frame_reset() {
    // A core reset (RIS) makes the eviction counter go backwards, latching
    // a frame reset that drops the absolute-row selection.
    let mut app = App::new();
    app.spawn_initial_tab();
    // Establish a non-zero eviction baseline first.
    app.tabs[0].test_backfill_eviction(8);
    // Drain the resulting delta so it does not also shift the selection.
    let _ = app.tabs[0].take_eviction_delta();
    app.selection = Some(Selection {
        anchor: Pos { row: 4, col: 0 },
        extent: Pos { row: 9, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 4, col: 0 },
    });
    // Counter goes backwards → frame reset latch.
    app.tabs[0].test_backfill_eviction(0);
    app.pump_all();
    assert!(
        app.selection.is_none(),
        "frame reset must clear the selection"
    );
}

/// TS-8 (integration): an off-thread snapshot swap completing on the
/// active tab during a single `pump_all` reconciles like the synchronous
/// path — the absolute-row selection is dropped (frame reset) and a full
/// redraw is forced (FR2: a shorter incoming pane leaves no residual
/// rows). No `pump_all` polling loop: the worker is blocked-ready first,
/// then `pump_all` is called exactly once.
#[test]
fn ts8_offthread_swap_reconciles_active_tab_on_pump() {
    use mux_ipc::protocol::{MessageType, MuxMessage};

    let mut app = App::new();
    app.spawn_initial_tab();
    // Seed a 2-pane mux group, active pane = 10.
    {
        let group = app.tabs[0]
            .mux_group
            .get_or_insert_with(crate::mux::window_group::MuxWindowGroup::new);
        group.seed(
            vec![
                crate::mux::window_group::MuxWindow {
                    id: 1,
                    name: "a".into(),
                },
                crate::mux::window_group::MuxWindow {
                    id: 2,
                    name: "b".into(),
                },
            ],
            vec![10, 20],
            0,
        );
    }
    // A stale absolute-row selection that the frame reset must drop.
    app.selection = Some(Selection {
        anchor: Pos { row: 2, col: 0 },
        extent: Pos { row: 6, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 2, col: 0 },
    });
    app.needs_full_redraw = false;

    // Dispatch a large snapshot off-thread for the active pane.
    let threshold = crate::tabs::OFFTHREAD_REPLAY_THRESHOLD_BYTES;
    let mut payload = b"SWAPPED-IN\r\n".to_vec();
    payload.resize(threshold + 16, 0);
    app.tabs[0].apply_mux_message(MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id: 10,
        payload,
    });
    assert!(app.tabs[0].test_has_pending_switch());

    // Block until the worker is ready (re-staged for try_recv), then pump
    // exactly once.
    app.tabs[0].test_block_worker_ready();
    app.pump_all();

    // Swap completed: no pending switch, content replaced, selection
    // dropped by the frame reset, full redraw forced.
    assert!(!app.tabs[0].test_has_pending_switch());
    assert_eq!(app.tabs[0].test_row_text(0), "SWAPPED-IN");
    assert!(
        app.selection.is_none(),
        "off-thread swap frame reset must drop the stale selection"
    );
    assert!(
        app.needs_full_redraw,
        "off-thread swap on the active tab must force a full redraw (FR2)"
    );
}

#[test]
fn switch_to_tab_clears_selection() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab(); // now 2 tabs, active = 1
    app.selection = Some(Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    });
    app.switch_to_tab(0);
    assert!(
        app.selection.is_none(),
        "switching tabs clears the active-tab-scoped selection"
    );
}

#[test]
fn width_change_clears_absolute_row_trackers() {
    // A column-width change reflows the buffer (rewriting the line
    // mapping without moving the eviction counter), so every App-owned
    // absolute-row tracker must be dropped (N3).
    //
    // per-tab-grid-size task0001 (D3 ownership split): the prompt/fold
    // clearing assertions that used to live here moved to tabs.rs — Tab
    // now clears its OWN reflow-invalidated trackers inside its own
    // resize when its column count changes, so App no longer calls
    // `Tab::clear_reflow_invalidated_state` from any resize path. This
    // test keeps only the App-owned trackers (selection, pending
    // anchor).
    let mut app = app_with_seeded_trackers();
    app.set_grid_size(40, 24); // width 80 -> 40
    assert!(app.selection.is_none(), "selection dropped on reflow");
    assert!(
        app.pending_selection_anchor.is_none(),
        "pending anchor dropped on reflow"
    );
}

#[test]
fn height_only_change_keeps_absolute_row_trackers() {
    // A height-only resize does not reflow (resize_same_width keeps the
    // wrap boundaries), so the absolute-row trackers stay valid.
    let mut app = app_with_seeded_trackers();
    app.set_grid_size(80, 30); // same width 80, taller
    assert!(
        app.selection.is_some(),
        "selection kept on height-only resize"
    );
    assert!(
        app.pending_selection_anchor.is_some(),
        "pending anchor kept on height-only resize"
    );
    assert_eq!(
        app.tabs[0].prompts.find_prev_prompt(u32::MAX),
        Some(5),
        "prompt marks kept on height-only resize"
    );
    assert!(
        app.tabs[0].folds.get_region_at_line(5).is_some(),
        "fold regions kept on height-only resize"
    );
}

/// AC-5, D3'''''' (round-9 rework, review round-8 finding
/// `1e7e069001cf22dc`): `App::set_grid_size` must clamp BEFORE recording
/// `self.cell_size`, so the app's own grid record always agrees with
/// what `Tab::resize` actually applies to the core it drives — never
/// the caller's raw, out-of-wire-domain request.
///
/// Confirmed to fail pre-fix: before this change, `self.cell_size` was
/// assigned the caller's RAW `(cols, rows)` and only `Tab::resize`
/// (called per-tab afterward) clamped the core — so
/// `app.cell_size.rows` would come out as `u16::MAX` while
/// `core.rows()` was already the clamped value, disagreeing with each
/// other exactly as the finding describes.
#[test]
fn set_grid_size_clamps_cell_size_to_agree_with_the_core() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.set_grid_size(u16::MAX, u16::MAX);
    let (expected_cols, expected_rows) =
        crate::mux::session::pane::clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
    assert_eq!(
        (app.cell_size.cols, app.cell_size.rows),
        (expected_cols, expected_rows),
        "the app's own grid record must be the CLAMPED wire-domain \
             dims, not the caller's raw, out-of-domain request"
    );
    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (expected_cols, expected_rows),
        "the tab's core must match the app's own grid record exactly"
    );
    assert_eq!(
        (app.cell_size.cols, app.cell_size.rows),
        (core.cols(), core.rows()),
        "App::cell_size and the tab's core must never disagree"
    );
}

// ── Settings window (child-process launcher) ───────────────

/// Counting launcher double: records `open()` calls instead of
/// spawning a real `--settings` child.
struct CountingLauncher(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::settings_launcher::SettingsWindowLauncher for CountingLauncher {
    fn open(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn install_counting_launcher(app: &mut App) -> std::rc::Rc<std::cell::Cell<usize>> {
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    app.settings_launcher = Box::new(CountingLauncher(count.clone()));
    count
}

#[test]
fn open_settings_action_spawns_the_settings_window() {
    let mut app = App::new();
    let opened = install_counting_launcher(&mut app);
    assert!(!app.apply_action(crate::ui::AppAction::OpenSettings));
    assert_eq!(opened.get(), 1);
}

#[test]
fn open_settings_tab_event_spawns_the_settings_window() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let opened = install_counting_launcher(&mut app);
    assert!(!app.apply_tab_event(crate::ui::TabEvent::OpenSettings));
    assert_eq!(opened.get(), 1);
    // The terminal pane keeps focus; no in-app tab is created.
    assert!(app.active_tab().is_some());
    assert_eq!(app.tabs.len(), 1);
}
