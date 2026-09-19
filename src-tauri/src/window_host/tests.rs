use super::event_loop::request_redraw_on_user_event;
use super::frame_pacing::{
    RESIZE_SETTLE_MAX_DURATION, RESIZE_SETTLE_QUIET_DURATION, RESIZE_SETTLE_SELF_WAKE_INTERVAL,
    has_actionable_egui_input, next_resize_settle_wake_deadline, next_wait_deadline,
    preedit_effective_dirty_rows, record_drawn_frame, record_rebuilt_rows,
    resize_settle_self_wake_due, resolve_build_dirty_rows,
    should_rotate_row_cache_for_scroll_event, should_skip_frame, status_bar_insets_changed,
    toast_redraw_due,
};
use super::input_translate::{
    MAX_ALT_SCROLL_NOTCHES, MAX_WHEEL_REPORT_NOTCHES, ShiftEnterRewrite, WheelConsumer,
    accumulate_alt_scroll_lines, accumulate_wheel_report_lines, alternate_scroll_wheel_bytes,
    is_skk_swallowed_chord, shift_enter_rewrite, should_clear_selection_on_forward,
    should_drop_synthetic_key_event, wheel_consumer, wheel_report_notches,
    winit_button_to_report_identity, winit_key_to_egui,
};
use super::link_hover::{detect_osc8_link_at, hover_link_cells_changed};
use super::mouse_report::MouseButtonId;
use super::pointer_routing::bounded_wheel_report_duplicate;
use super::resize_layout::resolve_grid_bot_inset;
use super::*;
use crate::selection::SelectionMode;
use crate::settings::ShiftEnterBehavior;
use crate::ui::chrome::build_egui_fonts;
use std::time::Duration;
use winit::event::MouseButton;
use winit::keyboard::{Key as WinitKey, NamedKey};

// ── task0006 AC-2: grid x-origin carries no sidebar term ───────────

/// Regression guard for the right-edge placement update:
/// `cell_metrics_px`'s `origin_x` computation must not read the
/// persistent mux-sidebar inset — only `grid_size`'s usable-WIDTH
/// computation may. Scans each function's own source text so a future
/// edit that moves the sidebar term back onto `origin_x` fails loudly.
#[test]
fn cell_metrics_px_origin_x_has_no_sidebar_term() {
    let src = include_str!("resize_layout.rs");
    let start = src
        .find("fn cell_metrics_px(&self, app: &App)")
        .expect("marker `fn cell_metrics_px` not found in resize_layout.rs");
    let body = &src[start..];
    let end = body
        .find("\n    pub fn grid_size(")
        .expect("`cell_metrics_px` should be immediately followed by `grid_size`");
    let cell_metrics_px_src = &body[..end];
    // Target the specific inset code terms rather than the bare word
    // "sidebar" — the function's own explanatory comment legitimately
    // mentions the sidebar in prose (documenting why there is no term).
    for needle in [
        "sidebar_inset",
        "mux_sidebar_inset_logical",
        "mux_sidebar_grid_inset",
    ] {
        assert!(
            !cell_metrics_px_src.contains(needle),
            "cell_metrics_px's origin_x must contain no sidebar term \
             (AC-2): found `{needle}` — the grid x-origin must be \
             identical with and without the persistent sidebar; only \
             grid_size's usable-width computation may read the \
             sidebar inset"
        );
    }
}

// ── AC-2/TS4 (mux-status-bar-removal task0001, FR5/FR6): status-bar
// row count / inset driven only by general status-bar visibility, not
// by mux attach state. `refresh_status_bar_insets` feeds
// `panel_height_logical(&app.status_bar_view_model())` into
// `grid_size_for_bot_inset`'s `bot_inset_logical` argument — proving
// that height (and therefore the inset / grid-size CANDIDATE) is
// unaffected by mux attach is the direct input-level pin for the
// grid-height invariant. ─────────────────────────────────────────

/// Build a tab attached to a single-window mux session, mirroring
/// `tabs.rs`'s own `mux_tab_active_pane` test helper (duplicated here
/// rather than shared across modules — both are private `#[cfg(test)]`
/// helpers).
fn attach_active_tab_to_mux_session(app: &mut App) {
    use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};
    let windows = vec![WindowInfo {
        id: 1,
        name: "win".to_string(),
        active_pane_id: 10,
    }];
    let session = SessionInfo {
        id: 1,
        name: "main".to_string(),
        window_count: windows.len() as u32,
        pane_count: windows.len() as u32,
        active_window_index: 0,
        windows,
    };
    let welcome = MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![session],
        },
    );
    app.on_mux_message(0, welcome);
}

/// AC-2/TS4: with the general status bar showing content (App Line 1
/// non-empty), attaching the active tab to a mux session must not
/// change `panel_height_logical` — the exact value
/// `refresh_status_bar_insets` feeds as the grid-size candidate's
/// bottom inset.
#[test]
fn status_bar_panel_height_unchanged_by_mux_attach_state() {
    let mut app = App::new();
    app.spawn_initial_tab();
    assert!(
        app.active_tab().unwrap().mux_session_name.is_none(),
        "precondition: tab starts unattached"
    );
    let height_before = crate::ui::status_bar::panel_height_logical(&app.status_bar_view_model());

    attach_active_tab_to_mux_session(&mut app);
    assert!(
        app.active_tab().unwrap().mux_session_name.is_some(),
        "precondition: tab is now mux-attached"
    );
    let height_after = crate::ui::status_bar::panel_height_logical(&app.status_bar_view_model());

    assert_eq!(
        height_before, height_after,
        "status-bar panel height (-> bottom inset -> grid-size candidate) \
         must be identical with and without mux attach"
    );
}

/// AC-2/TS4 counterpart: the same invariant holds when the general
/// status bar has NO content at all (fully auto-hidden) — mux attach
/// must not be able to force a row to appear that general status-bar
/// state alone would keep hidden.
#[test]
fn status_bar_panel_height_stays_zero_when_no_general_content_regardless_of_mux() {
    let mut app = App::new();
    app.spawn_initial_tab();
    // `app.settings` is an `Arc<Settings>` — clone-and-flip to mutate,
    // mirroring the pattern `app.rs`'s test module uses for the same
    // purpose (its `with_setting` helper is private to that module).
    app.settings = std::sync::Arc::new({
        let mut s = (*app.settings).clone();
        s.statusbar.app_line1_left.clear();
        s.statusbar.app_line1_right.clear();
        s.statusbar.app_line2_left.clear();
        s.statusbar.app_line2_right.clear();
        s
    });

    let height_before = crate::ui::status_bar::panel_height_logical(&app.status_bar_view_model());
    assert_eq!(height_before, 0.0, "precondition: no visible rows yet");

    attach_active_tab_to_mux_session(&mut app);
    let height_after = crate::ui::status_bar::panel_height_logical(&app.status_bar_view_model());
    assert_eq!(
        height_after, 0.0,
        "mux attach alone must not surface a status-bar row"
    );
}

// ── ResizeSettler (mux-tab-switch-replay-latency task0002, FR6;
// wall-clock quiescence redesign task0005) ─────────────────────────

/// AC-1 (task0005 findings 12cac263b7dab24b / 02546e5e10deb500-c):
/// reproduces the REAL render-driven call shape — `refresh_status_bar_
/// insets` calls `observe` at the head of EVERY render, so the same
/// candidate repeats across several consecutive calls before the
/// underlying `visible_row_count` actually transitions again (A,A,A,
/// B,B,B,A,A,A,... — not a fresh distinct value on every single call,
/// which is the one regime the pre-task0005 call-count debounce
/// happened to handle correctly). `a` is additionally seeded as
/// [`ResizeSettler::last_forwarded`] — the value already applied
/// before the storm began — reproducing 02546e5e10deb500-c's bias
/// concern: the caller must feed `observe` unconditionally (both `a`
/// and `b`), not just whichever side differs from applied. Each hold
/// lasts 3 renders (~12 ms of simulated wall-clock time at a 4 ms
/// render interval), comfortably less than
/// [`RESIZE_SETTLE_QUIET_DURATION`] (64 ms), so no transient hold
/// should ever be mistaken for settled.
#[test]
fn resize_settler_forwards_at_most_once_for_a_render_driven_storm_matching_applied() {
    let a = (120, 40); // stand-in for visible_row_count == 0; == applied
    let b = (120, 39); // stand-in for visible_row_count == 1
    let mut settler = ResizeSettler {
        window_opened_at: Some(Instant::now()),
        candidate: None,
        stable_since: None,
        last_forwarded: Some(a),
    };

    let base = Instant::now();
    let render_interval = Duration::from_millis(4);
    let mut stream = Vec::new();
    for t in 0..24u32 {
        let state = if t % 2 == 0 { a } else { b };
        for _ in 0..3 {
            stream.push(state);
        }
    }

    let mut forwarded = Vec::new();
    for (i, candidate) in stream.iter().enumerate() {
        let now = base + render_interval * i as u32;
        if let Some(size) = settler.observe(*candidate, now) {
            forwarded.push(size);
        }
    }
    assert!(
        forwarded.is_empty(),
        "AC-1: no transient transition during the render-driven storm \
         should reach Tab::resize, even though one oscillating state \
         matches the already-applied value; got {forwarded:?}"
    );

    // The storm stops changing and settles on `b`: keep feeding `b`
    // until enough simulated wall-clock time has passed to cross
    // RESIZE_SETTLE_QUIET_DURATION — bounded by RESIZE_SETTLE_MAX_
    // DURATION as a test-safety net against an infinite loop bug.
    let mut t = base + render_interval * stream.len() as u32;
    let settle_start = t;
    let result = loop {
        let result = settler.observe(b, t);
        if result.is_some() {
            break result;
        }
        assert!(
            t.duration_since(settle_start) < RESIZE_SETTLE_MAX_DURATION,
            "settle on `b` never completed within the backstop"
        );
        t += render_interval;
    };
    assert_eq!(
        result,
        Some(b),
        "AC-2: the settled size must still reach Tab::resize once \
         settling is confirmed, so no daemon-side pane is left at a \
         stale size"
    );
}

/// task0005 finding 02546e5e10deb500-c: when a storm settles back on
/// the value already applied, forwarding it would be a wasted no-op
/// resize — `observe` must return `None` (nothing new for
/// `Tab::resize`) while still closing the settling window, so a later
/// GENUINE change forwards immediately per AC-3 rather than waiting
/// through another debounce.
#[test]
fn resize_settler_settling_back_on_the_applied_value_is_a_no_op_but_closes_the_window() {
    let applied = (120, 40);
    let mut settler = ResizeSettler {
        window_opened_at: Some(Instant::now()),
        candidate: None,
        stable_since: None,
        last_forwarded: Some(applied),
    };
    let base = Instant::now();
    let mut t = base;
    let result = loop {
        let result = settler.observe(applied, t);
        if !settler.awaiting_decision() {
            break result;
        }
        assert!(
            t.duration_since(base) < RESIZE_SETTLE_MAX_DURATION,
            "settling on the already-applied value never closed the window"
        );
        t += RESIZE_SETTLE_QUIET_DURATION / 4;
    };
    assert_eq!(
        result, None,
        "settling back on the already-applied value must not forward"
    );

    // The window is now closed; a genuinely new candidate must still
    // forward immediately (AC-3), proving the no-op above did not
    // leave the settler stuck.
    let changed = (100, 30);
    assert_eq!(settler.observe(changed, t), Some(changed));
}

/// AC-1 (robustness beyond simple 2-state alternation): chaotic churn
/// across three distinct sizes never settles until it stops changing
/// entirely, regardless of the period. Each call advances simulated
/// time by 1 ms, so the whole 30-call churn (30 ms) stays well under
/// both `RESIZE_SETTLE_QUIET_DURATION` and `RESIZE_SETTLE_MAX_DURATION`.
#[test]
fn resize_settler_absorbs_chaotic_multi_value_churn() {
    let mut settler = ResizeSettler::new();
    let states = [(100, 30), (101, 30), (100, 31)];
    let base = Instant::now();
    let mut forwarded = Vec::new();
    for i in 0..30u32 {
        let candidate = states[(i % 3) as usize];
        let now = base + Duration::from_millis(i as u64);
        if let Some(size) = settler.observe(candidate, now) {
            forwarded.push(size);
        }
    }
    assert!(
        forwarded.is_empty(),
        "chaotic multi-value churn must not reach Tab::resize \
         mid-storm; got {forwarded:?}"
    );
}

/// AC-3: once the settling window has closed, an ordinary, isolated
/// resize reaches the caller on its very first observation — identical
/// to the pre-fix, undebounced behavior, so a real post-startup resize
/// (well after settling) is not perceptibly delayed.
#[test]
fn resize_settler_forwards_immediately_once_closed() {
    let mut settler = ResizeSettler::new();
    let mut t = Instant::now();
    // Close the window with a clean, non-oscillating settle: hold the
    // same candidate past RESIZE_SETTLE_QUIET_DURATION.
    let settled = (120, 40);
    assert_eq!(settler.observe(settled, t), None);
    t += RESIZE_SETTLE_QUIET_DURATION;
    assert_eq!(settler.observe(settled, t), Some(settled));

    // An ordinary later resize (a single, isolated new candidate) must
    // forward on its very next observation, with no further debounce.
    let resized = (100, 30);
    assert_eq!(
        settler.observe(resized, t),
        Some(resized),
        "AC-3: once closed, a genuine new candidate must not be \
         delayed by the settling debounce"
    );
}

/// task0005 finding 02546e5e10deb500-c: once closed, a render that
/// merely repeats the size already applied must never re-forward it —
/// this is what lets `refresh_status_bar_insets` feed `observe`
/// unconditionally on every render without retriggering
/// `Tab::resize`'s broadcast every single frame in steady state.
#[test]
fn resize_settler_closed_mode_does_not_reforward_the_same_value_every_render() {
    let mut settler = ResizeSettler::new();
    let mut t = Instant::now();
    let settled = (120, 40);
    settler.observe(settled, t);
    t += RESIZE_SETTLE_QUIET_DURATION;
    assert_eq!(settler.observe(settled, t), Some(settled));

    for _ in 0..10 {
        t += Duration::from_millis(16);
        assert_eq!(settler.observe(settled, t), None);
    }
}

/// AC-2 (pathological backstop case): a candidate that changes on
/// literally every observation (never repeating, so it never becomes
/// wall-clock stable) must still be forwarded once
/// `RESIZE_SETTLE_MAX_DURATION` of simulated time has elapsed since the
/// window opened — no daemon-side pane is left at a stale size
/// forever, even if a storm never quiesces.
#[test]
fn resize_settler_backstop_forwards_after_max_duration_even_if_never_settled() {
    let mut settler = ResizeSettler::new();
    let base = Instant::now();
    let step = Duration::from_millis(5);
    let mut i = 0u32;
    let forwarded = loop {
        let candidate = (100 + i as u16, 40); // strictly distinct every time
        let now = base + step * i;
        let result = settler.observe(candidate, now);
        if result.is_some() {
            break result;
        }
        i += 1;
        assert!(
            now.duration_since(base) < RESIZE_SETTLE_MAX_DURATION * 2,
            "the backstop must force a forward within roughly \
             RESIZE_SETTLE_MAX_DURATION even for a never-settling stream"
        );
    };
    assert!(
        forwarded.is_some(),
        "the backstop must force a forward within \
         RESIZE_SETTLE_MAX_DURATION even for a never-settling stream"
    );
}

/// A closed settler reopens its settling window on `reset` — mirrors
/// the mux reattach signal `WindowHost::refresh_status_bar_insets`
/// uses (a fresh `mux_session_name` transitioning from absent to
/// present), so a settling storm right after a mid-session reattach is
/// absorbed exactly as at construction, not forwarded immediately.
#[test]
fn resize_settler_reset_reopens_a_closed_window() {
    let mut settler = ResizeSettler::new();
    let mut t = Instant::now();
    let settled = (120, 40);
    settler.observe(settled, t);
    t += RESIZE_SETTLE_QUIET_DURATION;
    assert_eq!(settler.observe(settled, t), Some(settled)); // closes the window

    settler.reset();

    t += Duration::from_millis(1);
    assert_eq!(settler.observe((10, 10), t), None);
    t += Duration::from_millis(1);
    assert_eq!(settler.observe((20, 20), t), None);
}

/// AC-5 (task0005 findings 02546e5e10deb500 / 5b1878c41d3e02d6-perf-P2):
/// simulates a fully idle window where the ONLY thing driving further
/// observations is `ResizeSettler::awaiting_decision` — mirroring
/// `WindowHost::refresh_status_bar_insets`'s `request_redraw` call —
/// with no ime/pty/search/blink/bell/toast activity ever feeding this
/// loop. A pending candidate must still resolve within a bounded
/// amount of self-driven, simulated wall-clock time.
#[test]
fn resize_settler_self_drives_to_settlement_without_any_external_wake() {
    let mut settler = ResizeSettler::new();
    let candidate = (100, 30);
    let base = Instant::now();
    let step = Duration::from_millis(1);
    let mut t = base;
    let result = loop {
        let result = settler.observe(candidate, t);
        if result.is_some() {
            break result;
        }
        assert!(
            settler.awaiting_decision(),
            "AC-5: while a candidate is still pending, the settler \
             must report `awaiting_decision() == true` so the call \
             site knows to request another redraw itself"
        );
        assert!(
            t.duration_since(base) < RESIZE_SETTLE_MAX_DURATION * 2,
            "AC-5: settling must complete within a bounded amount of \
             simulated wall-clock time even with no external event \
             ever driving a redraw"
        );
        t += step;
    };
    assert_eq!(result, Some(candidate));
    assert!(
        !settler.awaiting_decision(),
        "once settled, no further self-driven redraw should be requested"
    );
}

// ── task0005 round-1 rework: `resolve_grid_bot_inset` — grid
// computation consumes only settler-forwarded inset values (findings
// `0029db1c89ab226f` / `5b2f22c5a14f7364`) ──────────────────────────

