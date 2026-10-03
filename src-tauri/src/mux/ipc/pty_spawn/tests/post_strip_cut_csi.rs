//! mux-cut-csi-post-strip-closure task0001 (FR1-FR4, FR6): the CSI state the
//! write filter writes its closing at a cut by, and carries into the next call,
//! is the CSI state of the bytes it actually wrote after the strip.
//!
//! A construct the strip removes together with its opening `ESC` (an OSC 777
//! viewer launch, OSC 9999 emterm-md, an agent-status report, a Kitty APC, a
//! SIXEL DCS, an answered CSI device query) would have aborted an open CSI on
//! the live client. The ring never holds that `ESC`, so its bytes still end
//! inside the CSI and a later byte completes it into a query on replay
//! (`ESC[6` ... `n` answers a cursor-position query the raw stream never made).
//!
//! Oracle convention (IMPLEMENTATION.md, test design): the reference is
//! term_core fed the raw stream. The comparisons keep the removed construct's
//! own effect out (EC-5): responses are compared for what the bytes after the
//! cut provoke, and the Kitty payload neither answers nor places an image.
//! The reader-level case uses the visibility-restore harness of
//! `round3_write_path`.
//!
//! Test identifiers (IMPLEMENTATION.md, Regression test identifiers): R3 the
//! carried state, R4 the overflow flush, R5 the production reader, R6 the
//! closing rules for the edge cases, R7 the budget. R1 and R2 live in
//! `round4_cut_csi`, R9 in `scrollback_filter::tests`.

use super::round3_write_path::{
    DIMS, client_view, new_core, reference_view, run_visibility_restore_at, switch_pairs,
};
use super::round4_cut_csi::{
    POST_STRIP_FORMS, client_is_inside_a_csi, emitted_through, osc_held_at_the_cap, text,
    view_after_a_cut,
};
use super::*;

const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

const ESC_BYTE: &[u8] = &[0x1b];

/// OSC 777 emterm markdown launch, BEL terminated.
const LAUNCH: &[u8] = b"\x1b]777;emterm;markdown;begin;id=x\x07";
/// OSC 9999 emterm-md, ST terminated.
const MD_LAUNCH: &[u8] = b"\x1b]9999;emterm-md;begin\x1b\\";
/// OSC 777 agent-status report, BEL terminated.
const AGENT_STATUS: &[u8] = b"\x1b]777;emterm;agent-status;v=1;state=idle\x07";
/// Kitty graphics APC with a payload that neither answers nor places an image.
const KITTY: &[u8] = b"\x1b_Gi=1,a=d;AAAA\x1b\\";
/// SIXEL DCS.
const SIXEL: &[u8] = b"\x1bPq#0;2;0;0;0\x1b\\";
/// An answered CSI device query (cursor-position report request).
const QUERY: &[u8] = b"\x1b[6n";

/// The strip targets the write filter HOLDS across calls until they are
/// complete. A CSI device query is not among them: CSI bytes are never held,
/// so a query split across calls is written to the ring as it arrives and only
/// the snapshot strip removes it (IMPLEMENTATION.md D2, SPEC FR6 item 3).
const HELD_TARGETS: &[(&str, &[u8])] = &[
    ("osc 777 launch", LAUNCH),
    ("osc 9999 emterm-md", MD_LAUNCH),
    ("agent-status", AGENT_STATUS),
    ("kitty apc", KITTY),
    ("sixel dcs", SIXEL),
];

/// Every strip target a single call removes whole, the query included.
fn all_targets() -> Vec<(&'static str, &'static [u8])> {
    let mut all = HELD_TARGETS.to_vec();
    all.push(("csi query", QUERY));
    all
}

/// The CSI sub-state a client is in after `stream`, read from term_core:
/// `None` in ground, otherwise the entry or the parameter state. The entry
/// state cancels on an intermediate `!`; the parameter state accepts it. Only
/// meaningful for a stream that ends in a CSI or in ground (not after a bare
/// `ESC`, not inside a charset designator wait).
fn client_csi_phase(stream: &[u8]) -> Option<CsiPhase> {
    if !client_is_inside_a_csi(stream) {
        return None;
    }
    let probe = [stream, b"!"].concat();
    Some(if client_is_inside_a_csi(&probe) {
        CsiPhase::Param
    } else {
        CsiPhase::Entry
    })
}

