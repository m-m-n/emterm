//! mux-suppressed-output-round3-fixes task0004 (FR6, FR7, NFR2): the
//! suppressed pipeline decides tail omission from the destination's boundary
//! record as it stands when the replacement is sent, and a later snapshot
//! recorded at an equal boundary leaves its own construct.
//!
//! Second snapshots in these tests:
//! - **Before the covered decision** (the reader paused at P2): the
//!   production visibility restore (`resume_pane_with_permit`) and an
//!   on-demand snapshot assembled and recorded exactly as the on-demand
//!   handler does (`build_snapshot_bytes_for_ring`, the construct decided
//!   from the assembled payload, `try_send` and record under one boundary
//!   hold).
//! - **At P4** (the reader holds a slot, no exclusion): the same two forms.
//!   The visibility restore there is the production path after the pane is
//!   hidden again (target `Detached { HiddenByVisibility }`), which is the
//!   sequence a hide/show cycle makes. A higher boundary (case (c)) cannot
//!   arise from a real reader and is recorded through the boundary-record API
//!   only; parity is not asserted for it.
//!
//! The client / reference oracle is the reader-level one of the parent test
//! module (`r2_*`, `SuppressedRun`), reused unchanged. The visibility-restore
//! driver of the inline `fr8_snapshot_tail` module is private, so this module
//! has its own.

use super::*;
use crate::mux::session::pane::{AnyPermit, ResumeOutcome, resume_pane_with_permit};
use crate::mux::snapshot_bytes::build_snapshot_bytes_for_ring;
use crate::mux::snapshot_tail::trailing_construct_bytes;
use std::time::Duration;

const ESC: u8 = 0x1b;
const WAIT: Duration = Duration::from_secs(5);

/// Poll `condition` until it holds or `WAIT` has passed; whether it held.
fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + WAIT;
    while std::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::yield_now();
    }
    false
}

/// Join the reader thread, failing (instead of hanging the suite) when it
/// does not finish: a leaked slot or an unconsumed EOF marker would leave it
/// blocked on a full channel.
fn join_reader(handle: std::thread::JoinHandle<()>) {
    assert!(
        wait_for(|| handle.is_finished()),
        "the reader must finish (it is blocked on a full channel or a wait)"
    );
    handle.join().unwrap();
}

/// A pane with a `Connected` destination of `cap` slots.
struct Harness {
    pane: MuxPane,
    tx: mpsc::Sender<PtyOutputChunk>,
    rx: mpsc::Receiver<PtyOutputChunk>,
}

impl Harness {
    fn new(pane_id: PaneId, cap: usize) -> Self {
        let (tx, rx) = mpsc::channel::<PtyOutputChunk>(cap);
        let output_target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
        let pane = MuxPane::new_test(pane_id, R2_COLS, R2_ROWS, output_target);
        Self { pane, tx, rx }
    }

    /// Run `pty_reader_loop` over `chunks` on a background thread, WITHOUT
    /// touching the pane's output target.
    fn spawn_reader(&self, chunks: Vec<Vec<u8>>) -> std::thread::JoinHandle<()> {
        let pane = &self.pane;
        std::thread::spawn({
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
            move || {
                pty_reader_loop(
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
            }
        })
    }

    /// What the destination received, as the parent module's oracle input.
    fn finish(mut self) -> SuppressedRun {
        let mut received = Vec::new();
        while let Ok(c) = self.rx.try_recv() {
            received.push(c);
        }
        let snapshots = received
            .iter()
            .filter(|c| c.kind == R2ChunkKind::Snapshot)
            .map(|c| {
                let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&c.data);
                (
                    content.to_vec(),
                    segments
                        .iter()
                        .map(|s| (s.offset as usize, s.cols, s.rows))
                        .collect(),
                )
            })
            .collect();
        let ring = self.pane.scrollback.lock().unwrap().read_all();
        SuppressedRun {
            received,
            snapshots,
            ring,
        }
    }
}

