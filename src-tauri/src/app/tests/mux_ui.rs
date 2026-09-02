use super::*;

// ── Phase 4-C (APC redesign): mux message routing ────────────────

/// TS-mux-msg-1: `App::on_mux_message` applies a `Snapshot` to the
/// target tab's `TerminalCore` via `reset_and_replay`. The grid
/// content visible afterward must reflect the replayed bytes.
#[test]
fn on_mux_message_snapshot_resets_and_replays_into_core() {
    use mux_ipc::protocol::{MessageType, MuxMessage};

    let mut app = App::new();
    app.spawn_initial_tab();

    // Prime the grid with something the snapshot must overwrite.
    {
        let tab = app.active_tab().unwrap();
        tab.core.lock().process_pty_data(b"BEFORE");
    }

    // Snapshot payload: clear + print "AFTER" at home.
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(b"\x1b[2J\x1b[H");
    payload.extend_from_slice(b"AFTER");

    let msg = MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id: 0,
        payload,
    };
    let changed = app.on_mux_message(0, msg);
    assert!(changed, "Snapshot must mark state changed");

    // The first 5 cells of row 0 should now spell A F T E R.
    let tab = app.active_tab().unwrap();
    let core = tab.core.lock();
    let row0: String = (0..5).map(|c| core.get_cell_char(c, 0)).collect();
    assert_eq!(row0, "AFTER");
}

// TS-mux-msg-2 (`on_mux_message_status_update_caches_payload_on_tab`)
// is replaced by `tabs.rs`'s
// `retired_status_update_opcode_is_ignored_by_gui_receive_path`
// (AC-3/TS2, mux-status-bar-removal task0001): the retired opcode
// can no longer be named through the typed `MuxMessage` API, so its
// tolerance is exercised as a raw byte frame at the GUI receive
// boundary instead of through `App::on_mux_message`.

/// `App::on_mux_message` with an out-of-range tab index is a no-op
/// (logs a warning) and never panics.
#[test]
fn on_mux_message_out_of_range_returns_false() {
    use mux_ipc::protocol::{MessageType, MuxMessage};

    let mut app = App::new();
    // No tabs spawned.
    let msg = MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id: 0,
        payload: b"hello".to_vec(),
    };
    assert!(!app.on_mux_message(0, msg));
}

// ── Phase 3/4: mux action dispatch (TS-12, TS-13, TS-14) ──────────

use crate::mux::prefix::PrefixAction;
use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};

/// An app with one tab seeded with `n` mux windows (panes 100+i, ids
/// 0..n, active 0) via a real Welcome message.
fn app_with_mux_windows(n: usize) -> App {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.on_mux_message(0, mux_welcome_message(n));
    app
}

fn active_idx(app: &App) -> usize {
    app.active_tab()
        .unwrap()
        .mux_group
        .as_ref()
        .unwrap()
        .active_index()
}

#[test]
fn dispatch_next_prev_wrap() {
    let mut app = app_with_mux_windows(3);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextWindow),
        MuxActionOutcome::Changed
    );
    assert_eq!(active_idx(&app), 1);
    app.dispatch_mux_action(PrefixAction::NextWindow);
    app.dispatch_mux_action(PrefixAction::NextWindow);
    assert_eq!(active_idx(&app), 0); // wrapped
    app.dispatch_mux_action(PrefixAction::PrevWindow);
    assert_eq!(active_idx(&app), 2); // wrapped backwards
}

// ── next-agent-window (mux-agent-tab-cycle task0001 AC-2 … AC-6) ──────
// `app_with_mux_windows(n)` seeds panes at ids 100 + i (mux_welcome_message).

/// AC-2/AC-3: with a subset of qualifying windows, repeated invocations
/// visit exactly the qualifying windows in display order, skipping
/// non-qualifying ones, and wrap back to the first once past the last.
#[test]
fn dispatch_next_agent_window_skips_non_qualifying_and_wraps() {
    let mut app = app_with_mux_windows(4); // panes 100,101,102,103
    let scope = crate::agent_status_model::ConnectionScope(app.tabs[0].stable_id);
    // Only windows 1 and 3 (panes 101, 103) qualify.
    app.agent_status.apply_daemon_update(
        scope,
        101,
        Some(crate::agent_status::AgentState::Working),
        None,
        1,
        false,
    );
    app.agent_status.apply_daemon_update(
        scope,
        103,
        Some(crate::agent_status::AgentState::Idle),
        None,
        1,
        false,
    );

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::Changed
    );
    assert_eq!(active_idx(&app), 1, "skips non-qualifying window 0");

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::Changed
    );
    assert_eq!(active_idx(&app), 3, "skips non-qualifying window 2");

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::Changed
    );
    assert_eq!(
        active_idx(&app),
        1,
        "wraps back to the first qualifying window"
    );
}

/// AC-4: with exactly one qualifying window, invocation lands on (or
/// stays on) that window and never activates a non-qualifying window.
#[test]
fn dispatch_next_agent_window_single_qualifying_stays_put() {
    let mut app = app_with_mux_windows(3); // panes 100,101,102
    let scope = crate::agent_status_model::ConnectionScope(app.tabs[0].stable_id);
    app.agent_status.apply_daemon_update(
        scope,
        101,
        Some(crate::agent_status::AgentState::Blocked),
        None,
        1,
        false,
    );
    // Move onto the qualifying window first.
    app.dispatch_mux_action(PrefixAction::SelectWindow(1));
    assert_eq!(active_idx(&app), 1);

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::None,
        "already-active sole qualifier must not trigger a SwitchWindow + snapshot replay"
    );
    assert_eq!(active_idx(&app), 1, "stays on the only qualifying window");

    // From a different starting window, still lands on the only qualifier.
    app.dispatch_mux_action(PrefixAction::SelectWindow(0));
    assert_eq!(active_idx(&app), 0);
    app.dispatch_mux_action(PrefixAction::NextAgentWindow);
    assert_eq!(active_idx(&app), 1);
}

/// AC-5: with zero qualifying windows, the active window does not change.
#[test]
fn dispatch_next_agent_window_zero_qualifying_is_noop() {
    let mut app = app_with_mux_windows(3);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::None
    );
    assert_eq!(active_idx(&app), 0);
}

