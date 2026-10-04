//! mux-strip-non-sixel-dcs-linear task0001 (FR1-FR6, NFR1, NFR2, TM-1, TM-2):
//! budget regression guards for repeated non-SIXEL DCS introducers on every mux
//! daemon path that scans PTY output.
//!
//! The input is `(ESC P x)*N` followed by one `ESC \` ([`input_of_len`]). The DCS
//! body `x` is not SIXEL, so no path removes a byte of it: the output of every
//! path is the input itself. Each `ESC P x` is closed by the `ESC` that opens the
//! next one, so the input is one chain of aborted strings that the trailing ST
//! settles. A scan that re-walks the chain for each introducer is quadratic in
//! the input length; at the sizes below it would exceed the 10 s budget by
//! orders of magnitude.
//!
//! The scans stop at the first `ESC` of a body (`scan_body_end`, `find_st`,
//! `find_st_terminator`), so these tests pass from their first run: they are
//! regression guards and no red state can be observed (SPEC.md A1, A3).
//!
//! Sizes (NFR1): 2 MiB for the strip, snapshot, direct-scan and write-filter
//! overflow calls; just under the 512 KiB pending cap for the held-chain write
//! filter cases; reader reads of at most 65,536 bytes. A debug build runs these
//! tests, so each 2 MiB input is passed over a small number of times.

use super::post_strip_cut_csi::BUDGET;
use super::round3_write_path::{DIMS, run_reader_without_owner};
use super::*;
use crate::mux::ipc::pty_spawn::client_parity_scan;
use crate::mux::scrollback_buffer::DEFAULT_SCROLLBACK_CAPACITY;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write_with_written_state, strip_replayable_rich_content,
    strip_rich_content_and_remap,
};
use crate::mux::snapshot_bytes::{build_resume_snapshot_bytes, build_snapshot_bytes};
use std::time::{Duration, Instant};

const ESC: u8 = 0x1b;

/// The size of the strip, snapshot, direct-scan and overflow inputs.
const TWO_MIB: usize = 2 * 1024 * 1024;

/// The read buffer of `pty_reader_loop`: no read is larger.
const READ_SIZE: usize = 65_536;

/// A held chain that stays under `SCROLLBACK_FILTER_PENDING_CAP` (512 KiB):
/// the filter holds all of it until the trailing ST.
const JUST_UNDER_THE_CAP: usize = SCROLLBACK_FILTER_PENDING_CAP - 3;

/// `(ESC P x)*N` + `ESC \`, with `N` the largest count that keeps the length at
/// or below `total` (the length is `3 * N + 2`, so it is `total` or up to two
/// bytes less). One generator for every case, so every case runs the same shape.
fn input_of_len(total: usize) -> Vec<u8> {
    assert!(total >= 5, "room for one unit and the ST");
    let units = (total - 2) / 3;
    let mut input = b"\x1bPx".repeat(units);
    input.extend_from_slice(b"\x1b\\");
    assert_eq!(input.len(), 3 * units + 2);
    assert!(input.len() <= total && input.len() + 3 > total);
    input
}

/// The input split into reads of at most [`READ_SIZE`] bytes.
fn reads_of(input: &[u8]) -> Vec<Vec<u8>> {
    input.chunks(READ_SIZE).map(<[u8]>::to_vec).collect()
}

/// Run `f` and return its result with the time it took. The observations are
/// asserted after the call, so the time never includes them.
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let result = f();
    (result, start.elapsed())
}

#[track_caller]
fn assert_within_the_budget(case: &str, elapsed: Duration) {
    assert!(elapsed < BUDGET, "{case}: took {elapsed:?}");
}

#[track_caller]
fn assert_same_bytes(case: &str, got: &[u8], input: &[u8]) {
    assert!(
        got == input,
        "{case}: expected the input ({} bytes) unchanged, got {} bytes",
        input.len(),
        got.len()
    );
}

// ── AC-1 (FR1, NFR1, TM-1, TM-2): the shared strip ───────────────────────

/// AC-1 (FR1, NFR1, TM-1, TM-2): `strip_replayable_rich_content` on the 2 MiB
/// input returns it byte for byte within the budget.
#[test]
fn non_sixel_dcs_linear_strip_replayable_rich_content_returns_the_input_within_the_budget() {
    let case = "strip_replayable_rich_content";
    let input = input_of_len(TWO_MIB);
    let (out, elapsed) = timed(|| strip_replayable_rich_content(&input));
    assert_same_bytes(case, &out, &input);
    assert_within_the_budget(case, elapsed);
}