/// An on-demand snapshot, assembled and recorded as the on-demand handler
/// does: the ring + shadow read under the capture exclusion (which yields the
/// boundary), the bytes from `build_snapshot_bytes_for_ring`, the construct
/// decided from the assembled payload, and the insertion and the record under
/// one boundary hold.
fn send_on_demand_snapshot(pane: &MuxPane, tx: &mpsc::Sender<PtyOutputChunk>) {
    let (
        (scrollback_data, scrollback_segments, ring_wrapped, screen_data, alt_screen, current_dims),
        boundary,
    ) = pane.output_capture.captured_read(|| {
        let (scrollback_data, scrollback_segments, ring_wrapped) = pane
            .scrollback
            .lock()
            .unwrap()
            .read_segments_with_wrap_state();
        let (screen_data, alt_screen, current_dims) = {
            let parser = pane.shadow_parser.lock().unwrap();
            let screen = parser.screen();
            let (rows, cols) = screen.size();
            (
                screen.contents_formatted(),
                screen.alternate_screen(),
                (cols, rows),
            )
        };
        (
            scrollback_data,
            scrollback_segments,
            ring_wrapped,
            screen_data,
            alt_screen,
            current_dims,
        )
    });
    let (payload, segments) = build_snapshot_bytes_for_ring(
        &scrollback_data,
        &scrollback_segments,
        &screen_data,
        alt_screen,
        ring_wrapped,
        current_dims,
        10_000,
    );
    let construct = trailing_construct_bytes(&payload);
    let encoded = crate::mux::session::pane::encode_snapshot_segments(&payload, &segments);
    let mut guard = pane.output_capture.hold_boundary();
    tx.try_send(PtyOutputChunk::snapshot(pane.id, encoded))
        .expect("the on-demand snapshot must fit in the channel");
    guard.record_with_construct(tx, boundary, construct);
}

/// The production visibility restore (`resume_pane_with_permit`) after the
/// pane was hidden: the target is `Detached { HiddenByVisibility }` owned by
/// `tx`, and the resume sends its snapshot, records its boundary and
/// construct, and makes the target `Connected` again.
fn visibility_restore(pane: &MuxPane, tx: &mpsc::Sender<PtyOutputChunk>) {
    *pane.output_target.lock().unwrap() = PaneOutputTarget::Detached {
        reason: DetachReason::HiddenByVisibility,
        owner: Some(tx.clone()),
    };
    let permit = tx.try_reserve().expect("capacity for the resume permit");
    let outcome = resume_pane_with_permit(pane, tx, AnyPermit::Borrowed(permit), 10_000);
    assert!(matches!(outcome, ResumeOutcome::Resumed));
}

/// Drive `chunks` through the production reader. The reader is paused at P2
/// on the first chunk (after its capture step, before its forward decision)
/// and `at_p2` runs there; with `at_p4` given, the reader is then paused at
/// P4 (suppressed pipeline: slot secured, no exclusion held) and `at_p4`
/// runs there.
fn run<F: FnOnce(&Harness), G: FnOnce(&Harness)>(
    pane_id: PaneId,
    cap: usize,
    chunks: &[Vec<u8>],
    at_p2: F,
    at_p4: Option<G>,
) -> SuppressedRun {
    let h = Harness::new(pane_id, cap);
    let (p2_arrived, p2_release) = h.pane.output_capture.p2.arm();
    let p4 = at_p4.as_ref().map(|_| h.pane.output_capture.p4.arm());
    let handle = h.spawn_reader(chunks.to_vec());
    p2_arrived
        .recv_timeout(WAIT)
        .expect("the reader must reach P2 on the first chunk");
    at_p2(&h);
    p2_release.send(()).unwrap();
    if let (Some(at_p4), Some((p4_arrived, p4_release))) = (at_p4, p4) {
        p4_arrived
            .recv_timeout(WAIT)
            .expect("the reader must reach P4: the suppressed pipeline has a send-time point");
        at_p4(&h);
        p4_release.send(()).unwrap();
    }
    join_reader(handle);
    h.finish()
}