/// AC-6: with a non-mux GUI tab active, the action changes nothing —
/// the existing dispatch guard applies with no new mechanism.
#[test]
fn dispatch_next_agent_window_non_mux_tab_is_noop() {
    let mut app = App::new();
    app.spawn_initial_tab();
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::None
    );
}

/// AC-7: qualification follows the any-reported-state predicate —
/// cleared panes do not keep a window qualified.
#[test]
fn dispatch_next_agent_window_ignores_cleared_status() {
    let mut app = app_with_mux_windows(2); // panes 100,101
    let scope = crate::agent_status_model::ConnectionScope(app.tabs[0].stable_id);
    app.agent_status.apply_daemon_update(
        scope,
        101,
        Some(crate::agent_status::AgentState::Working),
        None,
        1,
        false,
    );
    app.agent_status
        .apply_daemon_update(scope, 101, None, None, 2, false); // cleared

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextAgentWindow),
        MuxActionOutcome::None,
        "the only reporting pane was cleared: no window qualifies"
    );
    assert_eq!(active_idx(&app), 0);
}

#[test]
fn dispatch_digit_clamps_and_noops_past_range() {
    let mut app = app_with_mux_windows(3);
    app.dispatch_mux_action(PrefixAction::SelectWindow(2));
    assert_eq!(active_idx(&app), 2);
    // digit 5 is past range → no-op, stays on 2.
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(5)),
        MuxActionOutcome::None
    );
    assert_eq!(active_idx(&app), 2);
}

// ── FR4 / TS-9: mux window switch raises scroll-into-view flag ──────────

#[test]
fn ts9_mux_window_switch_sets_scroll_into_view_only_on_real_change() {
    // TS-9 (option b — strict): a committed window change (next/prev/digit
    // that actually moves `active`) raises the one-shot scroll-into-view
    // flag; switching to the already-active window via a same-window digit
    // jump reports `Changed` but does not move `active`, so the flag stays
    // down. We chose the same-index guard (compare active_index before vs
    // after) so TS-9's "already-active does not set it" holds for the
    // digit path too, not only next/prev.
    let mut app = app_with_mux_windows(3);
    assert!(!app.scroll_active_tab_into_view());

    // NextWindow moves 0 → 1: flag set.
    app.dispatch_mux_action(PrefixAction::NextWindow);
    assert_eq!(active_idx(&app), 1);
    assert!(
        app.scroll_active_tab_into_view(),
        "NextWindow moved active window → flag set"
    );

    // PrevWindow moves 1 → 0: flag set.
    app.clear_scroll_active_tab_into_view();
    app.dispatch_mux_action(PrefixAction::PrevWindow);
    assert_eq!(active_idx(&app), 0);
    assert!(
        app.scroll_active_tab_into_view(),
        "PrevWindow moved active window → flag set"
    );

    // SelectWindow(2) moves 0 → 2: flag set.
    app.clear_scroll_active_tab_into_view();
    app.dispatch_mux_action(PrefixAction::SelectWindow(2));
    assert_eq!(active_idx(&app), 2);
    assert!(
        app.scroll_active_tab_into_view(),
        "digit jump to a different window → flag set"
    );

    // SelectWindow(2) again — already on window 2. dispatch reports
    // `Changed` (no same-index short-circuit before the SwitchWindow send),
    // but `active` does not move, so the strict guard keeps the flag down.
    app.clear_scroll_active_tab_into_view();
    app.dispatch_mux_action(PrefixAction::SelectWindow(2));
    assert_eq!(active_idx(&app), 2);
    assert!(
        !app.scroll_active_tab_into_view(),
        "same-window digit jump must NOT set the flag (TS-9 strict)"
    );
}

#[test]
fn ts9_mux_single_window_switch_does_not_set_flag() {
    // With <2 windows, next/prev return None (no switch); the flag stays
    // down.
    let mut app = app_with_mux_windows(1);
    app.clear_scroll_active_tab_into_view();
    app.dispatch_mux_action(PrefixAction::NextWindow);
    assert!(
        !app.scroll_active_tab_into_view(),
        "single-window next is a no-op → flag stays down"
    );
}

#[test]
fn dispatch_single_window_switch_is_noop() {
    let mut app = app_with_mux_windows(1);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextWindow),
        MuxActionOutcome::None
    );
    assert_eq!(active_idx(&app), 0);
}

// ── task0003: toggle-window-sidebar + overlay-open flag ──────────────

/// Clone `app.settings` with `mux.window_sidebar_overlay` forced to
/// `overlay`. Test-only helper — production loading of this field is
/// out of this task's scope (see the field doc in `settings.rs`).
fn with_overlay_mode(app: &mut App, overlay: bool) {
    let mut settings = (*app.settings).clone();
    settings.mux.window_sidebar_overlay = overlay;
    app.settings = std::sync::Arc::new(settings);
}

#[test]
fn ac2_toggle_round_trips_when_overlay_mode_enabled() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);

    // task0002 AC-7: the runtime flag now starts open by default.
    assert!(app.mux_sidebar_overlay_open());
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::ToggleWindowSidebar),
        MuxActionOutcome::None
    );
    assert!(!app.mux_sidebar_overlay_open());
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::ToggleWindowSidebar),
        MuxActionOutcome::None
    );
    assert!(app.mux_sidebar_overlay_open(), "round-trip back to initial");
}

#[test]
fn ac3_toggle_is_strict_noop_when_overlay_mode_disabled() {
    // Persistent mode: explicitly forced (task0001 flipped the
    // settings default to overlay, so this can no longer rely on the
    // ambient default).
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, false);
    assert!(!app.settings.mux.window_sidebar_overlay);

    let idx_before = active_idx(&app);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::ToggleWindowSidebar),
        MuxActionOutcome::None
    );
    assert!(
        // task0002 AC-7: the flag starts open (unconditionally, even in
        // persistent mode where it drives no rendering) and the
        // no-op toggle must leave it untouched.
        app.mux_sidebar_overlay_open(),
        "persistent mode: toggle is a no-op, flag stays at its initial value"
    );
    assert_eq!(active_idx(&app), idx_before, "no window switch side effect");
}

