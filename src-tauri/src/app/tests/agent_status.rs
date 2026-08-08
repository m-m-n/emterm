use super::*;
use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};

// ── Agent-status notifications (task0007) ───────────────────────

/// Capturing `NotificationSink` for [`App::maybe_notify_agent_transition`]
/// tests — mirrors `callbacks::tests::TestSink`.
#[derive(Default)]
struct TestNotifySink {
    calls: parking_lot::Mutex<Vec<(String, String)>>,
}

impl TestNotifySink {
    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().clone()
    }
}

impl NotificationSink for TestNotifySink {
    fn send(&self, title: &str, body: &str) {
        self.calls
            .lock()
            .push((title.to_string(), body.to_string()));
    }
}

fn agent_transition(
    new_state: crate::notifications::AgentState,
) -> crate::notifications::AgentTransition {
    crate::notifications::AgentTransition {
        old_state: Some(crate::notifications::AgentState::Working),
        new_state,
        name: Some("claude".to_string()),
    }
}

/// `App::new()` already defaults `agent_status_notifications` and
/// `notification_enabled` to `true` (see `settings.rs`), so this
/// starts every test from the "both switches on" baseline.
fn app_with_test_sink() -> (App, Arc<TestNotifySink>) {
    let app = App::new();
    assert!(app.settings.agent_status_notifications);
    assert!(app.settings.notification_enabled);
    let sink: Arc<TestNotifySink> = Arc::new(TestNotifySink::default());
    let mut app = app;
    app.notification_sink = sink.clone() as Arc<dyn NotificationSink>;
    (app, sink)
}

/// Clone-and-flip one bool field on `app.settings` (an `Arc<Settings>`)
/// — mirrors the existing `sidebar_hidden_on_local_tab_regardless_of_mode`-
/// style pattern elsewhere in this test module.
fn with_setting(app: &mut App, set: impl FnOnce(&mut Settings)) {
    app.settings = Arc::new({
        let mut s = (*app.settings).clone();
        set(&mut s);
        s
    });
}

// AC-1/AC-2: a qualifying transition on a non-visible pane fires
// exactly one notification.
#[test]
fn maybe_notify_agent_transition_ac1_fires_for_blocked_on_non_visible_pane() {
    let (mut app, sink) = app_with_test_sink();
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        false,
        &agent_transition(crate::notifications::AgentState::Blocked),
        "my-tab",
    );
    assert!(fired);
    assert_eq!(sink.calls().len(), 1);
}

// AC-1: working/idle transitions never fire.
#[test]
fn maybe_notify_agent_transition_ac1_working_and_idle_never_fire() {
    let (mut app, sink) = app_with_test_sink();
    for state in [
        crate::notifications::AgentState::Working,
        crate::notifications::AgentState::Idle,
    ] {
        let fired =
            app.maybe_notify_agent_transition("pane-1", false, &agent_transition(state), "my-tab");
        assert!(!fired);
    }
    assert!(sink.calls().is_empty());
}

// AC-2: a transition on the visible pane does not fire.
#[test]
fn maybe_notify_agent_transition_ac2_visible_pane_does_not_fire() {
    let (mut app, sink) = app_with_test_sink();
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        true,
        &agent_transition(crate::notifications::AgentState::Done),
        "my-tab",
    );
    assert!(!fired);
    assert!(sink.calls().is_empty());
}

// AC-3: either settings switch off suppresses the notification.
#[test]
fn maybe_notify_agent_transition_ac3_settings_off_suppress() {
    let (mut app, sink) = app_with_test_sink();
    with_setting(&mut app, |s| s.agent_status_notifications = false);
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        false,
        &agent_transition(crate::notifications::AgentState::Blocked),
        "my-tab",
    );
    assert!(!fired);

    with_setting(&mut app, |s| {
        s.agent_status_notifications = true;
        s.notification_enabled = false;
    });
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        false,
        &agent_transition(crate::notifications::AgentState::Blocked),
        "my-tab",
    );
    assert!(!fired);
    assert!(sink.calls().is_empty());
}

