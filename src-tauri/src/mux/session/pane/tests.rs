use super::*;
use crate::agent_status::AgentState;
use tokio::sync::mpsc;

fn make_output_target() -> SharedOutputTarget {
    let (tx, _rx) = mpsc::channel(1);
    Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
}

/// Decode a `Snapshot`-kind chunk's wire-encoded bytes (task0004 round-4
/// rework D1', `mux_ipc::protocol::decode_snapshot_payload`) back into
/// plain content bytes, discarding the structural segment header — used
/// by tests that only care about the ANSI content layout.
fn decode_snapshot_content(data: &[u8]) -> Vec<u8> {
    mux_ipc::protocol::decode_snapshot_payload(data).1.to_vec()
}

// ── AgentStatus (SPEC FR3, task0003 AC-1/AC-2/AC-6) ──────────────────

#[test]
fn test_new_pane_has_no_agent_status_and_revision_zero() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, None);
    assert_eq!(status.name, None);
    assert_eq!(status.revision, 0);
}

/// AC-1: a Set event updates state/name and increments revision.
#[test]
fn test_apply_agent_status_event_set_updates_state_and_increments_revision() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);

    let revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Working,
        name: Some("claude".to_string()),
    });
    assert_eq!(revision, 1);

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, Some(AgentState::Working));
    assert_eq!(status.name.as_deref(), Some("claude"));
    assert_eq!(status.revision, 1);
}

/// AC-1: a Clear event empties state/name and increments revision.
#[test]
fn test_apply_agent_status_event_clear_empties_state_and_increments_revision() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Blocked,
        name: Some("agent".to_string()),
    });

    let revision = pane.apply_agent_status_event(AgentStatusEvent::Clear);
    assert_eq!(revision, 2);

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, None);
    assert_eq!(status.name, None);
    assert_eq!(status.revision, 2);
}

/// AC-2: a same-state re-report still increments revision (it is only
/// ever invoked for an ACCEPTED event; "same state" is not itself a
/// rejection reason).
#[test]
fn test_apply_agent_status_event_same_state_re_report_increments_revision() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    let r1 = pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Working,
        name: None,
    });
    let r2 = pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Working,
        name: None,
    });
    assert_eq!(r1, 1);
    assert_eq!(r2, 2);
}

/// AC-2: rejected sequences (parse returning `None`) never reach
/// `apply_agent_status_event`, so state/revision are naturally
/// untouched. This test pins that contract at the call-site level: a
/// caller that only calls `apply_agent_status_event` for `Some(event)`
/// leaves state/revision alone when `agent_status::parse` rejects.
#[test]
fn test_rejected_parse_never_reaches_apply_leaves_state_untouched() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Idle,
        name: None,
    });

    // Simulate the caller's contract: a rejected report is never
    // applied.
    let rejected = crate::agent_status::parse("emterm;agent-status;v=1;state=bogus");
    assert_eq!(rejected, None);
    if let Some(event) = rejected {
        pane.apply_agent_status_event(event);
    }

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, Some(AgentState::Idle));
    assert_eq!(status.revision, 1);
}

/// AC-6: pane destroy discards agent-status state — `MuxWindow::remove_pane`
/// drops the `MuxPane` (and its `Arc<Mutex<AgentStatus>>`) entirely, so a
/// removed pane's status is gone, not merely reset.
#[test]
fn test_pane_removal_discards_agent_status() {
    use super::super::window::MuxWindow;

    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Done,
        name: Some("agent".to_string()),
    });
    let status_handle = pane.agent_status.clone();
    assert_eq!(Arc::strong_count(&status_handle), 2, "pane + our clone");

    let mut window = MuxWindow::new(1, "w".to_string());
    window.add_pane(pane);
    let removed = window.remove_pane(1);
    assert!(removed.is_some());
    drop(removed);

    // The pane (and its only other Arc handle to `agent_status`) is
    // gone; only our test-held clone remains.
    assert_eq!(
        Arc::strong_count(&status_handle),
        1,
        "agent_status must be discarded along with the destroyed pane"
    );
}

// ── task0003: inferred-clear latch wiring (SPEC FR1/FR2/FR3) ─────────

/// AC-7: a freshly created pane's inferred-clear latch is present and
/// disarmed (mirrors "new pane has no agent-status", the sibling field
/// this one is shaped after).
#[test]
fn test_new_pane_has_disarmed_latch() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    assert_eq!(
        *pane.agent_status_exit_latch.lock().unwrap(),
        AgentStatusExitLatch::new()
    );
}

/// AC-1 (pane-level): `Set` then live `D` then live `A` fires the
/// inferred clear through `apply_agent_status_event`'s exact effects —
/// state becomes `None` and the revision increments exactly once more.
#[test]
fn test_record_live_osc133_mark_set_then_d_then_a_fires_inferred_clear() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    let set_revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Working,
        name: Some("claude".to_string()),
    });
    assert_eq!(set_revision, 1);

    let d_result = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
    assert_eq!(d_result, None, "a lone D must not fire a clear");

    let a_result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert_eq!(a_result, Some(2), "D followed by A must fire exactly once");

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, None);
    assert_eq!(status.name, None);
    assert_eq!(status.revision, 2);
}

/// AC-2 (pane-level): `Set` followed only by live `A` (no `D`) leaves
/// state unchanged — no inferred clear, no revision bump.
#[test]
fn test_record_live_osc133_mark_a_without_prior_d_is_a_noop() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Blocked,
        name: None,
    });

    let result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert_eq!(result, None);

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, Some(AgentState::Blocked));
    assert_eq!(status.revision, 1);
}

/// AC-3 (pane-level): an explicit `Clear` disarms the latch, so a
/// subsequent live `D`/`A` pair does not produce a second/duplicate
/// clear or a second revision increment.
#[test]
fn test_record_live_osc133_mark_after_explicit_clear_is_a_noop() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Done,
        name: None,
    });
    let clear_revision = pane.apply_agent_status_event(AgentStatusEvent::Clear);
    assert_eq!(clear_revision, 2);

    let d_result = pane.record_live_osc133_mark(PromptMarkKind::CommandEnd);
    assert_eq!(d_result, None);
    let a_result = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
    assert_eq!(a_result, None);

    let status = pane.agent_status.lock().unwrap();
    assert_eq!(status.state, None);
    assert_eq!(status.revision, 2, "no third revision from D/A after Clear");
}

/// AC-4: OSC 133 marks captured on scrollback content REPLAYED for a
/// reattach/visibility-resume snapshot (`resume_pane_with_permit`, the
/// real production snapshot-construction path — not a hand-rolled
/// substitute) never drive the latch. Even with a full `Set` -> `D`
/// -> `A` byte sequence sitting in scrollback, building and sending a
/// resume snapshot from it must leave `agent_status` exactly as the
/// explicit report left it.
#[tokio::test]
async fn test_resume_snapshot_construction_with_osc133_bytes_in_scrollback_never_fires_latch() {
    let (owned_tx, _rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(9, 80, 24, target.clone());

    // A full Set -> D -> A OSC 133 byte sequence, literally present in
    // scrollback content (as it would be after a real shell session) —
    // nothing strips OSC 133 bytes from scrollback (it is not a
    // viewer-launch sequence), so this is exactly what a replay would
    // carry.
    pane.scrollback
        .lock()
        .unwrap()
        .write(b"$ claude\r\n\x1b]133;D\x07\x1b]133;A\x07$ ");
    let set_revision = pane.apply_agent_status_event(AgentStatusEvent::Set {
        state: AgentState::Working,
        name: Some("claude".to_string()),
    });
    assert_eq!(set_revision, 1);

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    // The real snapshot-construction path ran (and — for a sanity
    // check that this test actually exercised the D/A bytes — the
    // scrollback content in fact contains them), yet the latch/state
    // must be untouched by it.
    let status = pane.agent_status.lock().unwrap();
    assert_eq!(
        status.state,
        Some(AgentState::Working),
        "snapshot/replay construction must never fire the inferred-clear latch"
    );
    assert_eq!(
        status.revision, 1,
        "no extra revision from snapshot assembly"
    );
    assert_eq!(
        *pane.agent_status_exit_latch.lock().unwrap(),
        {
            let mut expected = AgentStatusExitLatch::new();
            expected.record_set();
            expected
        },
        "the latch must still be exactly what the explicit Set left it as"
    );
}

// ── task0004 round-4 rework (review round-3 finding `b546481e9c2fcc85`):
// pane creation validates dims against the same domain resize() uses ──

/// AC-6: `MuxPane::new` clamps out-of-domain dimensions through the
/// SAME path `resize()` uses (`clamp_dims_to_wire_domain`), instead of
/// storing the caller's raw values unvalidated. Uses a real PTY (like
/// the existing `test_new_pane_records_initial_dims_marker_in_scrollback`)
/// since the test-only `new_test`/`new_test_with_writer` constructors
/// are a separate, simplified path that does not call `MuxPane::new`
/// at all.
///
/// Confirmed to fail pre-fix: before this change, `MuxPane::new` stored
/// `cols`/`rows` directly (no clamp call at all), so passing `(0, 0)`
/// left `pane.cols == 0` — outside `clamp_resize_dims`'s `1..=4096`
/// domain that this task's replay path assumes dimensions never
/// violate.
///
/// D6'''' (round-7 rework, review round-6 finding `6cefb1dd16c126b6`):
/// `u16::MAX` per axis clamps to `RESIZE_MARKER_MAX_COLS` /
/// `RESIZE_MARKER_MAX_ROWS` (4096 each) — a product of 16,777,216,
/// still far above the wire decoder's per-segment ceiling. `rows` must
/// clamp FURTHER, preserving `cols` at the per-axis max.
///
/// D4''''' (round-8 rework, review round-7 finding `4bc6ab813edd6d22`):
/// the product ceiling this now clamps to is
/// `PRODUCER_SEGMENT_CELL_BUDGET` (derived from the decoder's
/// CUMULATIVE budget), not the decoder's raw per-segment
/// `MAX_SEGMENT_CELLS` (1,000,000) — see that constant's doc.
#[cfg(unix)]
#[test]
fn new_pane_clamps_out_of_domain_dimensions() {
    let pty_system = portable_pty::native_pty_system();
    // `portable_pty` itself may reject a literal 0x0 openpty size on
    // some platforms, so this drives the clamp with an OVERSIZED value
    // instead (still out of `clamp_resize_dims`'s domain) to keep the
    // PTY open call itself valid.
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new(1, u16::MAX, u16::MAX, target, writer, pair.master, None);
    let expected_cols = term_core::terminal_core::RESIZE_MARKER_MAX_COLS;
    let expected_rows = (PRODUCER_SEGMENT_CELL_BUDGET / expected_cols as u32) as u16;
    assert_eq!(
        (pane.cols, pane.rows),
        (expected_cols, expected_rows),
        "oversized dimensions must clamp down to the wire domain \
         (per-axis max, THEN product ceiling), matching \
         clamp_dims_to_wire_domain"
    );
    assert!(
        (pane.cols as u32) * (pane.rows as u32) <= mux_ipc::protocol::MAX_SEGMENT_CELLS,
        "clamped dims must never exceed MAX_SEGMENT_CELLS as a product"
    );
    // The clamped dims are ALSO what gets recorded structurally (the
    // initial segment `MuxPane::new` writes) — not the caller's raw,
    // out-of-domain values.
    let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert_eq!(segments, vec![(0usize, expected_cols, expected_rows)]);
    // AC-5 (D3''''', round-8 rework, review round-7 finding
    // `1d1b6b6297e3b6a0`): the PTY itself was opened at (80, 24) — a
    // DIFFERENT size than the clamped dims recorded above. `MuxPane::new`
    // must resize the ACTUAL PTY to match what it records, not leave it
    // at whatever size the caller happened to open it at.
    //
    // Confirmed to fail pre-fix: before this change, `MuxPane::new`
    // never resized `master` at all, so `master_size()` would still
    // report the PTY's ORIGINAL open size (80, 24) — disagreeing with
    // the (4096, 244) this test records above.
    let actual = pane
        .master_size()
        .expect("PTY master must still be present");
    assert_eq!(
        (actual.cols, actual.rows),
        (expected_cols, expected_rows),
        "MuxPane::new must resize the underlying PTY to the CLAMPED \
         dims it records, not leave it at whatever size the caller \
         originally opened it at"
    );
}

/// AC-8 (D6'''', round-7 rework, review round-6 finding
/// `6cefb1dd16c126b6`): dimensions the daemon accepts always produce a
/// snapshot segment the wire decoder ALSO accepts — round-trips
/// `clamp_dims_to_wire_domain`'s output through the REAL
/// `mux_ipc::protocol` encode/decode path, not just an inline product
/// check, so a drift between the two crates' notions of "in domain"
/// would surface here even if a future change duplicated the ceiling
/// incorrectly instead of sharing `MAX_SEGMENT_CELLS`.
///
/// Confirmed to fail pre-fix: before `clamp_dims_to_wire_domain`
/// existed, `clamp_resize_dims(2000, 600)` returned `(2000, 600)`
/// unchanged (both axes are within `1..=4096`) — a product of
/// 1,200,000, which `mux_ipc::protocol`'s segment decoder rejects as
/// `Malformed` (`> MAX_SEGMENT_CELLS`). This test's "round-trips
/// cleanly" assertion would fail against that dimension pair.
#[test]
fn clamp_dims_to_wire_domain_output_always_decodes_cleanly() {
    for (raw_cols, raw_rows) in [
        (2000u16, 600u16), // in-per-axis-domain, over-product (the finding's exact repro)
        (4096, 4096),      // both axes at the per-axis max
        (u16::MAX, u16::MAX),
        (1, 1),
        (80, 24),
    ] {
        let (cols, rows) = clamp_dims_to_wire_domain(raw_cols, raw_rows);
        assert!(
            (cols as u32) * (rows as u32) <= mux_ipc::protocol::MAX_SEGMENT_CELLS,
            "clamp_dims_to_wire_domain({raw_cols}, {raw_rows}) = \
             ({cols}, {rows}) still exceeds MAX_SEGMENT_CELLS as a product"
        );
        // Round-trip through the REAL wire encode/decode, mirroring
        // what a snapshot carrying this pane's initial segment does.
        let segments = [mux_ipc::protocol::DimSegment {
            offset: 0,
            cols,
            rows,
        }];
        let payload = mux_ipc::protocol::encode_snapshot_payload(&segments, b"x");
        let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&payload);
        assert!(
            matches!(
                decoded,
                mux_ipc::protocol::DecodedSnapshotPayload::Structured { .. }
            ),
            "clamp_dims_to_wire_domain({raw_cols}, {raw_rows}) = \
             ({cols}, {rows}) produced a segment the wire decoder \
             rejected as Malformed: {decoded:?}"
        );
    }
}

/// AC-6 (D4''''', round-8 rework, review round-7 finding
/// `4bc6ab813edd6d22`, independently confirmed by `codex:architecture`):
/// the LARGEST segment list the daemon can actually produce — every one
/// of `MAX_DAEMON_SNAPSHOT_SEGMENTS` segments at the producer's own
/// per-segment cell budget — decodes successfully, not `Malformed`.
/// This test builds the segment LIST from the constants themselves
/// (a structural/tautological check); `largest_real_producer_segment_
/// list_round_trips_cleanly` below drives the REAL ring → snapshot →
/// encode → decode path instead, so a future drift between these
/// constants and what the producer actually emits is still caught
/// (review round-8 finding `45033eaafbdf8e25`, AC-7).
///
/// Confirmed to fail pre-fix: before D4''''' existed,
/// `clamp_dims_to_wire_domain` bounded every segment to the decoder's
/// PER-SEGMENT ceiling alone (`MAX_SEGMENT_CELLS`, 1,000,000) — a full
/// `MAX_DAEMON_SNAPSHOT_SEGMENTS`-segment list at that size sums to far
/// more than `MAX_CUMULATIVE_SEGMENT_CELLS`, so
/// `decode_snapshot_payload_typed` would return `Malformed` for this
/// exact payload and the assertion below would fail.
#[test]
fn largest_daemon_producible_segment_list_round_trips_cleanly() {
    let (cols, rows) = clamp_dims_to_wire_domain(u16::MAX, u16::MAX);
    let segment_count = MAX_DAEMON_SNAPSHOT_SEGMENTS as usize;
    let segments: Vec<mux_ipc::protocol::DimSegment> = (0..segment_count)
        .map(|i| mux_ipc::protocol::DimSegment {
            offset: i as u32,
            cols,
            rows,
        })
        .collect();
    let content = vec![b'x'; segment_count];
    let payload = mux_ipc::protocol::encode_snapshot_payload(&segments, &content);
    let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&payload);
    match decoded {
        mux_ipc::protocol::DecodedSnapshotPayload::Structured {
            segments: decoded_segments,
            ..
        } => {
            assert_eq!(decoded_segments.len(), segment_count);
        }
        other => panic!(
            "the largest segment list the daemon can produce \
             ({segment_count} segments at {cols}x{rows}) must decode as \
             Structured, not {other:?}"
        ),
    }
}

/// AC-7, D5'''''' (round-9 rework, review round-8 finding
/// `45033eaafbdf8e25`): drives the REAL producer path — a real
/// `ScrollbackRingBuffer` → `read_segments` → `build_snapshot_bytes` →
/// `encode_snapshot_segments` → `decode_snapshot_payload_typed` — at
/// the LARGEST shape the daemon can actually produce (the cap
/// saturated with exactly one eviction, so `read_segments` synthesizes
/// a head segment, plus a trailing alt-screen segment), instead of
/// `largest_daemon_producible_segment_list_round_trips_cleanly`'s
/// structural check, which builds its segment list from
/// `MAX_DAEMON_SNAPSHOT_SEGMENTS`/`PRODUCER_SEGMENT_CELL_BUDGET`
/// themselves and so cannot detect either constant drifting from what
/// the real producer emits.
///
/// Confirmed to fail pre-fix: reverting
/// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` to its pre-
/// round-9 value (8,000,000) while leaving `MAX_DIM_MARKERS` at 62
/// (`MAX_DAEMON_SNAPSHOT_SEGMENTS` == 64) derives a per-segment budget
/// of 125,000 — the `assert_eq!` on `(cols, rows)` below (asserting
/// this test's 700×700 == 490,000-cell shape survives
/// `clamp_dims_to_wire_domain` UNCLAMPED) fails first, surfacing the
/// drift instead of masking it behind a silently-smaller recorded
/// size.
#[test]
fn largest_real_producer_segment_list_round_trips_cleanly() {
    let (cols, rows) = clamp_dims_to_wire_domain(700, 700);
    assert_eq!(
        (cols, rows),
        (700, 700),
        "test prerequisite: this shape must fit PRODUCER_SEGMENT_CELL_BUDGET \
         unclamped, or this test no longer drives the LARGEST real shape \
         the producer can emit"
    );

    // Saturate `dim_markers` with exactly ONE cap eviction: MAX_DIM_MARKERS
    // + 1 real resize markers, each separated by real content so none
    // coalesce (`write_resize_marker` only coalesces when the offset is
    // UNCHANGED since the last entry).
    let content_per_step: &[u8] = b"real-producer-step;";
    let step_count = MAX_DIM_MARKERS + 1;
    let capacity = step_count * content_per_step.len() + 4096;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    for _ in 0..step_count {
        rb.write_resize_marker(cols, rows);
        rb.write(content_per_step);
    }
    let (raw, segments) = rb.read_segments();
    assert_eq!(
        segments.len(),
        MAX_DIM_MARKERS + 1,
        "test prerequisite: exactly one cap eviction must synthesize the \
         head segment (D1''''')"
    );

    // Trailing alt-screen dump segment (D7''): non-empty `screen` plus a
    // non-empty `scrollback_segments` appends one more segment at
    // `current_dims`, reaching the daemon's true maximum.
    let screen = vec![b'S'; 100];
    let (payload_bytes, snapshot_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
        &raw,
        &segments,
        &screen,
        true,
        (cols, rows),
    );
    assert_eq!(
        snapshot_segments.len(),
        MAX_DAEMON_SNAPSHOT_SEGMENTS as usize,
        "test prerequisite: the trailing alt-screen segment must be \
         present, reaching MAX_DAEMON_SNAPSHOT_SEGMENTS"
    );

    let wire_payload = encode_snapshot_segments(&payload_bytes, &snapshot_segments);
    let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&wire_payload);
    match decoded {
        mux_ipc::protocol::DecodedSnapshotPayload::Structured {
            segments: decoded_segments,
            ..
        } => {
            assert_eq!(
                decoded_segments.len(),
                MAX_DAEMON_SNAPSHOT_SEGMENTS as usize
            );
        }
        other => panic!(
            "the largest segment list the REAL producer path emits \
             ({} segments at {cols}x{rows}) must decode as Structured, \
             not {other:?}",
            MAX_DAEMON_SNAPSHOT_SEGMENTS
        ),
    }
}