#[test]
fn mux_sidebar_overlay_resets_when_focused_tabs_mux_group_tears_down() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    // task0002 AC-7: already open by construction — no toggle needed.
    assert!(app.mux_sidebar_overlay_open());

    // Establish the "focused tab is mux-attached" baseline pump_all
    // records for the reset comparison.
    app.pump_all();
    assert!(app.mux_sidebar_overlay_open());

    // Daemon confirms detach: the focused tab's mux group tears down
    // (mirrors `Tab::apply_mux_message`'s `Detached` arm, which
    // production reaches via `Tab::pump` decoding the same message).
    app.on_mux_message(
        0,
        MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        },
    );
    app.pump_all();

    assert!(
        !app.mux_sidebar_overlay_open(),
        "flag resets once the focused tab's mux group tears down"
    );
}

#[test]
fn mux_sidebar_overlay_survives_switching_away_from_the_mux_tab() {
    // Switching the active tab away from a mux-attached tab must NOT
    // reset the flag — only an actual teardown of the FOCUSED tab's
    // group does (see the `active_mux_attached_prev_pump` field doc).
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    // task0002 AC-7: already open by construction — no toggle needed.
    assert!(app.mux_sidebar_overlay_open());
    app.pump_all();

    app.spawn_initial_tab(); // a second, plain-local tab
    app.active = app.tabs.len() - 1;
    app.pump_all();

    assert!(
        app.mux_sidebar_overlay_open(),
        "flag persists across a mere tab switch"
    );
}

// ── task0001: overlay reopens on the not-attached→attached transition ──

/// Build a `Welcome` message seeding `n` mux windows (same payload
/// shape as `app_with_mux_windows`), for delivery to an app that
/// already exists — attach/reattach tests need to construct the app
/// first and deliver the attach as a separate step.
fn mux_welcome_message(n: usize) -> MuxMessage {
    let windows: Vec<WindowInfo> = (0..n)
        .map(|i| WindowInfo {
            id: i as u32,
            name: format!("w{i}"),
            active_pane_id: 100 + i as u32,
        })
        .collect();
    MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: n as u32,
                pane_count: n as u32,
                active_window_index: 0,
                windows,
            }],
        },
    )
}

#[test]
fn ac1_overlay_reopens_on_the_not_attached_to_attached_transition() {
    // AC-1: plain (non-mux) tab, overlay mode forced on, flag forced
    // closed, pumped once to establish the not-attached baseline; a
    // real Welcome attach delivered and pumped must reopen the flag.
    let mut app = App::new();
    app.spawn_initial_tab();
    with_overlay_mode(&mut app, true);
    app.mux_sidebar_overlay_open = false;
    app.pump_all();
    assert!(
        !app.mux_sidebar_overlay_open(),
        "not-attached baseline: flag stays closed"
    );

    app.on_mux_message(0, mux_welcome_message(1));
    app.pump_all();

    assert!(
        app.mux_sidebar_overlay_open(),
        "AC-1: not-attached -> attached transition reopens the overlay"
    );
}

#[test]
fn ac2_overlay_reopens_on_reattach_after_an_explicit_close() {
    // AC-2 (FR3, accepted per IMPLEMENTATION.md D2): attach, explicit
    // close via the toggle action, a Detached reply (flag stays
    // closed), then a second attach reopens the flag.
    let mut app = app_with_mux_windows(1);
    with_overlay_mode(&mut app, true);
    app.pump_all();
    assert!(app.mux_sidebar_overlay_open());

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::ToggleWindowSidebar),
        MuxActionOutcome::None
    );
    assert!(!app.mux_sidebar_overlay_open(), "explicitly closed");

    app.on_mux_message(
        0,
        MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        },
    );
    app.pump_all();
    assert!(
        !app.mux_sidebar_overlay_open(),
        "detach does not reopen an already-closed flag"
    );

    app.on_mux_message(0, mux_welcome_message(1));
    app.pump_all();

    assert!(
        app.mux_sidebar_overlay_open(),
        "AC-2: reattach after an explicit close reopens the overlay"
    );
}

#[test]
fn ac1_pristine_startup_reopens_overlay_via_transient_detach_without_manual_flag_reset() {
    // Reproduces the pristine-startup path the task's root-cause
    // diagnosis describes (a transient detach fires during the startup
    // sequence, closing the flag via the existing detach guard, before
    // the real attach completes) end-to-end via `on_mux_message` +
    // `pump_all` — the production path. Unlike
    // `ac1_overlay_reopens_on_the_not_attached_to_attached_transition`,
    // this does NOT force `mux_sidebar_overlay_open = false` by direct
    // field assignment; the flag starts open at construction (AC-7) and
    // every transition below is driven through real mux messages.
    let mut app = App::new();
    app.spawn_initial_tab();
    with_overlay_mode(&mut app, true);
    assert!(app.mux_sidebar_overlay_open(), "AC-7: open by construction");

    // The initial attach completing during startup.
    app.on_mux_message(0, mux_welcome_message(1));
    app.pump_all();
    assert!(app.mux_sidebar_overlay_open());

    // A transient detach fires during startup (per this task's
    // root-cause diagnosis), tearing the mux group down and closing the
    // flag via the existing detach guard.
    app.on_mux_message(
        0,
        MuxMessage {
            msg_type: MessageType::Detached,
            pane_id: 0,
            payload: Vec::new(),
        },
    );
    app.pump_all();
    assert!(
        !app.mux_sidebar_overlay_open(),
        "the transient detach closes the flag via the detach guard"
    );

    // The real attach completes.
    app.on_mux_message(0, mux_welcome_message(1));
    app.pump_all();

    assert!(
        app.mux_sidebar_overlay_open(),
        "pristine startup (attach -> transient detach -> re-attach) \
             reopens the overlay without any manual flag reset"
    );
}

#[test]
fn ac3_overlay_stays_closed_across_further_pumps_while_continuously_attached() {
    // AC-3 (FR1 negative): the bookkeeping already records the active
    // tab as attached; an explicit close followed by further pumps
    // must NOT reopen the flag — the rule fires only on the
    // not-attached -> attached transition, never on a steady attached
    // state.
    let mut app = app_with_mux_windows(1);
    with_overlay_mode(&mut app, true);
    app.pump_all();
    assert!(app.mux_sidebar_overlay_open());

    assert_eq!(
        app.dispatch_mux_action(PrefixAction::ToggleWindowSidebar),
        MuxActionOutcome::None
    );
    assert!(!app.mux_sidebar_overlay_open());

    app.pump_all();
    assert!(
        !app.mux_sidebar_overlay_open(),
        "AC-3: steady attached state does not reopen the flag"
    );
    app.pump_all();
    assert!(
        !app.mux_sidebar_overlay_open(),
        "AC-3: repeated pumps stay closed"
    );
}

