use super::*;

// ── task0004 AC-5: RIS restores an OSC 12 cursor-color override ────

#[test]
fn ris_bytes_restore_theme_cursor_color_to_scheme() {
    // Feeding RIS bytes after OSC 12 (AC-5) through the real
    // core -> NativeCallbacks -> theme wiring restores the resolved
    // cursor color to the scheme cursor color and clears the override
    // state, exactly as OSC 112 would.
    let tab = test_tab();
    {
        let mut theme = tab.theme.lock();
        assert!(theme.apply_osc(12, "rgb:aa/bb/cc"));
        assert!(theme.cursor_fg_override_active);
    }

    tab.core.lock().process_pty_data_fully(b"\x1bc"); // RIS

    let theme = tab.theme.lock();
    assert_eq!(theme.cursor_fg, theme.scheme_cursor_fg);
    assert!(!theme.cursor_fg_override_active);
}

#[test]
fn ris_bytes_after_reversed_order_still_apply_later_osc12() {
    // Guards the callback-based (in-order) design: a RIS followed BY a
    // fresh OSC 12 in the SAME chunk must leave the later OSC 12's
    // color in effect — the reset restore must not run after the whole
    // chunk is parsed and clobber a color set later in the same chunk.
    let tab = test_tab();
    tab.core
        .lock()
        .process_pty_data_fully(b"\x1bc\x1b]12;rgb:11/22/33\x07");

    let theme = tab.theme.lock();
    assert_eq!(theme.cursor_fg, crate::render::theme::Rgb(0x11, 0x22, 0x33));
    assert!(theme.cursor_fg_override_active);
}

// ── Off-thread snapshot replay (Phase 2/3/4) ──────────────────────────

// ── task0003 D3 (review round-2 findings `200b2c8beeb68fe4` /
// `87ba3cc2911d104e`): Snapshot|SnapshotRestore pane filter ───────────

/// AC-3: with two or more mux windows, a reattach-shaped
/// `SnapshotRestore` for a NON-active pane must not overwrite the tab's
/// displayed core.
#[test]
fn snapshot_restore_for_non_active_pane_does_not_overwrite_displayed_core() {
    let mut tab = test_tab();
    // Two windows: pane 10 (index 0, active) and pane 20 (index 1).
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(10));

    // Paint identifiable content into the displayed core first.
    {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(b"ACTIVE-A");
    }

    // A reattach-shaped SnapshotRestore arrives for the NON-active pane
    // (20) — this is exactly what `send_reattach_data` emits per pane in
    // the session, relying on the client to pick the right one.
    let msg = MuxMessage {
        msg_type: MessageType::SnapshotRestore,
        pane_id: 20,
        payload: b"NON-ACTIVE-B\r\n".to_vec(),
    };
    let changed = tab.apply_mux_message(msg);
    assert!(
        !changed,
        "a non-active pane's snapshot must be dropped (no redraw signalled)"
    );

    let c = tab.core.lock();
    let row0: String = (0..8).map(|col| c.get_cell_char(col, 0)).collect();
    assert_eq!(
        row0, "ACTIVE-A",
        "the displayed core must still show the active pane's content, \
         not the non-active pane's snapshot"
    );
}

/// AC-4: same fix, exercised via `MessageType::Snapshot` (the
/// visibility-resume shape — `resume_pane_with_permit` sends this kind)
/// and the OFF-THREAD (>= 64 KiB) path — a resume snapshot for a
/// NON-active pane must not engage the off-thread swap or otherwise
/// touch the displayed core; the SAME shape for the active pane still
/// does.
#[test]
fn resume_snapshot_for_non_active_pane_does_not_trigger_offthread_swap() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    assert_eq!(tab.mux_group.as_ref().unwrap().active_pane_id(), Some(10));

    {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(b"ACTIVE-A");
    }

    let changed = tab.apply_mux_message(snapshot_msg(20, large_payload("NON-ACTIVE-B")));
    assert!(
        !changed,
        "a non-active pane's resume snapshot must be dropped"
    );
    assert!(
        !tab.test_has_pending_switch(),
        "a non-active pane's resume snapshot must never engage the off-thread swap"
    );

    // Sanity: the SAME shape for the ACTIVE pane DOES engage the swap.
    let changed = tab.apply_mux_message(snapshot_msg(10, large_payload("ACTIVE-A-RESUMED")));
    assert!(changed);
    assert!(tab.test_has_pending_switch());
}

/// TS-4: exactly at the threshold goes off-thread; one byte below stays
/// synchronous (no pending switch).
#[test]
fn ts4_threshold_boundary_sync_vs_offthread() {
    // One byte below threshold → synchronous, no pending switch.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let below = vec![b'x'; OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1];
    tab.apply_mux_message(snapshot_msg(10, below));
    assert!(
        !tab.test_has_pending_switch(),
        "sub-threshold snapshot must stay synchronous"
    );

    // Exactly at the threshold → off-thread, pending switch entered.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let at = vec![b'x'; OFFTHREAD_REPLAY_THRESHOLD_BYTES];
    tab.apply_mux_message(snapshot_msg(10, at));
    assert!(
        tab.test_has_pending_switch(),
        "at-threshold snapshot must go off-thread"
    );
    // Active pane (index 0 → pane 10) is the queue target.
    assert_eq!(tab.test_pending_target(), Some(10));
}

/// AC-5 (task0005 rework D3'', review round-4 finding
/// `b1de83542bfe60bc`): a small-payload (well under
/// `OFFTHREAD_REPLAY_THRESHOLD_BYTES`), many-segment snapshot (at least
/// `OFFTHREAD_REPLAY_SEGMENT_THRESHOLD` entries) must dispatch
/// off-thread — the byte-size check alone would keep this synchronous,
/// defeating the purpose since each segment's reflow cost does not
/// scale with the segment's own byte count.
///
/// Confirmed to fail pre-fix: before the segment-count branch existed,
/// a payload this small (well under 64 KiB) with many segments stayed
/// on the synchronous path regardless of segment count — this test's
/// `test_has_pending_switch()` assertion would have been `false`.
#[test]
fn ac5_small_payload_many_segment_snapshot_dispatches_off_thread() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

    let content = b"tiny".to_vec();
    let segments: Vec<mux_ipc::protocol::DimSegment> = (0..OFFTHREAD_REPLAY_SEGMENT_THRESHOLD)
        .map(|i| mux_ipc::protocol::DimSegment {
            offset: 0,
            cols: 80 + i as u16,
            rows: 24,
        })
        .collect();
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    assert!(
        encoded.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES,
        "test prerequisite: encoded payload must stay well under the \
         byte-size threshold, got {}",
        encoded.len()
    );

    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(
        tab.test_has_pending_switch(),
        "a small-payload snapshot at the segment-count threshold must \
         still dispatch off-thread"
    );
}

/// The byte-size threshold alone still governs when segment count is
/// LOW — a small payload with only a couple of segments stays
/// synchronous, exactly as before this fix. Pins the "no change to the
/// common case" half of the AC-5 contract.
#[test]
fn ac5_small_payload_low_segment_count_stays_synchronous() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

    let content = b"tiny".to_vec();
    let segments = vec![mux_ipc::protocol::DimSegment {
        offset: 0,
        cols: 80,
        rows: 24,
    }];
    assert!(segments.len() < OFFTHREAD_REPLAY_SEGMENT_THRESHOLD);
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);

    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(
        !tab.test_has_pending_switch(),
        "a small payload with a low segment count must stay synchronous"
    );
}

/// FR1: a large snapshot dispatch must NOT mutate the displayed core —
/// the outgoing pane stays visible until the swap.
#[test]
fn ts4_offthread_dispatch_leaves_displayed_core_intact() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // Paint the outgoing pane's content into the displayed core.
    {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(b"OUTGOING");
    }
    tab.apply_mux_message(snapshot_msg(10, large_payload("INCOMING")));
    assert!(tab.test_has_pending_switch());
    // Displayed core still shows the outgoing content (not reset).
    let c = tab.core.lock();
    let row0: String = (0..8).map(|col| c.get_cell_char(col, 0)).collect();
    assert_eq!(row0, "OUTGOING");
}