// AC-4: a second qualifying transition on the same pane inside the
// rate-limit interval does not fire; a suppressed (visible-pane)
// attempt in between does not consume the window either.
#[test]
fn maybe_notify_agent_transition_ac4_rate_limits_per_pane() {
    let (mut app, sink) = app_with_test_sink();
    let t = agent_transition(crate::notifications::AgentState::Blocked);

    assert!(app.maybe_notify_agent_transition("pane-1", false, &t, "my-tab"));
    // Immediately after: still inside the rate-limit window.
    assert!(!app.maybe_notify_agent_transition("pane-1", false, &t, "my-tab"));
    // A different pane is unaffected by pane-1's window.
    assert!(app.maybe_notify_agent_transition("pane-2", false, &t, "my-tab"));

    assert_eq!(sink.calls().len(), 2);
}

// task0001 AC-2/AC-3: `App::maybe_notify_agent_transition` reads
// `Settings::agent_notify_on_done` / `agent_notify_on_blocked` and
// passes them through to the gating decision — verifies the wiring
// itself (the pure decision logic is exhaustively covered by
// `notifications::tests`).
#[test]
fn maybe_notify_agent_transition_reads_event_type_toggles_from_settings() {
    let (mut app, sink) = app_with_test_sink();

    with_setting(&mut app, |s| s.agent_notify_on_done = false);
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        false,
        &agent_transition(crate::notifications::AgentState::Done),
        "my-tab",
    );
    assert!(
        !fired,
        "agent_notify_on_done=false must suppress a Done transition"
    );

    // Blocked is unaffected by the done-only toggle (independence).
    let fired = app.maybe_notify_agent_transition(
        "pane-1",
        false,
        &agent_transition(crate::notifications::AgentState::Blocked),
        "my-tab",
    );
    assert!(
        fired,
        "agent_notify_on_done=false must not suppress Blocked"
    );

    with_setting(&mut app, |s| {
        s.agent_notify_on_done = true;
        s.agent_notify_on_blocked = false;
    });
    let fired = app.maybe_notify_agent_transition(
        "pane-2",
        false,
        &agent_transition(crate::notifications::AgentState::Blocked),
        "my-tab",
    );
    assert!(
        !fired,
        "agent_notify_on_blocked=false must suppress a Blocked transition"
    );

    assert_eq!(sink.calls().len(), 1);
}

// AC-6: the fire/suppress judgement never depends on `pane_key`'s
// format — a plain-tab-shaped key (`"tab:<id>"`, see
// `agent_notification_rate_limit_key`) and a mux-pane-shaped key (the
// daemon's `public_pane_id`, e.g. `"xyz-7"`) produce identical
// decisions for identical settings/visibility/state inputs, both when
// firing and when suppressed by an event-type toggle.
#[test]
fn maybe_notify_agent_transition_ac6_judgement_independent_of_pane_key_format() {
    let (mut app, sink) = app_with_test_sink();
    let t = agent_transition(crate::notifications::AgentState::Blocked);

    // Both key formats fire under the default (all-ON) settings.
    assert!(app.maybe_notify_agent_transition("tab:42", false, &t, "shell"));
    assert!(app.maybe_notify_agent_transition("xyz-7", false, &t, "shell"));
    assert_eq!(sink.calls().len(), 2);

    // Flip the event-type toggle matching this transition's state
    // OFF: both formats are suppressed identically (fresh keys so the
    // rate limiter from the block above cannot explain the result).
    with_setting(&mut app, |s| s.agent_notify_on_blocked = false);
    assert!(!app.maybe_notify_agent_transition("tab:99", false, &t, "shell"));
    assert!(!app.maybe_notify_agent_transition("mux-99", false, &t, "shell"));
    assert_eq!(sink.calls().len(), 2);
}

#[test]
fn discard_agent_notification_state_reopens_the_rate_limit_window() {
    let (mut app, sink) = app_with_test_sink();
    let t = agent_transition(crate::notifications::AgentState::Done);
    assert!(app.maybe_notify_agent_transition("pane-1", false, &t, "my-tab"));
    assert!(!app.maybe_notify_agent_transition("pane-1", false, &t, "my-tab"));

    app.discard_agent_notification_state("pane-1");
    assert!(app.maybe_notify_agent_transition("pane-1", false, &t, "my-tab"));
    assert_eq!(sink.calls().len(), 2);
}

// ── task0009: wire `AgentStatusModel::drain_transitions` into
// `pump_all` so blocked/done transitions reach the notification layer
// ("Rework context" — review round 1's `drain_wiring_spec` finding).

