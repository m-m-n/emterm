use super::*;

// ── tmux-startup-query-response-leak (task0001) ───────────────────────
//
// Root cause: `term_core` fires every synthesized device response
// through TWO independent channels for the SAME event —
// (1) the single-slot `response_buffer`, polled via `take_response()`
//     at each of `tabs.rs`'s three write-back sites
//     (`process_outer_via_core`, `apply_active_pane_output`,
//     `apply_queued_live_output`) and delivered via
//     `write_device_response` (mux-aware routing); and
// (2) `fire_device_response_callback()` → `NativeCallbacks::
//     on_device_response` → (pre-fix) `NativeCallbackState::
//     device_responses`, drained once per pump by `process_combined`
//     and written RAW via `Tab::write` — unconditionally, bypassing
//     `write_device_response`'s mux routing entirely.
// `crates/term_core/src/csi_dispatch.rs` fires both from the same CSI
// dispatch arm (e.g. `(Some(b'>'), b'c') => { ...; if len > 0 {
// self.fire_device_response_callback(); } }`), so a single query
// produces one write via each channel. In the plain-tab context (tmux
// running directly inside an eMterm tab — the reported symptom),
// `process_outer_via_core` delivers the reply once via `take_response`/
// `write_device_response`; the SAME pump's `process_combined` then
// drained the callback queue and wrote the IDENTICAL bytes a second
// time. tmux consumes the first copy for capability negotiation but has
// already moved past the query window when the redundant second copy
// arrives, so it forwards those bytes to the shell as ordinary input,
// which echoes them onto the screen — the observed leak. The fix
// removes the second (callback-based) channel entirely; the
// `take_response()`-based channel, already wired into all three
// write-back sites and mux-aware, remains the sole delivery route
// (matching IMPLEMENTATION.md's documented `take_response` contract).

/// AC-1: reproduces the leak mechanism at byte level. Pre-fix, a single
/// DA2 query delivered through the plain-tab pump path
/// (`process_combined` → `process_outer_via_core`, no mux session)
/// reaches the outbound channel TWICE — once via `write_device_response`
/// (the `take_response()` channel) and once more via the redundant
/// `NativeCallbackState::device_responses` drain in `process_combined`.
/// Post-fix, exactly one copy reaches the outbound channel.
#[test]
fn process_combined_delivers_da2_response_exactly_once() {
    let mut tab = test_tab();
    tab.process_combined(b"\x1b[>0c".to_vec()); // Secondary DA (DA2) query
    let writes = tab.test_outbound_writes();
    let matches = writes
        .iter()
        .filter(|w| w.as_slice() == b"\x1b[>65;1;0c")
        .count();
    assert_eq!(
        matches, 1,
        "DA2 response must reach the querying PTY's inbound side exactly \
         once (root cause: term_core fires both the take_response() \
         poll channel and the on_device_response callback channel for \
         the same synthesized reply); got {matches} occurrences within \
         {writes:?}"
    );
}

/// The 8 in-scope response types (FR5 `generalize` resolution): each
/// query byte sequence paired with the exact reply `term_core` (fresh
/// 80x24 tab, cursor at 0,0, default 8x16 cell metrics) synthesizes for
/// it — see `crates/term_core/src/csi_device.rs`'s inline tests for the
/// same byte-for-byte expectations.
fn device_response_cases() -> Vec<(&'static str, &'static [u8], &'static [u8])> {
    vec![
        ("DA1", b"\x1b[c", b"\x1b[?65;1;4;22c"),
        ("DA2", b"\x1b[>0c", b"\x1b[>65;1;0c"),
        ("DSR status", b"\x1b[5n", b"\x1b[0n"),
        ("CPR", b"\x1b[6n", b"\x1b[1;1R"),
        (
            "XTWINOPS 14 (text area px)",
            b"\x1b[14t",
            b"\x1b[4;384;640t",
        ),
        ("XTWINOPS 16 (cell size)", b"\x1b[16t", b"\x1b[6;16;8t"),
        (
            "XTWINOPS 18 (text area chars)",
            b"\x1b[18t",
            b"\x1b[8;24;80t",
        ),
        ("DECRPM (mode 2026)", b"\x1b[?2026$p", b"\x1b[?2026;2$y"),
    ]
}

/// AC-2: on the plain-tab path (`process_combined`, no mux session),
/// every in-scope response type is delivered to the querying PTY's
/// inbound side exactly once, and the query/response bytes never
/// render into the grid.
#[test]
fn plain_tab_every_response_type_delivered_exactly_once_and_absent_from_grid() {
    for (name, query, expected) in device_response_cases() {
        let mut tab = test_tab();
        tab.core.lock().set_cursor(0, 0);
        tab.process_combined(query.to_vec());
        let writes = tab.test_outbound_writes();
        let matches = writes.iter().filter(|w| w.as_slice() == expected).count();
        assert_eq!(
            matches, 1,
            "{name}: expected exactly one delivery of {expected:?}, got \
             {matches} within {writes:?}"
        );
        let (rows, _, _) = displayed_fingerprint(&tab);
        assert!(
            rows.iter().all(|r| r.is_empty()),
            "{name}: query/response bytes must not render into the \
             grid, got {rows:?}"
        );
    }
}

/// AC-2: five distinct in-scope queries fed as ONE combined buffer
/// through `process_combined`'s pre-mux branch (`process_outer_via_core`,
/// a single `process_pty_data_fully` + `take_response` call) must all
/// reach the querying PTY's inbound side, in query order, and never
/// render into the grid. Pre-task0002 (single-slot `response_buffer`),
/// this delivers only the LAST query's reply.
#[test]
fn plain_tab_multi_query_chunk_delivers_all_responses_in_order_exactly_once() {
    let mut tab = test_tab();
    tab.core.lock().set_cursor(0, 0);
    let combined: Vec<u8> = [
        &b"\x1b[c"[..],   // DA1
        &b"\x1b[>0c"[..], // DA2
        &b"\x1b[5n"[..],  // DSR status
        &b"\x1b[14t"[..], // XTWINOPS 14
        &b"\x1b[18t"[..], // XTWINOPS 18
    ]
    .concat();

    tab.process_combined(combined);

    let expected: Vec<u8> = [
        &b"\x1b[?65;1;4;22c"[..], // DA1
        &b"\x1b[>65;1;0c"[..],    // DA2
        &b"\x1b[0n"[..],          // DSR status
        &b"\x1b[4;384;640t"[..],  // XTWINOPS 14
        &b"\x1b[8;24;80t"[..],    // XTWINOPS 18
    ]
    .concat();
    let writes = tab.test_outbound_writes();
    let matches = writes
        .iter()
        .filter(|w| w.as_slice() == expected.as_slice())
        .count();
    assert_eq!(
        matches, 1,
        "one combined chunk with 5 queries must produce exactly one \
         outbound write containing all 5 replies concatenated in \
         synthesis order, got {matches} within {writes:?}"
    );

    let (rows, _, _) = displayed_fingerprint(&tab);
    assert!(
        rows.iter().all(|r| r.is_empty()),
        "multi-query chunk bytes must not render into the grid, got {rows:?}"
    );
}

