use super::*;

fn tab(id: u64) -> PaneKey {
    PaneKey::Tab(id)
}

fn pane(id: u32) -> PaneKey {
    PaneKey::MuxPane(id)
}

// ── AC-1: plain-tab OSC updates the model like a daemon update would ──

#[test]
fn plain_tab_set_mints_revision_and_enqueues_transition() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(
        1,
        AgentStatusEvent::Set {
            state: AgentState::Working,
            name: Some("claude".to_string()),
        },
    );

    let status = model.status(&tab(1)).expect("entry created");
    assert_eq!(status.state, Some(AgentState::Working));
    assert_eq!(status.name, Some("claude".to_string()));
    assert_eq!(status.revision, 1);
    assert!(status.unseen);

    let transitions = model.drain_transitions();
    assert_eq!(
        transitions,
        vec![Transition {
            pane: tab(1),
            old_state: None,
            new_state: Some(AgentState::Working),
            name: Some("claude".to_string()),
        }]
    );
}

#[test]
fn plain_tab_clear_advances_revision_and_enqueues_transition() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(
        1,
        AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        },
    );
    model.drain_transitions();

    model.apply_plain_tab_event(1, AgentStatusEvent::Clear);

    let status = model.status(&tab(1)).expect("entry retained after clear");
    assert_eq!(status.state, None);
    assert_eq!(status.revision, 2);

    let transitions = model.drain_transitions();
    assert_eq!(
        transitions,
        vec![Transition {
            pane: tab(1),
            old_state: Some(AgentState::Working),
            new_state: None,
            name: None,
        }]
    );
}

#[test]
fn plain_tab_and_daemon_paths_produce_equivalent_status_shape() {
    let mut plain = AgentStatusModel::new();
    plain.apply_plain_tab_event(
        1,
        AgentStatusEvent::Set {
            state: AgentState::Blocked,
            name: Some("agent".to_string()),
        },
    );

    let mut daemon = AgentStatusModel::new();
    daemon.apply_daemon_update(
        10,
        Some(AgentState::Blocked),
        Some("agent".to_string()),
        1,
        false,
    );

    let plain_status = plain.status(&tab(1)).unwrap();
    let daemon_status = daemon.status(&pane(10)).unwrap();
    assert_eq!(plain_status.state, daemon_status.state);
    assert_eq!(plain_status.name, daemon_status.name);
    assert_eq!(plain_status.revision, daemon_status.revision);
    assert_eq!(plain_status.unseen, daemon_status.unseen);
}

// ── AC-2: daemon replay_derived / same-state re-report gating ─────────

#[test]
fn daemon_update_real_change_enqueues_transition() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
    assert_eq!(model.drain_transitions().len(), 1);
}

#[test]
fn daemon_update_same_state_re_report_enqueues_nothing() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
    model.drain_transitions();

    model.apply_daemon_update(10, Some(AgentState::Working), None, 2, false);
    assert!(model.drain_transitions().is_empty());
    // Revision still advances even though nothing else changed.
    assert_eq!(model.status(&pane(10)).unwrap().revision, 2);
}

#[test]
fn daemon_update_replay_derived_never_enqueues_even_on_real_change() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(10, Some(AgentState::Blocked), None, 5, true);
    assert!(model.drain_transitions().is_empty());
    assert_eq!(
        model.status(&pane(10)).unwrap().state,
        Some(AgentState::Blocked)
    );
}

#[test]
fn daemon_update_replay_derived_state_change_still_marks_unseen() {
    // Replay silences the *notification* (no transition) but the
    // "unseen" flag still reflects the real change, per the design
    // note: "seen flags are ... reset to unseen on a real state
    // change" (stated independently of replay_derived).
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
    model.drain_transitions(); // discard the first (non-replay) transition
    model.mark_seen([&pane(10)]);
    assert!(!model.status(&pane(10)).unwrap().unseen);

    model.apply_daemon_update(10, Some(AgentState::Blocked), None, 2, true);
    assert!(model.status(&pane(10)).unwrap().unseen);
    assert!(model.drain_transitions().is_empty());
}

// ── AC-3: aggregate priority order + unseen distinction ───────────────