/// AC-1: a real Set-transition to Blocked on a NON-visible (background)
/// pane, drained by `pump_all`, fires exactly one notification; the
/// SAME kind of transition on the VISIBLE (active, focused) pane fires
/// none.
#[test]
fn pump_all_ac1_notifies_background_pane_not_visible_pane() {
    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab(); // tab 0: stays background (non-visible)
    app.spawn_new_tab(); // tab 1: freshly active
    app.window_focused = true;

    // Background tab's agent reports straight to Blocked — a
    // qualifying first-ever report (old_state None -> new Blocked).
    app.tabs[0].cb_state.lock().pending_agent_status.push(
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Blocked,
            name: Some("claude".to_string()),
        },
    );
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);

    // The ACTIVE tab's own agent reports Done: window focused AND it
    // IS the displayed tab, so this must not fire (count unchanged).
    app.tabs[1].cb_state.lock().pending_agent_status.push(
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Done,
            name: Some("claude".to_string()),
        },
    );
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);
}

/// AC-2: a same-state re-report and a Clear (`new_state: None`)
/// transition both drain WITHOUT firing a notification — only the
/// original real Set counts.
#[test]
fn pump_all_ac2_same_state_and_clear_transitions_do_not_notify() {
    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab(); // tab 0: stays background (non-visible)
    app.spawn_new_tab(); // tab 1: active
    app.window_focused = true;

    app.tabs[0].cb_state.lock().pending_agent_status.push(
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Blocked,
            name: None,
        },
    );
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);

    // Same-state re-report: `AgentStatusModel` enqueues no transition
    // at all for this, so nothing reaches the notification layer.
    app.tabs[0].cb_state.lock().pending_agent_status.push(
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Blocked,
            name: None,
        },
    );
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);

    // Clear: a transition IS enqueued (old=Blocked, new=None), but
    // `new_state: None` is never notification-eligible.
    app.tabs[0]
        .cb_state
        .lock()
        .pending_agent_status
        .push(crate::agent_status::AgentStatusEvent::Clear);
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);
}

/// AC-3: the transition queue drains every `pump_all`, even while
/// `agent_status_notifications` is off — the queue must not grow while
/// the setting is toggled off (NFR3); the settings gate lives inside
/// `maybe_notify_agent_transition`, not in whether draining happens.
#[test]
fn pump_all_ac3_drains_transitions_even_when_setting_is_off() {
    let (mut app, sink) = app_with_test_sink();
    with_setting(&mut app, |s| s.agent_status_notifications = false);
    app.spawn_initial_tab();
    app.window_focused = true;

    app.tabs[0].cb_state.lock().pending_agent_status.push(
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Blocked,
            name: None,
        },
    );
    app.pump_all();

    // The setting being off suppressed the notification…
    assert!(sink.calls().is_empty());
    // …but `pump_all` still drained the queue; nothing is left for a
    // caller to drain again.
    assert!(app.agent_status.drain_transitions().is_empty());
}

/// AC-4: closing a tab discards its agent-status entry (task0005
/// AC-6, already covered by `close_tab_discards_agent_status_entry`)
/// AND its notification rate-limiter state (task0009). Re-arming the
/// SAME key immediately after close proves the window was discarded,
/// not merely orphaned.
#[test]
fn close_tab_discards_agent_notification_rate_limit_state() {
    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab();
    let stable_id = app.tabs[0].stable_id;
    let key = crate::agent_status_model::PaneKey::Tab(stable_id);
    let rate_limit_key = agent_notification_rate_limit_key(&app.mux_public_pane_ids, &key);

    let t = agent_transition(crate::notifications::AgentState::Blocked);
    assert!(app.maybe_notify_agent_transition(rate_limit_key.clone(), false, &t, "shell"));
    assert!(!app.maybe_notify_agent_transition(rate_limit_key.clone(), false, &t, "shell"));

    app.close_tab(0);

    assert!(app.maybe_notify_agent_transition(rate_limit_key, false, &t, "shell"));
    assert_eq!(sink.calls().len(), 2);
}

