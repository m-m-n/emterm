use super::*;

// ── Phase 2: inbound window reconcile (TS-6..TS-10, TS-16) ────────────

fn pane_created(pane_id: u32) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::PaneCreated,
        pane_id,
        payload: Vec::new(),
    }
}

fn rename_window(pane_id: u32, name: &str) -> MuxMessage {
    // The RenameWindow frame addresses the window by *pane id* (the
    // outbound side at `confirm_mux_rename` sends `pane_ids()[idx]`, and
    // the daemon re-broadcasts the same field). The inbound handler
    // resolves the window from this pane id via `index_of_pane_id`.
    MuxMessage::control(
        MessageType::RenameWindow,
        pane_id,
        &RenameWindowMsg {
            name: name.to_string(),
        },
    )
}

fn pty_exited(pane_id: u32) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::PtyExited,
        pane_id,
        payload: Vec::new(),
    }
}

// ── TS-6: Welcome ingest ──────────────────────────────────────────────

#[test]
fn welcome_seeds_window_list_and_active_index() {
    let mut tab = test_tab();
    let changed = tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 1));
    assert!(changed);
    let g = tab.mux_group.as_ref().expect("group seeded");
    assert_eq!(g.len(), 2);
    assert_eq!(g.windows()[0].name, "shell");
    assert_eq!(g.windows()[1].name, "editor");
    assert_eq!(g.pane_ids(), &[10, 20]);
    assert_eq!(g.active_index(), 1);
    // F3: session name still set.
    assert_eq!(tab.mux_session_name.as_deref(), Some("main"));
}

#[test]
fn welcome_without_windows_preinstalls_empty_group_for_fresh_start() {
    // Fresh-start mux (`pane_count == 0`, `windows == []`): the Welcome
    // handler pre-installs an empty group and dispatches CreateWindow so
    // the daemon's PaneCreated reply can land (1d9ec548 — before that
    // fix, `mux_group` stayed `None` and every keystroke was dropped).
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    let g = tab.mux_group.as_ref().expect("empty group pre-installed");
    assert_eq!(g.len(), 0);
    assert_eq!(tab.mux_session_name.as_deref(), Some("main"));
}

// ── TS-7: PaneCreated append ──────────────────────────────────────────

#[test]
fn pane_created_appends_window_named_terminal_and_activates() {
    let mut tab = test_tab();
    // Seed two windows, then request + confirm a create.
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
    tab.mux_group.as_mut().unwrap().inc_pending_create();
    let changed = tab.apply_mux_message(pane_created(30));
    assert!(changed);
    let g = tab.mux_group.as_ref().unwrap();
    assert_eq!(g.len(), 3);
    assert_eq!(g.windows()[2].name, "Terminal");
    assert_eq!(g.active_index(), 2);
    assert_eq!(g.active_pane_id(), Some(30));
}

#[test]
fn pane_created_without_pending_is_appended_as_daemon_authoritative() {
    // SPEC FR4 / Message Mapping: daemon-pushed PaneCreated is the
    // append-window signal regardless of whether *this* client requested
    // the create. Earlier behavior dropped such frames as "phantom" —
    // that silently lost panes other clients (or daemon-side actions)
    // spawned. Now the daemon is the authority; pending_create is only
    // an optimistic-UX counter for the originating client.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
    let changed = tab.apply_mux_message(pane_created(30));
    assert!(changed);
    assert_eq!(tab.mux_group.as_ref().unwrap().len(), 3);
}

#[test]
fn pane_created_is_idempotent_on_resend() {
    // A duplicate PaneCreated for an already-known pane (bridge replay)
    // must not double-append, otherwise the same pane would surface twice
    // in the sub-tab strip.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
    let first = tab.apply_mux_message(pane_created(30));
    let second = tab.apply_mux_message(pane_created(30));
    assert!(first);
    assert!(!second);
    assert_eq!(tab.mux_group.as_ref().unwrap().len(), 3);
}

#[test]
fn pane_created_before_attach_does_not_install_empty_group() {
    // PaneCreated arriving before Welcome must not allocate an empty
    // MuxWindowGroup on the tab. Otherwise every is_some() check
    // downstream (PtyOutput pane filter, write_input mux branch,
    // mux_session_name badge) would treat the tab as mux-attached even
    // though it isn't.
    let mut tab = test_tab();
    assert!(tab.mux_group.is_none());
    let changed = tab.apply_mux_message(pane_created(30));
    assert!(!changed);
    assert!(tab.mux_group.is_none());
}

