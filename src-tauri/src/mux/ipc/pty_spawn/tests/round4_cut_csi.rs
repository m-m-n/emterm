//! mux-suppressed-output-round4-fixes task0004 (FR4): a CSI still in progress
//! at a cut.
//!
//! A removed 47 / 1047 / 1049 `h` / `l` switch starts with an `ESC`, which
//! aborts the CSI the live parser is inside. The ring never holds that
//! switch, so a CSI the ring's last bytes leave open must be closed at the cut;
//! otherwise a later byte completes it on replay (`ESC[6` ... `n` answers a
//! cursor-position query the raw stream never made).
//!
//! Oracle convention (IMPLEMENTATION.md "Reader-level oracle convention"): the
//! reference is term_core fed the raw stream once; responses, screen, cursor
//! and displayed characters of the replaying client are compared with it. The
//! reader-level cases use the visibility-restore harness of
//! `round3_write_path`; the filter-level cases feed the write filter directly
//! and replay what it emitted through term_core.

use super::round3_write_path::{
    DIMS, assert_client_equals_reference, run_reader_without_owner, run_visibility_restore_at,
    switch_pairs,
};
use super::*;
use crate::mux::scrollback_filter::{
    strip_pty_output_for_scrollback_write, strip_replayable_rich_content,
};
use term_core::terminal_core::TerminalCore;

const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

// ── helpers ──────────────────────────────────────────────────────────────

/// A client model with a small scrollback: the many-case loops below build
/// thousands of them.
fn small_core() -> TerminalCore {
    TerminalCore::new(80, 24, 100)
}

/// What a client shows and answers after a byte stream.
#[derive(Debug, PartialEq)]
struct View {
    rows: Vec<String>,
    cursor: (u16, u16),
    responses: Vec<u8>,
}

fn view_of(stream: &[u8]) -> View {
    let mut core = small_core();
    core.process_pty_data_fully(stream);
    let responses = core.take_response();
    View {
        rows: (0..24)
            .map(|r| core.get_line_text(r).trim_end().to_string())
            .collect(),
        cursor: (core.get_cursor_row(), core.get_cursor_col()),
        responses,
    }
}