/// Everything a filter has emitted and still carries after being fed.
#[derive(Debug, PartialEq)]
struct State {
    emitted: Vec<u8>,
    pending: Vec<u8>,
    awaiting: bool,
    csi: Option<CsiPhase>,
}

/// Feed `pieces` as consecutive calls (`cut_at_end`: the last call carries a
/// cut at its end). With `check_oracle`, after every call the carried CSI state
/// is compared with term_core's on the bytes emitted so far.
fn run_pieces(pieces: &[&[u8]], cut_at_end: bool, check_oracle: bool) -> State {
    let mut filter = ScrollbackWriteFilter::new();
    let mut emitted = Vec::new();
    for (idx, piece) in pieces.iter().enumerate() {
        let last = idx + 1 == pieces.len();
        let cuts: Vec<usize> = if cut_at_end && last {
            vec![piece.len()]
        } else {
            Vec::new()
        };
        emitted.extend_from_slice(&filter.feed_with_cuts(piece, DIMS, &cuts).bytes);
        if check_oracle && !filter.awaiting_designator() {
            // After a cut the state is cleared and the emitted stream carries
            // the closing, so the oracle applies to the cut-free calls.
            if !(cut_at_end && last) {
                assert_eq!(
                    filter.csi_phase(),
                    client_csi_phase(&emitted),
                    "after call {idx} of {pieces:?}: the carried state is term_core's on the \
                     emitted bytes {:?}",
                    text(&emitted)
                );
            }
        }
    }
    State {
        emitted,
        pending: filter.pending().to_vec(),
        awaiting: filter.awaiting_designator(),
        csi: filter.csi_phase(),
    }
}

// ── R3 (AC-4): the carried CSI state ─────────────────────────────────────

/// Open CSIs and the sub-state they leave.
const OPEN_HEADS: &[(&[u8], CsiPhase)] = &[
    (b"\x1b[", CsiPhase::Entry),
    (b"\x1b[6", CsiPhase::Param),
    (b"\x1b[?25", CsiPhase::Param),
    (b"\x1b[6 ", CsiPhase::Param),
    (b"abc\x1b[12;3", CsiPhase::Param),
];