// ── TS-8: PtyExited removal + group dissolve ──────────────────────────

#[test]
fn pty_exited_removes_window() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    let changed = tab.apply_mux_message(pty_exited(30));
    assert!(changed);
    let g = tab.mux_group.as_ref().unwrap();
    assert_eq!(g.len(), 2);
    assert_eq!(g.active_index(), 1); // re-clamped
}

#[test]
fn pty_exited_dissolves_group_at_zero() {
    let mut tab = test_tab();
    // One-window group (seeded directly).
    tab.apply_mux_message(welcome_msg(&[(1, "only", 10)], 0));
    tab.apply_mux_message(pty_exited(10));
    assert!(tab.mux_group.is_none());
    // The last window's shell exited: the tab closes (reaped by
    // `App::pump_all`), unlike an explicit detach which keeps it alive.
    assert!(tab.exited);
}

// ── task0005 AC-6: PtyExited latches the closed pane id ────────────────

#[test]
fn pty_exited_latches_closed_pane_for_agent_status_discard() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(pty_exited(10));
    assert_eq!(tab.take_closed_agent_status_panes(), vec![10]);
}

#[test]
fn pty_exited_unknown_pane_does_not_latch() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    tab.apply_mux_message(pty_exited(999));
    assert!(tab.take_closed_agent_status_panes().is_empty());
}

// ── task0005 AC-2: daemon AgentStatusUpdate is decoded and latched ─────

#[test]
fn agent_status_update_decodes_and_latches_for_app_pump_all() {
    let mut tab = test_tab();
    let update = mux_ipc::protocol::AgentStatusUpdateMsg {
        pane_id: 10,
        public_pane_id: "abc-10".to_string(),
        state: Some(mux_ipc::protocol::AgentState::Blocked),
        name: Some("claude".to_string()),
        revision: 3,
        replay_derived: false,
    };
    let msg = MuxMessage::control(MessageType::AgentStatusUpdate, 10, &update);
    let changed = tab.apply_mux_message(msg);
    assert!(changed);
    let latched = tab.take_pending_agent_status_updates();
    assert_eq!(latched.len(), 1);
    assert_eq!(latched[0].pane_id, 10);
    assert_eq!(
        latched[0].state,
        Some(mux_ipc::protocol::AgentState::Blocked)
    );
    assert_eq!(latched[0].revision, 3);
    assert!(!latched[0].replay_derived);
}

#[test]
fn agent_status_update_malformed_payload_is_rejected() {
    let mut tab = test_tab();
    let msg = MuxMessage {
        msg_type: MessageType::AgentStatusUpdate,
        pane_id: 10,
        payload: vec![0xFF, 0xFF, 0xFF], // not a valid bincode AgentStatusUpdateMsg
    };
    let changed = tab.apply_mux_message(msg);
    assert!(!changed);
    assert!(tab.take_pending_agent_status_updates().is_empty());
}

// ── agent-exit-after-icon (task0002): latch feed reconciliation,
// end-to-end via `test_process_combined` (real OSC byte parsing through
// `NativeCallbacks::on_osc` + `process_outer_via_core`'s reconciliation
// — the actual callbacks.rs -> latch-feed -> reconcile path, per
// task0002.md's Test Notes) ─────────────────────────────────────────

#[test]
fn latch_feed_end_to_end_set_then_live_d_a_resolves_in_order() {
    let mut tab = test_tab();
    let bytes = [
        b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
        b"\x1b]133;D;0\x07",
        b"\x1b]133;A\x07",
    ]
    .concat();
    tab.test_process_combined(bytes);

    assert_eq!(
        tab.take_pending_latch_inputs(),
        vec![
            crate::agent_status_model::ResolvedLatchInput::Set,
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::CommandEnd
            ),
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::PromptStart
            ),
        ]
    );
}