// ── task0002: mux sidebar overlay auto-dim ──────────────────────────

// AC-1: no hover, no switch => idle; hover => full regardless of
// switch; switch just now, no hover => full.

#[test]
fn ac1_resolver_idle_opacity_when_no_hover_and_no_switch() {
    let now = Instant::now();
    let (opacity, animating) = resolve_mux_sidebar_dim_opacity(false, None, None, now);
    assert_eq!(opacity, OVERLAY_IDLE_OPACITY);
    assert!(!animating);
}

#[test]
fn ac1_resolver_full_opacity_when_hovered_regardless_of_switch_timestamp() {
    let now = Instant::now();
    // A switch far enough in the past that its hold+fade already
    // elapsed — hover must still win.
    let long_ago = now
        .checked_sub(OVERLAY_BRIGHT_HOLD + OVERLAY_DIM_FADE + Duration::from_secs(1))
        .expect("test instant underflow — machine uptime too short for this offset");
    let (opacity, animating) = resolve_mux_sidebar_dim_opacity(true, Some(long_ago), None, now);
    assert_eq!(opacity, 1.0);
    assert!(!animating);
}

#[test]
fn ac1_resolver_full_opacity_when_switch_recorded_just_now_no_hover() {
    let now = Instant::now();
    let (opacity, animating) = resolve_mux_sidebar_dim_opacity(false, Some(now), None, now);
    assert_eq!(opacity, 1.0);
    assert!(!animating);
}

// AC-2: switch older than hold+fade => exactly idle; mid-fade =>
// strictly between; entering bright => full immediately, no
// interpolation.

#[test]
fn ac2_resolver_idle_exactly_after_hold_plus_fade_elapsed() {
    let now = Instant::now();
    let switch = now
        .checked_sub(OVERLAY_BRIGHT_HOLD + OVERLAY_DIM_FADE + Duration::from_millis(1))
        .expect("test instant underflow — machine uptime too short for this offset");
    let fade_started = switch + OVERLAY_BRIGHT_HOLD;
    let (opacity, animating) =
        resolve_mux_sidebar_dim_opacity(false, Some(switch), Some(fade_started), now);
    assert_eq!(opacity, OVERLAY_IDLE_OPACITY);
    assert!(!animating);
}

#[test]
fn ac2_resolver_midfade_value_strictly_between_idle_and_full() {
    let now = Instant::now();
    let fade_started = now - OVERLAY_DIM_FADE / 2;
    let (opacity, animating) =
        resolve_mux_sidebar_dim_opacity(false, None, Some(fade_started), now);
    assert!(
        opacity > OVERLAY_IDLE_OPACITY && opacity < 1.0,
        "expected a strictly mid-fade value, got {opacity}"
    );
    assert!(animating);
}

#[test]
fn ac2_resolver_bright_state_is_immediate_no_interpolation() {
    let now = Instant::now();
    // Even with a fade already tracked (as if mid-dim a moment ago),
    // hover must snap straight to full with no partial value.
    let fade_started = now - OVERLAY_DIM_FADE / 2;
    let (opacity, animating) = resolve_mux_sidebar_dim_opacity(true, None, Some(fade_started), now);
    assert_eq!(opacity, 1.0);
    assert!(!animating);
}

// AC-3: a second switch inside the hold window extends brightness past
// the first switch's own would-be expiry; releasing hover during a
// pending hold keeps the card bright until the hold itself expires.

#[test]
fn ac3_second_switch_extends_brightness_past_the_first_switchs_expiry() {
    let switch1 = Instant::now();
    let switch2 = switch1 + Duration::from_secs(1); // well inside switch1's hold
    // Sample after switch1's own hold would have expired, but still
    // within switch2's hold.
    let sample_at = switch1 + OVERLAY_BRIGHT_HOLD + Duration::from_millis(500);
    let (opacity, animating) =
        resolve_mux_sidebar_dim_opacity(false, Some(switch2), None, sample_at);
    assert_eq!(
        opacity, 1.0,
        "switch2's hold should still be active at {sample_at:?}"
    );
    assert!(!animating);
}

#[test]
fn ac3_releasing_hover_during_pending_hold_keeps_bright_until_hold_expires() {
    let switch = Instant::now();
    // Hover is false (released) but the switch's hold has not expired.
    let sample_within_hold = switch + OVERLAY_BRIGHT_HOLD - Duration::from_millis(1);
    let (opacity, animating) =
        resolve_mux_sidebar_dim_opacity(false, Some(switch), None, sample_within_hold);
    assert_eq!(opacity, 1.0, "the hold keeps the card bright without hover");
    assert!(!animating);
    // Once the hold expires, brightness ends (the App arms the fade on
    // the next tick — the pure resolver, given no fade tracked yet,
    // reads this as already idle).
    let sample_after_hold = switch + OVERLAY_BRIGHT_HOLD + Duration::from_millis(1);
    let (opacity_after, _animating_after) =
        resolve_mux_sidebar_dim_opacity(false, Some(switch), None, sample_after_hold);
    assert_eq!(opacity_after, OVERLAY_IDLE_OPACITY);
}

// AC-4: output always clamped to 0.0..=1.0, including adversarial
// hold/fade origins (future, or far past).

#[test]
fn ac4_resolver_output_always_in_range_for_future_and_far_past_origins() {
    let now = Instant::now();
    let far_past = now
        .checked_sub(OVERLAY_BRIGHT_HOLD + OVERLAY_DIM_FADE + Duration::from_secs(2))
        .expect("test instant underflow — machine uptime too short for this offset");
    let far_future = now + Duration::from_secs(999);
    let cases: Vec<(bool, Option<Instant>, Option<Instant>)> = vec![
        (false, None, None),
        (false, Some(far_future), None), // switch recorded "in the future"
        (false, None, Some(far_future)), // fade origin "in the future"
        (false, Some(far_past), Some(far_past)),
        (true, Some(far_past), Some(far_future)),
    ];
    for (hovered, last_switch, fade_started) in cases {
        let (opacity, _animating) =
            resolve_mux_sidebar_dim_opacity(hovered, last_switch, fade_started, now);
        assert!(
            (0.0..=1.0).contains(&opacity),
            "opacity {opacity} out of range for hovered={hovered}, \
                 last_switch={last_switch:?}, fade_started={fade_started:?}"
        );
    }
}

