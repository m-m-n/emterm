use super::*;
use crate::ui::chrome::build_egui_fonts;
use std::time::Duration;

// ── task0006 AC-2: grid x-origin carries no sidebar term ───────────

/// Regression guard for the right-edge placement update:
/// `cell_metrics_px`'s `origin_x` computation must not read the
/// persistent mux-sidebar inset — only `grid_size`'s usable-WIDTH
/// computation may. Scans each function's own source text so a future
/// edit that moves the sidebar term back onto `origin_x` fails loudly.
#[test]
fn cell_metrics_px_origin_x_has_no_sidebar_term() {
    let src = include_str!("mod.rs");
    let start = src
        .find("fn cell_metrics_px(&self, app: &App)")
        .expect("marker `fn cell_metrics_px` not found in window_host.rs");
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
    let src = include_str!("mod.rs");
    let start = src
        .find("WindowEvent::MouseWheel { delta, .. } =>")
        .expect("MouseWheel arm not found in window_host.rs");
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
    let src = include_str!("mod.rs");
    let arm_start = src
        .find("WindowEvent::PointerButton { state, button, .. } =>")
        .expect("PointerButton arm not found in window_host.rs");
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
    let selection_start_pos = arm_body
        .find("(MouseButton::Left, ElementState::Pressed) => {")
        .expect("selection-start arm not found in the PointerButton handler");
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
    let src = include_str!("mod.rs");
    let press_start = src
        .find("WindowEvent::PointerButton { state, button, .. } =>")
        .expect("PointerButton arm not found in window_host.rs");
    let wheel_start = src
        .find("WindowEvent::MouseWheel { delta, .. } =>")
        .expect("MouseWheel arm not found in window_host.rs");
    assert!(
        press_start < wheel_start,
        "expected the PointerButton arm to appear before the MouseWheel arm"
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
    let src = include_str!("mod.rs");
    let arm_start = src
        .find("WindowEvent::PointerMoved { position, .. } =>")
        .expect("PointerMoved arm not found in window_host.rs");
    let arm_body = &src[arm_start..];
    let arm_end = arm_body
        .find("\n            WindowEvent::PointerButton")
        .expect("PointerButton arm not found after PointerMoved");
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