#[test]
fn latch_feed_end_to_end_drops_alt_screen_suppressed_candidate() {
    // AC-5: a D mark emitted while on the alternate screen is captured
    // by `on_osc` (candidate) but never reaches `take_prompt_marks()`
    // (term_core's alt-screen gate) — so it must not resolve, while the
    // later live D/A pair (after leaving the alt screen) still does.
    let mut tab = test_tab();
    let bytes = [
        b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
        b"\x1b[?1049h",      // enter alt screen
        b"\x1b]133;D;0\x07", // suppressed candidate
        b"\x1b[?1049l",      // leave alt screen
        b"\x1b]133;D;0\x07", // live
        b"\x1b]133;A\x07",   // live
    ]
    .concat();
    tab.test_process_combined(bytes);

    assert_eq!(
        tab.take_pending_latch_inputs(),
        vec![
            crate::agent_status_model::ResolvedLatchInput::Set,
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::CommandEnd
            ),
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::PromptStart
            ),
        ],
        "the alt-screen-suppressed D candidate is dropped; only the live pair resolves"
    );
}

// ── task0005 rework (review round1 findings 6b2e83f10c94ad7e /
// 929859ff2b4e431e / 5cd6f305dcdeceb7): mux-attached inner OSC 777 /
// OSC 133 must never populate the GUI-local plain-tab agent-status
// queue or inferred-clear latch — the daemon's `AgentStatusUpdate` /
// `MuxPane.agent_status_exit_latch` are the sole authority for mux
// panes (SPEC FR3). Uses `mux_tab_active_pane` / `pty_output_apc`
// (defined further below, in the mux coalesce test section) to route
// OSC bytes through the mux inner-content path exactly as the daemon's
// `PtyOutput` frames would. ─────────────────────────────────────────

#[test]
fn plain_tab_agent_status_set_surfaces_via_pending_agent_status_events() {
    // AC-4 regression guard: a non-mux (plain) tab's OSC 777 Set must
    // still reach `take_pending_agent_status_events()` — this rework
    // moved WHERE `pending_agent_status` is drained within
    // `process_combined`, not WHETHER a plain tab's events are drained.
    let mut tab = test_tab();
    tab.test_process_combined(b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".to_vec());
    assert_eq!(
        tab.take_pending_agent_status_events(),
        vec![crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Working,
            name: None,
        }]
    );
}

#[test]
fn mux_inner_agent_status_set_does_not_create_plain_tab_status() {
    // AC-1: an inner OSC 777 `Set` carried by a mux pane's `PtyOutput`
    // must not populate the GUI-local `pending_agent_status_events`
    // queue that `App::pump_all` applies as a `PaneKey::Tab` status —
    // neither the SAME pump that parsed it, nor a LATER pump (a
    // per-pump-delayed drain of the same stale queue is just as much a
    // leak, only postponed).
    let mut tab = mux_tab_active_pane(10);
    let combined = pty_output_apc(10, b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
    tab.test_process_combined(combined);
    // A second, still-mux pump with no new bytes: proves the mux-inner
    // Set from the first pump cannot surface on a later drain either.
    tab.test_process_combined(Vec::new());
    assert!(
        tab.take_pending_agent_status_events().is_empty(),
        "a mux pane's inner OSC 777 Set must not create a GUI-local tab status"
    );
}

#[test]
fn mux_inner_agent_status_set_then_da_leaves_no_residual_plain_tab_status() {
    // AC-2/AC-3: a full Set + D + A sequence inside the mux inner
    // stream — which would arm and fire the plain-tab inferred-clear
    // latch if it were live plain-tab content — must leave neither a
    // residual GUI-local Set nor any latch candidates once mux-owned,
    // including on a later pump's drain (see the sibling test above).
    let mut tab = mux_tab_active_pane(10);
    let inner = [
        b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\".as_slice(),
        b"\x1b]133;D;0\x07",
        b"\x1b]133;A\x07",
    ]
    .concat();
    let combined = pty_output_apc(10, &inner);
    tab.test_process_combined(combined);
    tab.test_process_combined(Vec::new());
    assert!(
        tab.take_pending_agent_status_events().is_empty(),
        "no residual GUI-local Set after a mux-inner D->A pair"
    );
    assert!(
        tab.take_pending_latch_inputs().is_empty(),
        "mux-inner OSC 133/777 candidates must not feed the plain-tab inferred-clear latch"
    );
}

#[test]
fn mux_inner_candidates_do_not_leak_into_same_pump_post_detach_tail() {
    // AC-3/AC-5 explicit scenario: one coalesced pump carries, in
    // order: (1) a mux inner `PtyOutput` with an OSC 777 Set
    // (mux-pane-owned — must be discarded), (2) the `Detached` control
    // frame, (3) plain shell bytes the now-reattached shell printed,
    // carrying its OWN OSC 777 Set + OSC 133 D/A (plain-tab-owned —
    // must resolve normally, AC-4). Before the fix, the mux-inner
    // Set/marks were queued ahead of the tail re-route's own
    // `process_outer_via_core` call and got taken together with it.
    let mut tab = mux_tab_active_pane(10);

    let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });

    let mut combined = pty_output_apc(10, b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
    combined.extend_from_slice(&detached);
    combined.extend_from_slice(b"\x1b]777;emterm;agent-status;v=1;state=working\x1b\\");
    combined.extend_from_slice(b"\x1b]133;D;0\x07");
    combined.extend_from_slice(b"\x1b]133;A\x07");

    tab.test_process_combined(combined);

    assert!(
        tab.mux_session_name.is_none(),
        "Detached frame must clear mux_session_name"
    );

    let events = tab.take_pending_agent_status_events();
    assert_eq!(
        events,
        vec![crate::agent_status::AgentStatusEvent::Set {
            state: crate::agent_status::AgentState::Working,
            name: None,
        }],
        "only the post-detach plain-tab Set must surface, not the mux-inner one; events={events:?}"
    );

    assert_eq!(
        tab.take_pending_latch_inputs(),
        vec![
            crate::agent_status_model::ResolvedLatchInput::Set,
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::CommandEnd
            ),
            crate::agent_status_model::ResolvedLatchInput::Mark(
                crate::prompts::PromptMarkKind::PromptStart
            ),
        ],
        "only the post-detach live Set/D/A latch candidates must resolve; mux-inner \
         candidates discarded"
    );
}

