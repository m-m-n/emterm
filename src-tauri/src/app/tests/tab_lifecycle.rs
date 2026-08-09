use super::*;

// ── Phase 4-B: tab event + AppAction routing ─────────────

/// TS-tab-2 (App half): closing the last tab empties the tabs
/// vector. The run loop in `window_host` translates an empty
/// `app.tabs` into `ControlFlow::Exit` (see `run` in
/// `window_host.rs`), so this is the `ExitWindow` signal.
#[test]
fn closing_last_tab_signals_exit_window() {
    let mut app = App::new();
    // Manually push a Tab-like value would require a PTY; instead
    // we exercise the `close_tab` path on a synthetic tabs vector.
    // Tab::spawn_shell is fine in tests — it returns pty=None when
    // spawn fails, but the tab itself is constructed.
    app.spawn_initial_tab();
    assert_eq!(app.tabs.len(), 1, "exactly one tab after init");
    let exit = app.close_tab(0);
    assert!(exit, "closing the last tab must return true");
    assert!(app.tabs.is_empty(), "tabs vector must be empty after close");

    // The same routing via TabEvent must agree.
    let mut app2 = App::new();
    app2.spawn_initial_tab();
    let exit2 = app2.apply_tab_event(crate::ui::TabEvent::Close(0));
    assert!(exit2);
    assert!(app2.tabs.is_empty());
}

#[test]
fn close_tab_in_middle_shifts_active_left_when_needed() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.spawn_new_tab();
    assert_eq!(app.tabs.len(), 3);
    app.active = 2;
    // Close idx 0 → active was 2, now should be 1.
    let exit = app.close_tab(0);
    assert!(!exit);
    assert_eq!(app.active, 1);
    assert_eq!(app.tabs.len(), 2);
}

#[test]
fn close_tab_clamps_active_when_closing_the_active_last_one() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.active = 1;
    let exit = app.close_tab(1);
    assert!(!exit);
    // Active falls back to the new last tab.
    assert_eq!(app.active, 0);
    assert_eq!(app.tabs.len(), 1);
}

// ── TabEvent::Reorder — drag-and-drop reorder ──────────

#[test]
fn reorder_tab_moves_first_to_end_and_keeps_active_pointing_at_moved() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.spawn_new_tab();
    assert_eq!(app.tabs.len(), 3);
    app.active = 0;
    // Drop the first tab past the last: insertion index 3 → after
    // removal of slot 0 it lands at slot 2.
    app.reorder_tab(0, 3);
    assert_eq!(app.tabs.len(), 3, "tab count must not change");
    assert_eq!(app.active, 2, "moved tab follows its new slot");
}

#[test]
fn reorder_tab_shifts_active_when_moving_a_tab_past_it() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.spawn_new_tab();
    app.active = 1;
    // Move tab 0 (not active) past the active one to the end.
    // After removal, insert_at = 3 - 1 = 2. Active was 1, from(0)
    // < active(1) and insert_at(2) >= active(1) → active shifts to 0.
    app.reorder_tab(0, 3);
    assert_eq!(app.active, 0);
}

#[test]
fn reorder_tab_ignores_no_op_targets() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.active = 1;
    // to == from
    app.reorder_tab(0, 0);
    assert_eq!(app.active, 1);
    // to == from + 1 (would land in the same slot)
    app.reorder_tab(0, 1);
    assert_eq!(app.active, 1);
}

#[test]
fn reorder_tab_ignores_out_of_range_from() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let active_before = app.active;
    let len_before = app.tabs.len();
    app.reorder_tab(42, 0);
    assert_eq!(app.active, active_before);
    assert_eq!(app.tabs.len(), len_before);
}

#[test]
fn apply_tab_event_routes_reorder_to_reorder_tab() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.spawn_new_tab();
    app.active = 0;
    let exit = app.apply_tab_event(crate::ui::TabEvent::Reorder { from: 0, to: 3 });
    assert!(!exit);
    assert_eq!(app.active, 2);
}

#[test]
fn next_tab_wraps_at_end() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.active = 1;
    let exit = app.apply_action(crate::ui::AppAction::NextTab);
    assert!(!exit);
    assert_eq!(app.active, 0);
}

#[test]
fn prev_tab_wraps_at_start() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.active = 0;
    let exit = app.apply_action(crate::ui::AppAction::PrevTab);
    assert!(!exit);
    assert_eq!(app.active, 1);
}