/// AC-4 (FR2, TM-1, R3): after a cut-free call that ends in an open CSI
/// followed by a complete strip target, the filter's CSI state is the state of
/// the CSI the written bytes end in, and a following fallback closing writes
/// one DEL. With the CSI in one call, the strip target held and completed in a
/// later call, the state after the later call is still that state (EC-1: the
/// target strips to nothing, so the CSI carried in is not aborted). Over a
/// corpus mixing open CSIs, held strip targets and text, every split position
/// and byte-by-byte feeding give the same written bytes, pending bytes,
/// awaiting-designator state and CSI state as one call, and after every call
/// the state equals term_core's on the bytes written so far.
#[test]
fn post_strip_the_carried_csi_state_follows_the_stripped_output() {
    // (a) One cut-free call: the open CSI, then a complete strip target.
    for (head, phase) in OPEN_HEADS.iter().copied() {
        for (name, target) in all_targets() {
            let label = format!("{:?} then {name}", text(head));
            let fed = [head, target].concat();
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed(&fed, DIMS).1;
            assert_eq!(out, head, "{label}: the target is stripped");
            assert!(filter.pending().is_empty(), "{label}");
            assert!(!filter.awaiting_designator(), "{label}");
            assert_eq!(
                filter.csi_phase(),
                Some(phase),
                "{label}: the written bytes end inside the CSI"
            );
            assert_eq!(client_csi_phase(&out), Some(phase), "{label}: term_core");
            let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert_eq!(closing, CSI_CLOSING, "{label}: the fallback closing");
            assert_eq!(filter.csi_phase(), None, "{label}");
            assert!(
                filter.feed_with_cuts(b"", DIMS, &[0]).bytes.is_empty(),
                "{label}: one closing"
            );
        }
    }

    // (b) EC-1: the CSI in one call, the strip target held in a later call and
    // completed in another. The held target strips to nothing, so the CSI the
    // filter carried in stays open.
    for (head, phase) in OPEN_HEADS.iter().copied() {
        for (name, target) in HELD_TARGETS.iter().copied() {
            for split in 1..target.len() {
                let label = format!("{:?} then {name} split at {split}", text(head));
                let mut filter = ScrollbackWriteFilter::new();
                let mut emitted = filter.feed(head, DIMS).1;
                assert_eq!(filter.csi_phase(), Some(phase), "{label}: after the CSI");

                emitted.extend_from_slice(&filter.feed(&target[..split], DIMS).1);
                assert_eq!(filter.pending(), &target[..split], "{label}: held");
                assert_eq!(
                    filter.csi_phase(),
                    Some(phase),
                    "{label}: the state before the held target"
                );

                emitted.extend_from_slice(&filter.feed(&target[split..], DIMS).1);
                assert_eq!(emitted, head, "{label}: the target strips to nothing");
                assert!(filter.pending().is_empty(), "{label}");
                assert_eq!(
                    filter.csi_phase(),
                    Some(phase),
                    "{label}: EC-1, the carried-in CSI is still open"
                );
                let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
                assert_eq!(closing, CSI_CLOSING, "{label}: the fallback closing");
            }
        }
    }

    // (c) Split invariance over a corpus mixing open CSIs, held strip targets
    // and text. CSI-query strip targets stay out (D2).
    let mut corpus: Vec<Vec<u8>> = Vec::new();
    for (_, target) in HELD_TARGETS.iter().copied() {
        corpus.push([&b"\x1b[6"[..], target, b"abc"].concat());
        corpus.push([&b"\x1b[6"[..], target, b"\x1b[7"].concat());
        corpus.push([&b"\x1b["[..], target].concat());
        corpus.push([&b"ab\x1b[6;7\rHcd\x1b[1m\x1b[?25"[..], target].concat());
        corpus.push([&b"\x1b[6"[..], target, b"\x1b]0;t\x07"].concat());
        corpus.push([&b"\x1b[6"[..], target, b"\x1b[?25", target, b"\x1b[7"].concat());
        corpus.push([&b"\x1b[6"[..], target, target].concat());
        corpus.push([&b"x"[..], target, b"\x1b[6", target].concat());
    }
    corpus.push(
        [
            &b"\x1b[6"[..],
            LAUNCH,
            KITTY,
            SIXEL,
            MD_LAUNCH,
            AGENT_STATUS,
        ]
        .concat(),
    );
    corpus.push(b"abc\x1b[6m\x1b[".to_vec());
    for input in &corpus {
        for cut_at_end in [false, true] {
            let whole = run_pieces(&[input], cut_at_end, true);
            if !cut_at_end {
                assert_eq!(
                    whole.csi,
                    client_csi_phase(&whole.emitted),
                    "{:?}: the state after one call is term_core's on the written bytes",
                    text(input)
                );
            }
            for split in 0..=input.len() {
                let got = run_pieces(&[&input[..split], &input[split..]], cut_at_end, true);
                assert_eq!(
                    got,
                    whole,
                    "{:?} split at {split}, cut at end {cut_at_end}",
                    text(input)
                );
            }
            let bytes: Vec<&[u8]> = input.chunks(1).collect();
            assert_eq!(
                run_pieces(&bytes, cut_at_end, true),
                whole,
                "{:?} byte by byte, cut at end {cut_at_end}",
                text(input)
            );
        }
    }
}

// ── R4 (AC-5): the overflow flush ────────────────────────────────────────