/// FR3 / TS-3 (queue): target-pane live output during a pending switch is
/// queued in arrival order, not applied to the displayed core; output for
/// a different pane is dropped.
#[test]
fn ts3_live_output_queued_during_pending_switch() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
    assert_eq!(tab.test_pending_target(), Some(10));

    // Two live chunks for the target pane → queued in order.
    tab.apply_mux_message(pty_output(10, b"first".to_vec()));
    tab.apply_mux_message(pty_output(10, b"second".to_vec()));
    // A chunk for a non-target pane → dropped (the PtyOutput pane filter
    // also drops non-active panes, but the pending guard covers it too).
    tab.apply_mux_message(pty_output(20, b"other".to_vec()));

    assert_eq!(
        tab.test_pending_live_queue(),
        vec![b"first".to_vec(), b"second".to_vec()]
    );
}

/// β: target-pane live output exceeding OFFTHREAD_LIVE_QUEUE_CAP_BYTES
/// during a pending replay abandons the off-thread switch and reparses the
/// snapshot synchronously, applying the accumulated output (nothing lost,
/// no unbounded backlog / swap-time burst).
#[test]
fn offthread_live_queue_cap_falls_back_to_sync() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
    assert!(tab.test_has_pending_switch());

    // 1 MiB of NUL padding per chunk: counts toward the byte budget but is
    // ignored by the parser (does not move the cursor / paint).
    let chunk = vec![0u8; 1024 * 1024];
    // Four chunks = 4 MiB == cap (not strictly greater) → still pending.
    for _ in 0..4 {
        tab.apply_mux_message(pty_output(10, chunk.clone()));
        assert!(
            tab.test_has_pending_switch(),
            "at-or-below the cap must stay off-thread"
        );
    }
    // The fifth chunk crosses the cap → synchronous fallback.
    let changed = tab.apply_mux_message(pty_output(10, chunk.clone()));
    assert!(changed, "the synchronous fallback repaints");
    assert!(
        !tab.test_has_pending_switch(),
        "exceeding the cap must abandon the off-thread switch"
    );
    // The snapshot was replayed synchronously (row 0 == marker) and the
    // NUL live output was applied on top without corrupting it.
    assert_eq!(tab.test_row_text(0), "SNAP");
}

/// TS-6 / FR5: a newer switch supersedes the in-flight one — only the
/// latest target's snapshot ends up being the one built/queued.
#[test]
fn ts6_newer_switch_supersedes_in_flight() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

    // First off-thread switch to pane 10.
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    tab.apply_mux_message(pty_output(10, b"stale".to_vec()));
    assert_eq!(tab.test_pending_target(), Some(10));
    assert_eq!(tab.test_pending_live_queue(), vec![b"stale".to_vec()]);

    // The daemon moved the active pane to 20 (a newer SwitchWindow), then
    // a second large snapshot arrives for it.
    tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
    tab.apply_mux_message(snapshot_msg(20, large_payload("SECOND")));

    // The pending switch now targets 20 and its queue was re-keyed (the
    // stale pane-10 bytes are discarded).
    assert_eq!(tab.test_pending_target(), Some(20));
    assert!(tab.test_pending_live_queue().is_empty());
    // The worker that actually completes built the *latest* target.
    assert_eq!(tab.test_wait_pending_first_row(), "SECOND");
}

/// FR5: a sub-threshold snapshot arriving mid-parse supersedes the
/// in-flight off-thread switch (it applies synchronously and clears it).
#[test]
fn ts6_sync_snapshot_supersedes_pending_switch() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("BIG")));
    assert!(tab.test_has_pending_switch());

    // A small snapshot for the now-active pane applies synchronously and
    // supersedes the in-flight parse.
    tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
    tab.apply_mux_message(snapshot_msg(20, b"small".to_vec()));
    assert!(!tab.test_has_pending_switch());
}

/// TS-12 / FR5 / FR7 (task0006 redesign, review round-1 finding
/// `64baa639d71792f9`, AC-9 regression guard): a grid resize during a
/// pending switch supersedes the DISPLAYED grid but no longer
/// re-dispatches the in-flight worker — it defers the resize
/// (`PendingSwitch::pending_resize`) so the worker keeps building at
/// its ORIGINAL dispatch-time target (where its bypass split, if any,
/// is valid; see `ac1_...` below for that half). The target and queued
/// live output are still preserved end to end, and the tab ends up at
/// the NEW grid once the swap completes — this test pins the adapted
/// mechanics while keeping TS-12's original intent (resize supersedes
/// correctly, nothing lost).
#[test]
fn ts12_resize_supersedes_and_redispatches_at_new_grid() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (orig_cols, orig_rows) = {
        let c = tab.core.lock();
        (c.cols(), c.rows())
    };
    tab.apply_mux_message(snapshot_msg(10, large_payload("PANE")));
    tab.apply_mux_message(pty_output(10, b"queued".to_vec()));
    assert!(tab.test_has_pending_switch());

    // Resize to a different grid → deferred, NOT a re-dispatch.
    tab.resize(100, 40);
    assert!(
        !tab.test_has_pending_redispatch(),
        "a resize alone must not coalesce a re-dispatch (FR7 fix: the \
         in-flight worker keeps its original, bypass-valid target)"
    );
    assert!(tab.test_has_pending_switch());
    assert_eq!(tab.test_pending_target(), Some(10));
    // Queue preserved across the deferred resize.
    assert_eq!(tab.test_pending_live_queue(), vec![b"queued".to_vec()]);
    assert_eq!(tab.test_pending_resize(), Some((100, 40)));
    // The in-flight worker's OWN build target is untouched.
    {
        let pending = tab.pending_switch.as_ref().unwrap();
        assert_eq!((pending.cols, pending.rows), (orig_cols, orig_rows));
    }

    // Once the (still ORIGINAL-target) worker completes, the deferred
    // resize is applied to the swapped-in core, then the queued live
    // output lands on top of it.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(!tab.test_has_pending_switch());
    let c = tab.core.lock();
    assert_eq!((c.cols(), c.rows()), (100, 40));
    drop(c);
    assert_eq!(tab.test_row_text(0), "PANE");
    assert_eq!(tab.test_row_text(1), "queued");
}

/// FR5: a resize that does not change the grid leaves the in-flight parse
/// untouched (the core is still correctly sized).
#[test]
fn ts12_noop_resize_keeps_in_flight_parse() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("PANE")));
    let (cols, rows) = {
        let c = tab.core.lock();
        (c.cols(), c.rows())
    };
    tab.resize(cols, rows);
    assert!(tab.test_has_pending_switch());
    assert_eq!(tab.test_pending_target(), Some(10));
}

/// AC-4 (D3''''', round-8 rework, review round-7 finding
/// `1d1b6b6297e3b6a0`): `Tab::resize` clamps to the SAME wire domain the
/// daemon applies (`MuxPane::new` / `MuxPane::resize`'s
/// `clamp_dims_to_wire_domain`) BEFORE resizing its own core, so the
/// dimensions the client renders at are always the dimensions the
/// daemon would accept for a pane — never the caller's raw,
/// out-of-wire-domain request. Both ends run the SAME pure function
/// against the SAME input, so they agree without a wire round trip.
///
/// Confirmed to fail pre-fix: before this change, `Tab::resize` resized
/// `self.core` directly to the caller's raw `(cols, rows)` with no
/// clamp at all — `core.cols()`/`core.rows()` would come out as
/// `(u16::MAX, u16::MAX)` instead of the clamped wire-domain values
/// asserted below.
#[test]
fn resize_clamps_to_the_wire_domain_before_resizing_the_core() {
    let mut tab = test_tab();
    tab.resize(u16::MAX, u16::MAX);
    let (expected_cols, expected_rows) =
        crate::mux::session::pane::clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
    let core = tab.core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (expected_cols, expected_rows),
        "the client's core must be resized to the CLAMPED wire-domain \
         dims, matching what MuxPane::new/resize would accept — not \
         the caller's raw, out-of-domain request"
    );
}

/// AC-5, D3'''''' (round-9 rework, review round-8 finding
/// `1e7e069001cf22dc`): `Tab::spawn_shell` clamps its FIRST-ever
/// dimensions the same way `Tab::resize` clamps every later one, so a
/// tab's initial core is never out of the wire domain even before any
/// resize has happened.
///
/// Confirmed to fail pre-fix: before this change, `Tab::spawn_shell`
/// passed the caller's raw `cols`/`rows` straight into `TerminalCore::new`
/// with no clamp at all — with this test's `u16::MAX` input, that means
/// a `u16::MAX × u16::MAX`-cell grid allocation, which aborts the test
/// process outright (a real allocation failure, not just a mismatched
/// assertion) rather than settling on the clamped wire-domain values
/// asserted below.
#[test]
fn spawn_shell_clamps_the_initial_core_to_the_wire_domain() {
    let tab = Tab::spawn_shell(
        "test",
        u16::MAX,
        u16::MAX,
        100,
        Arc::new(Settings::default()),
        None,
        None,
        Arc::new(NoopSink),
        None,
    );
    let (expected_cols, expected_rows) =
        crate::mux::session::pane::clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
    let core = tab.core.lock();
    assert_eq!(
        (core.cols(), core.rows()),
        (expected_cols, expected_rows),
        "the tab's INITIAL core must already be clamped to the wire \
         domain, matching what a later Tab::resize (or MuxPane::new) \
         would accept — not the caller's raw, out-of-domain request"
    );
}