// Boundary cases (Test Notes): exactly at the hold expiry, exactly at
// fade completion.

#[test]
fn boundary_resolver_exactly_at_hold_expiry_is_no_longer_bright() {
    let switch = Instant::now();
    let sample = switch + OVERLAY_BRIGHT_HOLD; // exactly the boundary
    let (opacity, _animating) = resolve_mux_sidebar_dim_opacity(false, Some(switch), None, sample);
    assert_eq!(
        opacity, OVERLAY_IDLE_OPACITY,
        "bright's `now < hold_end` is false exactly at the boundary"
    );
}

#[test]
fn boundary_resolver_exactly_at_fade_completion_is_idle_and_not_animating() {
    let now = Instant::now();
    let fade_started = now - OVERLAY_DIM_FADE; // elapsed == OVERLAY_DIM_FADE exactly
    let (opacity, animating) =
        resolve_mux_sidebar_dim_opacity(false, None, Some(fade_started), now);
    assert_eq!(opacity, OVERLAY_IDLE_OPACITY);
    assert!(
        !animating,
        "elapsed == OVERLAY_DIM_FADE must already be settled"
    );
}

#[test]
fn boundary_closing_overlay_mid_fade_then_reopening_resolves_to_a_defined_opacity() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    assert!(
        app.mux_sidebar_overlay_open(),
        "open by construction (AC-7)"
    );
    let now = Instant::now();
    app.mux_sidebar_fade_started = Some(now - OVERLAY_DIM_FADE / 2);
    app.mux_sidebar_overlay_open = false;
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Hidden);
    app.mux_sidebar_overlay_open = true;
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Overlay);
    let (opacity, _animating) = app.resolve_mux_sidebar_opacity(Instant::now());
    assert!(
        (0.0..=1.0).contains(&opacity),
        "reopening must resolve to a defined, in-range opacity — no stale/invalid fade state, got {opacity}"
    );
}

// AC-5: deadline provider — absent when settled or hovered, present
// while a hold is pending or a fade is running; absent at the App
// level when the overlay is not shown at all.

#[test]
fn ac5_deadline_absent_when_settled() {
    let now = Instant::now();
    let far_past = now
        .checked_sub(OVERLAY_BRIGHT_HOLD + OVERLAY_DIM_FADE + Duration::from_secs(1))
        .expect("test instant underflow — machine uptime too short for this offset");
    let deadline = resolve_mux_sidebar_dim_deadline(
        false,
        Some(far_past),
        Some(far_past + OVERLAY_BRIGHT_HOLD),
        now,
    );
    assert_eq!(deadline, None);
}

#[test]
fn ac5_deadline_absent_when_hovered() {
    let now = Instant::now();
    assert_eq!(
        resolve_mux_sidebar_dim_deadline(true, None, None, now),
        None
    );
}

#[test]
fn ac5_deadline_present_while_hold_pending() {
    let now = Instant::now();
    let switch = now;
    assert_eq!(
        resolve_mux_sidebar_dim_deadline(false, Some(switch), None, now),
        Some(switch + OVERLAY_BRIGHT_HOLD)
    );
}

#[test]
fn ac5_deadline_present_while_fade_in_flight() {
    let now = Instant::now();
    let fade_started = now - OVERLAY_DIM_FADE / 2;
    assert_eq!(
        resolve_mux_sidebar_dim_deadline(false, None, Some(fade_started), now),
        Some(fade_started + OVERLAY_DIM_FADE)
    );
}

#[test]
fn ac5_app_level_deadline_absent_when_overlay_not_shown() {
    let mut app = App::new();
    app.spawn_initial_tab(); // plain local tab — not mux-attached => Hidden
    app.mux_sidebar_last_switch = Some(Instant::now()); // would be a pending hold if shown
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Hidden);
    assert_eq!(app.next_mux_sidebar_dim_deadline(Instant::now()), None);
}

#[test]
fn ac5_app_level_dim_due_false_when_overlay_not_shown() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.mux_sidebar_last_switch = Some(Instant::now());
    assert!(!app.mux_sidebar_dim_due(Instant::now()));
}

#[test]
fn mux_sidebar_dim_due_false_while_hovered() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    app.set_mux_sidebar_hovered(true);
    assert!(!app.mux_sidebar_dim_due(Instant::now()));
}

#[test]
fn mux_sidebar_dim_due_false_while_hold_pending() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    app.mux_sidebar_last_switch = Some(Instant::now());
    assert!(!app.mux_sidebar_dim_due(Instant::now()));
}

#[test]
fn mux_sidebar_dim_due_true_while_fade_in_flight() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    let now = Instant::now();
    app.mux_sidebar_fade_started = Some(now - OVERLAY_DIM_FADE / 2);
    assert!(app.mux_sidebar_dim_due(now));
}

#[test]
fn mux_sidebar_dim_due_false_once_settled() {
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, true);
    let now = Instant::now();
    let far_past = now
        .checked_sub(OVERLAY_BRIGHT_HOLD + OVERLAY_DIM_FADE + Duration::from_secs(1))
        .expect("test instant underflow — machine uptime too short for this offset");
    app.mux_sidebar_fade_started = Some(far_past);
    assert!(!app.mux_sidebar_dim_due(now));
}

// AC-6: `dispatch_mux_action` records the switch timestamp for
// next / prev / select-by-digit; the sidebar-row-click path
// (`apply_tab_event`'s `MuxSwitch` arm) records it too.

#[test]
fn ac6_next_window_records_a_switch_timestamp() {
    let mut app = app_with_mux_windows(2);
    assert!(app.mux_sidebar_last_switch.is_none(), "precondition");
    let before = Instant::now();
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextWindow),
        MuxActionOutcome::Changed
    );
    let recorded = app
        .mux_sidebar_last_switch
        .expect("NextWindow must record a switch timestamp");
    assert!(recorded >= before);
}

#[test]
fn ac6_prev_window_records_a_switch_timestamp() {
    let mut app = app_with_mux_windows(2);
    assert!(app.mux_sidebar_last_switch.is_none(), "precondition");
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::PrevWindow),
        MuxActionOutcome::Changed
    );
    assert!(app.mux_sidebar_last_switch.is_some());
}