/// task0005 AC-1: reproduces the traced mux-attach/reattach firing
/// order — `refresh_status_bar_insets` resets the settler and writes
/// the transient inset, then a sidebar-driven `pending_resize` (a
/// source entirely independent of the settler, mirroring
/// `refresh_mux_sidebar_inset`) fires, then `apply_pending_resize`
/// would compute the grid size — and proves that size is NOT derived
/// from the transient, not-yet-settled inset: it equals the
/// settler's last-forwarded size. Against the pre-task0005 code
/// (`grid_size()` reading `status_bar_bot_inset_logical` directly,
/// with no settled/transient split at all) this scenario computed
/// `size_for(transient_bot) = (120, 0)` instead of `(120, 40)` — the
/// exact defect these findings report.
#[test]
fn grid_bot_inset_ignores_a_transient_write_during_a_freshly_reopened_settle() {
    // Stand-in for `WindowHost::grid_size_for_bot_inset`: a grid size
    // is simply a pure, monotonic function of the bot inset — the
    // actual geometry is untouched by this task and already covered
    // elsewhere; only WHICH inset value reaches it is under test.
    fn size_for(bot_inset_logical: f32) -> (u16, u16) {
        (120, 40 - bot_inset_logical as u16)
    }

    let applied_bot = 0.0_f32; // no status bar, before mux attach
    let mut settled_bot = applied_bot;
    let mut settler = ResizeSettler {
        window_opened_at: None, // closed: already settled pre-attach
        candidate: None,
        stable_since: None,
        last_forwarded: Some(size_for(applied_bot)),
    };

    // 1. Mux attach: `refresh_status_bar_insets` resets the settler...
    settler.reset();
    // ...and writes the transient inset immediately (Change 2, D-D) —
    // the status bar's first-frame height, not yet judged stable.
    let transient_bot = 40.0_f32;
    let candidate = size_for(transient_bot);
    let now = Instant::now();
    let forwarded = settler.observe(candidate, now);
    assert_eq!(
        forwarded, None,
        "precondition: a freshly reopened window has not settled yet"
    );
    settled_bot = resolve_grid_bot_inset(settled_bot, transient_bot, settler.awaiting_decision());
    assert_eq!(
        settled_bot, applied_bot,
        "the settled inset must not move while the settling window is open"
    );

    // 2. A sidebar-driven `pending_resize` fires — independent of the
    //    settler entirely (mirrors `refresh_mux_sidebar_inset`).
    let pending_resize = true;

    // 3. `apply_pending_resize` would compute the grid size from the
    //    settled inset, not the transient one.
    assert!(pending_resize);
    let applied_size = size_for(settled_bot);
    assert_eq!(
        applied_size,
        size_for(applied_bot),
        "AC-1: the size apply_pending_resize would broadcast must not \
         be derived from the transient inset ({transient_bot}); it \
         must equal the settler's last-forwarded size"
    );
    assert_ne!(
        applied_size, candidate,
        "sanity: the transient candidate really would have differed"
    );
}

/// task0005 AC-2: a compositor-sourced `pending_resize` (`Resized` /
/// `ScaleFactorChanged`) arriving while the settling window is still
/// open must also compute its grid size from the settled inset, not
/// the transient one — the fix is not scoped to the mux-sidebar
/// trigger alone. The window WIDTH component legitimately reflects
/// the real compositor resize (unrelated to the inset); only the
/// bot-inset-derived ROWS component must stay pinned to the settled
/// value while the settler has not yet judged it stable.
#[test]
fn grid_bot_inset_ignores_a_transient_write_when_a_compositor_resize_triggers_apply() {
    fn size_for(window_width: u16, bot_inset_logical: f32) -> (u16, u16) {
        (window_width / 6, 50 - bot_inset_logical as u16)
    }

    let applied_bot = 0.0_f32;
    let old_width = 800u16;
    let mut settled_bot = applied_bot;
    let base = Instant::now();
    let mut settler = ResizeSettler {
        window_opened_at: Some(base), // mid-settle (e.g. just reopened by attach)
        candidate: Some(size_for(old_width, applied_bot)),
        stable_since: Some(base),
        last_forwarded: Some(size_for(old_width, applied_bot)),
    };

    // The status-bar height changes mid-settle (transient, immediate
    // per D-D)...
    let transient_bot = 8.0_f32;
    let now = base + Duration::from_millis(1);
    let forwarded = settler.observe(size_for(old_width, transient_bot), now);
    assert_eq!(
        forwarded, None,
        "precondition: still mid-settle, not stable long enough yet"
    );
    settled_bot = resolve_grid_bot_inset(settled_bot, transient_bot, settler.awaiting_decision());

    // ...and, independently, a compositor `Resized` event changes the
    // window width and sets `pending_resize` directly — it never
    // touches the settler at all.
    let new_width = 1000u16;
    let pending_resize = true;
    assert!(pending_resize);

    let applied_size = size_for(new_width, settled_bot);
    assert_eq!(
        applied_size,
        size_for(new_width, applied_bot),
        "AC-2: the compositor-triggered apply must use the settled \
         bot inset, not the transient write ({transient_bot}), even \
         though the window WIDTH component legitimately reflects the \
         new size"
    );
}

/// task0005 AC-3: across a sequence of settle-then-apply cycles, the
/// settled inset this task introduces must reproduce exactly the
/// size `ResizeSettler` recorded as `last_forwarded` — no divergence
/// between what the settler believes it forwarded and what would
/// actually be applied/broadcast.
#[test]
fn grid_bot_inset_settled_value_reproduces_the_settlers_last_forwarded_size() {
    fn size_for(bot_inset_logical: f32) -> (u16, u16) {
        (120, 50 - bot_inset_logical as u16)
    }

    let mut settler = ResizeSettler::new();
    let mut settled_bot = 0.0_f32;
    let mut t = Instant::now();

    for &bot in &[0.0_f32, 12.0, 24.0] {
        let mut iterations = 0;
        loop {
            let candidate = size_for(bot);
            let forwarded = settler.observe(candidate, t);
            settled_bot = resolve_grid_bot_inset(settled_bot, bot, settler.awaiting_decision());
            t += Duration::from_millis(4);
            if let Some(size) = forwarded {
                assert_eq!(
                    size_for(settled_bot),
                    size,
                    "AC-3: the size derived from the settled inset must \
                     equal exactly what the settler recorded as \
                     last_forwarded"
                );
                assert_eq!(settler.last_forwarded, Some(size));
                break;
            }
            iterations += 1;
            assert!(iterations < 1000, "settle loop should have converged");
        }
    }
}

/// task0005 AC-4 (FR4 non-regression): a status-bar height change
/// whose derived grid-size candidate does not move (cell-height
/// rounding / row clamping) must not set `pending_resize` — unchanged
/// from before this task — and the settled-inset tracking this task
/// introduces must still advance to the new value once the settler is
/// not withholding judgment, so a LATER genuine resize is computed
/// from the current inset rather than one left stale by the no-op
/// change.
#[test]
fn grid_bot_inset_tracks_transient_when_settler_closed_even_on_a_noop_candidate() {
    fn size_for(bot_inset_logical: f32) -> (u16, u16) {
        // Deliberately coarse: several nearby inset values floor to
        // the same row count, mirroring the real cell-height
        // rounding / row-clamping AC-4 describes.
        (120, 50 - (bot_inset_logical / 10.0).floor() as u16)
    }

    let mut settler = ResizeSettler::new();
    let mut settled_bot = 0.0_f32;
    let mut t = Instant::now();
    // Settle on bot = 2.0 first.
    loop {
        let forwarded = settler.observe(size_for(2.0), t);
        settled_bot = resolve_grid_bot_inset(settled_bot, 2.0, settler.awaiting_decision());
        t += Duration::from_millis(4);
        if forwarded.is_some() {
            break;
        }
    }
    assert_eq!(settled_bot, 2.0);

    // A small height change to bot = 4.0: same candidate (still
    // floors to the same row count), settler is closed, so `observe`
    // reports no genuine change...
    let candidate = size_for(4.0);
    assert_eq!(
        candidate,
        size_for(2.0),
        "precondition: candidate unchanged"
    );
    let forwarded = settler.observe(candidate, t);
    assert_eq!(
        forwarded, None,
        "AC-4: an unchanged derived candidate must not set pending_resize"
    );
    settled_bot = resolve_grid_bot_inset(settled_bot, 4.0, settler.awaiting_decision());
    assert_eq!(
        settled_bot, 4.0,
        "AC-4: the settled inset must still track the current value \
         with no stale lock-in, even though the derived candidate did \
         not move"
    );
}

/// task0005 AC-5 (no stale lock-in): once the settler has forwarded
/// and closed, the settled inset this task introduces keeps pace with
/// every further genuine change on an otherwise idle window — it is
/// never pinned to the value at the moment of the first forward.
#[test]
fn grid_bot_inset_keeps_pace_with_further_genuine_changes_after_closing() {
    fn size_for(bot_inset_logical: f32) -> (u16, u16) {
        (120, 50 - bot_inset_logical as u16)
    }
    let mut settler = ResizeSettler::new();
    let mut settled_bot = 0.0_f32;
    let mut t = Instant::now();
    loop {
        let forwarded = settler.observe(size_for(0.0), t);
        settled_bot = resolve_grid_bot_inset(settled_bot, 0.0, settler.awaiting_decision());
        t += Duration::from_millis(4);
        if forwarded.is_some() {
            break;
        }
    }
    assert_eq!(settled_bot, 0.0);

    // Idle window, later: each genuine status-bar height change must
    // forward and settle immediately (closed-mode behavior, unrelated
    // to this task), and the settled inset must follow every one.
    for &bot in &[6.0_f32, 14.0, 3.0] {
        let forwarded = settler.observe(size_for(bot), t);
        assert_eq!(
            forwarded,
            Some(size_for(bot)),
            "no stale lock-in: a genuine post-settle change must \
             forward immediately"
        );
        settled_bot = resolve_grid_bot_inset(settled_bot, bot, settler.awaiting_decision());
        assert_eq!(
            settled_bot, bot,
            "settled inset must track each new genuine value"
        );
        t += Duration::from_millis(4);
    }
}

// ── mux-tab-switch-bypass-refix task0002 Change 1: rate-limited
// resize-settle self-wake (finding 81507f39e384b34e) ────────────────

/// AC-1: not awaiting a decision at all → never wake, regardless of
/// `last_self_wake`.
#[test]
fn resize_settle_self_wake_due_false_when_not_awaiting() {
    let now = Instant::now();
    assert!(!resize_settle_self_wake_due(false, None, now));
    assert!(!resize_settle_self_wake_due(false, Some(now), now));
}

/// AC-1: awaiting a decision with no prior self-wake in this window
/// (`None`) → the first wake fires immediately.
#[test]
fn resize_settle_self_wake_due_true_on_first_wake() {
    assert!(resize_settle_self_wake_due(true, None, Instant::now()));
}

/// AC-1/AC-3: awaiting a decision, but less than
/// `RESIZE_SETTLE_SELF_WAKE_INTERVAL` has elapsed since the last
/// self-wake-driven request → no further wake yet (the rate limit).
#[test]
fn resize_settle_self_wake_due_false_within_interval() {
    let now = Instant::now();
    let last = now - (RESIZE_SETTLE_SELF_WAKE_INTERVAL / 2);
    assert!(!resize_settle_self_wake_due(true, Some(last), now));
}

/// AC-1/AC-3: once `RESIZE_SETTLE_SELF_WAKE_INTERVAL` has elapsed since
/// the last self-wake-driven request, the next one fires — this is
/// what bounds the wake rate to a modest cadence (NFR2) instead of the
/// unconditional per-frame request that used to spin the render loop.
#[test]
fn resize_settle_self_wake_due_true_after_interval() {
    let now = Instant::now();
    let last = now - RESIZE_SETTLE_SELF_WAKE_INTERVAL;
    assert!(resize_settle_self_wake_due(true, Some(last), now));
}

/// Supporting `next_resize_settle_wake_deadline` (feeds
/// `control_flow_for`'s `WaitUntil` so `about_to_wait` re-enters at the
/// self-wake cadence even with zero external activity): closed window
/// → no deadline to arm.
#[test]
fn next_resize_settle_wake_deadline_none_when_not_awaiting() {
    assert_eq!(
        next_resize_settle_wake_deadline(false, Instant::now()),
        None
    );
}

/// Open window → the deadline is exactly one self-wake interval out
/// from `now`, regardless of `last_self_wake` (mirrors
/// `App::next_toast_deadline`, which similarly ignores its own gate's
/// last-fired timestamp).
#[test]
fn next_resize_settle_wake_deadline_some_one_interval_out_when_awaiting() {
    let now = Instant::now();
    assert_eq!(
        next_resize_settle_wake_deadline(true, now),
        Some(now + RESIZE_SETTLE_SELF_WAKE_INTERVAL)
    );
}

/// AC-2 (mirrors `resize_settler_self_drives_to_settlement_without_
/// any_external_wake`): simulates a fully idle window (no ime/pty/
/// search/blink/bell/toast activity) where `ResizeSettler::observe` is
/// driven ONLY at the rate-limited self-wake cadence — i.e. the exact
/// call pattern `resize_settle_self_wake_due` now permits in
/// production — rather than on every simulated millisecond. The
/// settler must still reach its decision within
/// `RESIZE_SETTLE_MAX_DURATION`, proving the rate limit does not
/// starve quiescence detection (regression guard: findings
/// 02546e5e10deb500 / 5b1878c41d3e02d6-perf-P2 must not return).
#[test]
fn resize_settle_self_wake_drives_settler_to_settlement_at_the_rate_limited_cadence() {
    let mut settler = ResizeSettler::new();
    let candidate = (100, 30);
    let base = Instant::now();
    let mut t = base;
    let mut last_self_wake: Option<Instant> = None;
    let result = loop {
        let result = settler.observe(candidate, t);
        if result.is_some() {
            break result;
        }
        let awaiting = settler.awaiting_decision();
        assert!(
            awaiting,
            "AC-2: while a candidate is still pending, the settler \
             must report awaiting_decision() == true"
        );
        assert!(
            resize_settle_self_wake_due(awaiting, last_self_wake, t),
            "AC-2: the self-wake predicate must keep permitting a wake \
             at each rate-limited tick, or the loop below would spin \
             forever without ever calling `observe` again"
        );
        last_self_wake = Some(t);
        assert!(
            t.duration_since(base) < RESIZE_SETTLE_MAX_DURATION * 2,
            "AC-2: settling must complete within a bounded amount of \
             simulated wall-clock time even when observed only at the \
             rate-limited self-wake cadence"
        );
        t += RESIZE_SETTLE_SELF_WAKE_INTERVAL;
    };
    assert_eq!(result, Some(candidate));
    assert!(!settler.awaiting_decision());
}

// ── mux-tab-switch-bypass-refix task0002 Change 2: settler-independent
// inset application (findings a82206113b8160fd / aba5ebbdf9a9addb)
// ────────────────────────────────────────────────────────────────

/// AC-5: identical current/candidate insets → unchanged (no-op).
#[test]
fn status_bar_insets_changed_false_when_identical() {
    assert!(!status_bar_insets_changed(0.0, 40.0, 0.0, 40.0));
}

/// AC-4: the bottom inset alone differing (the status-bar-height-
/// change case whose derived grid-size candidate does not move) must
/// still be reported as changed — this is the defect-(a) fix: the new
/// inset value must apply even when `ResizeSettler` never forwards a
/// grid-size decision for it.
#[test]
fn status_bar_insets_changed_true_when_bot_inset_differs() {
    assert!(status_bar_insets_changed(0.0, 40.0, 0.0, 44.0));
}

/// The top inset differing alone must also be reported as changed
/// (symmetric with the bottom inset check).
#[test]
fn status_bar_insets_changed_true_when_top_inset_differs() {
    assert!(status_bar_insets_changed(0.0, 40.0, 2.0, 40.0));
}

/// AC-1/AC-2 (task0006, finding `869ddd643c123a44`): the smallest
/// possible non-zero perturbation — one bit-step away from the stored
/// value — is still representable and must be reported as "changed".
/// This replaces the former `..._false_within_epsilon` case, whose
/// `40.0 + f32::EPSILON / 2.0` perturbation rounded back to exactly
/// `40.0` in f32 and therefore pinned nothing (it duplicated
/// `..._false_when_identical`). The `assert_ne!` on the raw bits
/// proves this perturbation is real before the predicate is even
/// called, guarding against a repeat of that defect.
#[test]
fn status_bar_insets_changed_true_for_minimal_representable_difference() {
    let current_bot = 0.0_f32;
    let candidate_bot = f32::from_bits(current_bot.to_bits() + 1);
    assert_ne!(
        current_bot.to_bits(),
        candidate_bot.to_bits(),
        "perturbation must be a real, bit-distinct value or this test \
         proves nothing about the predicate"
    );
    assert!(status_bar_insets_changed(
        0.0,
        current_bot,
        0.0,
        candidate_bot
    ));
}

/// AC-3: the same pin at a magnitude representative of a real
/// status-bar inset (tens of logical px) — the exact site of the
/// former vacuous test. Even here, where the retired epsilon
/// threshold was already smaller than one ULP and thus unreachable,
/// the smallest representable step away from the stored value is
/// reported as "changed".
#[test]
fn status_bar_insets_changed_true_for_minimal_step_at_real_inset_magnitude() {
    let current_top = 40.0_f32;
    let candidate_top = f32::from_bits(current_top.to_bits() + 1);
    assert_ne!(current_top.to_bits(), candidate_top.to_bits());
    assert!(status_bar_insets_changed(
        current_top,
        0.0,
        candidate_top,
        0.0
    ));
}

// ── task0002 AC-5: should_skip_frame pure decision ───────────────

/// AC-5: `Some(0)` dirty AND status bar unchanged AND no overlay work
/// AND no pending egui input → skip.
#[test]
fn should_skip_frame_when_no_dirty_rows_and_status_bar_unchanged() {
    assert!(should_skip_frame(Some(0), false, false, false));
}

/// AC-5: dirty rows present (even with an unchanged status bar and no
/// overlay work) → never skip.
#[test]
fn should_skip_frame_false_when_dirty_rows_present() {
    assert!(!should_skip_frame(Some(3), false, false, false));
}

/// AC-5: status bar changed (even with zero dirty rows and no overlay
/// work) → never skip — this is the carve-out that keeps the clock /
/// git-branch / OSC 777 wake chain alive on an otherwise-idle shell.
#[test]
fn should_skip_frame_false_when_status_bar_changed() {
    assert!(!should_skip_frame(Some(0), true, false, false));
}

/// AC-5: no active tab (`None`) → never skip; the hint-message frame
/// must still draw.
#[test]
fn should_skip_frame_false_when_no_active_tab() {
    assert!(!should_skip_frame(None, false, false, false));
}

/// Overlay work pending (a toast counting down or a visual-bell flash
/// still decaying), even with zero dirty rows and an unchanged status
/// bar, must never skip — otherwise the 60 Hz wake `about_to_wait`
/// schedules while a toast/bell is active spins uselessly without the
/// egui pass ever running `pump_sftp` / the toast prune / the bell
/// paint.
#[test]
fn should_skip_frame_false_when_overlay_work_pending() {
    assert!(!should_skip_frame(Some(0), false, true, false));
}

/// task0005 AC-2: the search UI being visible must also veto the skip,
/// exercised through the same `overlay_work` parameter as the toast /
/// bell carve-out above (the call site ORs `App::search_visible()` into
/// it).
#[test]
fn should_skip_frame_false_when_search_visible() {
    assert!(!should_skip_frame(Some(0), false, true, false));
}

// ── toast_redraw_due pure decision ──────────────────────────────────

/// No active toast → no toast-driven redraw, regardless of when the
/// last one fired.
#[test]
fn toast_redraw_due_false_when_no_toast() {
    let now = Instant::now() + Duration::from_secs(10);
    assert!(!toast_redraw_due(false, None, now));
    assert!(!toast_redraw_due(false, Some(now), now));
}

/// First request for a freshly armed toast fires immediately (no
/// previous toast-driven redraw recorded).
#[test]
fn toast_redraw_due_true_on_first_request() {
    assert!(toast_redraw_due(true, None, Instant::now()));
}

