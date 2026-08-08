use super::*;

fn make_output_target() -> SharedOutputTarget {
    let (tx, _rx) = mpsc::channel(1);
    Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)))
}

/// Decode a `Snapshot`-kind chunk's / `EvalResult::ResumeWithSnapshot`'s
/// wire-encoded bytes (task0004 round-4 rework D1',
/// `mux_ipc::protocol::decode_snapshot_payload`) back into plain
/// content bytes, discarding the structural segment header — used by
/// tests that only care about the ANSI content layout.
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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

/// TS-14 (revised): Detached -> Connected returns snapshot bytes that
/// route through `build_resume_snapshot_bytes` (the visibility-resume
/// SSOT). For a main-buffer pane (shadow_parser never entered alt-screen)
/// the helper drops the daemon vt100 `contents_formatted()` slice and
/// rebuilds the visible viewport from scrollback alone — same
/// main/alt split contract as the reattach path. Captured
/// raw_passthrough must NOT appear (replaying it would re-spawn
/// viewers / re-render inline images) and the buffer must still be
/// drained + cleared.
#[test]
fn test_evaluate_output_target_detached_to_connected_returns_snapshot() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    pane.scrollback.lock().unwrap().write(b"buffered-from-ring");
    pane.shadow_parser.lock().unwrap().process(b"hello-shadow");
    pane.raw_passthrough
        .lock()
        .unwrap()
        .append(b"\x1b_Gi=1;ZZ\x1b\\");
    let result = evaluate_output_target(&pane, false, true, &owned_tx);
    match result {
        EvalResult::ResumeWithSnapshot { chunk } => {
            // D6''''' (AC-9): the chunk must already be tagged
            // Snapshot-kind, not the default PtyOutput — a caller
            // sending it as PtyOutput would render the raw envelope
            // literally instead of decoding it.
            assert_eq!(chunk.kind, ChunkKind::Snapshot);
            let snapshot = decode_snapshot_content(&chunk.data);
            assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
            let s = String::from_utf8_lossy(&snapshot);
            assert!(
                snapshot
                    .windows(b"buffered-from-ring".len())
                    .any(|w| w == b"buffered-from-ring"),
                "snapshot must include ring data"
            );
            // Main-buffer pane: the daemon vt100 dump must NOT appear in
            // the snapshot. `build_resume_snapshot_bytes` follows the
            // main/alt split — the client rebuilds the visible viewport
            // from scrollback alone.
            assert!(
                !snapshot
                    .windows(b"hello-shadow".len())
                    .any(|w| w == b"hello-shadow"),
                "main-buffer resume snapshot must omit the shadow screen dump"
            );
            assert!(
                !s.contains("\u{1b}_Gi=1"),
                "snapshot must NOT include captured passthrough"
            );
        }
        _ => panic!("expected ResumeWithSnapshot"),
    }
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
    assert_eq!(pane.raw_passthrough.lock().unwrap().len(), 0);
}

/// D6''' (round-6 rework, review round-5 finding `89b58cd82d7aa713`):
/// mirrors `test_resume_pane_with_permit_stays_detached_when_snapshot_exceeds_frame_limit`
/// for `evaluate_output_target`'s parallel `ResumeWithSnapshot` branch
/// — an oversize encoded snapshot must not transition the pane to
/// Connected at all.
#[test]
fn test_evaluate_output_target_stays_detached_when_snapshot_exceeds_frame_limit() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(2, 80, 24, target.clone());
    let oversize_capacity = mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD + 1024 * 1024;
    *pane.scrollback.lock().unwrap() =
        crate::mux::scrollback_buffer::ScrollbackRingBuffer::new(oversize_capacity);
    pane.scrollback
        .lock()
        .unwrap()
        .write(&vec![b'x'; oversize_capacity]);

    let result = evaluate_output_target(&pane, false, true, &owned_tx);
    assert!(
        matches!(result, EvalResult::Unchanged),
        "oversize snapshot must not resume the pane"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "pane must stay Detached rather than swap to Connected with an \
         unsendable snapshot"
    );
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