/// AC-5 (FR3, TM-1, R4): the held OSC grows to the 512 KiB cap, and the next
/// call completes it past the cap and ends in `ESC[6` with a complete strip
/// target. The overflow flush strips the run, so the written bytes end inside
/// the CSI the target's `ESC` would have aborted:
/// - a cut at the end of that call writes the stripped run and one DEL;
/// - a cut followed by `n` in the same call writes the stripped run, one DEL
///   and the `n`;
/// - without a cut the flush carries the state, and a later fallback closing
///   writes the DEL.
/// term_core replaying the ring and a later `n` gives no cursor-position
/// report and equals the raw stream (the target's own effect kept out).
#[test]
fn post_strip_an_overflow_flush_followed_by_a_cut_closes_an_open_csi() {
    let (enter, leave) = (&b"\x1b[?1049h"[..], &b"\x1b[?1049l"[..]);
    let pair = [enter, leave].concat();
    let held = osc_held_at_the_cap();
    for (form, target) in POST_STRIP_FORMS.iter().copied() {
        // The continuation of the held OSC: more body bytes, its BEL, text and
        // an open CSI followed by the strip target; it takes the run past the
        // cap.
        let tail: Vec<u8> = [&b"pppppppppp"[..], b"\x07", b"abc", b"\x1b[6", target].concat();
        let stripped = strip_pty_output_for_scrollback_write(&[&held[..], &tail[..]].concat());
        assert!(
            stripped.ends_with(b"abc\x1b[6"),
            "{form}: the run is stripped"
        );
        let expected = [&stripped[..], CSI_CLOSING].concat();
        let before_raw = [&held[..], &tail[..], &pair[..]].concat();
        let reference = view_after_a_cut(&before_raw, b"n");
        assert!(
            reference.responses.is_empty(),
            "{form}: `n` is text in the raw stream"
        );

        // A cut at the end of the flushing call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty());
        let outcome = filter.feed_with_cuts(&tail, DIMS, &[tail.len()]);
        assert!(
            outcome.carried.is_none(),
            "{form}: the flushed call reports no completion"
        );
        assert!(
            outcome.bytes == expected,
            "{form}: cut at the end of the flushing call"
        );
        assert!(filter.pending().is_empty(), "{form}");
        assert_eq!(filter.csi_phase(), None, "{form}: the cut clears the state");
        let later = filter.feed(b"n", DIMS).1;
        assert_eq!(
            view_after_a_cut(&outcome.bytes, &later),
            reference,
            "{form}: the ring replays like the raw stream"
        );

        // A cut followed by `n` in the same call.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty());
        let fed = [&tail[..], b"n"].concat();
        let outcome = filter.feed_with_cuts(&fed, DIMS, &[tail.len()]);
        assert!(
            outcome.bytes == [&expected[..], b"n"].concat(),
            "{form}: cut, then `n` in the same call"
        );
        assert_eq!(
            view_after_a_cut(&outcome.bytes[..expected.len()], b"n"),
            reference,
            "{form}"
        );

        // No cut: the flush carries the state; the fallback closing writes
        // the DEL.
        let mut filter = ScrollbackWriteFilter::new();
        assert!(filter.feed(&held, DIMS).1.is_empty());
        let outcome = filter.feed_with_cuts(&tail, DIMS, &[]);
        assert!(
            outcome.bytes == stripped,
            "{form}: the flush emits the stripped run"
        );
        assert_eq!(
            filter.csi_phase(),
            Some(CsiPhase::Param),
            "{form}: the flush carries the post-strip state"
        );
        assert!(!filter.awaiting_designator(), "{form}");
        let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
        assert_eq!(closing, CSI_CLOSING, "{form}: the fallback closing");
    }
}

// ── R5 (AC-6): the production reader ─────────────────────────────────────