#[test]
fn jump_tab_clamps_to_existing_range() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    // Only two tabs; Ctrl+9 should clamp to the last (idx 1).
    let exit = app.apply_action(crate::ui::AppAction::JumpTab(9));
    assert!(!exit);
    assert_eq!(app.active, 1);
    // Ctrl+1 jumps to idx 0.
    app.apply_action(crate::ui::AppAction::JumpTab(1));
    assert_eq!(app.active, 0);
}

#[test]
fn new_tab_action_appends_and_switches() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let before = app.tabs.len();
    let exit = app.apply_action(crate::ui::AppAction::NewTab);
    assert!(!exit);
    assert_eq!(app.tabs.len(), before + 1);
    assert_eq!(app.active, app.tabs.len() - 1);
}

#[test]
fn close_tab_action_can_signal_exit() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let exit = app.apply_action(crate::ui::AppAction::CloseTab);
    assert!(exit);
}

#[test]
fn tab_event_switch_changes_active_without_exit() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    app.active = 0;
    let exit = app.apply_tab_event(crate::ui::TabEvent::Switch(1));
    assert!(!exit);
    assert_eq!(app.active, 1);
}

// ── per-tab-grid-size task0001: active-tab-only resize routing ──────

/// AC-1 (TS1), FR1/FR2: `App::set_grid_size` resizes ONLY the active
/// tab's core — an inactive tab's PTY/core must never receive a resize
/// behind the user's back.
#[test]
fn set_grid_size_resizes_only_the_active_tab() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0
    app.spawn_new_tab(); // tab1, active = 1
    let tab0_before = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.rows())
    };
    app.set_grid_size(100, 40);
    let tab1_after = {
        let core = app.tabs[1].core.lock();
        (core.cols(), core.rows())
    };
    assert_eq!(
        tab1_after,
        (100, 40),
        "the active tab is resized to the new grid size"
    );
    let tab0_after = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.rows())
    };
    assert_eq!(
        tab0_before, tab0_after,
        "an inactive tab's core dims are untouched by a resize routed to \
             the active tab only"
    );
}

/// AC-4 (TS5), FR6/D3: an inactive tab is never resized, so its
/// tab-owned trackers (prompt marks, fold regions) survive another
/// tab's width-changing resize. The positive "the resized tab's own
/// marks get cleared" assertion is deliberately NOT covered here — it
/// is the sibling tabs.rs task's responsibility (D3 ownership split).
#[test]
fn width_change_of_active_tab_leaves_inactive_tabs_prompt_and_fold_marks_untouched() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0
    app.set_grid_size(80, 24); // normalize width before seeding
    app.tabs[0]
        .prompts
        .push(crate::prompts::ResolvedPromptMark {
            kind: crate::prompts::PromptMarkKind::PromptStart,
            row: 5,
            exit_code: None,
        });
    app.tabs[0]
        .folds
        .register_osc133_region(5, 8, "cmd".to_string(), None);
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(40, 24); // width change while tab1 is active

    assert_eq!(
        app.tabs[0].prompts.find_prev_prompt(u32::MAX),
        Some(5),
        "inactive tab0's prompt mark survives tab1's width change"
    );
    assert!(
        app.tabs[0].folds.get_region_at_line(5).is_some(),
        "inactive tab0's fold region survives tab1's width change"
    );
}

/// AC-5 (TS4) / AC-6 (TS10), FR4: an inactive mux-flavored tab's core
/// dims are unchanged by a grid-size change, AND — read directly from
/// the frame observation hook rather than inferred from dims — it
/// records ZERO pane `Resize` frames.
///
/// Updated (task0003 rework, NFR2, finding cfcbfae57964beb5): the
/// original assertion here claimed "restricting `Tab::resize` to the
/// active tab is the ONLY emission site for pane `Resize` frames, so
/// unresized dims prove no frame was sent" — that premise is FALSE
/// (mux attach/Welcome pane seeding and `PaneCreated` handling are
/// separate, non-resize-path emission sites this tab's core-dims check
/// cannot see), and a dims-only proxy cannot distinguish "no frame
/// sent" from "a frame sent that happened not to change dims" in the
/// first place. The frame-log assertion below observes emission
/// directly; the dims check is kept alongside it as a still-true,
/// weaker fact.
#[test]
fn set_grid_size_leaves_inactive_mux_tab_core_dims_unchanged() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0: will be the inactive mux tab
    app.tabs[0].mux_session_name = Some("main".to_string());
    let mut group = crate::mux::window_group::MuxWindowGroup::new();
    group.seed(
        vec![crate::mux::window_group::MuxWindow {
            id: 0,
            name: "w0".to_string(),
        }],
        vec![9],
        0,
    );
    app.tabs[0].mux_group = Some(group);
    let before = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.rows())
    };
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40);
    let after = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.rows())
    };
    assert_eq!(
        before, after,
        "inactive mux tab's core dims unchanged (no Tab::resize \
             invocation reached it)"
    );
    assert!(
        app.tabs[0].test_resize_frames().is_empty(),
        "inactive mux tab records zero pane Resize frames — observed \
             directly via the frame hook, not inferred from dims"
    );
}