// ── FR6 / IMPLEMENTATION.md contract (c), D3 (task0002):
// Tab::resize self-invalidation ──────────────────────────

/// AC-1 (TS5, FR6): a resize that changes the tab's column count clears
/// its own reflow-invalidated trackers — the prompt-mark tracker and
/// fold regions, both tab-owned and reachable directly here (mirroring
/// the seeding pattern app.rs's tests use for the same trackers).
/// `clear_reflow_invalidated_state`'s own doc comment explains why: a
/// reflow rewraps the logical↔physical line mapping, so retained
/// absolute-row marks would point at the wrong line after the resize.
#[test]
fn resize_that_changes_columns_clears_the_tabs_own_reflow_trackers() {
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
        ],
    );
    assert_eq!(
        tab.prompts.find_prev_prompt(u32::MAX),
        Some(2),
        "prompt mark seeded before the resize"
    );
    assert!(
        tab.folds.get_region_at_line(4).is_some(),
        "fold region seeded before the resize"
    );

    tab.resize(100, 24); // cols 80 -> 100: a width change

    assert_eq!(
        tab.prompts.find_prev_prompt(u32::MAX),
        None,
        "a width-changing resize must clear the tab's own prompt marks"
    );
    assert!(
        tab.folds.get_region_at_line(4).is_none(),
        "a width-changing resize must clear the tab's own fold regions"
    );
}

/// AC-2 (TS5, FR6): a height-only resize (column count unchanged) keeps
/// the tab's prompt marks and fold regions — only a WIDTH change
/// invalidates the tab-owned reflow trackers.
#[test]
fn resize_that_only_changes_rows_keeps_the_tabs_own_reflow_trackers() {
    let mut tab = test_tab();
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
        ],
    );

    tab.resize(80, 30); // same cols (80), rows 24 -> 30: height-only

    assert_eq!(
        tab.prompts.find_prev_prompt(u32::MAX),
        Some(2),
        "a height-only resize must NOT clear the tab's prompt marks"
    );
    assert!(
        tab.folds.get_region_at_line(4).is_some(),
        "a height-only resize must NOT clear the tab's fold regions"
    );
}

/// AC-3 (TS5, FR5, FR6): a raw resize request whose column count clamps
/// back to the tab's CURRENT (post-clamp) column count is not a width
/// change — the tab-owned trackers are left untouched. Uses the
/// per-axis clamp floor (`clamp_resize_dims` clamps 0 up to 1) to build
/// a case where the raw request (`0`) differs from the tab's current
/// column count in absolute terms but clamps to the SAME value.
#[test]
fn resize_whose_raw_cols_clamp_back_to_the_current_cols_clears_nothing() {
    let mut tab = test_tab();
    tab.resize(1, 24); // drive the tab's own column count down to 1
    tab.backfill_prompt_marks(
        0,
        vec![
            pending_kind(b'A', 2, None),
            pending_kind(b'C', 4, None),
            pending_kind(b'D', 9, Some(0)),
        ],
    );

    tab.resize(0, 24); // raw cols 0 clamps to 1 == current cols
    {
        let core = tab.core.lock();
        assert_eq!(core.cols(), 1, "clamp floor sanity check");
    }

    assert_eq!(
        tab.prompts.find_prev_prompt(u32::MAX),
        Some(2),
        "a raw request that clamps back to the current column count \
         must NOT clear the tab's prompt marks"
    );
    assert!(
        tab.folds.get_region_at_line(4).is_some(),
        "a raw request that clamps back to the current column count \
         must NOT clear the tab's fold regions"
    );
}

/// Regression: a DA1/DSR/XTWINOPS query embedded in snapshot bytes (e.g.
/// because some past program in the pane's scrollback wrote `\x1b[c` to
/// `/dev/tty`) generates a reply inside `reset_and_replay`. That reply
/// must NOT be left in `term_core`'s `response_buffer` — otherwise the
/// next live `apply_active_pane_output` (triggered by the user's first
/// keystroke echo after the switch) picks it up via `take_response` and
/// delivers a stale `\x1b[?65;1;4;22c` to the shell's stdin as PtyInput,
/// where zsh/zle eats the `\x1b[?` prefix as an unbound key-binding and
/// inserts the remaining `65;1;4;22c` at the prompt.
#[test]
fn reset_frame_for_replay_discards_historic_device_responses() {
    let mut tab = test_tab();
    // Snapshot-shaped payload with embedded DA1 and CPR queries.
    let mut snapshot = Vec::new();
    snapshot.extend_from_slice(b"row one\r\n");
    snapshot.extend_from_slice(b"\x1b[c"); // DA1 query
    snapshot.extend_from_slice(b"row two\r\n");
    snapshot.extend_from_slice(b"\x1b[6n"); // CPR query

    let _ = tab.reset_frame_for_replay(&snapshot, &[]);

    let core = tab.core.lock();
    assert_eq!(
        core.get_response_len(),
        0,
        "reset_frame_for_replay must drop device responses generated by \
         historic queries baked into the snapshot; residual bytes would \
         leak as PtyInput on the next live take_response and corrupt the \
         user's prompt on the first keystroke after a window switch"
    );
}

// ── 2nd-pass scrollback restore (snapshot-replay-scrollback-restore) ──

/// Build a payload at or above the off-thread threshold that scrolls
/// many rows so the rebuilt scrollback has content to compare against.
/// 100 "line N" rows pad up over 64 KiB easily.
fn large_scrollable_payload() -> Vec<u8> {
    let mut p = Vec::with_capacity(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 1024);
    // Tag the first row with a recognizable marker.
    p.extend_from_slice(b"FIRST\r\n");
    // Filler lines until we comfortably exceed the off-thread threshold.
    let mut i: u32 = 0;
    while p.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8 * 1024 {
        p.extend_from_slice(format!("line {i:06}\r\n").as_bytes());
        i += 1;
    }
    // Last row marker so we can spot the visible tail in tests.
    p.extend_from_slice(b"LAST\r\n");
    p
}

/// TS-13 (FR1 / FR6): an at-or-above-threshold payload installs a
/// `pending_scrollback_restore` after the 1st-pass swap finishes. Also
/// covers the FR4 wiring side: `poll_pending_scrollback_restore` is the
/// thing that consumes the pending state.
#[test]
fn ts13_offthread_swap_installs_pending_scrollback_restore() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let payload = large_scrollable_payload();
    tab.apply_mux_message(snapshot_msg(10, payload));
    assert!(
        tab.test_has_pending_switch(),
        "test prerequisite: large payload must go off-thread"
    );
    // Drive the 1st-pass swap to completion.
    let outcome = tab.test_poll_until_swapped();
    assert_eq!(outcome, SwapOutcome::Swapped);
    // After the swap, the 2nd-pass scrollback restore must be installed.
    assert!(
        tab.test_has_pending_scrollback_restore(),
        "apply_offthread_swap must spawn a 2nd-pass scrollback restore worker"
    );
}

/// TS-12 (FR6): a sub-threshold payload takes the synchronous path and
/// installs no `pending_scrollback_restore` (the live core's scrollback
/// is already correct).
#[test]
fn ts12_subthreshold_payload_does_not_install_scrollback_restore() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let mut small = b"hello\r\n".to_vec();
    small.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES - 1, b'.');
    tab.apply_mux_message(snapshot_msg(10, small));
    assert!(
        !tab.test_has_pending_switch(),
        "sub-threshold snapshot must take the synchronous path"
    );
    assert!(
        !tab.test_has_pending_scrollback_restore(),
        "sub-threshold snapshot must NOT install a 2nd-pass scrollback restore"
    );
}