#[test]
fn aggregate_prefers_blocked_over_everything() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
    model.apply_daemon_update(2, Some(AgentState::Blocked), None, 1, false);
    model.apply_daemon_update(3, Some(AgentState::Done), None, 1, false);

    let panes = vec![pane(1), pane(2), pane(3)];
    let agg = model.aggregate(&panes).unwrap();
    assert_eq!(agg.state, AgentState::Blocked);
}

#[test]
fn aggregate_ranks_unseen_done_above_working_above_seen_done_above_idle() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Idle), None, 1, false);
    model.apply_daemon_update(2, Some(AgentState::Done), None, 1, false);
    model.mark_seen([&pane(2)]); // seen-done
    model.apply_daemon_update(3, Some(AgentState::Working), None, 1, false);

    // seen-done(1) < working(2): working wins over idle+seen-done.
    let agg = model.aggregate(&[pane(1), pane(2), pane(3)]).unwrap();
    assert_eq!(agg.state, AgentState::Working);

    // unseen-done(3) > working(2): add an unseen-done pane and it wins.
    model.apply_daemon_update(4, Some(AgentState::Done), None, 1, false);
    let agg = model
        .aggregate(&[pane(1), pane(2), pane(3), pane(4)])
        .unwrap();
    assert_eq!(agg.state, AgentState::Done);
    assert!(agg.unseen);
}

#[test]
fn aggregate_seen_done_reports_seen_unseen_false() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Done), None, 1, false);
    model.mark_seen([&pane(1)]);

    let agg = model.aggregate(&[pane(1)]).unwrap();
    assert_eq!(agg.state, AgentState::Done);
    assert!(!agg.unseen);
}

#[test]
fn aggregate_returns_none_when_no_pane_has_status() {
    let model = AgentStatusModel::new();
    assert_eq!(model.aggregate(&[pane(1), tab(2)]), None);
}

#[test]
fn aggregate_ignores_cleared_panes() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
    model.apply_daemon_update(1, None, None, 2, false); // cleared
    assert_eq!(model.aggregate(&[pane(1)]), None);
}

// ── mux-agent-tab-cycle task0001 AC-7: any_pane_has_reported_state ────

#[test]
fn any_pane_has_reported_state_true_for_each_reported_state_kind() {
    for state in [
        AgentState::Idle,
        AgentState::Working,
        AgentState::Blocked,
        AgentState::Done,
    ] {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(state), None, 1, false);
        assert!(
            model.any_pane_has_reported_state(&[1]),
            "{state:?} must qualify"
        );
    }
}

#[test]
fn any_pane_has_reported_state_false_for_cleared_and_never_reported() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
    model.apply_daemon_update(1, None, None, 2, false); // cleared
    assert!(
        !model.any_pane_has_reported_state(&[1]),
        "cleared pane must not qualify"
    );
    assert!(
        !model.any_pane_has_reported_state(&[999]),
        "never-reported pane must not qualify"
    );
}

#[test]
fn any_pane_has_reported_state_multi_pane_qualifies_existentially() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(2, Some(AgentState::Idle), None, 1, false);
    // pane 1 never reported, pane 2 reported Idle — the set qualifies
    // because at least one pane does (existential, FR6).
    assert!(model.any_pane_has_reported_state(&[1, 2]));
    // Neither pane in this set ever reported: does not qualify.
    assert!(!model.any_pane_has_reported_state(&[3, 4]));
}

#[test]
fn any_pane_has_reported_state_empty_set_is_false() {
    let model = AgentStatusModel::new();
    assert!(!model.any_pane_has_reported_state(&[]));
}

// ── AC-4: counts ignore seen, empty model reports zero ───────────────

#[test]
fn counts_are_zero_for_empty_model() {
    let model = AgentStatusModel::new();
    assert_eq!(model.counts(), Counts::default());
}

#[test]
fn counts_reflect_semantic_state_regardless_of_seen() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Blocked), None, 1, false);
    model.apply_daemon_update(2, Some(AgentState::Blocked), None, 1, false);
    model.mark_seen([&pane(2)]); // seen, still counted as blocked
    model.apply_daemon_update(3, Some(AgentState::Working), None, 1, false);
    model.apply_daemon_update(4, Some(AgentState::Done), None, 1, false);
    model.apply_daemon_update(5, Some(AgentState::Idle), None, 1, false);
    model.apply_daemon_update(6, None, None, 1, false); // cleared, excluded

    let counts = model.counts();
    assert_eq!(
        counts,
        Counts {
            idle: 1,
            working: 1,
            blocked: 2,
            done: 1,
        }
    );
}