/// AC-4 (reap-exited-tab variant): a tab that exits (`exited = true`,
/// reaped by `pump_all` rather than closed via `close_tab`) also
/// discards its agent-notification rate-limiter state.
#[test]
fn pump_all_reap_exited_tab_discards_agent_notification_rate_limit_state() {
    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab();
    app.spawn_new_tab(); // two tabs; active is tab 1, tab 0 will be reaped
    let stable_id = app.tabs[0].stable_id;
    let key = crate::agent_status_model::PaneKey::Tab(stable_id);
    let rate_limit_key = agent_notification_rate_limit_key(&app.mux_public_pane_ids, &key);

    let t = agent_transition(crate::notifications::AgentState::Done);
    assert!(app.maybe_notify_agent_transition(rate_limit_key.clone(), false, &t, "shell"));
    assert!(!app.maybe_notify_agent_transition(rate_limit_key.clone(), false, &t, "shell"));

    app.tabs[0].exited = true;
    app.pump_all();
    assert_eq!(app.tabs.len(), 1, "exited tab was reaped");

    assert!(app.maybe_notify_agent_transition(rate_limit_key, false, &t, "shell"));
    assert_eq!(sink.calls().len(), 2);
}

/// AC-4 (closed-mux-pane variant): a mux pane closing via `PtyExited`
/// (routed into `pump_all`'s closed-panes loop, not `close_tab`) also
/// discards its notification rate-limiter state, keyed by the
/// daemon-learned `public_pane_id`.
#[test]
fn pump_all_closed_mux_pane_discards_agent_notification_rate_limit_state() {
    use mux_ipc::protocol::{AgentState as WireState, AgentStatusUpdateMsg};

    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab(); // tab 0: will host the mux group
    app.spawn_new_tab(); // tab 1: active — tab 0's mux pane is non-visible

    let welcome = MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: 1,
                pane_count: 1,
                active_window_index: 0,
                windows: vec![WindowInfo {
                    id: 0,
                    name: "w0".to_string(),
                    active_pane_id: 7,
                }],
            }],
        },
    );
    app.on_mux_message(0, welcome);
    let update = AgentStatusUpdateMsg {
        pane_id: 7,
        public_pane_id: "xyz-7".to_string(),
        state: Some(WireState::Blocked),
        name: None,
        revision: 1,
        replay_derived: false,
    };
    app.on_mux_message(
        0,
        MuxMessage::control(MessageType::AgentStatusUpdate, 7, &update),
    );
    app.pump_all();
    assert_eq!(sink.calls().len(), 1);

    // Rate limiter is armed for "xyz-7": a second attempt does not fire.
    let t = agent_transition(crate::notifications::AgentState::Blocked);
    assert!(!app.maybe_notify_agent_transition("xyz-7", false, &t, "shell"));

    let pty_exited = MuxMessage {
        msg_type: MessageType::PtyExited,
        pane_id: 7,
        payload: Vec::new(),
    };
    app.on_mux_message(0, pty_exited);
    app.pump_all();

    // The window reopened for "xyz-7" — proves the closed-panes loop
    // discarded the rate-limiter bookkeeping, not just the model entry.
    assert!(app.maybe_notify_agent_transition("xyz-7", false, &t, "shell"));
    assert_eq!(sink.calls().len(), 2);
}

/// AC-5: a `replay_derived: true` daemon update never enqueues a
/// transition, so `pump_all`'s drain sees nothing for it — no
/// notification fires even for a qualifying (Blocked) state and even
/// with both notification switches on and the pane non-visible.
#[test]
fn pump_all_ac5_replay_derived_update_does_not_notify() {
    use mux_ipc::protocol::{AgentState as WireState, AgentStatusUpdateMsg};

    let (mut app, sink) = app_with_test_sink();
    app.spawn_initial_tab();
    app.window_focused = false;

    let update = AgentStatusUpdateMsg {
        pane_id: 42,
        public_pane_id: "abc-42".to_string(),
        state: Some(WireState::Blocked),
        name: None,
        revision: 1,
        replay_derived: true,
    };
    app.on_mux_message(
        0,
        MuxMessage::control(MessageType::AgentStatusUpdate, 42, &update),
    );
    app.pump_all();

    assert!(sink.calls().is_empty());
    assert!(app.agent_status.drain_transitions().is_empty());
}

// ── task0009: pure helper unit coverage (visibility / title / key) ──

#[test]
fn agent_status_pane_visible_false_when_window_unfocused() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let key = crate::agent_status_model::PaneKey::Tab(app.tabs[0].stable_id);
    assert!(!agent_status_pane_visible(false, app.tabs.first(), &key));
}