/// AC-1 (FR1, NFR1, TM-1, TM-2): `strip_pty_output_for_scrollback_write` on the
/// 2 MiB input returns it byte for byte within the budget.
#[test]
fn non_sixel_dcs_linear_strip_pty_output_for_scrollback_write_returns_the_input_within_the_budget()
{
    let case = "strip_pty_output_for_scrollback_write";
    let input = input_of_len(TWO_MIB);
    let (out, elapsed) = timed(|| strip_pty_output_for_scrollback_write(&input));
    assert_same_bytes(case, &out, &input);
    assert_within_the_budget(case, elapsed);
}

/// AC-1 (FR1, NFR1, TM-1, TM-2): `strip_rich_content_and_remap` on the 2 MiB
/// input with ascending watch offsets spread over it (at a unit start, inside a
/// unit, at the last byte and at the end) returns it byte for byte within the
/// budget. Nothing is removed, so every watch offset maps to itself.
#[test]
fn non_sixel_dcs_linear_strip_rich_content_and_remap_returns_the_input_within_the_budget() {
    let case = "strip_rich_content_and_remap";
    let input = input_of_len(TWO_MIB);
    let len = input.len();
    let offsets = [
        0,
        1,
        2,
        3,
        4,
        1_000,
        len / 4,
        len / 4 + 1,
        len / 2,
        len / 2 + 2,
        len - 5,
        len - 2,
        len - 1,
        len,
    ];
    assert!(offsets.windows(2).all(|w| w[0] <= w[1]), "ascending");
    let ((out, remapped), elapsed) = timed(|| strip_rich_content_and_remap(&input, &offsets));
    assert_same_bytes(case, &out, &input);
    assert_eq!(
        remapped, offsets,
        "{case}: every watch offset maps to itself"
    );
    assert_within_the_budget(case, elapsed);
}

/// AC-1 (FR1, NFR1, TM-1, TM-2): the written-state form
/// `strip_pty_output_for_scrollback_write_with_written_state`, started in ground
/// with no pending designator, returns the 2 MiB input byte for byte within the
/// budget and reports ground after the trailing ST.
#[test]
fn non_sixel_dcs_linear_the_written_state_form_returns_the_input_within_the_budget() {
    let case = "strip_pty_output_for_scrollback_write_with_written_state";
    let input = input_of_len(TWO_MIB);
    let ((out, state), elapsed) = timed(|| {
        strip_pty_output_for_scrollback_write_with_written_state(
            &input,
            false,
            WrittenState::Ground,
        )
    });
    assert_same_bytes(case, &out, &input);
    assert_eq!(
        state,
        WrittenState::Ground,
        "{case}: the trailing ST settles the chain"
    );
    assert_within_the_budget(case, elapsed);
}

// ── AC-2 (FR2, NFR1, TM-1): the write filter ─────────────────────────────

/// Feed `pieces` to one filter in order, with no cut. Returns the bytes emitted
/// per feed (in feed order), the filter, and the time the feeds took (the
/// assertions are not timed). With `held_chain`, the filter must hold all bytes
/// fed so far after every feed but the last: the chain is held whole.
fn feed_pieces_timed(
    pieces: &[&[u8]],
    held_chain: bool,
    case: &str,
) -> (Vec<Vec<u8>>, ScrollbackWriteFilter, Duration) {
    let mut filter = ScrollbackWriteFilter::new();
    let mut emitted = Vec::with_capacity(pieces.len());
    let mut fed = 0usize;
    let mut elapsed = Duration::ZERO;
    for (index, piece) in pieces.iter().enumerate() {
        let (outcome_bytes, took) = timed(|| filter.feed(piece, DIMS).1);
        elapsed += took;
        fed += piece.len();
        emitted.push(outcome_bytes);
        if held_chain && index + 1 < pieces.len() {
            assert_eq!(
                filter.pending_len(),
                fed,
                "{case}: after piece {index} the filter holds the whole chain fed so far"
            );
        }
    }
    (emitted, filter, elapsed)
}