fn run_p2<F: FnOnce(&Harness)>(pane_id: PaneId, chunks: &[Vec<u8>], at_p2: F) -> SuppressedRun {
    run(pane_id, 16, chunks, at_p2, None::<fn(&Harness)>)
}

fn run_p2_p4<F: FnOnce(&Harness), G: FnOnce(&Harness)>(
    pane_id: PaneId,
    chunks: &[Vec<u8>],
    at_p2: F,
    at_p4: G,
) -> SuppressedRun {
    run(pane_id, 16, chunks, at_p2, Some(at_p4))
}

/// The only empty `PtyOutput` the reader sends is the EOF marker.
fn assert_only_eof_is_empty(run: &SuppressedRun, ctx: &str) {
    let empties = run
        .received
        .iter()
        .filter(|c| c.kind == R2ChunkKind::PtyOutput && c.data.is_empty())
        .count();
    assert_eq!(empties, 1, "{ctx}: only the EOF marker may be empty");
    assert!(
        run.received.last().is_some_and(|c| c.data.is_empty()),
        "{ctx}: the EOF marker comes last"
    );
}

/// Whether the last snapshot the client received reproduces the screen and
/// cursor a reference client shows for `prefix` (the bytes the snapshot
/// stands for). The on-demand snapshot assembly of a ring that ends in
/// `ESC (` appends its own screen-switch sequence, which the pending
/// designator swallows; the client then shows `[?1049l` as text. That
/// mismatch lies in the snapshot assembly (out of scope here), not in the
/// replacement the reader sends, so the screen comparison is skipped for it
/// (reader-level oracle convention, IMPLEMENTATION.md).
fn last_snapshot_reproduces_prefix(run: &SuppressedRun, prefix: &[u8]) -> bool {
    let (payload, segments) = run.snapshots.last().expect("a snapshot was delivered");
    let mut client = r2_client();
    client.reset_and_replay_segments(payload, &to_replay_segments(segments));
    let _discarded = client.take_response();
    let mut reference = r2_client();
    reference.process_pty_data_fully(prefix);
    let _discarded = reference.take_response();
    client.get_cursor_row() == reference.get_cursor_row()
        && client.get_cursor_col() == reference.get_cursor_col()
        && (0..R2_ROWS)
            .all(|r| client.get_line_text(r).trim_end() == reference.get_line_text(r).trim_end())
}

/// The responses equal the raw-stream reference's and no `(` is displayed
/// (the designator the pre-fix re-send prints; the reference rows of these
/// streams hold none). Screen, cursor and displayed characters equal the
/// reference's too, unless the last snapshot itself does not reproduce the
/// prefix (see [`last_snapshot_reproduces_prefix`]). Returns whether the
/// screen comparison ran.
fn check_client_against_reference(run: &SuppressedRun, chunks: &[Vec<u8>], ctx: &str) -> bool {
    assert_r2_responses_match(run, chunks, true, ctx);
    let (client, _responses) = r2_client_view(run);
    for r in 0..R2_ROWS {
        assert!(
            !client.get_line_text(r).contains('('),
            "{ctx}: row {r} displays a stray '(': {:?}",
            client.get_line_text(r)
        );
    }
    let compared = last_snapshot_reproduces_prefix(run, &chunks[0]);
    if compared {
        assert_r2_screen_matches(run, chunks, ctx);
    }
    compared
}

/// [`check_client_against_reference`], with the screen comparison required to
/// run (the last snapshot is a visibility restore, which reproduces the
/// prefix).
fn assert_client_equals_reference(run: &SuppressedRun, chunks: &[Vec<u8>], ctx: &str) {
    assert!(
        check_client_against_reference(run, chunks, ctx),
        "{ctx}: the last snapshot must reproduce the prefix, so the screen is compared"
    );
}