/// TS-10 (AC-7(a), mux-snapshot-ring-wrap-restore task0001): the same
/// shape as [`largest_real_producer_segment_list_round_trips_cleanly`],
/// but through the WRAP-AWARE builder for a wrapped MAIN-BUFFER pane
/// (`alt_screen = false`, `ring_wrapped = true`) instead of the alt-screen
/// branch. The wrap-restore trailing dump segment shares the same wire-
/// budget slot as the alt-screen trailing segment (`scrollback_buffer.rs`'s
/// `MAX_DIM_MARKERS` doc) — this pins that the daemon's largest
/// wrap-restore producible shape (the cap saturated with exactly one
/// eviction, plus the trailing dump segment) also saturates at exactly
/// `MAX_DAEMON_SNAPSHOT_SEGMENTS` without exceeding the wire decoder's
/// `MAX_SEGMENTS`.
#[test]
fn largest_real_wrap_restore_producer_segment_list_round_trips_cleanly() {
    let (cols, rows) = clamp_dims_to_wire_domain(700, 700);
    assert_eq!(
        (cols, rows),
        (700, 700),
        "test prerequisite: this shape must fit PRODUCER_SEGMENT_CELL_BUDGET \
         unclamped, or this test no longer drives the LARGEST real shape \
         the producer can emit"
    );

    let content_per_step: &[u8] = b"real-producer-step;";
    let step_count = MAX_DIM_MARKERS + 1;
    // Same generous headroom as the sibling test — deliberately NOT
    // pushing the ring's own byte window into a real wrap, so the
    // dim_markers count-cap eviction stays the sole, isolated eviction
    // mechanism in play (D1''''': combining it with a real byte-level wrap
    // shifts `oldest_offset` and folds additional surviving markers into
    // the single synthesized head segment instead of leaving them as
    // distinct `mid` entries, which would change the segment COUNT this
    // test pins for reasons unrelated to what it is testing). `ring_wrapped`
    // is instead passed directly to the wrap-aware builder below,
    // independent of this ring's own (accurately reported) non-wrapped
    // state — this test is about the wrap-aware builder's wire-budget
    // saturation, not a second exercise of `read_segments_with_wrap_state`
    // (already covered exhaustively by the AC-2 `scrollback_buffer` tests).
    let capacity = step_count * content_per_step.len() + 4096;
    let mut rb = ScrollbackRingBuffer::new(capacity);
    for _ in 0..step_count {
        rb.write_resize_marker(cols, rows);
        rb.write(content_per_step);
    }
    let (raw, segments) = rb.read_segments();
    assert_eq!(
        segments.len(),
        MAX_DIM_MARKERS + 1,
        "test prerequisite: exactly one cap eviction must synthesize the \
         head segment (D1''''')"
    );
    let ring_wrapped = true;

    // Trailing wrap-restore dump segment (D2): non-empty `screen` (the
    // shadow parser's dump, standing in here as an opaque byte string
    // since only the segment COUNT is under test) plus a non-empty
    // `scrollback_segments` appends one more segment at `current_dims`,
    // reaching the daemon's true maximum — same slot the alt-screen
    // trailing segment would otherwise occupy.
    let screen = vec![b'S'; 100];
    let (payload_bytes, snapshot_segments) =
        crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring(
            &raw,
            &segments,
            &screen,
            false,
            ring_wrapped,
            (cols, rows),
            10_000,
        );
    assert_eq!(
        snapshot_segments.len(),
        MAX_DAEMON_SNAPSHOT_SEGMENTS as usize,
        "test prerequisite: the trailing wrap-restore dump segment must be \
         present, reaching MAX_DAEMON_SNAPSHOT_SEGMENTS"
    );

    let wire_payload = encode_snapshot_segments(&payload_bytes, &snapshot_segments);
    let decoded = mux_ipc::protocol::decode_snapshot_payload_typed(&wire_payload);
    match decoded {
        mux_ipc::protocol::DecodedSnapshotPayload::Structured {
            segments: decoded_segments,
            ..
        } => {
            assert_eq!(
                decoded_segments.len(),
                MAX_DAEMON_SNAPSHOT_SEGMENTS as usize
            );
        }
        other => panic!(
            "the largest segment list the REAL wrap-restore producer path \
             emits ({} segments at {cols}x{rows}) must decode as \
             Structured, not {other:?}",
            MAX_DAEMON_SNAPSHOT_SEGMENTS
        ),
    }
}

/// AC-2 (round-9 rework, review round-8 finding `6082de4e619d7f51`):
/// raising `MAX_DIM_MARKERS` (and so `MAX_DAEMON_SNAPSHOT_SEGMENTS`)
/// must not shrink `PRODUCER_SEGMENT_CELL_BUDGET` underneath a REAL
/// large terminal size — not just avoid `Malformed` decodes for a
/// synthetic worst case. A large display at a small font
/// (`mux_ipc::protocol::MAX_SEGMENT_CELLS`'s own doc: "a few hundred
/// thousand cells") must fit unclamped.
///
/// Confirmed to fail pre-fix: reverting
/// `mux_ipc::protocol::MAX_CUMULATIVE_SEGMENT_CELLS` to its pre-
/// round-9 value (8,000,000) against the raised
/// `MAX_DAEMON_SNAPSHOT_SEGMENTS` (64) derives a 125,000-cell budget —
/// every shape below (all comfortably under the pre-round-9 307,692
/// budget, which real terminal sizes were already expected to fit
/// under) exceeds 125,000 and gets silently clamped, failing the
/// assertion.
#[test]
fn producer_segment_cell_budget_fits_a_real_large_terminal() {
    for (cols, rows) in [(400u16, 900u16), (700, 700), (1000, 500)] {
        let (clamped_cols, clamped_rows) = clamp_dims_to_wire_domain(cols, rows);
        assert_eq!(
            (clamped_cols, clamped_rows),
            (cols, rows),
            "a real large terminal size ({cols}x{rows}, {} cells) must \
             fit PRODUCER_SEGMENT_CELL_BUDGET ({PRODUCER_SEGMENT_CELL_BUDGET}) \
             without being clamped down",
            cols as u32 * rows as u32
        );
    }
}

// ── task0004 round-4 rework (review round-3 finding `ae43417cee647afa`):
// PaneDims packs cols/rows into a single AtomicU32 ───────────────────

/// Pack/unpack round-trips for boundary values, including the shared
/// max the decoder accepts and adjacent-but-distinguishable pairs
/// (guards against a swapped high/low half).
#[test]
fn pane_dims_pack_unpack_round_trips_boundary_values() {
    for (cols, rows) in [
        (1u16, 1u16),
        (80, 24),
        (4096, 4096),
        (65535, 65535),
        (1, 65535),
        (65535, 1),
    ] {
        let dims = PaneDims::new(cols, rows);
        assert_eq!(dims.get(), (cols, rows));
    }
}

/// `set` followed by `get` always observes the LATEST pair, never a mix
/// of an old and new value — trivially true for a single atomic, but
/// pinned here as the observable contract this field's whole design
/// exists to guarantee (review round-3 finding `ae43417cee647afa`).
#[test]
fn pane_dims_set_then_get_observes_the_latest_pair_atomically() {
    let dims = PaneDims::new(80, 24);
    assert_eq!(dims.get(), (80, 24));
    dims.set(120, 40);
    assert_eq!(
        dims.get(),
        (120, 40),
        "must never observe a mix like (80, 40) or (120, 24)"
    );
}

#[test]
fn test_resize_fails_without_master() {
    let target = make_output_target();
    let mut pane = MuxPane::new_test(1, 80, 24, target);
    let result = pane.resize(120, 40);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("PTY master closed"));
    // Dimensions should not change on error
    assert_eq!(pane.cols, 80);
    assert_eq!(pane.rows, 24);
}

// ── D7'' (task0005 rework, review round-4 finding `ef9ab1689853785c`):
// a failed `master.resize()` must not leave `PaneDims` advanced ───────

/// Test double whose `resize` always fails, so `MuxPane::resize` can be
/// exercised past the point where a REAL master is already open (unlike
/// `test_resize_fails_without_master`, which only covers the "no master
/// at all" branch).
#[cfg(unix)]
struct FailingResizeMaster;

#[cfg(unix)]
impl portable_pty::MasterPty for FailingResizeMaster {
    fn resize(&self, _size: portable_pty::PtySize) -> Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("simulated resize failure"))
    }
    fn get_size(&self) -> Result<portable_pty::PtySize, anyhow::Error> {
        Ok(portable_pty::PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
    }
    fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("not supported in test double"))
    }
    fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("not supported in test double"))
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }
    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        None
    }
}

/// D7'': a PTY resize failure must leave `PaneDims` (and
/// `self.cols`/`self.rows`) unchanged — not advanced to the size the
/// PTY never actually reached. Left advanced, the reader thread would
/// read the bogus new size on its very next chunk and hand it to
/// `ScrollbackRingBuffer::attribute_write`, which — seeing a mismatch
/// against the ring's last-recorded dims — would record a CORRECTIVE
/// marker for dimensions the PTY was never actually at, misattributing
/// every later chunk in this pane's scrollback.
///
/// Confirmed to fail pre-fix: before the rollback, `self.dims.set(cols,
/// rows)` ran unconditionally before `master.resize()`'s early return,
/// so a resize failure left `pane.dims.get()` reporting the NEW
/// (never-applied) size while `pane.cols`/`pane.rows` stayed at the OLD
/// size — this test's `dims.get()` assertion would then observe
/// `(120, 40)` instead of the expected `(80, 24)`.
#[cfg(unix)]
#[test]
fn resize_failure_rolls_back_published_dims() {
    let target = make_output_target();
    let mut pane = MuxPane::new(
        1,
        80,
        24,
        target,
        Box::new(std::io::sink()),
        Box::new(FailingResizeMaster),
        None,
    );
    let before = pane.dims.get();
    assert_eq!(before, (80, 24));

    let result = pane.resize(120, 40);
    assert!(result.is_err(), "resize must surface the PTY failure");

    assert_eq!(
        pane.dims.get(),
        before,
        "PaneDims must roll back to the size the PTY actually still has \
         after a failed resize — a stale published size would \
         misattribute every later chunk"
    );
    assert_eq!(pane.cols, 80);
    assert_eq!(pane.rows, 24);

    // No corrective marker should have been recorded either — the
    // ring's only segment is still the pane's initial construction
    // dims.
    let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert_eq!(segments, vec![(0usize, 80u16, 24u16)]);
}

/// AC-6, D4'''''' (round-9 rework, review round-8 finding
/// `7be271b2ead1bf07`, independently confirmed by `codex:architecture`):
/// when the corrective `master.resize()` inside `MuxPane::new` FAILS,
/// the pane must record the PTY's ACTUAL size (queried via
/// `get_size()`), not the clamped values it never reached — mirroring
/// `MuxPane::resize`'s own rollback on the same failure just above
/// (D7'', task0005). Reuses `FailingResizeMaster` (this module,
/// `get_size()` reports a fixed `(80, 24)`, `resize()` always errors).
///
/// Confirmed to fail pre-fix: before this change, `MuxPane::new`
/// recorded `(clamped_cols, clamped_rows)` unconditionally once the
/// resize attempt returned (log-and-continue), so this test — whose
/// `FailingResizeMaster.resize()` always errors — would have left
/// `pane.cols`/`pane.rows` at the CLAMPED
/// `(RESIZE_MARKER_MAX_COLS, ...)` values instead of the simulated
/// PTY's real, never-changed `(80, 24)`, and the initial scrollback
/// segment would describe a size the PTY does not have.
#[cfg(unix)]
#[test]
fn new_pane_records_actual_pty_size_when_resize_fails() {
    let target = make_output_target();
    // u16::MAX is out of domain — triggers the clamp-then-resize path;
    // the resize call always fails via `FailingResizeMaster`.
    let pane = MuxPane::new(
        1,
        u16::MAX,
        u16::MAX,
        target,
        Box::new(std::io::sink()),
        Box::new(FailingResizeMaster),
        None,
    );
    assert_eq!(
        (pane.cols, pane.rows),
        (80, 24),
        "when the corrective resize fails, MuxPane::new must record the \
         PTY's ACTUAL size (FailingResizeMaster's get_size(), (80, 24)), \
         not the clamped values it never reached"
    );
    let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16)],
        "the initial scrollback segment must match what the PTY \
         actually has, not the refused clamp"
    );
}

#[test]
fn test_mark_exited_clears_writer_and_master() {
    let target = make_output_target();
    let mut pane = MuxPane::new_test(1, 80, 24, target);
    assert!(!pane.exited);

    pane.mark_exited();
    assert!(pane.exited);

    // Writing should fail after exit
    let result = pane.write_input(b"hello");
    assert!(result.is_err());
}

// ── Child handle retention + reap (task0001) ───────────────────────────

/// A child double that never reports an exit and is deliberately slow
/// to respond to any query — used to prove `mark_exited` never
/// synchronously touches the child at all (TS-10, NFR1). If a future
/// regression made `mark_exited` call any `Child`/`ChildKiller` method
/// itself, this double's artificial delay would make that regression
/// obvious in the timing assertion below. The reap this double is
/// eventually handed off to runs on a detached background thread, so
/// its slowness never blocks test completion.
#[derive(Debug)]
struct SlowExitChild;

impl portable_pty::ChildKiller for SlowExitChild {
    fn kill(&mut self) -> std::io::Result<()> {
        std::thread::sleep(std::time::Duration::from_secs(2));
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        unimplemented!("not exercised by this test")
    }
}

impl portable_pty::Child for SlowExitChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        std::thread::sleep(std::time::Duration::from_secs(2));
        Ok(None)
    }

    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        std::thread::sleep(std::time::Duration::from_secs(2));
        Ok(portable_pty::ExitStatus::with_exit_code(0))
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// AC-2 (TS-1): `mark_exited` on a pane with no child handle (the
/// `new_test` construction, which never had a child to begin with)
/// starts no reap and does not panic.
#[test]
fn mark_exited_on_childless_pane_does_not_panic() {
    let target = make_output_target();
    let mut pane = MuxPane::new_test(1, 80, 24, target);
    assert!(!pane.has_child());

    pane.mark_exited(); // must not panic

    assert!(pane.exited);
}

/// AC-3 (TS-2): `mark_exited` removes the child handle from the pane —
/// a second call (concurrent teardown paths racing) finds no handle,
/// does not panic, and starts no second reap.
#[cfg(unix)]
#[test]
fn mark_exited_removes_child_handle_and_second_call_is_a_noop() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(
        1,
        80,
        24,
        target,
        writer,
        pair.master,
        Some(Box::new(SlowExitChild)),
    );
    assert!(pane.has_child());

    pane.mark_exited();
    assert!(
        !pane.has_child(),
        "the handle must be removed so a second mark_exited starts no second reap"
    );

    // A second call must find nothing and not panic.
    pane.mark_exited();
    assert!(!pane.has_child());
}

/// AC-3, NFR1 (TS-10): `mark_exited` returns promptly even when the
/// pane holds a child whose exit-status/kill/wait calls are
/// deliberately slow — proving it hands the child off to the reaper
/// rather than waiting on it itself. A wide margin (well below the
/// double's multi-second delay) keeps this assertion CI-safe.
#[cfg(unix)]
#[test]
fn mark_exited_returns_promptly_even_with_a_slow_to_reap_child() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(
        1,
        80,
        24,
        target,
        writer,
        pair.master,
        Some(Box::new(SlowExitChild)),
    );

    let started = std::time::Instant::now();
    pane.mark_exited();
    assert!(
        started.elapsed() < std::time::Duration::from_millis(500),
        "mark_exited must return promptly regardless of the child's own \
         responsiveness — its runtime must be independent of the \
         child's exit behavior (NFR1)"
    );
}

// ── Process-id based child (task plan task0007, IMPLEMENTATION.md D6) ──

/// Poll `/proc/<pid>` until the pid is gone entirely — the outcome once
/// the background reaper `mark_exited` hands the process id off to has
/// actually collected it. Mirrors `child_reaper`'s own
/// `assert_pid_reaped` test helper (kept local here since that one is
/// private to its own module).
#[cfg(unix)]
fn assert_pid_eventually_reaped(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "pid {pid} should have been reaped via the process-id path, \
                 but /proc/{pid} still exists"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// AC-4: a pane holding a process id, when marked exited, is reaped
/// through the process-id path (confirmed by the OS-level `/proc` check
/// below, not just the pane's own bookkeeping) and ends in the same
/// observable state as a pane holding an owned handle — compare
/// `test_mark_exited_clears_writer_and_master` and
/// `mark_exited_removes_child_handle_and_second_call_is_a_noop`: `exited`
/// set, writer/master released, and the child reference cleared so a
/// second `mark_exited` is a no-op.
#[cfg(unix)]
#[test]
fn mark_exited_on_pane_with_process_id_reaps_via_pid_path_and_matches_observable_state() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();

    let child = std::process::Command::new("true")
        .spawn()
        .expect("failed to spawn test child process");
    let pid = child.id();

    let mut pane = MuxPane::new_with_process_id(1, 80, 24, target, writer, pair.master, pid);
    assert!(pane.has_child());
    assert!(!pane.exited);

    pane.mark_exited();

    assert!(pane.exited);
    assert!(
        !pane.has_child(),
        "the process id reference must be cleared so a second mark_exited is a no-op"
    );
    assert!(
        pane.write_input(b"hello").is_err(),
        "writer must be released, matching the owned-handle path's observable state"
    );

    // A second call must find nothing and not panic (mirrors
    // `mark_exited_removes_child_handle_and_second_call_is_a_noop`).
    pane.mark_exited();
    assert!(!pane.has_child());

    assert_pid_eventually_reaped(pid);
    // Do not call `child.wait()` — the pid was already reaped via the
    // process-id path above.
    drop(child);
}