/// AC-3: the same delivery/visibility assertions hold on the mux-pane
/// path — a device query arriving as a per-frame `PtyOutput` over the
/// REAL pump entry point (`process_combined`'s mux-transport branch,
/// not `apply_mux_message` called directly) is delivered to the active
/// pane exactly once (as a `PtyInput` frame via `send_control`, not a
/// raw write) and never renders into the grid.
///
/// The second, empty `process_combined` call matters: in mux mode the
/// (pre-fix) `NativeCallbackState::device_responses` drain runs BEFORE
/// the frame-apply loop that parses mux inner content, so a callback
/// push from THIS pump's query is only observed and (wrongly, via a
/// raw unrouted `Tab::write`) delivered on the NEXT pump — going
/// straight through `apply_mux_message` alone (as the earlier revision
/// of this test did) never exercises that drain and would pass even
/// pre-fix, silently missing the mux-context half of the bug.
#[test]
fn mux_pane_every_response_type_delivered_exactly_once_and_absent_from_grid() {
    for (name, query, expected) in device_response_cases() {
        let mut tab = test_tab();
        tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
        tab.core.lock().set_cursor(0, 0);
        tab.process_combined(pty_output_apc(10, query));
        tab.process_combined(Vec::new());
        let writes = tab.test_outbound_writes();
        let matches = writes.iter().filter(|w| w.as_slice() == expected).count();
        assert_eq!(
            matches, 1,
            "{name} (mux pane): expected exactly one delivery of \
             {expected:?}, got {matches} within {writes:?}"
        );
        let (rows, _, _) = displayed_fingerprint(&tab);
        assert!(
            rows.iter().all(|r| r.is_empty()),
            "{name} (mux pane): query/response bytes must not render \
             into the grid, got {rows:?}"
        );
    }
}

/// AC-3: five distinct in-scope queries embedded in ONE mux `PtyOutput`
/// frame's inner payload (`apply_active_pane_output`, a single
/// `process_pty_data_fully` + `take_response` call) must all reach the
/// active pane, as a single `PtyInput` frame containing all 5 replies
/// concatenated in query order, and never render into the grid.
#[test]
fn mux_pane_multi_query_chunk_delivers_all_responses_in_order_exactly_once() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    tab.core.lock().set_cursor(0, 0);
    let combined_inner: Vec<u8> = [
        &b"\x1b[c"[..],   // DA1
        &b"\x1b[>0c"[..], // DA2
        &b"\x1b[5n"[..],  // DSR status
        &b"\x1b[14t"[..], // XTWINOPS 14
        &b"\x1b[18t"[..], // XTWINOPS 18
    ]
    .concat();

    tab.process_combined(pty_output_apc(10, &combined_inner));
    tab.process_combined(Vec::new());

    let expected: Vec<u8> = [
        &b"\x1b[?65;1;4;22c"[..], // DA1
        &b"\x1b[>65;1;0c"[..],    // DA2
        &b"\x1b[0n"[..],          // DSR status
        &b"\x1b[4;384;640t"[..],  // XTWINOPS 14
        &b"\x1b[8;24;80t"[..],    // XTWINOPS 18
    ]
    .concat();
    let writes = tab.test_outbound_writes();
    let matches = writes
        .iter()
        .filter(|w| w.as_slice() == expected.as_slice())
        .count();
    assert_eq!(
        matches, 1,
        "one mux PtyOutput frame with 5 queries must produce exactly \
         one outbound PtyInput write containing all 5 replies \
         concatenated in synthesis order, got {matches} within {writes:?}"
    );

    let (rows, _, _) = displayed_fingerprint(&tab);
    assert!(
        rows.iter().all(|r| r.is_empty()),
        "multi-query chunk bytes must not render into the grid (mux \
         pane), got {rows:?}"
    );
}

/// AC-3: a device query arriving inside a pending-switch's queued live
/// output (`apply_queued_live_output`, the mux off-thread-switch replay
/// path) is delivered to the active pane exactly once. The trailing
/// empty `process_combined` call drains any stale
/// `NativeCallbackState::device_responses` entry the callback fired
/// during `apply_queued_live_output`'s own `process_pty_data_fully`
/// call would (pre-fix) leave queued for the next pump — see the mux
/// per-frame test's doc for why a caller that never reaches
/// `process_combined` cannot observe that half of the bug.
#[test]
fn apply_queued_live_output_delivers_device_response_exactly_once() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10)], 0));
    tab.apply_queued_live_output(vec![b"\x1b[c".to_vec()]); // DA1 query
    tab.process_combined(Vec::new());
    let writes = tab.test_outbound_writes();
    let matches = writes
        .iter()
        .filter(|w| w.as_slice() == b"\x1b[?65;1;4;22c")
        .count();
    assert_eq!(
        matches, 1,
        "DA1 response queued via apply_queued_live_output must reach \
         the active pane exactly once, got {matches} within {writes:?}"
    );
}

/// AC-3: a device query embedded in snapshot/reattach replay bytes
/// (`reset_frame_for_replay`) must produce NO outbound write at all —
/// the originating program is long gone. Complements
/// `reset_frame_for_replay_discards_historic_device_responses` (which
/// asserts the `response_buffer` half of this invariant); this asserts
/// the outbound side stays silent too, which the callback-based
/// duplicate-delivery channel this task removes would otherwise have
/// broken (it drained independently of `reset_frame_for_replay`'s
/// `take_response()` discard).
#[test]
fn reset_frame_for_replay_produces_no_outbound_device_response_write() {
    let mut tab = test_tab();
    let mut snapshot = Vec::new();
    snapshot.extend_from_slice(b"row one\r\n");
    snapshot.extend_from_slice(b"\x1b[c"); // DA1 query baked into snapshot
    let _ = tab.reset_frame_for_replay(&snapshot, &[]);
    assert!(
        tab.test_outbound_writes().is_empty(),
        "a historic query baked into snapshot bytes must not produce \
         ANY outbound write — the originating program is long gone"
    );
}