/// Responses, screen and cursor of the client equal the raw-stream
/// reference's, except for the answer to the removed construct itself (EC-5).
/// The reference answers it from the raw stream (`own_answer`, the start of its
/// responses); the ring never replays it. The client answers it only when the
/// production reader re-delivers the query live: a restore that covers the read
/// holding the query suppresses that read, and the replacement payload carries
/// the query to the live client (`own_answer_on_client` is then `own_answer`,
/// otherwise empty). The responses are otherwise equal, so no second answer is
/// produced by the later `n`.
fn assert_client_equals_reference_except(
    received: &[PtyOutputChunk],
    chunks: &[Vec<u8>],
    own_answer: &[u8],
    own_answer_on_client: &[u8],
    ctx: &str,
) {
    let (client, client_responses) = client_view(received);
    let (reference, reference_responses) = reference_view(chunks);
    let provoked_later = reference_responses
        .strip_prefix(own_answer)
        .unwrap_or_else(|| {
            panic!("{ctx}: the reference's responses {reference_responses:?} lack {own_answer:?}")
        });
    assert_eq!(
        client_responses,
        [own_answer_on_client, provoked_later].concat(),
        "{ctx}: response bytes differ from the raw-stream reference"
    );
    for r in 0..R2_ROWS {
        assert_eq!(
            client.get_line_text(r).trim_end(),
            reference.get_line_text(r).trim_end(),
            "{ctx}: row {r} differs from the reference"
        );
    }
    assert_eq!(
        client.get_cursor_row(),
        reference.get_cursor_row(),
        "{ctx}: cursor row"
    );
    assert_eq!(
        client.get_cursor_col(),
        reference.get_cursor_col(),
        "{ctx}: cursor col"
    );
}

/// What term_core answers the bytes before the cut (the removed construct's
/// own answer): empty for every form but the CSI query.
fn own_answer_of(prefix: &[u8]) -> Vec<u8> {
    let mut core = new_core();
    core.process_pty_data_fully(prefix);
    core.take_response()
}

/// AC-6 (FR1, FR2, NFR3, TM-1, R5): through the production reader and the
/// production visibility restore of a main-screen pane whose ring has not
/// wrapped, with `ESC[6`, each form and a removed 47 / 1047 / 1049 `h` / `l`
/// pair in one read and `n` in a later read:
/// - the restore covering the switch read;
/// - the restore between the switch read and the `n`;
/// - the CSI split across reads (its last byte arrives with the construct),
///   with the restore covering the switch read and between.
/// The client's responses, screen and cursor equal the raw-stream reference's
/// (for the CSI-query form, apart from the reference's own answer to the
/// stripped query, which the client gives only when the restore covers the read
/// holding the query and the reader re-delivers it live): the live parser's CSI
/// was aborted by the construct's ESC, so the later `n` is plain text there.
#[test]
fn post_strip_reader_restore_matches_the_raw_stream_reference() {
    for (form, construct) in POST_STRIP_FORMS.iter().copied() {
        for (enter, leave) in switch_pairs() {
            let pair = [enter, leave].concat();
            let label = format!("{form}, {}", text(enter));
            let head = &b"\x1b[6"[..];
            let prefix = [head, construct].concat();
            let own = own_answer_of(&prefix);
            assert_eq!(
                own.is_empty(),
                form != "csi query",
                "{label}: only the query form has an answer of its own"
            );
            let together = [&prefix[..], &pair[..]].concat();

            // One read, the snapshot covering the switch read.
            let chunks = vec![together.clone(), b"n".to_vec()];
            let run = run_visibility_restore_at(&chunks, 0);
            assert_client_equals_reference_except(
                &run.received,
                &chunks,
                &own,
                &own,
                &format!("(a) {label}"),
            );

            // The snapshot between the switch read and the `n`.
            let chunks = vec![together.clone(), b"\r".to_vec(), b"n".to_vec()];
            let run = run_visibility_restore_at(&chunks, 1);
            assert_client_equals_reference_except(
                &run.received,
                &chunks,
                &own,
                &[],
                &format!("(b) {label}"),
            );

            // The CSI split across reads: its last byte arrives with the
            // construct and the switch.
            let (first, rest) = head.split_at(head.len() - 1);
            let second = [rest, construct, &pair[..]].concat();
            let chunks = vec![first.to_vec(), second.clone(), b"n".to_vec()];
            let run = run_visibility_restore_at(&chunks, 1);
            assert_client_equals_reference_except(
                &run.received,
                &chunks,
                &own,
                &own,
                &format!("(c) {label}"),
            );

            // The split with the snapshot between.
            let chunks = vec![first.to_vec(), second, b"\r".to_vec(), b"n".to_vec()];
            let run = run_visibility_restore_at(&chunks, 2);
            assert_client_equals_reference_except(
                &run.received,
                &chunks,
                &own,
                &[],
                &format!("(d) {label}"),
            );
        }
    }
}