/// Feed `fed` to a fresh filter with `cuts`, then each of `later` as a
/// cut-free call; returns every emitted byte, in order.
fn emitted_through(fed: &[u8], cuts: &[usize], later: &[&[u8]]) -> Vec<u8> {
    let mut filter = ScrollbackWriteFilter::new();
    let mut out = filter.feed_with_cuts(fed, DIMS, cuts).bytes;
    for piece in later {
        out.extend_from_slice(&filter.feed(piece, DIMS).1);
    }
    out
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether a client that has been fed `stream` is still inside a CSI: an `m`
/// (a final byte) is then consumed by that CSI, in ground it is displayed.
/// Only meaningful for a stream that ends in a CSI or in ground.
fn client_is_inside_a_csi(stream: &[u8]) -> bool {
    let probe = [stream, b"m"].concat();
    let view = view_of(&probe);
    !view.rows.iter().any(|row| row.contains('m'))
}

// ── AC-1 / AC-2 (FR4, TM-1, registry): the reader-level test ─────────────

/// An open CSI and the byte that would complete it into a query the raw
/// stream never makes.
const HEADS_AND_FINALS: &[(&[u8], &[u8])] = &[
    (b"\x1b[6", b"n"),
    (b"\x1b[5", b"n"),
    (b"\x1b[", b"c"),
    (b"\x1b[>", b"c"),
    (b"\x1b[?6", b"n"),
    (b"\x1b[18", b"t"),
];

/// AC-1 / AC-2 (FR4, SPEC AC-6, TM-1, TS-8, registry): an open CSI, a removed
/// 47 / 1047 / 1049 `h` / `l` pair and the completing byte in a later read,
/// through the production reader and the production visibility restore of a
/// main-screen pane whose ring has not wrapped:
/// - everything in one read, the snapshot covering the switch read;
/// - the same with the snapshot taken between the switch read and the
///   completing byte;
/// - the CSI split across reads (its last byte arrives with the switch), the
///   snapshot covering the switch read;
/// - the split with the snapshot between the switch read and the completing
///   byte.
///
/// The client's responses, screen, cursor and displayed characters equal the
/// raw-stream reference's: the live parser's CSI was aborted by the switch's
/// ESC, so the later byte is plain text there.
#[test]
fn round4_fr4_in_progress_csi_at_a_cut_matches_the_raw_stream_reference() {
    for (enter, leave) in switch_pairs() {
        let form = text(enter);
        let pair = [enter, leave].concat();
        for (head, last) in HEADS_AND_FINALS {
            let label = format!("{form} head {} final {}", text(head), text(last));

            // One read, the snapshot covering the switch read.
            let chunks = vec![[*head, &pair].concat(), last.to_vec()];
            let run = run_visibility_restore_at(&chunks, 0);
            assert_client_equals_reference(&run.received, &chunks, &format!("(a) {label}"));

            // The snapshot between the switch read and the completing byte.
            let chunks = vec![[*head, &pair].concat(), b"\r".to_vec(), last.to_vec()];
            let run = run_visibility_restore_at(&chunks, 1);
            assert_client_equals_reference(&run.received, &chunks, &format!("(b) {label}"));

            // Split across reads: the last byte of the CSI head arrives with
            // the switch.
            let (first, rest) = head.split_at(head.len() - 1);
            let chunks = vec![first.to_vec(), [rest, &pair].concat(), last.to_vec()];
            let run = run_visibility_restore_at(&chunks, 1);
            assert_client_equals_reference(&run.received, &chunks, &format!("(c) {label}"));

            // The split with the snapshot between.
            let chunks = vec![
                first.to_vec(),
                [rest, &pair].concat(),
                b"\r".to_vec(),
                last.to_vec(),
            ];
            let run = run_visibility_restore_at(&chunks, 2);
            assert_client_equals_reference(&run.received, &chunks, &format!("(d) {label}"));
        }
    }
}

/// AC-2 (FR4, TM-1): the reader's fallback closing. The switch straddles
/// reads, so the reader feeds an empty range with one cut at fed 0 once the
/// shadow parser has entered the alternate screen; the CSI the ring was left
/// inside - through a held lone ESC the fallback drops, or through the ring's
/// own bytes - is closed there. The snapshot covers the read that leaves the
/// alternate screen, so the byte after it reaches the client with the ring's
/// last state and not behind a live ESC.
#[test]
fn round4_fr4_the_reader_fallback_closes_an_open_csi() {
    for (enter, leave) in switch_pairs() {
        let form = text(enter);
        // The ring is inside `ESC[6` when the fallback runs, with the ESC of
        // the straddling switch held by the filter and dropped by the cut.
        let chunks = vec![
            b"\x1b[6".to_vec(),
            b"\x1b".to_vec(),
            enter[1..].to_vec(),
            leave.to_vec(),
            b"n".to_vec(),
        ];
        let run = run_visibility_restore_at(&chunks, 3);
        assert_client_equals_reference(&run.received, &chunks, &format!("held ESC, {form}"));

        // The straddling tail itself leaves a new CSI open in the ring: the
        // switch's bytes up to its last character arrive in one read, the
        // rest of the switch in the next.
        let split_at = enter.len() - 1;
        let chunks = vec![
            b"\x1b[6".to_vec(),
            enter[..split_at].to_vec(),
            enter[split_at..].to_vec(),
            leave.to_vec(),
            b"n".to_vec(),
        ];
        let run = run_visibility_restore_at(&chunks, 3);
        assert_client_equals_reference(&run.received, &chunks, &format!("open CSI, {form}"));
    }
}

// ── AC-3 (FR4): the write filter against a term_core oracle ──────────────

/// What a feed ending in a cut leaves, for each way a CSI can stand at the
/// end of the emitted run. None of these holds a complete device query, which
/// the ring write strips and the raw stream answers.
const CUT_PREFIXES: &[&[u8]] = &[
    // Parameters.
    b"\x1b[",
    b"\x1b[6",
    b"\x1b[12;34",
    b"\x1b[?25",
    b"\x1b[?",
    b"\x1b[>",
    b"\x1b[;",
    b"\x1b[:",
    b"\x1b[38:2:1:2:3",
    // Intermediates.
    b"\x1b[ ",
    b"\x1b[6 ",
    b"\x1b[6$",
    b"\x1b[?1$",
    b"\x1b[ !",
    // C0 controls execute inside the CSI and leave it open.
    b"\x1b[6\r",
    b"\x1b[\n",
    b"\x1b[6\x08",
    b"\x1b[6\x18",
    b"\x1b[6\x1a",
    b"\x1b[\x00",
    // Text before.
    b"abc\x1b[6",
    b"abc\x1b[1m\x1b[6",
    // An ESC aborts the CSI; what follows may open another one, or a
    // construct a cut drops.
    b"\x1b[6\x1b[7",
    b"\x1b[6\x1b[",
    b"\x1b[6\x1b",
    b"\x1b[6\x1b]0;t",
    // The CSI is completed before the cut.
    b"\x1b[6m",
    b"\x1b[1;2H",
    b"abc\x1b[6m",
    // Bytes that cancel it before the cut.
    b"\x1b[6\x7f",
    b"\x1b[!",
    b"\x1b[6?",
    b"\x1b[6<",
    b"\x1b[6=",
    b"\x1b[6>",
    b"\x1b[6\x80",
    b"\x1b[6\xff",
    // An ESC aborts it and starts a complete escape.
    b"\x1b[6\x1bX",
    b"\x1b[6\x1b7",
    b"\x1b[6\x1bM",
];

/// Continuations that tell a parser still inside the aborted CSI from one in
/// ground: finals that complete queries, other finals, a C0 control, text,
/// and bytes that cancel.
const CONTINUATIONS: &[&[u8]] = &[
    b"n", b"m", b"c", b"A", b"H", b"t", b"\r", b"[6n", b"6n", b";5H", b"x", b"Z", b"\x7f", b"?25h",
    b" q", b"$p",
];

/// AC-3 (FR4, TM-1): for every way a CSI can stand at a cut, term_core fed
/// what the filter emitted at the cut followed by a continuation shows and
/// answers what it does for the raw stream with the removed switch inline.
#[test]
fn round4_fr4_the_filter_output_at_a_cut_replays_like_the_raw_stream() {
    let start = std::time::Instant::now();
    for (enter, leave) in switch_pairs() {
        let pair = [enter, leave].concat();
        for prefix in CUT_PREFIXES {
            for continuation in CONTINUATIONS {
                let reference = [*prefix, &pair[..], *continuation].concat();
                let ring = emitted_through(prefix, &[prefix.len()], &[*continuation]);
                assert_eq!(
                    view_of(&ring),
                    view_of(&reference),
                    "prefix {:?} cut, then {:?}, switch {}: the replayed ring differs \
                     from the raw stream",
                    text(prefix),
                    text(continuation),
                    text(enter),
                );
            }
        }
    }
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

/// AC-3 (FR4): the same with the cut in the middle of one call (bytes after
/// it start from ground) and with the CSI carried in from an earlier call.
#[test]
fn round4_fr4_a_cut_inside_a_call_and_a_carried_csi_replay_like_the_raw_stream() {
    let (enter, leave) = (&b"\x1b[?1049h"[..], &b"\x1b[?1049l"[..]);
    let pair = [enter, leave].concat();
    for prefix in CUT_PREFIXES {
        for continuation in CONTINUATIONS {
            let reference = [*prefix, &pair[..], *continuation].concat();

            // One call: the bytes after the cut are fed with it.
            let one_call =
                emitted_through(&[*prefix, *continuation].concat(), &[prefix.len()], &[]);
            assert_eq!(
                view_of(&one_call),
                view_of(&reference),
                "prefix {:?} cut, then {:?} in the same call",
                text(prefix),
                text(continuation),
            );

            // The prefix in an earlier call, the cut at fed 0 of the next.
            let mut filter = ScrollbackWriteFilter::new();
            let mut carried = filter.feed(prefix, DIMS).1;
            carried.extend_from_slice(&filter.feed_with_cuts(continuation, DIMS, &[0]).bytes);
            assert_eq!(
                view_of(&carried),
                view_of(&reference),
                "prefix {:?} fed earlier, cut at fed 0, then {:?}",
                text(prefix),
                text(continuation),
            );

            // The prefix in an earlier call, an empty fed range with the
            // cut (the reader's fallback), then the continuation.
            let mut filter = ScrollbackWriteFilter::new();
            let mut fallback = filter.feed(prefix, DIMS).1;
            fallback.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
            fallback.extend_from_slice(&filter.feed(continuation, DIMS).1);
            assert_eq!(
                view_of(&fallback),
                view_of(&reference),
                "prefix {:?} fed earlier, fallback closing, then {:?}",
                text(prefix),
                text(continuation),
            );
        }
    }
}

/// The held run of an OSC that fills the pending buffer exactly to the cap
/// (still held: only a run past the cap is flushed).
fn osc_held_at_the_cap() -> Vec<u8> {
    let mut held = b"\x1b]0;".to_vec();
    held.resize(SCROLLBACK_FILTER_PENDING_CAP, b'p');
    held
}

/// AC-2 (FR4): an overflow flush followed by a cut. The held OSC grows to the
/// cap, the next feed completes it past the cap and leaves a CSI open; the
/// flush emits the run and the cut that follows in the same call closes the
/// CSI.
#[test]
fn round4_fr4_an_overflow_flush_followed_by_a_cut_closes_an_open_csi() {
    let pair = b"\x1b[?1049h\x1b[?1049l".to_vec();
    let held = osc_held_at_the_cap();
    // The continuation of the held OSC: more body bytes, its BEL, text and an
    // open CSI; it takes the run past the cap.
    let tail: Vec<u8> = [&b"pppppppppp"[..], b"\x07", b"abc", b"\x1b[6"].concat();

    for completing in [&b"n"[..], b"c", b"t"] {
        let reference = view_of(&[&held[..], &tail[..], &pair[..], completing].concat());

        // The flush, then the cut at the end of the call; the byte that would
        // complete the CSI arrives in a later call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty());
        let outcome = filter.feed_with_cuts(&tail, DIMS, &[tail.len()]);
        assert!(
            outcome.carried.is_none(),
            "the flushed call reports no completion"
        );
        let mut ring = outcome.bytes.clone();
        ring.extend_from_slice(&filter.feed(completing, DIMS).1);
        assert_eq!(
            view_of(&ring),
            reference,
            "completing {:?}: flush then cut",
            text(completing)
        );

        // The flush in a segment that a cut follows, with the completing byte
        // fed after the cut in the same call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty());
        let fed = [&tail[..], completing].concat();
        let ring = filter.feed_with_cuts(&fed, DIMS, &[tail.len()]).bytes;
        assert_eq!(
            view_of(&ring),
            reference,
            "completing {:?}: flush, cut, then the byte in the same call",
            text(completing)
        );
    }
}

// ── AC-4 (TM-2, NFR3, NFR5): adversarial input ───────────────────────────

/// AC-4: a very long run of CSI parameters, in one call and split, with and
/// without a cut, finishes within the budget and never panics; the replayed
/// ring equals the raw stream's.
#[test]
fn round4_fr4_long_csi_parameter_runs_finish_within_the_budget() {
    for unit in [&b"1;"[..], b"1", b":", b"\r", b"\x1b[1"] {
        let mut input = b"\x1b[".to_vec();
        input.extend(std::iter::repeat_n(unit, 100_000).flatten());
        let start = std::time::Instant::now();
        let whole = emitted_through(&input, &[input.len()], &[]);
        for split in [1usize, 2, 3, input.len() / 2, input.len() - 1] {
            let (a, b) = input.split_at(split);
            let mut filter = ScrollbackWriteFilter::new();
            let mut got = filter.feed(a, DIMS).1;
            got.extend_from_slice(&filter.feed_with_cuts(b, DIMS, &[b.len()]).bytes);
            assert_eq!(got, whole, "unit {unit:?} split at {split}");
        }
        assert!(
            start.elapsed() < BUDGET,
            "unit {unit:?} took {:?}",
            start.elapsed()
        );
    }
}

/// AC-4: many CSIs straddling calls, each followed by a cut, finish within the
/// budget and never panic; the replayed ring equals the raw stream's.
#[test]
fn round4_fr4_many_csis_straddling_calls_with_cuts_finish_within_the_budget() {
    let rounds = 20_000usize;
    let mut filter = ScrollbackWriteFilter::new();
    let mut ring = Vec::new();
    let mut raw = Vec::new();
    let pair = b"\x1b[?1049h\x1b[?1049l";
    let start = std::time::Instant::now();
    for _ in 0..rounds {
        ring.extend_from_slice(&filter.feed(b"\x1b[1", DIMS).1);
        ring.extend_from_slice(&filter.feed_with_cuts(b";2", DIMS, &[2]).bytes);
        raw.extend_from_slice(b"\x1b[1;2");
        raw.extend_from_slice(pair);
    }
    ring.extend_from_slice(&filter.feed(b"n", DIMS).1);
    raw.extend_from_slice(b"n");
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_eq!(view_of(&ring), view_of(&raw));
}

/// AC-4: the same through the production reader: many reads, each an open CSI
/// and a removed switch. The ring replays like the raw stream.
#[test]
fn round4_fr4_many_open_csis_through_the_reader_finish_within_the_budget() {
    let rounds = 400usize;
    let pair = b"\x1b[?1049h\x1b[?1049l".to_vec();
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    for _ in 0..rounds {
        chunks.push([&b"\x1b[6"[..], &pair].concat());
    }
    chunks.push(b"n".to_vec());
    let start = std::time::Instant::now();
    let ring = run_reader_without_owner(chunks.clone());
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
    assert_eq!(
        view_of(&ring),
        view_of(&chunks.concat()),
        "the ring replays like the raw stream"
    );
}

// ── AC-3 (FR4): closing rules, split invariance, the closing bytes ───────

/// `(fed, what the emitted stream is before any closing, whether it ends
/// inside a CSI)`, for a feed ending in a cut. The third field is checked
/// against term_core on the second.
const CLOSING_CASES: &[(&[u8], &[u8], bool)] = &[
    // Open in the call.
    (b"\x1b[", b"\x1b[", true),
    (b"\x1b[6", b"\x1b[6", true),
    (b"abc\x1b[12;3", b"abc\x1b[12;3", true),
    (b"\x1b[?25", b"\x1b[?25", true),
    (b"\x1b[6 ", b"\x1b[6 ", true),
    (b"\x1b[6\r", b"\x1b[6\r", true),
    (b"\x1b[6\x18", b"\x1b[6\x18", true),
    // An ESC aborted it and a new CSI is open.
    (b"\x1b[6\x1b[7", b"\x1b[6\x1b[7", true),
    // An ESC after the CSI starts a construct the cut drops: the bytes before
    // it still leave the ring inside the CSI.
    (b"\x1b[6\x1b", b"\x1b[6", true),
    (b"\x1b[6\x1b]0;t", b"\x1b[6", true),
    (b"\x1b[6\x1bPq#0", b"\x1b[6", true),
    (b"\x1b[6\x1b_Gi=1;P", b"\x1b[6", true),
    // Completed before the cut.
    (b"\x1b[6m", b"\x1b[6m", false),
    (b"abc\x1b[1;2H", b"abc\x1b[1;2H", false),
    // Cancelled before the cut.
    (b"\x1b[6\x7f", b"\x1b[6\x7f", false),
    (b"\x1b[!", b"\x1b[!", false),
    (b"\x1b[6?", b"\x1b[6?", false),
    (b"\x1b[6\x80", b"\x1b[6\x80", false),
    // Aborted by an ESC that completes a two-byte escape or starts a string
    // that completed.
    (b"\x1b[6\x1bX", b"\x1b[6\x1bX", false),
    (b"\x1b[6\x1b]0;t\x07", b"\x1b[6\x1b]0;t\x07", false),
    // No CSI at all.
    (b"abc", b"abc", false),
    (b"\x1b]0;t", b"", false),
];

/// AC-3 (FR4): the closing is written after the emitted bytes exactly when
/// the emitted stream ends inside a CSI at the cut, and once; the state is
/// clear afterwards. The CSI claim of each case is checked against term_core.
#[test]
fn round4_fr4_the_closing_is_written_only_when_the_emitted_stream_ends_inside_a_csi() {
    for (fed, kept, inside) in CLOSING_CASES {
        if !kept.is_empty() {
            assert_eq!(
                client_is_inside_a_csi(kept),
                *inside,
                "term_core on {:?}: the table's CSI claim",
                text(kept)
            );
        }
        let expected: Vec<u8> = if *inside {
            [*kept, CSI_CLOSING].concat()
        } else {
            kept.to_vec()
        };

        // The cut at the end of the fed bytes.
        let mut filter = ScrollbackWriteFilter::new();
        let outcome = filter.feed_with_cuts(fed, DIMS, &[fed.len()]);
        assert_eq!(
            outcome.bytes,
            expected,
            "fed {:?}, cut at the end",
            text(fed)
        );
        assert!(filter.pending().is_empty(), "fed {:?}", text(fed));
        assert!(!filter.awaiting_designator(), "fed {:?}", text(fed));
        assert_eq!(
            filter.csi_phase(),
            None,
            "fed {:?}: the cut clears it",
            text(fed)
        );

        // Several cuts at the same place close once.
        let mut filter = ScrollbackWriteFilter::new();
        let outcome = filter.feed_with_cuts(fed, DIMS, &[fed.len(), fed.len(), fed.len()]);
        assert_eq!(outcome.bytes, expected, "fed {:?}, three cuts", text(fed));

        // Fed one byte at a time, the cut in the last call.
        let mut filter = ScrollbackWriteFilter::new();
        let mut got = Vec::new();
        let (init, last) = fed.split_at(fed.len() - 1);
        for byte in init {
            got.extend_from_slice(&filter.feed(&[*byte], DIMS).1);
        }
        got.extend_from_slice(&filter.feed_with_cuts(last, DIMS, &[1]).bytes);
        assert_eq!(got, expected, "fed {:?} byte by byte", text(fed));

        // The CSI carried in from an earlier call, the cut at fed 0 of the next
        // (an empty fed range as the reader's fallback closing).
        let mut filter = ScrollbackWriteFilter::new();
        let mut got = filter.feed(fed, DIMS).1;
        got.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
        assert_eq!(
            got,
            expected,
            "fed {:?}, then the fallback closing",
            text(fed)
        );
        assert_eq!(filter.csi_phase(), None);
        // A second closing finds nothing to close.
        let again = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
        assert!(again.is_empty(), "fed {:?}: one closing per cut", text(fed));
    }
}

/// AC-3 (FR4): two cuts, each after a CSI, close each once; cuts with
/// nothing open between them write nothing.
#[test]
fn round4_fr4_each_cut_closes_at_most_once() {
    let mut filter = ScrollbackWriteFilter::new();
    let fed = b"\x1b[6\x1b[7";
    let out = filter.feed_with_cuts(fed, DIMS, &[3, 6]).bytes;
    assert_eq!(
        out,
        [&b"\x1b[6"[..], CSI_CLOSING, b"\x1b[7", CSI_CLOSING].concat()
    );

    // A CSI completed between the cuts is not closed again.
    let mut filter = ScrollbackWriteFilter::new();
    let fed = b"\x1b[6m\x1b[7";
    let out = filter.feed_with_cuts(fed, DIMS, &[4, 4, 7]).bytes;
    assert_eq!(
        out,
        [&b"\x1b[6m"[..], b"\x1b[7", CSI_CLOSING].concat(),
        "cut after the completed CSI writes nothing; the cut after the open one closes"
    );

    // No cut: no closing, the CSI state is carried.
    let mut filter = ScrollbackWriteFilter::new();
    let out = filter.feed_with_cuts(b"\x1b[6", DIMS, &[]).bytes;
    assert_eq!(out, b"\x1b[6");
    assert_eq!(filter.csi_phase(), Some(CsiPhase::Param));
}

/// AC-3 (FR4): never together with the FR3 designator ESC. A CSI that an
/// `ESC (` / `ESC )` aborted is gone before the wait starts, and the wait is
/// never a CSI.
#[test]
fn round4_fr4_the_closing_never_coincides_with_the_designator_wait() {
    for head in [&b"\x1b[6"[..], b"\x1b["] {
        for brace in [&b"("[..], b")"] {
            let fed = [head, b"\x1b", brace].concat();
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
            assert!(
                !out.windows(CSI_CLOSING.len()).any(|w| w == CSI_CLOSING),
                "{:?}: an aborted CSI is not closed",
                text(&fed)
            );

            // Without a cut: the wait, and no CSI.
            let mut filter = ScrollbackWriteFilter::new();
            filter.feed(&fed, DIMS);
            assert!(filter.awaiting_designator());
            assert_eq!(filter.csi_phase(), None);

            // The wait carried in with an empty `pending`, then a cut at fed 0.
            let mut filter = ScrollbackWriteFilter::new();
            filter.feed(&fed, DIMS);
            let out = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert!(
                !out.windows(CSI_CLOSING.len()).any(|w| w == CSI_CLOSING),
                "{:?}: the wait is not a CSI",
                text(&fed)
            );
        }
    }
}

/// AC-3 (FR4): for every split position and byte-at-a-time feeding, the
/// emitted bytes and the carried state equal the single-call result.
#[test]
fn round4_fr4_filter_output_and_state_are_split_invariant_across_a_csi() {
    // Corpus without a complete device query (which the ring write strips
    // only when it arrives within one call).
    let mut corpus: Vec<Vec<u8>> = CUT_PREFIXES.iter().map(|p| p.to_vec()).collect();
    corpus.extend([
        b"\x1b[6\x1b]0;t\x07\x1b[7".to_vec(),
        b"ab\x1b[6;7\rHcd\x1b[1m\x1b[?25".to_vec(),
        b"\x1b[6\x1b(B\x1b[7 ".to_vec(),
        b"\x1b[6\x1b\x1b[7".to_vec(),
        b"\x1b[6\x1b]0;t\x1b\\\x1b[7".to_vec(),
        b"\x1b[\x1b[\x1b[".to_vec(),
    ]);
    #[derive(Debug, PartialEq)]
    struct State {
        emitted: Vec<u8>,
        pending: Vec<u8>,
        awaiting: bool,
        csi: Option<CsiPhase>,
    }
    // `cut_at_end`: the last call carries a cut at its end.
    let run = |pieces: &[&[u8]], cut_at_end: bool| {
        let mut filter = ScrollbackWriteFilter::new();
        let mut emitted = Vec::new();
        for (idx, piece) in pieces.iter().enumerate() {
            let cuts: Vec<usize> = if cut_at_end && idx + 1 == pieces.len() {
                vec![piece.len()]
            } else {
                Vec::new()
            };
            emitted.extend_from_slice(&filter.feed_with_cuts(piece, DIMS, &cuts).bytes);
        }
        State {
            emitted,
            pending: filter.pending().to_vec(),
            awaiting: filter.awaiting_designator(),
            csi: filter.csi_phase(),
        }
    };
    for input in &corpus {
        for cut_at_end in [false, true] {
            let whole = run(&[input], cut_at_end);
            for split in 0..=input.len() {
                let got = run(&[&input[..split], &input[split..]], cut_at_end);
                assert_eq!(
                    got,
                    whole,
                    "{:?} split at {split}, cut at end {cut_at_end}",
                    text(input)
                );
            }
            let bytes: Vec<&[u8]> = input.chunks(1).collect();
            assert_eq!(
                run(&bytes, cut_at_end),
                whole,
                "{:?} byte by byte, cut at end {cut_at_end}",
                text(input)
            );
        }
    }
}

/// AC-3 (FR4): the carried state after a feed is the CSI sub-state of
/// term_core's parser: right after `ESC [` the entry state, after a
/// parameter, separator, marker or space the parameter state, none once the
/// CSI is completed or cancelled.
#[test]
fn round4_fr4_the_carried_state_names_the_csi_sub_state() {
    let cases: &[(&[u8], Option<CsiPhase>)] = &[
        (b"", None),
        (b"abc", None),
        (b"\x1b", None),
        (b"\x1b[", Some(CsiPhase::Entry)),
        (b"\x1b[\r", Some(CsiPhase::Entry)),
        (b"\x1b[6", Some(CsiPhase::Param)),
        (b"\x1b[?", Some(CsiPhase::Param)),
        (b"\x1b[ ", Some(CsiPhase::Param)),
        (b"\x1b[;", Some(CsiPhase::Param)),
        (b"\x1b[6$", Some(CsiPhase::Param)),
        (b"\x1b[6m", None),
        (b"\x1b[6\x7f", None),
        (b"\x1b[!", None),
        (b"\x1b[6?", None),
        (b"\x1b[6\x1b", None),
        (b"\x1b[6\x1b[", Some(CsiPhase::Entry)),
        (b"\x1b[6\x1bX", None),
        (b"\x1b[6\x1b(", None),
    ];
    for (input, expected) in cases {
        let mut filter = ScrollbackWriteFilter::new();
        filter.feed(input, DIMS);
        // A lone trailing ESC is held; the state stays what it was before it.
        let held = input.ends_with(b"\x1b");
        let expected = if held && input.len() > 1 {
            Some(CsiPhase::Param)
        } else {
            *expected
        };
        assert_eq!(filter.csi_phase(), expected, "after {:?}", text(input));
    }
}

/// AC-3 (FR4): every byte in each CSI sub-state is classified as term_core
/// does: it leaves the parser inside the CSI, or in ground. Compared through
/// the filter's closing at a cut.
#[test]
fn round4_fr4_every_byte_in_each_csi_state_is_classified_like_term_core() {
    // Heads that put term_core in the entry state and in the parameter state;
    // `ESC[7` and the like are not device queries, so the ring write keeps
    // them.
    let heads: &[&[u8]] = &[
        b"\x1b[", b"\x1b[7", b"\x1b[?", b"\x1b[ ", b"\x1b[;", b"\x1b[:", b"\x1b[7$",
    ];
    for head in heads {
        for byte in (0u8..=255).filter(|b| *b != 0x1b) {
            let stream = [*head, &[byte][..]].concat();
            let inside = client_is_inside_a_csi(&stream);
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&stream, DIMS, &[stream.len()]).bytes;
            let kept = strip_pty_output_for_scrollback_write(&stream);
            let expected = if inside {
                [&kept[..], CSI_CLOSING].concat()
            } else {
                kept
            };
            assert_eq!(
                out,
                expected,
                "head {:?} then byte {byte:#04x}: term_core is {} the CSI",
                text(head),
                if inside { "inside" } else { "outside" }
            );
        }
    }
}

/// IN-CSI streams, for the closing-bytes checks: one per sub-state.
const OPEN_CSI_STREAMS: &[&[u8]] = &[
    b"\x1b[",
    b"\x1b[6",
    b"\x1b[?25",
    b"\x1b[6 ",
    b"\x1b[12;34\r",
    b"abc\x1b[1;",
];

/// AC-3 (D3, FR4): the closing bytes, fed to term_core after an open CSI,
/// leave it in ground with no response and no displayed change; in ground
/// they change nothing either.
#[test]
fn round4_fr4_the_closing_bytes_cancel_the_csi_without_effect_in_term_core() {
    for stream in OPEN_CSI_STREAMS {
        assert!(client_is_inside_a_csi(stream), "{:?}", text(stream));
        let closed = [*stream, CSI_CLOSING].concat();
        assert_eq!(
            view_of(&closed),
            view_of(stream),
            "{:?}: no displayed change, cursor move or response",
            text(stream)
        );
        assert!(
            !client_is_inside_a_csi(&closed),
            "{:?}: the parser is back in ground",
            text(stream)
        );
        // The next bytes are plain text, not the rest of the CSI.
        for next in [&b"n"[..], b"c", b"m", b"A", b"[6n"] {
            let view = view_of(&[&closed[..], next].concat());
            assert!(
                view.responses.is_empty(),
                "{:?} closed, then {:?}: a response",
                text(stream),
                text(next)
            );
            assert!(
                view.rows.iter().any(|row| row.ends_with(&text(next))),
                "{:?} closed, then {:?}: displayed as text",
                text(stream),
                text(next)
            );
        }
    }
    for ground in [&b""[..], b"abc", b"\x1b[6m", b"\x1b[1;2H"] {
        assert_eq!(
            view_of(&[ground, CSI_CLOSING].concat()),
            view_of(ground),
            "{:?}: the closing is ignored in ground",
            text(ground)
        );
    }
}

/// AC-3 (D3, FR4): neither strip treats the closing bytes as the start of a
/// strip target, and a strip target after them is still removed.
#[test]
fn round4_fr4_neither_strip_reads_the_closing_bytes_as_a_strip_target() {
    let launch = b"\x1b]777;emterm;markdown;begin;id=x\x07";
    let query = b"\x1b[6n";
    for strip in [
        strip_pty_output_for_scrollback_write as fn(&[u8]) -> Vec<u8>,
        strip_replayable_rich_content,
    ] {
        assert_eq!(strip(CSI_CLOSING), CSI_CLOSING.to_vec());
        for open in OPEN_CSI_STREAMS {
            let ring = [*open, CSI_CLOSING, b"n"].concat();
            assert_eq!(
                strip(&ring),
                ring,
                "{:?} closed, then n: the pair is not a query",
                text(open)
            );
            // The closing keeps the open CSI from completing into a query.
            let ring = [*open, CSI_CLOSING, query].concat();
            assert_eq!(
                strip(&ring),
                [*open, CSI_CLOSING].concat(),
                "{:?} closed, then a query: only the query goes",
                text(open)
            );
            // A launch after the closing is removed, the closing stays.
            let ring = [*open, CSI_CLOSING, launch].concat();
            assert_eq!(
                strip(&ring),
                [*open, CSI_CLOSING].concat(),
                "{:?} closed, then a launch",
                text(open)
            );
        }
    }
}

/// AC-4 (TM-2, NFR3): CSI bytes are never held and the state carried is
/// constant: a long CSI fed one byte at a time stays linear and `pending`
/// stays empty throughout.
#[test]
fn round4_fr4_a_long_csi_fed_byte_by_byte_is_never_held() {
    let start = std::time::Instant::now();
    let mut filter = ScrollbackWriteFilter::new();
    let mut emitted = Vec::new();
    emitted.extend_from_slice(&filter.feed(b"\x1b[", DIMS).1);
    for idx in 0..300_000usize {
        let byte = [b'0' + (idx % 10) as u8];
        emitted.extend_from_slice(&filter.feed(&byte, DIMS).1);
        assert_eq!(filter.pending_len(), 0, "no CSI byte is held (byte {idx})");
    }
    assert_eq!(filter.csi_phase(), Some(CsiPhase::Param));
    assert_eq!(
        emitted.len(),
        300_002,
        "every byte is written as it arrives"
    );
    let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
    assert_eq!(closing, CSI_CLOSING);
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}

// ── AC-5 (FR4, FR6): the decision record ─────────────────────────────────

/// The text of the section that starts at the line `heading` and runs to the
/// next heading of the same or a higher level.
fn section_of<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let start = doc
        .lines()
        .scan(0usize, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(_, line)| *line == heading)
        .map(|(at, _)| at)
        .unwrap_or_else(|| panic!("DECISIONS.md has no heading {heading:?}"));
    let body = &doc[start..];
    let rest_from = heading.len();
    let end = body[rest_from..]
        .lines()
        .scan(rest_from, |offset, line| {
            let at = *offset;
            *offset += line.len() + 1;
            Some((at, line))
        })
        .find(|(_, line)| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            hashes > 0 && hashes <= level && line[hashes..].starts_with(' ')
        })
        .map(|(at, _)| at)
        .unwrap_or(body.len());
    &body[..end]
}