/// AC-4 / NFR3: a byte sequence that merely resembles a device query or
/// its response, embedded in ordinary printable output (no leading ESC,
/// so it never dispatches as a query), reaches the grid unchanged and
/// synthesizes no response.
#[test]
fn plain_tab_query_lookalike_text_reaches_grid_unchanged() {
    let mut tab = test_tab();
    let payload = b">65;1;0c literal, not a query\r\nnext line".to_vec();
    tab.process_combined(payload);
    let (rows, _, _) = displayed_fingerprint(&tab);
    assert!(rows[0].starts_with(">65;1;0c literal, not a query"));
    assert!(rows[1].starts_with("next line"));
    assert!(
        tab.test_outbound_writes().is_empty(),
        "lookalike text must not synthesize any device response"
    );
}

/// AC-4 / NFR3: ordinary output containing no device queries renders
/// byte-identically whether it goes through the pump path
/// (`process_combined`) or a direct core parse, and produces no
/// outbound write.
#[test]
fn plain_tab_ordinary_output_without_queries_matches_direct_core_parse() {
    let mut tab = test_tab();
    let payload = b"hello world\r\nsecond line\r\n".to_vec();
    tab.process_combined(payload.clone());
    let (via_pump, _, _) = displayed_fingerprint(&tab);

    let reference = test_tab();
    reference.core.lock().process_pty_data_fully(&payload);
    let (direct, _, _) = displayed_fingerprint(&reference);

    assert_eq!(
        via_pump, direct,
        "ordinary output must be byte-identical whether it goes \
         through process_combined or a direct core parse"
    );
    assert!(tab.test_outbound_writes().is_empty());
}

/// TS-5 / FR3: an off-thread snapshot parse + queued live output applied
/// after the swap is byte/grid-identical to one contiguous synchronous
/// parse of `snapshot ++ live`, and the prompt-mark tracker matches.
#[test]
fn ts5_offthread_swap_plus_live_equals_contiguous_parse() {
    // Build a large snapshot with an OSC 133 prompt mark + visible text,
    // then two live chunks that add more text and another prompt mark.
    let mut snapshot = b"\x1b]133;A\x07first-row\r\n".to_vec();
    snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
    let live1 = b"live-line-1\r\n".to_vec();
    let live2 = b"\x1b]133;A\x07live-line-2".to_vec();

    // Reference: a tab that replays snapshot ++ live as one synchronous
    // frame (reset_frame_for_replay) then feeds the live chunks as
    // ordinary output — exactly the legacy behavior with no off-thread gap.
    let mut reference = test_tab();
    reference.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    reference.reset_frame_for_replay(&snapshot, &[]);
    reference.apply_queued_live_output(vec![live1.clone(), live2.clone()]);

    // Off-thread path: dispatch, queue the live chunks, then swap.
    let mut offthread = test_tab();
    offthread.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    offthread.apply_mux_message(snapshot_msg(10, snapshot));
    offthread.apply_mux_message(pty_output(10, live1));
    offthread.apply_mux_message(pty_output(10, live2));
    assert_eq!(offthread.test_poll_until_swapped(), SwapOutcome::Swapped);
    assert!(!offthread.test_has_pending_switch());

    // Grid + cursor identical.
    assert_eq!(
        displayed_fingerprint(&offthread),
        displayed_fingerprint(&reference)
    );
    // Prompt-mark tracker identical (both prompt marks present, same rows).
    assert_eq!(
        offthread.prompts.find_prev_prompt(u32::MAX),
        reference.prompts.find_prev_prompt(u32::MAX)
    );
    assert_eq!(
        offthread.prompts.find_next_prompt(0),
        reference.prompts.find_next_prompt(0)
    );
}

/// TS-5: queued live output is applied in arrival order (a later chunk's
/// content overwrites / follows an earlier chunk's, never reordered).
#[test]
fn ts5_queued_live_output_applied_in_order() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("SNAP")));
    // Three ordered chunks, each writing to a fresh line.
    tab.apply_mux_message(pty_output(10, b"AAA\r\n".to_vec()));
    tab.apply_mux_message(pty_output(10, b"BBB\r\n".to_vec()));
    tab.apply_mux_message(pty_output(10, b"CCC".to_vec()));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    // Row 0 = snapshot marker; rows 1..3 = the live chunks in order.
    assert_eq!(tab.test_row_text(0), "SNAP");
    assert_eq!(tab.test_row_text(1), "AAA");
    assert_eq!(tab.test_row_text(2), "BBB");
    assert_eq!(tab.test_row_text(3), "CCC");
}

/// TS-7 / FR7: on worker failure the swap falls back to a synchronous
/// reparse of the latest target, with the queued live output applied in
/// order — the displayed result is correct.
#[test]
fn ts7_worker_failure_falls_back_to_sync_reparse() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    tab.apply_mux_message(snapshot_msg(10, large_payload("FALLBACK")));
    tab.apply_mux_message(pty_output(10, b"after\r\n".to_vec()));
    // Simulate the worker panicking (sender dropped → Disconnected).
    tab.test_force_worker_disconnect();
    assert_eq!(tab.poll_pending_switch(), SwapOutcome::Swapped);
    assert!(!tab.test_has_pending_switch());
    // Snapshot reparsed synchronously + queued live applied in order.
    assert_eq!(tab.test_row_text(0), "FALLBACK");
    assert_eq!(tab.test_row_text(1), "after");
}

/// FR1: polling with no pending switch is a cheap no-op.
#[test]
fn poll_pending_switch_idle_when_none() {
    let mut tab = test_tab();
    assert_eq!(tab.poll_pending_switch(), SwapOutcome::Idle);
}

/// FR1: the swap replaces the displayed core's content (the outgoing
/// pane's content is gone after the swap).
#[test]
fn swap_replaces_outgoing_content() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    {
        let mut c = tab.core.lock();
        c.process_pty_data_fully(b"OUTGOING-PANE");
    }
    tab.apply_mux_message(snapshot_msg(10, large_payload("NEWPANE")));
    // Before the swap, outgoing content is still shown.
    assert_eq!(tab.test_row_text(0), "OUTGOING-PANE");
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    // After the swap, the worker-built content replaced it.
    assert_eq!(tab.test_row_text(0), "NEWPANE");
}

// Keep `small_snapshot_bytes` referenced (used by the integration test in
// Phase 4) without an unused-fn warning when that test is filtered out.
#[test]
fn small_snapshot_helper_is_below_threshold() {
    assert!(small_snapshot_bytes("hi").len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES);
}