/// Within the poll interval of the previous toast-driven redraw the
/// request is suppressed — this is what keeps the redraw →
/// `about_to_wait` cycle from spinning at full speed while a toast is
/// up (the egui pass would otherwise consume the toast's lifetime at
/// frame-rate speed; with the old `time: None` frame-counter clock
/// that dismissed a 4 s toast almost instantly).
#[test]
fn toast_redraw_due_false_within_poll_interval() {
    let now = Instant::now() + Duration::from_secs(10);
    let last = now - Duration::from_millis(crate::app::TOAST_POLL_MS / 2);
    assert!(!toast_redraw_due(true, Some(last), now));
}

/// Once the poll interval has elapsed the next request fires, keeping
/// the toast's prune cadence at ~`TOAST_POLL_MS`.
#[test]
fn toast_redraw_due_true_after_poll_interval() {
    let now = Instant::now() + Duration::from_secs(10);
    let last = now - Duration::from_millis(crate::app::TOAST_POLL_MS);
    assert!(toast_redraw_due(true, Some(last), now));
}

// ── has_actionable_egui_input pure decision ─────────────────────────

/// A `PointerMoved`-only queue (plain mouse-move hover over the
/// terminal body, no button held) must NOT be actionable — this is
/// the fix that lets an idle terminal skip the frame while the mouse
/// hovers over it.
#[test]
fn has_actionable_egui_input_false_for_pointer_moved_only() {
    let events = vec![egui::Event::PointerMoved(egui::pos2(1.0, 2.0))];
    assert!(!has_actionable_egui_input(&events, false));
}