/// The next chunk's first byte (`0`) was consumed as the designator, so the
/// client displays line drawing and no literal `0lqk`.
fn assert_next_chunk_selected_the_charset(run: &SuppressedRun, ctx: &str) {
    let (client, _responses) = r2_client_view(run);
    let row = client.get_line_text(0);
    assert!(
        row.contains('\u{250c}') && !row.contains("0lqk"),
        "{ctx}: the next chunk's first byte must select DEC line drawing: {row:?}"
    );
}

fn paren_chunks() -> Vec<Vec<u8>> {
    vec![b"abc\x1b(".to_vec(), b"0lqk\x1b(B\r\nZ".to_vec()]
}

fn utf8_chunks(tail: &[u8], rest: &[u8]) -> Vec<Vec<u8>> {
    vec![
        [b"abc".as_slice(), tail].concat(),
        [rest, b"def\r\n"].concat(),
    ]
}

// ---- AC-4: the decision follows the record current at send time ----

/// (a) The stale record is `ESC (`, the current one (same boundary) is none:
/// the tail `ESC (` is sent, although the stale decision gave an empty
/// replacement.
fn check_case_a_stale_construct_current_none() {
    let chunks = paren_chunks();
    let run = run_p2_p4(
        500,
        &chunks,
        |h| visibility_restore(&h.pane, &h.tx),
        |h| send_on_demand_snapshot(&h.pane, &h.tx),
    );
    assert_eq!(
        run.pty_output(),
        vec![vec![ESC, b'('], chunks[1].clone()],
        "(a): the tail is sent, then the next chunk"
    );
    assert_only_eof_is_empty(&run, "(a)");
    check_client_against_reference(&run, &chunks, "(a)");
    assert_next_chunk_selected_the_charset(&run, "(a)");
}

/// (b) The stale record is none, the current one is `ESC (`: the tail is
/// omitted, so nothing is sent without items.
fn check_case_b_stale_none_current_construct() {
    let chunks = paren_chunks();
    let run = run_p2_p4(
        501,
        &chunks,
        |h| send_on_demand_snapshot(&h.pane, &h.tx),
        |h| visibility_restore(&h.pane, &h.tx),
    );
    assert_eq!(
        run.pty_output(),
        vec![chunks[1].clone()],
        "(b): nothing is re-sent between the snapshot and the next chunk"
    );
    assert_only_eof_is_empty(&run, "(b)");
    assert_client_equals_reference(&run, &chunks, "(b)");
}

/// (b) with items: the filler form is sent (filler, the query, the tail).
fn check_case_b_with_items_sends_the_filler_form() {
    let chunks = vec![b"\x1b[cabc\x1b(".to_vec(), b"0lqk\x1b(B\r\nZ".to_vec()];
    let run = run_p2_p4(
        502,
        &chunks,
        |h| send_on_demand_snapshot(&h.pane, &h.tx),
        |h| visibility_restore(&h.pane, &h.tx),
    );
    assert_eq!(
        run.pty_output(),
        vec![b"B\x1b[c\x1b(".to_vec(), chunks[1].clone()],
        "(b) with items: filler, query, tail, then the next chunk"
    );
    assert_only_eof_is_empty(&run, "(b) with items");
    assert_client_equals_reference(&run, &chunks, "(b) with items");
}

/// (c) The current record has a higher boundary: the tail is sent. Recorded
/// through the boundary-record API (a real reader cannot produce it); no
/// parity assertion.
fn check_case_c_current_boundary_is_higher() {
    let chunks = vec![b"abc\x1b(".to_vec()];
    let run = run_p2_p4(
        503,
        &chunks,
        |h| visibility_restore(&h.pane, &h.tx),
        |h| {
            h.pane
                .output_capture
                .record_boundary_with_construct(&h.tx, 2, None);
        },
    );
    assert_eq!(
        run.pty_output(),
        vec![vec![ESC, b'(']],
        "(c): the construct never applies below the boundary, so the tail is sent"
    );
    assert_only_eof_is_empty(&run, "(c)");
}