/// TS-7 (FR1 + NFR6): after the 1st-pass swap, the live core has empty
/// scrollback (bypass left it empty). After the 2nd-pass restore
/// completes, the merged scrollback matches the synchronous reference
/// (built bypass-off).
#[test]
fn ts7_offthread_swap_then_restored_scrollback_matches_reference() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let payload = large_scrollable_payload();
    // Reference: synchronous bypass-off build at the same grid.
    let never = std::sync::atomic::AtomicBool::new(false);
    let reference = term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
        80,
        24,
        100,
        &payload,
        &[],
        &never,
    )
    .expect("reference build not cancelled");
    let reference_scrollback_count = reference.core.get_scrollback_length();

    tab.apply_mux_message(snapshot_msg(10, payload));
    // 1st-pass swap.
    let _ = tab.test_poll_until_swapped();
    // Right after the swap, the live core's scrollback is empty (the
    // bypass intentionally left it so).
    assert_eq!(
        tab.test_scrollback_length(),
        0,
        "bypass-on 1st-pass leaves scrollback empty"
    );
    // Drive the 2nd-pass to completion (blocking-recv re-stage).
    assert!(tab.test_has_pending_scrollback_restore());
    tab.test_drain_pending_scrollback_restore_for_blocking_recv();
    let outcome = tab.poll_pending_scrollback_restore();
    assert_eq!(outcome, ScrollbackRestoreOutcome::Merged);
    // Now the live core's scrollback length matches the reference.
    assert_eq!(
        tab.test_scrollback_length(),
        reference_scrollback_count,
        "merged scrollback length must match the synchronous reference"
    );
    // Polling again is Idle (state was cleared by Merged).
    assert_eq!(
        tab.poll_pending_scrollback_restore(),
        ScrollbackRestoreOutcome::Idle
    );
}

/// TS-8 (FR5 / NFR4): a newer off-thread switch supersedes any
/// in-flight 2nd-pass restore — the prior restore's state is dropped
/// and the cancel flag is set so the worker bails.
#[test]
fn ts8_new_offthread_switch_supersedes_in_flight_restore() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // First off-thread switch: drive to swap so the restore installs.
    tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
    let _ = tab.test_poll_until_swapped();
    assert!(tab.test_has_pending_scrollback_restore());
    // New off-thread switch to a different pane. Since task0003 D3 the
    // Snapshot arm drops frames for non-active panes, so make pane 20
    // active first — matching the real switch flow (SwitchWindow, then
    // the reconciling per-pane snapshot).
    tab.apply_mux_message(switch_window(20));
    tab.apply_mux_message(snapshot_msg(20, large_scrollable_payload()));
    // The prior restore is cleared immediately on the supersede arm
    // inside `dispatch_offthread_replay`.
    assert!(
        !tab.test_has_pending_scrollback_restore(),
        "supersede must clear the prior pending_scrollback_restore"
    );
}

/// TS-10 (FR5 / UC03): a resize during a pending 2nd-pass restore
/// cancels it; no respawn (history-restore is abandoned).
#[test]
fn ts10_resize_cancels_pending_restore_without_respawn() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
    let _ = tab.test_poll_until_swapped();
    assert!(tab.test_has_pending_scrollback_restore());
    // Different grid → resize cancels.
    tab.resize(100, 30);
    assert!(
        !tab.test_has_pending_scrollback_restore(),
        "resize must cancel the pending 2nd-pass scrollback restore"
    );
    // No respawn.
    assert!(
        !tab.test_has_pending_scrollback_restore(),
        "resize must NOT respawn the 2nd-pass restore at the new grid (UC03)"
    );
}

/// TS-11 (FR7): worker panic → `poll_pending_scrollback_restore`
/// observes `Disconnected`, returns `Failed`, clears state, app
/// continues. Force-disconnect simulates the panic path.
#[test]
fn ts11_restore_worker_panic_returns_failed_and_clears_state() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
    let _ = tab.test_poll_until_swapped();
    assert!(tab.test_has_pending_scrollback_restore());
    // Force the sender to drop without ever sending a build — the next
    // try_recv will observe Disconnected.
    tab.test_force_scrollback_restore_disconnect();
    let outcome = tab.poll_pending_scrollback_restore();
    assert_eq!(outcome, ScrollbackRestoreOutcome::Failed);
    assert!(
        !tab.test_has_pending_scrollback_restore(),
        "Failed arm must clear pending state"
    );
    // Polling again is Idle.
    assert_eq!(
        tab.poll_pending_scrollback_restore(),
        ScrollbackRestoreOutcome::Idle
    );
}

/// TS-9 (FR3 + NFR5): between the 1st-pass swap and the 2nd-pass
/// arrival, feeding live PTY output advances
/// `scrollback_evicted_total` on the live core; `apply_scrollback_restore`
/// trims that many trailing rebuilt rows so the merged scrollback has
/// no duplicates.
///
/// Approach: rather than feeding async PTY bytes, drive the bookkeeping
/// directly: after the swap, feed a known set of `\r\n`s via the live
/// core to bump `scrollback_evicted_total` by N, then complete the
/// 2nd-pass via the blocking-recv re-stage and assert the final
/// scrollback length is the reference length minus N (the trim
/// arithmetic).
#[test]
fn ts9_concurrent_live_drain_trims_rebuilt_tail_no_duplicates() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let payload = large_scrollable_payload();
    // Reference scrollback length.
    let never = std::sync::atomic::AtomicBool::new(false);
    let reference = term_core::terminal_core::TerminalCore::build_scrollback_only_from_snapshot(
        80,
        24,
        100,
        &payload,
        &[],
        &never,
    )
    .expect("reference");
    let reference_count = reference.core.get_scrollback_length() as usize;

    tab.apply_mux_message(snapshot_msg(10, payload));
    let _ = tab.test_poll_until_swapped();
    assert_eq!(tab.test_scrollback_length(), 0);
    // Drive live drain on the swapped-in core: push N lines that each
    // generate one scrollback row.
    let n_live: u32 = 12;
    let mut bytes = Vec::new();
    for _ in 0..n_live {
        bytes.extend_from_slice(b"live\r\n");
    }
    {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(&bytes);
    }
    let live_scrollback_before_merge = tab.test_scrollback_length();
    // Each "live\r\n" past the 24-row viewport pushes one row in.
    // Confirm we genuinely grew the scrollback before the merge.
    assert!(
        live_scrollback_before_merge > 0,
        "live drain must have pushed rows into scrollback before the merge"
    );
    // Drive the 2nd-pass to completion.
    assert!(tab.test_has_pending_scrollback_restore());
    tab.test_drain_pending_scrollback_restore_for_blocking_recv();
    assert_eq!(
        tab.poll_pending_scrollback_restore(),
        ScrollbackRestoreOutcome::Merged
    );
    // After the merge: scrollback total = (reference_count) for a payload
    // that does not saturate the ring. The FR3 trim removes the
    // last `live_growth` rows from the rebuilt half, but `live_growth`
    // is 0 here because the live drain did not push the eviction
    // counter past `base_evicted_total` (the rebuilt scrollback's
    // capacity is 100 and `live_scrollback_before_merge < 100`). So
    // the merged length is the rebuilt length plus the live half.
    let final_scrollback = tab.test_scrollback_length() as usize;
    let live_count = live_scrollback_before_merge as usize;
    // Upper bound: reference_count + live_count (no duplication beyond
    // the FR3 trim). Lower bound: reference_count (live is appended;
    // the rebuilt prepend lands the historical half in front).
    assert!(
        final_scrollback <= reference_count + live_count,
        "no row duplication: final {final_scrollback} <= reference {reference_count} + live {live_count}",
    );
    assert!(
        final_scrollback >= reference_count.min(100),
        "the historical half must be merged in"
    );
}