/// TS-9 / FR2: swapping in a snapshot whose content occupies fewer rows
/// than the outgoing pane leaves NO residual rows — every row past the
/// snapshot's content is blank in the swapped-in core. The worker builds
/// a fresh core (`reset_and_replay`), so residual rows cannot survive the
/// swap; this locks that invariant in under the off-thread path.
#[test]
fn ts9_no_residual_rows_after_offthread_swap_to_shorter_pane() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // Outgoing pane: fill many rows with content.
    {
        let mut c = tab.core.lock();
        let mut bytes = Vec::new();
        for i in 0..20 {
            bytes.extend_from_slice(format!("outgoing row {i}\r\n").as_bytes());
        }
        c.process_pty_data_fully(&bytes);
    }
    // Confirm the outgoing pane really has content on a deep row.
    assert!(!tab.test_row_text(10).is_empty());

    // Incoming snapshot: only two rows of content (large enough to go
    // off-thread).
    let mut snapshot = b"only-row-0\r\nonly-row-1".to_vec();
    snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
    tab.apply_mux_message(snapshot_msg(10, snapshot));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    // Rows 0/1 hold the snapshot; every later row is blank — no residual.
    assert_eq!(tab.test_row_text(0), "only-row-0");
    assert_eq!(tab.test_row_text(1), "only-row-1");
    let rows = tab.core.lock().rows();
    for r in 2..rows {
        assert_eq!(
            tab.test_row_text(r),
            "",
            "row {r} must be blank after swap (no residual rows, FR2)"
        );
    }
}

/// TS-9 / NFR1: marks/folds + the eviction baseline after an off-thread
/// swap match the synchronous `reset_frame_for_replay` path for the same
/// snapshot (parity).
#[test]
fn ts9_marks_and_baseline_parity_with_sync_path() {
    // A snapshot with an OSC 133 A/B/C/D cycle (a foldable command region)
    // plus scrollback growth so the eviction baseline is exercised.
    let mut snapshot = Vec::new();
    snapshot.extend_from_slice(b"\x1b]133;A\x07$ \x1b]133;B\x07cmd\x1b]133;C\x07\r\n");
    for i in 0..30 {
        snapshot.extend_from_slice(format!("out {i}\r\n").as_bytes());
    }
    snapshot.extend_from_slice(b"\x1b]133;D;0\x07");
    let mut large = snapshot.clone();
    large.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + large.len(), 0);

    // Synchronous reference (sub-threshold, legacy path).
    let mut reference = test_tab();
    reference.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    reference.reset_frame_for_replay(&snapshot, &[]);

    // Off-thread path (padded past the threshold; NUL padding does not
    // change the grid/marks).
    let mut offthread = test_tab();
    offthread.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    offthread.apply_mux_message(snapshot_msg(10, large));
    assert_eq!(offthread.test_poll_until_swapped(), SwapOutcome::Swapped);

    // Prompt navigation parity.
    assert_eq!(
        offthread.prompts.find_prev_prompt(u32::MAX),
        reference.prompts.find_prev_prompt(u32::MAX)
    );
    // Fold-region parity: both paths registered the same number of
    // foldable OSC 133 C→D regions.
    assert_eq!(
        offthread.folds.region_count(),
        reference.folds.region_count(),
        "off-thread and sync paths must register the same fold regions"
    );
}

// ── tmux-startup-query-response-leak (task0003, review round-1 rework)
//
// `reset_frame_for_replay` (the synchronous replay path) explicitly
// discards device responses synthesized while replaying historic
// snapshot bytes (see `reset_frame_for_replay_discards_historic_device_
// responses` / `reset_frame_for_replay_produces_no_outbound_device_
// response_write` above). Review round-1 finding `8bebc1e532a1b597`
// reported that `apply_offthread_swap` installs the worker-built core
// (`*live = new_core;`) without that discard.
//
// Investigation for this task found the finding's premise does not
// hold against the current codebase: `TerminalCore::
// build_from_snapshot_inner` (the function underlying BOTH
// `build_from_snapshot`, the 1st-pass worker build, and
// `build_scrollback_only_from_snapshot`, the 2nd-pass restore) already
// drains `response_buffer` before returning `SnapshotReplay` — added by
// commit `4380805c` ("fix(mux): drop stale device responses from
// snapshot replay", 2026-06-24), which fixed BOTH the synchronous
// (`reset_frame_for_replay`) and off-thread-worker paths in the same
// change, predating this feature entirely. By the time
// `apply_offthread_swap` installs `replay.core` as the live core, its
// response buffer is already empty. The test below (AC-1) confirms
// this holds today; it could NOT be driven red by any change confined
// to `tabs.rs` (this task's file scope) — removing term_core's
// existing discard would be required, which is out of this task's
// scope. The explicit discard added at the swap site below is kept as
// defense-in-depth (D5's documented intent: the invariant should not
// depend implicitly on term_core internals), not as a fix for an
// active leak.

/// AC-1: a device query embedded in snapshot bytes replayed through the
/// OFF-THREAD swap path (`apply_offthread_swap`) must produce NO
/// outbound write and leave no pending response on the swapped-in
/// core — mirrors `reset_frame_for_replay_produces_no_outbound_device_
/// response_write`'s synchronous-path assertion, but drives the
/// off-thread worker-built-core swap instead. NOTE: this passes even
/// without the swap-site discard added by this task — see the module
/// comment above for why (the invariant already holds via
/// `build_from_snapshot_inner`'s pre-existing discard).
#[test]
fn apply_offthread_swap_discards_replay_generated_device_response() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    // `welcome_msg` itself emits mux session-setup control writes
    // (window activation / grid-size handshake) unrelated to device
    // responses; snapshot the log length here so the assertion below
    // only judges writes produced by the snapshot replay + swap.
    let writes_before = tab.test_outbound_writes().len();
    let mut snapshot = b"row one\r\n".to_vec();
    snapshot.extend_from_slice(b"\x1b[c"); // DA1 query baked into the snapshot
    snapshot.extend_from_slice(b"row two\r\n");
    snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
    tab.apply_mux_message(snapshot_msg(10, snapshot));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);
    let writes_after = tab.test_outbound_writes();
    assert_eq!(
        writes_after.len(),
        writes_before,
        "a DA1 query baked into snapshot bytes replayed off-thread must \
         not produce ANY new outbound write — the originating program \
         is long gone; new writes: {:?}",
        &writes_after[writes_before..]
    );
    assert_eq!(
        tab.core.lock().get_response_len(),
        0,
        "apply_offthread_swap must discard the worker-built core's \
         pending device response after installing it as the live core; \
         residual bytes would leak as PtyInput on the next live \
         take_response and corrupt the shell's stdin"
    );
}