#[test]
fn agent_status_pane_visible_true_only_for_the_active_tabs_keys() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.spawn_new_tab();
    let background_key = crate::agent_status_model::PaneKey::Tab(app.tabs[0].stable_id);
    let active_key = crate::agent_status_model::PaneKey::Tab(app.tabs[1].stable_id);
    let active_tab = app.tabs.get(1);
    assert!(!agent_status_pane_visible(
        true,
        active_tab,
        &background_key
    ));
    assert!(agent_status_pane_visible(true, active_tab, &active_key));
}

#[test]
fn agent_status_pane_tab_title_resolves_plain_tab_and_mux_pane() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.tabs[0].title = "my-title".to_string();
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

    assert_eq!(
        agent_status_pane_tab_title(
            &app.tabs,
            &crate::agent_status_model::PaneKey::Tab(app.tabs[0].stable_id)
        ),
        Some("my-title")
    );
    assert_eq!(
        agent_status_pane_tab_title(&app.tabs, &crate::agent_status_model::PaneKey::MuxPane(9)),
        Some("my-title")
    );
    assert_eq!(
        agent_status_pane_tab_title(&app.tabs, &crate::agent_status_model::PaneKey::MuxPane(999)),
        None
    );
}

#[test]
fn agent_notification_rate_limit_key_prefers_public_pane_id_falls_back_to_prefixed_id() {
    use std::collections::HashMap;
    let mut ids: HashMap<u32, String> = HashMap::new();
    ids.insert(7, "xyz-7".to_string());

    assert_eq!(
        agent_notification_rate_limit_key(&ids, &crate::agent_status_model::PaneKey::MuxPane(7)),
        "xyz-7"
    );
    // No learned public id: falls back to a prefixed pane-id string.
    assert_eq!(
        agent_notification_rate_limit_key(&ids, &crate::agent_status_model::PaneKey::MuxPane(8)),
        "mux:8"
    );
    assert_eq!(
        agent_notification_rate_limit_key(&ids, &crate::agent_status_model::PaneKey::Tab(3)),
        "tab:3"
    );
}

// TS-5: arm(now) sets the dismissal instant to now + linger window.
#[test]
fn restart_toast_arm_sets_dismiss_at() {
    let mut toast = RestartToast::default();
    assert!(!toast.active());
    toast.arm(10.0);
    assert!(toast.active());
    assert_eq!(toast.dismiss_at, Some(10.0 + RESTART_TOAST_LINGER_SECS));
}

// TS-6: prune keeps the toast while now < instant, clears it at/after.
#[test]
fn restart_toast_prune_keeps_then_clears() {
    let mut toast = RestartToast::default();
    toast.arm(0.0);
    // Before the linger window elapses → still active.
    toast.prune(RESTART_TOAST_LINGER_SECS - 0.1);
    assert!(toast.active());
    // At/after the dismissal instant → cleared.
    toast.prune(RESTART_TOAST_LINGER_SECS);
    assert!(!toast.active());
    assert_eq!(toast.dismiss_at, None);
}

// TS-7: re-arm after a prior arm keeps a single toast and refreshes the
// dismissal instant (no second toast, instant moves forward).
#[test]
fn restart_toast_rearm_refreshes_single_toast() {
    let mut toast = RestartToast::default();
    toast.arm(0.0);
    assert_eq!(toast.dismiss_at, Some(RESTART_TOAST_LINGER_SECS));
    // A later failed spawn re-arms: same single toast, refreshed instant.
    toast.arm(5.0);
    assert_eq!(toast.dismiss_at, Some(5.0 + RESTART_TOAST_LINGER_SECS));
    // The earlier instant would have dismissed by now, but the refresh
    // keeps the toast active.
    toast.prune(RESTART_TOAST_LINGER_SECS + 0.1);
    assert!(toast.active());
}

#[test]
fn visual_bell_idle_by_default() {
    let mut app = App::new();
    assert_eq!(app.visual_bell_progress(), None);
    assert!(!app.needs_bell_repaint());
}

#[test]
fn open_search_sets_visible_and_focus_request() {
    let mut app = App::new();
    assert!(!app.search_visible());
    app.open_search();
    assert!(app.search_visible());
    assert!(app.search_focus_request);
}