#[test]
fn test_write_input_to_sink() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    // sink() writer always succeeds
    assert!(pane.write_input(b"hello world").is_ok());
}

#[test]
fn test_channel_backpressure_full() {
    // Channel capacity 1: second send should fail with Full
    let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
    // First send succeeds
    assert!(tx.try_send(PtyOutputChunk::pty_output(1, vec![1])).is_ok());
    // Second send hits backpressure (channel full)
    let result = tx.try_send(PtyOutputChunk::pty_output(1, vec![2]));
    assert!(result.is_err());
    match result {
        Err(mpsc::error::TrySendError::Full(_)) => {} // expected
        _ => panic!("Expected Full error"),
    }
}

#[test]
fn test_channel_closed_detection() {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(PTY_CHANNEL_CAPACITY);
    drop(rx); // Close receiver
    let result = tx.try_send(PtyOutputChunk::pty_output(1, vec![1]));
    assert!(result.is_err());
    match result {
        Err(mpsc::error::TrySendError::Closed(_)) => {} // expected
        _ => panic!("Expected Closed error"),
    }
}

/// Phase 1 ergonomics: `pty_output(...)` tags as `PtyOutput`,
/// `snapshot(...)` tags as `Snapshot`. Default reader / resume callers
/// keep `kind == PtyOutput`; only the snapshot handler opts into
/// `kind == Snapshot`. Verifies the discriminator is honored by the
/// two named constructors.
#[test]
fn test_chunk_kind_constructors_round_trip() {
    let live = PtyOutputChunk::pty_output(1, b"abc".to_vec());
    assert_eq!(live.pane_id, 1);
    assert_eq!(live.data, b"abc");
    assert_eq!(live.kind, ChunkKind::PtyOutput);

    let snap = PtyOutputChunk::snapshot(2, b"snapshot-bytes".to_vec());
    assert_eq!(snap.pane_id, 2);
    assert_eq!(snap.data, b"snapshot-bytes");
    assert_eq!(snap.kind, ChunkKind::Snapshot);
}

#[test]
fn test_bounded_channel_capacity_constant() {
    // Verify the constant is reasonable (not too small, not too large)
    assert!(PTY_CHANNEL_CAPACITY >= 64);
    assert!(PTY_CHANNEL_CAPACITY <= 4096);
}

#[cfg(unix)]
#[test]
fn test_resize_with_real_pty() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();

    let target = make_output_target();
    let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

    let result = pane.resize(120, 40);
    assert!(result.is_ok());
    assert_eq!(pane.cols, 120);
    assert_eq!(pane.rows, 40);
}

// ── resize marker recording (task0001, IMPLEMENTATION.md D1/D2) ──────

/// `MuxPane::new` records the pane's INITIAL dimensions as the very
/// first scrollback bytes, so a replay always has a marker to resize
/// into before the earliest retained segment.
#[cfg(unix)]
#[test]
fn test_new_pane_records_initial_dims_marker_in_scrollback() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
    let (bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert!(bytes.is_empty(), "no content bytes were ever written");
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16)],
        "the initial dims must be recorded structurally, not as bytes"
    );
}

/// A resize that actually changes dimensions records a marker with the
/// NEW dimensions into the pane's scrollback ring.
#[cfg(unix)]
#[test]
fn test_resize_records_marker_in_scrollback_when_dims_change() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

    pane.resize(120, 40).unwrap();

    let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert!(
        segments
            .iter()
            .any(|&(_, cols, rows)| (cols, rows) == (120, 40)),
        "resize must record a segment with the new dimensions: {segments:?}"
    );
}

/// A no-op resize (same dimensions as current) must NOT record a
/// redundant marker — only `MuxPane::new`'s initial marker is present.
#[cfg(unix)]
#[test]
fn test_resize_same_dims_does_not_record_extra_marker() {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);

    pane.resize(80, 24).unwrap(); // same dims as construction

    let (bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    assert!(bytes.is_empty());
    assert_eq!(
        segments,
        vec![(0usize, 80u16, 24u16)],
        "a no-op resize must not add a second segment"
    );
}

/// review round-1 rework, finding 83bed291fb779f52 (high) / task0002
/// AC-4: `resize()` must hold the scrollback lock across BOTH the
/// PTY-visible resize and the marker write, establishing a single
/// ordering owner against a concurrent scrollback writer (the PTY
/// reader thread). Proven deterministically: while a competing thread
/// holds `pane.scrollback`'s lock (standing in for a reader thread's
/// in-flight append), `resize()` must be unable to complete — if it
/// could, that would mean it never needed the lock across its whole
/// body, reopening the exact race the fix closes.
#[cfg(unix)]
#[test]
fn test_resize_holds_scrollback_lock_establishing_ordering_with_reader_thread() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
    let scrollback = pane.scrollback.clone();

    // Hold the scrollback lock from the TEST thread first, standing in
    // for the PTY reader thread's write() call already in flight.
    let guard = scrollback.lock().unwrap();

    let resize_done = Arc::new(AtomicBool::new(false));
    let rd = resize_done.clone();
    let resizer = std::thread::spawn(move || {
        let result = pane.resize(120, 40);
        rd.store(true, Ordering::SeqCst);
        (pane, result)
    });

    // resize() must NOT be able to complete while the lock is held —
    // pre-fix, master.resize() ran outside any lock and the whole call
    // could finish freely here.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        !resize_done.load(Ordering::SeqCst),
        "resize() must block on the scrollback lock, establishing that \
         no concurrent write can land ahead of its marker"
    );

    drop(guard);
    let (pane, result) = resizer.join().unwrap();
    assert!(result.is_ok());
    assert_eq!(pane.cols, 120);
    assert_eq!(pane.rows, 40);
}

/// mux-snapshot-output-boundary task0001, AC-2 ("Resize"): `resize()`'s
/// ring-marker + PTY-resize + shadow-resize step runs under the SAME
/// capture exclusion (`output_capture.captured_read`) a snapshot path's
/// own captured read uses. Proven the same way
/// `test_resize_holds_scrollback_lock_establishing_ordering_with_reader_thread`
/// proves scrollback-lock ordering: hold the capture exclusion open from
/// the test thread (standing in for a concurrent snapshot's captured
/// read) via a `captured_read` call whose closure blocks on a channel,
/// and show `resize()` cannot complete until it is released. Once both
/// complete, the ring's latest dimension marker equals the shadow
/// parser's dimensions — they were never able to interleave.
#[cfg(unix)]
#[test]
fn test_resize_and_a_concurrent_captured_read_are_mutually_excluded() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty_system.openpty(size).unwrap();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
    let output_capture = pane.output_capture.clone();

    let (arrived_tx, arrived_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        output_capture.captured_read(|| {
            arrived_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
    });
    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the standing-in captured_read must signal arrival");

    let resize_done = Arc::new(AtomicBool::new(false));
    let rd = resize_done.clone();
    let resizer = std::thread::spawn(move || {
        let mut pane = pane;
        let result = pane.resize(120, 40);
        rd.store(true, Ordering::SeqCst);
        (pane, result)
    });

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        !resize_done.load(Ordering::SeqCst),
        "resize() must block on the capture exclusion while a concurrent \
         captured_read (standing in for a snapshot path) holds it open"
    );

    release_tx.send(()).unwrap();
    holder.join().unwrap();
    let (pane, result) = resizer.join().unwrap();
    assert!(result.is_ok());

    let (_bytes, segments) = pane.scrollback.lock().unwrap().read_segments();
    let (shadow_rows, shadow_cols) = pane.shadow_parser.lock().unwrap().screen().size();
    let last_marker = *segments.last().expect("resize must have recorded a marker");
    assert_eq!(
        (last_marker.1, last_marker.2),
        (shadow_cols, shadow_rows),
        "the ring's latest dimension marker must equal the shadow parser's \
         dimensions — they were never able to interleave"
    );
}

/// Build a `Detached` target with a `NetworkDetach`-only reason and
/// `owner = None` (system origin), matching the daemon's pre-attach state.
fn detached_system_target() -> SharedOutputTarget {
    Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::NetworkDetach,
        owner: None,
    }))
}

/// TS-12: detached + visible -> stays Detached.
#[test]
fn test_evaluate_output_target_network_detached_visible_stays_detached() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target = detached_system_target();
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    let result = evaluate_output_target(&pane, true, true, &owned_tx);
    assert!(matches!(result, EvalResult::Unchanged));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Detached { .. }
    ));
}

/// TS-13: identity-scoped Connected -> Detached.
#[test]
fn test_evaluate_output_target_identity_scoped_connected_to_detached() {
    let (owner_tx, _rx) = mpsc::channel(16);
    let (other_tx, _other_rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(other_tx)));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    let result = evaluate_output_target(&pane, false, false, &owner_tx);
    assert!(matches!(result, EvalResult::Unchanged));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
}

#[test]
fn test_evaluate_output_target_owner_can_detach() {
    let (owner_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owner_tx.clone())));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    let result = evaluate_output_target(&pane, false, false, &owner_tx);
    assert!(matches!(result, EvalResult::SwitchedToDetached));
    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, owner, .. } => {
            assert_eq!(*reason, DetachReason::HiddenByVisibility);
            let owner = owner.as_ref().expect("owner must be set");
            assert!(owner.same_channel(&owner_tx));
        }
        _ => panic!("expected Detached"),
    }
}

/// AC-1 (FR10; task0004): `evaluate_output_target` no longer has a
/// resume branch — only `resume_pane_with_permit` performs a Detached ->
/// Connected transition. For a Detached pane whose owner matches the
/// caller and whose reasons would all clear (a "would-be resume"),
/// `evaluate_output_target` leaves the target — state, reason and owner —
/// exactly as it was, returns `Unchanged`, and puts nothing on the
/// destination channel.
///
/// Replaces `test_evaluate_output_target_detached_to_connected_returns_snapshot`,
/// `test_evaluate_output_target_restores_the_shadow_parsers_header_row_for_a_wrapped_ring`
/// and `test_evaluate_output_target_stays_detached_when_snapshot_exceeds_frame_limit`
/// (deleted: all three asserted the now-removed `EvalResult::ResumeWithSnapshot`
/// variant). The header-row and oversize-frame-limit properties stay
/// covered, unmodified, by their production-path (`resume_pane_with_permit`)
/// siblings: `test_resume_pane_with_permit_restores_the_shadow_parsers_header_row_for_a_wrapped_ring`
/// and `test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit`.
#[test]
fn evaluate_output_target_never_resumes_a_detached_pane() {
    let (owned_tx, mut rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(15, 80, 24, target.clone());
    pane.scrollback
        .lock()
        .unwrap()
        .write(b"would-be-resume-data");

    let result = evaluate_output_target(&pane, false, true, &owned_tx);

    assert!(
        matches!(result, EvalResult::Unchanged),
        "a would-be resume must return Unchanged, never a resume result"
    );
    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, owner } => {
            assert_eq!(*reason, DetachReason::HiddenByVisibility);
            let owner = owner.as_ref().expect("owner must remain set");
            assert!(owner.same_channel(&owned_tx));
        }
        _ => panic!("expected Detached; evaluate_output_target must never resume a pane"),
    }
    assert!(
        rx.try_recv().is_err(),
        "nothing must be sent on the destination channel"
    );
}

/// AC-6 site-reach fixture: a wrapped ring whose last recorded segment dims
/// (80x24) diverge from the shadow parser's current live size (80x40), so
/// the wrap-aware dump block differs between a small probe capacity and the
/// legacy 10,000 default. Mirrors `seed_wrapped_diverging_ring_and_shadow`
/// in `mux::ipc::handlers::tests` and the analogous fixtures in
/// `mux::ipc::connection::tests` / `mux::ipc::reattach::tests`.
fn seed_diverging_wrapped_pane(
    pane: &MuxPane,
) -> (Vec<u8>, Vec<(usize, u16, u16)>, Vec<u8>, (u16, u16)) {
    let mut pre = Vec::new();
    for i in 0..60u32 {
        pre.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    let mut ring = crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(512);
    ring.attribute_write(80, 24, &pre);
    let (raw, segments, wrapped) = ring.read_segments_with_wrap_state();
    assert!(wrapped, "test prerequisite: the ring must have wrapped");
    *pane.scrollback.lock().unwrap() = ring;
    let shadow_dump = {
        let mut parser = pane.shadow_parser.lock().unwrap();
        parser.process(&pre);
        parser.screen_mut().set_size(40, 80);
        parser.screen().contents_formatted()
    };
    (raw, segments, shadow_dump, (80u16, 40u16))
}

#[test]
fn test_evaluate_output_target_already_connected_visible_no_op() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    let result = evaluate_output_target(&pane, false, true, &owned_tx);
    assert!(matches!(result, EvalResult::Unchanged));
}

/// F6 regression: connection A puts a pane into HiddenByVisibility
/// (Detached, owner=A). Connection B then calls SetVisibility(true) with
/// its own tx — must NOT reclaim the pane.
#[test]
fn test_evaluate_output_target_other_connection_cannot_reclaim_hidden() {
    let (a_tx, _a_rx) = mpsc::channel(16);
    let (b_tx, _b_rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(a_tx.clone()),
    }));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());

    let result = evaluate_output_target(&pane, false, true, &b_tx);
    assert!(matches!(result, EvalResult::Unchanged));
    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, owner, .. } => {
            assert_eq!(*reason, DetachReason::HiddenByVisibility);
            let owner = owner.as_ref().expect("owner must remain A");
            assert!(
                owner.same_channel(&a_tx),
                "pane must still be owned by connection A"
            );
        }
        _ => panic!("expected Detached, got Connected"),
    }
}

/// AC-4 (FR10; TS-12; task0004): same connection's hide -> show round
/// trip restores Connected. Ported from
/// `test_evaluate_output_target_same_connection_hide_show_roundtrip`
/// (deleted: it asserted the now-removed `EvalResult::ResumeWithSnapshot`
/// variant on the show side) — hide still goes through
/// `evaluate_output_target` (visible = false), but show now goes through
/// the production `resume_pane_with_permit`, the only function that
/// performs a Detached -> Connected transition.
#[test]
fn resume_pane_with_permit_hide_show_roundtrip_on_same_connection() {
    let (a_tx, mut a_rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(a_tx.clone())));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());

    let r1 = evaluate_output_target(&pane, false, false, &a_tx);
    assert!(matches!(r1, EvalResult::SwitchedToDetached));

    let permit = a_tx
        .try_reserve()
        .expect("channel has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &a_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));

    let chunk = a_rx
        .try_recv()
        .expect("exactly one snapshot chunk delivered");
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
    assert!(a_rx.try_recv().is_err(), "nothing else must be delivered");
}

/// F6: when both NetworkDetach and HiddenByVisibility are active,
/// SetVisibility(true) only clears the hidden bit. The pane stays
/// Detached because the network reason is still active. Only the reattach
/// path may clear `NetworkDetach`.
#[test]
fn test_evaluate_output_target_both_reasons_visible_keeps_detached() {
    let (a_tx, _a_rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::Both,
        owner: Some(a_tx.clone()),
    }));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());

    let result = evaluate_output_target(&pane, false, true, &a_tx);
    assert!(matches!(result, EvalResult::Unchanged));
    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, .. } => {
            assert_eq!(
                *reason,
                DetachReason::NetworkDetach,
                "hidden bit cleared but network bit stays"
            );
        }
        _ => panic!("expected Detached"),
    }
}

/// F6: system-origin Detached (`owner = None`, reason = NetworkDetach)
/// is NOT cleared by `evaluate_output_target` — the `NetworkDetach` bit
/// only resolves through the reattach path. Until then, the pane stays
/// Detached even when the caller asserts `visible = true`. The owner
/// slot is adopted so a subsequent visibility transition is matched
/// against the correct connection.
#[test]
fn test_evaluate_output_target_system_origin_stays_detached_until_reattach() {
    let (a_tx, _a_rx) = mpsc::channel(16);
    let target = detached_system_target();
    let pane = MuxPane::new_test(1, 80, 24, target.clone());

    let result = evaluate_output_target(&pane, false, true, &a_tx);
    assert!(matches!(result, EvalResult::Unchanged));
    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, owner, .. } => {
            assert_eq!(*reason, DetachReason::NetworkDetach);
            let owner = owner.as_ref().expect("owner adopted from caller");
            assert!(owner.same_channel(&a_tx));
        }
        _ => panic!("expected Detached"),
    }
}

/// F2: `resume_pane_with_permit` must enqueue the snapshot via the
/// caller-supplied permit and only swap to Connected after the send.
/// The pane mutex is held for the full sequence, so a reader thread
/// taking the same mutex cannot push a live chunk between the two
/// steps. This test asserts the post-conditions.
#[tokio::test]
async fn test_resume_pane_with_permit_sends_then_swaps() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(7, 80, 24, target.clone());
    pane.scrollback.lock().unwrap().write(b"ring-data");
    pane.shadow_parser.lock().unwrap().process(b"resume-shadow");
    pane.raw_passthrough
        .lock()
        .unwrap()
        .append(b"\x1b_Gi=7;PASS\x1b\\");

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    // Target switched to Connected.
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));

    // Snapshot is on the channel.
    let chunk = rx.try_recv().expect("snapshot enqueued under pane lock");
    assert_eq!(chunk.pane_id, 7);
    let content = decode_snapshot_content(&chunk.data);
    assert!(content.starts_with(b"\x1b[H\x1b[2J"));
    // Captured passthrough must NOT be replayed (would re-render the image).
    let needle_passthrough = b"\x1b_Gi=7;PASS\x1b\\";
    assert!(
        !content
            .windows(needle_passthrough.len())
            .any(|w| w == needle_passthrough),
        "snapshot must NOT contain captured passthrough"
    );
    // Plain-text ring history is still restored.
    assert!(
        content
            .windows(b"ring-data".len())
            .any(|w| w == b"ring-data"),
        "snapshot must contain ring data"
    );
    // Main-buffer pane (shadow_parser never entered alt-screen): the
    // daemon vt100 `contents_formatted()` dump must NOT appear in the
    // snapshot. The client rebuilds the visible viewport from scrollback
    // alone — this is the resume-path counterpart of the main/alt split
    // in `build_snapshot_bytes`.
    assert!(
        !content
            .windows(b"resume-shadow".len())
            .any(|w| w == b"resume-shadow"),
        "main-buffer resume snapshot must omit the shadow screen dump"
    );

    // raw_passthrough drained.
    assert!(pane.raw_passthrough.lock().unwrap().is_empty());

    // review round-1 rework, finding 20b2bed0aaf48f94: the resume
    // snapshot must be tagged Snapshot (not the default PtyOutput) so
    // the client routes it through the marker-interpreting
    // `reset_and_replay` path instead of the marker-blind live path.
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
}

/// Companion to `test_resume_pane_with_permit_sends_then_swaps`: when
/// the shadow parser is in alt-screen mode the resume snapshot DOES
/// include the daemon vt100 dump (so the TUI surface is restored).
/// Mirror of the alt branch in `build_snapshot_bytes` applied to the
/// visibility-resume code path.
#[tokio::test]
async fn test_resume_pane_with_permit_includes_screen_for_alt_screen() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(11, 80, 24, target.clone());
    // Flip the shadow parser into alt-screen mode BEFORE feeding the
    // screen content so the resume builder follows the alt branch.
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"ALT-RESUME-SHADOW");

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx.try_recv().expect("snapshot enqueued");
    assert_eq!(chunk.pane_id, 11);
    let content = decode_snapshot_content(&chunk.data);
    assert!(content.starts_with(b"\x1b[H\x1b[2J"));
    assert!(
        content
            .windows(b"ALT-RESUME-SHADOW".len())
            .any(|w| w == b"ALT-RESUME-SHADOW"),
        "alt-screen resume snapshot must include the shadow screen dump"
    );
}