/// AC-3: a device query arriving in LIVE output AFTER an off-thread
/// swap is still answered and delivered exactly once — the swap-site
/// discard added for AC-1 must not over-discard a live-arriving
/// response. Complements `apply_queued_live_output_delivers_device_
/// response_exactly_once` by driving the swap through the actual
/// dispatch/poll machinery instead of calling `apply_queued_live_
/// output` directly.
#[test]
fn query_arriving_live_after_offthread_swap_is_delivered_exactly_once() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "a", 10), (2, "b", 20)], 0));
    let mut snapshot = b"row one\r\n".to_vec();
    snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 8, 0);
    tab.apply_mux_message(snapshot_msg(10, snapshot));
    assert_eq!(tab.test_poll_until_swapped(), SwapOutcome::Swapped);

    // A query arriving in live PTY output AFTER the swap.
    tab.process_combined(pty_output_apc(10, b"\x1b[c")); // DA1
    tab.process_combined(Vec::new());
    let writes = tab.test_outbound_writes();
    let matches = writes
        .iter()
        .filter(|w| w.as_slice() == b"\x1b[?65;1;4;22c")
        .count();
    assert_eq!(
        matches, 1,
        "a DA1 query arriving in live output after an off-thread swap \
         must be delivered exactly once, got {matches} within {writes:?}"
    );
}

// ── mux transport/content parser isolation (TS-4..TS-9) ───────────────

use base64::Engine as _;

/// Base64-encode bytes the way the Kitty payload field expects.
fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A complete-in-one Kitty APC for a `w`×`h` raw-RGB image (`f=24`).
fn kitty_rgb_single(w: u32, h: u32) -> Vec<u8> {
    let raw = vec![0xABu8; (w * h * 3) as usize];
    let payload = b64(&raw);
    let mut v = vec![0x1b, b'_'];
    v.extend_from_slice(format!("Ga=T,f=24,s={w},v={h};{payload}").as_bytes());
    v.extend_from_slice(&[0x1b, b'\\']);
    v
}

/// A `w`×`h` raw-RGB Kitty image split into `parts` chunked APC frames
/// (`m=1` … `m=0`). The base64 payload is split at arbitrary character
/// boundaries across the chunks; the decoder concatenates the base64
/// strings before decoding, so any split reconstructs the same image.
fn kitty_rgb_chunked(w: u32, h: u32, parts: usize) -> Vec<Vec<u8>> {
    assert!(parts >= 2);
    let raw = vec![0xABu8; (w * h * 3) as usize];
    let payload = b64(&raw);
    let bytes = payload.as_bytes();
    let chunk = bytes.len().div_ceil(parts);
    let slices: Vec<&[u8]> = bytes.chunks(chunk).collect();
    let mut out = Vec::new();
    for (i, slice) in slices.iter().enumerate() {
        let first = i == 0;
        let last = i == slices.len() - 1;
        let m = if last { 0 } else { 1 };
        let mut apc = vec![0x1b, b'_'];
        let control = if first {
            format!("Ga=T,i=1,f=24,s={w},v={h},m={m};")
        } else {
            format!("Ga=T,i=1,m={m};")
        };
        apc.extend_from_slice(control.as_bytes());
        apc.extend_from_slice(slice);
        apc.extend_from_slice(&[0x1b, b'\\']);
        out.push(apc);
    }
    out
}

fn has_image_ready(events: &[ImageEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, ImageEvent::ImageReady { .. }))
}

// ── TS-4: split inner Kitty over mux PtyOutput boundaries ─────────────
#[test]
fn ts4_split_inner_kitty_over_mux_pty_output_assembles_one_image() {
    let mut tab = test_tab();
    // Establish mux with no window group → all PtyOutput accepted, and the
    // extractor engages from the next pump on.
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some(), "mux established");

    // A 4×4 RGB image (48 raw bytes → 64 base64 chars) split into 3 inner
    // Kitty chunks, each delivered as its own outer PtyOutput APC frame,
    // with a plain-text outer pump interleaved between them — the exact
    // shape that corrupted a shared parser.
    let chunks = kitty_rgb_chunked(4, 4, 3);

    // Chunk 1 (m=1): inner parser left mid-transfer.
    tab.test_process_combined(pty_output_apc(0, &chunks[0]));
    // Interleaving outer pump: a second mux PtyOutput carrying plain text.
    tab.test_process_combined(pty_output_apc(0, b"intervening text\r\n"));
    // Chunk 2 (m=1).
    tab.test_process_combined(pty_output_apc(0, &chunks[1]));
    // Chunk 3 (m=0): finalizes the transfer.
    let _ = tab.test_process_combined(pty_output_apc(0, &chunks[2]));

    let events = tab.drain_image_events();
    assert!(
        has_image_ready(&events),
        "split inner Kitty chunks must assemble into one decodable image; events={events:?}"
    );

    // No base64 of the image payload leaked onto the grid.
    let raw = vec![0xABu8; 4 * 4 * 3];
    let payload = b64(&raw);
    let grid = tab.test_grid_text();
    assert!(
        !grid.contains(&payload[..16]),
        "image base64 must not leak to the grid"
    );
    assert!(
        !grid.contains("emterm-mux"),
        "outer transport prefix must not leak to the grid"
    );
    assert!(
        !grid.contains("Ga=T"),
        "Kitty control data must not leak to the grid"
    );
    // The interleaved inner plain text DID reach the core (inner content
    // is what self.core renders).
    assert!(
        tab.test_grid_text().contains("intervening text"),
        "inner plain text must render via self.core"
    );
}

// ── TS-9: non-mux Kitty image still decodes (no regression) ───────────
#[test]
fn ts9_non_mux_kitty_image_still_decodes() {
    let mut tab = test_tab();
    assert!(tab.mux_session_name.is_none(), "pre-mux tab");
    // A complete Kitty image fed as a plain PTS buffer (pre-mux branch:
    // parsed by self.core, on_apc → pending_apc → image pipeline).
    let _ = tab.test_process_combined(kitty_rgb_single(3, 3));
    let events = tab.drain_image_events();
    assert!(
        has_image_ready(&events),
        "non-mux Kitty image must decode as before; events={events:?}"
    );
}