// ── per-tab-grid-size task0001/task0003: activation reconcile (D2) ──

/// AC-1 (TS8), FR3/NFR1: reproduces the exact round-1 defect scenario —
/// per-tab-dependent insets mean the incoming tab's settled display
/// area can differ from the outgoing tab's. `cell_size` is settled
/// TWICE here: once while the OUTGOING tab is active (120x40, standing
/// in for e.g. no persistent-sidebar inset), and once for the INCOMING
/// tab AFTER it becomes active (90x30, standing in for a narrower
/// inset). Before this rework, the reconcile ran synchronously inside
/// `switch_to_tab` and would have resized the incoming tab to the
/// STILL-CURRENT 120x40 — a wrong-dims resize corrected only on the
/// next unrelated trigger. Confirmed to fail pre-fix: the old
/// `reconcile_active_tab_size` ran inline in `switch_to_tab`, so the
/// assertion below (dims still 80x24 immediately after `switch_to_tab`,
/// never 120x40) would have failed with `(120, 40)`.
#[test]
fn switch_to_tab_reconciles_to_incoming_dims_never_outgoing_when_insets_differ_per_tab() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0, at 80x24
    app.spawn_new_tab(); // tab1, active = 1, at 80x24
    app.set_grid_size(120, 40); // settle OUTGOING tab1's own display area
    app.switch_to_tab(0); // request activation of tab0; must not resize synchronously
    {
        let core = app.tabs[0].core.lock();
        assert_eq!(
            (core.cols(), core.rows()),
            (80, 24),
            "switch_to_tab must not resize the incoming tab synchronously \
                 against the OUTGOING tab's still-current cell_size (120x40) \
                 — that was the round-1 defect (dbb7766a6212fb1a)"
        );
    }
    // Render pass settles insets for the NEW active tab (tab0) — its own
    // display area differs from tab1's.
    app.set_grid_size(90, 30);
    app.execute_pending_reconcile();
    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (90, 30),
        "the incoming tab reconciles to ITS OWN settled display area \
             (90x30), never the outgoing tab's dims (120x40) — the transition \
             observed above was 80x24 -> 90x30 directly, with no intermediate \
             120x40 resize"
    );
}

/// AC-2 (TS2), FR3: activating a tab whose dims differ from the current
/// display area resizes it once the deferred reconcile executes.
///
/// Updated (task0003 rework, NFR2): `switch_to_tab` no longer resizes
/// synchronously — it only requests a reconcile (D2 request/execute
/// split). `execute_pending_reconcile()` now stands in for the
/// `window_host::render` call point that consumes the request once
/// insets have settled.
#[test]
fn switch_to_tab_resizes_incoming_tab_when_its_dims_differ_from_cell_size() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0, spawned at cell_size (80x24)
    app.spawn_new_tab(); // tab1, active = 1, spawned at cell_size (80x24)
    app.set_grid_size(100, 40); // resizes only the active tab1; tab0 stays 80x24
    app.switch_to_tab(0); // requests activation of tab0, whose dims differ from cell_size
    app.execute_pending_reconcile(); // consumes the request (mirrors window_host::render)

    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (100, 40),
        "activating a tab whose dims differ from the display area \
             resizes it once the deferred reconcile executes"
    );
}