/// AC-3 (FR2, FR5, FR6, FR7; mux-snapshot-ring-wrap-restore task0001):
/// `resume_pane_with_permit` also restores the shadow parser's header row
/// for a wrapped MAIN-BUFFER pane (not just alt-screen) — the
/// visibility-resume counterpart of the reattach / on-demand reproduction
/// fixtures. Fixture per the task plan's Test Notes: replace the pane's
/// ring with a small-capacity one, feed the same bytes to both the ring
/// (via `attribute_write`) and the shadow parser until cumulative bytes
/// exceed capacity.
#[tokio::test]
async fn test_resume_pane_with_permit_restores_the_shadow_parsers_header_row_for_a_wrapped_ring() {
    let cols: u16 = 80;
    let rows: u16 = 24;
    const SMALL_CAPACITY: usize = 2048;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(13, cols, rows, target.clone());
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(SMALL_CAPACITY);

    let mut cumulative: usize = 0;
    let mut header = Vec::new();
    header.extend_from_slice(b"\x1b[H\x1b[2J\x1b[1;1H");
    header.extend_from_slice(b"PID USER HEADER-ROW-TEXT");
    pane.scrollback
        .lock()
        .unwrap()
        .attribute_write(cols, rows, &header);
    pane.shadow_parser.lock().unwrap().process(&header);
    cumulative += header.len();
    for i in 0..200u32 {
        let frame = format!("\x1b[2;1Hframe {i:>4}\x1b[K").into_bytes();
        pane.scrollback
            .lock()
            .unwrap()
            .attribute_write(cols, rows, &frame);
        pane.shadow_parser.lock().unwrap().process(&frame);
        cumulative += frame.len();
    }
    assert!(
        cumulative > SMALL_CAPACITY,
        "test prerequisite: ring must have wrapped"
    );

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx.try_recv().expect("snapshot enqueued");
    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
    let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = segments
        .iter()
        .map(|s| term_core::terminal_core::ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    let mut core = term_core::terminal_core::TerminalCore::new(cols, rows, 10_000);
    core.reset_and_replay_segments(content, &replay_segments);
    let header_row = core.get_line_text(0);
    assert!(
        header_row.contains("HEADER-ROW-TEXT"),
        "post-wrap visibility-resume snapshot must restore the shadow \
         parser's header row; replayed row 0 was {header_row:?}"
    );
}

/// AC-6: `resume_pane_with_permit` passes its `probe_capacity` argument all
/// the way to `build_resume_snapshot_bytes_for_ring`.
#[tokio::test]
async fn test_resume_pane_with_permit_site_reach_uses_the_passed_probe_capacity() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(16, 80, 24, target.clone());
    let (raw, segments, shadow_dump, current_dims) = seed_diverging_wrapped_pane(&pane);

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 5);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx.try_recv().expect("snapshot enqueued");
    let actual = decode_snapshot_content(&chunk.data);

    let (expected_payload_5, _) = crate::mux::snapshot_bytes::build_resume_snapshot_bytes_for_ring(
        &raw,
        &segments,
        &shadow_dump,
        false,
        true,
        current_dims,
        5,
    );
    let (expected_payload_10k, _) =
        crate::mux::snapshot_bytes::build_resume_snapshot_bytes_for_ring(
            &raw,
            &segments,
            &shadow_dump,
            false,
            true,
            current_dims,
            10_000,
        );

    assert_eq!(
        actual, expected_payload_5,
        "resume_pane_with_permit must match the builder output at the \
         passed probe_capacity (5)"
    );
    assert_ne!(
        actual, expected_payload_10k,
        "the passed probe_capacity (5) must actually reach the builder, \
         not silently fall back to the legacy 10,000 default"
    );
}

/// F2: full Both reason cannot be cleared by `resume_pane_with_permit`
/// alone — NetworkDetach stays. The permit is dropped without sending.
#[tokio::test]
async fn test_resume_pane_with_permit_keeps_detached_when_network_bit_set() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::Both,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(8, 80, 24, target.clone());

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::NoChange));

    match &*target.lock().unwrap() {
        PaneOutputTarget::Detached { reason, .. } => {
            assert_eq!(*reason, DetachReason::NetworkDetach);
        }
        _ => panic!("expected Detached"),
    }
    assert!(rx.try_recv().is_err(), "no snapshot must be sent");
}

/// F2: connected pane is a no-op (already resumed).
#[tokio::test]
async fn test_resume_pane_with_permit_no_change_when_already_connected() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let pane = MuxPane::new_test(9, 80, 24, target.clone());

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::NoChange));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
    assert!(rx.try_recv().is_err());
}

/// F2: owner mismatch (different connection's tx) must be NoChange.
#[tokio::test]
async fn test_resume_pane_with_permit_owner_mismatch_keeps_detached() {
    let (a_tx, _a_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let (b_tx, mut b_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(a_tx.clone()),
    }));
    let pane = MuxPane::new_test(10, 80, 24, target.clone());

    let permit = b_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &b_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::NoChange));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Detached { .. }
    ));
    assert!(b_rx.try_recv().is_err(), "no snapshot must reach B");
}

/// D6''' (round-6 rework, review round-5 finding `89b58cd82d7aa713`):
/// an encoded snapshot too large for a single codec frame must NOT be
/// enqueued — the pane stays Detached (fail recoverably) rather than
/// being handed a frame `mux::ipc::connection`'s codec would reject
/// (which previously tore the whole connection down).
///
/// Confirmed to fail pre-fix: the oversize check only LOGGED and still
/// unconditionally sent + swapped to Connected — this test's
/// `ResumeOutcome::NoChange` / `PaneOutputTarget::Detached` /
/// "nothing enqueued" assertions would all have failed.
#[tokio::test]
async fn test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(12, 80, 24, target.clone());
    // Replace the default (2 MiB) ring with one large enough to hold
    // content that, once encoded, exceeds `MAX_SNAPSHOT_FRAME_PAYLOAD`
    // (~16 MiB) — the default ring's own cap makes this unreachable
    // otherwise (real panes never approach the codec's frame limit).
    let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
    pane.scrollback
        .lock()
        .unwrap()
        .write(&vec![b'x'; oversize_capacity]);

    let permit = owned_tx.reserve().await.expect("reserve permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(
        matches!(outcome, ResumeOutcome::NoChange),
        "oversize snapshot must not resume the pane"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "pane must stay Detached rather than swap to Connected with an \
         unsendable snapshot"
    );
    assert!(
        rx.try_recv().is_err(),
        "no snapshot may reach the channel when it would exceed the \
         single-frame limit"
    );
}

// ── mux-snapshot-output-boundary task0003: reader-level test driving the
// PRODUCTION resume_pane_with_permit while the PTY reader thread is
// paused at P2 (TS-2) ───────────────────────────────────────────────────

/// Test-only scripted `Read` source feeding a fixed sequence of chunks,
/// then EOF. A file-local copy of
/// `mux::ipc::pty_spawn::tests::ScriptedReader` (that one is private to
/// its own module).
struct ScriptedReader {
    chunks: std::collections::VecDeque<Vec<u8>>,
}

impl ScriptedReader {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: chunks.into_iter().collect(),
        }
    }
}

impl std::io::Read for ScriptedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.chunks.pop_front() {
            Some(chunk) => {
                let n = chunk.len();
                buf[..n].copy_from_slice(&chunk);
                Ok(n)
            }
            None => Ok(0),
        }
    }
}

/// Spawn the PRODUCTION `pty_reader_loop` against `pane`'s OWN
/// already-set `output_target` and shared state (unlike this module's
/// synthetic setup helpers, this drives the SAME reader flow the daemon
/// runs), fed `chunks` by a [`ScriptedReader`]. Mirrors
/// `mux::ipc::pty_spawn::register_pane_and_start_reader`'s own clone
/// dance. Returns the join handle.
fn spawn_reader_thread(pane: &MuxPane, chunks: Vec<Vec<u8>>) -> std::thread::JoinHandle<()> {
    let output_target = pane.output_target.clone();
    let shadow_parser = pane.shadow_parser.clone();
    let cwd = pane.cwd.clone();
    let title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let dims = pane.dims.clone();
    let output_capture = pane.output_capture.clone();
    let pane_id = pane.id;
    std::thread::spawn(move || {
        crate::mux::ipc::pty_spawn::pty_reader_loop(
            pane_id,
            Box::new(ScriptedReader::new(chunks)),
            output_target,
            shadow_parser,
            cwd,
            title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            dims,
            Arc::new(StdMutex::new(None)),
            output_capture,
        );
    })
}

/// AC-1 (FR1, FR2, FR3; TS-2): with the PTY reader paused at P2 — capture
/// step finished, `output_target` not yet taken — on a chunk containing
/// an insert-line sequence (`CSI L`), the PRODUCTION
/// `resume_pane_with_permit` on a `Detached { HiddenByVisibility, owner:
/// Some(S) }` pane owned by S returns `Resumed`. After release: S's
/// channel holds the snapshot first, the paused chunk's raw bytes never
/// follow it, and the client view (snapshot replay, then every later
/// delivered byte) matches a fresh reference fed the raw stream once.
///
/// AC-2 (sensitivity for AC-1): this test was confirmed, once, to FAIL
/// when `resume_pane_with_permit`'s
/// `pane.output_capture.record_boundary(owned_tx, boundary);` call was
/// temporarily commented out — with no boundary recorded, the reader's
/// post-release forward decision found the paused chunk uncovered and
/// delivered its raw bytes right after the snapshot, failing the "never
/// follow it" assertion below (`assert!(!received.iter().any(|c| c.data
/// == paused_chunk))`). Production code was restored byte-for-byte
/// afterward — see this task's `*.tests.yaml` for the observed failure.
#[test]
fn resume_pane_with_permit_drives_the_paused_reader_to_suppress_the_paused_chunk_and_matches_the_raw_stream_reference()
 {
    let cols: u16 = 80;
    let rows: u16 = 24;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(20, cols, rows, target.clone());
    let output_capture = pane.output_capture.clone();

    // Baseline read (number 1) establishing pre-existing screen content,
    // mirroring `mux::ipc::pty_spawn::tests`'s own on-demand-snapshot
    // convention: seed the ring/shadow directly without driving it
    // through the reader, so the chunk under test becomes number 2.
    let baseline: &[u8] = b"line1\r\nline2\r\nline3\r\n";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(baseline);
        pane.scrollback.lock().unwrap().write(baseline);
    });

    let paused_chunk = b"\x1b[1;1H\x1b[L".to_vec();
    let continuation_chunk = b"more output after resume\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = spawn_reader_thread(
        &pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    );

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the paused chunk");

    // Reserve a slot on S and drive the PRODUCTION resume path while the
    // reader is paused.
    let permit = owned_tx
        .try_reserve()
        .expect("S has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }

    assert!(
        !received.iter().any(|c| c.data == paused_chunk),
        "the paused chunk's raw bytes must never reach S after the \
         snapshot"
    );
    assert_eq!(
        received[0].kind,
        ChunkKind::Snapshot,
        "S must hold the snapshot first"
    );
    assert_eq!(
        received[1].data, continuation_chunk,
        "the next (unsuppressed) reader chunk must follow the snapshot"
    );
    assert!(received[2].data.is_empty(), "EOF chunk must follow");
    assert_eq!(received.len(), 3, "nothing else must have been delivered");

    // Reference equality: feed a fresh term_core [snapshot replay, then
    // every later delivered byte] and compare against a fresh reference
    // fed the whole raw stream once.
    use term_core::terminal_core::{MODE_ORIGIN, TerminalCore};
    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&received[0].data);
    let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = segments
        .iter()
        .map(|s| term_core::terminal_core::ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(content, &replay_segments);
    client.process_pty_data_fully(&continuation_chunk);

    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(baseline);
    reference.process_pty_data_fully(&paused_chunk);
    reference.process_pty_data_fully(&continuation_chunk);

    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
    assert_eq!(
        client.get_scroll_region_top(),
        reference.get_scroll_region_top()
    );
    assert_eq!(
        client.get_scroll_region_bottom(),
        reference.get_scroll_region_bottom()
    );
    assert_eq!(
        client.get_mode(MODE_ORIGIN),
        reference.get_mode(MODE_ORIGIN)
    );
    assert_eq!(client.get_cursor_row(), reference.get_cursor_row());
    assert_eq!(client.get_cursor_col(), reference.get_cursor_col());
}

/// AC-3 (FR10, NFR2; TS-12): production visible-resume path with a
/// reader paused at P2, on a chunk whose only special content is one
/// complete main-buffer CSI device query — the IMPLEMENTATION.md
/// "Suppressed-chunk delivery contract" input, whose replacement is
/// exactly the query's bytes both before and after this feature. Unlike
/// `resume_pane_with_permit_drives_the_paused_reader_to_suppress_the_paused_chunk_and_matches_the_raw_stream_reference`
/// above (an insert-line-only paused chunk, whose replacement is empty —
/// nothing is delivered for it), this exercises the FR9/FR10 replacement
/// delivery: the destination must receive the Snapshot-kind chunk first,
/// then the replacement, then the next reader chunk, and nothing else.
#[test]
fn production_visible_resume_delivers_snapshot_before_replacement_and_next_chunk() {
    let cols: u16 = 80;
    let rows: u16 = 24;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(22, cols, rows, target.clone());
    let output_capture = pane.output_capture.clone();

    // Main-buffer text, one complete CSI device query (cursor-position
    // report), more main-buffer text — the delivery-contract input.
    let paused_chunk = b"before\x1b[6nafter".to_vec();
    let continuation_chunk = b"more output after resume\r\n".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = spawn_reader_thread(
        &pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    );

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the paused chunk");

    // Drive the production visible path down to `resume_pane_with_permit`
    // while the reader is paused.
    let permit = owned_tx
        .try_reserve()
        .expect("S has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }

    assert_eq!(
        received.len(),
        4,
        "expected snapshot, replacement, next chunk, EOF and nothing else"
    );
    assert_eq!(
        received[0].kind,
        ChunkKind::Snapshot,
        "the destination must receive the snapshot first"
    );
    assert_eq!(
        received[1].data, b"\x1b[6n",
        "the replacement must be exactly the device query's bytes"
    );
    assert_eq!(
        received[1].kind,
        ChunkKind::PtyOutput,
        "the replacement is an ordinary PtyOutput chunk, not another snapshot"
    );
    assert_eq!(
        received[2].data, continuation_chunk,
        "the next (unsuppressed) reader chunk must follow the replacement"
    );
    assert!(received[3].data.is_empty(), "EOF chunk must follow");
}

/// AC-1 (FR8; TS-9) / AC-3 (FR8; TS-9; mux-suppressed-output-fixes
/// task0003): a chunk that enters the alternate screen (`ESC[?1049h`) and
/// draws content, in one read, is suppressed by a visibility resume that
/// runs while the PRODUCTION reader is paused on it — mirrors
/// `resume_pane_with_permit_drives_the_paused_reader_to_suppress_the_paused_chunk_and_matches_the_raw_stream_reference`,
/// this task's alt-screen variant. The chunk carries no device query,
/// incomplete tail, or viewer-launch sequence, so its suppressed-chunk
/// replacement is empty regardless of which classifier implementation is
/// in place (task0001 of this feature, developed in parallel — see
/// IMPLEMENTATION.md's "Suppressed-chunk delivery contract"). After the
/// client model applies [the resume Snapshot, the (empty) replacement, and
/// the next chunk], it is on the alternate screen and its visible rows +
/// cursor equal the shadow parser's own. The next chunk keeps drawing on
/// the alternate screen, so this one test also covers AC-3 (the pane stays
/// on the alternate screen through the whole hide/resume cycle).
#[test]
fn visibility_resume_restores_alt_screen_mode_and_content() {
    use term_core::terminal_core::{MODE_ALT_SCREEN, TerminalCore};

    let cols: u16 = 80;
    let rows: u16 = 24;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(30, cols, rows, target.clone());
    let output_capture = pane.output_capture.clone();

    let baseline: &[u8] = b"before-alt\r\n";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(baseline);
        pane.scrollback.lock().unwrap().write(baseline);
    });

    // Enters the alternate screen and draws content in the SAME chunk — no
    // device query, incomplete tail, or viewer-launch sequence, so its
    // suppressed-chunk replacement is empty.
    let paused_chunk = b"\x1b[?1049h\x1b[1;1HALT-CONTENT".to_vec();
    // Stays on the alternate screen (AC-3).
    let continuation_chunk = b"\x1b[2;1HMORE-ALT".to_vec();

    let (arrived_rx, release_tx) = output_capture.p2.arm();
    let handle = spawn_reader_thread(
        &pane,
        vec![paused_chunk.clone(), continuation_chunk.clone()],
    );

    arrived_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the reader must reach P2 on the paused chunk");

    let permit = owned_tx
        .try_reserve()
        .expect("S has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    release_tx.send(()).unwrap();
    handle.join().unwrap();

    let mut received = Vec::new();
    while let Ok(c) = rx.try_recv() {
        received.push(c);
    }

    assert!(
        !received.iter().any(|c| c.data == paused_chunk),
        "the paused chunk's raw bytes must never reach S after the snapshot"
    );
    assert_eq!(
        received[0].kind,
        ChunkKind::Snapshot,
        "S must hold the snapshot first"
    );
    assert_eq!(
        received[1].data, continuation_chunk,
        "the next (unsuppressed) reader chunk must follow the snapshot \
         directly — the paused chunk's own replacement is empty"
    );
    assert!(received[2].data.is_empty(), "EOF chunk must follow");
    assert_eq!(received.len(), 3, "nothing else must have been delivered");

    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&received[0].data);
    let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = segments
        .iter()
        .map(|s| term_core::terminal_core::ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(content, &replay_segments);
    client.process_pty_data_fully(&continuation_chunk);

    assert!(
        client.get_mode(MODE_ALT_SCREEN),
        "AC-1/AC-3: after applying the resume snapshot and the next chunk, \
         the client model must be on the alternate screen"
    );

    // Oracle: the shadow parser, which the reader has already updated in
    // real time with the same paused + continuation bytes by the time the
    // reader thread joined above.
    let parser = pane.shadow_parser.lock().unwrap();
    assert!(
        parser.screen().alternate_screen(),
        "test prerequisite: the shadow parser must also be on the \
         alternate screen"
    );
    for r in 0..rows {
        let got = client.get_line_text(r);
        let want = parser
            .screen()
            .rows(0, cols)
            .nth(r as usize)
            .unwrap_or_default();
        assert_eq!(got.trim_end(), want.trim_end(), "row {r} mismatch");
    }
    let (want_row, want_col) = parser.screen().cursor_position();
    assert_eq!(client.get_cursor_row(), want_row);
    assert_eq!(client.get_cursor_col(), want_col);
}