/// AC-5 (FR4, FR6, SPEC AC-6): the decision record's FR4 section states
/// whether the path reproduced, with the registry test path, and its
/// behavior-changing-tests subsection lists the existing test whose
/// expectation this task changed. Every name the record cites resolves to a
/// test in the source.
#[test]
fn round4_fr4_the_decision_record_states_the_outcome_and_lists_the_changed_test() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../feature-docs/mux-suppressed-output-round4-fixes/DECISIONS.md");
    let doc = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the decision record {} is unreadable: {e}", path.display()));

    let adjacent = section_of(&doc, "### FR4: in-progress CSI at a cut");
    let fixed = adjacent.contains("reproduced and fixed");
    let not_reproducing = adjacent.contains("not reproducing");
    assert!(
        fixed != not_reproducing,
        "the FR4 section states exactly one outcome: {adjacent}"
    );
    let registry_file = "src-tauri/src/mux/ipc/pty_spawn/tests/round4_cut_csi.rs";
    let registry_test = "mux::ipc::pty_spawn::tests::round4_cut_csi::\
                         round4_fr4_in_progress_csi_at_a_cut_matches_the_raw_stream_reference";
    assert!(adjacent.contains(registry_file), "{adjacent}");
    assert!(adjacent.contains(registry_test), "{adjacent}");
    // The cited path resolves: this module, and a test function of that name.
    let (module, function) = registry_test.rsplit_once("::").unwrap();
    assert!(module_path!().ends_with(module), "{}", module_path!());
    assert!(
        include_str!("round4_cut_csi.rs").contains(&format!("fn {function}()")),
        "{function} is defined in {registry_file}"
    );
    // This task's fix is in the write filter, so the production path was
    // reproduced; a record that says otherwise contradicts the tests above.
    assert!(
        fixed,
        "the pre-change test failed (task0004 record), so the outcome is `reproduced and fixed`"
    );

    let behavior_changing = {
        let start = doc
            .find("## Behavior-changing tests")
            .expect("the Behavior-changing tests section");
        section_of(&doc[start..], "### FR4")
    };
    assert!(
        behavior_changing.contains("`fallback_path_keeps_the_whole_chunk_gate`"),
        "the FR4 subsection lists the test whose expectation changed: {behavior_changing}"
    );
    assert!(
        behavior_changing.contains("src-tauri/src/mux/ipc/pty_spawn/tests.rs"),
        "{behavior_changing}"
    );
    assert!(
        include_str!("../tests.rs").contains("fn fallback_path_keeps_the_whole_chunk_gate()"),
        "the listed test exists under that name"
    );
}