/// A queue containing a `PointerButton` (the discrete event a click
/// delivers after its leading `PointerMoved`) must be actionable, so
/// click latency is unaffected by the `PointerMoved` exclusion above.
#[test]
fn has_actionable_egui_input_true_with_pointer_button() {
    let events = vec![
        egui::Event::PointerMoved(egui::pos2(1.0, 2.0)),
        egui::Event::PointerButton {
            pos: egui::pos2(1.0, 2.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ];
    assert!(has_actionable_egui_input(&events, false));
}

/// An empty queue (no egui input arrived this frame) must not be
/// actionable — even mid-drag (a held button with no new motion needs
/// no frame).
#[test]
fn has_actionable_egui_input_false_when_empty() {
    assert!(!has_actionable_egui_input(&[], false));
    assert!(!has_actionable_egui_input(&[], true));
}

/// While a pointer button is held, motion alone IS actionable: egui
/// chrome drags (scrollbar thumb, tab reorder) are driven purely by
/// the press→release motion stream, and skipping those frames would
/// freeze the drag's live tracking over an idle grid.
#[test]
fn has_actionable_egui_input_true_for_motion_while_button_held() {
    let events = vec![egui::Event::PointerMoved(egui::pos2(1.0, 2.0))];
    assert!(has_actionable_egui_input(&events, true));
}

/// Post-merge regression fix: undrained egui input (a tab-bar click,
/// wheel over the chrome, a search-box key) must veto the skip even on
/// a fully idle grid — `build_raw_input` is the only drain and runs
/// after this decision, so skipping would park the click until the
/// next unrelated wakeup (worst case a blink flip, ~530 ms of
/// perceived tab-switch lag).
#[test]
fn should_skip_frame_false_when_egui_input_pending() {
    assert!(!should_skip_frame(Some(0), false, false, true));
}

// ── post-merge regression fix: resolve_build_dirty_rows ─────────────

/// A full redraw raised mid-frame (tab switch / scrollbar jump applied
/// from this frame's egui pass) invalidates the frame-top snapshot:
/// the build must widen to every row (`None` routes both build
/// branches to their existing full-rebuild path).
#[test]
fn resolve_build_dirty_rows_widens_to_full_when_flag_pending() {
    assert_eq!(resolve_build_dirty_rows(Some(vec![3, 7]), true), None);
}

/// No mid-frame invalidation → the snapshot is trusted as-is (the
/// ordinary cached path keeps its dirty-rows-only rebuild).
#[test]
fn resolve_build_dirty_rows_keeps_snapshot_when_no_flag() {
    assert_eq!(
        resolve_build_dirty_rows(Some(vec![3, 7]), false),
        Some(vec![3, 7])
    );
}

/// An absent snapshot (forced full redraw path, `was_surface_dirty`)
/// stays absent regardless of the flag.
#[test]
fn resolve_build_dirty_rows_none_snapshot_stays_none() {
    assert_eq!(resolve_build_dirty_rows(None, false), None);
    assert_eq!(resolve_build_dirty_rows(None, true), None);
}

// ── task0006: should_rotate_row_cache_for_scroll_event pure decision ──

/// A pending scroll event on the ordinary cached path (dirty rows
/// captured this turn) must rotate the cache.
#[test]
fn should_rotate_row_cache_for_scroll_event_true_on_cached_path() {
    assert!(should_rotate_row_cache_for_scroll_event(1, true));
}

/// A turn whose effective dirty set is already every row (forced full
/// redraw, fold layout, or a scrolled-back viewport reacting to new
/// output) must NOT rotate — every row rebuilds from scratch
/// regardless, so rotating first would just be overwritten (task0006
/// Design: "needs_full_redraw frames: full rebuild already; just
/// clear the event").
#[test]
fn should_rotate_row_cache_for_scroll_event_false_on_full_redraw() {
    assert!(!should_rotate_row_cache_for_scroll_event(1, false));
}

/// No pending scroll event (`scroll_count == 0`) never rotates, even
/// on the cached path.
#[test]
fn should_rotate_row_cache_for_scroll_event_false_when_no_event() {
    assert!(!should_rotate_row_cache_for_scroll_event(0, true));
}

/// Neither a pending event nor the cached path → false (defensive
/// combination; never actually reached since the call site only
/// calls this inside `scroll_count > 0`).
#[test]
fn should_rotate_row_cache_for_scroll_event_false_when_neither() {
    assert!(!should_rotate_row_cache_for_scroll_event(0, false));
}

// ── task0005 AC-1: hover_link_cells_changed pure decision ─────────

/// AC-1: a link span appearing (empty → non-empty) counts as a change.
#[test]
fn hover_link_cells_changed_true_on_appear() {
    assert!(hover_link_cells_changed(&[], &[(3, 5, 9)]));
}

/// AC-1: a link span moving (different cell range) counts as a change.
#[test]
fn hover_link_cells_changed_true_on_move() {
    assert!(hover_link_cells_changed(&[(3, 5, 9)], &[(3, 10, 14)]));
}

/// AC-1: a link span disappearing (non-empty → empty) counts as a
/// change.
#[test]
fn hover_link_cells_changed_true_on_disappear() {
    assert!(hover_link_cells_changed(&[(3, 5, 9)], &[]));
}

/// AC-1: an unchanged span (hover-stable idle frame) must not be
/// reported as a change, so the idle-skip path stays honest.
#[test]
fn hover_link_cells_changed_false_when_unchanged() {
    assert!(!hover_link_cells_changed(&[(3, 5, 9)], &[(3, 5, 9)]));
    assert!(!hover_link_cells_changed(&[], &[]));
}

// ── task0004 AC-1/AC-2: next_wait_deadline pure decision ──────────

/// AC-2: nothing pending → `None` (the caller maps this to
/// `ControlFlow::Wait`) — an idle terminal never reschedules a
/// periodic wakeup.
#[test]
fn next_wait_deadline_none_when_nothing_pending() {
    assert_eq!(next_wait_deadline(None, None, None, None), None);
}

/// AC-1: only the blink deadline is pending → that deadline wins.
#[test]
fn next_wait_deadline_blink_only() {
    let t = Instant::now() + Duration::from_millis(530);
    assert_eq!(next_wait_deadline(Some(t), None, None, None), Some(t));
}

/// AC-1: only the bell deadline is pending → that deadline wins.
#[test]
fn next_wait_deadline_bell_only() {
    let t = Instant::now() + Duration::from_millis(150);
    assert_eq!(next_wait_deadline(None, Some(t), None, None), Some(t));
}

/// AC-1: only the toast deadline is pending → that deadline wins.
#[test]
fn next_wait_deadline_toast_only() {
    let t = Instant::now() + Duration::from_millis(16);
    assert_eq!(next_wait_deadline(None, None, Some(t), None), Some(t));
}

/// task0002 AC-5: only the mux sidebar dim deadline is pending → that
/// deadline wins.
#[test]
fn next_wait_deadline_mux_sidebar_dim_only() {
    let t = Instant::now() + Duration::from_millis(200);
    assert_eq!(next_wait_deadline(None, None, None, Some(t)), Some(t));
}

/// AC-1: blink and bell both pending, blink is the sooner deadline →
/// the nearer (blink) deadline wins.
#[test]
fn next_wait_deadline_picks_sooner_of_blink_and_bell() {
    let now = Instant::now();
    let sooner = now + Duration::from_millis(50);
    let later = now + Duration::from_millis(500);
    assert_eq!(
        next_wait_deadline(Some(sooner), Some(later), None, None),
        Some(sooner)
    );
    // Order of arguments must not matter — the later one is bell here.
    assert_eq!(
        next_wait_deadline(Some(later), Some(sooner), None, None),
        Some(sooner)
    );
}

/// AC-1: all four concerns pending → the earliest of the four wins.
#[test]
fn next_wait_deadline_picks_earliest_of_all_four() {
    let now = Instant::now();
    let blink = now + Duration::from_millis(500);
    let bell = now + Duration::from_millis(10);
    let toast = now + Duration::from_millis(16);
    let mux_sidebar_dim = now + Duration::from_millis(200);
    assert_eq!(
        next_wait_deadline(Some(blink), Some(bell), Some(toast), Some(mux_sidebar_dim)),
        Some(bell)
    );
}

// ── task0002 AC-6: EMTERM_RENDER_PERF frame counter ──────────────

/// AC-6: the first recorded frame always logs (no prior log point).
#[test]
fn frame_counter_logs_first_frame_immediately() {
    let mut counter = FrameCounter::default();
    let now = Instant::now();
    assert_eq!(counter.record_draw(now), Some(1));
}

/// AC-6: a second frame within the same one-second window still
/// counts but does not re-log.
#[test]
fn frame_counter_suppresses_log_within_one_second_window() {
    let mut counter = FrameCounter::default();
    let t0 = Instant::now();
    assert_eq!(counter.record_draw(t0), Some(1));
    let t1 = t0 + Duration::from_millis(500);
    assert_eq!(counter.record_draw(t1), None);
    assert_eq!(counter.drawn, 2, "count must still advance without logging");
}

/// AC-6: once a full second has elapsed since the last log, the next
/// drawn frame logs again with the updated running total.
#[test]
fn frame_counter_logs_again_after_one_second_elapsed() {
    let mut counter = FrameCounter::default();
    let t0 = Instant::now();
    assert_eq!(counter.record_draw(t0), Some(1));
    let t1 = t0 + Duration::from_secs(1);
    assert_eq!(counter.record_draw(t1), Some(2));
}

/// AC-6: with the gate disabled, `record_drawn_frame` never touches
/// the counter — "no counting side effects occur" when
/// `EMTERM_RENDER_PERF` is unset.
#[test]
fn record_drawn_frame_disabled_never_touches_counter() {
    let mut counter = FrameCounter::default();
    let now = Instant::now();
    assert_eq!(record_drawn_frame(false, &mut counter, now), None);
    assert_eq!(counter.drawn, 0, "disabled gate must not count frames");
}

/// AC-6: with the gate enabled, `record_drawn_frame` delegates to
/// the counter and surfaces its log payload.
#[test]
fn record_drawn_frame_enabled_delegates_to_counter() {
    let mut counter = FrameCounter::default();
    let now = Instant::now();
    assert_eq!(record_drawn_frame(true, &mut counter, now), Some(1));
    assert_eq!(counter.drawn, 1);
}

// ── task0003 AC-5: EMTERM_RENDER_PERF rows-rebuilt counter ────────

/// AC-5: the first recorded batch always logs (no prior log point).
#[test]
fn rows_rebuilt_counter_logs_first_batch_immediately() {
    let mut counter = RowsRebuiltCounter::default();
    let now = Instant::now();
    assert_eq!(counter.record_rebuilt(3, now), Some(3));
}

/// AC-5: a second batch within the same one-second window still
/// accumulates but does not re-log.
#[test]
fn rows_rebuilt_counter_suppresses_log_within_one_second_window() {
    let mut counter = RowsRebuiltCounter::default();
    let t0 = Instant::now();
    assert_eq!(counter.record_rebuilt(3, t0), Some(3));
    let t1 = t0 + Duration::from_millis(500);
    assert_eq!(counter.record_rebuilt(2, t1), None);
    assert_eq!(
        counter.rebuilt, 5,
        "total must still advance without logging"
    );
}

/// AC-5: once a full second has elapsed since the last log, the next
/// rebuilt batch logs again with the updated running total.
#[test]
fn rows_rebuilt_counter_logs_again_after_one_second_elapsed() {
    let mut counter = RowsRebuiltCounter::default();
    let t0 = Instant::now();
    assert_eq!(counter.record_rebuilt(1, t0), Some(1));
    let t1 = t0 + Duration::from_secs(1);
    assert_eq!(counter.record_rebuilt(1, t1), Some(2));
}

/// AC-5: with the gate disabled, `record_rebuilt_rows` never touches
/// the counter — "no side effects" when `EMTERM_RENDER_PERF` is unset.
#[test]
fn record_rebuilt_rows_disabled_never_touches_counter() {
    let mut counter = RowsRebuiltCounter::default();
    let now = Instant::now();
    assert_eq!(record_rebuilt_rows(false, &mut counter, 5, now), None);
    assert_eq!(counter.rebuilt, 0, "disabled gate must not count rows");
}

/// AC-3/AC-5: a stable (fully cache-served) frame reports zero rebuilt
/// rows; even with the gate enabled this must not touch the counter
/// (nothing meaningful to log on a frame with no rebuild work).
#[test]
fn record_rebuilt_rows_enabled_with_zero_rows_never_touches_counter() {
    let mut counter = RowsRebuiltCounter::default();
    let now = Instant::now();
    assert_eq!(record_rebuilt_rows(true, &mut counter, 0, now), None);
    assert_eq!(counter.rebuilt, 0);
}

/// AC-5: with the gate enabled, `record_rebuilt_rows` delegates to the
/// counter and surfaces its log payload.
#[test]
fn record_rebuilt_rows_enabled_delegates_to_counter() {
    let mut counter = RowsRebuiltCounter::default();
    let now = Instant::now();
    assert_eq!(record_rebuilt_rows(true, &mut counter, 4, now), Some(4));
    assert_eq!(counter.rebuilt, 4);
}

// ── skk_mode: bare Ctrl+J swallow ────────────────────────────────

// ── FR3 (OSC 8 hyperlink) detect_osc8_link_at helper ─────

/// TS-19: cell carries a safe `http://` OSC 8 URI → `Some(link)`
/// with `LinkKind::Url(uri)` and the cell range covering the run.
#[test]
fn fr3_osc8_safe_uri_returns_link_with_run() {
    let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    // Open OSC 8 with safe URI, write 5 chars, close OSC 8, then
    // a few non-hyperlinked chars.
    core.process_pty_data(b"\x1b]8;;https://example.com/pr/1\x07Hello\x1b]8;;\x07world");

    let link = detect_osc8_link_at(&core, 0, 2).expect("hover on 'l' (col 2) should hit");
    match &link.kind {
        crate::links::LinkKind::Url(u) => assert_eq!(u, "https://example.com/pr/1"),
        other => panic!("expected Url, got {other:?}"),
    }
    // The whole run (cols 0..5 inclusive-exclusive) underlines.
    assert_eq!(link.cells, vec![(0u16, 0u16, 5u16)]);
}

/// TS-20: cell carries an unsafe `javascript:` URI → `None` (and a
/// `warn` log line, not asserted here).
#[test]
fn fr3_osc8_unsafe_uri_returns_none() {
    let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    core.process_pty_data(b"\x1b]8;;javascript:alert(1)\x07x\x1b]8;;\x07");
    assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
}

/// TS-21: cell with `hyperlink_id == 0` (no OSC 8 marker) → `None`.
#[test]
fn fr3_osc8_plain_cell_returns_none() {
    let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    // No OSC 8 at all — just plain text.
    core.process_pty_data(b"plain text");
    assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
    assert_eq!(detect_osc8_link_at(&core, 0, 3), None);
}

/// TS-22: cell has a non-zero hyperlink_id but the URI is missing
/// from the table → `None`. Synthesize this by writing a cell with
/// a stale id via direct table manipulation. Falls back to a
/// process-cleared scenario: the helper sees `get_hyperlink_uri()`
/// return an empty string and returns `None`.
#[test]
fn fr3_osc8_missing_uri_returns_none() {
    let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    // Real-world reproduction is hard without internal accessors;
    // instead we lean on the documented behaviour of
    // `get_hyperlink_uri()` returning empty when the id is missing.
    // Set up a hyperlink, then call detect on an unrelated cell
    // whose id is 0 — that's TS-21. To exercise the empty-URI
    // branch specifically, use an OSC 8 with an empty URI string
    // (also documented to be treated as "no link" per SPEC edge
    // cases).
    core.process_pty_data(b"\x1b]8;;\x07x\x1b]8;;\x07");
    assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
}

/// FR3: out-of-bounds cell coordinates → `None`.
#[test]
fn fr3_osc8_out_of_bounds_returns_none() {
    let core = term_core::terminal_core::TerminalCore::new(80, 24, 0);
    assert_eq!(detect_osc8_link_at(&core, 100, 0), None);
    assert_eq!(detect_osc8_link_at(&core, 0, 100), None);
}

/// FR3: hover on a cell in the middle of a 5-cell OSC 8 run yields
/// the run that starts at col 0 and extends to col 5.
#[test]
fn fr3_osc8_run_expansion_from_middle_cell() {
    let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    core.process_pty_data(b"\x1b]8;;https://example.com\x07Click\x1b]8;;\x07");
    // Hover the last cell of the run.
    let link = detect_osc8_link_at(&core, 0, 4).expect("hover on 'k' should hit");
    assert_eq!(link.cells, vec![(0u16, 0u16, 5u16)]);
}

// ── FR1 (DECSET 1007) wheel → arrow bytes ────────────────

/// TS-3: AltScreen + mode bit + setting all ON + wheel-up 1 notch
/// emits three `ESC[A` bytes (xterm: 3 arrows per notch).
#[test]
fn fr1_wheel_up_in_alt_screen_emits_three_arrow_up() {
    let bytes = alternate_scroll_wheel_bytes(1.0, true, true, true);
    assert_eq!(bytes.as_deref(), Some(b"\x1b[A\x1b[A\x1b[A".as_slice()));
}

/// FR1: wheel-down emits `ESC[B` instead of `ESC[A`.
#[test]
fn fr1_wheel_down_in_alt_screen_emits_three_arrow_down() {
    let bytes = alternate_scroll_wheel_bytes(-1.0, true, true, true);
    assert_eq!(bytes.as_deref(), Some(b"\x1b[B\x1b[B\x1b[B".as_slice()));
}

/// FR1: notch count scales the byte count (2 notches → 6 arrows).
#[test]
fn fr1_wheel_scales_with_notches() {
    let bytes = alternate_scroll_wheel_bytes(2.0, true, true, true);
    assert_eq!(
        bytes.as_deref(),
        Some(b"\x1b[A\x1b[A\x1b[A\x1b[A\x1b[A\x1b[A".as_slice())
    );
}

/// TS-4: same gates as TS-3 but the user setting is OFF; the
/// helper declines so the caller falls through to scrollback.
#[test]
fn fr1_wheel_suppressed_when_setting_off() {
    assert_eq!(alternate_scroll_wheel_bytes(1.0, true, true, false), None);
}

/// TS-5: the terminal-side mode bit (DECSET 1007) is OFF; helper
/// declines.
#[test]
fn fr1_wheel_suppressed_when_mode_bit_off() {
    assert_eq!(alternate_scroll_wheel_bytes(1.0, true, false, true), None);
}

/// TS-6: AltScreen is OFF (normal screen); helper always declines
/// so the existing scrollback-view wheel path runs unchanged.
#[test]
fn fr1_wheel_inert_outside_alt_screen() {
    assert_eq!(alternate_scroll_wheel_bytes(1.0, false, true, true), None);
    assert_eq!(alternate_scroll_wheel_bytes(-1.0, false, true, true), None);
}

/// FR1 edge case: sub-notch pixel deltas (|lines| < 1) round to 0
/// notches and are treated as no-ops. Without this guard a tiny
/// drift would send a stream of arrow bytes per pixel of motion.
#[test]
fn fr1_wheel_sub_notch_pixel_delta_is_noop() {
    assert_eq!(alternate_scroll_wheel_bytes(0.4, true, true, true), None);
    assert_eq!(alternate_scroll_wheel_bytes(-0.4, true, true, true), None);
}

// ── task0003 FR2/FR3: winit button → decision-layer identity ──────

#[test]
fn winit_button_to_report_identity_maps_the_three_modeled_buttons() {
    assert_eq!(
        winit_button_to_report_identity(MouseButton::Left),
        Some(MouseButtonId::Left)
    );
    assert_eq!(
        winit_button_to_report_identity(MouseButton::Middle),
        Some(MouseButtonId::Middle)
    );
    assert_eq!(
        winit_button_to_report_identity(MouseButton::Right),
        Some(MouseButtonId::Right)
    );
}

#[test]
fn winit_button_to_report_identity_none_for_side_buttons() {
    assert_eq!(winit_button_to_report_identity(MouseButton::Back), None);
    assert_eq!(winit_button_to_report_identity(MouseButton::Forward), None);
    assert_eq!(winit_button_to_report_identity(MouseButton::Button6), None);
}

// ── task0003 FR4: scroll-delta → wheel-notch conversion ─────────────

#[test]
fn wheel_report_notches_signed_whole_counts() {
    assert_eq!(wheel_report_notches(2.5), 2);
    assert_eq!(wheel_report_notches(-2.5), -2);
    assert_eq!(wheel_report_notches(0.0), 0);
}

#[test]
fn wheel_report_notches_sub_notch_delta_is_zero() {
    assert_eq!(wheel_report_notches(0.4), 0);
    assert_eq!(wheel_report_notches(-0.4), 0);
}

#[test]
fn wheel_report_notches_non_finite_is_zero() {
    assert_eq!(wheel_report_notches(f32::NAN), 0);
    assert_eq!(wheel_report_notches(f32::INFINITY), 0);
}

// ── task0001 (wheel-report-notch-clamp) AC-1..AC-8: report-path notch cap ──

/// TS1: values just below, at, and just above the cap saturate correctly,
/// and the negated input yields the negated result in every case.
#[test]
fn wheel_report_notches_boundary_values_saturate_at_the_cap() {
    assert_eq!(wheel_report_notches(99.999), 99);
    assert_eq!(wheel_report_notches(100.0), 100);
    assert_eq!(wheel_report_notches(100.999), 100);
    assert_eq!(wheel_report_notches(101.0), 100);
    assert_eq!(wheel_report_notches(-99.999), -99);
    assert_eq!(wheel_report_notches(-100.0), -100);
    assert_eq!(wheel_report_notches(-100.999), -100);
    assert_eq!(wheel_report_notches(-101.0), -100);
}

/// TS2: NaN and both infinities yield 0, and the largest/smallest (most
/// negative) finite `f32` values saturate to the cap / negated cap rather
/// than to `i32::MAX`/`i32::MIN` (D4: the clamp happens before the
/// float-to-integer conversion).
#[test]
fn wheel_report_notches_pathological_inputs_stay_within_the_cap() {
    assert_eq!(wheel_report_notches(f32::NAN), 0);
    assert_eq!(wheel_report_notches(f32::INFINITY), 0);
    assert_eq!(wheel_report_notches(f32::NEG_INFINITY), 0);
    assert_eq!(
        wheel_report_notches(f32::MAX),
        MAX_WHEEL_REPORT_NOTCHES as i32
    );
    assert_eq!(
        wheel_report_notches(f32::MIN),
        -(MAX_WHEEL_REPORT_NOTCHES as i32)
    );
}

/// TS3: both signed zeros and sub-one-line magnitudes yield 0; the first
/// whole line above 1.0 yields the signed single notch.
#[test]
fn wheel_report_notches_near_zero_and_sub_notch_deltas_are_zero() {
    assert_eq!(wheel_report_notches(0.0), 0);
    assert_eq!(wheel_report_notches(-0.0), 0);
    assert_eq!(wheel_report_notches(0.999), 0);
    assert_eq!(wheel_report_notches(-0.999), 0);
    assert_eq!(wheel_report_notches(1.999), 1);
    assert_eq!(wheel_report_notches(-1.999), -1);
}

/// TS4: a hand-written sweep of finite inputs (small, boundary, large,
/// extreme, both signs) — every one must have magnitude at most the cap.
/// Not a property-testing crate (NFR6): a fixed input list.
#[test]
fn wheel_report_notches_invariant_sweep_never_exceeds_the_cap() {
    let inputs: &[f32] = &[
        0.0, 0.5, 1.0, 3.7, 50.0, 99.999, 100.0, 100.999, 101.0, 1_000.0, 1.0e6, 1.0e30,
        f32::MAX, -0.5, -1.0, -3.7, -50.0, -99.999, -100.0, -100.999, -101.0, -1_000.0, -1.0e6,
        -1.0e30, f32::MIN,
    ];
    for &lines in inputs {
        let result = wheel_report_notches(lines);
        assert!(
            result.unsigned_abs() <= MAX_WHEEL_REPORT_NOTCHES,
            "wheel_report_notches({lines}) = {result}, exceeds the cap of \
             {MAX_WHEEL_REPORT_NOTCHES}"
        );
    }
}

/// Realistic single-notch mouse-report payload shape for the duplication
/// helper tests below (task plan Test Notes: about ten bytes; need not be
/// produced by the encoder under test — the invariant is about length
/// arithmetic).
const SAMPLE_WHEEL_REPORT_PAYLOAD: &[u8] = b"\x1b[<64;12;7M";

/// TS5: requested count 0 yields an empty buffer.
#[test]
fn bounded_wheel_report_duplicate_zero_count_yields_empty_buffer() {
    assert_eq!(
        bounded_wheel_report_duplicate(SAMPLE_WHEEL_REPORT_PAYLOAD, 0),
        Vec::<u8>::new()
    );
}

/// TS5: requested count 1 yields the payload byte-for-byte, unduplicated.
#[test]
fn bounded_wheel_report_duplicate_one_count_yields_payload_verbatim() {
    assert_eq!(
        bounded_wheel_report_duplicate(SAMPLE_WHEEL_REPORT_PAYLOAD, 1),
        SAMPLE_WHEEL_REPORT_PAYLOAD.to_vec()
    );
}

/// TS6: requested count exactly at the cap yields exactly
/// `MAX_WHEEL_REPORT_NOTCHES` concatenations.
#[test]
fn bounded_wheel_report_duplicate_at_cap_yields_exactly_cap_concatenations() {
    let got = bounded_wheel_report_duplicate(SAMPLE_WHEEL_REPORT_PAYLOAD, MAX_WHEEL_REPORT_NOTCHES);
    assert_eq!(
        got,
        SAMPLE_WHEEL_REPORT_PAYLOAD.repeat(MAX_WHEEL_REPORT_NOTCHES as usize)
    );
}

/// TS6: requested counts above the cap — including the largest possible
/// `u32` — still cap at exactly `MAX_WHEEL_REPORT_NOTCHES` concatenations,
/// never more.
#[test]
fn bounded_wheel_report_duplicate_above_cap_still_caps_at_exactly_cap() {
    let expected = SAMPLE_WHEEL_REPORT_PAYLOAD.repeat(MAX_WHEEL_REPORT_NOTCHES as usize);
    assert_eq!(
        bounded_wheel_report_duplicate(
            SAMPLE_WHEEL_REPORT_PAYLOAD,
            MAX_WHEEL_REPORT_NOTCHES + 1
        ),
        expected
    );
    assert_eq!(
        bounded_wheel_report_duplicate(SAMPLE_WHEEL_REPORT_PAYLOAD, u32::MAX),
        expected
    );
}

/// TS7: across representative payloads (an empty payload and a realistic
/// single-notch report payload) and requested counts spanning 0 through
/// `u32::MAX`, the returned buffer's length never exceeds payload length ×
/// the cap (D5: capacity and repetition bound derive from the same capped
/// value, so they cannot diverge).
#[test]
fn bounded_wheel_report_duplicate_length_never_exceeds_payload_len_times_cap() {
    let payloads: &[&[u8]] = &[b"", SAMPLE_WHEEL_REPORT_PAYLOAD];
    let counts: &[u32] = &[
        0,
        1,
        MAX_WHEEL_REPORT_NOTCHES - 1,
        MAX_WHEEL_REPORT_NOTCHES,
        MAX_WHEEL_REPORT_NOTCHES + 1,
        u32::MAX,
    ];
    for payload in payloads {
        for &count in counts {
            let got = bounded_wheel_report_duplicate(payload, count);
            assert!(
                got.len() <= payload.len() * MAX_WHEEL_REPORT_NOTCHES as usize,
                "payload.len()={} count={count} got.len()={}",
                payload.len(),
                got.len()
            );
        }
    }
}

// ── task0003 (SC-6, owned by task0002 — see D3): wheel-consumer decision ──

/// TS-8 tracking-active branch: Shift held always yields scroll-scrollback,
/// and never arrow translation — including the alternate-screen cell with
/// the alternate-scroll mode bit and setting both on (the cell the D5
/// collision was resolved against).
#[test]
fn wheel_consumer_tracking_active_shift_held_scrolls_scrollback_never_arrows() {
    for on_alt_screen in [false, true] {
        for mode_bit in [false, true] {
            for setting in [false, true] {
                assert_eq!(
                    wheel_consumer(true, true, on_alt_screen, mode_bit, setting),
                    WheelConsumer::ScrollScrollback
                );
            }
        }
    }
}

/// TS-8 tracking-active branch: Shift not held always reports to the
/// application, regardless of alternate-screen / alternate-scroll state.
#[test]
fn wheel_consumer_tracking_active_shift_not_held_reports_to_application() {
    for on_alt_screen in [false, true] {
        for mode_bit in [false, true] {
            for setting in [false, true] {
                assert_eq!(
                    wheel_consumer(true, false, on_alt_screen, mode_bit, setting),
                    WheelConsumer::ReportToApplication
                );
            }
        }
    }
}

/// TS-8 tracking-inactive branch: today's matrix, reproduced exactly and
/// without consulting Shift.
#[test]
fn wheel_consumer_tracking_inactive_reproduces_todays_matrix_ignoring_shift() {
    for shift_held in [false, true] {
        assert_eq!(
            wheel_consumer(false, shift_held, true, true, true),
            WheelConsumer::TranslateToArrows
        );
        assert_eq!(
            wheel_consumer(false, shift_held, true, true, false),
            WheelConsumer::ScrollScrollback
        );
        assert_eq!(
            wheel_consumer(false, shift_held, true, false, true),
            WheelConsumer::ScrollScrollback
        );
        assert_eq!(
            wheel_consumer(false, shift_held, false, true, true),
            WheelConsumer::ScrollScrollback
        );
    }
}

// ── task0006 AC-1/AC-5: the pointer handlers hold no decision ──────────
//
// task0003/task0004's precedence-order source scans above this comment
// (mouse-report press/release/motion/wheel ordering) pinned the ONLY
// coverage the old inline wiring had, and verify round 1 rejected them as
// such for TS-19/TS-20/TS-21: a correct decision unit whose call site was
// miswired kept those assertions green. task0005/task0006 (D12) moved
// every one of those decisions into `mouse_report::decide_button_event` /
// `decide_motion` / `decide_wheel` — pure functions callable with no
// window, no GPU surface and no live PTY — so the properties those scans
// pinned are now behavioural assertions against the seam in
// `mouse_report::tests` (grid-ownership precedence, gesture-ownership
// release routing including the D12 tab-follows-press correction, the two
// reset observations extended to every path, the motion-gate/cell-filter
// ordering, and the wheel decision's tracking-active/inactive split
// including the arrow-translation impossibility). What remains testable
// only by scanning source is the ONE property that is genuinely
// structural (AC-1's subject): the handler bodies never re-implement any
// part of that decision themselves.

/// AC-1/AC-5: none of the three pointer handlers calls a decision/
/// encoding primitive directly — `compose_button_code`, `encode_report`,
/// `motion_gate` or `point_belongs_to_grid` — every one of those now runs
/// exclusively inside `mouse_report::decide_button_event` /
/// `decide_motion` / `decide_wheel`. A decision unit whose call site
/// re-implemented (rather than delegated to) the decision would still
/// pass every behavioural test in `mouse_report::tests`, so this pins the
/// structural half AC-1 also requires.
#[test]
fn pointer_routing_handlers_hold_no_decision_only_delegate_to_the_seam() {
    let src = include_str!("pointer_routing.rs");
    for marker in [
        "pub(super) fn handle_pointer_moved(",
        "pub(super) fn handle_pointer_button(",
        "pub(super) fn handle_mouse_wheel(",
        "fn run_button_decision(",
    ] {
        assert!(
            src.contains(marker),
            "expected handler `{marker}` not found in pointer_routing.rs"
        );
    }
    for forbidden in [
        "mouse_report::compose_button_code(",
        "mouse_report::encode_report(",
        "mouse_report::motion_gate(",
        "mouse_report::point_belongs_to_grid(",
    ] {
        assert!(
            !src.contains(forbidden),
            "pointer_routing.rs must not call `{forbidden}` directly (AC-1) — every \
             decision primitive runs exclusively inside mouse_report::decide_button_event \
             / decide_motion_event / decide_wheel_event (task0005's SC-10)"
        );
    }
    for delegate in [
        "mouse_report::decide_button_event(",
        "mouse_report::decide_motion_event(",
        "mouse_report::decide_wheel_event(",
        "mouse_report::apply_outcome(",
    ] {
        assert!(
            src.contains(delegate),
            "expected the handlers to delegate to `{delegate}` (AC-1)"
        );
    }
}

/// task0006 (D12): `event_loop.rs`'s `WindowEvent::Focused(false)` arm calls
/// exactly this function, `mouse_report::clear_all`, on the same two host
/// fields (`mouse_report_gesture_owner`, `mouse_report_held`) it zeroes
/// `pointer_buttons_down` alongside — that call site cannot be driven in a
/// test without a winit window, so this pins the call's OWN two guaranteed
/// properties (AC-3) at the record level the task plan's Test Notes name:
/// both records end up empty, and a decision taken against them afterward
/// behaves exactly as a decision against records that were never touched —
/// i.e. a release with no recorded owner reports nothing and takes no local
/// arm, matching `mouse_report::tests::ac6_release_with_no_recorded_press_
/// produces_no_bytes` (task0005) for the identical "no owner" case, now
/// reached via the actual clear path instead of a records value that was
/// simply never populated.
#[test]
fn focus_loss_clear_all_empties_gesture_and_held_records_so_the_next_decision_starts_fresh() {
    use mouse_report::{
        ButtonEventInputs, Disposition, GestureOwner, GestureOwnership, GridOwnershipInputs,
        HeldButtons, MouseButtonId, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        clear_all, decide_button_event,
    };

    // A left-button drag is mid-flight (as it would be if the window lost
    // focus before the matching release arrived) and the right button is
    // also physically held down.
    let mut gesture = GestureOwnership::new();
    gesture.record_press(MouseButtonId::Left, GestureOwner::Report);
    let mut held = HeldButtons {
        left: true,
        middle: false,
        right: true,
    };

    clear_all(&mut gesture, &mut held);

    assert_eq!(
        gesture.peek(MouseButtonId::Left),
        None,
        "focus loss must clear the gesture-ownership record (AC-3)"
    );
    assert_eq!(
        held,
        HeldButtons::default(),
        "focus loss must clear the held-button record (AC-3)"
    );

    // The first pointer event after focus returns — here, a left release
    // arriving with no matching press in the (now-empty) records, exactly
    // what a stranded drag's release looks like post-clear — must be
    // decided from those empty records: no report, no local arm.
    let outcome = decide_button_event(ButtonEventInputs {
        kind: MouseEventKind::Release,
        button: MouseButtonId::Left,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::default(),
        mode_1000: true,
        mode_1002: false,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        hovered_link: false,
        middle_click_paste_enabled: false,
        records: MouseReportRecords {
            gesture_owner: gesture,
            built_for_tab: Some(0),
            ..MouseReportRecords::default()
        },
    });

    assert_eq!(
        outcome.disposition,
        Disposition::Nothing,
        "a release decided from post-clear (empty) records must report nothing and take no \
         local arm (AC-3) — the gesture the focus-loss path stranded is gone, not silently \
         resumed"
    );
}

// ── task0010 AC-2/AC-3: mux sidebar wheel-routing guard wiring ─────

/// Regression guard: the `MouseWheel` handler must query
/// `ui::mux_sidebar::point_in_sidebar` (the shared hit-region
/// derivation task0010 introduces) and `return` early on a hit, BEFORE
/// it reaches the terminal scroll path — this is what makes AC-2 ("the
/// terminal scroll path is skipped": no scrollback movement, no
/// AltScreen arrow bytes, no alt-scroll accumulator change) true, and
/// what makes AC-3 (byte-identical behavior everywhere the helper
/// returns `false`) hold — the branch does nothing but query-and-maybe-
/// return, so a `false` answer falls through to the untouched code
/// below unconditionally. Source-scans the `MouseWheel` arm's body the
/// same way `cell_metrics_px_origin_x_has_no_sidebar_term` guards
/// `cell_metrics_px`'s origin math: the correctness of the DECISION
/// itself (which points are "inside" the sidebar) is exercised by
/// `ui::mux_sidebar::tests::ac1_*` / `ac4_*`; this test pins the
/// STRUCTURAL property that wires that decision to the right place in
/// the winit handler. Pixel-level scroll feel is manual (M-4, per the
/// task plan's Test Notes).
#[test]
fn mouse_wheel_handler_routes_sidebar_hits_to_egui_before_the_terminal_scroll_path() {
    let src = include_str!("pointer_routing.rs");
    let start = src
        .find("pub(super) fn handle_mouse_wheel(")
        .expect("MouseWheel handler not found in pointer_routing.rs");
    let body = &src[start..];
    let sidebar_guard_pos = body.find("mux_sidebar::point_in_sidebar").expect(
        "MouseWheel handler must query ui::mux_sidebar::point_in_sidebar (AC-4: the \
             shared hit-region derivation, not a re-derived guard)",
    );
    let terminal_scroll_pos = body
        .find("let lines = match delta {")
        .expect("terminal scroll path marker (`let lines = match delta {`) not found");
    assert!(
        sidebar_guard_pos < terminal_scroll_pos,
        "the sidebar hit-region guard must run BEFORE the terminal scroll path so a hit \
         skips scrollback / AltScreen-arrow movement (AC-2)"
    );
    let between_guard_and_scroll = &body[sidebar_guard_pos..terminal_scroll_pos];
    assert!(
        between_guard_and_scroll.contains("return;"),
        "the sidebar hit-region guard must `return` on a hit so the terminal scroll path \
         is genuinely skipped, not merely forwarded-then-continued (AC-2)"
    );
}

// ── task0011 AC-1/AC-3/AC-4: mux sidebar press-suppression guard ───

/// AC-1/AC-3: the PointerButton handler's Pressed-edge suppression guard
/// (the same `if button == MouseButton::Left && state ==
/// ElementState::Pressed` block that already covers the bottom status
/// bar and the scrollbar) must query the shared
/// `ui::mux_sidebar::point_in_sidebar` helper and `return` on a hit —
/// this is what makes a press on the overlay card (zero grid inset, so
/// the old persistent-only width test missed it) stop before the
/// selection-start arm, while keeping the guard scoped to the Pressed
/// edge only (a drag that started inside the terminal still gets its
/// Released event processed normally, since this block never runs for
/// `ElementState::Released`). Source-scans the way
/// `mouse_wheel_handler_routes_sidebar_hits_to_egui_before_the_terminal_scroll_path`
/// does; the geometric correctness of "is this point inside the
/// sidebar" is exercised by `ui::mux_sidebar::tests::ac1_*`/`ac4_*`.
/// AC-2 (overlay closed / local tab: selection starts as before)
/// follows from `point_in_sidebar` answering `false` there — pinned by
/// `ui::mux_sidebar::tests` (`visible_placement: None` returns
/// `false` unconditionally), so the guard here is a complete no-op in
/// that case and this test does not re-derive that coverage.
#[test]
fn mouse_input_press_guard_queries_shared_sidebar_hit_region_before_selection_start() {
    let src = include_str!("pointer_routing.rs");
    let arm_start = src
        .find("pub(super) fn handle_pointer_button(")
        .expect("PointerButton handler not found in pointer_routing.rs");
    let arm_body = &src[arm_start..];
    let guard_start = arm_body
        .find("// Same rule for the bottom status-bar panel")
        .expect("bottom-strip/scrollbar/sidebar press guard comment not found");
    let guard_end = arm_body
        .find("// While the profile-selector modal is up")
        .expect("profile-selector guard marker not found after the press guard");
    let guard_section = &arm_body[guard_start..guard_end];
    assert!(
        guard_section
            .contains("if button == MouseButton::Left && state == ElementState::Pressed {"),
        "the sidebar press guard must stay inside the Pressed-edge-only conditional \
         shared with the bottom-strip/scrollbar guards (AC-3)"
    );
    let sidebar_guard_pos = guard_section.find("mux_sidebar::point_in_sidebar(").expect(
        "PointerButton's press guard must query ui::mux_sidebar::point_in_sidebar \
         (AC-4: the shared hit-region derivation, not a re-derived guard)",
    );
    assert!(
        guard_section.contains("return;"),
        "the sidebar press guard must `return` on a hit so the selection-start arm \
         is genuinely skipped (AC-1)"
    );
    // task0006 (D12): the selection-start arm is no longer a literal
    // `match (button, state)` arm — it is `LocalArm::BeginSelectionDrag`,
    // named by SC-10 and performed by `begin_selection_drag` only after
    // every guard above (including this one) has had its chance to
    // return. The ordering property this test pins is unchanged: the
    // guard section located above ends before this marker appears.
    let selection_start_pos = arm_body
        .find("LocalArm::BeginSelectionDrag")
        .expect("selection-start arm (LocalArm::BeginSelectionDrag) not found in the \
                 PointerButton handler");
    assert!(
        guard_start + sidebar_guard_pos < selection_start_pos,
        "the sidebar hit-region guard must run BEFORE the selection-start arm so a hit \
         on the overlay card never starts a terminal selection (AC-1)"
    );
}

/// AC-4: the press guard and the wheel guard both resolve the sidebar
/// region through `ui::mux_sidebar::point_in_sidebar` — neither
/// independently re-derives the sidebar's geometry (e.g. by calling
/// `sidebar_width` directly), which is exactly the class of drift the
/// round-2 scrollbar click-guard regression came from
/// (IMPLEMENTATION.md decision 3.5).
#[test]
fn press_and_wheel_guards_share_the_same_sidebar_hit_region_helper() {
    let src = include_str!("pointer_routing.rs");
    let press_start = src
        .find("pub(super) fn handle_pointer_button(")
        .expect("PointerButton handler not found in pointer_routing.rs");
    let wheel_start = src
        .find("pub(super) fn handle_mouse_wheel(")
        .expect("MouseWheel handler not found in pointer_routing.rs");
    assert!(
        press_start < wheel_start,
        "expected the PointerButton handler to appear before the MouseWheel handler"
    );
    let press_body = &src[press_start..wheel_start];
    assert!(
        press_body.contains("mux_sidebar::point_in_sidebar("),
        "PointerButton press guard must call the shared hit-region helper"
    );
    assert!(
        !press_body.contains("mux_sidebar::sidebar_width("),
        "PointerButton press guard must not re-derive the sidebar width itself"
    );
    let wheel_body = &src[wheel_start..];
    let wheel_arm_end = wheel_body
        .find("let lines = match delta {")
        .expect("terminal scroll path marker not found after the MouseWheel guard");
    let wheel_guard_section = &wheel_body[..wheel_arm_end];
    assert!(
        wheel_guard_section.contains("mux_sidebar::point_in_sidebar("),
        "MouseWheel guard must call the shared hit-region helper"
    );
    assert!(
        !wheel_guard_section.contains("mux_sidebar::sidebar_width("),
        "MouseWheel guard must not re-derive the sidebar width itself"
    );
}

// ── task0002 D5 "Hover feed": overlay hover shares the same hit test ──

/// The hover feed (which maintains `App::mux_sidebar_overlay_hovered`)
/// must query the SAME `ui::mux_sidebar::point_in_sidebar` helper the
/// press/wheel guards above use, not a re-derived boundary check — this
/// is what makes "the hit test and the click routing must agree on the
/// boundary" (task0002 task plan Scheduling §1) structurally true
/// rather than merely coincidental. Mirrors
/// `press_and_wheel_guards_share_the_same_sidebar_hit_region_helper`'s
/// source-scan approach; the geometric correctness of "is this point
/// inside the sidebar" is exercised by `ui::mux_sidebar::tests::ac1_*`.
#[test]
fn pointer_moved_hover_feed_shares_the_same_sidebar_hit_region_helper() {
    let src = include_str!("pointer_routing.rs");
    let arm_start = src
        .find("pub(super) fn handle_pointer_moved(")
        .expect("PointerMoved handler not found in pointer_routing.rs");
    let arm_body = &src[arm_start..];
    let arm_end = arm_body
        .find("\npub(super) fn handle_pointer_button(")
        .expect("PointerButton handler not found after handle_pointer_moved");
    let moved_body = &arm_body[..arm_end];
    assert!(
        moved_body.contains("mux_sidebar::point_in_sidebar("),
        "the PointerMoved hover feed must call the shared hit-region helper"
    );
    assert!(
        !moved_body.contains("mux_sidebar::sidebar_width("),
        "the PointerMoved hover feed must not re-derive the sidebar width itself"
    );
    assert!(
        moved_body.contains("set_mux_sidebar_hovered("),
        "the PointerMoved handler must feed the hit-test result into \
         App::set_mux_sidebar_hovered"
    );
}

// ── skk_mode: bare Ctrl+J swallow ────────────────────────────────

// ── preedit_effective_dirty_rows: row-cache invalidation during IME
//    preedit (fix for the stale/blank-row High finding) ─────────────

/// The anchor row is force-included even when `term_core`'s own dirty
/// set is empty, and the row below it (composition wrap) too — the
/// core bug this fixes: without this, `row_cache` would never learn
/// about the row the composition overlays while term_core considers
/// it clean.
#[test]
fn preedit_dirty_rows_forces_anchor_and_next_row() {
    let rows = preedit_effective_dirty_rows(Some(vec![]), 24, 5);
    assert_eq!(rows, vec![5, 6]);
}

/// `None` (a forced full redraw) still expands to the full row range
/// with the anchor rows folded in (already present, so no duplicates).
#[test]
fn preedit_dirty_rows_none_means_full_redraw() {
    let rows = preedit_effective_dirty_rows(None, 4, 1);
    assert_eq!(rows, vec![0, 1, 2, 3]);
}

/// An anchor row already present in term_core's dirty set is not
/// duplicated, and the existing dirty rows are preserved alongside it.
#[test]
fn preedit_dirty_rows_merges_without_duplicates() {
    let rows = preedit_effective_dirty_rows(Some(vec![2, 5]), 24, 5);
    assert_eq!(rows, vec![2, 5, 6]);
}

/// The anchor row's "next row" (wrap case) is clamped at the grid
/// bottom — no out-of-range row index is ever produced.
#[test]
fn preedit_dirty_rows_clamps_anchor_at_last_row() {
    let rows = preedit_effective_dirty_rows(Some(vec![]), 24, 23);
    assert_eq!(rows, vec![23]);
}

#[test]
fn skk_chord_matches_bare_ctrl_j_case_insensitive() {
    let ctrl = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    assert!(is_skk_swallowed_chord(
        &WinitKey::Character("j".into()),
        ctrl
    ));
    assert!(is_skk_swallowed_chord(
        &WinitKey::Character("J".into()),
        ctrl
    ));
}

#[test]
fn skk_chord_rejects_extra_mods_and_other_keys() {
    let ctrl = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    // Extra modifiers — the WebView skip requires Ctrl alone.
    assert!(!is_skk_swallowed_chord(
        &WinitKey::Character("j".into()),
        Modifiers {
            shift: true,
            ..ctrl
        }
    ));
    assert!(!is_skk_swallowed_chord(
        &WinitKey::Character("j".into()),
        Modifiers { alt: true, ..ctrl }
    ));
    // No Ctrl at all.
    assert!(!is_skk_swallowed_chord(
        &WinitKey::Character("j".into()),
        Modifiers::NONE
    ));
    // Other keys keep flowing to the PTY encoder.
    assert!(!is_skk_swallowed_chord(
        &WinitKey::Character("k".into()),
        ctrl
    ));
    assert!(!is_skk_swallowed_chord(
        &WinitKey::Named(NamedKey::Enter),
        ctrl
    ));
}

// ── task0001: shift_enter_rewrite pure decision (AC-3 / AC-4) ──────

#[test]
fn shift_enter_rewrite_none_drops_shift_and_encodes_plain_enter() {
    // AC-3: `none` -> the plain Enter encoding (Shift dropped, no Alt).
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::None);
    assert_eq!(
        rewrite,
        ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
        })
    );
}