/// AC-2 (TS2), FR3: activating a tab whose dims already match
/// `cell_size` issues no resize — dims stay unchanged and the incoming
/// tab's own trackers are left alone. The FR3 no-op guarantee must
/// survive the request/execute split.
///
/// Updated (task0003 rework, NFR2): added the `execute_pending_reconcile()`
/// call so the no-op assertion covers the deferred path, not just the
/// (now nonexistent) synchronous one.
#[test]
fn switch_to_tab_issues_no_resize_when_incoming_dims_already_match_cell_size() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0
    app.tabs[0]
        .prompts
        .push(crate::prompts::ResolvedPromptMark {
            kind: crate::prompts::PromptMarkKind::PromptStart,
            row: 5,
            exit_code: None,
        });
    app.tabs[0]
        .folds
        .register_osc133_region(5, 8, "cmd".to_string(), None);
    app.spawn_new_tab(); // tab1, active = 1 — both tabs still at cell_size
    app.switch_to_tab(0);
    app.execute_pending_reconcile();

    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (80, 24),
        "dims stay unchanged when the incoming tab already matches \
             cell_size"
    );
    drop(core);
    assert_eq!(
        app.tabs[0].prompts.find_prev_prompt(u32::MAX),
        Some(5),
        "no resize means no tracker invalidation on the incoming tab"
    );
    assert!(
        app.tabs[0].folds.get_region_at_line(5).is_some(),
        "no resize means no tracker invalidation on the incoming tab"
    );
}

/// AC-3 (TS2/TS8), FR3/D2: the close-tab active-index fix-up also
/// produces a reconcile request whose execution reconciles the
/// newly-active tab's size — identical to the explicit switch path.
///
/// Updated (task0003 rework, NFR2): added the `execute_pending_reconcile()`
/// call — `close_tab` now only requests the reconcile (D2 request/execute
/// split); it no longer resizes synchronously.
#[test]
fn close_tab_active_index_fixup_reconciles_newly_active_tab_size() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40); // resizes only the active tab1; tab0 stays 80x24
    app.close_tab(1); // closes the active tab; fix-up requests a reconcile for tab0
    app.execute_pending_reconcile();

    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active, 0);
    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (100, 40),
        "close-tab active-index fix-up reconciles the newly-active \
             tab's size"
    );
}

/// AC-3 (TS2/TS8), FR3/D2: the exited-tab reap active-index fix-up also
/// produces a reconcile request whose execution reconciles the
/// newly-active tab's size — identical to the explicit switch path.
///
/// Updated (task0003 rework, NFR2): added the `execute_pending_reconcile()`
/// call — the reap fix-up now only requests the reconcile; it no longer
/// resizes synchronously.
#[test]
fn pump_all_reap_active_index_fixup_reconciles_newly_active_tab_size() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40); // resizes only the active tab1; tab0 stays 80x24
    app.tabs[1].exited = true;
    app.pump_all(); // reap removes tab1; fix-up requests a reconcile for tab0
    app.execute_pending_reconcile();

    assert_eq!(app.tabs.len(), 1, "exited tab reaped");
    assert_eq!(app.active, 0);
    let core = app.tabs[0].core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (100, 40),
        "reap active-index fix-up reconciles the newly-active tab's size"
    );
}

/// AC-4 (TS9), FR6/D3: a reconcile execution that changes the target
/// tab's column count clears the App-owned trackers (selection, pending
/// anchor) on the explicit-switch activation origin. Trackers are
/// seeded AFTER `switch_to_tab` (which unconditionally clears both at
/// the top of the function, independent of D3) so the assertion below
/// isolates the reconcile's OWN width-change clearing, exercised
/// through the shared App resize application path
/// (`apply_tab_resize`).
#[test]
fn switch_to_tab_reconcile_width_change_clears_selection_and_pending_anchor() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0, 80x24
    app.spawn_new_tab(); // tab1, active = 1, 80x24
    app.set_grid_size(100, 40); // settles cell_size to 100x40 via active tab1
    app.switch_to_tab(0); // request only; tab0 still 80x24
    app.selection = Some(Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
    app.execute_pending_reconcile(); // resizes tab0 80->100 cols: width change

    assert!(
        app.selection.is_none(),
        "reconcile width-change clears the App-owned selection"
    );
    assert!(
        app.pending_selection_anchor.is_none(),
        "reconcile width-change clears the App-owned pending anchor"
    );
}