#[test]
fn close_search_clears_state() {
    let mut app = App::new();
    app.open_search();
    app.search.query = "x".to_string();
    app.close_search();
    assert!(!app.search_visible());
    assert!(app.search.query.is_empty());
    assert!(!app.search_focus_request);
}

// ── task0005: agent-status pump_all wiring ────────────────────────

/// AC-1: a plain-tab `agent-status` OSC event (buffered by
/// `NativeCallbacks::on_osc` into `cb_state.pending_agent_status`, as
/// the real OSC dispatch path would) reaches `App::agent_status` via
/// `pump_all`'s latch-drain, keyed by the tab's `stable_id`.
#[test]
fn pump_all_applies_plain_tab_agent_status_event_to_model() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let tab_stable_id = app.active_tab().unwrap().stable_id;
    app.active_tab()
        .unwrap()
        .cb_state
        .lock()
        .pending_agent_status
        .push(crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Working,
            name: Some("claude".to_string()),
        });

    app.pump_all();

    let status = app
        .agent_status
        .status(&crate::agent_status_model::PaneKey::Tab(tab_stable_id))
        .expect("plain-tab event applied to the model");
    assert_eq!(status.state, Some(crate::agent_status::AgentState::Working));
    assert_eq!(status.name, Some("claude".to_string()));
}

/// AC-2: a daemon `AgentStatusUpdate` decoded by `Tab::apply_mux_message`
/// reaches `App::agent_status` via `pump_all`, with the wire-level
/// `mux_ipc::protocol::AgentState` converted to the core
/// `crate::agent_status::AgentState` the model stores.
#[test]
fn pump_all_applies_daemon_agent_status_update_to_model() {
    use mux_ipc::protocol::{AgentState as WireState, AgentStatusUpdateMsg};

    // Subject under test is the model update, not the notification, so
    // the capturing sink's handle is intentionally unused (bound as
    // `_sink`) rather than asserted on — its role here is purely to keep
    // this test off the production `NotifyRustSink`.
    let (mut app, _sink) = app_with_test_sink();
    app.spawn_initial_tab();
    let update = AgentStatusUpdateMsg {
        pane_id: 42,
        public_pane_id: "abc-42".to_string(),
        state: Some(WireState::Blocked),
        name: Some("agent".to_string()),
        revision: 7,
        replay_derived: false,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 42, &update);
    app.on_mux_message(0, msg);

    app.pump_all();

    let status = app
        .agent_status
        .status(&crate::agent_status_model::PaneKey::MuxPane(42))
        .expect("daemon update applied to the model");
    assert_eq!(status.state, Some(crate::agent_status::AgentState::Blocked));
    assert_eq!(status.revision, 7);
}

/// AC-6: closing a tab discards its plain-tab `App::agent_status` entry.
#[test]
fn close_tab_discards_agent_status_entry() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let tab_stable_id = app.active_tab().unwrap().stable_id;
    app.agent_status.apply_plain_tab_event(
        tab_stable_id,
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Done,
            name: None,
        },
    );

    app.close_tab(0);

    assert!(
        app.agent_status
            .status(&crate::agent_status_model::PaneKey::Tab(tab_stable_id))
            .is_none()
    );
}

/// AC-5: `pump_all` marks the active tab's panes seen when the OS
/// window is focused.
#[test]
fn pump_all_marks_active_tab_seen_when_window_focused() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let tab_stable_id = app.active_tab().unwrap().stable_id;
    app.agent_status.apply_plain_tab_event(
        tab_stable_id,
        crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Blocked,
            name: None,
        },
    );
    assert!(
        app.agent_status
            .status(&crate::agent_status_model::PaneKey::Tab(tab_stable_id))
            .unwrap()
            .unseen
    );
    app.window_focused = true;

    app.pump_all();

    assert!(
        !app.agent_status
            .status(&crate::agent_status_model::PaneKey::Tab(tab_stable_id))
            .unwrap()
            .unseen
    );
}

// ── task0006: agent-status query surface + public-pane-ID map ────