/// TS-14 (要件定義書 §4.2 F02 edge case): when `live_growth >=
/// rebuilt_count`, the merge is a full no-op (zero rows prepended) and
/// the call returns cleanly. Drive directly via the merge primitive
/// since plumbing a >100-row live drain through the test harness is
/// brittle.
#[test]
fn ts14_live_growth_exceeds_rebuilt_count_full_noop() {
    // 6-row grid → easier to push rows past the viewport into scrollback
    // without needing 30+ lines of seed bytes.
    let mut live = term_core::terminal_core::TerminalCore::new(80, 6, 100);
    // Push 10 lines: 6 stay in viewport, the rest land in scrollback.
    live.process_pty_data_fully(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\n");
    let live_count_before = live.get_scrollback_length();
    assert!(
        live_count_before > 0,
        "test prerequisite: live has scrollback"
    );

    let mut rebuilt = term_core::terminal_core::TerminalCore::new(80, 6, 100);
    rebuilt.process_pty_data_fully(b"x\r\ny\r\nz\r\n1\r\n2\r\n3\r\n4\r\n5\r\n");
    let rebuilt_count = rebuilt.get_scrollback_length() as usize;
    assert!(
        rebuilt_count > 0,
        "test prerequisite: rebuilt has scrollback"
    );

    // live_growth = rebuilt_count (= "everything was already drained
    // live"): merge must be a full no-op.
    let merged = live.merge_scrollback_from(rebuilt, rebuilt_count);
    assert_eq!(
        merged, 0,
        "merge must be a noop when live_growth >= rebuilt_count"
    );
    assert_eq!(
        live.get_scrollback_length(),
        live_count_before,
        "live scrollback must be unchanged on a noop merge"
    );
}

/// TS-15 (FR8): `merge_scrollback_from` only touches scrollback;
/// `prompt_marks` / `fold_marks` from the 2nd-pass replay are dropped
/// by `apply_scrollback_restore` and never reach the live tab's mark
/// trackers. Concretely: the live core's prompt_marks count is
/// unchanged by the merge.
#[test]
fn ts15_merge_does_not_duplicate_prompt_marks_or_fold_marks() {
    let mut live = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    // Seed the live core with one prompt mark via OSC 133 A and leave
    // it in `pending_prompt_marks` (do NOT take, so we have a non-zero
    // baseline to compare against after the merge).
    live.process_pty_data_fully(b"\x1b]133;A\x07$ \r\n");

    // The 2nd-pass rebuilt core has its OWN prompt marks accumulated
    // during the parse — those marks would be `take_prompt_marks`'d by
    // the worker before sending if anyone consumed them, but
    // `apply_scrollback_restore` discards the marks instead (FR8).
    // To assert the merge primitive itself does not leak them, leave
    // them on the rebuilt core's pending queue and merge.
    let mut rebuilt = term_core::terminal_core::TerminalCore::new(80, 24, 100);
    rebuilt.process_pty_data_fully(b"\x1b]133;A\x07rebuilt\r\n");
    // Confirm the rebuilt core has at least one pending prompt mark
    // before the merge — touch via a getter that does NOT drain.
    // `pending_prompt_marks` is `pub(crate)` and crate-private; we
    // confirm via `get_scrollback_length` instead that the parse landed.
    assert!(
        rebuilt.get_scrollback_length() == 0,
        "test wiring: rebuilt prepared with a single A mark, no scroll"
    );

    // Merge.
    let merged = live.merge_scrollback_from(rebuilt, 0);
    // The rebuilt scrollback was 0 rows, so the merge inserted 0 rows.
    // What matters here is the FR8 invariant: live's marks queue is
    // still exactly the one we seeded.
    assert_eq!(merged, 0);
    let live_marks_after = live.take_prompt_marks();
    assert_eq!(
        live_marks_after.len(),
        1,
        "live's pending prompt marks must be exactly the one originally seeded; \
         the merge primitive must not append the rebuilt core's marks (FR8)"
    );
    assert_eq!(live_marks_after[0].kind, b'A');
}

// ── task0001: transplant callbacks + OSC registration across the
// off-thread core swap ────────────────────────────────────────────────

/// Minimal recording [`term_core::callbacks::TerminalCallbacks`] double
/// (mirrors `term_core`'s internal `Recorder` test pattern) used to
/// prove AC-1: the exact pre-swap callbacks instance is still the one
/// firing after `apply_offthread_swap`, not merely *a* fresh instance.
#[derive(Default)]
struct OscRecorder {
    events: Mutex<Vec<(u8, String)>>,
}

struct RecorderCallbacks(Arc<OscRecorder>);

impl term_core::callbacks::TerminalCallbacks for RecorderCallbacks {
    fn on_osc(&self, action_type: u8, data: &str) {
        self.0.events.lock().push((action_type, data.to_string()));
    }
    fn on_apc(&self, _data: &[u8]) {}
    fn on_dcs(&self, _data: &[u8]) {}
    fn on_bell(&self) {}
}

/// AC-1 (SPEC TS-1): after `apply_offthread_swap`, the live core's
/// callbacks is the pre-swap instance — a recording callbacks double
/// installed before the swap still receives events fed to the
/// swapped-in core afterward.
#[test]
fn ac1_offthread_swap_transplants_the_preswap_callbacks_instance() {
    let mut tab = test_tab();
    let recorder = Arc::new(OscRecorder::default());
    tab.core.lock().callbacks = Some(Box::new(RecorderCallbacks(recorder.clone())));

    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert!(
        tab.test_has_pending_switch(),
        "test prerequisite: large payload must go off-thread"
    );
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    // Feed an OSC directly through the now-swapped core; the SAME
    // recorder installed before the swap must observe it.
    tab.core
        .lock()
        .process_pty_data_fully(b"\x1b]2;hello\x1b\\");
    assert_eq!(
        recorder.events.lock().as_slice(),
        &[(2u8, "hello".to_string())],
        "the pre-swap callbacks instance must still be the one firing after the swap"
    );
}

/// AC-2 (SPEC TS-2): after `apply_offthread_swap`, feeding an OSC 9999
/// (`MUX_OSC_PARAM`) sequence to the live core triggers the same
/// registered app-param action as on a never-swapped tab core. Without
/// the registration surviving the swap, OSC 9999 maps to action_type 255
/// (Unknown) and never reaches `pending_apc`.
#[test]
fn ac2_offthread_swap_preserves_osc_9999_app_param_registration() {
    let mut tab = test_tab();
    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    let welcome = welcome_msg(&[(1, "a", 10)], 0);
    let osc_bytes = welcome.to_osc();
    tab.core.lock().process_pty_data_fully(osc_bytes.as_bytes());
    assert_eq!(
        tab.cb_state.lock().pending_apc.len(),
        1,
        "OSC 9999 must still map to OSC_MUX_INBAND and reach pending_apc after the swap"
    );
}

/// AC-3 (SPEC TS-3): after an off-thread swap, a pre-mux Welcome frame in
/// OSC 9999 form arriving on the outer-stream path (`process_outer_via_core`,
/// taken while `mux_session_name` is `None`) reaches `apply_mux_message`.
#[test]
fn ac3_offthread_swap_preserves_premux_welcome_osc_form_reaching_apply_mux_message() {
    let mut tab = test_tab();
    // No prior Welcome: the tab starts pre-mux, mirroring the Windows
    // ConPTY fallback scenario where the OSC 9999 Welcome has not yet
    // arrived when a large snapshot triggers the off-thread swap.
    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(
        tab.mux_session_name.is_none(),
        "test prerequisite: tab is still pre-mux after the swap"
    );

    let welcome = welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0);
    let osc_bytes = welcome.to_osc().into_bytes();
    // Drive the pre-mux outer-stream path (`process_combined` routes
    // through `self.core` when `mux_session_name` is `None`).
    tab.test_process_combined(osc_bytes);
    assert_eq!(
        tab.mux_session_name.as_deref(),
        Some("main"),
        "the OSC 9999 Welcome frame must still reach apply_mux_message after the swap"
    );
}

/// AC-4 (SPEC TS-4): after an off-thread swap, a pre-mux Welcome frame in
/// APC form is also processed to `apply_mux_message`. Unlike AC-3 (OSC
/// 9999), the APC path needs only the transplanted callbacks (`on_apc`
/// fires unconditionally for any APC, no app-param registration
/// involved) — this pins that path separately.
#[test]
fn ac4_offthread_swap_preserves_premux_welcome_apc_form_reaching_apply_mux_message() {
    let mut tab = test_tab();
    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(
        tab.mux_session_name.is_none(),
        "test prerequisite: tab is still pre-mux after the swap"
    );

    let welcome = welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0);
    let apc_bytes = welcome.to_apc().into_bytes();
    tab.test_process_combined(apc_bytes);
    assert_eq!(
        tab.mux_session_name.as_deref(),
        Some("main"),
        "the APC-form Welcome frame must still reach apply_mux_message after the swap"
    );
}

/// AC-5 (SPEC TS-5): after an off-thread swap, a callback-driven OSC
/// (title change) in subsequent PTY output invokes the transplanted
/// callbacks end to end (`NativeCallbacks::on_osc` -> `cb_state.title`
/// -> `Tab::title`), not merely proving a callback object is present.
#[test]
fn ac5_offthread_swap_transplanted_callbacks_apply_title_change() {
    let mut tab = test_tab();
    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    tab.test_process_combined(b"\x1b]2;post-swap-title\x1b\\".to_vec());
    assert_eq!(tab.title, "post-swap-title");
}