#[test]
fn shift_enter_rewrite_alt_enter_drops_shift_and_sets_alt() {
    // AC-3: `alt_enter` -> the Alt+Enter encoding.
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::AltEnter);
    assert_eq!(
        rewrite,
        ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            ctrl: false,
            alt: true,
        })
    );
}

#[test]
fn shift_enter_rewrite_kitty_csi_u_emits_exact_raw_bytes() {
    // AC-3: `kitty_csi_u` -> the exact bytes
    // 0x1B 0x5B 0x31 0x33 0x3B 0x32 0x75, independent of host-PTY vs
    // mux encode target (the raw-bytes path bypasses the encoder
    // entirely, so the target never enters this decision).
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::KittyCsiU);
    match rewrite {
        ShiftEnterRewrite::RawBytes(bytes) => {
            assert_eq!(bytes, &[0x1B, 0x5B, 0x31, 0x33, 0x3B, 0x32, 0x75]);
        }
        other => panic!("expected RawBytes, got {other:?}"),
    }
}

#[test]
fn shift_enter_rewrite_lf_emits_exact_raw_byte() {
    // AC-1 (task0001): `lf` -> the exact single byte 0x0a, independent
    // of host-PTY vs mux encode target (the raw-bytes path bypasses
    // the encoder entirely, so the target never enters this decision).
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::Lf);
    match rewrite {
        ShiftEnterRewrite::RawBytes(bytes) => {
            assert_eq!(bytes, &[0x0A]);
        }
        other => panic!("expected RawBytes, got {other:?}"),
    }
}

#[test]
fn shift_enter_rewrite_unchanged_when_ctrl_held() {
    // AC-4: Enter with Ctrl+Shift is not rewritten under any value.
    let mods = Modifiers {
        shift: true,
        ctrl: true,
        alt: false,
    };
    for behavior in [
        ShiftEnterBehavior::None,
        ShiftEnterBehavior::AltEnter,
        ShiftEnterBehavior::KittyCsiU,
        ShiftEnterBehavior::Lf,
    ] {
        assert_eq!(
            shift_enter_rewrite(true, mods, behavior),
            ShiftEnterRewrite::Unchanged
        );
    }
}

#[test]
fn shift_enter_rewrite_unchanged_when_alt_already_held() {
    // AC-4: Enter with Alt (Shift+Alt) is not rewritten under any value.
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: true,
    };
    for behavior in [
        ShiftEnterBehavior::None,
        ShiftEnterBehavior::AltEnter,
        ShiftEnterBehavior::KittyCsiU,
        ShiftEnterBehavior::Lf,
    ] {
        assert_eq!(
            shift_enter_rewrite(true, mods, behavior),
            ShiftEnterRewrite::Unchanged
        );
    }
}

#[test]
fn shift_enter_rewrite_unchanged_when_plain_ctrl_enter_no_shift() {
    // AC-4: Enter with Ctrl (no Shift) is not rewritten under any value.
    let mods = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
    for behavior in [
        ShiftEnterBehavior::None,
        ShiftEnterBehavior::AltEnter,
        ShiftEnterBehavior::KittyCsiU,
        ShiftEnterBehavior::Lf,
    ] {
        assert_eq!(
            shift_enter_rewrite(true, mods, behavior),
            ShiftEnterRewrite::Unchanged
        );
    }
}

#[test]
fn shift_enter_rewrite_unchanged_when_not_enter_key() {
    // Bare Shift on a non-Enter key is never rewritten.
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    assert_eq!(
        shift_enter_rewrite(false, mods, ShiftEnterBehavior::KittyCsiU),
        ShiftEnterRewrite::Unchanged
    );
    assert_eq!(
        shift_enter_rewrite(false, mods, ShiftEnterBehavior::Lf),
        ShiftEnterRewrite::Unchanged
    );
}

// ── selection-clear-on-enter-copy task0001: clear-decision predicate ──

#[test]
fn should_clear_selection_on_forward_truth_table() {
    // AC-1: true only when both the event was forwarded to the PTY and
    // the key is the named Enter key; false in every other combination.
    // This also covers the "printable/cursor key forwarded" and
    // "non-forwarded Enter" edge cases from the task plan's Test Notes.
    assert!(should_clear_selection_on_forward(true, true));
    assert!(!should_clear_selection_on_forward(true, false));
    assert!(!should_clear_selection_on_forward(false, true));
    assert!(!should_clear_selection_on_forward(false, false));
}

#[test]
fn should_clear_selection_on_forward_true_across_all_shift_enter_modes_when_forwarded() {
    // AC-2: `is_enter` is captured once at event_loop.rs:448, BEFORE the
    // `shift_enter_rewrite` decision runs, so its value does not depend
    // on which of the four `shift_enter_behavior` forms the rewrite
    // produces. Exercising `shift_enter_rewrite` for all four modes (and
    // observing it takes different shapes) while holding `is_enter` at
    // the single upstream-computed `true` shows the predicate answers
    // true in every mode whenever the key was forwarded.
    let mods = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    let is_enter = true;
    for behavior in [
        ShiftEnterBehavior::None,
        ShiftEnterBehavior::AltEnter,
        ShiftEnterBehavior::KittyCsiU,
        ShiftEnterBehavior::Lf,
    ] {
        let _ = shift_enter_rewrite(is_enter, mods, behavior);
        assert!(
            should_clear_selection_on_forward(true, is_enter),
            "predicate must answer true for shift_enter_behavior {behavior:?} \
             when the key was forwarded"
        );
    }
}

// ── selection-clear-on-enter-copy task0001: call-site source scans ────

/// AC-4a: the forwarded branch in `event_loop.rs` must consult the
/// Enter-conditioned clear predicate and call the clear helper when it
/// answers true. Scoped between `if forwarded {` and the next
/// `request_redraw` call (the smallest stable landmark that closes the
/// branch), the same shape as the pointer-routing scans above.
#[test]
fn forwarded_branch_clears_selection_when_enter_predicate_holds() {
    let src = include_str!("event_loop.rs");
    let start = src
        .find("if forwarded {")
        .expect("`if forwarded {` branch not found in event_loop.rs");
    let end = start
        + src[start..]
            .find("host.window().request_redraw();")
            .expect("request_redraw marker not found after the forwarded branch");
    let body = &src[start..end];
    assert!(
        body.contains("should_clear_selection_on_forward(forwarded, is_enter)"),
        "the forwarded branch must consult should_clear_selection_on_forward \
         with the forwarded flag and the pre-rewrite Enter flag (AC-4a)"
    );
    assert!(
        body.contains("self.app.clear_selection()"),
        "the forwarded branch must call the clear helper when the predicate \
         holds (AC-4a, FR1)"
    );
}

/// AC-4b: the copy branch in `key_routing.rs` must call the clear helper
/// immediately after the clipboard write, and only inside the
/// selection-present branch.
#[test]
fn copy_chord_clears_selection_immediately_after_clipboard_write() {
    let src = include_str!("key_routing.rs");
    let fn_start = src
        .find("pub(super) fn handle_special_chord(")
        .expect("handle_special_chord not found in key_routing.rs");
    let body = &src[fn_start..];
    let sel_branch_start = body
        .find("if let Some(sel) = app.selection {")
        .expect("selection-present branch not found in the copy chord handler");
    let clipboard_marker = "host.set_clipboard(&text);";
    let clipboard_rel = body[sel_branch_start..]
        .find(clipboard_marker)
        .expect("clipboard write not found inside the selection-present branch");
    let after_clipboard = &body[sel_branch_start + clipboard_rel + clipboard_marker.len()..];
    let clear_rel = after_clipboard
        .find("app.clear_selection();")
        .expect("clear call not found after the clipboard write (AC-4b, FR5)");
    let between = &after_clipboard[..clear_rel];
    // Allow whitespace and `//` line comments (explanatory notes on the
    // call site) but nothing else — no other statement may sit between
    // the clipboard write and the clear call.
    let only_whitespace_and_comments = between.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with("//")
    });
    assert!(
        only_whitespace_and_comments,
        "expected only whitespace/comments between the clipboard write and the \
         clear call so the clear runs immediately after it (AC-4b); found: {between:?}"
    );
}