// ── AC-5: mark_seen clears unseen without touching state/revision ────

#[test]
fn mark_seen_clears_unseen_only_for_listed_panes() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Blocked), Some("a".into()), 3, false);
    model.apply_daemon_update(2, Some(AgentState::Working), None, 1, false);

    model.mark_seen([&pane(1)]);

    let seen = model.status(&pane(1)).unwrap();
    assert!(!seen.unseen);
    assert_eq!(seen.state, Some(AgentState::Blocked));
    assert_eq!(seen.name, Some("a".to_string()));
    assert_eq!(seen.revision, 3);

    // pane(2) was not in the mark_seen call: still unseen.
    assert!(model.status(&pane(2)).unwrap().unseen);
}

#[test]
fn mark_seen_on_missing_pane_is_a_no_op() {
    let mut model = AgentStatusModel::new();
    model.mark_seen([&pane(99)]); // must not panic
    assert!(model.status(&pane(99)).is_none());
}

// ── AC-6: tab/pane close removes entries ──────────────────────────────

#[test]
fn discard_removes_entry_and_updates_aggregate_and_counts() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Blocked), None, 1, false);
    model.apply_daemon_update(2, Some(AgentState::Working), None, 1, false);

    model.discard(&pane(1));

    assert!(model.status(&pane(1)).is_none());
    assert_eq!(
        model.aggregate(&[pane(1), pane(2)]).unwrap().state,
        AgentState::Working
    );
    assert_eq!(
        model.counts(),
        Counts {
            working: 1,
            ..Counts::default()
        }
    );
}

#[test]
fn discard_plain_tab_entry_via_tab_key() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(
        7,
        AgentStatusEvent::Set {
            state: AgentState::Done,
            name: None,
        },
    );
    model.discard(&tab(7));
    assert!(model.status(&tab(7)).is_none());
}

// ── AC-7: a real state change resets seen to unseen ───────────────────

#[test]
fn real_state_change_resets_seen_to_unseen() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
    model.mark_seen([&pane(1)]);
    assert!(!model.status(&pane(1)).unwrap().unseen);

    model.apply_daemon_update(1, Some(AgentState::Blocked), None, 2, false);
    assert!(model.status(&pane(1)).unwrap().unseen);
}

#[test]
fn same_state_re_report_preserves_seen() {
    let mut model = AgentStatusModel::new();
    model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
    model.mark_seen([&pane(1)]);
    assert!(!model.status(&pane(1)).unwrap().unseen);

    model.apply_daemon_update(1, Some(AgentState::Working), None, 2, false);
    assert!(!model.status(&pane(1)).unwrap().unseen);
}

// ── wire-state conversion helper ───────────────────────────────────────

#[test]
fn state_from_wire_maps_every_variant() {
    assert_eq!(
        state_from_wire(mux_ipc::protocol::AgentState::Idle),
        AgentState::Idle
    );
    assert_eq!(
        state_from_wire(mux_ipc::protocol::AgentState::Working),
        AgentState::Working
    );
    assert_eq!(
        state_from_wire(mux_ipc::protocol::AgentState::Blocked),
        AgentState::Blocked
    );
    assert_eq!(
        state_from_wire(mux_ipc::protocol::AgentState::Done),
        AgentState::Done
    );
}

#[test]
fn state_to_wire_maps_every_variant() {
    assert_eq!(
        state_to_wire(AgentState::Idle),
        mux_ipc::protocol::AgentState::Idle
    );
    assert_eq!(
        state_to_wire(AgentState::Working),
        mux_ipc::protocol::AgentState::Working
    );
    assert_eq!(
        state_to_wire(AgentState::Blocked),
        mux_ipc::protocol::AgentState::Blocked
    );
    assert_eq!(
        state_to_wire(AgentState::Done),
        mux_ipc::protocol::AgentState::Done
    );
}

#[test]
fn state_to_wire_and_state_from_wire_round_trip() {
    for state in [
        AgentState::Idle,
        AgentState::Working,
        AgentState::Blocked,
        AgentState::Done,
    ] {
        assert_eq!(state_from_wire(state_to_wire(state)), state);
    }
}