/// AC-5: `pump_all` learns a mux pane's public ID from the daemon's
/// `AgentStatusUpdate` payload, queryable via `App::mux_public_pane_id`.
#[test]
fn pump_all_learns_public_pane_id_from_daemon_agent_status_update() {
    use mux_ipc::protocol::{AgentState as WireState, AgentStatusUpdateMsg};

    let mut app = App::new();
    app.spawn_initial_tab();
    assert_eq!(app.mux_public_pane_id(42), None);

    let update = AgentStatusUpdateMsg {
        pane_id: 42,
        public_pane_id: "abc-42".to_string(),
        state: Some(WireState::Working),
        name: None,
        revision: 1,
        replay_derived: false,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 42, &update);
    app.on_mux_message(0, msg);
    app.pump_all();

    assert_eq!(app.mux_public_pane_id(42), Some("abc-42"));
}

/// A closed mux pane's public ID is forgotten alongside its
/// `agent_status` entry (mirrors `discard_removes_entry_and_updates_
/// aggregate_and_counts` in `agent_status_model`, at the `App` layer).
/// Drives the real `Welcome` -> `AgentStatusUpdate` -> `PtyExited`
/// sequence rather than reaching into `Tab` internals, matching the
/// existing `on_mux_message`-based tests in this module.
#[test]
fn closing_a_mux_pane_forgets_its_public_pane_id() {
    use mux_ipc::protocol::{AgentState as WireState, AgentStatusUpdateMsg};

    let mut app = App::new();
    app.spawn_initial_tab();
    let welcome = MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: 1,
                pane_count: 1,
                active_window_index: 0,
                windows: vec![WindowInfo {
                    id: 0,
                    name: "w0".to_string(),
                    active_pane_id: 7,
                }],
            }],
        },
    );
    app.on_mux_message(0, welcome);

    let update = AgentStatusUpdateMsg {
        pane_id: 7,
        public_pane_id: "xyz-7".to_string(),
        state: Some(WireState::Idle),
        name: None,
        revision: 1,
        replay_derived: false,
    };
    let update_msg = MuxMessage::control(MessageType::AgentStatusUpdate, 7, &update);
    app.on_mux_message(0, update_msg);
    app.pump_all();
    assert_eq!(app.mux_public_pane_id(7), Some("xyz-7"));

    let pty_exited = MuxMessage {
        msg_type: MessageType::PtyExited,
        pane_id: 7,
        payload: Vec::new(),
    };
    app.on_mux_message(0, pty_exited);
    app.pump_all();

    assert_eq!(app.mux_public_pane_id(7), None);
}

/// `agent_status_pane_badge` is a thin, single-pane wrapper over
/// `AgentStatusModel::aggregate` — returns `None` for an unreported
/// pane and the pane's own state once reported.
#[test]
fn agent_status_pane_badge_reflects_the_single_pane_queried() {
    let mut app = App::new();
    assert_eq!(app.agent_status_pane_badge(1), None);

    app.agent_status.apply_daemon_update(
        1,
        Some(crate::agent_status::AgentState::Blocked),
        None,
        1,
        false,
    );
    let badge = app.agent_status_pane_badge(1).expect("pane 1 has status");
    assert_eq!(badge.state, crate::agent_status::AgentState::Blocked);
    assert!(badge.unseen);
}

/// `agent_status_badge_for` aggregates across a mux-attached tab's own
/// plain-tab key AND every pane in its window group (task0006 AC-1),
/// delegating to the same `agent_status_keys_for_tab` set `pump_all`'s
/// mark_seen path already uses.
#[test]
fn agent_status_badge_for_aggregates_across_a_tabs_mux_panes() {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let tab = app.active_tab_mut().unwrap();
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
            vec![10, 11],
            0,
        );
        tab.mux_group = Some(group);
    }
    app.agent_status.apply_daemon_update(
        11,
        Some(crate::agent_status::AgentState::Blocked),
        None,
        1,
        false,
    );

    let tab = app.active_tab().unwrap();
    let badge = app
        .agent_status_badge_for(tab)
        .expect("group's pane 11 has status");
    assert_eq!(badge.state, crate::agent_status::AgentState::Blocked);
}

/// `agent_status_badge_for` returns `None` when neither the tab itself
/// nor any pane in its group has ever reported a state (task0006 AC-2).
#[test]
fn agent_status_badge_for_is_none_when_nothing_reported() {
    let mut app = App::new();
    app.spawn_initial_tab();
    let tab = app.active_tab().unwrap();
    assert_eq!(app.agent_status_badge_for(tab), None);
}