#[test]
fn ac6_select_window_by_digit_records_a_switch_timestamp() {
    let mut app = app_with_mux_windows(3);
    assert!(app.mux_sidebar_last_switch.is_none(), "precondition");
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(2)),
        MuxActionOutcome::Changed
    );
    assert!(app.mux_sidebar_last_switch.is_some());
}

#[test]
fn ac6_new_window_does_not_record_a_switch_timestamp() {
    // NewWindow reports `Changed` too but is explicitly excluded from
    // this feature (task0002 task plan scope) — appending a window is
    // not "switching to" one.
    let mut app = app_with_mux_windows(1);
    assert!(app.mux_sidebar_last_switch.is_none(), "precondition");
    let _ = app.dispatch_mux_action(PrefixAction::NewWindow);
    assert!(
        app.mux_sidebar_last_switch.is_none(),
        "NewWindow must not record a switch timestamp"
    );
}

#[test]
fn ac6_sidebar_row_click_via_apply_tab_event_records_a_switch_timestamp() {
    let mut app = app_with_mux_windows(2);
    assert!(app.mux_sidebar_last_switch.is_none(), "precondition");
    let before = Instant::now();
    let exit = app.apply_tab_event(crate::ui::TabEvent::MuxSwitch { tab: 0, window: 1 });
    assert!(!exit);
    let recorded = app
        .mux_sidebar_last_switch
        .expect("the sidebar row-click path must record a switch timestamp");
    assert!(recorded >= before);
}

// AC-7: the overlay's runtime open flag is open on a freshly
// constructed app state.

#[test]
fn ac7_mux_sidebar_overlay_open_on_a_freshly_constructed_app() {
    let app = App::new();
    assert!(app.mux_sidebar_overlay_open());
}

// AC-9: a hover-predicate transition is observable via
// `mux_sidebar_hover_changed` (window_host folds this into
// `overlay_work`) and clears once `record_render_state` snapshots it.

#[test]
fn ac9_hover_changed_true_after_a_transition_false_once_snapshotted() {
    let mut app = App::new();
    assert!(
        !app.mux_sidebar_hover_changed(),
        "no transition yet on a fresh app"
    );
    app.set_mux_sidebar_hovered(true);
    assert!(
        app.mux_sidebar_hover_changed(),
        "hover flipped from the construction-time snapshot"
    );
    app.record_render_state_no_tab();
    assert!(
        !app.mux_sidebar_hover_changed(),
        "settled again once the transition was actually rendered"
    );
}

#[test]
fn pump_all_scrolls_new_mux_window_into_view_on_active_tab() {
    // FR6 (mux), App-level integration: a daemon `PaneCreated` on the ACTIVE
    // mux tab raises scroll-into-view through `pump_all` — the path the
    // tabs.rs latch unit tests do not reach (the `idx == active` gating plus
    // the latch → `scroll_active_tab_into_view` conversion).
    let mut app = app_with_mux_windows(2); // tab 0 is mux and active
    app.clear_scroll_active_tab_into_view();
    // The daemon confirms a new window on the active tab; the push activates
    // it, and the PaneCreated handler latches the FR6 signal.
    app.on_mux_message(
        0,
        MuxMessage {
            msg_type: MessageType::PaneCreated,
            pane_id: 200,
            payload: Vec::new(),
        },
    );
    app.pump_all();
    assert!(
        app.scroll_active_tab_into_view(),
        "a PaneCreated on the active mux tab scrolls the new sub-tab into view"
    );
}

#[test]
fn pump_all_skips_scroll_for_background_tab_mux_window_and_drains_latch() {
    // FR6 (mux), App-level integration: a `PaneCreated` on a NON-active tab
    // must NOT raise scroll-into-view (the `idx == active` gate), and its
    // latch is still drained (drain-every-tab) so it cannot fire on a later
    // pump. This locks both invariants the unit tests cannot reach.
    let mut app = app_with_mux_windows(2); // tab 0 is mux, active = 0
    app.spawn_new_tab(); // tab 1 becomes active
    app.clear_scroll_active_tab_into_view(); // spawn_new_tab raises it (FR6)
    assert_eq!(app.active, 1);
    // A new window is appended to the BACKGROUND mux tab (tab 0).
    app.on_mux_message(
        0,
        MuxMessage {
            msg_type: MessageType::PaneCreated,
            pane_id: 200,
            payload: Vec::new(),
        },
    );
    app.pump_all();
    assert!(
        !app.scroll_active_tab_into_view(),
        "a background-tab window add must not scroll the active tab"
    );
    // The background latch was drained, not stranded: a later pump with no
    // new event keeps the flag down.
    app.pump_all();
    assert!(
        !app.scroll_active_tab_into_view(),
        "the drained latch does not resurface on a subsequent pump"
    );
}

// ── TS-5 / TS-6 / TS-7 (pane): local pane-switch scroll save/restore (FR3) ──

#[test]
fn local_pane_switch_round_trip_restores_scroll_position() {
    // TS-5 (local switch path): scroll up in pane A, switch to B, return
    // to A — A's saved offset is restored; B is unaffected (Live).
    let mut app = app_with_mux_windows(2);
    assert_eq!(active_idx(&app), 0);

    // Scroll up in pane A (index 0), then switch to pane B (index 1).
    app.scroll_position = ScrollPosition::OffsetFromLive(15);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
        MuxActionOutcome::Changed
    );
    assert_eq!(active_idx(&app), 1);
    assert_eq!(
        app.scroll_position,
        ScrollPosition::Live,
        "incoming pane B restores to its own (Live) position"
    );
    assert!(app.needs_full_redraw, "pane switch forces a full redraw");

    // Return to pane A (index 0): its saved offset comes back.
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(0)),
        MuxActionOutcome::Changed
    );
    assert_eq!(active_idx(&app), 0);
    assert_eq!(
        app.scroll_position,
        ScrollPosition::OffsetFromLive(15),
        "returning to pane A restores A's saved offset"
    );
}

#[test]
fn local_pane_switch_all_live_introduces_no_scroll() {
    // TS-7 (pane): all panes at Live → switching introduces no scroll.
    let mut app = app_with_mux_windows(2);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
        MuxActionOutcome::Changed
    );
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(0)),
        MuxActionOutcome::Changed
    );
    assert_eq!(app.scroll_position, ScrollPosition::Live);
}