// ── close-reconcile decision (FR1/FR2/FR3) ────────────────────────────

// TS-1: the active window's shell exits in a 3-window group → the decision
// helper returns the now-active pane id (a snapshot reconcile is wanted).
#[test]
fn close_reconcile_active_window_close_returns_new_active_pane() {
    let mut tab = test_tab();
    // Active index 2 (pane 30) is the displayed window.
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    let before = tab.mux_group.as_ref().unwrap().active_pane_id();
    assert_eq!(before, Some(30));
    // Close the active window.
    let changed = tab.apply_mux_message(pty_exited(30));
    assert!(changed);
    let after = tab.mux_group.as_ref().unwrap().active_pane_id();
    // The re-clamp moved active onto pane 20; the helper wants its snapshot.
    assert_eq!(after, Some(20));
    assert_eq!(Tab::close_reconcile_target(before, after), Some(20));
}

// TS-2: a non-active window's shell exits → the helper returns None even
// though the active *index* shifts, because the displayed pane id is
// unchanged.
#[test]
fn close_reconcile_nonactive_window_close_returns_none() {
    let mut tab = test_tab();
    // Active index 2 (pane 30). Close an EARLIER window (pane 10) so the
    // active index re-clamps from 2 → 1 yet still points at pane 30.
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    let before = tab.mux_group.as_ref().unwrap().active_pane_id();
    assert_eq!(before, Some(30));
    let changed = tab.apply_mux_message(pty_exited(10));
    assert!(changed);
    let g = tab.mux_group.as_ref().unwrap();
    // Index shifted 2 → 1 but the displayed window (pane 30) is unchanged.
    assert_eq!(g.active_index(), 1);
    let after = g.active_pane_id();
    assert_eq!(after, Some(30));
    assert_eq!(Tab::close_reconcile_target(before, after), None);
}

// TS-3: the last remaining window's shell exits → the group empties, the
// tab is marked exited, and the helper returns None (no reconcile).
#[test]
fn close_reconcile_last_window_close_returns_none() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "only", 10)], 0));
    let before = tab.mux_group.as_ref().unwrap().active_pane_id();
    assert_eq!(before, Some(10));
    let changed = tab.apply_mux_message(pty_exited(10));
    assert!(changed);
    // Group emptied → no displayed pane → helper returns None.
    assert!(tab.mux_group.is_none());
    assert!(tab.exited);
    assert_eq!(Tab::close_reconcile_target(before, None), None);
}