/// AC-2 (FR2, NFR1, TM-1): the whole chain just under the pending cap in one
/// call is emitted unchanged within the budget and nothing stays pending.
#[test]
fn non_sixel_dcs_linear_the_write_filter_takes_a_chain_under_the_cap_in_one_call() {
    let case = "write filter, one call under the cap";
    let input = input_of_len(JUST_UNDER_THE_CAP);
    assert!(input.len() <= SCROLLBACK_FILTER_PENDING_CAP);
    let (emitted, filter, elapsed) = feed_pieces_timed(&[&input], false, case);
    assert_same_bytes(case, &emitted.concat(), &input);
    assert!(filter.pending().is_empty(), "{case}: pending is empty");
    assert_eq!(filter.written_state(), WrittenState::Ground, "{case}");
    assert_within_the_budget(case, elapsed);
}

/// AC-2 (FR2, NFR1, TM-1): the same chain in reads of at most 65,536 bytes. The
/// filter holds the whole chain after every read but the last (it is one chain of
/// aborted strings and the ST is still to come), then the trailing ST releases
/// it: the emitted bytes equal the input within the budget and nothing stays
/// pending.
#[test]
fn non_sixel_dcs_linear_the_write_filter_takes_a_chain_under_the_cap_in_reader_sized_pieces() {
    let case = "write filter, pieces under the cap";
    let input = input_of_len(JUST_UNDER_THE_CAP);
    let pieces: Vec<&[u8]> = input.chunks(READ_SIZE).collect();
    assert!(pieces.len() > 1, "{case}: more than one piece");
    let (emitted, filter, elapsed) = feed_pieces_timed(&pieces, true, case);
    assert_same_bytes(case, &emitted.concat(), &input);
    assert!(filter.pending().is_empty(), "{case}: pending is empty");
    assert_eq!(filter.written_state(), WrittenState::Ground, "{case}");
    assert_within_the_budget(case, elapsed);
}

/// AC-2 (FR2, NFR1, TM-1): a chain past the pending cap makes the overflow flush
/// run, in one call (the 2 MiB input) and in reads of at most 65,536 bytes (the
/// flush runs several times, at every alignment of the cut-off against a unit).
/// Each case emits the input unchanged within the budget and leaves nothing
/// pending after the trailing ST. In pieces, bytes are emitted before the last
/// piece, which shows that the flush ran.
#[test]
fn non_sixel_dcs_linear_the_write_filter_flushes_a_chain_past_the_cap_in_linear_time() {
    let input = input_of_len(TWO_MIB);
    assert!(input.len() > SCROLLBACK_FILTER_PENDING_CAP);

    let case = "write filter, one call past the cap";
    let (emitted, filter, elapsed) = feed_pieces_timed(&[&input], false, case);
    assert_same_bytes(case, &emitted.concat(), &input);
    assert!(filter.pending().is_empty(), "{case}: pending is empty");
    assert_within_the_budget(case, elapsed);

    let case = "write filter, pieces past the cap";
    let pieces: Vec<&[u8]> = input.chunks(READ_SIZE).collect();
    let (emitted, filter, elapsed) = feed_pieces_timed(&pieces, false, case);
    let before_the_last: usize = emitted[..emitted.len() - 1].iter().map(Vec::len).sum();
    assert!(
        before_the_last > 0,
        "{case}: the overflow flush emitted nothing before the last piece"
    );
    assert_same_bytes(case, &emitted.concat(), &input);
    assert!(filter.pending().is_empty(), "{case}: pending is empty");
    assert_eq!(filter.written_state(), WrittenState::Ground, "{case}");
    assert_within_the_budget(case, elapsed);
}

// ── AC-3 (FR3, NFR1, TM-1): the snapshot builders ────────────────────────

/// What `build_snapshot_bytes` puts before and after the scrollback of a
/// main-buffer pane with no screen dump (`snapshot_bytes::tests::
/// build_snapshot_bytes_main_buffer_omits_screen_part` pins the layout).
const SNAPSHOT_HEAD: &[u8] = b"\x1b[3J\x1b[H\x1b[2J";
const SNAPSHOT_TAIL: &[u8] = b"\x1b[?1049l";

/// What `build_resume_snapshot_bytes` puts before the scrollback of a main-buffer
/// pane (it has no toggle after it).
const RESUME_HEAD: &[u8] = b"\x1b[H\x1b[2J";

/// Dimension segments over `len` bytes of scrollback: offsets spread over the
/// input, at a unit start and inside a unit, ascending.
fn dimension_segments(len: usize) -> Vec<(usize, u16, u16)> {
    let offsets = [
        0,
        1,
        2,
        100,
        len / 4,
        len / 4 + 1,
        len / 2 + 2,
        len - 5,
        len - 1,
    ];
    let dims = [(80, 24), (100, 40), (120, 30)];
    offsets
        .iter()
        .enumerate()
        .map(|(i, &off)| (off, dims[i % dims.len()].0, dims[i % dims.len()].1))
        .collect()
}