// ── TS-5: pre-mux PTS bytes route through self.core ───────────────────
#[test]
fn ts5_pre_mux_pts_routes_through_core() {
    let mut tab = test_tab();
    assert!(tab.mux_session_name.is_none(), "extractor not engaged yet");
    // Plain printable bytes fed as the outer PTS stream: pre-mux they must
    // be parsed by self.core and land on the grid (the extractor would
    // discard non-transport Print actions).
    tab.test_process_combined(b"pre-mux line\r\n".to_vec());
    assert!(
        tab.test_grid_text().contains("pre-mux line"),
        "pre-mux plain text must render via self.core"
    );
}

#[test]
fn ts5_switch_to_extractor_after_welcome_discards_outer_print() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some(), "mux established");
    // After the switch, raw printable bytes on the OUTER stream are not
    // content — they are not valid mux transport, so the extractor drops
    // them and they never reach self.core / the grid.
    tab.test_process_combined(b"outer-noise-xyz\r\n".to_vec());
    assert!(
        !tab.test_grid_text().contains("outer-noise-xyz"),
        "outer-stream Print must NOT reach the core once mux is established"
    );
}

// ── TS-6: detach restores self.core routing ───────────────────────────
#[test]
fn ts6_detach_restores_core_routing() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some());
    // Detach: the daemon confirms with a Detached frame delivered as an
    // outer PtyOutput-equivalent control message. Apply it directly.
    let detached = MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    };
    tab.apply_mux_message(detached);
    assert!(tab.mux_session_name.is_none(), "detached clears mux");
    // Pre-mux routing resumed: plain PTS bytes are parsed by self.core
    // again and render on the grid.
    tab.test_process_combined(b"post-detach line\r\n".to_vec());
    assert!(
        tab.test_grid_text().contains("post-detach line"),
        "after detach, plain text must render via self.core again"
    );
}

#[test]
fn ts6_detach_resets_extractor_partial_frame() {
    // A partial outer frame in flight when detach happens must be dropped,
    // not carried into the resumed pre-mux core parse.
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    // Feed half of an outer APC frame — the extractor is now mid-sequence.
    let half = pty_output_apc(0, b"GG");
    let split = half.len() / 2;
    tab.test_process_combined(half[..split].to_vec());
    // Detach (resets the extractor).
    tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    // The remainder, now fed pre-mux to self.core, is the tail of an APC
    // sequence with no introducer: self.core stays in Ground for the
    // trailing ST and prints nothing garbled. Then a clean line renders.
    tab.test_process_combined(half[split..].to_vec());
    tab.test_process_combined(b"clean\r\n".to_vec());
    assert!(
        tab.test_grid_text().contains("clean"),
        "post-detach core parse is clean after extractor reset"
    );
}

// ── TS-7: double-Welcome does not corrupt the stream ──────────────────
#[test]
fn ts7_double_welcome_does_not_corrupt_decoding() {
    let mut tab = test_tab();
    // The bridge/daemon can deliver Welcome twice (a known duplication).
    tab.apply_mux_message(welcome_msg(&[], 0));
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some(), "mux still established");

    // A split inner Kitty image after the double Welcome must still
    // assemble into one image — the extractor state stayed consistent.
    let chunks = kitty_rgb_chunked(4, 4, 3);
    tab.test_process_combined(pty_output_apc(0, &chunks[0]));
    tab.test_process_combined(pty_output_apc(0, &chunks[1]));
    tab.test_process_combined(pty_output_apc(0, &chunks[2]));
    let events = tab.drain_image_events();
    assert!(
        has_image_ready(&events),
        "image must decode despite double Welcome; events={events:?}"
    );
}

// ── TS-11: post-Detached tail re-routed to self.core (FR5) ────────────
#[test]
fn ts11_post_detached_tail_in_coalesced_buffer_renders_via_core() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some(), "mux established");

    // One coalesced PTS buffer carrying, in order:
    //   1. an inner PtyOutput frame (rendered into self.core via the inner
    //      content path),
    //   2. the Detached control frame (clears mux_session_name mid-buffer),
    //   3. plain shell prompt bytes printed by the shell that regained the
    //      PTY — these follow the Detached frame in the SAME buffer.
    //
    // Before the fix, routing was decided once per pump: the whole buffer
    // went to the extractor, which discards non-APC bytes, so the prompt
    // bytes were silently dropped and (with the Detached grid clear) the
    // screen stayed blank until the next keystroke.
    let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    let mut combined = pty_output_apc(0, b"inner shell output\r\n");
    combined.extend_from_slice(&detached);
    combined.extend_from_slice(b"detached-prompt$ \r\n");

    let _ = tab.test_process_combined(combined);

    // Detach actually took effect.
    assert!(
        tab.mux_session_name.is_none(),
        "Detached frame must clear mux_session_name"
    );
    // The plain prompt bytes coalesced behind the Detached frame rendered
    // via self.core instead of being swallowed by the extractor. The
    // Detached arm clears the grid first (reset_frame_for_replay), so the
    // re-routed tail is what repaints — exactly the bytes we expect.
    let grid = tab.test_grid_text();
    assert!(
        grid.contains("detached-prompt$"),
        "post-Detached shell bytes must render via self.core; grid={grid:?}"
    );
    // The transport prefix must never leak onto the grid.
    assert!(
        !grid.contains("emterm-mux"),
        "outer transport prefix must not leak to the grid; grid={grid:?}"
    );
}

// ── TS-11b: a non-mux image coalesced behind Detached decodes exactly once ──
#[test]
fn ts11_post_detached_image_decodes_exactly_once() {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[], 0));
    assert!(tab.mux_session_name.is_some(), "mux established");

    // One coalesced buffer: the Detached control frame, then a complete
    // (non-mux) Kitty image the shell printed right after regaining the PTY.
    // `feed_with_offsets` surfaces the bare Kitty APC AND the post-detach
    // tail re-route re-parses the same bytes through self.core. Without the
    // loop `break` at the detach boundary, the image was decoded twice (once
    // from the extracted image_apc, once from the re-routed tail).
    let detached = crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    });
    let mut combined = detached;
    combined.extend_from_slice(&kitty_rgb_single(3, 3));

    let _ = tab.test_process_combined(combined);

    assert!(
        tab.mux_session_name.is_none(),
        "Detached frame must clear mux_session_name"
    );
    let ready = tab
        .drain_image_events()
        .into_iter()
        .filter(|e| matches!(e, ImageEvent::ImageReady { .. }))
        .count();
    assert_eq!(
        ready, 1,
        "post-Detached image must decode exactly once, not double-processed \
         via the extracted-frame loop AND the tail re-route"
    );
}