// TS-4: PtyExited for an unknown pane id → no removal, no change, helper
// input is unchanged so it would yield None.
#[test]
fn close_reconcile_unknown_pane_is_noop() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let before = tab.mux_group.as_ref().unwrap().active_pane_id();
    let changed = tab.apply_mux_message(pty_exited(999));
    assert!(!changed);
    let g = tab.mux_group.as_ref().unwrap();
    assert_eq!(g.len(), 2, "no window removed");
    let after = g.active_pane_id();
    assert_eq!(after, before, "active unchanged");
    assert_eq!(Tab::close_reconcile_target(before, after), None);
}

// TS-5: several PtyExited for distinct panes drain in one pump → the final
// active window is the one that needs reconciling. The helper, fed the
// pre-pump active id against the post-pump active id, names the survivor.
#[test]
fn close_reconcile_multi_exit_in_one_pump_targets_final_active() {
    let mut tab = test_tab();
    // Active index 2 (pane 30).
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    let before = tab.mux_group.as_ref().unwrap().active_pane_id();
    assert_eq!(before, Some(30));
    // Two distinct windows exit in the same pump: first the active (pane
    // 30 → re-clamp onto pane 20), then pane 20 (→ re-clamp onto pane 10).
    assert!(tab.apply_mux_message(pty_exited(30)));
    assert!(tab.apply_mux_message(pty_exited(20)));
    let g = tab.mux_group.as_ref().unwrap();
    assert_eq!(g.len(), 1);
    let after = g.active_pane_id();
    assert_eq!(after, Some(10), "final active window survives");
    // Reconcile target is the final active window, not an intermediate one.
    assert_eq!(Tab::close_reconcile_target(before, after), Some(10));
}

// TS-6: regression — inbound SwitchWindow still syncs the active index and
// reconciles the now-active window (the close fix must not alter it).
#[test]
fn switch_window_still_reconciles_after_close_fix() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let changed = tab.apply_mux_message(switch_window(20));
    assert!(changed, "an inbound switch still reports a visible change");
    assert_eq!(
        tab.mux_group.as_ref().unwrap().active_index(),
        1,
        "the active index is synced to the switched-to window"
    );
    assert_eq!(
        tab.take_pending_pane_switch(),
        Some(10),
        "the outgoing pane is still latched for the App-side scroll save"
    );
}

// TS-7: closing the active mux window latches the exited pane id so
// App::pump_all reloads the now-active pane's saved scroll position,
// mirroring the SwitchWindow path. Closing a NON-active window must
// NOT latch (the displayed pane did not change).
#[test]
fn close_reconcile_latches_outgoing_pane_for_scroll_reload() {
    // Active window close: pane 30 (active) exits → group re-clamps to
    // pane 20. The exited pane id (30) must be latched so the App-side
    // scroll restore runs. index_of_pane_id(30) will return None (already
    // removed), so the park is skipped and only active_pane_scroll() for
    // the new active (20) is reloaded — by App::pump_all's existing block.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(30));
    let changed = tab.apply_mux_message(pty_exited(30));
    assert!(changed);
    assert_eq!(
        tab.take_pending_pane_switch(),
        Some(30),
        "the exited active pane id must be latched for the App-side scroll reload"
    );
    // One-shot: consumed by take_pending_pane_switch.
    assert_eq!(tab.take_pending_pane_switch(), None);

    // Non-active window close: pane 10 (non-active) exits → the displayed
    // pane (30, active) is unchanged. No latch should be set.
    let mut tab2 = test_tab();
    tab2.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 2));
    assert_eq!(tab2.mux_group.as_ref().unwrap().active_pane_id(), Some(30));
    let changed2 = tab2.apply_mux_message(pty_exited(10));
    assert!(changed2);
    assert_eq!(
        tab2.take_pending_pane_switch(),
        None,
        "closing a non-active window must not latch (displayed pane unchanged)"
    );
}

// ── Detached: exit mux mode (group + session name cleared) ────────────