/// (d) The stale record is none, the current one is a UTF-8 lead byte: the
/// tail is omitted, and no replacement character appears when the next read
/// completes the character.
fn check_case_d_stale_none_current_utf8_lead() {
    for (index, (tail, rest)) in [
        (vec![0xe4u8, 0xb8], vec![0xadu8]),
        (vec![0xe4u8], vec![0xb8u8, 0xad]),
    ]
    .into_iter()
    .enumerate()
    {
        let chunks = utf8_chunks(&tail, &rest);
        let ctx = format!("(d) tail {tail:02x?}");
        let run = run_p2_p4(
            510 + index as PaneId,
            &chunks,
            |h| send_on_demand_snapshot(&h.pane, &h.tx),
            |h| visibility_restore(&h.pane, &h.tx),
        );
        assert_eq!(
            run.pty_output(),
            vec![chunks[1].clone()],
            "{ctx}: the lead byte is not re-sent"
        );
        assert_only_eof_is_empty(&run, &ctx);
        assert_client_equals_reference(&run, &chunks, &ctx);
    }
}

/// A record for another destination made at P4 does not change S's decision,
/// and S's replacement never reaches the other destination.
fn check_case_e_another_destinations_record_changes_nothing() {
    let chunks = paren_chunks();

    // S's stale record is none; the other destination records `ESC (`.
    let (other_tx, mut other_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let run = run_p2_p4(
        520,
        &chunks,
        |h| send_on_demand_snapshot(&h.pane, &h.tx),
        |h| {
            h.pane.output_capture.record_boundary_with_construct(
                &other_tx,
                1,
                Some(vec![ESC, b'(']),
            );
        },
    );
    assert_eq!(
        run.pty_output(),
        vec![vec![ESC, b'('], chunks[1].clone()],
        "(e1): S's own record (none) decides: the tail is sent"
    );
    assert!(
        other_rx.try_recv().is_err(),
        "(e1): S's replacement never reaches the other destination"
    );
    check_client_against_reference(&run, &chunks, "(e1)");
    assert_next_chunk_selected_the_charset(&run, "(e1)");

    // S's record carries `ESC (`; the other destination records none.
    let (other_tx, mut other_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let run = run_p2_p4(
        521,
        &chunks,
        |h| visibility_restore(&h.pane, &h.tx),
        |h| {
            h.pane
                .output_capture
                .record_boundary_with_construct(&other_tx, 1, None);
        },
    );
    assert_eq!(
        run.pty_output(),
        vec![chunks[1].clone()],
        "(e2): S's own record (`ESC (`) decides: nothing is re-sent"
    );
    assert!(
        other_rx.try_recv().is_err(),
        "(e2): nothing reaches the other destination"
    );
    assert_client_equals_reference(&run, &chunks, "(e2)");
}

/// AC-4 (FR6, TM-3, TS-6; registry, finding `6230e66b979e312e`): with the
/// reader paused at P4 after the covered decision, a second snapshot recorded
/// for the same destination before the send decides the tail omission. All
/// cases of the criterion.
#[test]
fn round3_6230e66b_tail_omission_uses_the_record_current_at_send_time() {
    check_case_a_stale_construct_current_none();
    check_case_b_stale_none_current_construct();
    check_case_b_with_items_sends_the_filler_form();
    check_case_c_current_boundary_is_higher();
    check_case_d_stale_none_current_utf8_lead();
    check_case_e_another_destinations_record_changes_nothing();
}

#[test]
fn round3_case_a_a_stale_construct_is_replaced_by_the_current_none() {
    check_case_a_stale_construct_current_none();
}

#[test]
fn round3_case_b_a_stale_none_is_replaced_by_the_current_construct() {
    check_case_b_stale_none_current_construct();
}

#[test]
fn round3_case_b_with_items_the_current_construct_gives_the_filler_form() {
    check_case_b_with_items_sends_the_filler_form();
}

#[test]
fn round3_case_c_a_higher_current_boundary_sends_the_tail() {
    check_case_c_current_boundary_is_higher();
}

#[test]
fn round3_case_d_a_utf8_lead_byte_is_not_re_sent_after_a_snapshot_that_carries_it() {
    check_case_d_stale_none_current_utf8_lead();
}

#[test]
fn round3_case_e_another_destinations_record_never_changes_the_decision() {
    check_case_e_another_destinations_record_changes_nothing();
}

/// With the record unchanged between the covered decision and the send, the
/// captured construct stands: the pre-existing behavior is kept.
#[test]
fn round3_an_unchanged_record_keeps_the_captured_decision() {
    let chunks = paren_chunks();
    let run = run_p2_p4(
        530,
        &chunks,
        |h| visibility_restore(&h.pane, &h.tx),
        |_h| {},
    );
    assert_eq!(run.pty_output(), vec![chunks[1].clone()]);
    assert_only_eof_is_empty(&run, "unchanged, construct");
    assert_client_equals_reference(&run, &chunks, "unchanged, construct");

    let run = run_p2_p4(
        531,
        &chunks,
        |h| send_on_demand_snapshot(&h.pane, &h.tx),
        |_h| {},
    );
    assert_eq!(
        run.pty_output(),
        vec![vec![ESC, b'('], chunks[1].clone()],
        "no construct recorded: the tail is re-sent"
    );
    assert_only_eof_is_empty(&run, "unchanged, none");
    check_client_against_reference(&run, &chunks, "unchanged, none");
    assert_next_chunk_selected_the_charset(&run, "unchanged, none");
}

// ---- AC-5: two snapshots at the same boundary before the covered decision ----

/// AC-5 (FR7, TS-7): on-demand (none) then visibility restore (`ESC (`) at
/// the same boundary: the later record wins, no `(` is re-sent, and the next
/// chunk's first byte is consumed as the designator, as in the reference.
#[test]
fn round3_on_demand_then_visibility_restore_leaves_the_restore_construct() {
    let chunks = paren_chunks();
    let run = run_p2(540, &chunks, |h| {
        send_on_demand_snapshot(&h.pane, &h.tx);
        visibility_restore(&h.pane, &h.tx);
    });
    assert_eq!(run.snapshots.len(), 2, "both snapshots reach the client");
    assert_eq!(
        run.pty_output(),
        vec![chunks[1].clone()],
        "no `(` is re-sent between the snapshot and the next chunk"
    );
    assert_only_eof_is_empty(&run, "on-demand then restore");
    assert_client_equals_reference(&run, &chunks, "on-demand then restore");
    assert_next_chunk_selected_the_charset(&run, "on-demand then restore");
}

/// AC-5 (FR7, TS-7): visibility restore (`ESC (`) then on-demand (none) at
/// the same boundary: the later record wins, so the tail `ESC (` is re-sent.
#[test]
fn round3_visibility_restore_then_on_demand_re_sends_the_tail() {
    let chunks = paren_chunks();
    let run = run_p2(541, &chunks, |h| {
        visibility_restore(&h.pane, &h.tx);
        send_on_demand_snapshot(&h.pane, &h.tx);
    });
    assert_eq!(run.snapshots.len(), 2, "both snapshots reach the client");
    assert_eq!(
        run.pty_output(),
        vec![vec![ESC, b'('], chunks[1].clone()],
        "the tail is re-sent, then the next chunk"
    );
    assert_only_eof_is_empty(&run, "restore then on-demand");
    check_client_against_reference(&run, &chunks, "restore then on-demand");
    assert_next_chunk_selected_the_charset(&run, "restore then on-demand");
}

// ---- AC-3: both call sites use the send sequence; empty results ----

/// The channel of the NeedsSlot tests: it holds the snapshot, the
/// replacement and the EOF marker at once, because the test joins the reader
/// before it drains the channel.
const NEEDS_SLOT_CAP: usize = 4;

/// AC-3: a suppressed chunk whose final replacement is empty sends no
/// `PtyOutput` chunk and releases its slot. The channel has room for exactly
/// the snapshot, the pipeline's slot at P4 and (after the slot is released)
/// the next chunk and the EOF marker.
#[test]
fn round3_an_empty_final_replacement_sends_nothing_and_releases_its_slot() {
    let chunks = paren_chunks();
    let run = run(
        550,
        3,
        &chunks,
        |h| visibility_restore(&h.pane, &h.tx),
        Some(|h: &Harness| {
            assert_eq!(
                h.tx.capacity(),
                1,
                "at P4 the snapshot and the pipeline's reserved slot occupy the channel"
            );
        }),
    );
    assert_eq!(
        run.received.len(),
        3,
        "the snapshot, the next chunk and the EOF marker only"
    );
    assert_eq!(run.pty_output(), vec![chunks[1].clone()]);
    assert_only_eof_is_empty(&run, "empty final");
}

/// AC-3 (call site 2): the chunk is first found uncovered with a full
/// channel (the NeedsSlot arm); a snapshot covering it is recorded while the
/// reader waits for a slot (P3). The covered re-check then runs the same send
/// sequence: the reader reaches P4 and decides the tail omission from the
/// record current at that point.
#[test]
fn round3_needs_slot_arm_decides_tail_omission_at_send_time() {
    let h = Harness::new(560, NEEDS_SLOT_CAP);
    // Saturate the channel so the reader's first insertion attempt is Full.
    for i in 0..NEEDS_SLOT_CAP as u8 {
        h.tx.try_send(PtyOutputChunk::pty_output(560, vec![b'f', i]))
            .expect("filler must fit");
    }
    let (p3_arrived, p3_release) = h.pane.output_capture.p3.arm();
    let (p4_arrived, p4_release) = h.pane.output_capture.p4.arm();
    let chunk = b"abc\x1b(".to_vec();
    let handle = h.spawn_reader(vec![chunk.clone()]);
    p3_arrived
        .recv_timeout(WAIT)
        .expect("the reader must reach P3 (Full insertion attempt)");

    // Free the channel, then record a covering snapshot (none) that takes one
    // slot, as an on-demand snapshot entering the channel would.
    let mut h = h;
    for i in 0..NEEDS_SLOT_CAP as u8 {
        assert_eq!(h.rx.try_recv().unwrap().data, vec![b'f', i]);
    }
    send_on_demand_snapshot(&h.pane, &h.tx);
    p3_release.send(()).unwrap();

    p4_arrived
        .recv_timeout(WAIT)
        .expect("the NeedsSlot arm's covered re-check must run the send sequence (P4)");
    // At P4 a later record for the same boundary carries `ESC (`.
    h.pane
        .output_capture
        .record_boundary_with_construct(&h.tx, 1, Some(vec![ESC, b'(']));
    p4_release.send(()).unwrap();
    join_reader(handle);

    let run = h.finish();
    assert!(
        run.pty_output().is_empty(),
        "the current record carries the tail, so nothing is sent, got {:?}",
        run.pty_output()
    );
    assert_only_eof_is_empty(&run, "needs slot");
}

/// AC-3 (call site 2, the other direction): without a later record the
/// captured decision stands and the tail is sent.
#[test]
fn round3_needs_slot_arm_sends_the_tail_when_no_construct_is_recorded() {
    let h = Harness::new(561, NEEDS_SLOT_CAP);
    for i in 0..NEEDS_SLOT_CAP as u8 {
        h.tx.try_send(PtyOutputChunk::pty_output(561, vec![b'f', i]))
            .expect("filler must fit");
    }
    let (p3_arrived, p3_release) = h.pane.output_capture.p3.arm();
    let handle = h.spawn_reader(vec![b"abc\x1b(".to_vec()]);
    p3_arrived.recv_timeout(WAIT).expect("P3");
    let mut h = h;
    for _ in 0..NEEDS_SLOT_CAP {
        h.rx.try_recv().unwrap();
    }
    send_on_demand_snapshot(&h.pane, &h.tx);
    p3_release.send(()).unwrap();
    join_reader(handle);
    let run = h.finish();
    assert_eq!(run.pty_output(), vec![vec![ESC, b'(']]);
    assert_only_eof_is_empty(&run, "needs slot, none");
}

// ---- AC-6: no blocking wait under either lock ----

/// Whether another thread can take `output_target`, then the boundary
/// exclusion, and also pass through the capture exclusion, within `within`.
fn locks_can_be_taken_within(h: &Harness, within: Duration) -> bool {
    let target = h.pane.output_target.clone();
    let capture = h.pane.output_capture.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        {
            let _target = target.lock().unwrap();
            let _boundary = capture.hold_boundary();
        }
        capture.captured_read(|| ());
        let _ = done_tx.send(());
    });
    let taken = done_rx.recv_timeout(within).is_ok();
    if taken {
        thread.join().unwrap();
    }
    taken
}