// ── agent-exit-after-icon (task0002): plain-tab inferred-clear latch ──
//
// Integration tests exercising the actual callbacks.rs (`LatchFeedEvent`
// candidates) -> reconcile_latch_feed -> latch -> AgentStatusModel path,
// per task0002.md's Test Notes.

use crate::callbacks::LatchFeedEvent;
use crate::prompts::PromptMarkKind;

fn set_event(state: AgentState) -> AgentStatusEvent {
    AgentStatusEvent::Set { state, name: None }
}

// ── AC-1: Set -> live D -> live A clears via the existing Clear path ──

#[test]
fn ac1_set_then_live_d_then_live_a_clears_via_existing_clear_path() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    model.record_latch_set(1);
    model.drain_transitions();

    model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        Some(AgentState::Working)
    );

    model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

    let status = model.status(&tab(1)).unwrap();
    assert_eq!(status.state, None);
    assert_eq!(
        status.revision, 2,
        "went through the real revision-minting apply path"
    );

    let transitions = model.drain_transitions();
    assert_eq!(
        transitions,
        vec![Transition {
            pane: tab(1),
            old_state: Some(AgentState::Working),
            new_state: None,
            name: None,
        }],
        "the inferred clear enqueued exactly the transition an explicit Clear would"
    );
}

// ── AC-2: Set -> live A only (no D) leaves state unchanged ────────────

#[test]
fn ac2_set_then_live_a_without_d_leaves_state_unchanged() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    model.record_latch_set(1);
    model.drain_transitions();

    model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        Some(AgentState::Working)
    );
    assert!(model.drain_transitions().is_empty());
}

// ── AC-3: explicit Clear -> live D/A produces no duplicate clear ──────

#[test]
fn ac3_explicit_clear_then_live_d_a_does_not_duplicate_clear() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    model.record_latch_set(1);
    model.drain_transitions();

    model.apply_plain_tab_event(1, AgentStatusEvent::Clear);
    model.record_latch_clear(1);
    let explicit_clear_transitions = model.drain_transitions();
    assert_eq!(
        explicit_clear_transitions.len(),
        1,
        "the explicit clear itself"
    );
    let revision_after_explicit_clear = model.status(&tab(1)).unwrap().revision;

    model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
    model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

    let status = model.status(&tab(1)).unwrap();
    assert_eq!(status.state, None);
    // Revision is the discriminator: `apply_plain_tab_event` already
    // dedupes a same-state re-report's TRANSITION even without the
    // latch's own disarm, so an empty transition queue alone would not
    // prove the latch stayed disarmed. Revision still advances on every
    // `apply_plain_tab_event` call regardless of whether the state
    // changed, so it catches a disarmed-latch bug that a
    // transition-only assertion would miss.
    assert_eq!(
        status.revision, revision_after_explicit_clear,
        "the disarmed latch must not apply a second Clear at all"
    );
    assert!(
        model.drain_transitions().is_empty(),
        "no second/duplicate clear transition from the disarmed latch"
    );
}

// ── AC-4: marks from a snapshot/replay-equivalent scenario never fire ─

#[test]
fn ac4_reconcile_latch_feed_drops_replay_equivalent_candidates() {
    // A "scenario equivalent to snapshot/replay": the candidate never
    // reached `take_prompt_marks()`'s live-mark output at all (replay
    // bypasses `on_osc` entirely — see `LatchFeedEvent`'s doc), so
    // `live_marks` here is empty even though candidates exist.
    let feed = vec![
        LatchFeedEvent::Set,
        LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
        LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
    ];
    let resolved = reconcile_latch_feed(feed, &[]);
    assert_eq!(resolved, vec![ResolvedLatchInput::Set]);

    // Feeding the resolved (Mark-free) sequence to the model: no fire.
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    for input in resolved {
        apply_resolved(&mut model, 1, input);
    }
    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        Some(AgentState::Working)
    );
}

// ── AC-5: marks captured on the alternate screen never fire ───────────