/// AC-5: the IME-consume path returns before the clear predicate call
/// site is ever reached, so a composed key can never trigger the clear.
#[test]
fn ime_consume_path_returns_before_the_selection_clear_site() {
    let src = include_str!("event_loop.rs");
    let ime_pos = src
        .find("KeyDispatchResult::Consumed")
        .expect("IME consume dispatch result not found in event_loop.rs");
    let clear_pos = src
        .find("should_clear_selection_on_forward(")
        .expect("clear predicate call site not found in event_loop.rs");
    assert!(
        ime_pos < clear_pos,
        "the IME-consume check must appear (and return) before the clear \
         predicate call site, so a composed key never reaches it (AC-5)"
    );
    let between = &src[ime_pos..clear_pos];
    assert!(
        between.contains("return;"),
        "the IME-consume arm must `return` so it cannot fall through to the \
         clear site (AC-5)"
    );
}

/// AC-5: a modifier key held alone is neither a named key nor
/// `WinitKey::Character`, nor does it carry printable `event.text` — it
/// must still fall through to the final `_ => return None` arm in
/// `winit_key_to_bytes`, producing no bytes and therefore never setting
/// `forwarded = true` at the Enter-clear call site.
#[test]
fn winit_key_to_bytes_bare_modifier_still_falls_through_to_none() {
    let src = include_str!("input_translate.rs");
    let start = src
        .find("pub(super) fn winit_key_to_bytes(")
        .expect("winit_key_to_bytes not found in input_translate.rs");
    let end = start
        + src[start..]
            .find("/// Upper bound for a single wheel event's arrow-key emission")
            .expect("winit_key_to_bytes should be followed by the alt-scroll section");
    let body = &src[start..end];
    assert!(
        body.contains("WinitKey::Character(s) =>"),
        "expected the printable-character arm to still gate the final match"
    );
    assert!(
        body.contains("_ => return None,"),
        "expected a bare modifier (matching neither a named key, printable \
         text, nor Character) to still fall through to `return None` \
         (AC-5: no bytes means `forwarded` stays false)"
    );
}

// ── task0002: synthetic key press gate (AC-1 / AC-2) ──────────────

#[test]
fn synthetic_key_press_gate_drops_synthetic_press() {
    // AC-1: a synthetic Pressed event must be gated (dropped) so it
    // never reaches keybinding dispatch or a PTY write.
    assert!(should_drop_synthetic_key_event(true));
}

#[test]
fn synthetic_key_press_gate_drops_synthetic_release() {
    // AC-1 (Released arm): the same predicate governs the Released
    // arm — a synthetic release is dropped by the same gate (design
    // note in IMPLEMENTATION.md Shared Components). The gate does not
    // take press/release state, so a synthetic flag alone is enough
    // to prove the release arm is covered too.
    assert!(should_drop_synthetic_key_event(true));
}

#[test]
fn synthetic_key_press_gate_allows_non_synthetic_press() {
    // AC-2 (Pressed arm): a non-synthetic press is processed exactly
    // as before — the gate must not drop it.
    assert!(!should_drop_synthetic_key_event(false));
}

#[test]
fn synthetic_key_press_gate_allows_non_synthetic_release() {
    // AC-2 (Released arm): a non-synthetic release is processed
    // exactly as before — the gate must not drop it.
    assert!(!should_drop_synthetic_key_event(false));
}

#[test]
fn egui_fonts_empty_ui_font_keeps_default_proportional_head() {
    let fonts = build_egui_fonts("", "");
    assert!(!fonts.font_data.contains_key("EmtermUiFont"));
    assert!(!fonts.font_data.contains_key("EmtermTerminalFont"));
    // Bundled CJK / emoji fallbacks are appended to both chains.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let chain = &fonts.families[&family];
        assert!(chain.iter().any(|n| n == "EmtermBundledCJK"));
        assert!(chain.iter().any(|n| n == "EmtermBundledEmoji"));
        assert!(chain.iter().any(|n| n == "EmtermBundledSymbols"));
        // …but never as the primary face.
        assert_ne!(chain[0], "EmtermBundledCJK");
    }
    // Empty terminal font → Monospace HEAD falls back to bundled
    // Inconsolata (mirrors the terminal grid's BUNDLED_BASE_FONT
    // behavior). Without this, chrome would render on egui's
    // bundled Hack while the grid renders on Inconsolata.
    assert_eq!(
        fonts.families[&egui::FontFamily::Monospace][0],
        "EmtermBundledBase"
    );
    // The bundled base is Monospace-only — it must not leak into
    // Proportional (the tab-bar / title-bar font).
    assert!(
        fonts.families[&egui::FontFamily::Proportional]
            .iter()
            .all(|n| n != "EmtermBundledBase")
    );
}

#[test]
fn egui_fonts_unknown_ui_font_falls_back_to_default() {
    let fonts = build_egui_fonts("Emterm No Such Font Family 9000", "");
    assert!(!fonts.font_data.contains_key("EmtermUiFont"));
    let prop = &fonts.families[&egui::FontFamily::Proportional];
    assert_ne!(prop[0], "EmtermUiFont");
}

#[test]
fn egui_fonts_unknown_terminal_font_falls_back_to_default() {
    let fonts = build_egui_fonts("", "Emterm No Such Terminal Font 9000");
    assert!(!fonts.font_data.contains_key("EmtermTerminalFont"));
    let mono = &fonts.families[&egui::FontFamily::Monospace];
    assert_ne!(mono[0], "EmtermTerminalFont");
}

#[test]
fn egui_fonts_known_ui_font_prepends_to_proportional_only() {
    // Resolve a family that actually exists on this host via the
    // same fontdb scan the production path uses; skip silently on
    // fontless CI hosts.
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let Some(family) = db
        .faces()
        .flat_map(|f| f.families.first())
        .map(|(name, _)| name.clone())
        .next()
    else {
        return;
    };
    let fonts = build_egui_fonts(&family, "");
    assert!(
        fonts.font_data.contains_key("EmtermUiFont"),
        "host family {family:?} should load"
    );
    assert_eq!(
        fonts.families[&egui::FontFamily::Proportional][0],
        "EmtermUiFont"
    );
    // Monospace mirrors --terminal-font-family in the WebView build
    // and must not pick up the UI font.
    assert!(
        fonts.families[&egui::FontFamily::Monospace]
            .iter()
            .all(|n| n != "EmtermUiFont")
    );
}

#[test]
fn egui_fonts_known_terminal_font_prepends_to_monospace_only() {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let Some(family) = db
        .faces()
        .flat_map(|f| f.families.first())
        .map(|(name, _)| name.clone())
        .next()
    else {
        return;
    };
    let fonts = build_egui_fonts("", &family);
    assert!(
        fonts.font_data.contains_key("EmtermTerminalFont"),
        "host family {family:?} should load"
    );
    assert_eq!(
        fonts.families[&egui::FontFamily::Monospace][0],
        "EmtermTerminalFont"
    );
    // The terminal font must not leak into Proportional (that
    // chain is skinned by --ui-font-family).
    assert!(
        fonts.families[&egui::FontFamily::Proportional]
            .iter()
            .all(|n| n != "EmtermTerminalFont")
    );
}

#[test]
fn click_classifier_single_click_is_character() {
    let mut t = ClickTracker::default();
    let now = Instant::now();
    let cls = t.classify(now, 5, 10);
    assert_eq!(cls.count, 1);
    assert_eq!(cls.mode, SelectionMode::Character);
}

#[test]
fn click_classifier_double_click_within_window_at_same_cell() {
    let mut t = ClickTracker::default();
    let t0 = Instant::now();
    let _ = t.classify(t0, 5, 10);
    let t1 = t0 + Duration::from_millis(200);
    let cls = t.classify(t1, 5, 10);
    assert_eq!(cls.count, 2);
    assert_eq!(cls.mode, SelectionMode::Word);
}

#[test]
fn click_classifier_triple_click_at_same_cell() {
    let mut t = ClickTracker::default();
    let t0 = Instant::now();
    let _ = t.classify(t0, 5, 10);
    let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
    let cls = t.classify(t0 + Duration::from_millis(200), 5, 10);
    assert_eq!(cls.count, 3);
    assert_eq!(cls.mode, SelectionMode::Line);
}

#[test]
fn click_classifier_resets_after_triple() {
    let mut t = ClickTracker::default();
    let t0 = Instant::now();
    let _ = t.classify(t0, 5, 10);
    let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
    let _ = t.classify(t0 + Duration::from_millis(200), 5, 10);
    // Fourth click within window collapses back to Character.
    let cls = t.classify(t0 + Duration::from_millis(300), 5, 10);
    assert_eq!(cls.count, 1);
    assert_eq!(cls.mode, SelectionMode::Character);
}

#[test]
fn click_classifier_resets_when_position_changes() {
    let mut t = ClickTracker::default();
    let t0 = Instant::now();
    let _ = t.classify(t0, 5, 10);
    let cls = t.classify(t0 + Duration::from_millis(100), 5, 11);
    // Different cell → back to single click.
    assert_eq!(cls.count, 1);
    assert_eq!(cls.mode, SelectionMode::Character);
}

#[test]
fn click_classifier_resets_when_window_expires() {
    let mut t = ClickTracker::default();
    let t0 = Instant::now();
    let _ = t.classify(t0, 5, 10);
    // 600 ms > MULTI_CLICK_WINDOW_MS (500 ms) → reset.
    let cls = t.classify(t0 + Duration::from_millis(600), 5, 10);
    assert_eq!(cls.count, 1);
    assert_eq!(cls.mode, SelectionMode::Character);
}

/// TS-32 (host=Some): `PocApp::user_event` must call `request_redraw`
/// on the active window exactly once. We exercise the extracted
/// `request_redraw_on_user_event` helper because constructing a
/// real `winit::Window` here would require an active event loop +
/// display, which is unavailable in `cargo test`.
///
/// A `Cell<u32>` counter stands in for the winit window's
/// `request_redraw()` side effect. Without the `user_event`
/// override the provider-owned wake chain (`WakeFn` →
/// `EventLoopProxy::send_event(())` → `user_event`) was silently
/// dropped, freezing the status-bar clock on idle (release-build
/// regression observed twice during sdd.6-verify).
#[test]
fn user_event_dispatches_redraw_when_host_present() {
    use std::cell::Cell;
    let redraws: Cell<u32> = Cell::new(0);
    let host_stub: u8 = 0;
    request_redraw_on_user_event(Some(&host_stub), |_| {
        redraws.set(redraws.get() + 1);
    });
    assert_eq!(redraws.get(), 1);
}

#[test]
fn resize_edge_interior_is_none() {
    // Dead-center of a 800×600 window: nowhere near any edge.
    assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 300.0, 6.0), None);
}

#[test]
fn resize_edge_corners_classify_to_diagonals() {
    use ResizeDirection::*;
    // Each corner pixel grabs the diagonal direction so the user
    // can resize width + height together.
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 1.0, 1.0, 6.0),
        Some(NorthWest)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 799.0, 1.0, 6.0),
        Some(NorthEast)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 1.0, 599.0, 6.0),
        Some(SouthWest)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 799.0, 599.0, 6.0),
        Some(SouthEast)
    );
}

#[test]
fn resize_edge_sides_classify_to_cardinals() {
    use ResizeDirection::*;
    // Mid-edge sample on each of the four sides.
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 400.0, 1.0, 6.0),
        Some(North)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 400.0, 599.0, 6.0),
        Some(South)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 1.0, 300.0, 6.0),
        Some(West)
    );
    assert_eq!(
        classify_resize_edge(800.0, 600.0, 799.0, 300.0, 6.0),
        Some(East)
    );
}

#[test]
fn resize_edge_outside_window_is_none() {
    // Wayland can deliver negative or past-edge coords during
    // pointer leave; both must yield `None` so the hot-zone
    // cache doesn't latch a stale direction.
    assert_eq!(classify_resize_edge(800.0, 600.0, -1.0, 300.0, 6.0), None);
    assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 700.0, 6.0), None);
}

/// TS-32 (host=None): before `Resumed` constructs the `WindowHost`
/// or after `CloseRequested` tears it down, `self.host` is `None`.
/// In that window `user_event` must be a no-op rather than panic.
#[test]
fn user_event_is_noop_when_host_absent() {
    use std::cell::Cell;
    let redraws: Cell<u32> = Cell::new(0);
    let host: Option<&u8> = None;
    request_redraw_on_user_event(host, |_| {
        redraws.set(redraws.get() + 1);
    });
    assert_eq!(redraws.get(), 0);
}

/// Verify that winit_key_to_egui covers every function key F1..=F20.
///
/// parse_main_key in keybinds.rs accepts F1..=F20 as valid chord keys.
/// This test keeps the two domains in sync: if either side drifts, this
/// test will catch it before a user-configured F13–F20 shortcut silently
/// falls through to PTY input at runtime.
#[test]
fn winit_key_to_egui_covers_f1_through_f20() {
    let pairs: &[(WinitKey, egui::Key)] = &[
        (WinitKey::Named(NamedKey::F1), egui::Key::F1),
        (WinitKey::Named(NamedKey::F2), egui::Key::F2),
        (WinitKey::Named(NamedKey::F3), egui::Key::F3),
        (WinitKey::Named(NamedKey::F4), egui::Key::F4),
        (WinitKey::Named(NamedKey::F5), egui::Key::F5),
        (WinitKey::Named(NamedKey::F6), egui::Key::F6),
        (WinitKey::Named(NamedKey::F7), egui::Key::F7),
        (WinitKey::Named(NamedKey::F8), egui::Key::F8),
        (WinitKey::Named(NamedKey::F9), egui::Key::F9),
        (WinitKey::Named(NamedKey::F10), egui::Key::F10),
        (WinitKey::Named(NamedKey::F11), egui::Key::F11),
        (WinitKey::Named(NamedKey::F12), egui::Key::F12),
        (WinitKey::Named(NamedKey::F13), egui::Key::F13),
        (WinitKey::Named(NamedKey::F14), egui::Key::F14),
        (WinitKey::Named(NamedKey::F15), egui::Key::F15),
        (WinitKey::Named(NamedKey::F16), egui::Key::F16),
        (WinitKey::Named(NamedKey::F17), egui::Key::F17),
        (WinitKey::Named(NamedKey::F18), egui::Key::F18),
        (WinitKey::Named(NamedKey::F19), egui::Key::F19),
        (WinitKey::Named(NamedKey::F20), egui::Key::F20),
    ];
    for (winit_key, expected) in pairs {
        assert_eq!(
            winit_key_to_egui(winit_key),
            Some(*expected),
            "winit_key_to_egui({winit_key:?}) did not return {expected:?}"
        );
    }
}

// ── FR1 clamp + non-finite guard (Finding B) + accumulator (Finding A) ──

/// Non-finite inputs (NaN, Infinity) must return None without
/// panicking or triggering a runaway Vec allocation.
#[test]
fn alternate_scroll_wheel_bytes_rejects_non_finite() {
    assert_eq!(
        alternate_scroll_wheel_bytes(f32::NAN, true, true, true),
        None
    );
    assert_eq!(
        alternate_scroll_wheel_bytes(f32::INFINITY, true, true, true),
        None
    );
}

/// A huge positive delta is clamped to MAX_ALT_SCROLL_NOTCHES notches;
/// the resulting Vec is never a multi-GB allocation.
#[test]
fn alternate_scroll_wheel_bytes_clamps_huge_delta() {
    let bytes = alternate_scroll_wheel_bytes(1.0e9, true, true, true).unwrap();
    // 3 bytes per arrow, 3 arrows per notch, at most MAX_ALT_SCROLL_NOTCHES notches.
    assert!(bytes.len() <= (MAX_ALT_SCROLL_NOTCHES as usize) * 3 * 3);
}

/// Four successive 0.3-line trackpad events accumulate: the first
/// three resolve to 0.0 whole lines (no arrow fired), and on the
/// fourth the accumulator crosses 1.0 and one notch is consumed
/// with ~0.2 fractional remainder.
#[test]
fn accumulate_alt_scroll_lines_collects_sub_notch_deltas() {
    let (w, a) = accumulate_alt_scroll_lines(0.0, 0.3);
    assert_eq!(w, 0.0);
    assert!((a - 0.3).abs() < 1e-6, "after 1st event: accum={a}");

    let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
    assert_eq!(w, 0.0);
    assert!((a - 0.6).abs() < 1e-6, "after 2nd event: accum={a}");

    let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
    assert_eq!(w, 0.0);
    assert!((a - 0.9).abs() < 1e-6, "after 3rd event: accum={a}");

    let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
    assert_eq!(w, 1.0, "4th event should yield one notch");
    assert!((a - 0.2).abs() < 1e-6, "4th event remainder={a}");
}

// ── task0001 (wheel-report-fraction-accum): report-path accumulator ──

/// AC-1: a run of same-direction sub-notch deltas whose running sum
/// reaches one notch produces exactly one notch at the crossing event,
/// and zero notches (no report) at every earlier event in the run.
#[test]
fn accumulate_wheel_report_lines_collects_sub_notch_deltas_same_direction() {
    let (n, a) = accumulate_wheel_report_lines(0.0, 0.3);
    assert_eq!(n, 0, "1st event must not cross a notch");
    assert!((a - 0.3).abs() < 1e-6, "after 1st event: accum={a}");

    let (n, a) = accumulate_wheel_report_lines(a, 0.3);
    assert_eq!(n, 0, "2nd event must not cross a notch");
    assert!((a - 0.6).abs() < 1e-6, "after 2nd event: accum={a}");

    let (n, a) = accumulate_wheel_report_lines(a, 0.3);
    assert_eq!(n, 0, "3rd event must not cross a notch");
    assert!((a - 0.9).abs() < 1e-6, "after 3rd event: accum={a}");

    let (n, a) = accumulate_wheel_report_lines(a, 0.3);
    assert_eq!(n, 1, "4th event must cross exactly one notch");
    assert!((a - 0.2).abs() < 1e-6, "4th event remainder={a}");
}

/// AC-2: the leftover fraction is sign-preserving and an opposite-direction
/// delta nets against it instead of restarting from zero — demonstrated by
/// comparing the netted result against what a FRESH accumulator (starting
/// at zero) would have produced from the same opposite delta alone, which
/// differs. The emitted direction (the sign of the consumed notch count)
/// matches the delta that actually crossed the boundary.
#[test]
fn accumulate_wheel_report_lines_nets_opposite_direction_delta_against_the_remainder() {
    // Build up a positive remainder.
    let (n, a) = accumulate_wheel_report_lines(0.0, 1.9);
    assert_eq!(n, 1, "one notch up consumed");
    assert!((a - 0.9).abs() < 1e-6, "remainder after 1st event: {a}");

    // A same-magnitude-ish opposite delta nets against the 0.9 remainder
    // rather than starting a fresh -0.95 accumulation.
    let (n, netted) = accumulate_wheel_report_lines(a, -0.95);
    assert_eq!(n, 0, "netting must not itself cross a notch here");
    let (fresh_n, fresh) = accumulate_wheel_report_lines(0.0, -0.95);
    assert_eq!(fresh_n, 0);
    assert_ne!(
        netted, fresh,
        "the remainder from the first event must change the second event's \
         outcome — a restart-from-zero implementation would make these equal"
    );
    assert!(
        (netted - (-0.05)).abs() < 1e-6,
        "netted remainder should be close to -0.05, got {netted}"
    );

    // A larger opposite delta both nets AND flips the consumed direction —
    // the emitted direction must match the sign of the notch actually
    // consumed (down), not the sign of the very first delta that built up
    // the remainder (up).
    let (n2, a2) = accumulate_wheel_report_lines(0.9, -1.95);
    assert_eq!(n2, -1, "the notch consumed here must be signed down");
    assert!((a2 - (-0.05)).abs() < 1e-6, "remainder after flip: {a2}");
}