/// AC-6 (NFR2, TS-13): while the reader is blocked securing a slot on a full
/// channel inside the suppressed pipeline, another thread takes
/// `output_target` and then the boundary exclusion within a short timeout.
/// While it is paused at P4 (the slot secured), the capture exclusion, the
/// boundary exclusion and `output_target` are all free.
#[test]
fn round3_slot_wait_and_p4_hold_neither_exclusion() {
    let h = Harness::new(570, 1);
    let (notif_tx, mut notif_rx) = mpsc::channel(4);
    *h.pane.notification_sender.lock().unwrap() = Some(notif_tx);
    let (p2_arrived, p2_release) = h.pane.output_capture.p2.arm();
    let (p4_arrived, p4_release) = h.pane.output_capture.p4.arm();
    // A complete OSC 9 notification: once it is delivered the reader has run
    // the pipeline's passthrough capture and is about to secure its slot.
    let chunk = b"\x1b]9;slot-wait\x07abc\x1b(".to_vec();
    let handle = h.spawn_reader(vec![chunk]);
    p2_arrived.recv_timeout(WAIT).expect("P2");
    // The capacity-1 channel is full once the snapshot is in it.
    send_on_demand_snapshot(&h.pane, &h.tx);
    assert_eq!(h.tx.capacity(), 0, "the channel is full");
    p2_release.send(()).unwrap();

    wait_for(|| notif_rx.try_recv().is_ok())
        .then_some(())
        .expect("the reader runs the suppressed pipeline's passthrough capture");
    // The reader now waits for a slot. Give it time to reach the wait, then
    // require both locks to be takeable.
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        locks_can_be_taken_within(&h, Duration::from_secs(2)),
        "output_target and the boundary exclusion must be takeable while the reader waits for a slot"
    );

    // Free the slot (the snapshot is consumed): the reader proceeds to P4.
    let mut h = h;
    let snapshot = h.rx.try_recv().expect("the snapshot");
    assert_eq!(snapshot.kind, R2ChunkKind::Snapshot);
    p4_arrived.recv_timeout(WAIT).expect("P4");
    assert!(
        h.pane.output_capture.exclusions_are_free(),
        "neither the capture exclusion nor the boundary exclusion is held at P4"
    );
    assert!(
        h.pane.output_target.try_lock().is_ok(),
        "output_target is not held at P4"
    );
    assert!(locks_can_be_taken_within(&h, Duration::from_secs(2)));
    p4_release.send(()).unwrap();

    // The capacity-1 channel holds the replacement until it is consumed: drain
    // until the EOF marker, then join.
    let mut got = Vec::new();
    loop {
        let chunk = {
            let mut next = None;
            wait_for(|| {
                next = h.rx.try_recv().ok();
                next.is_some()
            })
            .then_some(())
            .expect("the reader sends its replacement and the EOF marker");
            next.unwrap()
        };
        let eof = chunk.data.is_empty();
        got.push(chunk);
        if eof {
            break;
        }
    }
    join_reader(handle);
    assert_eq!(
        got.iter()
            .filter(|c| !c.data.is_empty())
            .map(|c| c.data.clone())
            .collect::<Vec<_>>(),
        vec![vec![ESC, b'(']],
        "the tail is sent once the slot is secured (the on-demand record carries none)"
    );
}