#[test]
fn ac5_reconcile_latch_feed_drops_alt_screen_suppressed_candidates() {
    // Two D candidates observed by `on_osc` (fires unconditionally),
    // but `take_prompt_marks()` (alt-screen-gated) only ever captured
    // the SECOND one — the first was suppressed while on the alt
    // screen. `live_marks` reflects that: only one CommandEnd.
    let feed = vec![
        LatchFeedEvent::Set,
        LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd), // alt-screen: suppressed
        LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd), // live
        LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart), // live
    ];
    let live_marks = [PromptMarkKind::CommandEnd, PromptMarkKind::PromptStart];
    let resolved = reconcile_latch_feed(feed, &live_marks);
    assert_eq!(
        resolved,
        vec![
            ResolvedLatchInput::Set,
            ResolvedLatchInput::Mark(PromptMarkKind::CommandEnd),
            ResolvedLatchInput::Mark(PromptMarkKind::PromptStart),
        ],
        "the alt-screen-suppressed D candidate is dropped, not the live pair"
    );

    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    for input in resolved {
        apply_resolved(&mut model, 1, input);
    }
    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        None,
        "the live D/A pair still fires normally"
    );
}

#[test]
fn ac5_alt_screen_suppressed_d_a_pair_never_reaches_the_model() {
    // The alt-screen scenario that must NOT fire: a D/A pair observed
    // by `on_osc` while on the alt screen never appears in
    // `take_prompt_marks()`'s live output at all.
    let feed = vec![
        LatchFeedEvent::Set,
        LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
        LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
    ];
    let resolved = reconcile_latch_feed(feed, &[]); // nothing was live
    assert_eq!(resolved, vec![ResolvedLatchInput::Set]);

    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    for input in resolved {
        apply_resolved(&mut model, 1, input);
    }
    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        Some(AgentState::Working)
    );
}

// ── AC-6: closing a tab discards its latch instance too ───────────────

#[test]
fn ac6_discard_removes_latch_so_a_later_mark_cannot_resurrect_state() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    model.record_latch_set(1);
    model.drain_transitions();

    model.discard(&tab(1));
    assert!(model.status(&tab(1)).is_none());

    // A stray D/A pair for the now-closed tab id creates a FRESH
    // (unarmed) latch and must not resurrect an entry or fire.
    model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
    model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);
    assert!(model.status(&tab(1)).is_none());
    assert!(model.drain_transitions().is_empty());
}

// ── AC-7: no OSC 133 ever -> Set persists indefinitely (regression) ───

#[test]
fn ac7_tab_without_osc133_support_never_auto_clears() {
    let mut model = AgentStatusModel::new();
    model.apply_plain_tab_event(1, set_event(AgentState::Working));
    model.record_latch_set(1);
    model.drain_transitions();

    // No live marks ever arrive for this tab (shell has no OSC 133
    // integration) — the icon must stay exactly as reported.
    assert_eq!(
        model.status(&tab(1)).unwrap().state,
        Some(AgentState::Working)
    );
    assert!(model.drain_transitions().is_empty());
}

// ── reconcile_latch_feed: ordering + pass-through behavior ────────────

#[test]
fn reconcile_latch_feed_preserves_true_relative_order() {
    let feed = vec![
        LatchFeedEvent::Set,
        LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
        LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
        LatchFeedEvent::Clear,
    ];
    let live_marks = [PromptMarkKind::CommandEnd, PromptMarkKind::PromptStart];
    let resolved = reconcile_latch_feed(feed, &live_marks);
    assert_eq!(
        resolved,
        vec![
            ResolvedLatchInput::Set,
            ResolvedLatchInput::Mark(PromptMarkKind::CommandEnd),
            ResolvedLatchInput::Mark(PromptMarkKind::PromptStart),
            ResolvedLatchInput::Clear,
        ]
    );
}

#[test]
fn reconcile_latch_feed_empty_feed_yields_empty_resolved() {
    assert_eq!(reconcile_latch_feed(vec![], &[]), vec![]);
}

/// Test helper mirroring what `App::pump_all` does with a
/// [`ResolvedLatchInput`] drained from a tab (see `tabs.rs`'s
/// `pending_latch_inputs` / `app.rs`'s consumer loop).
fn apply_resolved(model: &mut AgentStatusModel, tab_stable_id: u64, input: ResolvedLatchInput) {
    match input {
        ResolvedLatchInput::Set => model.record_latch_set(tab_stable_id),
        ResolvedLatchInput::Clear => model.record_latch_clear(tab_stable_id),
        ResolvedLatchInput::Mark(kind) => model.record_live_prompt_mark(tab_stable_id, kind),
    }
}