/// AC-6 (SPEC TS-7 / risk mitigation): the 2nd-pass scrollback restore
/// path (`spawn_scrollback_restore` -> `apply_scrollback_restore`) merges
/// into the live core via `merge_scrollback_from` and never replaces it
/// — the transplanted callbacks and OSC registration from the 1st-pass
/// swap survive the 2nd-pass merge too.
#[test]
fn ac6_scrollback_restore_merge_does_not_clear_callbacks_or_osc_registration() {
    let mut tab = test_tab();
    // Pre-mux (no Welcome), same as the AC tests above, so
    // `test_process_combined` routes through `process_outer_via_core`
    // below (mux established would route through the independent mux
    // extractor instead, which is not what this test exercises).
    tab.apply_mux_message(snapshot_msg(10, large_scrollable_payload()));
    assert!(tab.test_has_pending_switch());
    // Blocking-recv re-stage (not the spin-based `test_poll_until_swapped`)
    // so this 1st-pass swap is robust to worker-thread scheduling delays
    // under system load — this test drives both an off-thread swap AND a
    // 2nd-pass restore in sequence, so it is more sensitive to that than
    // the single-swap AC tests above.
    tab.test_block_worker_ready();
    assert_eq!(tab.poll_pending_switch(), SwapOutcome::Swapped);
    assert!(tab.test_has_pending_scrollback_restore());

    // Drive the 2nd-pass restore to completion (the merge under test).
    tab.test_drain_pending_scrollback_restore_for_blocking_recv();
    assert_eq!(
        tab.poll_pending_scrollback_restore(),
        ScrollbackRestoreOutcome::Merged
    );

    // Callbacks must still be installed and wired end to end (title
    // sync)...
    tab.test_process_combined(b"\x1b]2;after-restore\x1b\\".to_vec());
    assert_eq!(tab.title, "after-restore");

    // ...and the OSC 9999 app-param registration must still be in
    // effect (pending_apc sink reached).
    let welcome = welcome_msg(&[(3, "c", 30)], 0);
    tab.core
        .lock()
        .process_pty_data_fully(welcome.to_osc().as_bytes());
    assert_eq!(tab.cb_state.lock().pending_apc.len(), 1);
}

/// AC-7 (SPEC edge case): an old core whose callbacks slot is empty
/// swaps without panic and yields a live core with no callbacks.
#[test]
fn ac7_offthread_swap_with_no_preswap_callbacks_yields_none_without_panic() {
    let mut tab = test_tab();
    // Simulate a live core with an empty callbacks slot.
    tab.core.lock().callbacks = None;

    tab.apply_mux_message(snapshot_msg(10, large_payload("SWAP")));
    assert!(tab.test_has_pending_switch());
    // Must not panic.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    assert!(
        tab.core.lock().callbacks.is_none(),
        "an old core with no callbacks must swap to a live core with no callbacks"
    );
}

// ── task0003 FR7/FR8: resize-race bypass resilience + duplicate
// snapshot fetch dedup ──────────────────────────────────────────────

/// A structurally-segmented (`EMSNAP2` framed) snapshot payload with
/// `OFFTHREAD_REPLAY_SEGMENT_THRESHOLD` segments, all recorded at
/// `(cols, rows)` — forcing the off-thread dispatch path via segment
/// COUNT rather than byte size (mirrors the existing
/// `ac5_small_payload_many_segment_snapshot_dispatches_off_thread`
/// fixture), so the underlying content stays tiny and a worker's
/// build always completes fast regardless of a test's polling budget.
/// Every segment already matching `(cols, rows)` makes
/// `stable_target_suffix_start` return `k == 0` for that SAME target —
/// the trivial "every segment already matches" bypass-engage case —
/// and, symmetrically, `k == segments.len()` (no bypass) for any OTHER
/// target, which is exactly the shape task0003 FR7/FR8 need: a
/// dispatch-consistent-target-and-segments regression guard (AC-2), and
/// a target-mismatch-after-the-fact case (AC-1) that must not pay for
/// more than one wasted rebuild.
fn many_segment_payload_at(cols: u16, rows: u16) -> (Vec<mux_ipc::protocol::DimSegment>, Vec<u8>) {
    let content = b"content\r\n".to_vec();
    let segments: Vec<mux_ipc::protocol::DimSegment> = (0..OFFTHREAD_REPLAY_SEGMENT_THRESHOLD)
        .map(|_| mux_ipc::protocol::DimSegment {
            offset: 0,
            cols,
            rows,
        })
        .collect();
    (segments, content)
}

/// AC-2: an ordinary (unraced) switch — segments' tail already matches
/// the dispatch-time target — engages bypass, observed indirectly via
/// the 2nd-pass scrollback-restore worker being spawned
/// (`scrollback_populated: false` on the 1st-pass replay is exactly
/// when `apply_offthread_swap` spawns it; see D3' in that method's
/// doc). Regression guard: this must stay true after the FR7 fix
/// below, not just before it. Also satisfies task0006's AC-2 (FR7
/// regression guard: the unraced case is unaffected by that task's
/// `PendingSwitch::pending_resize` fix, since no resize means
/// `pending_resize` is never touched) — unchanged, no separate test
/// needed.
#[test]
fn ac2_unraced_switch_engages_bypass() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (cols, rows) = {
        let c = tab.core.lock();
        (c.cols(), c.rows())
    };
    let (segments, content) = many_segment_payload_at(cols, rows);
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(tab.test_has_pending_switch(), "must go off-thread");
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(
        tab.test_has_pending_scrollback_restore(),
        "an unraced switch whose segments already match the target must \
         engage bypass (observed via the 2nd-pass scrollback-restore \
         worker being spawned)"
    );
}

/// AC-1 / AC-7 (FR7, task0006 redesign, review round-1 findings
/// `64baa639d71792f9` / `34a708465d04f983`, AC-9 regression guard): a
/// resize STORM landing during an in-flight switch — several resize
/// events, each superseding the last, before the worker is ever polled
/// — must not pay for one wasted off-thread build per intermediate,
/// already-superseded target. Adapted from task0003's original test of
/// the same name/intent: the round-1 fix collapsed the storm into ONE
/// re-dispatch (`test_offthread_spawn_count` going from 1 to 2 once);
/// task0006's redesign collapses it further into ZERO re-dispatches —
/// the in-flight worker's own build is never touched by a resize at
/// all (`PendingSwitch::pending_resize` just tracks the latest target),
/// so the count now stays at 1 for the whole storm AND after the swap
/// (AC-7: no payload/segments clone per resize event either, since
/// there is no re-dispatch to clone for).
#[test]
fn ac1_resize_storm_during_pending_switch_collapses_to_one_redispatch() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (cols, rows) = {
        let c = tab.core.lock();
        (c.cols(), c.rows())
    };
    let (segments, content) = many_segment_payload_at(cols, rows);
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(tab.test_has_pending_switch(), "must go off-thread");
    let spawns_after_dispatch = tab.test_offthread_spawn_count();
    assert_eq!(spawns_after_dispatch, 1);

    // A resize storm: several resize events land before the in-flight
    // worker is ever polled.
    tab.resize(90, 30);
    tab.resize(95, 35);
    tab.resize(100, 40);
    assert_eq!(
        tab.test_offthread_spawn_count(),
        spawns_after_dispatch,
        "a resize storm must not spawn an extra worker per resize event \
         — the in-flight worker's own build is untouched by any of them"
    );
    assert!(
        !tab.test_has_pending_redispatch(),
        "a resize-only storm must never coalesce a re-dispatch (FR7 \
         fix: it would defeat the in-flight worker's bypass split)"
    );
    assert_eq!(
        tab.test_pending_resize(),
        Some((100, 40)),
        "the storm's final target collapses into one deferred resize"
    );

    // The SAME worker (never re-dispatched) resolves the switch; the
    // deferred resize is applied to the swapped-in core.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(
        tab.test_offthread_spawn_count(),
        spawns_after_dispatch,
        "no second worker was ever needed to resolve the storm"
    );
    let c = tab.core.lock();
    assert_eq!((c.cols(), c.rows()), (100, 40));
}