#[test]
fn local_pane_switch_with_empty_scrollback_does_not_crash() {
    // TS-6 (switch side): switching to a pane whose shared core has no
    // scrollback succeeds and leaves the active scroll value at the
    // incoming (Live) pane's saved position with no panic.
    let mut app = app_with_mux_windows(2);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::SelectWindow(1)),
        MuxActionOutcome::Changed
    );
    assert_eq!(app.scroll_position, ScrollPosition::Live);
    assert_eq!(app.scroll_offset(), 0);
}

#[test]
fn local_pane_switch_forces_full_redraw() {
    // FR2: a committed pane switch sets the renderer's full-redraw flag so
    // a shorter incoming pane leaves no residual rows.
    let mut app = app_with_mux_windows(2);
    app.needs_full_redraw = false;
    app.dispatch_mux_action(PrefixAction::SelectWindow(1));
    assert!(app.needs_full_redraw);
}

#[test]
fn local_pane_switch_noop_does_not_touch_scroll_or_redraw() {
    // NFR1: a no-op switch (single window) leaves scroll + redraw flag
    // untouched (scroll-pin / single-window mux unaffected).
    let mut app = app_with_mux_windows(1);
    app.scroll_position = ScrollPosition::OffsetFromLive(9);
    app.needs_full_redraw = false;
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextWindow),
        MuxActionOutcome::None
    );
    assert_eq!(app.scroll_position, ScrollPosition::OffsetFromLive(9));
    assert!(!app.needs_full_redraw);
}

#[test]
fn dispatch_new_window_increments_pending() {
    let mut app = app_with_mux_windows(2);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NewWindow),
        MuxActionOutcome::Changed
    );
    assert_eq!(
        app.active_tab()
            .unwrap()
            .mux_group
            .as_ref()
            .unwrap()
            .pending_create(),
        1
    );
}

#[test]
fn dispatch_detach_emits_detach_outcome() {
    let mut app = app_with_mux_windows(2);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::Detach),
        MuxActionOutcome::Detach
    );
}

#[test]
fn dispatch_rename_opens_dialog_with_stable_id() {
    let mut app = app_with_mux_windows(2);
    app.dispatch_mux_action(PrefixAction::NextWindow); // active idx 1, id 1
    match app.dispatch_mux_action(PrefixAction::RenameWindow) {
        MuxActionOutcome::OpenRename {
            window_id,
            current_name,
        } => {
            assert_eq!(window_id, 1);
            assert_eq!(current_name, "w1");
        }
        other => panic!("expected OpenRename, got {other:?}"),
    }
}

#[test]
fn dispatch_move_requires_two_windows() {
    let mut app = app_with_mux_windows(1);
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::MoveWindow),
        MuxActionOutcome::None
    );
    let mut app = app_with_mux_windows(2);
    match app.dispatch_mux_action(PrefixAction::MoveWindow) {
        MuxActionOutcome::OpenMove {
            window_id,
            current_position,
            window_count,
        } => {
            assert_eq!(window_id, 0);
            assert_eq!(current_position, 1);
            assert_eq!(window_count, 2);
        }
        other => panic!("expected OpenMove, got {other:?}"),
    }
}

#[test]
fn dispatch_without_mux_group_is_noop() {
    let mut app = App::new();
    app.spawn_initial_tab();
    assert_eq!(
        app.dispatch_mux_action(PrefixAction::NextWindow),
        MuxActionOutcome::None
    );
}

// ── FR1: tab-bar mux sub-tab click routing (MuxSwitch) ────────────────

#[test]
fn apply_tab_event_mux_switch_moves_active_window() {
    let mut app = app_with_mux_windows(3);
    assert_eq!(active_idx(&app), 0);
    let exit = app.apply_tab_event(crate::ui::TabEvent::MuxSwitch { tab: 0, window: 2 });
    assert!(!exit);
    assert_eq!(active_idx(&app), 2);
}

#[test]
fn apply_tab_event_mux_switch_on_missing_tab_is_noop() {
    let mut app = app_with_mux_windows(2);
    // Out-of-range tab index must not panic and must leave state intact.
    assert!(!app.apply_tab_event(crate::ui::TabEvent::MuxSwitch { tab: 9, window: 1 }));
    assert_eq!(active_idx(&app), 0);
}

// ── task0005 AC-6: mux_sidebar_visibility matrix ───────────────────────

#[test]
fn sidebar_hidden_on_local_tab_regardless_of_mode() {
    let mut app = App::new();
    app.spawn_initial_tab();
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Hidden);
    app.settings = Arc::new({
        let mut s = (*app.settings).clone();
        s.mux.window_sidebar_overlay = true;
        s
    });
    app.mux_sidebar_overlay_open = true;
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Hidden);
}

#[test]
fn sidebar_persistent_when_mux_attached_and_overlay_mode_off() {
    // Explicitly forced off (task0001 flipped the settings default to
    // overlay, so this can no longer rely on the ambient default).
    let mut app = app_with_mux_windows(2);
    with_overlay_mode(&mut app, false);
    assert!(!app.settings.mux.window_sidebar_overlay);
    assert_eq!(
        app.mux_sidebar_visibility(),
        MuxSidebarVisibility::Persistent
    );
}

#[test]
fn sidebar_overlay_when_mux_attached_mode_on_and_flag_open() {
    let mut app = app_with_mux_windows(2);
    app.settings = Arc::new({
        let mut s = (*app.settings).clone();
        s.mux.window_sidebar_overlay = true;
        s
    });
    app.mux_sidebar_overlay_open = true;
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Overlay);
}

#[test]
fn sidebar_hidden_when_mux_attached_mode_on_but_flag_closed() {
    let mut app = app_with_mux_windows(2);
    app.settings = Arc::new({
        let mut s = (*app.settings).clone();
        s.mux.window_sidebar_overlay = true;
        s
    });
    // task0002 AC-7 flipped the flag's default to open; set it closed
    // explicitly here since that is the scenario this test exercises.
    app.mux_sidebar_overlay_open = false;
    assert!(!app.mux_sidebar_overlay_open);
    assert_eq!(app.mux_sidebar_visibility(), MuxSidebarVisibility::Hidden);
}

// ── task0005 AC-3/AC-4: mux_sidebar_grid_inset pure math ───────────────

#[test]
fn grid_inset_zero_when_hidden() {
    assert_eq!(
        mux_sidebar_grid_inset(MuxSidebarVisibility::Hidden, 1000.0),
        0.0
    );
}