/// AC-3: a non-finite delta (NaN, +inf, -inf) produces no report and
/// leaves the accumulator bit-identical; a positive infinity followed by a
/// negative infinity does not silence the report path — a following finite
/// delta behaves exactly as if the two infinities had never arrived.
#[test]
fn accumulate_wheel_report_lines_non_finite_deltas_are_rejected_without_mutation() {
    for lines in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let (n, a) = accumulate_wheel_report_lines(0.4, lines);
        assert_eq!(n, 0, "non-finite delta {lines} must yield zero notches");
        assert_eq!(a, 0.4, "non-finite delta {lines} must leave acc unchanged");
    }

    // +inf then -inf then a finite delta, all starting from the same acc,
    // must behave exactly as if only the finite delta had ever arrived.
    let start = 0.2_f32;
    let (n1, a1) = accumulate_wheel_report_lines(start, f32::INFINITY);
    assert_eq!((n1, a1), (0, start));
    let (n2, a2) = accumulate_wheel_report_lines(a1, f32::NEG_INFINITY);
    assert_eq!((n2, a2), (0, start));
    let after_infinities = accumulate_wheel_report_lines(a2, 1.5);
    let never_arrived = accumulate_wheel_report_lines(start, 1.5);
    assert_eq!(
        after_infinities, never_arrived,
        "a +inf then -inf pair must not silence or alter the report path"
    );
}

/// Edge case (Test Notes): a delta that lands exactly on a notch boundary
/// leaves an exact zero remainder, not a denormal leftover.
#[test]
fn accumulate_wheel_report_lines_exact_notch_boundary_leaves_zero_remainder() {
    let (n, a) = accumulate_wheel_report_lines(0.5, 0.5);
    assert_eq!(n, 1);
    assert_eq!(a, 0.0, "boundary-exact crossing must leave exactly 0.0, not a denormal leftover");

    let (n, a) = accumulate_wheel_report_lines(-0.5, -0.5);
    assert_eq!(n, -1);
    assert_eq!(a, 0.0);
}

/// Edge case (Test Notes): an accumulator holding a near-one magnitude when
/// a same-direction delta arrives crosses cleanly, with no overflow/NaN.
#[test]
fn accumulate_wheel_report_lines_near_one_accumulator_plus_same_direction_delta() {
    let (n, a) = accumulate_wheel_report_lines(0.999_999, 0.5);
    assert_eq!(n, 1);
    assert!(a.is_finite() && a.abs() < 1.0, "remainder must be finite and sub-notch, got {a}");
}

/// Edge case (Test Notes): a long run of alternating-sign sub-notch deltas
/// must never emit a notch.
#[test]
fn accumulate_wheel_report_lines_alternating_sign_sub_notch_run_never_emits() {
    let mut acc = 0.0_f32;
    for i in 0..50 {
        let delta = if i % 2 == 0 { 0.4 } else { -0.4 };
        let (n, new_acc) = accumulate_wheel_report_lines(acc, delta);
        assert_eq!(n, 0, "iteration {i} (delta={delta}) must not emit a notch");
        acc = new_acc;
    }
}

/// AC-4: for every delta tested — including floating-point extremes and an
/// accumulator pre-loaded near the cap — the notch count handed to the
/// duplication step has magnitude at most the report cap, and the returned
/// fraction stays finite and sub-notch (postcondition e). A fixed sweep
/// (NFR6: no property-testing crate), not one hand-picked value.
#[test]
fn accumulate_wheel_report_lines_never_exceeds_the_cap_across_a_sweep_of_extremes() {
    let cases: &[(f32, f32)] = &[
        (0.0, 0.0),
        (0.0, 50.0),
        (0.0, 99.999),
        (0.0, 100.0),
        (0.0, 100.999),
        (0.0, 101.0),
        (0.0, 1_000.0),
        (0.0, 1.0e6),
        (0.0, 1.0e30),
        (0.0, f32::MAX),
        (0.0, -50.0),
        (0.0, -100.999),
        (0.0, -1.0e30),
        (0.0, f32::MIN),
        (0.999, 99.5),
        (-0.999, -99.5),
        (0.5, f32::MAX),
        (-0.5, f32::MIN),
    ];
    for &(acc, lines) in cases {
        let (n, frac) = accumulate_wheel_report_lines(acc, lines);
        assert!(
            n.unsigned_abs() <= MAX_WHEEL_REPORT_NOTCHES,
            "accumulate_wheel_report_lines({acc}, {lines}) = ({n}, {frac}), notch magnitude \
             exceeds the cap of {MAX_WHEEL_REPORT_NOTCHES}"
        );
        assert!(
            frac.is_finite() && frac.abs() < 1.0,
            "accumulate_wheel_report_lines({acc}, {lines}) returned a non-finite or \
             out-of-range fraction {frac}"
        );
    }
}

/// D4: when saturation clips the notch magnitude, the clipped excess is
/// discarded, not banked — the returned fraction is exactly `0.0`, never a
/// leftover sliver of the huge delta that got clipped away.
#[test]
fn accumulate_wheel_report_lines_saturation_discards_excess_instead_of_banking_it() {
    let (n, frac) = accumulate_wheel_report_lines(0.0, 1.0e6);
    assert_eq!(n, MAX_WHEEL_REPORT_NOTCHES as i32);
    assert_eq!(frac, 0.0, "saturated-away magnitude must not survive as a leftover remainder");

    let (n, frac) = accumulate_wheel_report_lines(0.0, -1.0e6);
    assert_eq!(n, -(MAX_WHEEL_REPORT_NOTCHES as i32));
    assert_eq!(frac, 0.0);
}

/// AC-10 (D3): the report-path cap and the alternate-scroll cap are
/// separate constants, never aliased to nor derived from one another.
/// Value equality cannot distinguish "the same constant" from "two
/// independently-defined constants that happen to share a value" — this
/// scans the two accumulators' own source text (the same technique
/// `cell_metrics_px_origin_x_has_no_sidebar_term` above uses) so a future
/// edit that makes one path reference the other's constant fails this
/// test, not just a value comparison.
///
/// AC-7 (restores SPEC AC-10, D12): both regions are bounded at a located
/// following-item marker, exactly the same way on both sides — the
/// report-path region used to run open-ended to the end of the file,
/// which pulled this very file's own `#[cfg(test)]` test module into the
/// scanned text.
#[test]
fn wheel_report_and_alt_scroll_accumulators_reference_only_their_own_cap_constant() {
    let src = include_str!("input_translate.rs");

    let alt_start = src
        .find("pub(super) fn accumulate_alt_scroll_lines")
        .expect("marker `accumulate_alt_scroll_lines` not found");
    let alt_end = src[alt_start..]
        .find("pub(super) fn alternate_scroll_wheel_bytes")
        .map(|i| alt_start + i)
        .expect("marker `alternate_scroll_wheel_bytes` not found");
    let alt_scroll_region = &src[alt_start..alt_end];
    assert!(
        !alt_scroll_region.contains("MAX_WHEEL_REPORT_NOTCHES"),
        "accumulate_alt_scroll_lines must never reference the report-path cap"
    );

    let report_start = src
        .find("pub(super) fn accumulate_wheel_report_lines")
        .expect("marker `accumulate_wheel_report_lines` not found");
    let report_end = src[report_start..]
        .find("pub(super) fn input_mods_to_egui")
        .map(|i| report_start + i)
        .expect("marker `input_mods_to_egui` not found after accumulate_wheel_report_lines");
    let report_region = &src[report_start..report_end];
    assert!(
        !report_region.contains("MAX_ALT_SCROLL_NOTCHES"),
        "accumulate_wheel_report_lines must never reference the alt-scroll cap"
    );
    assert!(
        !report_region.contains("#[cfg(test)]"),
        "AC-7: the report-path cap-separation scan must be bounded at a following-item \
         marker, the same way the alt-scroll region already is — an open-ended scan would \
         reach into this file's own test module"
    );
}

/// AC-6 (seam level): a remainder accumulated on one tab contributes
/// nothing after the active tab changes. Asserts both halves the Test
/// Notes call for: the outcome carries the reset and the new tab marker,
/// and applying it zeroes the accumulator — after which the first wheel
/// event on the new tab reports exactly what a zero accumulator would.
#[test]
fn decide_wheel_event_active_tab_change_discards_the_carried_remainder() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_outcome, decide_wheel_event,
    };

    let mut records = MouseReportRecords {
        report_accum: 0.9,
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 1, // changed from the records' tab 0
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };

    let outcome = decide_wheel_event(&inputs);
    assert!(outcome.updates.reset, "AC-6: a tab change must reset");
    assert_eq!(outcome.updates.built_for_tab, Some(1));

    let mut dest = Vec::new();
    apply_outcome(outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "AC-6: the 0.9 remainder carried on tab 0 must not survive onto tab 1"
    );
    assert_eq!(records.built_for_tab, Some(1));

    let from_reset = accumulate_wheel_report_lines(records.report_accum, 1.5);
    let from_fresh = accumulate_wheel_report_lines(0.0, 1.5);
    assert_eq!(
        from_reset, from_fresh,
        "AC-6: the first wheel event on the new tab must report exactly what a zero \
         accumulator would"
    );
}

/// AC-7 (seam level, first half): a remainder accumulated while tracking is
/// active is discarded once a wheel event itself observes tracking
/// released — the simple case the existing `!tracking_active` reset
/// already covers.
#[test]
fn decide_wheel_event_observing_tracking_inactive_discards_the_carried_remainder() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_outcome, decide_wheel_event,
    };

    let mut records = MouseReportRecords {
        report_accum: 0.75,
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false, // tracking released
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };

    let outcome = decide_wheel_event(&inputs);
    assert!(outcome.updates.reset);
    let mut dest = Vec::new();
    apply_outcome(outcome, &mut records, &mut dest);
    assert_eq!(records.report_accum, 0.0);

    // Re-enabling tracking starts accumulation from zero: the next wheel
    // event on the same tab (no further observation needed — the
    // accumulator is already zero) reports as a fresh session would.
    let from_reset = accumulate_wheel_report_lines(records.report_accum, 2.5);
    let from_fresh = accumulate_wheel_report_lines(0.0, 2.5);
    assert_eq!(from_reset, from_fresh);
}

/// AC-4 (restores SPEC AC-7 / FR5, D10): re-pointed from task0001's
/// `became_active` latch mechanism to D10's discard-at-observation
/// mechanism. Tracking's inactive phase is observed only by an
/// owner-recorded button RELEASE, which — by design (D10, unchanged) —
/// does not itself reset the cell-change cache or gesture ownership. That
/// release's OWN `apply_outcome` call must now discard the report
/// accumulator immediately, at the observation itself — not by storing a
/// latch a LATER wheel event reads (an intervening accepted pointer event
/// could swallow such a latch before the wheel event ever sees it; see
/// the dedicated intervening-event test below). The behaviour this test
/// originally named is unchanged — a release-observed reactivation still
/// starts the next wheel event from zero — only the mechanism, and so
/// only the assertions, move: from the outcome's `reset` flag to the
/// accumulator's own value, checked right after the release's apply.
#[test]
fn decide_wheel_event_reactivation_after_an_owner_recorded_release_accumulates_from_zero() {
    use mouse_report::{
        ButtonEventInputs, GestureOwner, GestureOwnership, GridOwnershipInputs, MouseButtonId,
        MouseEventKind, MouseReportEncoding, MouseReportRecords, WheelEventInputs, apply_outcome,
        decide_button_event, decide_wheel_event,
    };

    // A left-button drag is mid-flight, reported ownership, from a tracking
    // session that already accumulated a 0.9 wheel remainder on tab 0.
    let mut gesture_owner = GestureOwnership::new();
    gesture_owner.record_press(MouseButtonId::Left, GestureOwner::Report);
    let mut records = MouseReportRecords {
        report_accum: 0.9,
        built_for_tab: Some(0),
        gesture_owner,
        ..MouseReportRecords::default()
    };

    // The application releases tracking mid-drag; the matching release
    // arrives while tracking is already inactive.
    let release_outcome = decide_button_event(ButtonEventInputs {
        kind: MouseEventKind::Release,
        button: MouseButtonId::Left,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false, // tracking released
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        hovered_link: false,
        middle_click_paste_enabled: false,
        records,
    });
    assert!(
        !release_outcome.updates.reset,
        "D10: an owner-recorded release must not itself reset gesture/cache state"
    );
    assert_eq!(
        release_outcome.updates.tracking_active,
        Some(false),
        "the release must still surface the tracking-inactive observation it made"
    );

    let mut dest = Vec::new();
    apply_outcome(release_outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "D10: the release's OWN apply_outcome call must discard the remainder AT the \
         observation of inactive tracking, not defer it to a later event"
    );

    // Tracking re-enables; the next wheel event on the SAME tab arrives.
    // The remainder is already gone, so no further reset is needed for
    // this event to accumulate from zero.
    let wheel_inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true, // tracking re-enabled
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let wheel_outcome = decide_wheel_event(&wheel_inputs);
    apply_outcome(wheel_outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "the stale 0.9 remainder must not reappear once tracking re-enables"
    );
}

/// AC-8: a wheel event rejected by the grid-ownership gate advances
/// nothing and resets nothing — the outcome's record updates equal the
/// default value, and applying it leaves the WHOLE record value
/// byte-identical, not just the accumulator field.
#[test]
fn decide_wheel_event_rejected_by_grid_leaves_the_whole_record_value_unchanged() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        RecordUpdates, WheelEventInputs, apply_outcome, decide_wheel_event,
    };

    let before = MouseReportRecords {
        report_accum: 0.42,
        built_for_tab: Some(2),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs {
            in_title_bar_band: true,
            ..GridOwnershipInputs::default()
        },
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 5, // even a tab change must not matter once rejected
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records: before,
    };

    let outcome = decide_wheel_event(&inputs);
    assert_eq!(outcome.updates, RecordUpdates::default());

    let mut records = before;
    let mut dest = Vec::new();
    apply_outcome(outcome, &mut records, &mut dest);
    assert_eq!(
        records, before,
        "AC-8: the whole record value must be unchanged, not just the accumulator field"
    );
    assert!(dest.is_empty());
}

/// Supporting unit check for the release test above: `decide_motion_event`
/// has the identical D10 gap for a mid-drag motion event whose gesture is
/// LOCALLY owned (never previously read tracking-mode bits at all). A
/// mid-drag motion must now surface the tracking-active observation
/// without touching the gesture/cache records it deliberately leaves
/// alone.
#[test]
fn decide_motion_event_owner_established_local_branch_surfaces_tracking_observation_only() {
    use mouse_report::{
        GestureOwner, GestureOwnership, GridOwnershipInputs, MotionEventInputs,
        MouseReportEncoding, MouseReportRecords, RecordUpdates, decide_motion_event,
    };

    let mut gesture_owner = GestureOwnership::new();
    gesture_owner.record_press(mouse_report::MouseButtonId::Left, GestureOwner::Local);
    let records = MouseReportRecords {
        built_for_tab: Some(0),
        gesture_owner,
        ..MouseReportRecords::default()
    };
    let inputs = MotionEventInputs {
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false, // tracking inactive during this mid-drag motion
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        held_left: true,
        held_middle: false,
        held_right: false,
        column: 9,
        row: 9,
        records,
    };

    let outcome = decide_motion_event(inputs);
    assert_eq!(
        outcome.updates,
        RecordUpdates {
            tracking_active: Some(false),
            ..RecordUpdates::default()
        },
        "a Local-owned mid-drag motion must surface only the tracking observation — no reset, \
         no cache commit, no gesture change (D10 unchanged)"
    );
}

// ── task0002 (wheel-report-fraction-accum, review round 1): the per-event
// report step gate, D9/D10's discard-ordering, and D11's single notch-rule
// implementation ─────────────────────────────────────────────────────────

/// AC-1 (restores SPEC AC-8 / FR6, step level): for a wheel event the
/// grid-ownership gate rejects, `apply_wheel_report_step` yields a zero
/// notch count and leaves the WHOLE record value — the report accumulator
/// included — bit-identical, for a delta large enough that folding it
/// would have crossed a notch boundary. Constructed through the real
/// decision layer (`decide_wheel_event`) so this exercises the actual
/// rejected disposition, not a hand-built one.
#[test]
fn apply_wheel_report_step_grid_rejected_notch_yields_zero_and_leaves_the_whole_record_unchanged()
 {
    use mouse_report::{
        Disposition, GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_wheel_report_step, decide_wheel_event,
    };

    let before = MouseReportRecords {
        report_accum: 0.9,
        built_for_tab: Some(3),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs {
            in_title_bar_band: true,
            ..GridOwnershipInputs::default()
        },
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 3,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records: before,
    };
    let outcome = decide_wheel_event(&inputs);
    assert!(
        !matches!(outcome.disposition, Disposition::Report { .. }),
        "test setup: a grid-rejected wheel event must never be a Report disposition"
    );

    let mut records = before;
    let mut dest = Vec::new();
    // A delta large enough that folding it into 0.9 would cross a notch
    // boundary (0.9 + 5.0 = 5.9 -> would report 5 notches if folded).
    let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 5.0);

    assert_eq!(
        notches, 0,
        "AC-1: a rejected notch must yield a zero notch count"
    );
    assert_eq!(
        records, before,
        "AC-1: the whole record value — the report accumulator included — must stay \
         bit-identical across a rejected notch"
    );
    assert!(dest.is_empty());
}