/// AC-3 (FR8): two `Snapshot` frames for the SAME pane arriving in
/// immediate succession (before the first's replay would complete) —
/// the second must coalesce into the first's in-flight request rather
/// than spawning a second worker right away. Confirmed to fail
/// pre-fix: before the same-pane coalesce existed,
/// `test_offthread_spawn_count` would have read 2 immediately after
/// the second frame, not 1.
#[test]
fn ac3_duplicate_same_pane_snapshot_coalesces_before_spawning_again() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (segments1, content1) = many_segment_payload_at(80, 24);
    let encoded1 = mux_ipc::protocol::encode_snapshot_payload(&segments1, &content1);
    tab.apply_mux_message(snapshot_msg(10, encoded1));
    assert!(tab.test_has_pending_switch(), "must go off-thread");
    assert_eq!(tab.test_offthread_spawn_count(), 1);

    // A second Snapshot for the SAME pane arrives before the first's
    // replay would complete (segment count differing by one, mirroring
    // the observed segs=9 then segs=10 trace) and with different
    // (identifiable) content.
    let mut segments2 = segments1;
    segments2.push(mux_ipc::protocol::DimSegment {
        offset: 0,
        cols: 80,
        rows: 24,
    });
    let content2 = b"SECOND\r\n".to_vec();
    let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
    tab.apply_mux_message(snapshot_msg(10, encoded2));

    assert_eq!(
        tab.test_offthread_spawn_count(),
        1,
        "a duplicate snapshot fetch for the pane already being switched \
         to must coalesce, not spawn a second worker immediately — the \
         discarded (first) request's work must not run alongside it"
    );
    assert!(tab.test_has_pending_redispatch());

    // Only the second's outcome ever completes and gets displayed.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(tab.test_offthread_spawn_count(), 2);
    assert!(
        tab.test_grid_text().contains("SECOND"),
        "the second fetch's content must be the one that ends up displayed"
    );
}

/// AC-4: two switches to DIFFERENT panes arriving in immediate
/// succession are NOT deduplicated against each other — the dedup in
/// AC-3 is scoped to same-pane frames only, so a switch to a different
/// pane must still spawn its own worker right away (an ordinary
/// pane-to-pane switch must not regress into the coalesce path).
#[test]
fn ac4_switch_to_different_pane_is_not_coalesced() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (segments, content) = many_segment_payload_at(80, 24);
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(tab.test_has_pending_switch());
    assert_eq!(tab.test_offthread_spawn_count(), 1);

    // The daemon moved the active pane to 20, then a second large
    // snapshot arrives for it — a genuinely different pane, not a
    // duplicate of the first.
    tab.mux_group.as_mut().unwrap().set_active_by_pane(20);
    let (segments2, content2) = many_segment_payload_at(80, 24);
    let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
    tab.apply_mux_message(snapshot_msg(20, encoded2));

    assert_eq!(
        tab.test_offthread_spawn_count(),
        2,
        "a switch to a DIFFERENT pane must spawn its own worker \
         immediately, not coalesce against the prior pane's request"
    );
    assert!(
        !tab.test_has_pending_redispatch(),
        "a different-pane switch is not a coalesce candidate"
    );
    assert_eq!(tab.test_pending_target(), Some(20));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
}

/// AC-5: a switch to the same pane arriving WELL AFTER the previous one
/// has already completed and been displayed (not a near-simultaneous
/// race) is NOT dropped — the AC-3 dedup must not degrade into "ignore
/// all repeat switches to a pane."
#[test]
fn ac5_late_repeat_switch_to_same_pane_is_not_dropped() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (segments1, content1) = many_segment_payload_at(80, 24);
    let encoded1 = mux_ipc::protocol::encode_snapshot_payload(&segments1, &content1);
    tab.apply_mux_message(snapshot_msg(10, encoded1));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(!tab.test_has_pending_switch());
    assert_eq!(tab.test_offthread_spawn_count(), 1);

    // Well after the first has settled (no in-flight switch left at
    // all), a repeat switch to the SAME pane arrives.
    let (segments2, content2) = many_segment_payload_at(80, 24);
    let encoded2 = mux_ipc::protocol::encode_snapshot_payload(&segments2, &content2);
    tab.apply_mux_message(snapshot_msg(10, encoded2));

    assert!(
        tab.test_has_pending_switch(),
        "a repeat switch arriving after the prior one already settled \
         must not be dropped"
    );
    assert_eq!(
        tab.test_offthread_spawn_count(),
        2,
        "with no in-flight request left to coalesce against, this must \
         spawn its own worker immediately"
    );
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
}

// ── task0006: pending_switch/pending_redispatch redesign — FR7
// target-dims mismatch, FR8 decode dedup scope, and the live-queue
// loss/duplication defect introduced by round-1's own auto-fix ────────

/// task0006 AC-1 (FR7, review round-1 finding `64baa639d71792f9`): a
/// grid resize racing an in-flight switch must not defeat the bypass
/// split for the build that eventually completes. Resolved by
/// deferring the resize (`PendingSwitch::pending_resize`) instead of
/// re-dispatching the in-flight worker at a target its `segments`
/// were never captured at — the worker's OWN dispatch-time target is
/// unaffected by the race, so its bypass split stays valid; the
/// resize is applied afterward via an ordinary `TerminalCore::resize`
/// on the swapped-in core. Bypass engagement is observed indirectly
/// via the 2nd-pass scrollback-restore worker being spawned — the
/// same signal `ac2_unraced_switch_engages_bypass` uses (see that
/// test's doc for why `scrollback_populated: false` <=> bypass
/// engaged <=> the restore worker spawns).
#[test]
fn t6_ac1_resize_during_in_flight_switch_still_engages_bypass() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let (cols, rows) = {
        let c = tab.core.lock();
        (c.cols(), c.rows())
    };
    // Segments recorded at the ORIGINAL (dispatch-time) target — the
    // shape a real daemon-captured payload has: it reflects whatever
    // grid the daemon knew about BEFORE this resize lands.
    let (segments, content) = many_segment_payload_at(cols, rows);
    let encoded = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    tab.apply_mux_message(snapshot_msg(10, encoded));
    assert!(tab.test_has_pending_switch(), "must go off-thread");

    // A resize races the in-flight switch, landing on a DIFFERENT
    // target than the segments were captured at.
    tab.resize(100, 40);
    assert_eq!(tab.test_pending_resize(), Some((100, 40)));

    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(
        tab.test_has_pending_scrollback_restore(),
        "a resize racing an in-flight switch must not defeat the \
         bypass split for the build that completes — the worker's \
         OWN target (unaffected by the race) still matches the \
         segments' recorded dims, so bypass still engages"
    );
    let c = tab.core.lock();
    assert_eq!(
        (c.cols(), c.rows()),
        (100, 40),
        "the racing resize still lands on the displayed core"
    );
}

/// task0006 AC-3 (live-output correctness, review round-1 findings
/// `7ed0ba7335376c20` / `ebc9de26bb15fcb1`): "Snapshot P1 dispatched
/// -> live output L1 arrives -> Snapshot P2 for the same pane arrives
/// (coalesces) -> more live output L2 arrives -> poll" — neither L1
/// nor L2 is lost or duplicately applied against the final,
/// P2-based core.
///
/// P2's own content bakes in L1's effect (row 1), simulating a real
/// daemon capture taken AFTER L1's PTY activity — the discard/keep
/// decision this task moves to coalesce time (`dispatch_offthread_replay`'s
/// same-pane branch) must clear the stale L1 there so it is not
/// re-applied on top of P2 (duplication), while L2 — arriving strictly
/// AFTER the coalesce, never captured by P2 — must still land exactly
/// once (loss is the pre-fix regression this test pins).
#[test]
fn t6_ac3_coalesced_snapshot_live_output_neither_lost_nor_duplicated() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

    // P1 dispatched off-thread.
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    assert!(tab.test_has_pending_switch());

    // L1 arrives while P1 is in flight.
    tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

    // P2: a fresh same-pane snapshot whose own content already bakes
    // in L1.
    let mut p2 = b"FIRST\r\nL1\r\n".to_vec();
    p2.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
    tab.apply_mux_message(snapshot_msg(10, p2));
    assert!(
        tab.test_has_pending_redispatch(),
        "a second snapshot for the same pane must coalesce"
    );
    assert!(
        tab.test_pending_live_queue().is_empty(),
        "L1 was subsumed into P2's own content — the coalesce must \
         clear it so it is not re-applied on top of P2 (duplication)"
    );

    // L2 arrives after the coalesce, before poll — genuinely new
    // output not reflected in P2's content.
    tab.apply_mux_message(pty_output(10, b"L2".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L2".to_vec()]);

    // Resolve: the fresh worker for P2 completes, L2 replays on top.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(tab.test_row_text(0), "FIRST");
    assert_eq!(
        tab.test_row_text(1),
        "L1",
        "L1 applied exactly once, via P2's own content"
    );
    assert_eq!(tab.test_row_text(2), "L2", "L2 must not be lost");
    assert_eq!(
        tab.test_row_text(3),
        "",
        "nothing duplicately applied past L2"
    );
}

/// task0006 AC-4 (live-output correctness, resize-driven case): a
/// resize-driven re-dispatch of the SAME payload (task0006 redesign:
/// this no longer re-dispatches at all — see
/// `PendingSwitch::pending_resize`) must still preserve queued live
/// output and apply it exactly once — the original, correct behavior
/// the round-1 fix (`old.payload == payload` at poll time) was trying
/// to preserve, now achieved for free since the queue is never touched
/// by a resize at all.
#[test]
fn t6_ac4_resize_driven_case_preserves_live_queue_exactly_once() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    assert!(tab.test_has_pending_switch());

    tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

    // A grid resize races the SAME in-flight payload (no new
    // snapshot arrives).
    tab.resize(100, 40);
    assert!(
        !tab.test_has_pending_redispatch(),
        "a resize alone must not coalesce a re-dispatch (FR7 fix)"
    );
    assert_eq!(
        tab.test_pending_live_queue(),
        vec![b"L1".to_vec()],
        "the queue must survive the resize untouched"
    );

    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(tab.test_row_text(0), "FIRST");
    assert_eq!(tab.test_row_text(1), "L1");
    assert_eq!(tab.test_row_text(2), "", "L1 applied exactly once");
    let c = tab.core.lock();
    assert_eq!((c.cols(), c.rows()), (100, 40));
}