#[test]
fn detached_clears_group_and_session_name() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
    assert!(tab.mux_group.is_some());
    assert_eq!(tab.mux_session_name.as_deref(), Some("main"));

    let changed = tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    assert!(changed);
    // The tab reverts to a plain tab: no group, no mux session badge.
    assert!(tab.mux_group.is_none());
    assert!(tab.mux_session_name.is_none());
}

#[test]
fn detached_clears_displayed_grid() {
    // After detach the bridge exits and the shell reprints its prompt; the
    // stale mux window content must not linger in the displayed grid.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
    // Paint a visible marker into the displayed core.
    tab.core.lock().process_pty_data_fully(b"STALE");
    assert_eq!(tab.core.lock().get_cell_char(0, 0), "S");

    let changed = tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    assert!(changed);
    // Grid reset to blank via reset_and_replay(b"").
    let c = tab.core.lock();
    let row0: String = (0..c.cols()).map(|col| c.get_cell_char(col, 0)).collect();
    assert!(
        row0.trim().is_empty(),
        "detach must clear the stale mux grid, got {row0:?}"
    );
}

#[test]
fn detached_cancels_in_flight_offthread_switch() {
    // A window switch dispatched just before detach (snapshot >= the
    // off-thread threshold) must not resolve after detach: otherwise a
    // later poll_pending_switch would swap the detached window's
    // worker-built core back over the grid the Detached arm just cleared.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("STALE")));
    assert!(
        tab.test_has_pending_switch(),
        "snapshot at/above threshold must enter the off-thread path"
    );

    let changed = tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    assert!(changed);
    // The in-flight switch is cancelled and dropped, so no later
    // poll_pending_switch can swap the detached content back in.
    assert!(
        !tab.test_has_pending_switch(),
        "detach must cancel the in-flight off-thread switch"
    );
}

// ── mux-detach-agent-status-cleanup task0001 AC-1/AC-2: detach queues
// the departing group's wire pane ids onto the closed-agent-status-pane
// latch, without double-pushing an id the pane-exit path already
// queued ──────────────────────────────────────────────────────────────

#[test]
fn detached_queues_group_pane_ids_for_agent_status_drain_then_drains_once() {
    // AC-1 (SPEC AC-1; TS-5): a daemon-confirmed detach on a tab holding a
    // seeded mux group makes the closed-agent-status-pane drain return
    // exactly that group's wire pane ids; a second, immediately following
    // drain in the same pump returns nothing (single-shot).
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let changed = tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    assert!(changed);
    let mut drained = tab.take_closed_agent_status_panes();
    drained.sort_unstable();
    assert_eq!(
        drained,
        vec![10, 20],
        "the drain returns exactly the departing group's wire pane ids"
    );
    assert!(
        tab.take_closed_agent_status_panes().is_empty(),
        "a second drain in the same pump returns nothing"
    );
}

#[test]
fn pane_exit_emptying_group_does_not_double_queue_pane_ids() {
    // AC-2 (SPEC AC-1; TS-6): a pane-exit sequence that removes every
    // window of a group yields each removed wire pane id exactly once —
    // the detach-side queueing (added for AC-1) must never combine with
    // the pane-exit path's own queueing to double-push an id. This
    // scenario never reaches the detach handler at all (the group empties
    // via PtyExited, not Detached), so it is a guard against the AC-1
    // change regressing this path, not a red-before/green-after case.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(pty_exited(10));
    tab.apply_mux_message(pty_exited(20));
    assert!(tab.mux_group.is_none(), "the group emptied");
    let mut drained = tab.take_closed_agent_status_panes();
    drained.sort_unstable();
    assert_eq!(
        drained,
        vec![10, 20],
        "each removed pane id is queued exactly once"
    );
}

// ── TS-9: RenameWindow inbound ────────────────────────────────────────

#[test]
fn rename_window_updates_label_by_pane_id() {
    // Inbound RenameWindow addresses the window by its active pane id —
    // window_id 2 has active_pane_id 20 per the welcome below, so the
    // rename frame's pane_id is 20 (matching the wire shape the daemon
    // re-broadcasts after our outbound `confirm_mux_rename` send).
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10), (2, "editor", 20)], 0));
    let changed = tab.apply_mux_message(rename_window(20, "vim"));
    assert!(changed);
    assert_eq!(tab.mux_group.as_ref().unwrap().windows()[1].name, "vim");
}