/// AC-1 (FR8; TS-9; mux-suppressed-output-fixes task0006): a pane that is
/// hidden while on the alternate screen, and that then receives
/// `ESC[?1049l` (exit the alternate screen) WHILE STILL HIDDEN, is on the
/// main screen by the time it is resumed through the PRODUCTION
/// `resume_pane_with_permit` — the mirror-image transition of
/// `visibility_resume_restores_alt_screen_mode_and_content` above (which
/// covers STAYING on the alternate screen through a hide/resume cycle;
/// this one covers LEAVING it while hidden). Restored from commit
/// 20c4cd9c, lost when task0003 was re-implemented through parent-side
/// adoption (06bf0714) — see
/// `feature-docs/mux-suppressed-output-fixes/DECISIONS.md`
/// (`830f950f39e499fa`).
#[test]
fn visibility_resume_after_hidden_alt_exit_shows_main_screen() {
    use term_core::terminal_core::{MODE_ALT_SCREEN, TerminalCore};

    let cols: u16 = 80;
    let rows: u16 = 24;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(34, cols, rows, target.clone());
    let output_capture = pane.output_capture.clone();

    let before_alt: &[u8] = b"before-alt-line\r\n";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(before_alt);
        pane.scrollback.lock().unwrap().write(before_alt);
    });
    // Enters the alternate screen and draws TUI content — excluded from
    // scrollback, matching real capture (alt-buffer bytes are never
    // written to the pane's scrollback ring).
    let enter_alt: &[u8] = b"\x1b[?1049h\x1b[1;1HALT-TUI-CONTENT";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(enter_alt);
    });
    assert!(
        pane.shadow_parser
            .lock()
            .unwrap()
            .screen()
            .alternate_screen(),
        "test prerequisite: the pane must be on the alternate screen \
         before it receives the exit sequence"
    );

    // Receives ESC[?1049l (exit the alternate screen) while hidden,
    // through the PRODUCTION reader — the pane is Detached the whole
    // time, so nothing is ever forwarded on the channel for this read.
    let exit_alt = b"\x1b[?1049l".to_vec();
    let handle = spawn_reader_thread(&pane, vec![exit_alt]);
    handle.join().unwrap();
    assert!(
        rx.try_recv().is_err(),
        "a Detached pane must never forward a live chunk"
    );

    let after_alt: &[u8] = b"after-alt-line\r\n";
    output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(after_alt);
        pane.scrollback.lock().unwrap().write(after_alt);
    });

    assert!(
        !pane
            .shadow_parser
            .lock()
            .unwrap()
            .screen()
            .alternate_screen(),
        "test prerequisite: the pane must be back on the main screen \
         before it is resumed"
    );

    let permit = owned_tx
        .try_reserve()
        .expect("S has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx.try_recv().expect("snapshot enqueued");
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
    assert!(rx.try_recv().is_err(), "nothing else must be delivered");

    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
    let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = segments
        .iter()
        .map(|s| term_core::terminal_core::ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(content, &replay_segments);

    assert!(
        !client.get_mode(MODE_ALT_SCREEN),
        "AC-1: after applying the resume snapshot, the client model must \
         be on the main screen"
    );

    // Reference: an independent client fed the surviving main-buffer bytes
    // once — the alt-screen content is never captured, so it plays no
    // part in the reference either.
    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(before_alt);
    reference.process_pty_data_fully(after_alt);
    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }
}

/// AC-2 (FR8; TS-10; mux-suppressed-output-fixes task0006): a main-screen
/// pane shows an apt-style progress bar — drawn with a narrowed scroll
/// region (DECSTBM) and a cursor save/restore pair (DECSC/DECRC) used to
/// paint a status line outside the region. Hiding it
/// (`evaluate_output_target`, `visible = false`) and resuming it
/// (`resume_pane_with_permit`) must leave the cursor position, scroll
/// region and visible rows exactly as they were before the hide. Restored
/// from commit 20c4cd9c, lost when task0003 was re-implemented through
/// parent-side adoption (06bf0714) — see
/// `feature-docs/mux-suppressed-output-fixes/DECISIONS.md`
/// (`830f950f39e499fa`).
#[test]
fn visibility_resume_keeps_main_pane_progress_bar_layout() {
    use term_core::terminal_core::TerminalCore;

    let cols: u16 = 80;
    let rows: u16 = 24;

    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let pane = MuxPane::new_test(35, cols, rows, target.clone());

    let mut stream = Vec::new();
    for i in 0..20u32 {
        stream.extend_from_slice(format!("apt line {i}\r\n").as_bytes());
    }
    stream.extend_from_slice(b"\x1b[1;23r"); // narrow the scroll region, leaving the last row free
    stream.extend_from_slice(b"\x1b7"); // save cursor (DECSC)
    stream.extend_from_slice(b"\x1b[24;1H99%"); // draw the progress bar on the last row
    stream.extend_from_slice(b"\x1b8"); // restore cursor (DECRC)

    let handle = spawn_reader_thread(&pane, vec![stream.clone()]);
    handle.join().unwrap();
    while rx.try_recv().is_ok() {} // drain the forwarded live chunk + EOF

    // Reference: an independent client model fed the same raw stream
    // once — the pre-hide values a resumed client model must still match.
    let mut reference = TerminalCore::new(cols, rows, 10_000);
    reference.process_pty_data_fully(&stream);

    let eval = evaluate_output_target(&pane, false, false, &owned_tx);
    assert!(matches!(eval, EvalResult::SwitchedToDetached));

    let permit = owned_tx
        .try_reserve()
        .expect("S has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx.try_recv().expect("snapshot enqueued");
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
    assert!(rx.try_recv().is_err(), "nothing else must be delivered");

    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
    let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = segments
        .iter()
        .map(|s| term_core::terminal_core::ReplaySegment {
            offset: s.offset,
            cols: s.cols,
            rows: s.rows,
        })
        .collect();
    let mut client = TerminalCore::new(cols, rows, 10_000);
    client.reset_and_replay_segments(content, &replay_segments);

    assert_eq!(
        client.get_cursor_row(),
        reference.get_cursor_row(),
        "AC-2: cursor row must survive the hide/resume cycle"
    );
    assert_eq!(
        client.get_cursor_col(),
        reference.get_cursor_col(),
        "AC-2: cursor col must survive the hide/resume cycle"
    );
    assert_eq!(
        client.get_scroll_region_top(),
        reference.get_scroll_region_top(),
        "AC-2: scroll region top must survive the hide/resume cycle"
    );
    assert_eq!(
        client.get_scroll_region_bottom(),
        reference.get_scroll_region_bottom(),
        "AC-2: scroll region bottom must survive the hide/resume cycle"
    );
    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "row {r} mismatch"
        );
    }

    // Behavioral corroboration (AC-2's alternative check): one LF at the
    // scroll region's bottom margin must scroll only the rows INSIDE the
    // region, identically in the resumed model and the reference — proves
    // the region is not merely reported correctly but ACTS correctly.
    let lf_at_bottom_margin = b"\x1b[23;1H\n";
    client.process_pty_data_fully(lf_at_bottom_margin);
    reference.process_pty_data_fully(lf_at_bottom_margin);
    for r in 0..rows {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "post-LF row {r} mismatch: the scroll region must act \
             identically in the resumed model and the reference"
        );
    }
}

/// D3'''' (round-7 rework, review round-6 finding `46c29c2c65970d26`):
/// settles the reachability question the round-6 reviewers disagreed
/// on — does an oversize resume failure freeze the pane permanently
/// (`Detached { HiddenByVisibility }` forever), or does a later
/// visibility cycle re-drive a successful resume once the oversize
/// condition clears?
///
/// `resume_pane_with_permit`'s oversize branch returns `NoChange`
/// WITHOUT touching `*target` or `*reason` at all (see its body: the
/// early return happens before any assignment) — the pane is left in
/// EXACTLY the state it was in before the attempt. `handle_set_visibility`
/// is the only production caller, and it re-invokes
/// `resume_pane_with_permit` for every non-exited pane on EVERY
/// `visible -> true` edge it does not short-circuit as a no-op (its
/// `prev == visible` guard only suppresses a REPEATED `true` with no
/// intervening `false`) — so a hide -> show cycle (a connection
/// toggling visibility false then true again, e.g. the client
/// minimizing and restoring the window) unconditionally retries this
/// exact call. This test proves the retry actually recovers: the first
/// call (oversize) leaves the pane detached, and a second call — after
/// the condition that caused the oversize snapshot clears (mirroring
/// what a later resize or scrollback eviction does in production) —
/// resumes cleanly. The pane is therefore never left detached with
/// visibility latched on FOREVER: recovery is reachable via the next
/// visibility toggle, without any state-machine change being required.
#[tokio::test]
async fn resume_pane_with_permit_recovers_after_oversize_condition_clears() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(13, 80, 24, target.clone());
    let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
    pane.scrollback
        .lock()
        .unwrap()
        .write(&vec![b'x'; oversize_capacity]);

    // First attempt: oversize, must stay detached (same assertion as
    // `test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit`).
    let permit = owned_tx.reserve().await.expect("reserve permit");
    let first_outcome =
        resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(
        matches!(first_outcome, ResumeOutcome::NoChange),
        "first (oversize) attempt must not resume the pane"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "pane must stay Detached after the oversize attempt"
    );
    assert!(
        rx.try_recv().is_err(),
        "no snapshot may reach the channel on the oversize attempt"
    );

    // The condition that caused the oversize snapshot clears (e.g. a
    // later resize / scrollback eviction shrinks it back under the
    // frame limit) — the pane's own `output_target` was NEVER touched
    // by the failed attempt above, so it is still exactly
    // `Detached { HiddenByVisibility, owner }`.
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(4096);
    pane.scrollback.lock().unwrap().write(b"small content now");

    // Second attempt (what a hide -> show cycle re-drives): must
    // resume cleanly.
    let permit = owned_tx.reserve().await.expect("reserve permit");
    let second_outcome =
        resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(
        matches!(second_outcome, ResumeOutcome::Resumed),
        "the retry must resume the pane once the oversize condition \
         has cleared — the pane must never stay detached forever"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Connected(_)),
        "pane must be Connected after the retry succeeds"
    );
    assert!(
        rx.try_recv().is_ok(),
        "the retry must enqueue a snapshot chunk"
    );
}

#[test]
fn test_detach_reason_combine() {
    assert_eq!(
        DetachReason::combine(
            DetachReason::NetworkDetach,
            DetachReason::HiddenByVisibility
        ),
        DetachReason::Both
    );
    assert_eq!(
        DetachReason::combine(DetachReason::NetworkDetach, DetachReason::NetworkDetach),
        DetachReason::NetworkDetach
    );
    assert_eq!(
        DetachReason::combine(DetachReason::Both, DetachReason::HiddenByVisibility),
        DetachReason::Both
    );
}

#[test]
fn test_detach_reason_clear_bits() {
    assert_eq!(DetachReason::NetworkDetach.clear_network(), None);
    assert_eq!(
        DetachReason::Both.clear_network(),
        Some(DetachReason::HiddenByVisibility)
    );
    assert_eq!(
        DetachReason::HiddenByVisibility.clear_network(),
        Some(DetachReason::HiddenByVisibility)
    );
    assert_eq!(DetachReason::HiddenByVisibility.clear_hidden(), None);
    assert_eq!(
        DetachReason::Both.clear_hidden(),
        Some(DetachReason::NetworkDetach)
    );
}

/// Regression for the vt100 0.15 panic that poisoned the shadow parser:
/// a saved cursor (DECSC) outside the grid after a shrink resize was
/// restored (DECRC) unclamped, and the next wide-character write hit an
/// out-of-bounds `drawing_cell(pos).unwrap()`. vt100 0.16 clamps
/// `saved_pos` in `set_size`, so this sequence must not panic.
#[test]
fn test_shadow_parser_survives_decrc_after_shrink_resize() {
    let mut parser = new_shadow_parser(24, 80);
    // Park the cursor near the bottom-right corner and save it (DECSC),
    // with wide characters at the edge.
    parser.process("\x1b[24;75Hあああ\x1b7".as_bytes());
    // Shrink the grid, restore the saved cursor (DECRC), then write
    // wide characters again.
    parser.screen_mut().set_size(10, 20);
    parser.process("\x1b8ああああああ".as_bytes());
    let (rows, cols) = parser.screen().size();
    assert_eq!((rows, cols), (10, 20));
}

/// OSC 0 / OSC 2 titles must surface through the TitleSink callback
/// (vt100 0.16 removed `Screen::title()`).
#[test]
fn test_title_sink_reports_osc_titles() {
    let mut parser = new_shadow_parser(24, 80);
    assert_eq!(parser.callbacks_mut().take_title(), None);
    parser.process(b"\x1b]0;from-osc-0\x07");
    assert_eq!(
        parser.callbacks_mut().take_title().as_deref(),
        Some("from-osc-0")
    );
    // Drained after take.
    assert_eq!(parser.callbacks_mut().take_title(), None);
    parser.process(b"\x1b]2;from-osc-2\x07");
    assert_eq!(
        parser.callbacks_mut().take_title().as_deref(),
        Some("from-osc-2")
    );
}

/// Poison the shadow parser mutex by panicking while holding the lock.
fn poison_shadow_parser(pane: &MuxPane) {
    let parser = pane.shadow_parser.clone();
    let _ = std::thread::spawn(move || {
        let _guard = parser.lock().unwrap();
        panic!("intentional poison");
    })
    .join();
    assert!(pane.shadow_parser.lock().is_err(), "mutex must be poisoned");
}

/// A poisoned shadow parser mutex must not panic the caller; the guard
/// is recovered and the parser stays usable.
#[test]
fn test_lock_shadow_parser_recovers_from_poison() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx)));
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.shadow_parser.lock().unwrap().process(b"before-poison");
    poison_shadow_parser(&pane);

    let parser = lock_shadow_parser(&pane.shadow_parser);
    let contents = parser.screen().contents();
    assert!(contents.contains("before-poison"));
}

/// AC-4 (FR10; TS-12; task0004): the production visibility resume must
/// still produce a snapshot after the shadow parser mutex was poisoned by
/// a reader-thread panic. Ported from
/// `test_evaluate_output_target_survives_poisoned_shadow_parser` (deleted:
/// it asserted the now-removed `EvalResult::ResumeWithSnapshot` variant)
/// onto the production `resume_pane_with_permit` path.
///
/// The main-buffer pane drops the shadow slice (same main/alt split as
/// `build_resume_snapshot_bytes`), so we feed scrollback bytes instead
/// and assert those survive the poisoned lock.
#[test]
fn resume_pane_with_permit_recovers_a_poisoned_shadow_parser() {
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(16, 80, 24, target.clone());
    pane.scrollback.lock().unwrap().write(b"ring-bytes-x");
    pane.shadow_parser.lock().unwrap().process(b"shadow-data");
    poison_shadow_parser(&pane);

    let permit = owned_tx
        .try_reserve()
        .expect("channel has capacity for the resume permit");
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));

    let chunk = rx
        .try_recv()
        .expect("snapshot enqueued despite the poisoned shadow lock");
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
    assert!(
        chunk
            .data
            .windows(b"ring-bytes-x".len())
            .any(|w| w == b"ring-bytes-x"),
        "snapshot must include scrollback even after poisoned shadow lock"
    );
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
}

// ── task0003: snapshot accessors + restore constructors ───────────────

/// A child double reporting a fixed, non-`None` process id — used to
/// exercise [`MuxPane::child_pid`] (the `PaneChild::Owned` arm) without
/// a real spawned process.
#[derive(Debug)]
struct FixedPidChild(u32);

impl portable_pty::ChildKiller for FixedPidChild {
    fn kill(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        unimplemented!("not exercised by these tests")
    }
}

impl portable_pty::Child for FixedPidChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        Ok(portable_pty::ExitStatus::with_exit_code(0))
    }
    fn process_id(&self) -> Option<u32> {
        Some(self.0)
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

fn open_test_pty_pair() -> portable_pty::PtyPair {
    let pty_system = portable_pty::native_pty_system();
    let size = portable_pty::PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };
    pty_system.openpty(size).unwrap()
}

/// AC-2 (snapshot groundwork): `master_raw_fd` reports the SAME fd
/// number the underlying PTY master actually has.
#[cfg(unix)]
#[test]
fn master_raw_fd_reports_the_ptys_actual_descriptor_number() {
    let pair = open_test_pty_pair();
    let expected_fd = pair.master.as_raw_fd().expect("PTY master must have an fd");
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new(1, 80, 24, target, writer, pair.master, None);
    assert_eq!(pane.master_raw_fd(), Some(expected_fd));
}

/// AC-2 (snapshot groundwork): `master_raw_fd` / `child_pid` both
/// become `None` once the pane has exited (master dropped, child
/// reaped) — an exited pane contributes no descriptor to snapshot.
#[cfg(unix)]
#[test]
fn master_raw_fd_and_child_pid_are_none_after_mark_exited() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut pane = MuxPane::new(
        1,
        80,
        24,
        target,
        writer,
        pair.master,
        Some(Box::new(FixedPidChild(4242))),
    );
    assert_eq!(pane.child_pid(), Some(4242));
    pane.mark_exited();
    assert_eq!(pane.master_raw_fd(), None);
    assert_eq!(pane.child_pid(), None);
}

/// AC-2 (snapshot groundwork): `child_pid` reports the owned child
/// double's process id verbatim (the `PaneChild::Owned` arm).
#[cfg(unix)]
#[test]
fn child_pid_reports_the_owned_childs_process_id() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new(
        1,
        80,
        24,
        target,
        writer,
        pair.master,
        Some(Box::new(FixedPidChild(777))),
    );
    assert_eq!(pane.child_pid(), Some(777));
}

/// AC-2 (snapshot groundwork): `child_pid` reports a restored pane's
/// bare process id verbatim (the `PaneChild::ProcessId` arm — task0007
/// IMPLEMENTATION.md D6).
#[cfg(unix)]
#[test]
fn child_pid_reports_a_restored_panes_process_id() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let pane = MuxPane::new_with_process_id(1, 80, 24, target, writer, pair.master, 5150);
    assert_eq!(pane.child_pid(), Some(5150));
}

/// AC-1/AC-4: `from_restored` sets cols/rows/cwd/title/agent-status and
/// scrollback verbatim, and carries the given restored pid through the
/// `PaneChild::ProcessId` path (task0007's reaping wiring).
#[cfg(unix)]
#[test]
fn from_restored_sets_attributes_and_scrollback_verbatim() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
    scrollback.write(b"restored scrollback bytes");
    let mut agent_status = AgentStatus::default();
    agent_status.state = Some(AgentState::Working);
    agent_status.name = Some("claude".to_string());
    agent_status.revision = 3;

    let pane = MuxPane::from_restored(
        9,
        80,
        24,
        target,
        writer,
        pair.master,
        scrollback,
        Some("/home/user/project".to_string()),
        Some("zsh".to_string()),
        agent_status,
        Some(4242),
        false,
        Vec::new(),
    );

    assert_eq!(pane.id, 9);
    assert_eq!((pane.cols, pane.rows), (80, 24));
    assert!(!pane.exited);
    assert_eq!(
        pane.child_pid(),
        Some(4242),
        "restored pid flows through PaneChild::ProcessId"
    );
    assert_eq!(
        *pane.cwd.lock().unwrap(),
        Some("/home/user/project".to_string())
    );
    assert_eq!(*pane.title.lock().unwrap(), Some("zsh".to_string()));
    {
        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, Some(AgentState::Working));
        assert_eq!(status.name.as_deref(), Some("claude"));
        assert_eq!(status.revision, 3);
    }
    assert_eq!(
        pane.scrollback.lock().unwrap().read_all(),
        b"restored scrollback bytes"
    );
    // AC-6: flag false must behave byte-identically to today — no
    // extra alt-screen-enter sequence is fed, so the parser must never
    // report the alternate screen as active.
    assert!(
        !pane
            .shadow_parser
            .lock()
            .unwrap()
            .screen()
            .alternate_screen(),
        "AC-6: from_restored with alt_screen=false must not activate the alternate screen"
    );
}