/// F6: same connection's hide -> show round trip restores Connected.
#[test]
fn test_evaluate_output_target_same_connection_hide_show_roundtrip() {
    let (a_tx, _a_rx) = mpsc::channel(16);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(a_tx.clone())));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());

    let r1 = evaluate_output_target(&pane, false, false, &a_tx);
    assert!(matches!(r1, EvalResult::SwitchedToDetached));

    let r2 = evaluate_output_target(&pane, false, true, &a_tx);
    assert!(matches!(r2, EvalResult::ResumeWithSnapshot { .. }));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let outcome = resume_pane_with_permit(&pane, &b_tx, AnyPermit::Borrowed(permit));
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
    let outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let first_outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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
    let second_outcome = resume_pane_with_permit(&pane, &owned_tx, AnyPermit::Borrowed(permit));
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

/// Reattach (Detached -> Connected) must still produce a snapshot after
/// the shadow parser mutex was poisoned by a reader-thread panic.
///
/// The main-buffer pane drops the shadow slice (same main/alt split as
/// `build_resume_snapshot_bytes`), so we feed scrollback bytes instead
/// and assert those survive the poisoned lock.
#[test]
fn test_evaluate_output_target_survives_poisoned_shadow_parser() {
    let (owned_tx, _rx) = mpsc::channel(16);
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let pane = MuxPane::new_test(1, 80, 24, target.clone());
    pane.scrollback.lock().unwrap().write(b"ring-bytes-x");
    pane.shadow_parser.lock().unwrap().process(b"shadow-data");
    poison_shadow_parser(&pane);

    let result = evaluate_output_target(&pane, false, true, &owned_tx);
    match result {
        EvalResult::ResumeWithSnapshot { chunk } => {
            assert_eq!(chunk.kind, ChunkKind::Snapshot);
            assert!(
                chunk
                    .data
                    .windows(b"ring-bytes-x".len())
                    .any(|w| w == b"ring-bytes-x"),
                "snapshot must include scrollback even after poisoned shadow lock"
            );
        }
        _ => panic!("expected ResumeWithSnapshot"),
    }
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
    );
    assert_eq!(
        deferred.len(),
        1,
        "full channel must defer, not send, the chunk"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk)) => {
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
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V1".to_vec()));
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"V2".to_vec()));
    assert_eq!(
        deferred.len(),
        1,
        "a second chunk for the same pane must coalesce, not add a second entry"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk)) => {
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
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"first".to_vec()));
    deferred.defer_visibility_resume(1);
    assert_eq!(deferred.len(), 2);

    // Second RequestPaneSnapshot for the SAME pane while both entries
    // are still queued: must coalesce the Chunk IN PLACE, not move it
    // to the tail.
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"second".to_vec()));
    assert_eq!(
        deferred.len(),
        2,
        "coalescing must not grow the queue past its pre-coalesce length"
    );

    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk)) => {
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

    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"pending".to_vec()));
    assert_eq!(
        deferred.len(),
        2,
        "the Chunk must be INSERTED alongside the already-queued VisibilityResume, \
         not dropped — a dropped Chunk here means the client's RequestPaneSnapshot \
         gets no reply at all whenever the Resume later no-ops"
    );

    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk)) => {
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
        deferred.defer_chunk(PtyOutputChunk::pty_output(pane_id, vec![pane_id as u8]));
    }
    assert_eq!(
        deferred.len(),
        MAX_DEFERRED_ITEMS,
        "queue must never grow past the documented cap"
    );

    let mut surviving_pane_ids = Vec::new();
    while let Some(item) = deferred.pop_front() {
        match item {
            DeferredOutputItem::Chunk(chunk) => surviving_pane_ids.push(chunk.pane_id),
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