#[test]
fn rename_window_unknown_pane_id_is_noop() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
    // pane_id 999 isn't in the group → no rename.
    let changed = tab.apply_mux_message(rename_window(999, "vim"));
    assert!(!changed);
}

// ── TS-10: SwitchWindow inbound ───────────────────────────────────────

#[test]
fn switch_window_syncs_active_index_by_pane() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let changed = tab.apply_mux_message(switch_window(20));
    assert!(changed);
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
}

#[test]
fn switch_window_unknown_pane_is_noop() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    let changed = tab.apply_mux_message(switch_window(999));
    assert!(!changed);
}

// ── FR3 pane wiring: inbound SwitchWindow latches the outgoing index ───

#[test]
fn inbound_switch_latches_outgoing_pane_index() {
    // A real inbound switch (active index moves 0 → 1) records the
    // outgoing pane id (10) so `App::pump_all` can park its scroll position.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    assert!(tab.apply_mux_message(switch_window(20)));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
    assert_eq!(
        tab.take_pending_pane_switch(),
        Some(10),
        "the outgoing pane id (10) is latched for the App-side scroll save"
    );
    // The latch is one-shot.
    assert_eq!(tab.take_pending_pane_switch(), None);
}

#[test]
fn inbound_switch_to_same_pane_does_not_latch() {
    // Switching onto the already-active pane must not latch a transition
    // (no scroll save/restore, no forced redraw).
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // active is 0 (pane 10); switch to pane 10 again.
    let changed = tab.apply_mux_message(switch_window(10));
    assert!(changed, "set_active_by_pane still reports a match");
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 0);
    assert_eq!(
        tab.take_pending_pane_switch(),
        None,
        "a no-op switch onto the current pane latches nothing"
    );
}

#[test]
fn inbound_switch_unknown_pane_does_not_latch() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    assert!(!tab.apply_mux_message(switch_window(999)));
    assert_eq!(tab.take_pending_pane_switch(), None);
}

#[test]
fn inbound_multiple_switches_in_one_pump_latch_first_only() {
    // Several SwitchWindow messages can drain in one `pump` before
    // `App::pump_all` consumes the latch. A→B→C must keep the FIRST
    // outgoing pane (A, id 10) — that is the genuinely-displayed pane whose
    // live scroll must be parked; the intermediate pane (B) was never
    // rendered. Overwriting with B would corrupt two panes' slots.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 0));
    assert!(tab.apply_mux_message(switch_window(20))); // 0 → 1, latch pane 10
    assert!(tab.apply_mux_message(switch_window(30))); // 1 → 2, must NOT overwrite
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 2);
    assert_eq!(
        tab.take_pending_pane_switch(),
        Some(10),
        "only the first outgoing pane id of the pump is latched"
    );
}

#[test]
fn pane_created_latches_outgoing_index_for_scroll_save() {
    // Creating a new window makes it the active sub-tab. That is a third
    // unit-switch path: latch the outgoing pane id so `App::pump_all` parks
    // the outgoing pane's scroll and resets the new (empty) pane to Live.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    assert!(tab.apply_mux_message(pane_created(20)));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 1);
    assert_eq!(
        tab.take_pending_pane_switch(),
        Some(10),
        "the outgoing pane id (10) is latched on new-window create"
    );
}

#[test]
fn pane_created_latches_pending_window_appended_fr6() {
    // FR6 (mux): a PaneCreated that pushes — and so activates — a new window
    // latches the one-shot scroll-into-view signal that App::pump_all drains
    // for the active tab. Latched at the push site (not inferred from a
    // window-count delta), so it is a single, unambiguous event.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    assert!(!tab.take_pending_window_appended(), "baseline: not latched");
    assert!(tab.apply_mux_message(pane_created(20)));
    assert!(
        tab.take_pending_window_appended(),
        "PaneCreated that appended + activated a window latches the FR6 signal"
    );
    assert!(
        !tab.take_pending_window_appended(),
        "one-shot: the latch is cleared by take"
    );
}