// ── R6 (AC-3): the closing rules for the edge cases ──────────────────────

/// Run `fed` with a cut at its end, with three cuts there, byte by byte with
/// the cut in the last call and with the fallback closing; every way must give
/// `expected`. CSI-device-query strip targets are written across calls when
/// fed byte by byte, so callers pass `byte_by_byte` for the rows where that
/// differs (None: the same bytes).
#[track_caller]
fn assert_closing_on_every_path(
    fed: &[u8],
    expected: &[u8],
    byte_by_byte: Option<&[u8]>,
    ctx: &str,
) {
    let mut filter = ScrollbackWriteFilter::new();
    assert_eq!(
        filter.feed_with_cuts(fed, DIMS, &[fed.len()]).bytes,
        expected,
        "{ctx}: cut at the end"
    );
    assert!(filter.pending().is_empty(), "{ctx}");
    assert!(!filter.awaiting_designator(), "{ctx}");
    assert_eq!(filter.csi_phase(), None, "{ctx}: the cut clears the state");

    let mut filter = ScrollbackWriteFilter::new();
    assert_eq!(
        filter
            .feed_with_cuts(fed, DIMS, &[fed.len(), fed.len(), fed.len()])
            .bytes,
        expected,
        "{ctx}: three cuts"
    );

    let mut filter = ScrollbackWriteFilter::new();
    let mut got = Vec::new();
    let (init, last) = fed.split_at(fed.len() - 1);
    for byte in init {
        got.extend_from_slice(&filter.feed(&[*byte], DIMS).1);
    }
    got.extend_from_slice(&filter.feed_with_cuts(last, DIMS, &[1]).bytes);
    assert_eq!(got, byte_by_byte.unwrap_or(expected), "{ctx}: byte by byte");

    let mut filter = ScrollbackWriteFilter::new();
    let mut got = filter.feed(fed, DIMS).1;
    got.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
    assert_eq!(got, expected, "{ctx}: fallback closing");
}