/// AC-4 (TS9), FR6/D3: the close-tab activation origin also clears the
/// App-owned trackers via the reconcile executor's shared resize path —
/// not just the explicit-switch origin. Round-1 findings
/// a172de726b3cbc29 / d39a6a9468ff892e: this origin previously reached
/// a width-changing resize with NO App-side clearing at all, because
/// the clearing lived only in `set_grid_size`.
#[test]
fn close_tab_reconcile_width_change_clears_selection_and_pending_anchor() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40); // tab0 stays 80x24; cell_size=100x40
    app.selection = Some(Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
    app.close_tab(1); // closes the active tab; fix-up requests a reconcile for tab0
    app.execute_pending_reconcile(); // resizes tab0 80->100 cols: width change

    assert!(
        app.selection.is_none(),
        "close-tab reconcile width-change clears the App-owned selection"
    );
    assert!(
        app.pending_selection_anchor.is_none(),
        "close-tab reconcile width-change clears the App-owned pending anchor"
    );
}

/// AC-4 (TS9), FR6/D3: the exited-tab reap activation origin also
/// clears the App-owned trackers via the reconcile executor (same
/// rationale as the close-tab case above).
#[test]
fn reap_reconcile_width_change_clears_selection_and_pending_anchor() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40); // tab0 stays 80x24; cell_size=100x40
    app.selection = Some(Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
    app.tabs[1].exited = true;
    app.pump_all(); // reap removes tab1; fix-up requests a reconcile for tab0
    app.execute_pending_reconcile(); // resizes tab0 80->100 cols: width change

    assert!(
        app.selection.is_none(),
        "reap reconcile width-change clears the App-owned selection"
    );
    assert!(
        app.pending_selection_anchor.is_none(),
        "reap reconcile width-change clears the App-owned pending anchor"
    );
}