// ── (C) client-side coalesce contract: consecutive PtyOutput ⇒ one parse ──
//
// The client coalesces, in `process_combined`, the inner payloads of
// consecutive active-pane `PtyOutput` frames that arrive within one pump:
// they are concatenated and parsed by `core.process_pty_data_fully` exactly
// ONCE per consecutive run, instead of once per frame. A control message,
// a non-active pane, a `pending_switch`, or a detach is a boundary that
// flushes the accumulator first; the buffer is also flushed at loop end.
//
// These tests observe the pass count directly through the `cfg(test)`-only
// `coalesce_parse_passes` counter (incremented at the flush parse site),
// which carries no taint in the production build. The grid is asserted to
// equal the single-concatenated result so the collapse is proven to be a
// pure performance change — the same equality the "split == concatenated"
// parity test pins as the before/after baseline.

/// AC-3/TS2 (mux-status-bar-removal task0001, FR1/FR8a): a raw frame
/// carrying the retired opcode 0x16 (see `mux_ipc::protocol`'s
/// reserved-opcode comment for what it used to mean, reserved and
/// never reused) arriving on the GUI's mux receive path is ignored
/// with at most a warn log — no error, no disconnect, no tab-state
/// mutation. Constructed as a raw wire frame (`[type=0x16][pane_id:
/// u32 LE][empty payload]`, wrapped exactly as the daemon/bridge
/// write it: `ESC _ emterm-mux;<base64> ESC \`) rather than through
/// the typed `MuxMessage` API, which can no longer name the retired
/// type — this keeps the test valid regardless of whether that type
/// still exists anywhere in the tree. Replaces (former app.rs
/// TS-mux-msg-2) `on_mux_message_status_update_caches_payload_on_tab`.
#[test]
fn retired_status_update_opcode_is_ignored_by_gui_receive_path() {
    let mut tab = mux_tab_active_pane(10);
    let before_session = tab.mux_session_name.clone();
    let before_pane_ids = tab.mux_group.as_ref().map(|g| g.pane_ids().to_vec());
    let before_active_pane = tab.mux_group.as_ref().and_then(|g| g.active_pane_id());

    let retired_frame_body: Vec<u8> = vec![0x16, 0, 0, 0, 0]; // [type][pane_id LE]
    let mut raw = vec![0x1b, b'_'];
    raw.extend_from_slice(format!("emterm-mux;{}", b64(&retired_frame_body)).as_bytes());
    raw.extend_from_slice(&[0x1b, b'\\']);

    // Must not panic.
    tab.test_process_combined(raw);

    assert_eq!(
        tab.mux_session_name, before_session,
        "mux session must be undisturbed by a retired-opcode frame"
    );
    assert_eq!(
        tab.mux_group.as_ref().map(|g| g.pane_ids().to_vec()),
        before_pane_ids,
        "mux window group must be undisturbed"
    );
    assert_eq!(
        tab.mux_group.as_ref().and_then(|g| g.active_pane_id()),
        before_active_pane,
        "active pane must be undisturbed"
    );

    // Connection stays up: ordinary traffic immediately afterward still
    // applies normally.
    let follow_up = pty_output_apc(10, b"still alive");
    let changed = tab.test_process_combined(follow_up);
    assert!(
        changed,
        "tab must keep processing ordinary frames after a retired-opcode frame"
    );
}

/// The batched (coalesce) behavior: K active-pane `PtyOutput` frames
/// arriving wire-encoded in ONE coalesced PTS buffer collapse into a single
/// parse pass. Every line still lands in the grid (output is unchanged), but
/// the core is parsed once for the whole consecutive run — not once per
/// frame. The `coalesce_parse_passes` counter makes the collapse observable.
/// This is the post-change contract the perf work establishes (previously
/// the per-frame path parsed K times).
#[test]
fn c_pty_output_parsed_per_message_grid_grows_step_by_step() {
    let mut tab = mux_tab_active_pane(10);

    // K active-pane PtyOutput frames, each a full line, encoded as the
    // daemon writes them and concatenated into ONE coalesced PTS buffer —
    // exactly what `pump` hands `process_combined` when many small frames
    // arrive within one drain.
    let lines: [&[u8]; 4] = [b"line0\r\n", b"line1\r\n", b"line2\r\n", b"line3\r\n"];
    let k = lines.len();
    let mut combined = Vec::new();
    for line in lines {
        combined.extend_from_slice(&pty_output_apc(10, line));
    }

    let before = tab.test_coalesce_parse_passes();
    let changed = tab.test_process_combined(combined);
    assert!(changed, "applied PtyOutput repaints the active pane");

    // One consecutive active-pane run ⇒ exactly one flush/parse, not K.
    assert_eq!(
        tab.test_coalesce_parse_passes() - before,
        1,
        "K={k} consecutive active-pane frames must coalesce into 1 parse pass"
    );
    // All K lines still landed — output is byte-for-byte unchanged.
    for (i, _) in lines.iter().enumerate() {
        assert_eq!(
            tab.test_row_text(i as u16),
            format!("line{i}"),
            "row {i} must show its line after the coalesced parse"
        );
    }
}

/// New required test (TS-1): consecutive active-pane `PtyOutput` frames
/// arriving in one coalesced buffer are parsed in a SINGLE pass, and the
/// resulting grid is identical to parsing the concatenation of their inner
/// payloads in one shot. Proves the coalesce both collapses the parse count
/// to 1 and preserves output exactly.
#[test]
fn c_consecutive_active_pane_pty_output_coalesces_into_one_parse() {
    let pane = 10;
    // Inner payloads whose chunk boundaries deliberately fall inside lines
    // and after newlines, so the streaming parser must carry state across
    // the frame boundaries (a per-frame parse and a coalesced parse would
    // otherwise be trivially identical).
    let inner: [&[u8]; 4] = [b"alp", b"ha\r\nbra", b"vo\r\ncharlie\r\n", b"delta"];

    // Coalesced path: K active-pane frames in ONE buffer ⇒ 1 parse pass.
    let mut tab = mux_tab_active_pane(pane);
    let mut combined = Vec::new();
    for chunk in inner {
        combined.extend_from_slice(&pty_output_apc(pane, chunk));
    }
    let before = tab.test_coalesce_parse_passes();
    tab.test_process_combined(combined);
    assert_eq!(
        tab.test_coalesce_parse_passes() - before,
        1,
        "consecutive active-pane PtyOutput run must parse exactly once"
    );

    // Reference: a single PtyOutput whose payload is the concatenation.
    let mut single = mux_tab_active_pane(pane);
    single.test_process_combined(pty_output_apc(pane, &inner.concat()));

    assert_eq!(
        tab.test_grid_text(),
        single.test_grid_text(),
        "coalesced grid must equal the single-concatenated parse"
    );
}