/// AC-5: `from_restored` with `alt_screen=true` feeds the
/// alternate-screen-enter sequence plus the dump into the shadow parser
/// AFTER the scrollback replay, so the parser reports the alternate
/// screen active with the dump's content visible, while the replayed
/// scrollback survives underneath on the main buffer (revealed by
/// leaving the alt screen).
#[cfg(unix)]
#[test]
fn from_restored_with_alt_screen_true_replays_dump_with_scrollback_beneath() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
    scrollback.write(b"pre-alt scrollback line");

    let pane = MuxPane::from_restored(
        3,
        80,
        24,
        target,
        writer,
        pair.master,
        scrollback,
        None,
        None,
        AgentStatus::default(),
        None,
        true,
        b"ALT-DUMP-CONTENT".to_vec(),
    );

    {
        let parser = pane.shadow_parser.lock().unwrap();
        assert!(
            parser.screen().alternate_screen(),
            "AC-5: restore with alt_screen=true must report the alternate screen active"
        );
        let content = parser.screen().contents_formatted();
        assert!(
            content
                .windows(b"ALT-DUMP-CONTENT".len())
                .any(|w| w == b"ALT-DUMP-CONTENT"),
            "AC-5: the dump's content must be visible on the alt screen"
        );
    }

    // Leaving the alt screen (as a live reattach eventually would, or a
    // program exiting its TUI) must reveal the scrollback-replayed main
    // buffer beneath it, proving the two replays targeted separate
    // buffers exactly like a live pane's real ESC[?1049h/l pair would.
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049l");
    let main_screen = pane.shadow_parser.lock().unwrap();
    assert!(!main_screen.screen().alternate_screen());
    let main_content = main_screen.screen().contents_formatted();
    assert!(
        main_content
            .windows(b"pre-alt scrollback line".len())
            .any(|w| w == b"pre-alt scrollback line"),
        "AC-5: the replayed scrollback must still be present on the main screen \
         beneath the alt overlay"
    );
}

/// AC-6 (continued): `alt_screen=true` with an EMPTY dump (the D1
/// overflow shape) must still activate the alternate screen — just with
/// blank contents, since only the dump content degrades, never the
/// mode flag.
#[cfg(unix)]
#[test]
fn from_restored_with_alt_screen_true_and_empty_dump_yields_blank_active_alt_screen() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();

    let pane = MuxPane::from_restored(
        4,
        80,
        24,
        target,
        writer,
        pair.master,
        ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY),
        None,
        None,
        AgentStatus::default(),
        None,
        true,
        Vec::new(),
    );

    let parser = pane.shadow_parser.lock().unwrap();
    assert!(
        parser.screen().alternate_screen(),
        "AC-6: alt_screen=true with an empty dump must still activate the alternate screen"
    );
    assert!(
        parser.screen().contents().trim().is_empty(),
        "AC-6: an empty dump must yield a blank alternate screen"
    );
}

/// AC-3: `capture_alt_state` on a main-buffer pane (shadow parser never
/// entered the alternate screen) records flag false and an empty dump.
#[test]
fn capture_alt_state_on_main_buffer_pane_returns_false_and_empty_dump() {
    let target = make_output_target();
    let pane = MuxPane::new_test(1, 80, 24, target);
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"plain main-buffer content");

    let (alt, dump) = pane.capture_alt_state();

    assert!(!alt, "AC-3: a main-buffer pane must record flag false");
    assert!(
        dump.is_empty(),
        "AC-3: a main-buffer pane must record an empty dump"
    );
}

/// AC-3: `capture_alt_state` on an alt-screen pane records flag true and
/// a dump equal to the parser's formatted alt-screen contents.
#[test]
fn capture_alt_state_on_alt_screen_pane_returns_true_and_the_formatted_alt_contents() {
    let target = make_output_target();
    let pane = MuxPane::new_test(2, 80, 24, target);
    pane.shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    pane.shadow_parser
        .lock()
        .unwrap()
        .process(b"ALT-SCREEN-CONTENT");

    let (alt, dump) = pane.capture_alt_state();

    assert!(alt, "AC-3: an alt-screen pane must record flag true");
    assert!(
        dump.windows(b"ALT-SCREEN-CONTENT".len())
            .any(|w| w == b"ALT-SCREEN-CONTENT"),
        "AC-3: the dump must contain the alt-screen content"
    );
    let expected = pane
        .shadow_parser
        .lock()
        .unwrap()
        .screen()
        .contents_formatted();
    assert_eq!(
        dump, expected,
        "AC-3: the dump must equal the parser's formatted alt-screen contents"
    );
}

/// AC-7: a dump AT the D1 cap (`MAX_SNAPSHOT_FRAME_PAYLOAD`) is stored
/// untouched. Exercises the real boundary (task plan Test Notes AC-7):
/// `cap_alt_screen_dump` takes a plain byte vector, so testing at the
/// real cap needs no vt100 screen at an unreasonable size — just an
/// allocation.
#[test]
fn cap_alt_screen_dump_returns_the_dump_untouched_at_the_cap() {
    let dump = vec![0xABu8; mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD];
    let result = cap_alt_screen_dump(1, dump.clone());
    assert_eq!(
        result, dump,
        "AC-7: a dump at the cap must be stored untouched"
    );
}

/// AC-7 (continued): a dump exceeding the D1 cap by a single byte is
/// replaced with an empty one (flag preservation is the caller's
/// concern — `capture_alt_state` keeps `alt_screen=true` regardless of
/// this function's outcome). The "warn-level log line naming the pane
/// id and the oversize length" half of AC-7 is verified by inspection
/// of the `log::warn!` call in `cap_alt_screen_dump` itself (matching
/// this project's established convention for asserting on log output —
/// see `mux::upgrade::tests::restore_handles_exited_and_unadoptable_panes_while_the_rest_of_the_tree_still_restores`
/// for the equivalent precedent).
#[test]
fn cap_alt_screen_dump_returns_empty_when_the_dump_exceeds_the_cap() {
    let dump = vec![0xABu8; mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1];
    let result = cap_alt_screen_dump(42, dump);
    assert!(
        result.is_empty(),
        "AC-7: a dump exceeding the D1 cap must be replaced with an empty one"
    );
}

/// AC-5: a restored live pane can be written to and read from through
/// its adopted master, demonstrated against a real PTY pair. The PTY's
/// line discipline echoes input written to the master back to the
/// master's own reader side, so a reader cloned from the master BEFORE
/// it is handed to `from_restored` observes the written bytes.
#[cfg(unix)]
#[test]
fn from_restored_pane_can_write_and_read_through_its_adopted_master() {
    use std::io::Read as _;
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let mut master_reader = pair
        .master
        .try_clone_reader()
        .expect("master must support a reader clone");
    let target = make_output_target();
    let pane = MuxPane::from_restored(
        1,
        80,
        24,
        target,
        writer,
        pair.master,
        ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY),
        None,
        None,
        AgentStatus::default(),
        None,
        false,
        Vec::new(),
    );

    pane.write_input(b"restored-write\n").unwrap();

    let mut buf = [0u8; 64];
    let n = master_reader
        .read(&mut buf)
        .expect("master read must succeed");
    assert!(
        buf[..n]
            .windows(b"restored-write".len())
            .any(|w| w == b"restored-write"),
        "bytes written through the adopted master's writer must be readable \
         back through the adopted master (echoed by the PTY line discipline)"
    );
}

/// mux-vt100-del-closing AC-3 (FR2, FR5, NFR1): the restored shadow screen
/// shows the text after a closed CSI parameter (the DEL in the ring is handed
/// to the shadow parser as CAN), while the pane's ring keeps the DEL.
#[cfg(unix)]
#[test]
fn from_restored_replays_a_closed_csi_into_the_shadow_and_keeps_the_ring_bytes() {
    let pair = open_test_pty_pair();
    let writer = pair.master.take_writer().unwrap();
    let target = make_output_target();
    let ring_bytes: &[u8] = b"\x1b[6\x7fHello";
    let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
    scrollback.write(ring_bytes);

    let pane = MuxPane::from_restored(
        4,
        80,
        24,
        target,
        writer,
        pair.master,
        scrollback,
        None,
        None,
        AgentStatus::default(),
        None,
        false,
        Vec::new(),
    );

    let row0 = {
        let parser = pane.shadow_parser.lock().unwrap();
        parser.screen().rows(0, 80).next().unwrap()
    };
    assert_eq!(row0, "Hello");
    assert_eq!(
        pane.scrollback.lock().unwrap().read_all(),
        ring_bytes,
        "the ring keeps the DEL (NFR1)"
    );
}

/// AC-6: `from_restored_exited` builds an already-exited pane that
/// adopts no descriptor, while still restoring its non-descriptor
/// attributes (cwd/title/agent-status/scrollback) verbatim.
#[test]
fn from_restored_exited_adopts_no_descriptor_and_is_marked_exited() {
    let target = make_output_target();
    let mut scrollback = ScrollbackRingBuffer::new(DEFAULT_SCROLLBACK_CAPACITY);
    scrollback.write(b"pre-exit scrollback");
    let pane = MuxPane::from_restored_exited(
        5,
        80,
        24,
        target,
        scrollback,
        Some("/tmp".to_string()),
        Some("bash".to_string()),
        AgentStatus::default(),
    );

    assert!(pane.exited);
    assert_eq!(pane.child_pid(), None);
    #[cfg(unix)]
    assert_eq!(pane.master_raw_fd(), None);
    assert_eq!(*pane.cwd.lock().unwrap(), Some("/tmp".to_string()));
    assert_eq!(*pane.title.lock().unwrap(), Some("bash".to_string()));
    assert_eq!(
        pane.scrollback.lock().unwrap().read_all(),
        b"pre-exit scrollback"
    );
    // Writing to an exited pane must fail (no writer).
    assert!(pane.write_input(b"x").is_err());
}

// ── enqueue_pane_output_chunk (mux-window-switch-output-hang task0001,
// reworked task0002) ──
//
// AC-1/AC-2/AC-3: the fix's core mechanism. `enqueue_pane_output_chunk`
// is deliberately a plain `fn` (not `async fn`), so it structurally
// cannot suspend the calling task on channel capacity — the tests below
// pin the OBSERVABLE behavior on top of that structural guarantee: the
// fast path delivers synchronously, the slow path still returns without
// blocking and defers into the connection-owned `DeferredOutputQueue`
// rather than sending, a closed channel is handled without a panic (no
// new unhandled error path), and the deferred queue itself is bounded
// (task0002 AC-3/AC-4).

/// AC-3 (fast path): with room in the channel, the chunk is enqueued
/// synchronously — no deferral.
#[test]
fn enqueue_pane_output_chunk_fast_path_delivers_synchronously() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);
    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::pty_output(1, b"hi".to_vec()),
        &mut deferred,
        None,
    );
    let chunk = rx.try_recv().expect("fast path must deliver synchronously");
    assert_eq!(chunk.data, b"hi");
    assert!(deferred.is_empty(), "fast path must not defer anything");
}

/// AC-1/AC-3 (task0002 rework): with the channel completely full,
/// `enqueue_pane_output_chunk` must still return immediately (this IS
/// the self-deadlock fix) by pushing the chunk onto `deferred` instead
/// of sending it.
///
/// task0003 rework (AC-5, review round 2 finding `6574d4221dcb5efe`):
/// this test used to also hand-roll a `while let Some(item) = pop_front()
/// { ... tx.try_send(...) ... }` loop here to "prove" FIFO delivery —
/// that copy could diverge from (and did diverge from — it never
/// exercised the `Full`-requeue or `Closed`-clear arms) the production
/// `handlers::flush_deferred_output`. This module cannot call that
/// `pub(super)` function (it lives in `mux::ipc`, a different module
/// tree), so the flush-side proof now lives in
/// `mux::ipc::handlers::tests` instead, calling the production function
/// directly — see `handle_request_pane_snapshot_returns_promptly_when_own_pane_channel_full`
/// and the dedicated `flush_deferred_output_*` tests there. This test is
/// trimmed to what THIS module owns: that the enqueue itself defers
/// without blocking.
#[tokio::test]
async fn enqueue_pane_output_chunk_full_channel_defers_without_blocking() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);
    tx.send(PtyOutputChunk::pty_output(1, b"a".to_vec()))
        .await
        .unwrap();
    tx.send(PtyOutputChunk::pty_output(1, b"b".to_vec()))
        .await
        .unwrap();
    assert!(
        tx.try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let mut deferred = DeferredOutputQueue::new();
    // This call must return immediately even though the channel is full
    // — it is a plain (non-async) function call, so there is no `.await`
    // point where it could suspend the test task either.
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::snapshot(1, b"SNAP".to_vec()),
        &mut deferred,
        None,
    );
    assert_eq!(
        deferred.len(),
        1,
        "full channel must defer, not send, the chunk"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk, _)) => {
            assert_eq!(chunk.pane_id, 1);
            assert_eq!(chunk.kind, ChunkKind::Snapshot);
            assert_eq!(chunk.data, b"SNAP");
        }
        other => panic!("expected a deferred Chunk, got {other:?}"),
    }

    // The two pre-existing chunks are still exactly as sent — enqueueing
    // never touched the channel's own contents.
    let c1 = rx.recv().await.expect("chunk a");
    assert_eq!(c1.data, b"a");
    let c2 = rx.recv().await.expect("chunk b");
    assert_eq!(c2.data, b"b");
}

/// A closed channel (client gone) is handled the same way the
/// pre-existing blocking-send call sites handled it: logged and
/// dropped, never a panic, and nothing is deferred (retrying a send that
/// can only ever fail the same way would be pointless).
#[test]
fn enqueue_pane_output_chunk_closed_channel_does_not_panic() {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(1);
    drop(rx);
    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::pty_output(1, b"x".to_vec()),
        &mut deferred,
        None,
    );
    // Reaching here without a panic is the assertion.
    assert!(deferred.is_empty(), "a closed channel is not retried");
}

/// AC-4: `enqueue_pane_output_chunk` no longer spawns a task on its Full
/// branch (task0002 rework), so it no longer depends on an active tokio
/// runtime at all. This is a plain `#[test]` (deliberately NOT
/// `#[tokio::test]` — there is no tokio runtime running here) hitting
/// the Full branch directly: it must not panic, and the chunk must land
/// in `deferred` exactly as it would inside a runtime.
#[test]
fn enqueue_pane_output_chunk_full_branch_does_not_panic_outside_tokio_runtime() {
    let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
    tx.try_send(PtyOutputChunk::pty_output(1, b"filler".to_vec()))
        .expect("fill the single slot");
    assert!(
        tx.try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
            .is_err()
    );

    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::pty_output(1, b"x".to_vec()),
        &mut deferred,
        None,
    );
    // Reaching here without a panic (no tokio runtime, no `Handle`
    // available) is the assertion.
    assert_eq!(deferred.len(), 1);
}

/// AC-4: this fix must not replace the bounded channel with an
/// unconditionally-growing one. Pins the capacity constant so a future
/// change to an unbounded mechanism is caught here.
#[test]
fn pty_channel_capacity_is_finite_and_unchanged() {
    assert_eq!(PTY_CHANNEL_CAPACITY, 256);
}

/// AC-2 (task0003 rework, review round 2 findings `4999311c8becf7eb`/
/// `ac1d20218d320b08`): a repeated chunk for the SAME pane coalesces —
/// the newer payload replaces the older one in place rather than
/// growing the queue, and the survivor is the newest content (never the
/// newest dropped in favour of the older one).
#[test]
fn deferred_output_queue_coalesces_repeated_chunk_for_same_pane_newest_wins() {
    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V1".to_vec()), None);
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V2".to_vec()), None);
    assert_eq!(
        deferred.len(),
        1,
        "a second chunk for the same pane must coalesce, not add a second entry"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk, _)) => {
            assert_eq!(
                chunk.data, b"V2",
                "the newest payload for the pane must survive"
            );
        }
        other => panic!("expected a Chunk, got {other:?}"),
    }
}

/// AC-5 (mux-window-switch-output-hang task0004 rework, review round 3
/// finding `0830abe1c16ad0fb`): coalescing a repeated chunk for the SAME
/// pane must preserve its QUEUE POSITION, not move it to the tail. With
/// `[Chunk(pane 1), VisibilityResume(pane 1)]` queued, a second
/// `RequestPaneSnapshot` for pane 1 must coalesce the `Chunk` IN PLACE
/// (still first), NOT reorder into `[VisibilityResume, Chunk]` — the
/// pre-fix `remove` + `push_back` behavior, which would let a stale,
/// already-built `Chunk` overtake and overwrite a `VisibilityResume`'s
/// fresher flush-time-built snapshot on the wire.
#[test]
fn deferred_output_queue_coalesce_preserves_position_ahead_of_a_later_visibility_resume() {
    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"first".to_vec()), None);
    deferred.defer_visibility_resume(1);
    assert_eq!(deferred.len(), 2);

    // Second RequestPaneSnapshot for the SAME pane while both entries
    // are still queued: must coalesce the Chunk IN PLACE, not move it
    // to the tail.
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"second".to_vec()), None);
    assert_eq!(
        deferred.len(),
        2,
        "coalescing must not grow the queue past its pre-coalesce length"
    );

    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk, _)) => {
            assert_eq!(chunk.data, b"second", "the newest payload must survive");
        }
        other => panic!(
            "expected the coalesced Chunk to remain FIRST (position preserved), got {other:?}"
        ),
    }
    match deferred.pop_front() {
        Some(DeferredOutputItem::VisibilityResume(pane_id)) => assert_eq!(pane_id, 1),
        other => panic!("expected the VisibilityResume to remain SECOND, got {other:?}"),
    }
    assert!(deferred.pop_front().is_none());
}