/// AC-3 (FR1, TM-1, R6): the closing follows the written bytes in the edge
/// cases of the plan.
/// - EC-2: `ESC[6` with a complete launch, then `ESC]0;t`, then a cut writes
///   `ESC[6` and one DEL (the OSC is dropped by the cut, the launch is
///   stripped).
/// - EC-3: `ESC[6` with a stripped CSI device query that embeds a C0 byte,
///   then a cut, writes `ESC[6`, that C0 byte and one DEL (the C0 byte
///   executes inside the open CSI and leaves it open).
/// - EC-4: `ESC[6` with `ESC]0;t` and BEL, and `ESC[6` with `ESC X`, each
///   followed by a cut, write no DEL (the written bytes end in ground).
/// - EC-6: no cut writes both the designator `ESC` and a DEL.
#[test]
fn post_strip_the_closing_follows_the_stripped_output() {
    // EC-2.
    let fed = [&b"\x1b[6"[..], LAUNCH, b"\x1b]0;t"].concat();
    let expected = [&b"\x1b[6"[..], CSI_CLOSING].concat();
    assert_closing_on_every_path(&fed, &expected, None, "EC-2");

    // EC-3: every C0 byte but ESC embeds in the query body and is re-emitted.
    for c0 in [0x00u8, 0x07, 0x08, 0x0a, 0x0d, 0x18, 0x1a, 0x1f] {
        for body in [&[b'6', c0, b'n'][..], &[c0, b'6', b'n'][..]] {
            let query = [&b"\x1b["[..], body].concat();
            let fed = [&b"\x1b[6"[..], &query].concat();
            let expected = [&b"\x1b[6"[..], &[c0], CSI_CLOSING].concat();
            let label = format!("EC-3, C0 {c0:#04x}, query {:?}", text(&query));
            let mut filter = ScrollbackWriteFilter::new();
            let out = filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes;
            assert_eq!(out, expected, "{label}: the C0 byte stays, the DEL follows");
            // Fed byte by byte the query is written across calls (D2): the
            // ring holds the fed bytes and ends in ground.
            let mut filter = ScrollbackWriteFilter::new();
            let mut got = Vec::new();
            let (init, last) = fed.split_at(fed.len() - 1);
            for byte in init {
                got.extend_from_slice(&filter.feed(&[*byte], DIMS).1);
            }
            got.extend_from_slice(&filter.feed_with_cuts(last, DIMS, &[1]).bytes);
            assert_eq!(got, fed, "{label}: byte by byte");
            // Replay: the C0 byte executes, the DEL cancels the CSI, `n` is text.
            let (before, after) = (&out[..], &b"n"[..]);
            let raw_before = [&fed[..], b"\x1b[?1049h\x1b[?1049l"].concat();
            assert_eq!(
                view_after_a_cut(before, after),
                view_after_a_cut(&raw_before, after),
                "{label}: the ring replays like the raw stream"
            );
            // The fallback closing after a cut-free call.
            let mut filter = ScrollbackWriteFilter::new();
            let mut got = filter.feed(&fed, DIMS).1;
            assert_eq!(filter.csi_phase(), Some(CsiPhase::Param), "{label}");
            got.extend_from_slice(&filter.feed_with_cuts(b"", DIMS, &[0]).bytes);
            assert_eq!(got, expected, "{label}: fallback closing");
        }
    }

    // EC-4: the written bytes end in ground, so nothing is closed.
    for fed in [
        [&b"\x1b[6"[..], b"\x1b]0;t\x07"].concat(),
        [&b"\x1b[6"[..], b"\x1bX"].concat(),
        [&b"\x1b[6"[..], b"\x1b]0;t\x1b\\"].concat(),
    ] {
        assert!(!client_is_inside_a_csi(&fed), "{:?}", text(&fed));
        assert_closing_on_every_path(&fed, &fed, None, &format!("EC-4 {:?}", text(&fed)));
    }

    // EC-6: the designator wait and the CSI never close together.
    for brace in [&b"("[..], b")"] {
        for target in [LAUNCH, QUERY, KITTY] {
            // In one call: a stripped target between the CSI and the designation.
            let designation = [&b"\x1b"[..], brace].concat();
            let fed = [&b"\x1b[6"[..], target, &designation].concat();
            let expected = [&b"\x1b[6"[..], &designation, ESC_BYTE].concat();
            let ctx = format!("EC-6, {:?}", text(&fed));
            let mut filter = ScrollbackWriteFilter::new();
            assert_eq!(
                filter.feed_with_cuts(&fed, DIMS, &[fed.len()]).bytes,
                expected,
                "{ctx}: the designator ESC only"
            );
            assert_eq!(filter.csi_phase(), None, "{ctx}");

            // The CSI and the target in an earlier call (the state is open),
            // the designation and the cut in the next.
            let mut filter = ScrollbackWriteFilter::new();
            let mut got = filter.feed(&[&b"\x1b[6"[..], target].concat(), DIMS).1;
            assert_eq!(filter.csi_phase(), Some(CsiPhase::Param), "{ctx}");
            got.extend_from_slice(&filter.feed_with_cuts(&designation, DIMS, &[2]).bytes);
            assert_eq!(got, expected, "{ctx}: designation and cut in a later call");
            assert_eq!(filter.csi_phase(), None, "{ctx}");
            assert!(!filter.awaiting_designator(), "{ctx}");

            // The designation without a cut: the wait stands, the CSI does not.
            let mut filter = ScrollbackWriteFilter::new();
            filter.feed(&[&b"\x1b[6"[..], target].concat(), DIMS);
            filter.feed(&designation, DIMS);
            assert!(filter.awaiting_designator(), "{ctx}");
            assert_eq!(
                filter.csi_phase(),
                None,
                "{ctx}: never together with the wait"
            );
            let closing = filter.feed_with_cuts(b"", DIMS, &[0]).bytes;
            assert_eq!(
                closing, ESC_BYTE,
                "{ctx}: the fallback closing writes the ESC only"
            );
        }
    }
}