/// AC-4 (TS9), FR6/D3: a HEIGHT-ONLY reconcile (no column-count change)
/// leaves the App-owned trackers untouched — the shared resize
/// application path (`apply_tab_resize`) only clears them when the
/// resize actually changed the column count (N3).
#[test]
fn reconcile_height_only_change_keeps_selection_and_pending_anchor() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0, active = 0, 80x24
    app.spawn_new_tab(); // tab1, active = 1, 80x24
    app.set_grid_size(80, 40); // same width, taller — settles cell_size to 80x40
    app.switch_to_tab(0); // request only; tab0 still 80x24
    app.selection = Some(Selection {
        anchor: Pos { row: 0, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 0, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
    app.execute_pending_reconcile(); // resizes tab0 80x24 -> 80x40: height-only

    assert!(
        app.selection.is_some(),
        "height-only reconcile keeps the App-owned selection"
    );
    assert!(
        app.pending_selection_anchor.is_some(),
        "height-only reconcile keeps the App-owned pending anchor"
    );
}

/// AC-5 (TS9), FR6: `set_grid_size` and the reconcile executor issue
/// their resize through the SAME App-side application path
/// (`apply_tab_resize`) — verified by both producing IDENTICAL
/// width-change clearing behavior under the same seeded tracker state.
#[test]
fn set_grid_size_and_reconcile_executor_share_width_change_clearing_behavior() {
    // Path 1: set_grid_size drives the width change directly.
    let mut via_set_grid_size = app_with_seeded_trackers();
    via_set_grid_size.set_grid_size(40, 24); // width 80 -> 40
    assert!(
        via_set_grid_size.selection.is_none(),
        "set_grid_size width-change clears selection"
    );
    assert!(
        via_set_grid_size.pending_selection_anchor.is_none(),
        "set_grid_size width-change clears pending anchor"
    );

    // Path 2: the reconcile executor drives an equivalent width change
    // on an activation, under an identically-seeded tracker state.
    let mut via_reconcile = App::new();
    via_reconcile.spawn_initial_tab(); // tab0, active = 0, 80x24
    via_reconcile.spawn_new_tab(); // tab1, active = 1, 80x24
    via_reconcile.set_grid_size(40, 24); // settle cell_size at the narrower width
    via_reconcile.switch_to_tab(0); // request only; tab0 still 80x24
    via_reconcile.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 3, col: 4 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    via_reconcile.pending_selection_anchor = Some(Pos { row: 2, col: 1 });
    via_reconcile.execute_pending_reconcile(); // resizes tab0 80->40 cols: width change

    assert!(
        via_reconcile.selection.is_none(),
        "reconcile executor width-change clears selection — identical \
             to the set_grid_size path above"
    );
    assert!(
        via_reconcile.pending_selection_anchor.is_none(),
        "reconcile executor width-change clears pending anchor — \
             identical to the set_grid_size path above"
    );
}

/// AC-6 (TS10), FR4: via the frame observation hook, a dims-changing
/// mux tab activation reconcile records EXACTLY one frame SET — one
/// frame per pane in the tab's mux group — at the incoming tab's
/// post-clamp dims, and no frame is ever recorded before the reconcile
/// executes (i.e. none at any stale/outgoing dims).
#[test]
fn reconcile_dims_changing_mux_tab_activation_records_one_frame_set_at_incoming_dims() {
    let mut app = App::new();
    app.spawn_initial_tab(); // tab0: mux-flavored, will be activated
    app.tabs[0].mux_session_name = Some("main".to_string());
    let mut group = crate::mux::window_group::MuxWindowGroup::new();
    group.seed(
        vec![
            crate::mux::window_group::MuxWindow {
                id: 0,
                name: "w0".to_string(),
            },
            crate::mux::window_group::MuxWindow {
                id: 1,
                name: "w1".to_string(),
            },
        ],
        vec![9, 11],
        0,
    );
    app.tabs[0].mux_group = Some(group);
    app.spawn_new_tab(); // tab1, active = 1
    app.set_grid_size(100, 40); // resizes only the active tab1; tab0 stays 80x24
    assert!(
        app.tabs[0].test_resize_frames().is_empty(),
        "no frame yet: tab0 is still inactive and untouched"
    );

    app.switch_to_tab(0); // request only; must not resize/emit synchronously
    assert!(
        app.tabs[0].test_resize_frames().is_empty(),
        "switch_to_tab alone must not emit a Resize frame — that would \
             be at the OUTGOING tab's stale cell_size, the round-1 defect"
    );

    app.execute_pending_reconcile(); // resizes tab0 80->100 cols: emits one frame per pane
    let frames = app.tabs[0].test_resize_frames();
    assert_eq!(
        frames.len(),
        2,
        "one frame per pane in the mux group == one frame SET"
    );
    for frame in &frames {
        assert_eq!(
            (frame.cols, frame.rows),
            (100, 40),
            "every frame in the set carries the INCOMING tab's \
                 post-clamp dims, never a stale/outgoing value"
        );
    }
}

#[test]
fn switch_to_tab_clears_pending_selection_anchor() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab(); // now 2 tabs, active = 1
    app.pending_selection_anchor = Some(Pos { row: 3, col: 2 });
    app.switch_to_tab(0);
    assert!(
        app.pending_selection_anchor.is_none(),
        "switching tabs clears the pending press anchor"
    );
}

#[test]
fn set_alt_screen_true_clears_selection_and_anchor() {
    // Toggling onto the alt screen changes the buffer the absolute-row
    // selection addresses, so both the selection and a pending press
    // anchor must be dropped.
    let mut app = App::new();
    app.selection = Some(Selection {
        anchor: Pos { row: 1, col: 0 },
        extent: Pos { row: 2, col: 3 },
        mode: SelectionMode::Character,
        origin: Pos { row: 1, col: 0 },
    });
    app.pending_selection_anchor = Some(Pos { row: 1, col: 1 });
    app.set_alt_screen(true);
    assert!(
        app.selection.is_none(),
        "alt-screen toggle clears selection"
    );
    assert!(
        app.pending_selection_anchor.is_none(),
        "alt-screen toggle clears the pending press anchor"
    );
}

// Known flaky when the full suite runs in parallel (host-load dependent);
// passes in isolation and with --test-threads=1. Rerun this test alone
// before treating a failure as a regression.
#[test]
fn pump_all_shifts_pending_anchor_by_eviction_delta() {
    // A pending press anchor in absolute rows is shifted down by the
    // active tab's accumulated eviction delta, exactly like `selection`.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.pending_selection_anchor = Some(Pos { row: 20, col: 4 });
    app.tabs[0].test_backfill_eviction(5);
    app.pump_all();
    assert_eq!(
        app.pending_selection_anchor,
        Some(Pos { row: 15, col: 4 }),
        "pending anchor shifts with the eviction delta"
    );
}

#[test]
fn pump_all_drops_pending_anchor_when_scrolled_off_top() {
    // When the eviction delta exceeds the anchor's row, the anchor scrolled
    // off the top of scrollback and is dropped.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.pending_selection_anchor = Some(Pos { row: 3, col: 0 });
    app.tabs[0].test_backfill_eviction(10);
    app.pump_all();
    assert!(
        app.pending_selection_anchor.is_none(),
        "a fully-evicted pending anchor is dropped"
    );
}

#[test]
fn pump_all_clears_pending_anchor_on_frame_reset() {
    // A frame reset (RIS) drops the absolute-row pending anchor alongside
    // the selection.
    let mut app = App::new();
    app.spawn_initial_tab();
    app.tabs[0].test_backfill_eviction(8);
    let _ = app.tabs[0].take_eviction_delta();
    app.pending_selection_anchor = Some(Pos { row: 4, col: 0 });
    // Counter goes backwards → frame reset latch.
    app.tabs[0].test_backfill_eviction(0);
    app.pump_all();
    assert!(
        app.pending_selection_anchor.is_none(),
        "frame reset must clear the pending press anchor"
    );
}

#[test]
fn select_all_uses_fold_layout_visible_span_when_collapsed() {
    // With a collapsed fold region in view, the screen rows are
    // non-contiguous in absolute space. select_all must take its
    // anchor/extent from the layout's first/last visible rows rather than
    // the linear `visible_start + (rows - 1)` model.
    let mut app = app_with_prompts(100, &[]);
    let (cols, scrollback_len) = {
        let core = app.tabs[0].core.lock();
        (core.cols(), core.get_scrollback_length())
    };
    // Collapse a region near the live tail so its summary survives in the
    // visible window.
    let region_start = scrollback_len + 1;
    let region_end = region_start + 5;
    app.tabs[0]
        .folds
        .register_osc133_region(region_start, region_end, "cmd".to_string(), Some(0));
    app.tabs[0].folds.toggle_fold(region_start);
    // Build the per-frame layout the renderer / select_all consult.
    app.refresh_fold_layout();
    let layout = app
        .fold_layout()
        .expect("collapsed region produces a layout")
        .clone();
    let expected_first = match layout.rows.first().unwrap() {
        crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
        crate::fold::FoldRowKind::Summary { region } => region.start_line,
    };
    let expected_last = match layout.rows.last().unwrap() {
        crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
        crate::fold::FoldRowKind::Summary { region } => region.start_line,
    };

    app.select_all();
    let sel = app.selection.expect("select_all set a selection");
    assert_eq!(
        sel.anchor,
        Pos {
            row: expected_first,
            col: 0
        }
    );
    assert_eq!(
        sel.extent,
        Pos {
            row: expected_last,
            col: cols - 1
        }
    );
}

#[test]
fn dirty_set_maps_scrolled_selection_to_screen_rows() {
    // A selection in absolute rows is dirtied at the screen rows it
    // currently occupies, honoring scroll_offset.
    let mut app = app_with_prompts(50, &[]);
    // Clone the core Arc so the lock guard doesn't borrow `app` while we
    // need `&mut app` for record_render_state.
    let core_arc = app.tabs[0].core.clone();
    let scrollback_len = core_arc.lock().get_scrollback_length();
    // Scroll back by 10 → visible_start = scrollback_len - 10. Clear the
    // full-redraw latch so the union path runs.
    app.scroll_set_offset(10);
    {
        let mut core = core_arc.lock();
        app.record_render_state(&mut core);
    }
    let visible_start = scrollback_len - 10;
    // Select two absolute rows that fall on screen rows 3 and 4.
    app.selection = Some(Selection {
        anchor: Pos {
            row: visible_start + 3,
            col: 0,
        },
        extent: Pos {
            row: visible_start + 4,
            col: 5,
        },
        mode: SelectionMode::Character,
        origin: Pos {
            row: visible_start + 3,
            col: 0,
        },
    });
    let set = {
        let core = core_arc.lock();
        app.dirty_rows_this_frame(&core)
    };
    assert!(set.contains(&3), "abs row visible_start+3 → screen row 3");
    assert!(set.contains(&4), "abs row visible_start+4 → screen row 4");
    // Screen row 0 holds neither a selected row nor the cursor (which sits
    // at the viewport bottom after the newline fill), and the core was
    // cleared of dirty bits by record_render_state, so it is absent.
    assert!(!set.contains(&0), "unselected screen row 0 stays clean");
}