/// Parity baseline for a future coalescing change: K split `PtyOutput`
/// messages and a single concatenated `PtyOutput` message produce the
/// identical final grid. A coalescing optimization (parse the K payloads in
/// one pass) must keep this equality — so this is the correctness contract
/// that lets parse-count be reduced from K to 1 without changing output.
#[test]
fn c_split_messages_equal_single_concatenated_message() {
    let total = b"alpha\r\nbravo\r\ncharlie\r\ndelta";
    // The four chunk boundaries deliberately fall *inside* lines and after
    // newlines, proving term_core's streaming parser carries state across
    // message boundaries (so coalescing is purely a perf change).
    let chunks: [&[u8]; 4] = [b"alp", b"ha\r\nbra", b"vo\r\ncharlie\r\n", b"delta"];
    assert_eq!(
        chunks.concat(),
        total,
        "chunks must reconstruct the whole stream"
    );

    // K-message path: one parse pass per message (current behavior).
    let mut split = mux_tab_active_pane(10);
    let k = chunks.len();
    for chunk in chunks {
        split.apply_mux_message(pty_output(10, chunk.to_vec()));
    }

    // Single-message path: one parse pass for the whole stream (the shape a
    // receive-side coalesce would collapse the K messages into).
    let mut single = mux_tab_active_pane(10);
    single.apply_mux_message(pty_output(10, total.to_vec()));

    assert_eq!(
        split.test_grid_text(),
        single.test_grid_text(),
        "K={k} per-message parses must yield the same grid as 1 concatenated parse"
    );
}

/// `payload_has_device_query` detects complete CSI sequences whose final
/// byte produces a device response in `term_core` — `n` (DSR), `c` (DA),
/// `t` (XTWINOPS size reports), `p` (DECRPM) — across params / intermediates
/// and DEC private (`?`) / secondary (`>`) forms, resynchronizes on a
/// malformed CSI, and rejects non-response finals, incomplete CSIs, and
/// plain text.
#[test]
fn payload_has_device_query_detects_response_producing_finals() {
    assert!(payload_has_device_query(b"\x1b[6n"), "CPR DSR");
    assert!(payload_has_device_query(b"\x1b[5n"), "status DSR");
    assert!(
        payload_has_device_query(b"\x1b[c"),
        "primary DA (no params)"
    );
    assert!(payload_has_device_query(b"\x1b[>0c"), "secondary DA");
    assert!(payload_has_device_query(b"\x1b[?6n"), "DEC private DSR");
    assert!(
        payload_has_device_query(b"\x1b[14t"),
        "XTWINOPS size report (t)"
    );
    assert!(
        payload_has_device_query(b"\x1b[?2026$p"),
        "DECRPM synchronized-output probe (p)"
    );
    assert!(
        payload_has_device_query(b"hello\x1b[6nworld"),
        "query embedded in surrounding text"
    );
    assert!(
        payload_has_device_query(b"\x1b[2\x1b[6n"),
        "aborted CSI then a real query must resync and detect the query"
    );
    assert!(
        payload_has_device_query(b"\x1b[\x076n"),
        "a C0 control mid-CSI does not abort the sequence (term_core keeps it alive)"
    );
    assert!(
        !payload_has_device_query(b"plain text\r\n"),
        "no CSI at all"
    );
    assert!(
        !payload_has_device_query(b"\x1b[1;2H"),
        "cursor position (final H) is not a query"
    );
    assert!(
        !payload_has_device_query(b"\x1b[0m"),
        "SGR (final m) is not a query"
    );
    assert!(
        !payload_has_device_query(b"\x1b[6"),
        "incomplete CSI is not a complete query"
    );
    assert!(
        !payload_has_device_query(b"\x1b[31mcn"),
        "literal c/n after a non-query CSI final must not count"
    );
}

/// Device-response parity (TS): a `PtyOutput` frame carrying a device query
/// (`\x1b[6n` CPR) must NOT be coalesced — it is parsed on its own via the
/// per-frame path so its reply is captured before a later query overwrites
/// `term_core`'s single-slot response buffer. Observable consequence: a
/// query frame BREAKS the consecutive active-pane run, so [text][query][text]
/// flushes the coalesce accumulator twice (leading run + loop-end) with the
/// query frame parsed per-frame in between — versus a single flush when no
/// query interrupts the run.
#[test]
fn c_device_query_frame_breaks_coalesce_run() {
    let pane = 10;

    // Baseline: three plain active-pane frames coalesce into ONE parse.
    let mut plain = mux_tab_active_pane(pane);
    let mut plain_buf = Vec::new();
    for chunk in [b"aaa\r\n".as_slice(), b"bbb\r\n", b"ccc\r\n"] {
        plain_buf.extend_from_slice(&pty_output_apc(pane, chunk));
    }
    let before = plain.test_coalesce_parse_passes();
    plain.test_process_combined(plain_buf);
    assert_eq!(
        plain.test_coalesce_parse_passes() - before,
        1,
        "three plain frames coalesce into a single parse"
    );

    // With a CPR query frame in the middle: the run splits. The query frame
    // is handled per-frame (not via the coalesce flush), so the accumulator
    // flushes for the leading run and again at loop end — two coalesce
    // parses — guaranteeing the query's reply is not clobbered by coalescing.
    let mut split = mux_tab_active_pane(pane);
    let mut split_buf = Vec::new();
    split_buf.extend_from_slice(&pty_output_apc(pane, b"aaa\r\n"));
    split_buf.extend_from_slice(&pty_output_apc(pane, b"\x1b[6n"));
    split_buf.extend_from_slice(&pty_output_apc(pane, b"ccc\r\n"));
    let before = split.test_coalesce_parse_passes();
    split.test_process_combined(split_buf);
    assert_eq!(
        split.test_coalesce_parse_passes() - before,
        2,
        "a device-query frame breaks the run: leading-run flush + loop-end flush"
    );
    // The query produced no visible cells; the surrounding text rendered.
    assert_eq!(split.test_row_text(0), "aaa");
    assert_eq!(split.test_row_text(1), "ccc");
}