// ── R7 (AC-8): adversarial input ─────────────────────────────────────────

/// `reps` copies of an open `ESC[6` followed by `target`.
fn alternating(target: &[u8], reps: usize) -> Vec<u8> {
    let unit = [&b"\x1b[6"[..], target].concat();
    std::iter::repeat_n(unit, reps).flatten().collect()
}

/// AC-8 (NFR2, NFR4, NFR5, TM-2, R7): long inputs alternating open CSIs and
/// strip targets, fed in one call, in two calls and byte by byte, with and
/// without cuts, finish within the budget and never panic, and every way of
/// feeding writes the same bytes. Inputs below the cap take the normal path;
/// inputs above it take the overflow flush (split at a unit boundary, so the
/// flushed halves end between targets).
#[test]
fn post_strip_alternating_strip_targets_and_open_csis_finish_within_the_budget() {
    let start = std::time::Instant::now();
    let strip_closing =
        |bytes: &[u8]| -> Vec<u8> { bytes.strip_suffix(CSI_CLOSING).unwrap_or(bytes).to_vec() };

    for (name, target) in HELD_TARGETS.iter().copied() {
        let unit_len = 3 + target.len();
        // Normal path: below the cap.
        let reps = 8_000;
        let input = alternating(target, reps);
        assert!(input.len() < SCROLLBACK_FILTER_PENDING_CAP, "{name}");
        let expected = b"\x1b[6".repeat(reps);
        for cut_at_end in [false, true] {
            let whole = run_pieces(&[&input], cut_at_end, false);
            assert_eq!(strip_closing(&whole.emitted), expected, "{name}: one call");
            assert!(whole.pending.is_empty(), "{name}");
            for at in [
                1,
                3,
                unit_len,
                unit_len + 3,
                input.len() / 2,
                input.len() - unit_len,
                input.len() - 1,
            ] {
                let (a, b) = input.split_at(at);
                assert_eq!(
                    run_pieces(&[a, b], cut_at_end, false),
                    whole,
                    "{name}: split at {at}, cut at end {cut_at_end}"
                );
            }
        }
        // Byte by byte, on a shorter input of the same shape.
        let short = alternating(target, 1_500);
        for cut_at_end in [false, true] {
            let whole = run_pieces(&[&short], cut_at_end, false);
            let bytes: Vec<&[u8]> = short.chunks(1).collect();
            assert_eq!(
                run_pieces(&bytes, cut_at_end, false),
                whole,
                "{name}: byte by byte, cut at end {cut_at_end}"
            );
        }

        // Overflow path: above the cap, in one call and in two calls split at
        // a unit boundary.
        let reps = 40_000usize.max(SCROLLBACK_FILTER_PENDING_CAP / unit_len + 10);
        let input = alternating(target, reps);
        assert!(input.len() > SCROLLBACK_FILTER_PENDING_CAP, "{name}");
        let expected = b"\x1b[6".repeat(reps);
        for cut_at_end in [false, true] {
            let whole = run_pieces(&[&input], cut_at_end, false);
            assert_eq!(
                strip_closing(&whole.emitted),
                expected,
                "{name}: overflow, one call"
            );
            let at = unit_len * (reps / 2);
            let (a, b) = input.split_at(at);
            assert_eq!(
                run_pieces(&[a, b], cut_at_end, false),
                whole,
                "{name}: overflow, two calls, cut at end {cut_at_end}"
            );
        }
    }

    // A CSI device query is removed when it arrives whole in a call.
    let reps = 8_000;
    let input = alternating(QUERY, reps);
    let out = emitted_through(&input, &[input.len()], &[]);
    assert_eq!(
        out.strip_suffix(CSI_CLOSING).unwrap_or(&out),
        b"\x1b[6".repeat(reps).as_slice(),
        "query: one call"
    );
    assert!(start.elapsed() < BUDGET, "took {:?}", start.elapsed());
}