/// AC-2 (restores SPEC AC-10 / FR7, step level): for a wheel event whose
/// outcome names a local arm, `apply_wheel_report_step` yields a zero
/// notch count and leaves the report accumulator exactly what
/// `apply_outcome`'s record updates alone would produce — the fold never
/// runs — for a delta large enough that folding it would have crossed a
/// notch boundary. Both arms named in the criterion: the scrollback arm
/// reached with tracking active and Shift held, and the arrow-translation
/// arm.
#[test]
fn apply_wheel_report_step_local_arm_dispositions_leave_the_accumulator_unaffected_by_the_fold() {
    use mouse_report::{
        Disposition, GridOwnershipInputs, LocalArm, MouseEventKind, MouseReportEncoding,
        MouseReportRecords, WheelEventInputs, apply_outcome, apply_wheel_report_step,
        decide_wheel_event,
    };

    // Arm 1: tracking active, Shift held -> ScrollScrollback. Tab and
    // tracking observation both match the records, so `reset` does not
    // fire — isolating the fold-suppression property on its own.
    {
        let before = MouseReportRecords {
            report_accum: 0.6,
            built_for_tab: Some(0),
            ..MouseReportRecords::default()
        };
        let inputs = WheelEventInputs {
            kind: MouseEventKind::WheelUp,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: false,
            alt_scroll_mode_bit: false,
            alt_scroll_setting: false,
            records: before,
        };
        let outcome = decide_wheel_event(&inputs);
        assert_eq!(outcome.disposition, Disposition::Local(LocalArm::ScrollScrollback));
        assert!(!outcome.updates.reset, "test setup: this arm must not itself reset");

        // Reference: what apply_outcome alone (no fold) would leave.
        let mut expected = before;
        let mut expected_dest = Vec::new();
        apply_outcome(outcome.clone(), &mut expected, &mut expected_dest);

        let mut records = before;
        let mut dest = Vec::new();
        let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 5.0);
        assert_eq!(notches, 0, "AC-2: the scrollback local arm must yield a zero notch count");
        assert_eq!(
            records, expected,
            "AC-2: the scrollback local arm must leave the accumulator exactly what \
             apply_outcome's own updates produce — unaffected by the fold"
        );
    }

    // Arm 2: tracking inactive, on the alt screen with both alt-scroll
    // gates on -> TranslateToArrowBytes.
    {
        let before = MouseReportRecords {
            report_accum: 0.6,
            built_for_tab: Some(0),
            ..MouseReportRecords::default()
        };
        let inputs = WheelEventInputs {
            kind: MouseEventKind::WheelUp,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: false,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: true,
            alt_scroll_mode_bit: true,
            alt_scroll_setting: true,
            records: before,
        };
        let outcome = decide_wheel_event(&inputs);
        assert_eq!(
            outcome.disposition,
            Disposition::Local(LocalArm::TranslateToArrowBytes)
        );

        // Reference: what apply_outcome alone (no fold) would leave — this
        // arm DOES reset (tracking is inactive), so the reference already
        // captures that zeroing; the property under test is that the fold
        // contributes nothing ON TOP of it.
        let mut expected = before;
        let mut expected_dest = Vec::new();
        apply_outcome(outcome.clone(), &mut expected, &mut expected_dest);

        let mut records = before;
        let mut dest = Vec::new();
        let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 5.0);
        assert_eq!(
            notches, 0,
            "AC-2: the arrow-translation local arm must yield a zero notch count"
        );
        assert_eq!(
            records, expected,
            "AC-2: the arrow-translation local arm must leave the accumulator exactly what \
             apply_outcome's own updates produce — unaffected by the fold"
        );
    }
}

/// AC-3 (FR1, FR2 regression): for a wheel event whose outcome is a
/// report, `apply_wheel_report_step` folds the delta exactly once — the
/// notch count it returns, and the fraction it stores back, are exactly
/// `accumulate_wheel_report_lines`'s own output for that fold, computed
/// once.
#[test]
fn apply_wheel_report_step_report_disposition_folds_the_delta_exactly_once() {
    use mouse_report::{
        Disposition, GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_wheel_report_step, decide_wheel_event,
    };

    let before = MouseReportRecords {
        report_accum: 0.6,
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true, // tracking active, no shift -> report
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records: before,
    };
    let outcome = decide_wheel_event(&inputs);
    assert!(matches!(outcome.disposition, Disposition::Report { .. }));

    let mut records = before;
    let mut dest = Vec::new();
    let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 0.7);

    let (expected_notches, expected_frac) = accumulate_wheel_report_lines(0.6, 0.7);
    assert_eq!(
        notches, expected_notches,
        "AC-3: the duplication step must receive exactly the folded unit's own output"
    );
    assert_eq!(
        records.report_accum, expected_frac,
        "AC-3: the delta must be folded exactly once into the stored fraction"
    );
    assert_eq!(dest.len(), 1, "a report disposition must still push exactly one report");
}

/// AC-3: a run of sub-notch deltas, driven entirely through the gate
/// (`apply_wheel_report_step`, not `accumulate_wheel_report_lines`
/// directly), still reports exactly at the crossing event — matching
/// what a direct, ungated run of the same deltas would produce.
#[test]
fn apply_wheel_report_step_sub_notch_run_still_reports_exactly_at_the_crossing_event() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_wheel_report_step, decide_wheel_event,
    };

    let deltas = [0.3_f32, 0.3, 0.3, 0.3, 0.3];
    let mut records = MouseReportRecords {
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let mut reference_acc = 0.0_f32;
    let mut dest = Vec::new();

    for (i, &delta) in deltas.iter().enumerate() {
        let inputs = WheelEventInputs {
            kind: MouseEventKind::WheelUp,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: false,
            alt_scroll_mode_bit: false,
            alt_scroll_setting: false,
            records,
        };
        let outcome = decide_wheel_event(&inputs);
        let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, delta);

        let (expected_notches, expected_frac) = accumulate_wheel_report_lines(reference_acc, delta);
        reference_acc = expected_frac;
        assert_eq!(
            notches, expected_notches,
            "iteration {i} (delta={delta}): gated run must match an ungated direct fold"
        );
        assert_eq!(records.report_accum, reference_acc, "iteration {i}: accumulator drifted");
    }
}

/// AC-4 (restores SPEC AC-7 / FR5): the discard-at-observation mechanism
/// (D10) survives an intervening ACCEPTED, non-wheel event between the
/// owner-recorded release that observes tracking inactive and the next
/// wheel event — the class of event the predecessor `became_active` latch
/// mechanism could be swallowed by (an intervening accepted pointer event
/// would consume the latch before the wheel event ever read it). Ordering
/// is asserted explicitly at each step.
#[test]
fn discard_at_release_survives_an_intervening_accepted_motion_before_the_next_wheel_event() {
    use mouse_report::{
        ButtonEventInputs, GestureOwner, GestureOwnership, GridOwnershipInputs, MotionEventInputs,
        MouseButtonId, MouseEventKind, MouseReportEncoding, MouseReportRecords, WheelEventInputs,
        apply_outcome, apply_wheel_report_step, decide_button_event, decide_motion_event,
        decide_wheel_event,
    };

    let mut gesture_owner = GestureOwnership::new();
    gesture_owner.record_press(MouseButtonId::Left, GestureOwner::Report);
    let mut records = MouseReportRecords {
        report_accum: 0.9,
        built_for_tab: Some(0),
        gesture_owner,
        ..MouseReportRecords::default()
    };
    let mut dest = Vec::new();

    // 1. The application releases tracking mid-drag; the matching release
    //    observes tracking inactive.
    let release_outcome = decide_button_event(ButtonEventInputs {
        kind: MouseEventKind::Release,
        button: MouseButtonId::Left,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        hovered_link: false,
        middle_click_paste_enabled: false,
        records,
    });
    apply_outcome(release_outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "step 1: the release must discard the remainder AT the observation, not later"
    );

    // 2. Tracking re-enables, THEN an accepted motion event (with no held
    //    button — a plain hover) runs BEFORE any wheel event. This is the
    //    intervening accepted event the old latch could be swallowed by.
    let motion_outcome = decide_motion_event(MotionEventInputs {
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true, // tracking re-enabled
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        held_left: false,
        held_middle: false,
        held_right: false,
        column: 6,
        row: 6,
        records,
    });
    assert_eq!(motion_outcome.updates.tracking_active, Some(true));
    apply_outcome(motion_outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "step 2: an intervening accepted motion event must not resurrect a remainder"
    );

    // 3. The next wheel event, on the same tab, with tracking active, must
    //    accumulate from zero — not from the stale 0.9.
    let wheel_inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let wheel_outcome = decide_wheel_event(&wheel_inputs);
    let notches = apply_wheel_report_step(wheel_outcome, &mut records, &mut dest, 0.5);
    let (expected_notches, expected_frac) = accumulate_wheel_report_lines(0.0, 0.5);
    assert_eq!(notches, expected_notches);
    assert_eq!(
        records.report_accum, expected_frac,
        "step 3: must accumulate from zero, not the stale 0.9"
    );
}

/// AC-5 (FR5 scope boundary): discarding the report accumulator at a
/// tracking-session boundary clears neither the cell-change cache nor a
/// gesture-ownership slot. Concretely: a press that records a report
/// owner, followed by tracking being toggled off (observed by an
/// owner-established motion event, which — D10, unchanged — does not
/// itself reset gesture/cache state) and back on, still leaves that owner
/// recorded, so the drag's matching release still produces its report —
/// not merely "the owner slot is non-empty".
#[test]
fn discard_at_tracking_session_boundary_leaves_the_owner_recorded_and_the_drags_release_still_reports()
 {
    use mouse_report::{
        ButtonEventInputs, Disposition, GestureOwner, MotionEventInputs, GridOwnershipInputs,
        MouseButtonId, MouseEventKind, MouseReportEncoding, MouseReportRecords, apply_outcome,
        decide_button_event, decide_motion_event,
    };

    let mut records = MouseReportRecords {
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let mut dest = Vec::new();

    // A left press is reported and records a Report-owned gesture.
    let press_outcome = decide_button_event(ButtonEventInputs {
        kind: MouseEventKind::Press,
        button: MouseButtonId::Left,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        hovered_link: false,
        middle_click_paste_enabled: false,
        records,
    });
    assert!(matches!(press_outcome.disposition, Disposition::Report { .. }));
    apply_outcome(press_outcome, &mut records, &mut dest);
    assert_eq!(
        records.gesture_owner.peek(MouseButtonId::Left),
        Some(GestureOwner::Report),
        "test setup: the press must have recorded a report owner"
    );
    records.report_accum = 0.6; // simulate a remainder accumulated during this session

    // Tracking observed inactive by an OWNER-ESTABLISHED motion event (the
    // held button matches the recorded gesture) — D10: this branch never
    // resets gesture/cache, so any owner loss here would be the bug AC-5
    // guards against.
    let inactive_motion = decide_motion_event(MotionEventInputs {
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false, // tracking inactive
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        held_left: true,
        held_middle: false,
        held_right: false,
        column: 6,
        row: 6,
        records,
    });
    assert_eq!(inactive_motion.updates.tracking_active, Some(false));
    apply_outcome(inactive_motion, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "the tracking-session discard must have fired"
    );
    assert_eq!(
        records.gesture_owner.peek(MouseButtonId::Left),
        Some(GestureOwner::Report),
        "AC-5: the tracking-session discard must not clear the recorded owner"
    );

    // Tracking re-enables, observed by another owner-established motion.
    let active_motion = decide_motion_event(MotionEventInputs {
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        held_left: true,
        held_middle: false,
        held_right: false,
        column: 6,
        row: 6,
        records,
    });
    apply_outcome(active_motion, &mut records, &mut dest);
    assert_eq!(
        records.gesture_owner.peek(MouseButtonId::Left),
        Some(GestureOwner::Report),
        "the owner must still be recorded once tracking re-enables"
    );

    // The original press's matching release now arrives: since the owner
    // survived, it must still route to Report and produce bytes.
    let release_outcome = decide_button_event(ButtonEventInputs {
        kind: MouseEventKind::Release,
        button: MouseButtonId::Left,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        hovered_link: false,
        middle_click_paste_enabled: false,
        records,
    });
    match &release_outcome.disposition {
        Disposition::Report { .. } => {}
        other => panic!("AC-5: the drag's release must still report, got {other:?}"),
    }
    let dest_len_before = dest.len();
    apply_outcome(release_outcome, &mut records, &mut dest);
    assert_eq!(
        dest.len(),
        dest_len_before + 1,
        "AC-5: the surviving release must have produced report bytes"
    );
    assert_eq!(records.gesture_owner.peek(MouseButtonId::Left), None);
}

/// AC-6 (restores SPEC AC-9 / FR1, FR4, NFR3, D11, structural half):
/// `wheel_report_notches` must be expressed in terms of
/// `accumulate_wheel_report_lines` rather than restating the non-finite
/// rejection, the float-domain saturation, the toward-zero truncation and
/// the sign application a second time, and must carry no dead-code
/// allowance. The behavioural half of this criterion is the existing
/// `wheel_report_notches_*` tests above continuing to pass unedited.
#[test]
fn wheel_report_notches_delegates_to_the_accumulating_unit_and_carries_no_dead_code_allowance() {
    let src = include_str!("input_translate.rs");
    // A stable, single-line anchor that precedes the function in both its
    // pre- and post-D11 form, so the region below also covers the
    // attribute line directly above the function signature (which `start`
    // itself would exclude).
    let doc_start = src
        .find("task0003 (L3): convert a wheel delta")
        .expect("wheel_report_notches doc comment anchor not found in input_translate.rs");
    let start = src
        .find("pub(super) fn wheel_report_notches(lines: f32) -> i32 {")
        .expect("wheel_report_notches not found in input_translate.rs");
    let end = src[start..]
        .find("pub(super) fn accumulate_wheel_report_lines")
        .map(|i| start + i)
        .expect("accumulate_wheel_report_lines marker not found after wheel_report_notches");
    let body = &src[start..end];
    assert!(
        body.contains("accumulate_wheel_report_lines("),
        "D11: wheel_report_notches must delegate to accumulate_wheel_report_lines, not \
         restate the non-finite rejection / saturation / truncation / sign rule itself"
    );
    let preamble_and_body = &src[doc_start..end];
    assert!(
        !preamble_and_body.contains("#[allow(dead_code)]"),
        "AC-6: no dead-code allowance may remain on wheel_report_notches"
    );
}

/// Edge case (Test Notes): a rejected event arriving mid-run of sub-notch
/// deltas must not perturb the run — it resumes exactly where it was, not
/// restarted and not skipped.
#[test]
fn rejected_event_mid_run_of_sub_notch_deltas_leaves_the_run_untouched() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_wheel_report_step, decide_wheel_event,
    };

    let mut records = MouseReportRecords {
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let mut dest = Vec::new();

    // Two Report-disposition sub-notch deltas.
    for _ in 0..2 {
        let inputs = WheelEventInputs {
            kind: MouseEventKind::WheelUp,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: false,
            alt_scroll_mode_bit: false,
            alt_scroll_setting: false,
            records,
        };
        let outcome = decide_wheel_event(&inputs);
        apply_wheel_report_step(outcome, &mut records, &mut dest, 0.3);
    }
    let acc_before_rejection = records.report_accum;
    assert!((acc_before_rejection - 0.6).abs() < 1.0e-5);

    // A grid-rejected event, with a delta that would cross a notch
    // boundary if it were (wrongly) folded.
    let rejected_inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs {
            in_title_bar_band: true,
            ..GridOwnershipInputs::default()
        },
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let rejected_outcome = decide_wheel_event(&rejected_inputs);
    let notches = apply_wheel_report_step(rejected_outcome, &mut records, &mut dest, 5.0);
    assert_eq!(notches, 0);
    assert_eq!(
        records.report_accum, acc_before_rejection,
        "the rejected event must not perturb the run's accumulator"
    );

    // The run resumes: one more 0.3 delta crosses the notch boundary
    // exactly where it would have without the rejected event in between.
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let outcome = decide_wheel_event(&inputs);
    let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 0.3);
    let (expected_notches, expected_frac) = accumulate_wheel_report_lines(acc_before_rejection, 0.3);
    assert_eq!(notches, expected_notches, "the run must resume, not restart or skip");
    assert_eq!(records.report_accum, expected_frac);
}

/// Edge case (Test Notes): a local-arm event arriving mid-run of sub-notch
/// deltas must not perturb the run either — same property as the rejected-
/// event case above, for a local arm instead.
#[test]
fn local_arm_event_mid_run_of_sub_notch_deltas_leaves_the_run_untouched() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_wheel_report_step, decide_wheel_event,
    };

    let mut records = MouseReportRecords {
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let mut dest = Vec::new();

    for _ in 0..2 {
        let inputs = WheelEventInputs {
            kind: MouseEventKind::WheelUp,
            grid: GridOwnershipInputs::default(),
            mods: Modifiers::NONE,
            mode_1000: false,
            mode_1002: true,
            mode_1003: false,
            encoding: MouseReportEncoding::Sgr,
            active_tab: 0,
            column: 5,
            row: 5,
            on_alt_screen: false,
            alt_scroll_mode_bit: false,
            alt_scroll_setting: false,
            records,
        };
        let outcome = decide_wheel_event(&inputs);
        apply_wheel_report_step(outcome, &mut records, &mut dest, 0.3);
    }
    let acc_before_local_arm = records.report_accum;

    // A local-arm event: tracking active, Shift held -> ScrollScrollback.
    // Tab and tracking observation both match the records, so `reset`
    // does not fire — isolating the fold-suppression property.
    let local_inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let local_outcome = decide_wheel_event(&local_inputs);
    assert!(
        !local_outcome.updates.reset,
        "test setup: this local arm must not itself reset"
    );
    let notches = apply_wheel_report_step(local_outcome, &mut records, &mut dest, 5.0);
    assert_eq!(notches, 0);
    assert_eq!(
        records.report_accum, acc_before_local_arm,
        "the local-arm event must not perturb the run's accumulator"
    );

    // The run resumes.
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: true,
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 0,
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let outcome = decide_wheel_event(&inputs);
    let notches = apply_wheel_report_step(outcome, &mut records, &mut dest, 0.3);
    let (expected_notches, expected_frac) =
        accumulate_wheel_report_lines(acc_before_local_arm, 0.3);
    assert_eq!(notches, expected_notches, "the run must resume, not restart or skip");
    assert_eq!(records.report_accum, expected_frac);
}

/// Edge case (Test Notes): a tab change and a tracking release observed by
/// the SAME event triggers both `apply_outcome`'s `reset` branch and its
/// D10 discard branch on one call — one discard, not a double-discard
/// that corrupts the field (it is set, not decremented, so this is
/// idempotent by construction; this test pins that as an explicit
/// regression guard).
#[test]
fn tab_change_and_tracking_release_observed_by_the_same_event_discard_cleanly_once() {
    use mouse_report::{
        GridOwnershipInputs, MouseEventKind, MouseReportEncoding, MouseReportRecords,
        WheelEventInputs, apply_outcome, decide_wheel_event,
    };

    let mut records = MouseReportRecords {
        report_accum: 0.5,
        built_for_tab: Some(0),
        ..MouseReportRecords::default()
    };
    let inputs = WheelEventInputs {
        kind: MouseEventKind::WheelUp,
        grid: GridOwnershipInputs::default(),
        mods: Modifiers::NONE,
        mode_1000: false,
        mode_1002: false, // tracking observed inactive by this same event
        mode_1003: false,
        encoding: MouseReportEncoding::Sgr,
        active_tab: 1, // AND the active tab changed, observed by this same event
        column: 5,
        row: 5,
        on_alt_screen: false,
        alt_scroll_mode_bit: false,
        alt_scroll_setting: false,
        records,
    };
    let outcome = decide_wheel_event(&inputs);
    assert!(outcome.updates.reset, "test setup: the tab change must reset");
    assert_eq!(
        outcome.updates.tracking_active,
        Some(false),
        "test setup: this event must also observe tracking inactive"
    );

    let mut dest = Vec::new();
    apply_outcome(outcome, &mut records, &mut dest);
    assert_eq!(
        records.report_accum, 0.0,
        "both triggers firing on the same event must still leave a clean, single-discard zero"
    );
    assert!(records.report_accum.is_finite() && records.report_accum == 0.0);
}