/// task0006 AC-5 (live-output correctness, chained coalesce): 3+
/// chained same-pane transitions before a single poll — P1 dispatched,
/// L1 arrives, a resize races (deferred, no re-dispatch), P2 arrives
/// (coalesces, subsumes L1), L2 arrives, P3 arrives (coalesces again,
/// subsumes L2, supersedes P2 entirely) — live output must be
/// attributed correctly across ALL the intermediate transitions, not
/// just a single coalesce hop. Only P3's replay ever completes.
#[test]
fn t6_ac5_chained_coalesce_across_resize_and_two_snapshots_attributes_live_output_correctly() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));

    // P1
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    assert!(tab.test_has_pending_switch());

    // L1
    tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

    // A resize races the in-flight P1 — deferred, no re-dispatch; the
    // queue is untouched by it (task0006 AC-4's own claim).
    tab.resize(100, 40);
    assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

    // P2: a fresh same-pane snapshot whose own content already bakes
    // in L1.
    let mut p2 = b"FIRST\r\nL1\r\n".to_vec();
    p2.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
    tab.apply_mux_message(snapshot_msg(10, p2));
    assert!(tab.test_has_pending_redispatch());
    assert!(
        tab.test_pending_live_queue().is_empty(),
        "L1 subsumed by P2's own content at THIS coalesce"
    );

    // L2 arrives after P2 coalesced, before poll.
    tab.apply_mux_message(pty_output(10, b"L2".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L2".to_vec()]);

    // P3: another fresh same-pane snapshot, superseding P2 (which is
    // never replayed), whose own content bakes in L2 too.
    let mut p3 = b"FIRST\r\nL1\r\nL2\r\n".to_vec();
    p3.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
    tab.apply_mux_message(snapshot_msg(10, p3));
    assert!(tab.test_has_pending_redispatch());
    assert!(
        tab.test_pending_live_queue().is_empty(),
        "L2 subsumed by P3's own content at THIS (second) coalesce — \
         each new coalesce re-evaluates the discard/keep decision, \
         not just the first hop"
    );

    // Resolve: only P3's replay ever completes, at the resized grid.
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(tab.test_row_text(0), "FIRST");
    assert_eq!(tab.test_row_text(1), "L1");
    assert_eq!(tab.test_row_text(2), "L2");
    assert_eq!(tab.test_row_text(3), "", "nothing duplicately applied");
    let c = tab.core.lock();
    assert_eq!((c.cols(), c.rows()), (100, 40));
}

/// task0006 AC-6 (FR8 scope): FR8's replay-BUILD dedup (task0003,
/// unaffected by this task) means a duplicate same-pane snapshot never
/// spawns a second WORKER — but every incoming `Snapshot`/
/// `SnapshotRestore` frame still runs `decode_snapshot_payload_typed`.
/// This task's redesign prioritized FR7 and the live-queue lifecycle
/// (both correctness-critical) and kept FR8 at the replay-build level
/// only rather than adding fetch/decode-level dedup (medium severity,
/// secondary per the task plan) — this test pins that narrower claim
/// explicitly so the requirement and the test do not silently
/// disagree. `red_confirmed: false` in this task's test record: this
/// documents PRE-EXISTING, unchanged behavior (identical before and
/// after this task), not a fix.
#[test]
fn t6_ac6_duplicate_same_pane_snapshot_still_decodes_twice_replay_build_dedup_only() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    assert_eq!(tab.test_snapshot_decode_count(), 1);
    assert_eq!(tab.test_offthread_spawn_count(), 1);

    // A duplicate same-pane snapshot arrives before the first swaps.
    tab.apply_mux_message(snapshot_msg(10, large_payload("SECOND")));
    assert_eq!(
        tab.test_snapshot_decode_count(),
        2,
        "FR8 dedup happens at the replay-BUILD level (no second \
         worker spawn, see test_offthread_spawn_count below) — decode \
         still runs for every incoming frame"
    );
    assert_eq!(
        tab.test_offthread_spawn_count(),
        1,
        "the worker spawn itself IS deduplicated"
    );
}

/// task0006 AC-7 (performance, review round-1 finding
/// `34a708465d04f983`): the wasteful full-payload clone
/// `Tab::resize`'s old redispatch branch performed on every resize
/// event while a same-pane coalesce was pending no longer happens —
/// demonstrated as a side effect of the AC-1 fix: since a resize
/// never calls `dispatch_offthread_replay` (the only call site that
/// ever cloned `pending.payload`/`.segments` for this case) any more,
/// there is no code path left that could perform that clone. Observed
/// indirectly via `test_offthread_spawn_count` staying flat across a
/// resize storm (`t6_ac1`'s sibling assertion in
/// `ac1_resize_storm_during_pending_switch_collapses_to_one_redispatch`
/// proves the same absence of a redispatch call, which is what the
/// clone was conditional on).
#[test]
fn t6_ac7_resize_storm_performs_no_redispatch_hence_no_payload_clone() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    assert!(tab.test_has_pending_switch());

    // A resize storm: several events, each landing on a different
    // target, before the worker is ever polled.
    for (cols, rows) in [(90, 30), (95, 35), (100, 40), (105, 45)] {
        tab.resize(cols, rows);
        assert!(
            !tab.test_has_pending_redispatch(),
            "no resize event may coalesce a re-dispatch — the clone \
             `Tab::resize`'s old branch performed to build one is \
             gone along with the redispatch call itself"
        );
    }
    assert_eq!(tab.test_pending_resize(), Some((105, 45)));
}

/// task0006 AC-8 (performance, review round-1 finding
/// `5b1878c41d3e02d6`): the O(n) `old.payload == payload` full-byte
/// comparison round-1's fix added to `poll_pending_switch` is gone —
/// removed as a direct consequence of moving the discard/keep decision
/// to coalesce time (AC-3). This is a structural/code-level property
/// (best confirmed by inspection: `poll_pending_switch`'s
/// `pending_redispatch`-take branch now unconditionally inherits
/// `old.live_queue`/`old.queued_bytes`, replacing the byte comparison
/// with a `debug_assert_eq!` on pane identity alone); this test pins
/// the OBSERVABLE half of that claim — the coalesce path (where the
/// decision now lives) runs in O(1) relative to payload size, checked
/// by exercising it with a large payload and confirming the outcome
/// is still correct (the byte-size-dependent cost, if it existed,
/// would not change the RESULT, only the time — so this test's real
/// value is pinning that the coalesce-time clear behaves correctly
/// even for a large payload, alongside the code-inspection evidence
/// noted above).
#[test]
fn t6_ac8_large_payload_coalesce_clears_queue_without_poll_time_comparison() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // A payload well past the off-thread threshold.
    tab.apply_mux_message(snapshot_msg(10, large_payload("FIRST")));
    tab.apply_mux_message(pty_output(10, b"L1".to_vec()));
    assert_eq!(tab.test_pending_live_queue(), vec![b"L1".to_vec()]);

    // A large, DIFFERENT same-pane payload coalesces.
    tab.apply_mux_message(snapshot_msg(10, large_payload("SECOND")));
    assert!(
        tab.test_pending_live_queue().is_empty(),
        "the discard decision at coalesce time does not depend on \
         comparing this (large) payload against the old one byte for \
         byte — see AC-8's doc for the removed poll-time comparison"
    );
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert_eq!(tab.test_row_text(0), "SECOND");
}