/// AC-2 (mux-window-switch-output-hang task0006 rework, review round 5
/// high findings `4043ee676f69ca15` / `1c8d86389ab4bf40`): the REVERSE
/// order from the pinned test above. `[VisibilityResume(1)]` queued
/// FIRST (the pane was resumed from hidden while the channel was full),
/// THEN a `RequestPaneSnapshot` for the SAME pane arrives and defers a
/// `Chunk` — since no `Chunk` entry exists yet for pane 1, this must
/// INSERT the new Chunk immediately BEFORE the queued Resume, producing
/// `[Chunk(1), VisibilityResume(1)]`, rather than dropping it
/// (task0005's now-reverted fix) or appending it after the Resume. This
/// ordering still yields newest-wins when the Resume's flush actually
/// produces a fresher snapshot (the Resume's flush-time-built content
/// lands LAST), while guaranteeing the `RequestPaneSnapshot` still gets
/// answered when the Resume no-ops at flush time (pane already
/// `Connected`, owner mismatch, a surviving `NetworkDetach` bit, or an
/// oversize snapshot — see `defer_chunk`'s own doc) — the NORMAL case,
/// since `handle_set_visibility` queues a Resume for every non-exited
/// pane on a visible edge without checking whether it is actually
/// detached-hidden. Delivery itself (the client still receiving a
/// snapshot when the Resume no-ops) is exercised end-to-end by
/// `mux::ipc::handlers::tests::flush_deferred_output_delivers_chunk_even_when_its_queued_visibility_resume_no_ops`
/// — this queue-level test only pins the ORDERING (AC-2), since
/// `DeferredOutputQueue` alone has no flush machinery or client channel
/// to observe delivery through.
#[test]
fn deferred_output_queue_inserts_chunk_immediately_before_queued_visibility_resume_for_same_pane() {
    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);
    assert_eq!(deferred.len(), 1);

    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"pending".to_vec()), None);
    assert_eq!(
        deferred.len(),
        2,
        "the Chunk must be INSERTED alongside the already-queued VisibilityResume, \
         not dropped — a dropped Chunk here means the client's RequestPaneSnapshot \
         gets no reply at all whenever the Resume later no-ops"
    );

    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk, _)) => {
            assert_eq!(
                chunk.data, b"pending",
                "the newly-deferred Chunk must survive"
            );
        }
        other => panic!(
            "expected the Chunk to be queued FIRST, immediately before the \
             VisibilityResume, got {other:?}"
        ),
    }
    match deferred.pop_front() {
        Some(DeferredOutputItem::VisibilityResume(pane_id)) => assert_eq!(pane_id, 1),
        other => panic!(
            "expected the VisibilityResume to remain queued SECOND (not dropped, not \
             overtaken), got {other:?}"
        ),
    }
    assert!(deferred.pop_front().is_none());
}

/// AC-2: once coalescing still leaves more than `MAX_DEFERRED_ITEMS`
/// DISTINCT panes' chunks queued, the OLDEST surviving chunk is evicted
/// — never the one just pushed (AC-2 forbids ever dropping the newest).
#[test]
fn deferred_output_queue_drops_oldest_distinct_pane_chunk_past_the_cap_never_the_newest() {
    let mut deferred = DeferredOutputQueue::new();
    let total = MAX_DEFERRED_ITEMS * 2;
    for pane_id in 0..(total as u32) {
        deferred.defer_chunk(
            PtyOutputChunk::pty_output(pane_id, vec![pane_id as u8]),
            None,
        );
    }
    assert_eq!(
        deferred.len(),
        MAX_DEFERRED_ITEMS,
        "queue must never grow past the documented cap"
    );

    let mut surviving_pane_ids = Vec::new();
    while let Some(item) = deferred.pop_front() {
        match item {
            DeferredOutputItem::Chunk(chunk, _) => surviving_pane_ids.push(chunk.pane_id),
            other => panic!("expected only Chunk items, got {other:?}"),
        }
    }
    let expected: Vec<u32> = ((total - MAX_DEFERRED_ITEMS) as u32..total as u32).collect();
    assert_eq!(
        surviving_pane_ids, expected,
        "the oldest distinct-pane chunks must be evicted first, so only the \
         most-recently-deferred MAX_DEFERRED_ITEMS panes survive — including \
         the very last (newest) one pushed"
    );
}

/// AC-1 (task0003 rework, review round 2 findings `4999311c8becf7eb`/
/// `ff58ab6fd17542f4`/`1d648d947b4dea8b`): visibility resumes are no
/// longer subject to `MAX_DEFERRED_ITEMS` — a session with more
/// non-exited panes than the (former, now chunk-only) cap must not
/// strand any of them.
#[test]
fn deferred_output_queue_never_drops_visibility_resumes_past_the_former_cap() {
    let mut deferred = DeferredOutputQueue::new();
    let total = MAX_DEFERRED_ITEMS * 2 + 3;
    for pane_id in 0..(total as u32) {
        deferred.defer_visibility_resume(pane_id);
    }
    assert_eq!(
        deferred.len(),
        total,
        "distinct-pane visibility resumes must never be dropped for capacity"
    );
}

/// AC-1: a repeated resume request for the SAME pane deduplicates
/// instead of growing the queue.
#[test]
fn deferred_output_queue_dedupes_repeated_visibility_resume_for_same_pane() {
    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(42);
    deferred.defer_visibility_resume(42);
    deferred.defer_visibility_resume(42);
    assert_eq!(
        deferred.len(),
        1,
        "repeated resume requests for the same pane must deduplicate"
    );
}

// ── mux-suppressed-output-round2-fixes task0004 (FR8): the per-destination
//    boundary record carries the trailing construct of the covering
//    snapshot ────────────────────────────────────────────────────────────

const ESC_BYTE: u8 = 0x1b;

fn construct_channel() -> (mpsc::Sender<PtyOutputChunk>, mpsc::Receiver<PtyOutputChunk>) {
    mpsc::channel(8)
}

fn cover_construct(
    capture: &OutputCapture,
    tx: &mpsc::Sender<PtyOutputChunk>,
    number: u64,
) -> Option<Option<Vec<u8>>> {
    capture.boundary_cover(tx, number).map(|c| c.construct)
}

/// AC-2: a higher boundary replaces both the boundary and the construct.
#[test]
fn ac2_higher_boundary_replaces_both_boundary_and_construct() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 5, Some(b"\x1b(".to_vec()));
    capture.record_boundary_with_construct(&tx, 7, Some(vec![0xe4, 0xb8]));
    assert_eq!(
        cover_construct(&capture, &tx, 7),
        Some(Some(vec![0xe4, 0xb8])),
        "the new boundary's construct applies to the new boundary"
    );
    assert_eq!(
        cover_construct(&capture, &tx, 5),
        Some(None),
        "chunk 5 is still covered, but the old construct was replaced and \
         never applies to a chunk below the recorded boundary"
    );
    assert_eq!(cover_construct(&capture, &tx, 8), None, "8 is not covered");
}

/// AC-2: a higher boundary recorded without a construct clears the old one.
#[test]
fn ac2_higher_boundary_without_a_construct_clears_the_old_construct() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 5, Some(b"\x1b(".to_vec()));
    capture.record_boundary(&tx, 9);
    assert_eq!(cover_construct(&capture, &tx, 9), Some(None));
    assert_eq!(cover_construct(&capture, &tx, 5), Some(None));
}

/// AC-2: an equal boundary with an identical construct keeps it.
#[test]
fn ac2_equal_boundary_with_an_identical_construct_keeps_it() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 5, Some(b"\x1b[1;".to_vec()));
    capture.record_boundary_with_construct(&tx, 5, Some(b"\x1b[1;".to_vec()));
    assert_eq!(
        cover_construct(&capture, &tx, 5),
        Some(Some(b"\x1b[1;".to_vec()))
    );
}

/// AC-2 (superseded by mux-suppressed-output-round3-fixes FR7): an equal
/// boundary keeps the later record's construct, none included. Every pairing
/// of (some, other, none) is covered.
#[test]
fn ac2_equal_boundary_with_a_different_construct_takes_the_later_record() {
    let a = Some(b"\x1b(".to_vec());
    let b = Some(b"\x1b)".to_vec());
    let pairs: Vec<(Option<Vec<u8>>, Option<Vec<u8>>)> =
        vec![(a.clone(), b.clone()), (a.clone(), None), (None, a.clone())];
    for (first, second) in pairs {
        let capture = OutputCapture::new();
        let (tx, _rx) = construct_channel();
        capture.record_boundary_with_construct(&tx, 5, first.clone());
        capture.record_boundary_with_construct(&tx, 5, second.clone());
        assert_eq!(
            cover_construct(&capture, &tx, 5),
            Some(second.clone()),
            "the later record wins: first={first:?} second={second:?}"
        );
    }
}

/// AC-2: a lower boundary leaves the entry unchanged (boundary and
/// construct), whatever construct it carries.
#[test]
fn ac2_lower_boundary_leaves_the_entry_unchanged() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 9, Some(b"\x1b(".to_vec()));
    capture.record_boundary_with_construct(&tx, 4, Some(b"\x1b)".to_vec()));
    capture.record_boundary(&tx, 3);
    assert_eq!(
        cover_construct(&capture, &tx, 9),
        Some(Some(b"\x1b(".to_vec())),
        "a lower boundary must not touch the recorded boundary or construct"
    );
}

/// AC-2: the boundary-only record form means "construct none".
#[test]
fn ac2_boundary_only_record_form_means_construct_none() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary(&tx, 3);
    assert_eq!(cover_construct(&capture, &tx, 3), Some(None));
    let mut guard = capture.hold_boundary();
    guard.record(&tx, 6);
    assert_eq!(guard.cover(&tx, 6).map(|c| c.construct), Some(None));
}

/// AC-2: a record for one destination never changes another's.
#[test]
fn ac2_a_record_for_one_destination_never_changes_another() {
    let capture = OutputCapture::new();
    let (tx_a, _rx_a) = construct_channel();
    let (tx_b, _rx_b) = construct_channel();
    capture.record_boundary_with_construct(&tx_a, 5, Some(b"\x1b(".to_vec()));
    capture.record_boundary_with_construct(&tx_b, 5, Some(b"\x1b)".to_vec()));
    capture.record_boundary_with_construct(&tx_b, 8, None);
    assert_eq!(
        cover_construct(&capture, &tx_a, 5),
        Some(Some(b"\x1b(".to_vec())),
        "A keeps only its own record"
    );
    assert_eq!(cover_construct(&capture, &tx_a, 6), None);
    assert_eq!(cover_construct(&capture, &tx_b, 8), Some(None));
    assert_eq!(cover_construct(&capture, &tx_b, 5), Some(None));
}

/// AC-2 / AC-6: with two snapshots recorded back to back for one
/// destination, a chunk uses the construct of the record that covers it —
/// the second snapshot's construct applies to its own boundary chunk only.
#[test]
fn ac2_two_back_to_back_snapshots_use_the_record_that_covers_the_chunk() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 3, Some(b"\x1b(".to_vec()));
    // Chunk 3 is checked before the second snapshot lands: first record.
    assert_eq!(
        cover_construct(&capture, &tx, 3),
        Some(Some(b"\x1b(".to_vec()))
    );
    capture.record_boundary_with_construct(&tx, 5, Some(vec![0xe4]));
    // Chunk 3 checked after the second snapshot: covered, but the construct
    // now belongs to chunk 5 only.
    assert_eq!(cover_construct(&capture, &tx, 3), Some(None));
    assert_eq!(cover_construct(&capture, &tx, 4), Some(None));
    assert_eq!(cover_construct(&capture, &tx, 5), Some(Some(vec![0xe4])));
}

/// AC-2: the covered query returns the construct only when the chunk
/// number equals the recorded boundary.
#[test]
fn ac2_covered_query_returns_the_construct_only_when_the_number_equals_the_boundary() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    assert_eq!(cover_construct(&capture, &tx, 1), None, "no entry");
    capture.record_boundary_with_construct(&tx, 6, Some(b"\x1b[".to_vec()));
    for number in 0..6 {
        assert_eq!(
            cover_construct(&capture, &tx, number),
            Some(None),
            "chunk {number} is below the boundary"
        );
    }
    assert_eq!(
        cover_construct(&capture, &tx, 6),
        Some(Some(b"\x1b[".to_vec()))
    );
    assert_eq!(cover_construct(&capture, &tx, 7), None);
    assert_eq!(
        capture.is_boundary_covered(&tx, 6),
        capture.boundary_cover(&tx, 6).is_some(),
        "the boolean query and the construct query agree"
    );
}

/// AC-2: the construct is read under the SAME boundary hold as the covered
/// decision — while a guard is held, a record from another thread waits, so
/// the (covered, construct) pair the guard reads is one consistent snapshot
/// of the entry.
#[test]
fn ac2_construct_is_read_under_the_same_boundary_hold_as_the_covered_decision() {
    let capture = Arc::new(OutputCapture::new());
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 5, Some(b"\x1b(".to_vec()));

    let guard = capture.hold_boundary();
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let writer = {
        let capture = capture.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            capture.record_boundary_with_construct(&tx, 9, Some(vec![0xe4]));
        })
    };
    started_rx.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(
        !writer.is_finished(),
        "a record must wait for the held boundary exclusion"
    );
    assert_eq!(
        guard.cover(&tx, 5).map(|c| c.construct),
        Some(Some(b"\x1b(".to_vec())),
        "the covered decision and the construct come from the same hold"
    );
    drop(guard);
    writer.join().unwrap();
    assert_eq!(
        cover_construct(&capture, &tx, 9),
        Some(Some(vec![0xe4])),
        "after the hold ends the waiting record lands"
    );
}

// ── mux-suppressed-output-round3-fixes task0004 (FR6, FR7): generation in
//    the boundary record, later record wins at an equal boundary ───────────

fn cover_generation(
    capture: &OutputCapture,
    tx: &mpsc::Sender<PtyOutputChunk>,
    number: u64,
) -> Option<u64> {
    capture.boundary_cover(tx, number).map(|c| c.generation)
}

/// AC-1 (FR7, TM-3, TS-7; registry, finding `240031761bfdf695`): the record
/// rules for one destination. An equal-boundary record leaves the later
/// construct whatever it is (none included, in both directions), an identical
/// construct is kept, a higher boundary replaces both values, a lower boundary
/// changes nothing (generation included), insert / higher / equal records each
/// give a strictly greater generation, and one destination's record never
/// changes another's construct or generation.
#[test]
fn round3_24003176_equal_boundary_takes_the_later_record() {
    let paren = Some(b"\x1b(".to_vec());
    let other_designator = Some(b"\x1b)".to_vec());

    // Equal boundary: the later construct wins, in every pairing.
    let pairs: Vec<(Option<Vec<u8>>, Option<Vec<u8>>)> = vec![
        (None, paren.clone()),
        (paren.clone(), None),
        (paren.clone(), other_designator.clone()),
        (paren.clone(), paren.clone()),
    ];
    for (first, second) in pairs {
        let capture = OutputCapture::new();
        let (tx, _rx) = construct_channel();
        capture.record_boundary_with_construct(&tx, 5, first.clone());
        let g_first = cover_generation(&capture, &tx, 5).expect("covered");
        capture.record_boundary_with_construct(&tx, 5, second.clone());
        assert_eq!(
            cover_construct(&capture, &tx, 5),
            Some(second.clone()),
            "first={first:?} second={second:?}: the later record's construct stands"
        );
        assert!(
            cover_generation(&capture, &tx, 5).expect("covered") > g_first,
            "first={first:?} second={second:?}: an equal-boundary record advances the generation"
        );
        assert_eq!(
            cover_construct(&capture, &tx, 6),
            None,
            "the boundary stays 5"
        );
    }

    // Higher boundary: replaces boundary and construct, generation advances.
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    capture.record_boundary_with_construct(&tx, 5, paren.clone());
    let g_insert = cover_generation(&capture, &tx, 5).expect("covered");
    capture.record_boundary_with_construct(&tx, 7, other_designator.clone());
    let g_higher = cover_generation(&capture, &tx, 7).expect("covered");
    assert!(
        g_higher > g_insert,
        "a higher boundary advances the generation"
    );
    assert_eq!(
        cover_construct(&capture, &tx, 7),
        Some(other_designator.clone())
    );
    assert_eq!(cover_construct(&capture, &tx, 8), None);

    // Lower boundary: boundary, construct and generation are all unchanged.
    capture.record_boundary_with_construct(&tx, 6, paren.clone());
    capture.record_boundary(&tx, 1);
    assert_eq!(
        cover_construct(&capture, &tx, 7),
        Some(other_designator.clone())
    );
    assert_eq!(
        cover_generation(&capture, &tx, 7),
        Some(g_higher),
        "a lower boundary leaves the generation alone"
    );
    assert_eq!(cover_construct(&capture, &tx, 8), None);

    // A record for one destination never changes another's construct or
    // generation, and generations are never reused across destinations.
    let (tx_b, _rx_b) = construct_channel();
    capture.record_boundary_with_construct(&tx_b, 7, paren.clone());
    let g_b = cover_generation(&capture, &tx_b, 7).expect("covered");
    assert!(g_b > g_higher, "a later record gets a later generation");
    capture.record_boundary_with_construct(&tx_b, 7, None);
    capture.record_boundary_with_construct(&tx_b, 9, paren.clone());
    assert_eq!(
        cover_construct(&capture, &tx, 7),
        Some(other_designator),
        "destination A keeps its own construct"
    );
    assert_eq!(
        cover_generation(&capture, &tx, 7),
        Some(g_higher),
        "destination A keeps its own generation"
    );
    assert_ne!(
        cover_generation(&capture, &tx_b, 9),
        cover_generation(&capture, &tx, 7),
        "no generation value is shared between destinations"
    );
}

/// AC-1: the guard form of the record gives the same results as the
/// free-standing form (equal boundary, later record wins; generation
/// advances).
#[test]
fn round3_guard_record_forms_apply_the_same_rules() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    let mut guard = capture.hold_boundary();
    guard.record_with_construct(&tx, 4, Some(b"\x1b(".to_vec()));
    let first = guard.cover(&tx, 4).expect("covered");
    assert_eq!(first.construct, Some(b"\x1b(".to_vec()));
    guard.record(&tx, 4);
    let second = guard.cover(&tx, 4).expect("covered");
    assert_eq!(
        second.construct, None,
        "the boundary-only form is the later record"
    );
    assert!(second.generation > first.generation);
    guard.record_with_construct(&tx, 3, Some(vec![0xe4]));
    assert_eq!(
        guard.cover(&tx, 4),
        Some(second),
        "a lower record changes nothing"
    );
}

/// AC-2 (FR6): the covered query returns the construct with the semantics it
/// always had, together with the destination's current generation — on the
/// free-standing form and on a held guard alike. The generation is the same
/// across repeated queries with no record in between, and a chunk number
/// below the boundary still sees the entry's generation with no construct.
#[test]
fn round3_covered_query_carries_the_current_generation_on_both_forms() {
    let capture = OutputCapture::new();
    let (tx, _rx) = construct_channel();
    assert_eq!(
        capture.boundary_cover(&tx, 1),
        None,
        "no entry: not covered"
    );

    capture.record_boundary_with_construct(&tx, 6, Some(b"\x1b[".to_vec()));
    let at_boundary = capture.boundary_cover(&tx, 6).expect("covered at 6");
    assert_eq!(at_boundary.construct, Some(b"\x1b[".to_vec()));
    let below = capture.boundary_cover(&tx, 3).expect("covered at 3");
    assert_eq!(
        below.construct, None,
        "the construct never applies below the boundary"
    );
    assert_eq!(
        below.generation, at_boundary.generation,
        "one entry, one generation, whatever number is asked"
    );
    assert_eq!(capture.boundary_cover(&tx, 7), None, "7 is not covered");
    assert_eq!(
        capture.boundary_cover(&tx, 6),
        Some(at_boundary.clone()),
        "repeated queries with no record in between agree"
    );

    {
        let guard = capture.hold_boundary();
        assert_eq!(
            guard.cover(&tx, 6),
            Some(at_boundary.clone()),
            "the guard form answers exactly as the free-standing form"
        );
        assert_eq!(guard.cover(&tx, 6), guard.cover(&tx, 6));
        assert_eq!(guard.cover(&tx, 7), None);
    }

    capture.record_boundary_with_construct(&tx, 6, Some(b"\x1b[".to_vec()));
    let after = capture.boundary_cover(&tx, 6).expect("covered at 6");
    assert_eq!(after.construct, at_boundary.construct);
    assert!(
        after.generation > at_boundary.generation,
        "a re-record of an identical construct is still a later record"
    );
    assert!(!capture.is_boundary_covered(&tx, 7));
    assert!(capture.is_boundary_covered(&tx, 6));
}