/// The checks of AC-3 for one builder: the scrollback part of `payload` between
/// `head` and `tail` is the input, and the mapped segments keep their dims, are
/// non-decreasing and lie within the payload. Nothing is removed, so each
/// segment but one at offset 0 sits at its input offset shifted by the head.
fn check_snapshot(
    case: &str,
    input: &[u8],
    given: &[(usize, u16, u16)],
    built: &(Vec<u8>, Vec<(usize, u16, u16)>),
    head: &[u8],
    tail: &[u8],
) {
    let (payload, segments) = built;
    let scrollback = payload
        .strip_prefix(head)
        .and_then(|rest| rest.strip_suffix(tail))
        .unwrap_or_else(|| {
            panic!(
                "{case}: unexpected payload layout ({} bytes)",
                payload.len()
            )
        });
    assert_same_bytes(case, scrollback, input);

    assert_eq!(segments.len(), given.len(), "{case}: one segment each");
    for (mapped, original) in segments.iter().zip(given) {
        assert_eq!(
            (mapped.1, mapped.2),
            (original.1, original.2),
            "{case}: dims of the segment at {}",
            original.0
        );
        let expected = if original.0 == 0 {
            0
        } else {
            original.0 + head.len()
        };
        assert_eq!(
            mapped.0, expected,
            "{case}: the segment at input offset {} maps to itself shifted by the head",
            original.0
        );
        assert!(
            mapped.0 <= payload.len(),
            "{case}: segment offset {} past the payload ({} bytes)",
            mapped.0,
            payload.len()
        );
    }
    assert!(
        segments.windows(2).all(|w| w[0].0 <= w[1].0),
        "{case}: mapped segment offsets are non-decreasing"
    );
}

/// AC-3 (FR3, NFR1, TM-1): `build_snapshot_bytes` on a 2 MiB scrollback of the
/// input (main-buffer pane, dimension segments spread over it, some inside a
/// unit) finishes within the budget; the scrollback part of the payload is the
/// input and the mapped segment offsets are non-decreasing and within the
/// payload.
#[test]
fn non_sixel_dcs_linear_build_snapshot_bytes_keeps_the_scrollback_within_the_budget() {
    let case = "build_snapshot_bytes";
    let input = input_of_len(TWO_MIB);
    let segments = dimension_segments(input.len());
    let (built, elapsed) = timed(|| build_snapshot_bytes(&input, &segments, b"", false, DIMS));
    check_snapshot(
        case,
        &input,
        &segments,
        &built,
        SNAPSHOT_HEAD,
        SNAPSHOT_TAIL,
    );
    assert_within_the_budget(case, elapsed);
}

/// AC-3 (FR3, NFR1, TM-1): `build_resume_snapshot_bytes` on the same scrollback
/// and segments, with the same observations (the layout of the visibility-resume
/// payload of a main-buffer pane has no tail).
#[test]
fn non_sixel_dcs_linear_build_resume_snapshot_bytes_keeps_the_scrollback_within_the_budget() {
    let case = "build_resume_snapshot_bytes";
    let input = input_of_len(TWO_MIB);
    let segments = dimension_segments(input.len());
    let (built, elapsed) =
        timed(|| build_resume_snapshot_bytes(&input, &segments, b"", false, DIMS));
    check_snapshot(case, &input, &segments, &built, RESUME_HEAD, b"");
    assert_within_the_budget(case, elapsed);
}

// ── AC-4 (FR4, NFR1, TM-1): the Detached production reader ───────────────

/// AC-4 (FR4, NFR1, TM-1): the production reader of a pane with no connected
/// owner (Detached) is driven with the input in reads of at most 65,536 bytes,
/// as long as the ring holds (2 MiB), which passes the 512 KiB pending cap (the
/// overflow flush runs in the reader's write filter several times). The run
/// finishes within the budget and the ring holds the input.
#[test]
fn non_sixel_dcs_linear_the_detached_reader_writes_the_input_to_the_ring_within_the_budget() {
    let case = "Detached reader";
    let input = input_of_len(DEFAULT_SCROLLBACK_CAPACITY);
    assert!(input.len() <= DEFAULT_SCROLLBACK_CAPACITY);
    assert!(input.len() > SCROLLBACK_FILTER_PENDING_CAP);
    let reads = reads_of(&input);
    assert!(reads.iter().all(|read| read.len() <= READ_SIZE));
    let (ring, elapsed) = timed(|| run_reader_without_owner(reads));
    assert_same_bytes(case, &ring, &input);
    assert_within_the_budget(case, elapsed);
}