#[test]
fn grid_inset_zero_when_overlay_regardless_of_width() {
    assert_eq!(
        mux_sidebar_grid_inset(MuxSidebarVisibility::Overlay, 1000.0),
        0.0
    );
}

#[test]
fn grid_inset_equals_width_fn_when_persistent() {
    let inset = mux_sidebar_grid_inset(MuxSidebarVisibility::Persistent, 1000.0);
    assert_eq!(inset, crate::ui::mux_sidebar::sidebar_width(1000.0));
    assert!(inset > 0.0);
}

#[test]
fn x_inset_helper_matches_free_function() {
    let app = app_with_mux_windows(2);
    let width = 900.0;
    assert_eq!(
        app.mux_sidebar_x_inset(width),
        mux_sidebar_grid_inset(app.mux_sidebar_visibility(), width)
    );
}

// ── TS-14: rename confirm re-resolves by stable id ────────────────

#[test]
fn confirm_rename_relabels_by_stable_id() {
    let mut app = app_with_mux_windows(3);
    assert!(app.confirm_mux_rename(2, "editor".to_string()));
    let g = app.active_tab().unwrap().mux_group.as_ref().unwrap();
    assert_eq!(g.windows()[2].name, "editor");
}

#[test]
fn confirm_rename_empty_name_is_noop() {
    let mut app = app_with_mux_windows(2);
    assert!(!app.confirm_mux_rename(0, String::new()));
}

#[test]
fn confirm_rename_closed_window_aborts() {
    let mut app = app_with_mux_windows(2);
    // window id 999 never existed → abort.
    assert!(!app.confirm_mux_rename(999, "x".to_string()));
}

// ── TS-13: move validation + optimistic reorder ───────────────────

#[test]
fn confirm_move_reorders_optimistically() {
    let mut app = app_with_mux_windows(3); // ids 0,1,2 panes 100,101,102
    // move window id 0 to position 3 → order 1,2,0
    assert!(app.confirm_mux_move(0, 3));
    let g = app.active_tab().unwrap().mux_group.as_ref().unwrap();
    assert_eq!(
        g.windows().iter().map(|w| w.id).collect::<Vec<_>>(),
        vec![1, 2, 0]
    );
}

#[test]
fn confirm_move_out_of_range_is_noop() {
    let mut app = app_with_mux_windows(3);
    assert!(!app.confirm_mux_move(0, 0)); // below range
    assert!(!app.confirm_mux_move(0, 4)); // above range
}

#[test]
fn confirm_move_same_position_is_noop() {
    let mut app = app_with_mux_windows(3); // active id 0 at position 1
    assert!(!app.confirm_mux_move(0, 1));
}

#[test]
fn confirm_move_closed_window_aborts() {
    let mut app = app_with_mux_windows(3);
    assert!(!app.confirm_mux_move(999, 2));
}

#[test]
fn confirm_move_rolls_back_on_send_failure() {
    let mut app = app_with_mux_windows(3); // ids 0,1,2
    // Drop the PTY so send_control fails.
    app.active_tab_mut().unwrap().pty = None;
    let before: Vec<u32> = app
        .active_tab()
        .unwrap()
        .mux_group
        .as_ref()
        .unwrap()
        .windows()
        .iter()
        .map(|w| w.id)
        .collect();
    // Attempt move id 0 → position 3; send fails → reverted.
    assert!(!app.confirm_mux_move(0, 3));
    let after: Vec<u32> = app
        .active_tab()
        .unwrap()
        .mux_group
        .as_ref()
        .unwrap()
        .windows()
        .iter()
        .map(|w| w.id)
        .collect();
    assert_eq!(before, after, "order reverted after send failure");
}

// ── observe_mux_key latch wiring + dialog reentry ─────────────────

#[test]
fn observe_mux_key_ignores_non_mux_tab() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let t0 = Instant::now();
    let (consumed, _) = app.observe_mux_key(&crate::mux::prefix::KeyInput::letter('b'), t0);
    assert!(!consumed, "non-mux tab falls through");
}

#[test]
fn observe_mux_key_arms_then_dispatches() {
    let mut app = app_with_mux_windows(3);
    let t0 = Instant::now();
    let (consumed, out) = app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
    assert!(consumed);
    assert_eq!(out, MuxActionOutcome::None);
    let (consumed, out) = app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('n'), t0);
    assert!(consumed);
    assert_eq!(out, MuxActionOutcome::Changed);
    assert_eq!(active_idx(&app), 1);
}

#[test]
fn observe_mux_key_unknown_followup_consumed() {
    let mut app = app_with_mux_windows(2);
    let t0 = Instant::now();
    app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
    let (consumed, out) = app.observe_mux_key(&crate::mux::prefix::KeyInput::letter('q'), t0);
    assert!(consumed);
    assert_eq!(out, MuxActionOutcome::None);
}

#[test]
fn observe_mux_key_rename_opens_dialog() {
    let mut app = app_with_mux_windows(2);
    let t0 = Instant::now();
    app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('z'), t0);
    let (consumed, out) = app.observe_mux_key(&crate::mux::prefix::KeyInput::ctrl_letter('r'), t0);
    assert!(consumed);
    assert!(matches!(out, MuxActionOutcome::OpenRename { .. }));
    app.handle_mux_outcome(out);
    assert!(matches!(
        app.mux_dialog,
        crate::mux::dialog::MuxDialogState::Rename { .. }
    ));
}

#[test]
fn rename_dialog_reentry_guard() {
    let mut app = app_with_mux_windows(2);
    app.open_mux_rename_dialog(0, "a".to_string());
    // Second open with a different id must not replace the first.
    app.open_mux_rename_dialog(1, "b".to_string());
    match &app.mux_dialog {
        crate::mux::dialog::MuxDialogState::Rename { window_id, .. } => {
            assert_eq!(*window_id, 0);
        }
        other => panic!("expected Rename dialog, got {other:?}"),
    }
}

#[test]
fn move_dialog_reentry_guard() {
    let mut app = app_with_mux_windows(2);
    app.open_mux_move_dialog(0, 1, 2);
    app.open_mux_move_dialog(1, 2, 2);
    match &app.mux_dialog {
        crate::mux::dialog::MuxDialogState::Move { window_id, .. } => {
            assert_eq!(*window_id, 0);
        }
        other => panic!("expected Move dialog, got {other:?}"),
    }
}