// ── task0004 AC-3: visibility restore records the construct of the
//    payload it sent ─────────────────────────────────────────────────────

/// A pane hidden by visibility for `owner`, ready for
/// `resume_pane_with_permit`.
fn hidden_pane_owned_by(
    id: PaneId,
    owner: &mpsc::Sender<PtyOutputChunk>,
) -> (MuxPane, SharedOutputTarget) {
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owner.clone()),
    }));
    (MuxPane::new_test(id, 80, 24, target.clone()), target)
}

/// Seed the ring and the shadow parser with `bytes` through the capture
/// step (as the reader would), returning the capture's new number.
fn seed_through_capture(pane: &MuxPane, ring_bytes: &[u8], shadow_bytes: &[u8]) -> u64 {
    let (_, number) = pane.output_capture.capture(|| {
        pane.shadow_parser.lock().unwrap().process(shadow_bytes);
        pane.scrollback.lock().unwrap().write(ring_bytes);
    });
    number
}

/// Run the production visibility restore and return the delivered
/// snapshot chunk together with the recorded boundary number.
fn resume_and_take_snapshot(
    pane: &MuxPane,
    tx: &mpsc::Sender<PtyOutputChunk>,
    rx: &mut mpsc::Receiver<PtyOutputChunk>,
) -> PtyOutputChunk {
    let permit = tx.try_reserve().expect("capacity for the resume permit");
    let outcome = resume_pane_with_permit(pane, tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));
    rx.try_recv().expect("the snapshot must have been sent")
}

/// The snapshot the assembly function builds for the pane's CURRENT ring and
/// shadow state (the byte-identity oracle of AC-3).
fn expected_resume_assembly(pane: &MuxPane) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
    let (ring, segments, wrapped) = pane
        .scrollback
        .lock()
        .unwrap()
        .read_segments_with_wrap_state();
    let (screen, alt, dims) = {
        let parser = pane.shadow_parser.lock().unwrap();
        let (rows, cols) = parser.screen().size();
        let alt = parser.screen().alternate_screen();
        let screen = if alt || wrapped {
            parser.screen().contents_formatted()
        } else {
            Vec::new()
        };
        (screen, alt, (cols, rows))
    };
    crate::mux::snapshot_bytes::build_resume_snapshot_bytes_for_ring(
        &ring, &segments, &screen, alt, wrapped, dims, 10_000,
    )
}

fn assert_sent_snapshot_is_the_assembly_output(
    chunk: &PtyOutputChunk,
    expected: &(Vec<u8>, Vec<(usize, u16, u16)>),
) {
    assert_eq!(chunk.kind, ChunkKind::Snapshot);
    let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&chunk.data);
    assert_eq!(content, expected.0.as_slice(), "snapshot bytes unchanged");
    let tuples: Vec<(usize, u16, u16)> = segments
        .iter()
        .map(|s| (s.offset as usize, s.cols, s.rows))
        .collect();
    assert_eq!(tuples, expected.1, "snapshot segments unchanged");
    assert_eq!(
        chunk.data,
        encode_snapshot_segments(&expected.0, &expected.1),
        "wire encoding unchanged"
    );
}

/// AC-3: a main-buffer visibility restore whose ring ends in each of the
/// three tail kinds records that construct together with the boundary,
/// and sends the byte-identical snapshot the assembly function builds.
#[test]
fn ac3_visibility_restore_records_the_construct_of_the_snapshot_it_sent() {
    let cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        (
            "cut utf-8",
            [b"abc".as_slice(), &[0xe4, 0xb8]].concat(),
            vec![0xe4, 0xb8],
        ),
        (
            "awaiting designator",
            [b"abc".as_slice(), &[ESC_BYTE, b'(']].concat(),
            vec![ESC_BYTE, b'('],
        ),
        (
            "cut csi with an executed C0",
            [b"abc".as_slice(), &[ESC_BYTE, b'[', b'1', b'\r', b';']].concat(),
            vec![ESC_BYTE, b'[', b'1', b';'],
        ),
    ];
    for (name, ring_bytes, construct) in cases {
        let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
        let (pane, _target) = hidden_pane_owned_by(31, &tx);
        let boundary = seed_through_capture(&pane, &ring_bytes, &ring_bytes);
        let expected = expected_resume_assembly(&pane);

        let chunk = resume_and_take_snapshot(&pane, &tx, &mut rx);

        assert_sent_snapshot_is_the_assembly_output(&chunk, &expected);
        assert_eq!(
            cover_construct(&pane.output_capture, &tx, boundary),
            Some(Some(construct)),
            "case {name}: the construct of the sent payload is recorded with the boundary"
        );
    }
}

/// AC-3 / AC-6: an alternate-screen visibility restore sends the byte-
/// identical snapshot and records the construct of THAT payload, which ends
/// in the screen dump (no construct).
#[test]
fn ac6_alternate_screen_visibility_restore_records_none() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (pane, _target) = hidden_pane_owned_by(32, &tx);
    let boundary = seed_through_capture(&pane, b"abc", b"abc\x1b[?1049hTUI\x1b(");
    assert!(
        pane.shadow_parser
            .lock()
            .unwrap()
            .screen()
            .alternate_screen(),
        "test prerequisite: the pane is on the alternate screen"
    );
    let expected = expected_resume_assembly(&pane);

    let chunk = resume_and_take_snapshot(&pane, &tx, &mut rx);

    assert_sent_snapshot_is_the_assembly_output(&chunk, &expected);
    assert_eq!(
        cover_construct(&pane.output_capture, &tx, boundary),
        Some(None),
        "an alternate-screen restore ends in the screen dump: nothing is carried"
    );
}

/// AC-3 / AC-6: a wrapped ring with a non-empty screen dump gets a dump
/// block appended after the ring, so the payload no longer ends in the ring's
/// cut construct and nothing is recorded. (The shadow parser's
/// `contents_formatted()` is never empty, so every production restore of a
/// wrapped main-screen ring ends this way; the "wrapped ring with an empty
/// dump" payload shape is reachable only through the assembly function, and
/// its tests live with the reader-level FR8 tests in `ipc/pty_spawn/tests.rs`.)
#[test]
fn ac6_wrapped_ring_with_a_dump_block_records_none() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (pane, _target) = hidden_pane_owned_by(33, &tx);
    *pane.scrollback.lock().unwrap() = ScrollbackRingBuffer::new(32);
    let mut ring_bytes = b"visible text on the shadow screen\r\n".repeat(3);
    ring_bytes.extend_from_slice(&[ESC_BYTE, b'(']);
    let boundary = seed_through_capture(&pane, &ring_bytes, &ring_bytes);
    let expected = expected_resume_assembly(&pane);
    assert!(
        !expected.0.ends_with(&[ESC_BYTE, b'(']),
        "test prerequisite: the dump block follows the ring's cut construct"
    );

    let chunk = resume_and_take_snapshot(&pane, &tx, &mut rx);

    assert_sent_snapshot_is_the_assembly_output(&chunk, &expected);
    assert_eq!(
        cover_construct(&pane.output_capture, &tx, boundary),
        Some(None)
    );
}

// ── task0004 AC-3: the enqueue carries the construct with the boundary ─

/// AC-3: the on-demand enqueue's fast path records the construct together
/// with the boundary, once the chunk is in the channel.
#[test]
fn ac3_enqueue_fast_path_records_the_construct_with_the_boundary() {
    let (tx, mut rx) = construct_channel();
    let capture = Arc::new(OutputCapture::new());
    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::snapshot(1, b"S".to_vec()),
        &mut deferred,
        Some((capture.clone(), 6, Some(b"\x1b(".to_vec()))),
    );
    assert!(rx.try_recv().is_ok(), "the chunk entered the channel");
    assert!(deferred.is_empty());
    assert_eq!(
        cover_construct(&capture, &tx, 6),
        Some(Some(b"\x1b(".to_vec()))
    );
}

/// AC-3: on a full channel the construct travels with the deferred boundary
/// commit and nothing is recorded until the flush.
#[test]
fn ac3_enqueue_on_a_full_channel_defers_the_construct_with_the_commit() {
    let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
    tx.try_send(PtyOutputChunk::pty_output(1, b"filler".to_vec()))
        .unwrap();
    let capture = Arc::new(OutputCapture::new());
    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::snapshot(1, b"S".to_vec()),
        &mut deferred,
        Some((capture.clone(), 6, Some(vec![0xe4, 0xb8]))),
    );
    assert_eq!(deferred.len(), 1);
    assert_eq!(
        cover_construct(&capture, &tx, 0),
        None,
        "nothing is recorded before the flush"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(_, Some(commit))) => {
            assert_eq!(commit.boundary, 6);
            assert_eq!(commit.construct, Some(vec![0xe4, 0xb8]));
        }
        other => panic!("expected a deferred chunk with a boundary commit, got {other:?}"),
    }
}

/// AC-3: an enqueue without a boundary commit records nothing.
#[test]
fn ac3_enqueue_without_a_boundary_commit_records_nothing() {
    let (tx, _rx) = construct_channel();
    let capture = Arc::new(OutputCapture::new());
    let mut deferred = DeferredOutputQueue::new();
    enqueue_pane_output_chunk(
        &tx,
        PtyOutputChunk::pty_output(1, b"x".to_vec()),
        &mut deferred,
        None,
    );
    assert_eq!(cover_construct(&capture, &tx, 0), None);
}

/// AC-2: visibility restore decides the construct outside the capture
/// exclusion and the boundary exclusion (the lock order is unchanged: the
/// decider only reads the payload and takes no lock of its own).
#[test]
fn ac2_visibility_restore_decides_the_construct_outside_both_exclusions() {
    let (tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);
    let (pane, _target) = hidden_pane_owned_by(35, &tx);
    seed_through_capture(&pane, b"abc\x1b(", b"abc\x1b(");
    let probe = pane.output_capture.probe_decider_exclusions();
    let _ = resume_and_take_snapshot(&pane, &tx, &mut rx);
    let (calls, free) = probe.finish();
    assert!(calls >= 1, "the decider must run on the restore path");
    assert!(
        free,
        "the decider must run with neither the capture exclusion nor the boundary exclusion held"
    );
}

// ── OSC 7501 record table in the pane's agent-status record
//    (osc7501-program-status task0004, AC-3 / AC-4) ───────────────────────

mod program_status_record {
    use super::*;
    use crate::program_status::{Parsed, Report, Terminator, parse};

    fn report(body: &str) -> Report {
        match parse(body, Terminator::Bel) {
            Parsed::Report(report) => report,
            other => panic!("{body}: expected a report, got {other:?}"),
        }
    }

    fn new_pane(id: PaneId) -> MuxPane {
        MuxPane::new_test(id, 80, 24, make_output_target())
    }

    fn table_len(pane: &MuxPane) -> usize {
        pane.agent_status.lock().unwrap().program_status.len()
    }

    fn revision(pane: &MuxPane) -> u64 {
        pane.agent_status.lock().unwrap().revision
    }

    /// AC-3: an accepted report changes the table and bumps the revision
    /// exactly once; a re-report of the same record is accepted too.
    #[test]
    fn ac3_an_accepted_report_changes_the_table_and_bumps_the_revision_once() {
        let pane = new_pane(1);
        assert_eq!(table_len(&pane), 0);

        let first = pane.apply_program_status_report(report("state=working:id=a"));
        assert_eq!(first, 1);
        assert_eq!(revision(&pane), 1);
        assert_eq!(table_len(&pane), 1);

        let again = pane.apply_program_status_report(report("state=working:id=a"));
        assert_eq!(again, 2);
        assert_eq!(table_len(&pane), 1);

        let cleared = pane.apply_program_status_report(report("state=clear:id=a"));
        assert_eq!(cleared, 3, "a clear is an accepted report");
        assert_eq!(table_len(&pane), 0);
    }

    /// AC-3: the OSC 777 part of the record is untouched by an OSC 7501
    /// report, and the composite follows both.
    #[test]
    fn ac3_the_composite_state_follows_both_sources() {
        let pane = new_pane(2);
        assert_eq!(pane.agent_status.lock().unwrap().composite_state(), None);

        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Done,
            name: None,
        });
        assert_eq!(
            pane.agent_status.lock().unwrap().composite_state(),
            Some(AgentState::Done),
            "an empty table leaves the OSC 777 state as it is"
        );

        pane.apply_program_status_report(report("state=working"));
        {
            let status = pane.agent_status.lock().unwrap();
            assert_eq!(status.state, Some(AgentState::Done));
            assert_eq!(status.composite_state(), Some(AgentState::Working));
        }

        pane.apply_agent_status_event(AgentStatusEvent::Clear);
        pane.apply_program_status_report(report("state=error"));
        assert_eq!(
            pane.agent_status.lock().unwrap().composite_state(),
            Some(AgentState::Error)
        );
    }

    fn fill_every_state(pane: &MuxPane) {
        for body in [
            "state=working:id=w",
            "state=blocked:id=b",
            "state=idle:id=i",
            "state=done:id=d",
            "state=error:id=e",
        ] {
            pane.apply_program_status_report(report(body));
        }
    }

    /// AC-4: a live prompt start removes `working`, `blocked` and `idle`
    /// records and keeps `done` and `error`, bumping the revision once.
    #[test]
    fn ac4_a_prompt_start_removes_working_blocked_and_idle_and_keeps_done_and_error() {
        let pane = new_pane(3);
        fill_every_state(&pane);
        let before = revision(&pane);

        let bumped = pane.record_live_osc133_mark(PromptMarkKind::PromptStart);
        assert_eq!(bumped, Some(before + 1));
        assert_eq!(revision(&pane), before + 1);

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.program_status.len(), 2);
        assert_eq!(status.composite_state(), Some(AgentState::Error));
        drop(status);

        // Only done and error remain: nothing more to remove.
        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::PromptStart),
            None
        );
        assert_eq!(revision(&pane), before + 1);
    }

    /// AC-4: a mark that is not a prompt start never reaches the table.
    #[test]
    fn ac4_marks_other_than_a_prompt_start_leave_the_table_alone() {
        let pane = new_pane(4);
        fill_every_state(&pane);
        let before = revision(&pane);
        for kind in [PromptMarkKind::CommandEnd] {
            assert_eq!(pane.record_live_osc133_mark(kind), None);
        }
        assert_eq!(revision(&pane), before);
        assert_eq!(table_len(&pane), 5);
    }

    /// AC-4: a prompt start with an empty table changes nothing.
    #[test]
    fn ac4_a_prompt_start_with_an_empty_table_does_not_bump() {
        let pane = new_pane(5);
        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::PromptStart),
            None
        );
        assert_eq!(revision(&pane), 0);
    }

    /// AC-4: a reset removes every record, bumping only when one existed.
    #[test]
    fn ac4_a_reset_removes_every_record_and_bumps_only_when_something_existed() {
        let pane = new_pane(6);
        assert_eq!(pane.apply_program_status_reset(), None);
        assert_eq!(revision(&pane), 0);

        fill_every_state(&pane);
        let before = revision(&pane);
        assert_eq!(pane.apply_program_status_reset(), Some(before + 1));
        assert_eq!(table_len(&pane), 0);

        assert_eq!(pane.apply_program_status_reset(), None);
        assert_eq!(revision(&pane), before + 1);
    }

    /// AC-4: when one prompt start both fires the OSC 777 inferred clear and
    /// empties records, the revision moves once.
    #[test]
    fn ac4_one_prompt_start_covering_the_inferred_clear_and_the_table_bumps_once() {
        let pane = new_pane(7);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        });
        pane.apply_program_status_report(report("state=working:id=a"));
        assert_eq!(table_len(&pane), 1);
        let before = revision(&pane);

        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::CommandEnd),
            None
        );
        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::PromptStart),
            Some(before + 1)
        );

        let status = pane.agent_status.lock().unwrap();
        assert_eq!(status.state, None, "the inferred clear applied");
        assert_eq!(status.program_status.len(), 0, "the records were removed");
        assert_eq!(status.revision, before + 1);
    }

    /// AC-4: the records live in the pane's agent-status record and nowhere
    /// else, so they go away with the pane.
    #[test]
    fn ac4_the_records_go_away_with_the_pane() {
        let pane = new_pane(8);
        pane.apply_program_status_report(report("state=done:id=a"));
        assert_eq!(table_len(&pane), 1);
        let record = pane.agent_status.clone();
        assert_eq!(Arc::strong_count(&record), 2);
        drop(pane);
        assert_eq!(
            Arc::strong_count(&record),
            1,
            "nothing but the pane held the record"
        );
    }

    // ---- osc7501-leading-zero-length task0002 AC-3 (FR6): the operation that
    //      applies only the OSC 7501 prompt start ----

    /// The operation removes `working`, `blocked` and `idle` records, keeps
    /// `done` and `error`, and moves the revision once.
    #[test]
    fn leadzero_ac3_the_prompt_start_only_operation_removes_records_and_bumps_once() {
        let pane = new_pane(9);
        fill_every_state(&pane);
        let before = revision(&pane);

        assert_eq!(pane.apply_program_status_prompt_start(), Some(before + 1));
        assert_eq!(revision(&pane), before + 1);
        {
            let status = pane.agent_status.lock().unwrap();
            assert_eq!(status.program_status.len(), 2);
            assert_eq!(status.composite_state(), Some(AgentState::Error));
        }

        // Only `done` and `error` remain: nothing is removed, so the
        // revision does not move.
        assert_eq!(pane.apply_program_status_prompt_start(), None);
        assert_eq!(revision(&pane), before + 1);
    }

    /// With an empty table the operation changes nothing.
    #[test]
    fn leadzero_ac3_the_prompt_start_only_operation_with_an_empty_table_does_not_bump() {
        let pane = new_pane(10);
        assert_eq!(pane.apply_program_status_prompt_start(), None);
        assert_eq!(revision(&pane), 0);
    }

    /// The operation never touches the OSC 777 inferred-clear latch or the
    /// OSC 777 state: a `D` the latch recorded still pairs with the next
    /// canonical prompt start, and the OSC 777 report stays set until then.
    #[test]
    fn leadzero_ac3_the_prompt_start_only_operation_does_not_touch_the_exit_latch() {
        let pane = new_pane(11);
        pane.apply_agent_status_event(AgentStatusEvent::Set {
            state: AgentState::Working,
            name: None,
        });
        pane.apply_program_status_report(report("state=working:id=a"));
        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::CommandEnd),
            None
        );
        let before = revision(&pane);

        // The records go; the OSC 777 report and the armed latch stay.
        assert_eq!(pane.apply_program_status_prompt_start(), Some(before + 1));
        {
            let status = pane.agent_status.lock().unwrap();
            assert_eq!(status.state, Some(AgentState::Working));
            assert_eq!(status.program_status.len(), 0);
        }
        // A second use removes nothing and still does not consume the `D`.
        assert_eq!(pane.apply_program_status_prompt_start(), None);

        // The canonical `A` after the `D` still fires the inferred clear.
        assert_eq!(
            pane.record_live_osc133_mark(PromptMarkKind::PromptStart),
            Some(before + 2)
        );
        assert_eq!(pane.agent_status.lock().unwrap().state, None);
    }
}