// ── AC-5 (FR5, FR6, NFR1, TM-1): the client-parity scan ──────────────────

/// AC-5 (FR5, NFR1, TM-1): `client_parity_scan::scan` called directly with an
/// empty window and the 2 MiB input as one chunk finishes within the budget with
/// no excluded piece and with excluded pieces (a piece in the middle and the
/// suffix a held chain would occupy; chunk coordinates, ascending, not
/// overlapping). Neither call reports an item; with no excluded piece and the
/// trailing complete ST the scan reports no tail.
#[test]
fn non_sixel_dcs_linear_the_client_parity_scan_reports_nothing_within_the_budget() {
    let input = input_of_len(TWO_MIB);
    let len = input.len();

    let case = "client_parity_scan::scan, no excluded piece";
    let (outcome, elapsed) = timed(|| client_parity_scan::scan(&[], &input, &[]));
    assert!(outcome.items.is_empty(), "{case}: no item");
    assert!(
        outcome.tail.is_none(),
        "{case}: no tail after the trailing ST, got {:?}",
        outcome.tail
    );
    assert_within_the_budget(case, elapsed);

    let case = "client_parity_scan::scan, excluded pieces";
    let excluded = [100_000..200_000, (len - 300_000)..len];
    let (outcome, elapsed) = timed(|| client_parity_scan::scan(&[], &input, &excluded));
    assert!(outcome.items.is_empty(), "{case}: no item");
    assert_within_the_budget(case, elapsed);
}

/// AC-5 (FR6, NFR1, TM-1): the production reader with suppressed reads, so both
/// branches of `prepare_suppressed_replacement` run. The input is cut into six
/// reads of at most 65,536 bytes, the ST at the end of the last read, and the
/// chain is held whole by the write filter until that ST (it stays under the
/// pending cap):
///
/// - read 2 is suppressed while the filter holds the chain (non-empty pending
///   after it): the scan runs with excluded pieces and the held chain is the
///   replacement, so the destination receives it as one chunk;
/// - read 5 carries the trailing ST and is suppressed (empty pending after it):
///   the scan runs with no excluded piece, finds neither item nor tail, and no
///   chunk is sent for it.
///
/// The harness's channel holds 16 chunks and has no consumer until join: reads
/// 0, 1, 3 and 4 (4 chunks), the two stand-in snapshots, the replacement of read
/// 2 and the EOF marker make 8. The run finishes within the budget, the ring
/// holds the input, and no chunk but the EOF marker is empty.
#[test]
fn non_sixel_dcs_linear_the_reader_with_suppressed_reads_runs_both_replacement_branches() {
    let case = "reader with suppressed reads";
    let input = input_of_len(6 * READ_SIZE);
    assert!(input.len() <= SCROLLBACK_FILTER_PENDING_CAP);
    let reads = reads_of(&input);
    assert_eq!(reads.len(), 6, "{case}: six reads");
    let (suppressed_while_held, suppressed_at_the_st) = (2, 5);
    let held_chain: Vec<u8> = reads[..=suppressed_while_held].concat();

    let (run, elapsed) = timed(|| {
        run_reader_with_suppressed_reads(&reads, &[suppressed_while_held, suppressed_at_the_st])
    });

    assert_same_bytes(case, &run.ring, &input);
    let (eof, rest) = run.received.split_last().expect("deliveries");
    assert!(
        eof.data.is_empty() && eof.kind == R2ChunkKind::PtyOutput,
        "{case}: the last chunk is the EOF marker"
    );
    for (index, chunk) in rest.iter().enumerate() {
        assert!(
            !chunk.data.is_empty(),
            "{case}: chunk {index} other than the EOF marker is empty"
        );
    }
    let output = run.pty_output();
    assert_eq!(
        output.len(),
        5,
        "{case}: reads 0, 1, 3 and 4, and the replacement of read 2; nothing for read 5"
    );
    assert!(
        output.iter().any(|data| *data == held_chain),
        "{case}: the replacement of read 2 is the held chain ({} bytes)",
        held_chain.len()
    );
    assert_within_the_budget(case, elapsed);
}