#[test]
fn window_appended_latch_survives_same_pump_pane_exit() {
    // Regression: the FR6 mux signal was previously inferred from a window-
    // count delta, which a same-pump PtyExited (removing a *different* pane)
    // could mask — PaneCreated (+1) and PtyExited (−1) net to zero, so the
    // delta missed the new active window. The push-site latch is immune: a
    // PaneCreated that activated a new window still latches even when another
    // pane exits in the same pump.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // active = pane 10 (index 0). Create a new window (pane 30) → pushed and
    // activated; then a different pane (20) exits in the same pump. Net
    // window count is unchanged (2 → 3 → 2), the case a count delta missed.
    assert!(tab.apply_mux_message(pane_created(30)));
    let _ = tab.apply_mux_message(pty_exited(20));
    assert!(
        tab.take_pending_window_appended(),
        "the FR6 latch survives a same-pump exit of a different pane"
    );
}

#[test]
fn latched_outgoing_pane_survives_same_pump_pane_removal() {
    // Regression: the latch stores the outgoing pane *id*, not its index,
    // so a same-pump `PtyExited` that removes a different pane (shifting the
    // parallel arrays) cannot make the consumer park the outgoing scroll
    // into the wrong slot. Sequence in one pump: active = pane 20 (index 1);
    // switch to pane 30 (latch outgoing = pane 20); then pane 10 exits,
    // shifting pane 20 from index 1 → 0. The latch still resolves to pane
    // 20's NEW index (0); an index-based latch would have pointed at pane 30.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20), (3, "c", 30)], 1));
    assert!(tab.apply_mux_message(switch_window(30))); // active 1 → 2, latch pane 20
    assert!(tab.apply_mux_message(pty_exited(10))); // removes index 0, arrays shift
    let latched = tab.take_pending_pane_switch();
    assert_eq!(
        latched,
        Some(20),
        "latch holds the outgoing pane id, not its index"
    );
    // The consumer resolves the id to its CURRENT index (pane 20 is now 0).
    let idx = tab
        .mux_group
        .as_ref()
        .unwrap()
        .index_of_pane_id(latched.unwrap());
    assert_eq!(
        idx,
        Some(0),
        "outgoing pane 20 resolved to its post-removal index"
    );
}

#[test]
fn latched_outgoing_pane_skipped_when_it_exits_same_pump() {
    // When the outgoing pane itself exits in the same pump, its scroll slot
    // is gone; the consumer's `index_of_pane_id` returns `None` and the
    // park is skipped (no panic, nothing parked into a stale slot).
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    assert!(tab.apply_mux_message(switch_window(20))); // active 0 → 1, latch pane 10
    assert!(tab.apply_mux_message(pty_exited(10))); // outgoing pane 10 exits
    let latched = tab.take_pending_pane_switch();
    assert_eq!(latched, Some(10));
    assert_eq!(
        tab.mux_group
            .as_ref()
            .unwrap()
            .index_of_pane_id(latched.unwrap()),
        None,
        "exited outgoing pane resolves to no index → consumer skips the park"
    );
}

// ── TS-16: scripted inbound sequence ──────────────────────────────────

#[test]
fn inbound_sequence_attach_create_switch_rename_exit() {
    let mut tab = test_tab();
    // attach: one window (id 1, pane 10).
    tab.apply_mux_message(welcome_msg(&[(1, "shell", 10)], 0));
    // create: request then confirm → fresh window (id 2, pane 50).
    tab.mux_group.as_mut().unwrap().inc_pending_create();
    tab.apply_mux_message(pane_created(50));
    let _created_id = {
        let g = tab.mux_group.as_ref().unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(g.active_index(), 1); // new window active
        g.windows()[1].id
    };
    // switch back to the first pane.
    tab.apply_mux_message(switch_window(10));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_index(), 0);
    // rename the freshly created window: addressed by its *pane id* (50)
    // on the wire, matching the inbound contract.
    tab.apply_mux_message(rename_window(50, "build"));
    assert_eq!(tab.mux_group.as_ref().unwrap().windows()[1].name, "build");
    // exit the created pane → drop back to one window. WebView parity:
    // the group still renders (one numbered sub-tab); it only dissolves
    // when the last window exits (the `Option` is cleared at zero).
    tab.apply_mux_message(pty_exited(50));
    let g = tab.mux_group.as_ref().unwrap();
    assert_eq!(g.len(), 1);
    assert!(g.is_group());
}
